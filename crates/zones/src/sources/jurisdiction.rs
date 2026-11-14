//! French jurisdictions inside a site box: communes, départements and the French part of
//! the box, from the commune outlines of geo.api.gouv.fr.
//!
//! The départements are discovered by asking geo.api.gouv.fr which commune contains each
//! point of an 11 × 11 grid over the box (one cached request per point, ~1.5 km apart on
//! the two sites), so that a new site needs no hand-written list.

use std::collections::BTreeSet;

use geo::{BoundingRect, MultiPolygon};
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;

use super::{RawZone, feature_parts, fetch_fc, fetch_json, prop_str};
use crate::error::{Error, Result};
use crate::geom::{clip_to_bbox, rects_overlap, union_all};
use crate::layer::{ZoneKind, ZoneProps};

/// geo.api.gouv.fr root.
pub const API: &str = "https://geo.api.gouv.fr";

/// Points per side of the discovery grid.
const GRID: usize = 11;

/// Communes, départements and French part of a site box, clipped to the box.
#[derive(Debug, Clone)]
pub struct French {
    /// Communes intersecting the box (`commune:<INSEE>`).
    pub communes: Vec<RawZone>,
    /// Départements intersecting the box (`dept:<code>`).
    pub departments: Vec<RawZone>,
    /// French part of the box (`country:fr`).
    pub country: RawZone,
}

/// Départements having a commune under one of the grid points.
pub fn departments_in_bbox(dl: &Downloader, b: BBox) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for i in 0..GRID {
        for j in 0..GRID {
            let lat = b.south + (b.north - b.south) * i as f64 / (GRID - 1) as f64;
            let lon = b.west + (b.east - b.west) * j as f64 / (GRID - 1) as f64;
            let url = format!(
                "{API}/communes?lat={lat:.5}&lon={lon:.5}&fields=code,codeDepartement&format=json"
            );
            let v = fetch_json(dl, "geo-api", &url)?;
            let arr = v
                .as_array()
                .ok_or_else(|| Error::data(&url, "expected an array"))?;
            for c in arr {
                if let Some(d) = c.get("codeDepartement").and_then(|d| d.as_str()) {
                    out.insert(d.to_owned());
                }
            }
        }
    }
    Ok(out)
}

fn props(id: String, kind: ZoneKind, name: String) -> ZoneProps {
    ZoneProps {
        id,
        kind,
        name,
        source: "geo-api".into(),
        url: None,
        months: None,
        practices: vec![],
        designated: None,
        area_ha: 0.0,
        clipped: true,
        note: None,
    }
}

/// Communes, départements and French part of the box.
pub fn french(dl: &Downloader, b: BBox) -> Result<French> {
    let depts = departments_in_bbox(dl, b)?;
    tracing::info!(?depts, "départements in the box");
    let rect: geo::Rect<f64> = b.into();
    let mut communes = Vec::new();
    let mut departments = Vec::new();
    for d in &depts {
        let url = format!("{API}/departements/{d}/communes?format=geojson&geometry=contour");
        let fc = fetch_fc(dl, "geo-api", &url)?;
        let mut parts = Vec::new();
        for f in fc.features {
            let (p, g) = feature_parts(f, &url)?;
            if !g.bounding_rect().is_some_and(|r| rects_overlap(r, rect)) {
                continue;
            }
            let Some(c) = clip_to_bbox(&g, b) else {
                continue;
            };
            let code =
                prop_str(&p, "code").ok_or_else(|| Error::data(&url, "commune without code"))?;
            let name = prop_str(&p, "nom").unwrap_or_else(|| code.clone());
            parts.push(c.geometry.clone());
            let mut zp = props(format!("commune:{code}"), ZoneKind::Commune, name);
            zp.clipped = c.clipped;
            communes.push(RawZone {
                props: zp,
                geometry: c.geometry,
            });
        }
        if parts.is_empty() {
            continue;
        }
        departments.push(RawZone {
            props: props(
                format!("dept:{d}"),
                ZoneKind::Department,
                format!("Département {d}"),
            ),
            geometry: union_all(parts.iter()),
        });
    }
    if departments.is_empty() {
        return Err(Error::data(
            "geo.api.gouv.fr",
            "no French commune in the site box",
        ));
    }
    let country = RawZone {
        props: props("country:fr".into(), ZoneKind::Country, "France".into()),
        geometry: union_all(departments.iter().map(|d| &d.geometry)),
    };
    Ok(French {
        communes,
        departments,
        country,
    })
}

/// French part of the box (WGS84), the analysed area of the coverage layer (P12).
pub fn france_part(dl: &Downloader, b: BBox) -> Result<MultiPolygon<f64>> {
    Ok(french(dl, b)?.country.geometry)
}
