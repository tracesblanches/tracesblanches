//! Biodiv'sports sensitive areas (Geotrek API v2).
//!
//! Without a `period` parameter the API only returns the areas sensitive in the current
//! month (report 04: 236 areas on France in November against 1358 with `period=ignore`).
//! We always ask `period=ignore` and keep each area's months from its `period` flags.

use std::collections::BTreeMap;

use geojson::feature::Id;
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;

use super::{RawZone, feature_parts, fetch_fc, fetch_json, prop_str};
use crate::error::{Error, Result};
use crate::layer::{Months, ZoneKind, ZoneProps};

/// API root.
pub const API: &str = "https://biodiv-sports.fr/api/v2";

/// URL of the sensitive areas intersecting the box, every month.
pub fn areas_url(b: BBox) -> String {
    format!(
        "{API}/sensitivearea/?in_bbox={},{},{},{}&format=geojson&period=ignore&language=fr",
        b.west, b.south, b.east, b.north
    )
}

/// Practice names by id (`1` → `terrestre`), from `/sportpractice/`.
pub fn practices(dl: &Downloader) -> Result<BTreeMap<u64, String>> {
    let url = format!("{API}/sportpractice/?format=json");
    let v = fetch_json(dl, "biodiv-sports", &url)?;
    let results = v
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| Error::data(&url, "no `results` array"))?;
    let mut out = BTreeMap::new();
    for r in results {
        let id = r.get("id").and_then(|i| i.as_u64());
        let name = r.pointer("/name/fr").and_then(|n| n.as_str());
        if let (Some(id), Some(name)) = (id, name) {
            out.insert(id, name.trim().to_lowercase());
        }
    }
    Ok(out)
}

/// Fetches every sensitive area intersecting the box (follows `next` pages).
pub fn fetch(dl: &Downloader, b: BBox) -> Result<Vec<RawZone>> {
    let names = practices(dl)?;
    let mut out = Vec::new();
    let mut url = Some(areas_url(b));
    while let Some(u) = url.take() {
        let fc = fetch_fc(dl, "biodiv-sports", &u)?;
        url = fc
            .foreign_members
            .as_ref()
            .and_then(|m| m.get("next"))
            .and_then(|n| n.as_str())
            .map(str::to_owned);
        for f in fc.features {
            let id = match &f.id {
                Some(Id::Number(n)) => n.to_string(),
                Some(Id::String(s)) => s.clone(),
                None => return Err(Error::data(&u, "sensitive area without id")),
            };
            let (p, geometry) = feature_parts(f, &u)?;
            let flags: Vec<bool> = p
                .get("period")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().map(|x| x.as_bool().unwrap_or(false)).collect())
                .unwrap_or_default();
            let months = Months::from_flags(&flags)
                .map_err(|e| Error::data(&u, format!("area {id}: {e}")))?;
            let practices = p
                .get("practices")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_u64())
                        .map(|i| {
                            names
                                .get(&i)
                                .cloned()
                                .unwrap_or_else(|| format!("practice-{i}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(RawZone {
                props: ZoneProps {
                    id: format!("biodiv:{id}"),
                    kind: ZoneKind::BiodivSports,
                    name: prop_str(&p, "name").unwrap_or_else(|| format!("Biodiv'sports {id}")),
                    source: "biodiv-sports".into(),
                    url: prop_str(&p, "info_url"),
                    months: Some(months),
                    practices,
                    designated: None,
                    area_ha: 0.0,
                    clipped: false,
                    note: None,
                },
                geometry,
            });
        }
    }
    tracing::info!(areas = out.len(), "Biodiv'sports");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_always_asks_every_month() {
        let u = areas_url(BBox::new(46.54, 6.02, 46.68, 6.2));
        assert!(u.contains("in_bbox=6.02,46.54,6.2,46.68"), "{u}");
        assert!(u.contains("period=ignore"));
    }
}
