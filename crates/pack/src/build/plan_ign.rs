//! Plan IGN v2 vector basemap (P4): TMS `PLAN.IGN` tiles of the site box, packed into a
//! PMTiles archive, with the IGN `standard.json` style, its sprite and its glyphs.
//!
//! Licence: Etalab 2.0 (IGN decision 2021-295, report 03). The CGU of cartes.gouv.fr
//! limit the vector TMS to 400 requests/s per IP; the shared downloader caps the `ign`
//! source at 20/s and keeps tiles 30 days. Offline storage is neither forbidden nor
//! expressly allowed by the CGU (ticket L2, #7, still open): personal use.

use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use serde_json::{Value, json};
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::BBox;

use crate::build::{assets, dir_size, source, write_json};
use crate::error::{Error, Result};
use crate::manifest::Basemap;
use crate::tiles::{TileData, tiles_covering};

/// Downloader source.
pub const SOURCE: &str = "ign";
/// Vector tiles (XYZ scheme).
pub const TMS_URL: &str = "https://data.geopf.fr/tms/1.0.0/PLAN.IGN/{z}/{x}/{y}.pbf";
/// TileJSON of the service (vector layers).
pub const METADATA_URL: &str = "https://data.geopf.fr/tms/1.0.0/PLAN.IGN/metadata.json";
/// Standard style.
pub const STYLE_URL: &str =
    "https://data.geopf.fr/annexes/ressources/vectorTiles/styles/PLAN.IGN/standard.json";
/// Identifier of the basemap in the pack.
pub const ID: &str = "plan-ign";
/// Lowest zoom stored: the whole site fits in one or two tiles at zoom 0 to 9.
pub const MINZOOM: u8 = 0;

/// URL of one tile.
pub fn tile_url(z: u8, x: u32, y: u32) -> String {
    TMS_URL
        .replace("{z}", &z.to_string())
        .replace("{x}", &x.to_string())
        .replace("{y}", &y.to_string())
}

/// Attempts on a 404: the TMS answered 404 once for tile 16/33983/23609 (Cervières) on
/// 14/11/2026 and 200 to the three following identical requests (the IGN WMTS does the
/// same, see `skimap_terrain::ign_slopes`), so a 404 is retried before failing.
const NOT_FOUND_ATTEMPTS: u32 = 4;

/// MVT bytes of one tile (cache, then network); gzip bodies are inflated.
pub fn fetch_tile(dl: &Downloader, z: u8, x: u32, y: u32) -> Result<Vec<u8>> {
    let url = tile_url(z, x, y);
    let mut attempt = 0;
    let body = loop {
        match dl.fetch(&Request::get(SOURCE, &url)) {
            Ok(f) => break f.bytes()?,
            Err(skimap_common::Error::Status { status: 404, .. })
                if attempt + 1 < NOT_FOUND_ATTEMPTS =>
            {
                tracing::warn!(%url, attempt, "TMS 404, retrying");
                std::thread::sleep(std::time::Duration::from_secs(1 << attempt));
                attempt += 1;
            }
            Err(e) => return Err(e.into()),
        }
    };
    if body.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        GzDecoder::new(&body[..])
            .read_to_end(&mut out)
            .map_err(|e| Error::Pack(format!("tile {z}/{x}/{y}: gzip: {e}")))?;
        Ok(out)
    } else {
        Ok(body)
    }
}

/// Every tile of the box for `minzoom..=maxzoom`, `(z, x, y, mvt)`; empty tiles skipped.
pub fn fetch_tiles(dl: &Downloader, bbox: BBox, minzoom: u8, maxzoom: u8) -> Result<Vec<TileData>> {
    let list = tiles_covering(bbox, minzoom, maxzoom);
    let mut out = Vec::with_capacity(list.len());
    for (i, (z, x, y)) in list.iter().copied().enumerate() {
        let t = fetch_tile(dl, z, x, y)?;
        if !t.is_empty() {
            out.push(((z, x, y), t));
        }
        if (i + 1) % 500 == 0 {
            tracing::info!(done = i + 1, total = list.len(), "Plan IGN tiles");
        }
    }
    Ok(out)
}

/// Writes MVT tiles to a PMTiles archive (gzip tiles, sorted by tile id). Returns the
/// archive size.
pub fn write_pmtiles(
    path: &Path,
    tiles: &[TileData],
    metadata: &Value,
    bbox: BBox,
    minzoom: u8,
    maxzoom: u8,
) -> Result<u64> {
    crate::build::raster::write_archive(
        path,
        pmtiles::TileType::Mvt,
        None,
        tiles,
        metadata,
        bbox,
        minzoom,
        maxzoom,
    )
}

