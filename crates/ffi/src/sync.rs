//! Download and update of packs from the server (K2 #52, S0, E1 #125), the phone side.
//!
//! The app makes the HTTP requests (it owns the network permission and WorkManager); this
//! module decides what to download and installs it, with the code of `skimap-pack`
//! (`sync::plan`, `update::apply_bundle`, `sync::write_state`), so that the phone runs
//! exactly what the server's tests and `crates/server/examples/phone_sync.rs` run:
//!
//! 1. [`parse_server_catalog`] reads `/v1/catalog.json` (sites, sizes, voice packs).
//! 2. `GET <index_url>` with `If-None-Match` = [`SyncStateInfo::index_etag`]; 304: done.
//! 3. [`sync_plan`]: the bundles to fetch (none, one delta, or the base then a delta).
//! 4. Each bundle is downloaded (resumed with `Range` / `If-Range`), checked whole with
//!    [`verify_bundle`] (length and SHA-256: its name), then installed with
//!    [`install_bundle`], which checks every blob again before writing anything and swaps
//!    the new version in. [`SyncError::MissingContents`]: start again with
//!    [`sync_plan_full`].
//! 5. [`record_sync`] keeps the version, base and `ETag` next to the pack.
//!
//! A voice language pack (VO6 #129) is a pack of its own (manifest and clips), published
//! with the same index and bundles; it syncs exactly like a site pack, into its own
//! directory.

use std::path::Path;

use serde::Deserialize;
use skimap_pack::manifest::SiteInfo;
use skimap_pack::reader::sha256_file;
use skimap_pack::sync::{self, LatestVersion, SiteIndex, SyncState};
use skimap_pack::update::{self, MISSING_CONTENTS};

/// Errors of the sync functions (`SyncException` in Kotlin).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SyncError {
    /// The bundle was made for contents this phone no longer has: use
    /// [`sync_plan_full`].
    #[error("missing contents: {detail}")]
    MissingContents {
        /// Message of the pack crate.
        detail: String,
    },
    /// Downloaded file of the wrong length or SHA-256: download it again.
    #[error("corrupt download: {detail}")]
    Corrupt {
        /// What differs.
        detail: String,
    },
    /// Unreadable index or catalog, unsupported format, or a pack that does not install.
    #[error("{detail}")]
    Invalid {
        /// What is wrong.
        detail: String,
    },
}

fn invalid(e: impl std::fmt::Display) -> SyncError {
    SyncError::Invalid {
        detail: e.to_string(),
    }
}

/// A bundle to download.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BundleInfo {
    /// Version it starts from; `None`: from nothing (the base bundle).
    pub from: Option<String>,
    /// Version it leads to.
    pub to: String,
    /// SHA-256 of the file: its name and its `ETag`.
    pub sha256: String,
    /// Size of the file.
    pub bytes: u64,
    /// URL relative to the server (`/v1/bundles/<sha256>.skb`).
    pub url: String,
}

/// What a sync of one pack has to do.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SyncPlan {
    /// Site of the index.
    pub site_id: String,
    /// Its name.
    pub site_name: String,
    /// Version installed, if any.
    pub installed_version: Option<String>,
    /// Latest published version.
    pub latest_version: String,
    /// Base version of the index.
    pub base_version: String,
    /// Bundles to download and install, in order (empty: up to date).
    pub bundles: Vec<BundleInfo>,
    /// Their total size.
    pub download_bytes: u64,
    /// Size of the pack once installed.
    pub pack_bytes: u64,
    /// Layers, basemaps and voice changed since the previous version (`id`).
    pub changed: Vec<String>,
}

/// What a phone remembers of its last sync of a pack.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncStateInfo {
    /// Installed version.
    pub version: String,
    /// Base version of the index it came from.
    pub base_version: String,
    /// `ETag` of that index, for `If-None-Match`.
    pub index_etag: Option<String>,
}

/// Outcome of [`install_bundle`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InstallReport {
    /// Version now installed.
    pub version: String,
    /// Files written from the bundle.
    pub fetched_files: u64,
    /// Files taken from the version installed before (hard links).
    pub reused_files: u64,
    /// Files of the old version left out.
    pub removed_files: u64,
}

/// A voice language pack of a site (VO6), listed by the server catalog.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct VoicePackInfo {
    /// Language (`fr`, `en`).
    pub lang: String,
    /// Index URL, relative to the server.
    pub index_url: String,
    /// Latest version.
    pub latest_version: String,
    /// Size of a first download.
    pub first_download_bytes: u64,
}

