//! Android library of the core (K1, #51), exposed to Kotlin through UniFFI.
//!
//! The app never parses the pack itself: it lists the packs copied to its storage
//! ([`list_packs`]), opens one with the phone-side reader of `skimap-pack`
//! ([`open_pack`], [`PackHandle::info`]) and receives a complete MapLibre style
//! ([`PackHandle::map_style`]): the basemap style of the pack with every URL made absolute
//! (`pmtiles://file://…`, `file://…` glyphs and sprites) and the pack layers drawn on top
//! (pistes with colour, shape, name and direction, piste status, refuges). The symbols the
//! style refers to and the legend rows come from [`map_symbols`] and
//! [`PackHandle::legend`], so colours and shapes are defined once, here.
//!
//! No network, no SQLite: the crate links the reader of `skimap-pack` without its
//! `pipeline` feature.

pub mod card;
pub mod catalog;
pub mod info;
pub mod style;
pub mod symbols;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use skimap_pack::Pack;

pub use card::{Card, Fact, Link, Phone, feature_card};
pub use catalog::{Catalog, Domain, DomainDistance, PackStatus};
pub use info::{BasemapInfo, Bounds, OverlayInfo, PackEntry, PackInfo, StatusSummary};
pub use style::MapStyle;
pub use symbols::{LegendEntry, MapSymbol, SymbolShape};

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
    /// Basemap identifier not in the pack.
    #[error("no basemap {id:?} in the pack")]
    UnknownBasemap {
        /// Identifier asked for.
        id: String,
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

/// Everything the app needs to show a pack, in one call across the FFI (calls are coarse:
/// one per pack opened, one per basemap switch).
#[derive(Debug, uniffi::Record)]
pub struct PackView {
    /// The opened pack, for later calls ([`PackHandle::map_style`]).
    pub handle: Arc<PackHandle>,
    /// Summary of the pack.
    pub info: PackInfo,
    /// Style with `basemap` (or the first basemap).
    pub style: MapStyle,
    /// Legend rows.
    pub legend: Vec<LegendEntry>,
    /// Symbols the style refers to.
    pub symbols: Vec<MapSymbol>,
    /// [`core_version`], shown with the pack sources.
    pub core_version: String,
}

/// Opens the pack in `dir` and composes its style with `basemap` (`None`: the first one).
#[uniffi::export]
pub fn open_pack(dir: String, basemap: Option<String>) -> Result<PackView, PackError> {
    let handle = Arc::new(PackHandle::open(PathBuf::from(dir))?);
    let style = handle.map_style(basemap)?;
    Ok(PackView {
        info: handle.info(),
        legend: handle.legend(),
        symbols: map_symbols(),
        core_version: core_version(),
        style,
        handle,
    })
}

/// Symbols (images) the map style refers to; the app draws them and registers them under
/// [`MapSymbol::id`] before the style loads.
#[uniffi::export]
pub fn map_symbols() -> Vec<MapSymbol> {
    symbols::map_symbols()
}

/// An opened pack.
#[derive(Debug, uniffi::Object)]
pub struct PackHandle {
    pack: Pack,
    info: PackInfo,
    /// Piste name points, computed once (reused by every basemap switch).
    labels: Option<serde_json::Value>,
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
        let labels = if info.overlays.iter().any(|o| o.id == "pistes") {
            Some(style::piste_labels(&pack)?)
        } else {
            None
        };
        Ok(Self { pack, info, labels })
    }

    /// The reader of `skimap-pack` behind this handle.
    pub fn pack(&self) -> &Pack {
        &self.pack
    }
}

#[uniffi::export]
impl PackHandle {
    /// Summary of the pack: site, validity, basemaps, overlays, piste status, attributions.
    pub fn info(&self) -> PackInfo {
        self.info.clone()
    }

    /// Complete MapLibre style: `basemap` (or the first basemap of the pack when `None`; a
    /// plain snow-white background when the pack has none) with the pack layers on top.
    pub fn map_style(&self, basemap: Option<String>) -> Result<MapStyle, PackError> {
        style::compose(&self.pack, basemap.as_deref(), self.labels.as_ref())
    }

    /// Legend rows of the layers drawn by [`PackHandle::map_style`] for this pack.
    pub fn legend(&self) -> Vec<LegendEntry> {
        symbols::legend(&self.info)
    }
}

/// Identifier of the free mode in style names and in [`PackInfo::site_id`].
pub const FREE_MODE_ID: &str = "libre";

/// Several packs shown together (free mode: everything installed in the frame), or one
/// (area mode). One call opens them all, one call per basemap switch.
#[derive(Debug, uniffi::Object)]
pub struct PackSet {
    handles: Vec<PackHandle>,
    info: PackInfo,
}

/// [`PackView`] of a [`PackSet`].
#[derive(Debug, uniffi::Record)]
pub struct PackSetView {
    /// The opened packs, for later calls ([`PackSet::map_style`]).
    pub set: Arc<PackSet>,
    /// Summary: the pack itself when there is one, else the packs merged
    /// ([`info::merge`]).
    pub info: PackInfo,
    /// Style with `basemap` (or the first basemap of the first pack).
    pub style: MapStyle,
    /// Legend rows.
    pub legend: Vec<LegendEntry>,
    /// Symbols the style refers to.
    pub symbols: Vec<MapSymbol>,
    /// [`core_version`].
    pub core_version: String,
}

/// Opens the packs in `dirs` together. Packs that do not open are skipped when at least one
/// opens (the error of the first one is returned otherwise).
#[uniffi::export]
pub fn open_packs(dirs: Vec<String>, basemap: Option<String>) -> Result<PackSetView, PackError> {
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
    if handles.is_empty() {
        return Err(first_error.unwrap_or(PackError::Invalid {
            detail: "no pack to open".to_owned(),
        }));
    }
    let infos: Vec<PackInfo> = handles.iter().map(|h| h.info.clone()).collect();
    let info = if infos.len() == 1 {
        infos[0].clone()
    } else {
        info::merge(&infos, FREE_MODE_ID)
    };
    let set = Arc::new(PackSet { handles, info });
    let style = set.map_style(basemap)?;
    Ok(PackSetView {
        info: set.info.clone(),
        legend: set.legend(),
        symbols: map_symbols(),
        core_version: core_version(),
        style,
        set,
    })
}

#[uniffi::export]
impl PackSet {
    /// Style of every pack of the set with `basemap`.
    pub fn map_style(&self, basemap: Option<String>) -> Result<MapStyle, PackError> {
        let parts: Vec<(&Pack, Option<&serde_json::Value>)> = self
            .handles
            .iter()
            .map(|h| (&h.pack, h.labels.as_ref()))
            .collect();
        style::compose_set(&parts, basemap.as_deref(), &self.info.site_id)
    }

    /// Legend rows of the layers of the set.
    pub fn legend(&self) -> Vec<LegendEntry> {
        symbols::legend(&self.info)
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
