//! Android library of the core (K1, #51), exposed to Kotlin through UniFFI.
//!
//! The app never parses the pack itself: it lists the packs of its storage
//! ([`list_packs`]), opens one or several with the phone-side reader of `skimap-pack`
//! ([`open_packs`]) and instantiates on them a style validated by the user (`styles/`,
//! K15: [`style::instantiate`], [`PackSet::map_style`]). Cards of touched features
//! ([`feature_card`]), the census of the nordic areas ([`Catalog`]) and the download and
//! update of packs from the server ([`sync`]: plan, bundle check and install, state) are
//! here too, in coarse calls.
//!
//! The day (J5, K4, K5, K9, C11): [`tracking_day`] computes the morning on the pack (briefing,
//! best time, wax, loops with the group's time) and writes the validated plan;
//! [`tracking`] follows it during the outing on a thread of its own (batches of fixes in,
//! what to say, alerts, switches, proposals, GPS mode and guidance out); [`tracking_back`]
//! guides « ramène-moi » along the way skied.
//!
//! No HTTP and no SQLite in this crate: the app makes the requests (K2), this crate
//! decides what to download and installs it with `skimap-pack` (without its `pipeline`
//! feature).

// Unit tests write their inputs directly; the code itself goes through
// `skimap_common::output` (clippy.toml, #164).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod card;
pub mod card_map;
pub mod catalog;
pub mod community;
pub mod export;
pub mod fees;
pub mod fog;
pub mod france_sync;
pub mod group;
pub mod info;
pub mod offpiste;
pub mod prep;
pub mod recap;
pub mod rules;
pub mod style;
pub mod sync;
pub mod text;
pub mod tracking;
pub mod tracking_back;
pub mod tracking_day;
pub mod voicecmd;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use skimap_pack::Pack;

pub use card::{Card, Fact, Lang, Link, Phone, feature_card};
pub use catalog::{Catalog, Domain, DomainDistance, PackStatus};
pub use fees::{FeePrice, FeeZone};
pub use info::{BasemapInfo, Bounds, PackEntry, PackInfo, StatusSummary};
pub use style::{LayerGroup, LegendEntry, LegendKind, MapStyle, OverlayLayers};

uniffi::setup_scaffolding!();

/// Errors returned to Kotlin (`PackException` on the Kotlin side).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PackError {
    /// No `manifest.json` in the directory.
    #[error("no pack at {path}")]
    NotFound {
        /// Directory looked at.
        path: String,
    },
    /// Unreadable or unsupported pack, or a path escaping the pack.
    #[error("invalid pack: {detail}")]
    Invalid {
        /// What is wrong (`message` would clash with `Exception.message` in Kotlin).
        detail: String,
    },
}

impl From<skimap_pack::Error> for PackError {
    fn from(e: skimap_pack::Error) -> Self {
        Self::Invalid {
            detail: e.to_string(),
        }
    }
}

/// Version of this library (`skimap-ffi x.y.z`), shown in the app.
#[uniffi::export]
pub fn core_version() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// Packs found in the sub-directories of `root` (one directory per pack, holding its
/// `manifest.json`), sorted by site name, ignoring case. A directory whose manifest does not open is
/// listed with its error; a directory without manifest is ignored. A missing `root` gives
/// an empty list.
#[uniffi::export]
pub fn list_packs(root: String) -> Vec<PackEntry> {
    info::list_packs(Path::new(&root))
}

/// An opened pack.
#[derive(Debug)]
pub struct PackHandle {
    pack: Pack,
    info: PackInfo,
}

impl PackHandle {
    /// Opens and summarises the pack in `dir` (made absolute: style URLs are absolute).
    pub fn open(dir: PathBuf) -> Result<Self, PackError> {
        let dir = std::path::absolute(&dir).map_err(|e| PackError::Invalid {
            detail: format!("{}: {e}", dir.display()),
        })?;
        if !dir.join(skimap_pack::manifest::MANIFEST_FILE).is_file() {
            return Err(PackError::NotFound {
                path: dir.display().to_string(),
            });
        }
        let pack = Pack::open(dir)?;
        let info = info::summarise(&pack)?;
        Ok(Self { pack, info })
    }

    /// The reader of `skimap-pack` behind this handle.
    pub fn pack(&self) -> &Pack {
        &self.pack
    }

    /// Summary of the pack: site, validity, basemaps, piste status, attributions.
    pub fn info(&self) -> &PackInfo {
        &self.info
    }
}