/// A site published by the server (`/v1/catalog.json`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ServerSite {
    /// Site identifier (the pack directory name on the phone).
    pub site_id: String,
    /// Display name.
    pub name: String,
    /// West, south, east, north.
    pub bbox: Vec<f64>,
    /// Index URL, relative to the server.
    pub index_url: String,
    /// Latest version.
    pub latest_version: String,
    /// Publication time of the latest version (RFC 3339).
    pub published_at: String,
    /// Size of a first download (base bundle and delta from the base).
    pub first_download_bytes: u64,
    /// Size of the pack once installed.
    pub pack_bytes: u64,
    /// Voice language packs, when the server publishes them apart (VO6).
    pub voice: Vec<VoicePackInfo>,
}

#[derive(Deserialize)]
struct CatalogFile {
    format: String,
    format_version: u32,
    sites: Vec<CatalogSite>,
}

#[derive(Deserialize)]
struct CatalogSite {
    site: SiteInfo,
    index_url: String,
    latest: LatestVersion,
    first_download_bytes: u64,
    #[serde(default)]
    voice: Vec<CatalogVoice>,
}

#[derive(Deserialize)]
struct CatalogVoice {
    lang: String,
    index_url: String,
    latest: LatestVersion,
    first_download_bytes: u64,
}

/// Format of `/v1/catalog.json` read by this library.
pub const CATALOG_FORMAT: &str = "skimap-catalog";

/// Sites of the server catalog, sorted by name.
#[uniffi::export]
pub fn parse_server_catalog(json: String) -> Result<Vec<ServerSite>, SyncError> {
    let c: CatalogFile =
        serde_json::from_str(&json).map_err(|e| invalid(format!("catalog: {e}")))?;
    if c.format != CATALOG_FORMAT || c.format_version != 1 {
        return Err(invalid(format!(
            "catalog format {} version {} not supported",
            c.format, c.format_version
        )));
    }
    let mut sites: Vec<ServerSite> = c
        .sites
        .into_iter()
        .map(|s| ServerSite {
            site_id: s.site.id,
            name: s.site.name,
            bbox: s.site.bbox.to_vec(),
            index_url: s.index_url,
            latest_version: s.latest.version,
            published_at: s.latest.published_at,
            first_download_bytes: s.first_download_bytes,
            pack_bytes: s.latest.total_bytes,
            voice: s
                .voice
                .into_iter()
                .map(|v| VoicePackInfo {
                    lang: v.lang,
                    index_url: v.index_url,
                    latest_version: v.latest.version,
                    first_download_bytes: v.first_download_bytes,
                })
                .collect(),
        })
        .collect();
    sites.sort_by(|a, b| a.name.cmp(&b.name).then(a.site_id.cmp(&b.site_id)));
    Ok(sites)
}

fn parse_index(json: &str) -> Result<SiteIndex, SyncError> {
    let i: SiteIndex = serde_json::from_str(json).map_err(|e| invalid(format!("index: {e}")))?;
    if i.format != sync::INDEX_FORMAT
        || i.format_version == 0
        || i.format_version > sync::INDEX_FORMAT_VERSION
    {
        return Err(invalid(format!(
            "index format {} version {} not supported",
            i.format, i.format_version
        )));
    }
    Ok(i)
}

fn plan_of(
    index: &SiteIndex,
    installed: Option<String>,
    bundles: Vec<sync::BundleRef>,
) -> SyncPlan {
    let bundles: Vec<BundleInfo> = bundles
        .into_iter()
        .map(|b| BundleInfo {
            from: b.from,
            to: b.to,
            sha256: b.sha256,
            bytes: b.bytes,
            url: b.url,
        })
        .collect();
    SyncPlan {
        site_id: index.site.id.clone(),
        site_name: index.site.name.clone(),
        installed_version: installed,
        latest_version: index.latest.version.clone(),
        base_version: index.base_version.clone(),
        download_bytes: bundles.iter().map(|b| b.bytes).sum(),
        bundles,
        pack_bytes: index.latest.total_bytes,
        changed: index
            .changes
            .iter()
            .filter(|c| !matches!(c.change, update::Change::Unchanged))
            .map(|c| c.id.clone())
            .collect(),
    }
}

/// Installed version of the pack in `pack_dir`: its manifest, else nothing.
fn installed_version(pack_dir: &Path) -> Option<String> {
    skimap_pack::Pack::open(pack_dir)
        .ok()
        .map(|p| p.manifest().pack_version.clone())
}