/// Rewrites the IGN style for the pack: tiles from the local archive, local sprite and
/// glyphs; URLs relative to the pack root.
pub fn rewrite_style(mut style: Value) -> Result<Value> {
    let obj = style
        .as_object_mut()
        .ok_or_else(|| Error::Pack("IGN style is not an object".into()))?;
    obj.insert("sprite".into(), json!("basemap/plan-ign/sprite/PlanIgn"));
    obj.insert(
        "glyphs".into(),
        json!("basemap/plan-ign/fonts/{fontstack}/{range}.pbf"),
    );
    obj.insert(
        "sources".into(),
        json!({"plan_ign": {
            "type": "vector",
            "url": "pmtiles://basemap/plan-ign.pmtiles",
            "attribution": "<a href=\"https://www.ign.fr\">© IGN</a> Plan IGN v2"
        }}),
    );
    let meta = obj
        .entry("metadata")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::Pack("IGN style metadata is not an object".into()))?;
    meta.insert("skimap:relative_urls".into(), json!(true));
    meta.insert("skimap:origin".into(), json!(STYLE_URL));
    Ok(style)
}

/// Builds the Plan IGN basemap into `root/basemap/`.
pub fn build(bbox: BBox, dl: &Downloader, root: &Path, maxzoom: u8) -> Result<Basemap> {
    let style_raw: Value = dl.fetch(&Request::get(SOURCE, STYLE_URL))?.json()?;
    let sprite_url = style_raw
        .get("sprite")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Pack("IGN style without sprite".into()))?
        .to_owned();
    let glyphs_url = style_raw
        .get("glyphs")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Pack("IGN style without glyphs".into()))?
        .to_owned();
    let stacks = assets::fontstacks(&style_raw);
    let style = rewrite_style(style_raw)?;
    let dir = root.join("basemap").join(ID);
    write_json(&dir.join("style.json"), &style)?;
    assets::fetch_sprite(dl, SOURCE, &sprite_url, &dir.join("sprite"), "PlanIgn")?;
    assets::fetch_glyphs(dl, SOURCE, &glyphs_url, &stacks, &dir.join("fonts"))?;

    let tilejson: Value = dl.fetch(&Request::get(SOURCE, METADATA_URL))?.json()?;
    let tiles = fetch_tiles(dl, bbox, MINZOOM, maxzoom)?;
    let metadata = json!({
        "name": "Plan IGN v2",
        "attribution": "© IGN Plan IGN v2 (Etalab 2.0)",
        "vector_layers": tilejson.get("vector_layers").cloned().unwrap_or(json!([])),
        "skimap:source": TMS_URL,
    });
    let archive = root.join("basemap").join(format!("{ID}.pmtiles"));
    let archive_bytes = write_pmtiles(&archive, &tiles, &metadata, bbox, MINZOOM, maxzoom)?;
    let bytes = archive_bytes + dir_size(&dir)?;
    Ok(Basemap {
        id: ID.to_owned(),
        title: "Plan IGN v2".to_owned(),
        tiles: format!("basemap/{ID}.pmtiles"),
        style: format!("basemap/{ID}/style.json"),
        minzoom: MINZOOM,
        maxzoom,
        tile_count: tiles.len() as u64,
        bytes,
        data_date: Some(chrono::Utc::now().format("%Y-%m-%d").to_string()),
        sources: vec![source(
            "Plan IGN v2, tuiles vectorielles (TMS PLAN.IGN) et style standard",
            TMS_URL,
            "Licence Ouverte 2.0 (Etalab), décision IGN 2021-295 ; stockage hors ligne : CGU muettes (ticket L2, #7)",
            "© IGN – Plan IGN v2",
        )],
        notes: vec![
            format!("Tuiles de l'emprise du site, zooms {MINZOOM} à {maxzoom} ; au-delà la carte agrandit le zoom {maxzoom}."),
            "Couches IGN visibles seulement au zoom 17 ou 18 (bornes kilométriques, parcelles, adresses) absentes.".to_owned(),
            format!("Glyphes : plages {} seulement.", assets::GLYPH_RANGES.join(", ")),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_rewrite_points_to_the_pack() {
        let s = json!({"version": 8, "sprite": "https://x/sprite", "glyphs": "https://x/{fontstack}/{range}.pbf",
                       "sources": {"plan_ign": {"type": "vector", "tiles": ["https://x/{z}/{x}/{y}.pbf"]}},
                       "layers": []});
        let r = rewrite_style(s).unwrap();
        assert_eq!(
            r["sources"]["plan_ign"]["url"],
            "pmtiles://basemap/plan-ign.pmtiles"
        );
        assert!(r["sources"]["plan_ign"].get("tiles").is_none());
        assert_eq!(r["sprite"], "basemap/plan-ign/sprite/PlanIgn");
        assert_eq!(r["metadata"]["skimap:relative_urls"], true);
        assert_eq!(
            tile_url(15, 1, 2),
            "https://data.geopf.fr/tms/1.0.0/PLAN.IGN/15/1/2.pbf"
        );
    }
}