/// Identifier of the free mode in style names and in [`PackInfo::site_id`].
pub const FREE_MODE_ID: &str = "libre";

/// Several packs shown together (free mode: everything installed), or one (area mode).
/// One call opens them all, one call per style switch (practice, theme, guidance).
#[derive(Debug, uniffi::Object)]
pub struct PackSet {
    handles: Vec<PackHandle>,
    info: PackInfo,
    free: bool,
    /// The France basemap under the packs (X20), when the phone has it.
    france: Option<style::France>,
}

/// What [`open_packs`] returns.
#[derive(Debug, uniffi::Record)]
pub struct PackSetView {
    /// The opened packs, for later calls ([`PackSet::map_style`]).
    pub set: Arc<PackSet>,
    /// Summary: the pack itself when there is one, else the packs merged
    /// ([`info::merge`]).
    pub info: PackInfo,
    /// `style_json` instantiated on the packs.
    pub style: MapStyle,
    /// [`core_version`].
    pub core_version: String,
}

/// Opens the packs in `dirs` together and instantiates `style_json` (a style of
/// `styles/`) on them. `free`: free mode, named `libre` even with a single pack. Packs that
/// do not open are skipped when at least one opens (the error of the first one is
/// returned otherwise). The texts of the merged summary are in `lang`.
#[uniffi::export]
pub fn open_packs(
    dirs: Vec<String>,
    style_json: String,
    free: bool,
    lang: Lang,
) -> Result<PackSetView, PackError> {
    open_map(dirs, None, style_json, free, lang)
}

/// [`open_packs`] over the France basemap of the phone cache (X20, #154), in
/// `france_dir` (`france.json`): drawn under the packs, and alone in free mode when no
/// pack is installed (`dirs` empty). A France basemap that does not open is left out
/// when a pack opens.
#[uniffi::export]
pub fn open_map(
    dirs: Vec<String>,
    france_dir: Option<String>,
    style_json: String,
    free: bool,
    lang: Lang,
) -> Result<PackSetView, PackError> {
    let france = match france_dir.map(|d| style::France::open(Path::new(&d))) {
        Some(Ok(f)) => Some(f),
        Some(Err(e)) => {
            if dirs.is_empty() {
                return Err(e);
            }
            None
        }
        None => None,
    };
    let mut handles = Vec::new();
    let mut first_error = None;
    for d in dirs {
        match PackHandle::open(PathBuf::from(d)) {
            Ok(h) => handles.push(h),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    let infos: Vec<PackInfo> = handles.iter().map(|h| h.info.clone()).collect();
    let info = match (infos.len(), &france) {
        (0, Some(f)) if free => info::france_only(&f.basemap, FREE_MODE_ID, lang),
        (0, _) => {
            return Err(first_error.unwrap_or(PackError::Invalid {
                detail: "no pack to open".to_owned(),
            }));
        }
        (1, _) if !free => infos[0].clone(),
        _ => info::merge(&infos, FREE_MODE_ID, lang),
    };
    // Free mode over the France basemap: the camera may go anywhere in France.
    let info = match &france {
        Some(f) if free => info::over_france(info, &f.basemap),
        _ => info,
    };
    let set = Arc::new(PackSet {
        handles,
        info,
        free,
        france,
    });
    let style = set.map_style(style_json)?;
    Ok(PackSetView {
        info: set.info.clone(),
        core_version: core_version(),
        style,
        set,
    })
}

#[uniffi::export]
impl PackSet {
    /// `style_json` (a style of `styles/`) instantiated on every pack of the set.
    pub fn map_style(&self, style_json: String) -> Result<MapStyle, PackError> {
        let packs: Vec<&Pack> = self.handles.iter().map(|h| &h.pack).collect();
        let name = if self.free {
            FREE_MODE_ID
        } else {
            &self.info.site_id
        };
        style::instantiate_over(&style_json, &packs, self.france.as_ref(), name)
    }

    /// Trail-pass zones of the packs of the set (H2), one per pass.
    pub fn fee_zones(&self) -> Result<Vec<FeeZone>, PackError> {
        let packs: Vec<&Pack> = self.handles.iter().map(|h| &h.pack).collect();
        fees::fee_zones(&packs)
    }

    /// Number of packs in the set.
    pub fn len(&self) -> u32 {
        u32::try_from(self.handles.len()).unwrap_or(u32::MAX)
    }

    /// Whether the set is empty (never, once opened).
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_names_the_crate() {
        assert!(super::core_version().starts_with("skimap-ffi "));
    }
}