/// Bundles that bring the pack in `pack_dir` (missing for a first download) to the
/// latest version of `index_json`. The sync state next to the pack says which version
/// and base it has; a pack copied by hand (no state) at the latest version is up to date.
#[uniffi::export]
pub fn sync_plan(index_json: String, pack_dir: String) -> Result<SyncPlan, SyncError> {
    let index = parse_index(&index_json)?;
    let dir = Path::new(&pack_dir);
    update::recover(dir).map_err(invalid)?;
    let installed = installed_version(dir);
    let state = match sync::read_state(dir).map_err(invalid)? {
        // A state for a version no longer installed (pack removed) is ignored.
        Some(s) if Some(&s.version) == installed.as_ref() => Some(s),
        _ => None,
    };
    let bundles = match (&state, &installed) {
        (None, Some(v)) if *v == index.latest.version => Vec::new(),
        (None, Some(v)) => {
            // Installed without state (copied, or a state lost): the delta from its
            // version if published, else the full plan.
            let probe = SyncState {
                version: v.clone(),
                base_version: String::new(),
                index_etag: None,
            };
            sync::plan(&index, Some(&probe))
        }
        _ => sync::plan(&index, state.as_ref()),
    };
    Ok(plan_of(&index, installed, bundles))
}

/// The base bundle, then the delta from the base: after [`SyncError::MissingContents`].
#[uniffi::export]
pub fn sync_plan_full(index_json: String, pack_dir: String) -> Result<SyncPlan, SyncError> {
    let index = parse_index(&index_json)?;
    let installed = installed_version(Path::new(&pack_dir));
    let bundles = sync::plan_full(&index);
    Ok(plan_of(&index, installed, bundles))
}

/// Checks a downloaded file: its length and its SHA-256 (the bundle's name). A bundle that
/// fails is deleted by the caller and downloaded again.
#[uniffi::export]
pub fn verify_bundle(path: String, sha256: String, bytes: u64) -> Result<(), SyncError> {
    let (n, h) = sha256_file(Path::new(&path)).map_err(invalid)?;
    if n != bytes || !h.eq_ignore_ascii_case(&sha256) {
        return Err(SyncError::Corrupt {
            detail: format!("{path}: {n} bytes, SHA-256 {h}; expected {bytes} bytes, {sha256}"),
        });
    }
    Ok(())
}

/// Installs a downloaded and verified bundle into `pack_dir` (created for a first
/// download). The installed version stays complete and readable until the new one is
/// swapped in; an interrupted swap is finished or rolled back on the next call.
#[uniffi::export]
pub fn install_bundle(pack_dir: String, bundle_path: String) -> Result<InstallReport, SyncError> {
    let dir = Path::new(&pack_dir);
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| invalid(format!("{}: {e}", parent.display())))?;
    }
    match update::apply_bundle(dir, Path::new(&bundle_path)) {
        Ok(a) => Ok(InstallReport {
            version: a.version,
            fetched_files: a.fetched_files,
            reused_files: a.reused_files,
            removed_files: a.removed_files,
        }),
        Err(e) => {
            let detail = e.to_string();
            if detail.contains(MISSING_CONTENTS) {
                Err(SyncError::MissingContents { detail })
            } else if detail.contains("incomplete download") {
                Err(SyncError::Corrupt { detail })
            } else {
                Err(SyncError::Invalid { detail })
            }
        }
    }
}

/// Records a finished sync of `pack_dir` to the latest version of `index_json`.
#[uniffi::export]
pub fn record_sync(
    pack_dir: String,
    index_json: String,
    index_etag: Option<String>,
) -> Result<(), SyncError> {
    let index = parse_index(&index_json)?;
    sync::write_state(
        Path::new(&pack_dir),
        &SyncState {
            version: index.latest.version,
            base_version: index.base_version,
            index_etag,
        },
    )
    .map_err(invalid)
}

/// The sync state of `pack_dir`, when its version is the one installed.
#[uniffi::export]
pub fn sync_state(pack_dir: String) -> Option<SyncStateInfo> {
    let dir = Path::new(&pack_dir);
    let s = sync::read_state(dir).ok().flatten()?;
    (installed_version(dir).as_ref() == Some(&s.version)).then_some(SyncStateInfo {
        version: s.version,
        base_version: s.base_version,
        index_etag: s.index_etag,
    })
}

/// Removes an installed pack, its sync state and any leftover of an interrupted update.
#[uniffi::export]
pub fn remove_pack(pack_dir: String) -> Result<(), SyncError> {
    let dir = Path::new(&pack_dir);
    for p in [
        dir.to_path_buf(),
        update::staging_dir(dir),
        update::previous_dir(dir),
    ] {
        match std::fs::remove_dir_all(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(invalid(format!("{}: {e}", p.display()))),
        }
    }
    match std::fs::remove_file(sync::state_path(dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(invalid(e)),
    }
}
