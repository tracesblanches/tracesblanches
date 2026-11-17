//! Phone-side synchronisation with the server (K2, S0, E1): the site index, which bundles
//! to download, and what the phone remembers between two syncs.
//!
//! The server publishes, per site, one small `index.json` (short cache, `ETag`) and
//! immutable bundles named by their SHA-256 (`/v1/bundles/<sha256>.skb`, see
//! [`crate::update::write_bundle`]). Nothing is computed per request: the bundles are made
//! when a pack version is published, for the domain, whatever the number of phones.
//!
//! - **Base bundle**: the whole pack at the *base* version, rebuilt only when the pack
//!   drifted from it by more than a tenth (relief, avalanche exposure, basemaps).
//! - **Deltas**: from the base and from each of the last versions to the latest; a
//!   daily sync is one delta of a few hundred kilobytes.
//!
//! A sync: `GET index.json` with `If-None-Match` (304: done, one tiny request), then the
//! bundles of [`plan`] (usually one), each installed with [`crate::update::apply_bundle`],
//! then [`write_state`]. A phone that stayed offline for weeks still gets the delta from
//! the base, not the whole pack.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::manifest::{PackValidity, SiteInfo};
use crate::update::ItemChange;

/// Value of [`SiteIndex::format`].
pub const INDEX_FORMAT: &str = "skimap-site-index";
/// Index format version written by this crate.
pub const INDEX_FORMAT_VERSION: u32 = 1;

/// One downloadable bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleRef {
    /// Version the bundle starts from; `None`: from nothing (the base bundle).
    pub from: Option<String>,
    /// Version it leads to.
    pub to: String,
    /// SHA-256 of the bundle file: its name.
    pub sha256: String,
    /// Size of the bundle file.
    pub bytes: u64,
    /// URL relative to the server: `/v1/bundles/<sha256>.skb`.
    pub url: String,
    /// Files of the new version written from the bundle.
    pub blobs: u64,
}

/// The latest version of a site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatestVersion {
    /// `Manifest::pack_version`.
    pub version: String,
    /// Publication time, RFC 3339.
    pub published_at: String,
    /// SHA-256 of its `manifest.json`.
    pub manifest_sha256: String,
    /// Size of the pack.
    pub total_bytes: u64,
    /// Number of files.
    pub files: u64,
    /// Validity of the pack as a whole.
    pub valid_for: PackValidity,
    /// Whether personal-use-only layers are included.
    pub personal_sources: bool,
}

/// `/v1/sites/<site>/index.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteIndex {
    /// [`INDEX_FORMAT`].
    pub format: String,
    /// [`INDEX_FORMAT_VERSION`].
    pub format_version: u32,
    /// The site.
    pub site: SiteInfo,
    /// Latest version.
    pub latest: LatestVersion,
    /// Base version (the base bundle installs it).
    pub base_version: String,
    /// The whole pack at [`SiteIndex::base_version`].
    pub base: BundleRef,
    /// Deltas to the latest version: from the base (unless it is the latest) and from each
    /// of the last versions.
    pub deltas: Vec<BundleRef>,
    /// Layers, basemaps and voice of the latest version with their change since the
    /// previous version and their validity.
    pub changes: Vec<ItemChange>,
}

/// What the phone remembers of its last sync of a site (next to the pack directory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    /// Installed version.
    pub version: String,
    /// Base version of the index it was installed from.
    pub base_version: String,
    /// `ETag` of that index, for `If-None-Match`.
    #[serde(default)]
    pub index_etag: Option<String>,
}

/// Bundles to download and install, in order, to reach [`SiteIndex::latest`]:
/// nothing if up to date; the delta from the installed version if the server has it;
/// the delta from the base if the phone was installed from the same base; else the base
/// bundle then the delta from the base.
pub fn plan(index: &SiteIndex, state: Option<&SyncState>) -> Vec<BundleRef> {
    let latest = &index.latest.version;
    if let Some(s) = state {
        if &s.version == latest {
            return Vec::new();
        }
        if let Some(d) = index
            .deltas
            .iter()
            .find(|d| d.from.as_deref() == Some(s.version.as_str()))
        {
            return vec![d.clone()];
        }
        if s.base_version == index.base_version
            && let Some(d) = from_base(index)
        {
            return vec![d.clone()];
        }
    }
    plan_full(index)
}

