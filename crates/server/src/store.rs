//! Publication of the pipeline's packs as static files: everything is computed once per
//! domain when a version is published, nothing per request or per phone.
//!
//! ```text
//! <store>/public/catalog.json                    every site (short cache, ETag)
//! <store>/public/sites/<site>/index.json         latest version, base and delta bundles
//! <store>/public/bundles/<sha256>.skb            bundles, immutable (name = SHA-256)
//! <store>/blobs/<aa>/<sha256>                    contents of the kept versions (internal)
//! <store>/sites/<site>/versions/<version>.json   kept versions (internal)
//! <store>/sites/<site>/base.json                 base version and its bundle (internal)
//! <store>/sites/<site>/index.prev.json           previous index: its bundles are kept one
//!                                                more publication (downloads in flight)
//! <store>/.lock                                  held while publishing
//! ```
//!
//! Only `public/` is served (and could be copied as is to an object store). A bundle is
//! written, then the index that names it (by rename): a phone never sees an index whose
//! bundles are missing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use skimap_pack::Pack;
use skimap_pack::manifest::{MANIFEST_FILE, Manifest, SiteInfo};
use skimap_pack::sync::{BundleRef, INDEX_FORMAT, INDEX_FORMAT_VERSION, LatestVersion, SiteIndex};
use skimap_pack::update::{Update, parse_manifest, sha256_hex, write_bundle};

/// File of the pack that changes at every build (its build time) without changing any
/// layer: a pack that differs from the latest only by it is not published again.
pub const VOLATILE: &[&str] = &["pack.meta.json"];

/// One published pack version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    /// Site identifier.
    pub site: String,
    /// `Manifest::pack_version`.
    pub pack_version: String,
    /// Publication time, RFC 3339.
    pub published_at: String,
    /// SHA-256 of the exact `manifest.json` bytes (a blob).
    pub manifest_sha256: String,
    /// Size of `manifest.json`.
    pub manifest_bytes: u64,
    /// `Manifest::total_bytes`.
    pub total_bytes: u64,
    /// Number of files (without the manifest).
    pub files: u64,
}

/// Base version of a site and its bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BaseEntry {
    version: String,
    bundle: BundleRef,
}

/// What to keep and when to rebuild the base.
#[derive(Debug, Clone, Copy)]
pub struct Retention {
    /// Versions kept besides the base: the deltas start from them.
    pub recent_versions: usize,
    /// The base is rebuilt when the delta from it exceeds this share of the pack.
    pub rebase_fraction: f64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            recent_versions: 8,
            rebase_fraction: 0.10,
        }
    }
}

/// Result of [`Store::publish`].
#[derive(Debug, Clone, Serialize)]
pub struct PublishReport {
    /// Site.
    pub site: String,
    /// Version of the pack given.
    pub pack_version: String,
    /// `published`, or `unchanged` (same files as the latest, apart from [`VOLATILE`]).
    pub status: String,
    /// Latest version after the call.
    pub latest: String,
    /// Base version after the call.
    pub base: String,
    /// Whether this publication rebuilt the base bundle.
    pub rebased: bool,
    /// Blobs copied into the store.
    pub new_blobs: u64,
    /// Bundles written: `(from, bytes)`, `from` empty for the base bundle.
    pub bundles: Vec<(String, u64)>,
    /// The delta from the previous latest version (what a daily sync downloads).
    pub daily_delta_bytes: Option<u64>,
    /// Layers changed or added since the previous latest version.
    pub changed: Vec<String>,
    /// Versions removed by the retention.
    pub versions_removed: Vec<String>,
    /// Blobs and bundles no longer referenced, removed.
    pub files_removed: u64,
    /// Their bytes.
    pub bytes_removed: u64,
    /// Seconds spent.
    pub seconds: f64,
}

/// The catalog served at `/v1/catalog.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    /// `skimap-catalog`.
    pub format: String,
    /// 1.
    pub format_version: u32,
    /// Sites with a published pack.
    pub sites: Vec<CatalogSite>,
}

