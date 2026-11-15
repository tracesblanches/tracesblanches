//! Pack listing and summary for the app.

use std::path::Path;

use skimap_pack::Pack;
use skimap_pack::manifest::MANIFEST_FILE;

use crate::PackError;

/// A pack directory found by [`crate::list_packs`].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PackEntry {
    /// Absolute pack directory.
    pub dir: String,
    /// Site identifier, or the directory name when the manifest does not open.
    pub site_id: String,
    /// Site display name, or the directory name when the manifest does not open.
    pub site_name: String,
    /// Pack version (`AAAAMMJJTHHMMSSZ`), empty when the manifest does not open.
    pub pack_version: String,
    /// Why the pack does not open, if it does not.
    pub error: Option<String>,
    /// Site bounds, when the manifest opens.
    pub bounds: Option<Bounds>,
}

/// WGS84 bounds.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Bounds {
    /// West longitude.
    pub west: f64,
    /// South latitude.
    pub south: f64,
    /// East longitude.
    pub east: f64,
    /// North latitude.
    pub north: f64,
}

/// A basemap of the pack.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BasemapInfo {
    /// Identifier (`plan-ign`, `osm`).
    pub id: String,
    /// Title, French.
    pub title: String,
    /// Lowest zoom stored.
    pub minzoom: u8,
    /// Highest zoom stored (the map overzooms beyond).
    pub maxzoom: u8,
}

/// Piste status (layer `status`), counted per itinerary.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StatusSummary {
    /// Itineraries reported open.
    pub open: u32,
    /// Itineraries reported closed.
    pub closed: u32,
    /// Itineraries with another or no state.
    pub unknown: u32,
    /// Latest poll time of the reports (RFC 3339), if any.
    pub polled_at: Option<String>,
}

/// Summary of an opened pack.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PackInfo {
    /// Absolute pack directory.
    pub dir: String,
    /// Site identifier.
    pub site_id: String,
    /// Site display name.
    pub site_name: String,
    /// Département number.
    pub department: String,
    /// Pack version (`AAAAMMJJTHHMMSSZ`).
    pub pack_version: String,
    /// Build time (RFC 3339).
    pub generated_at: String,
    /// Earliest end of validity of the layers (`AAAA-MM-JJ`), if any.
    pub valid_until: Option<String>,
    /// Validity note, French.
    pub validity_note: String,
    /// Site bounds.
    pub bounds: Bounds,
    /// Commune centre, longitude.
    pub center_lon: f64,
    /// Commune centre, latitude.
    pub center_lat: f64,
    /// Basemaps, in pack order (the first is the default).
    pub basemaps: Vec<BasemapInfo>,
    /// Piste status, when the pack has the `status` layer.
    pub status: Option<StatusSummary>,
    /// Whether personal-use-only sources were included in the pack.
    pub personal_sources: bool,
    /// Every attribution of the pack, deduplicated.
    pub attributions: Vec<String>,
}

pub(crate) fn list_packs(root: &Path) -> Vec<PackEntry> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        // Hidden directories are the work of an update (`.<site>.new`, `.<site>.old`,
        // downloads in progress), never a pack to show.
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                && p.join(MANIFEST_FILE).is_file()
        })
        .collect();
    dirs.sort();
    let mut out: Vec<PackEntry> = dirs
        .into_iter()
        .map(|dir| {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let path = dir.display().to_string();
            match Pack::open(&dir) {
                Ok(p) => {
                    let m = p.manifest();
                    let [west, south, east, north] = m.site.bbox;
                    PackEntry {
                        dir: path,
                        site_id: m.site.id.clone(),
                        site_name: m.site.name.clone(),
                        pack_version: m.pack_version.clone(),
                        error: None,
                        bounds: Some(Bounds {
                            west,
                            south,
                            east,
                            north,
                        }),
                    }
                }
                Err(e) => PackEntry {
                    dir: path,
                    site_id: name.clone(),
                    site_name: name,
                    pack_version: String::new(),
                    error: Some(e.to_string()),
                    bounds: None,
                },
            }
        })
        .collect();
    out.sort_by_cached_key(|e| (e.site_name.to_lowercase(), e.dir.clone()));
    out
}