/// The base bundle, then the delta from the base if the base is not the latest version.
/// Also the fallback when a delta fails with [`crate::update::MISSING_CONTENTS`].
pub fn plan_full(index: &SiteIndex) -> Vec<BundleRef> {
    let mut out = vec![index.base.clone()];
    if index.base_version != index.latest.version
        && let Some(d) = from_base(index)
    {
        out.push(d.clone());
    }
    out
}

fn from_base(index: &SiteIndex) -> Option<&BundleRef> {
    index
        .deltas
        .iter()
        .find(|d| d.from.as_deref() == Some(index.base_version.as_str()))
}

/// Where [`SyncState`] is kept: `.<pack directory name>.sync.json` next to the pack.
pub fn state_path(pack_dir: &Path) -> PathBuf {
    let name = pack_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pack".to_owned());
    pack_dir.with_file_name(format!(".{name}.sync.json"))
}

/// The sync state of a pack, if any.
pub fn read_state(pack_dir: &Path) -> Result<Option<SyncState>> {
    let p = state_path(pack_dir);
    match std::fs::read(&p) {
        Ok(b) => Ok(Some(
            serde_json::from_slice(&b).map_err(|e| Error::json(p.display().to_string(), e))?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(&p, e)),
    }
}

/// Records a sync (temporary file then rename).
pub fn write_state(pack_dir: &Path, state: &SyncState) -> Result<()> {
    let p = state_path(pack_dir);
    let body = serde_json::to_vec_pretty(state).map_err(|e| Error::json("sync state", e))?;
    Ok(skimap_common::output::write_bytes(&p, &body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(from: Option<&str>, to: &str) -> BundleRef {
        BundleRef {
            from: from.map(str::to_owned),
            to: to.into(),
            sha256: format!("{}-{to}", from.unwrap_or("none")),
            bytes: 1,
            url: String::new(),
            blobs: 1,
        }
    }

    fn index(base: &str, latest: &str, deltas: &[&str]) -> SiteIndex {
        SiteIndex {
            format: INDEX_FORMAT.into(),
            format_version: 1,
            site: SiteInfo {
                id: "cervieres".into(),
                name: "Cervières".into(),
                department: "05".into(),
                bbox: [0.0; 4],
                center: [0.0; 2],
                record: None,
            },
            latest: LatestVersion {
                version: latest.into(),
                published_at: String::new(),
                manifest_sha256: String::new(),
                total_bytes: 0,
                files: 0,
                valid_for: PackValidity {
                    until: None,
                    until_layer: None,
                    note: String::new(),
                },
                personal_sources: false,
            },
            base_version: base.into(),
            base: b(None, base),
            deltas: deltas.iter().map(|f| b(Some(f), latest)).collect(),
            changes: Vec::new(),
        }
    }

    fn st(v: &str, base: &str) -> SyncState {
        SyncState {
            version: v.into(),
            base_version: base.into(),
            index_etag: None,
        }
    }

    #[test]
    fn plans() {
        let i = index("v1", "v9", &["v1", "v7", "v8"]);
        assert!(plan(&i, Some(&st("v9", "v1"))).is_empty());
        assert_eq!(plan(&i, Some(&st("v8", "v1"))), [b(Some("v8"), "v9")]);
        // Offline for weeks, same base: the delta from the base.
        assert_eq!(plan(&i, Some(&st("v3", "v1"))), [b(Some("v1"), "v9")]);
        // Another base, or first install: base then delta.
        let full = [b(None, "v1"), b(Some("v1"), "v9")];
        assert_eq!(plan(&i, Some(&st("v3", "v0"))), full);
        assert_eq!(plan(&i, None), full);
        // Latest is the base: the base bundle alone.
        let i = index("v9", "v9", &["v7", "v8"]);
        assert_eq!(plan(&i, None), [b(None, "v9")]);
        assert_eq!(plan(&i, Some(&st("v8", "v1"))), [b(Some("v8"), "v9")]);
    }

    #[test]
    fn state_next_to_the_pack() {
        let d = tempfile::tempdir().unwrap();
        let pack = d.path().join("cervieres");
        assert_eq!(state_path(&pack), d.path().join(".cervieres.sync.json"));
        assert_eq!(read_state(&pack).unwrap(), None);
        write_state(&pack, &st("v2", "v1")).unwrap();
        assert_eq!(read_state(&pack).unwrap(), Some(st("v2", "v1")));
    }
}