/// One site of the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogSite {
    /// The site.
    pub site: SiteInfo,
    /// Index URL, relative to the server.
    pub index_url: String,
    /// Latest version.
    pub latest: LatestVersion,
    /// Size of a first download (base bundle + delta from the base).
    pub first_download_bytes: u64,
}

/// Site identifiers of the pipeline (`skimap_common::site::slug`); up to 128 bytes: SRN
/// areas keep the identifier of their first itinerary
/// (`srn-traversee-datatourisme-6-80785b47-…`, 66 bytes, plus a section suffix).
fn valid_site(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_version(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// Whether `s` is a SHA-256 in lowercase hex.
pub fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).with_context(|| format!("{}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("{}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(b) => Ok(Some(
            serde_json::from_slice(&b).with_context(|| format!("{}", path.display()))?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// A cached manifest: parsed and exact bytes.
pub type CachedManifest = Arc<(Manifest, Vec<u8>)>;

/// The store of published packs.
pub struct Store {
    root: PathBuf,
    manifests: Mutex<HashMap<String, CachedManifest>>,
}

impl Store {
    /// Opens (and creates) a store.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for d in ["blobs", "sites", "public/bundles", "public/sites"] {
            fs::create_dir_all(root.join(d))
                .with_context(|| format!("creating {}", root.join(d).display()))?;
        }
        Ok(Self {
            root,
            manifests: Mutex::new(HashMap::new()),
        })
    }

    /// Root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The served directory.
    pub fn public(&self) -> PathBuf {
        self.root.join("public")
    }

    /// Served path of the catalog.
    pub fn catalog_path(&self) -> PathBuf {
        self.public().join("catalog.json")
    }

    /// Served path of a site index, `None` for a malformed name.
    pub fn index_path(&self, site: &str) -> Option<PathBuf> {
        valid_site(site).then(|| self.public().join("sites").join(site).join("index.json"))
    }

    /// Served path of a bundle, `None` for a malformed name.
    pub fn bundle_path(&self, sha: &str) -> Option<PathBuf> {
        valid_sha(sha).then(|| self.public().join("bundles").join(format!("{sha}.skb")))
    }

    fn blob_path(&self, sha: &str) -> Option<PathBuf> {
        valid_sha(sha).then(|| self.root.join("blobs").join(&sha[..2]).join(sha))
    }

    fn site_dir(&self, site: &str) -> PathBuf {
        self.root.join("sites").join(site)
    }

    /// Sites with a published pack, sorted.
    pub fn sites(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for e in fs::read_dir(self.public().join("sites"))? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            if valid_site(&name) && e.path().join("index.json").is_file() {
                out.push(name);
            }
        }
        out.sort();
        Ok(out)
    }

    /// The index of a site.
    pub fn index(&self, site: &str) -> Result<Option<SiteIndex>> {
        match self.index_path(site) {
            Some(p) => read_json(&p),
            None => Ok(None),
        }
    }

    /// Kept versions of a site, oldest first.
    pub fn versions(&self, site: &str) -> Result<Vec<VersionEntry>> {
        let dir = self.site_dir(site).join("versions");
        let mut out = Vec::new();
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for e in rd {
            let p = e?.path();
            if p.extension().is_some_and(|x| x == "json") {
                out.push(serde_json::from_slice::<VersionEntry>(&fs::read(&p)?)?);
            }
        }
        out.sort_by(|a, b| a.pack_version.cmp(&b.pack_version));
        Ok(out)
    }

    fn manifest(&self, entry: &VersionEntry) -> Result<CachedManifest> {
        if let Some(m) = lock(&self.manifests).get(&entry.manifest_sha256) {
            return Ok(Arc::clone(m));
        }
        let path = self
            .blob_path(&entry.manifest_sha256)
            .ok_or_else(|| anyhow!("bad manifest SHA-256 in the entry"))?;
        let bytes = fs::read(&path).with_context(|| format!("{}", path.display()))?;
        let m = parse_manifest(&bytes, &path.display().to_string())?;
        let cached = Arc::new((m, bytes));
        let mut cache = lock(&self.manifests);
        if cache.len() >= 64 {
            cache.clear();
        }
        cache.insert(entry.manifest_sha256.clone(), Arc::clone(&cached));
        Ok(cached)
    }

    /// Copies `src` into the blob store under `sha` (checked while copying). Returns
    /// whether it was new.
    fn ingest(&self, src: &Path, sha: &str, bytes: u64) -> Result<bool> {
        let dst = self
            .blob_path(sha)
            .ok_or_else(|| anyhow!("bad SHA-256 {sha:?}"))?;
        if fs::metadata(&dst).is_ok_and(|m| m.len() == bytes) {
            return Ok(false);
        }
        let parent = dst.parent().expect("blob has a parent");
        fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(".{sha}.tmp"));
        let mut input = fs::File::open(src).with_context(|| format!("{}", src.display()))?;
        let mut out = fs::File::create(&tmp)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut n = 0u64;
        loop {
            let k = input.read(&mut buf)?;
            if k == 0 {
                break;
            }
            h.update(&buf[..k]);
            out.write_all(&buf[..k])?;
            n += k as u64;
        }
        out.sync_all()?;
        drop(out);
        let got = hex::encode(h.finalize());
        if got != sha || n != bytes {
            let _ = fs::remove_file(&tmp);
            bail!(
                "{}: {n} bytes, SHA-256 {got}; manifest says {bytes} bytes, {sha} (changed while publishing?)",
                src.display()
            );
        }
        fs::rename(&tmp, &dst)?;
        Ok(true)
    }

    /// Writes the bundle of `update` (blobs from the store), named by its SHA-256.
    fn write_bundle(&self, update: &Update) -> Result<BundleRef> {
        let dir = self.public().join("bundles");
        let tmp = dir.join(format!(".{}.tmp", skimap_server_tmp_name()));
        let result = (|| -> Result<(String, u64)> {
            let f = fs::File::create(&tmp)?;
            let mut w = HashingWriter {
                inner: std::io::BufWriter::new(f),
                hash: Sha256::new(),
            };
            let n = write_bundle(&mut w, update, |sha| {
                let p = self
                    .blob_path(sha)
                    .ok_or_else(|| skimap_pack::Error::Pack(format!("bad blob {sha}")))?;
                fs::File::open(&p).map_err(|e| skimap_pack::Error::io(&p, e))
            })?;
            let f = w.inner.into_inner().map_err(|e| e.into_error())?;
            f.sync_all()?;
            Ok((hex::encode(w.hash.finalize()), n))
        })();
        let (sha, bytes) = match result {
            Ok(r) => r,
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
        };
        let dst = self.bundle_path(&sha).expect("hex SHA-256");
        fs::rename(&tmp, &dst)?;
        Ok(BundleRef {
            from: update.from_version.clone(),
            to: update.to_version.clone(),
            url: format!("/v1/bundles/{sha}.skb"),
            sha256: sha,
            bytes,
            blobs: update.fetch.iter().map(|b| b.paths.len() as u64).sum(),
        })
    }

    /// Publishes the pack in `pack_dir` (a `data/out/<site>/pack/` of the pipeline):
    /// checks it (`Pack::verify`), copies its new contents, records the version, rebuilds
    /// the base when the pack drifted from it, writes the delta bundles and the index,
    /// applies the retention, removes what nothing references any more.
    pub fn publish(
        &self,
        pack_dir: &Path,
        now: DateTime<Utc>,
        retention: Retention,
    ) -> Result<PublishReport> {
        self.publish_with(pack_dir, now, retention, true)
    }

    /// Publishes many packs (every domain of France): each as [`Store::publish`], but the
    /// catalog and the removal of unreferenced contents run once at the end instead of
    /// after each pack (both read every site of the store). A pack that fails does not stop
    /// the others; the reports come in the order of `packs`.
    pub fn publish_many(
        &self,
        packs: &[PathBuf],
        now: DateTime<Utc>,
        retention: Retention,
    ) -> Result<Vec<Result<PublishReport>>> {
        let reports: Vec<Result<PublishReport>> = packs
            .iter()
            .map(|p| self.publish_with(p, now, retention, false))
            .collect();
        if reports
            .iter()
            .any(|r| r.as_ref().is_ok_and(|r| r.status == "published"))
        {
            let lock_file = fs::File::create(self.root.join(".lock"))?;
            lock_file.lock()?;
            self.write_catalog()?;
            self.collect_garbage()?;
        }
        Ok(reports)
    }

    fn publish_with(
        &self,
        pack_dir: &Path,
        now: DateTime<Utc>,
        retention: Retention,
        finish: bool,
    ) -> Result<PublishReport> {
        let t0 = std::time::Instant::now();
        let pack = Pack::open(pack_dir)?;
        let problems = pack.verify()?;
        if !problems.is_empty() {
            bail!(
                "pack {} does not verify ({} problems): {}",
                pack_dir.display(),
                problems.len(),
                problems
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ; ")
            );
        }
        let manifest_path = pack_dir.join(MANIFEST_FILE);
        let manifest_bytes = fs::read(&manifest_path)?;
        let new = parse_manifest(&manifest_bytes, &manifest_path.display().to_string())?;
        let site = new.site.id.clone();
        if !valid_site(&site) || !valid_version(&new.pack_version) {
            bail!("bad site {site:?} or version {:?}", new.pack_version);
        }

        let lock_file = fs::File::create(self.root.join(".lock"))?;
        lock_file.lock()?;

        let versions = self.versions(&site)?;
        let previous = versions.last().cloned();
        let previous_manifest = previous.as_ref().map(|p| self.manifest(p)).transpose()?;
        let base: Option<BaseEntry> = read_json(&self.site_dir(&site).join("base.json"))?;
        let mut report = PublishReport {
            site: site.clone(),
            pack_version: new.pack_version.clone(),
            status: "unchanged".into(),
            latest: previous
                .as_ref()
                .map_or_else(String::new, |p| p.pack_version.clone()),
            base: base
                .as_ref()
                .map_or_else(String::new, |b| b.version.clone()),
            rebased: false,
            new_blobs: 0,
            bundles: Vec::new(),
            daily_delta_bytes: None,
            changed: Vec::new(),
            versions_removed: Vec::new(),
            files_removed: 0,
            bytes_removed: 0,
            seconds: 0.0,
        };
        if let (Some(p), Some(pm)) = (&previous, &previous_manifest) {
            if new.pack_version < p.pack_version {
                bail!(
                    "pack {} is older than the published {}",
                    new.pack_version,
                    p.pack_version
                );
            }
            let content = |m: &Manifest| -> HashMap<String, String> {
                m.files
                    .iter()
                    .filter(|f| !VOLATILE.contains(&f.path.as_str()))
                    .map(|f| (f.path.clone(), f.sha256.clone()))
                    .collect()
            };
            if content(&pm.0) == content(&new) || new.pack_version == p.pack_version {
                report.seconds = t0.elapsed().as_secs_f64();
                return Ok(report);
            }
        }

        // Contents, then the version.
        for f in &new.files {
            if self.ingest(&pack_dir.join(&f.path), &f.sha256, f.bytes)? {
                report.new_blobs += 1;
            }
        }
        let manifest_sha = sha256_hex(&manifest_bytes);
        if self.ingest(&manifest_path, &manifest_sha, manifest_bytes.len() as u64)? {
            report.new_blobs += 1;
        }
        let entry = VersionEntry {
            site: site.clone(),
            pack_version: new.pack_version.clone(),
            published_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            manifest_sha256: manifest_sha.clone(),
            manifest_bytes: manifest_bytes.len() as u64,
            total_bytes: new.total_bytes,
            files: new.files.len() as u64,
        };
        write_atomic(
            &self
                .site_dir(&site)
                .join("versions")
                .join(format!("{}.json", entry.pack_version)),
            &serde_json::to_vec_pretty(&entry)?,
        )?;

        // Base: kept while the delta from it stays small.
        let base_entry = base
            .as_ref()
            .and_then(|b| versions.iter().find(|v| v.pack_version == b.version));
        let keep_base = match (base_entry, &base) {
            (Some(be), Some(b))
                if self
                    .bundle_path(&b.bundle.sha256)
                    .is_some_and(|p| p.is_file()) =>
            {
                let bm = self.manifest(be)?;
                let d = Update::between(Some(&bm.0), &new, &manifest_bytes);
                (d.fetch_bytes as f64) <= retention.rebase_fraction * new.total_bytes as f64
            }
            _ => false,
        };
        let base = if keep_base {
            base.expect("kept base")
        } else {
            let u = Update::between(None, &new, &manifest_bytes);
            let bundle = self.write_bundle(&u)?;
            report.bundles.push((String::new(), bundle.bytes));
            report.rebased = true;
            let b = BaseEntry {
                version: new.pack_version.clone(),
                bundle,
            };
            write_atomic(
                &self.site_dir(&site).join("base.json"),
                &serde_json::to_vec_pretty(&b)?,
            )?;
            b
        };

        // Retention: the base and the last versions.
        let all = self.versions(&site)?;
        let recent: HashSet<&str> = all
            .iter()
            .rev()
            .take(retention.recent_versions + 1)
            .map(|v| v.pack_version.as_str())
            .collect();
        let mut kept = Vec::new();
        for v in &all {
            if recent.contains(v.pack_version.as_str()) || v.pack_version == base.version {
                kept.push(v.clone());
            } else {
                fs::remove_file(
                    self.site_dir(&site)
                        .join("versions")
                        .join(format!("{}.json", v.pack_version)),
                )?;
                report.versions_removed.push(v.pack_version.clone());
            }
        }

        // Deltas from every kept version to the new latest.
        let mut deltas = Vec::new();
        for v in kept.iter().filter(|v| v.pack_version != new.pack_version) {
            let old = self.manifest(v)?;
            let u = Update::between(Some(&old.0), &new, &manifest_bytes);
            let b = self.write_bundle(&u)?;
            report.bundles.push((v.pack_version.clone(), b.bytes));
            if previous
                .as_ref()
                .is_some_and(|p| p.pack_version == v.pack_version)
            {
                report.daily_delta_bytes = Some(b.bytes);
                report.changed = u
                    .changes
                    .iter()
                    .filter(|c| {
                        matches!(
                            c.change,
                            skimap_pack::update::Change::Added
                                | skimap_pack::update::Change::Changed
                        )
                    })
                    .map(|c| c.id.clone())
                    .collect();
            }
            deltas.push(b);
        }

        let changes = Update::between(
            previous_manifest.as_ref().map(|p| &p.0),
            &new,
            &manifest_bytes,
        )
        .changes;
        let index = SiteIndex {
            format: INDEX_FORMAT.into(),
            format_version: INDEX_FORMAT_VERSION,
            site: new.site.clone(),
            latest: LatestVersion {
                version: new.pack_version.clone(),
                published_at: entry.published_at.clone(),
                manifest_sha256: manifest_sha,
                total_bytes: new.total_bytes,
                files: new.files.len() as u64,
                valid_for: new.valid_for.clone(),
                personal_sources: new.personal_sources,
            },
            base_version: base.version.clone(),
            base: base.bundle.clone(),
            deltas,
            changes,
        };
        let index_path = self.index_path(&site).expect("checked site");
        if index_path.is_file() {
            fs::copy(&index_path, self.site_dir(&site).join("index.prev.json"))?;
        }
        write_atomic(&index_path, &serde_json::to_vec_pretty(&index)?)?;
        if finish {
            self.write_catalog()?;
            let (n, bytes) = self.collect_garbage()?;
            report.files_removed = n;
            report.bytes_removed = bytes;
        }
        report.status = "published".into();
        report.latest = new.pack_version.clone();
        report.base = base.version;
        report.seconds = t0.elapsed().as_secs_f64();
        Ok(report)
    }

    fn write_catalog(&self) -> Result<()> {
        let mut sites = Vec::new();
        for id in self.sites()? {
            let Some(i) = self.index(&id)? else { continue };
            let from_base = i
                .deltas
                .iter()
                .find(|d| d.from.as_deref() == Some(i.base_version.as_str()))
                .map_or(0, |d| d.bytes);
            sites.push(CatalogSite {
                site: i.site.clone(),
                index_url: format!("/v1/sites/{id}/index.json"),
                first_download_bytes: i.base.bytes + from_base,
                latest: i.latest,
            });
        }
        let c = Catalog {
            format: "skimap-catalog".into(),
            format_version: 1,
            sites,
        };
        write_atomic(&self.catalog_path(), &serde_json::to_vec_pretty(&c)?)
    }

    /// Removes the blobs no kept version references and the bundles neither the current
    /// nor the previous index of a site names.
    fn collect_garbage(&self) -> Result<(u64, u64)> {
        let mut blobs: HashSet<String> = HashSet::new();
        let mut bundles: HashSet<String> = HashSet::new();
        for site in self.sites()? {
            for v in self.versions(&site)? {
                let m = self.manifest(&v)?;
                blobs.insert(v.manifest_sha256.clone());
                blobs.extend(m.0.files.iter().map(|f| f.sha256.clone()));
            }
            let prev: Option<SiteIndex> = read_json(&self.site_dir(&site).join("index.prev.json"))?;
            for i in [self.index(&site)?, prev].into_iter().flatten() {
                bundles.insert(i.base.sha256.clone());
                bundles.extend(i.deltas.iter().map(|d| d.sha256.clone()));
            }
            if let Some(b) = read_json::<BaseEntry>(&self.site_dir(&site).join("base.json"))? {
                bundles.insert(b.bundle.sha256);
            }
        }
        let (mut n, mut bytes) = (0u64, 0u64);
        for d in fs::read_dir(self.root.join("blobs"))? {
            let d = d?.path();
            if !d.is_dir() {
                continue;
            }
            for f in fs::read_dir(&d)? {
                let f = f?;
                let name = f.file_name().to_string_lossy().into_owned();
                if valid_sha(&name) && !blobs.contains(&name) {
                    bytes += f.metadata()?.len();
                    fs::remove_file(f.path())?;
                    n += 1;
                }
            }
        }
        for f in fs::read_dir(self.public().join("bundles"))? {
            let f = f?;
            let name = f.file_name().to_string_lossy().into_owned();
            if let Some(sha) = name.strip_suffix(".skb")
                && valid_sha(sha)
                && !bundles.contains(sha)
            {
                bytes += f.metadata()?.len();
                fs::remove_file(f.path())?;
                n += 1;
            }
        }
        lock(&self.manifests).retain(|k, _| blobs.contains(k));
        Ok((n, bytes))
    }
}

/// A temporary file name unique in the process.
fn skimap_server_tmp_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

struct HashingWriter<W: Write> {
    inner: W,
    hash: Sha256,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_site("chapelle-des-bois"));
        assert!(valid_site(
            "srn-traversee-datatourisme-6-80785b47-1e6c-37be-99fc-d401c59b1fb7-1"
        ));
        assert!(!valid_site(&"a".repeat(129)));
        assert!(!valid_site("../x") && !valid_site("A") && !valid_site(""));
        assert!(valid_version("20261115T014538Z"));
        assert!(!valid_version("../x") && !valid_version("a/b"));
        assert!(valid_sha(&"a".repeat(64)));
        assert!(!valid_sha(&"A".repeat(64)) && !valid_sha("ab"));
    }
}
