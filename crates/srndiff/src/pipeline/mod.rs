//! Pipeline side (feature `pipeline`): references, corridor profiles, evaluation.
//!
//! Outputs in `data/out/france/srndiff/`:
//!
//! | File | Content |
//! |---|---|
//! | `references.json` | tracks with their published ratings and the review tracks |
//! | `profiles/<id>.json` | profile of each track (cache of the corridor computation) |
//! | `evaluation.json` | agreement, confusion matrices, calibration, leave-one-route-out |
//! | `review.txt` | French text of the tracks reviewed by hand |
//! | `srndiff.meta.json` | sources, licences, parameters, figures |

pub mod corridor;
pub mod eval;
pub mod refs;
pub mod roads;

use std::path::PathBuf;

use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_zones::chrono::{DateTime, Utc};

use crate::profile::Profile;
use crate::{Error, Result};
use corridor::{Corridor, CorridorStats, SiteLayers};
use refs::RefRoute;

/// Layer name.
pub const LAYER: &str = "srndiff";

/// Output directory.
pub fn out_dir(layout: &DataLayout) -> Result<PathBuf> {
    layout
        .france_layer_dir(LAYER)
        .map_err(|e| Error::Data(e.to_string()))
}

/// File name of a profile.
pub fn profile_path(layout: &DataLayout, id: &str) -> Result<PathBuf> {
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(out_dir(layout)?
        .join("profiles")
        .join(format!("{safe}.json")))
}

/// Reads a cached profile.
pub fn read_profile(layout: &DataLayout, id: &str) -> Result<Option<Profile>> {
    let p = profile_path(layout, id)?;
    if !p.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&p).map_err(|e| Error::Data(format!("{}: {e}", p.display())))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| Error::Data(format!("{}: {e}", p.display())))
}

/// Builds (or reads from the cache) the profile of a reference: corridor terrain and
/// site layers. Exits are the two ends until [`add_roads`] adds the roads.
pub fn profile_of(
    layout: &DataLayout,
    dl: &Downloader,
    route: &RefRoute,
    sites: &[SiteLayers],
    at: DateTime<Utc>,
    c: &Corridor,
    force: bool,
) -> Result<(Profile, Option<CorridorStats>)> {
    if !force && let Some(p) = read_profile(layout, &route.id)? {
        return Ok((p, None));
    }
    let (p, st) = corridor::build_profile(&route.positions(), dl, sites, at, None, c)?;
    write_profile(layout, &route.id, &p)?;
    Ok((p, Some(st)))
}

/// Writes a profile to the cache.
pub fn write_profile(layout: &DataLayout, id: &str, p: &Profile) -> Result<()> {
    let path = profile_path(layout, id)?;
    skimap_common::output::write_json(&path, p).map_err(|e| Error::Data(e.to_string()))
}

/// Marks the roads (BD TOPO) as exits on a profile; returns the number of road points.
pub fn add_roads(dl: &Downloader, route: &RefRoute, p: &mut Profile) -> Result<usize> {
    let pts = roads::road_points(&route.positions(), dl)?;
    crate::profile::mark_exits(&mut p.samples, &pts);
    p.inputs.roads = true;
    Ok(pts.len())
}