pub(crate) fn summarise(pack: &Pack) -> Result<PackInfo, PackError> {
    let m = pack.manifest();
    let [west, south, east, north] = m.site.bbox;
    let status = if m.layer("status").is_some() {
        Some(status_summary(pack)?)
    } else {
        None
    };
    Ok(PackInfo {
        dir: pack.root().display().to_string(),
        site_id: m.site.id.clone(),
        site_name: m.site.name.clone(),
        department: m.site.department.clone(),
        pack_version: m.pack_version.clone(),
        generated_at: m.generated_at.clone(),
        valid_until: m.valid_for.until.clone(),
        validity_note: m.valid_for.note.clone(),
        bounds: Bounds {
            west,
            south,
            east,
            north,
        },
        center_lon: m.site.center[0],
        center_lat: m.site.center[1],
        basemaps: m
            .basemaps
            .iter()
            .map(|b| BasemapInfo {
                id: b.id.clone(),
                title: b.title.clone(),
                minzoom: b.minzoom,
                maxzoom: b.maxzoom,
            })
            .collect(),
        status,
        personal_sources: m.personal_sources,
        attributions: m.attributions.clone(),
    })
}

fn status_summary(pack: &Pack) -> Result<StatusSummary, PackError> {
    let fc = pack.read_geojson("status")?;
    let mut s = StatusSummary {
        open: 0,
        closed: 0,
        unknown: 0,
        polled_at: None,
    };
    for f in &fc.features {
        let props = f.properties.as_ref();
        let state = props.and_then(|p| p.get("state")).and_then(|v| v.as_str());
        match state {
            Some("open") => s.open += 1,
            Some("closed") => s.closed += 1,
            _ => s.unknown += 1,
        }
        if let Some(t) = props
            .and_then(|p| p.get("polled_at"))
            .and_then(|v| v.as_str())
            && s.polled_at.as_deref().is_none_or(|cur| t > cur)
        {
            s.polled_at = Some(t.to_owned());
        }
    }
    Ok(s)
}

/// Summary of several packs shown together (free mode): bounds and status added up, the
/// earliest validity, basemaps common to all, attributions of any.
pub fn merge(infos: &[PackInfo], id: &str) -> PackInfo {
    let mut out = infos[0].clone();
    out.dir = String::new();
    out.site_id = id.to_owned();
    out.site_name = "Tous les domaines installés".to_owned();
    out.department = String::new();
    for i in &infos[1..] {
        out.pack_version = out.pack_version.clone().max(i.pack_version.clone());
        out.generated_at = out.generated_at.clone().max(i.generated_at.clone());
        out.valid_until = match (out.valid_until.take(), i.valid_until.clone()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        out.bounds = Bounds {
            west: out.bounds.west.min(i.bounds.west),
            south: out.bounds.south.min(i.bounds.south),
            east: out.bounds.east.max(i.bounds.east),
            north: out.bounds.north.max(i.bounds.north),
        };
        out.basemaps
            .retain(|b| i.basemaps.iter().any(|c| c.id == b.id));
        out.status = match (out.status.take(), i.status.clone()) {
            (Some(a), Some(b)) => Some(StatusSummary {
                open: a.open + b.open,
                closed: a.closed + b.closed,
                unknown: a.unknown + b.unknown,
                polled_at: a.polled_at.max(b.polled_at),
            }),
            (a, b) => a.or(b),
        };
        out.personal_sources |= i.personal_sources;
        for a in &i.attributions {
            if !out.attributions.contains(a) {
                out.attributions.push(a.clone());
            }
        }
    }
    out.center_lon = (out.bounds.west + out.bounds.east) / 2.0;
    out.center_lat = (out.bounds.south + out.bounds.north) / 2.0;
    out.validity_note = format!(
        "{} packs : {}.",
        infos.len(),
        infos
            .iter()
            .map(|i| i.site_name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    out
}
