//! Plan IGN v2 vector basemap (P4): TMS `PLAN.IGN` tiles of the site box, packed into a
//! PMTiles archive, with the IGN `standard.json` style, its sprite and its glyphs.
//!
//! Licence: Etalab 2.0 (IGN decision 2021-295, report 03). The CGU of cartes.gouv.fr
//! limit the vector TMS to 400 requests/s per IP; the shared downloader caps the `ign`
//! source at 20/s and keeps tiles 30 days. Offline storage is neither forbidden nor
//! expressly allowed by the CGU (ticket L2, #7, still open): personal use.
//!
//! The Licence Ouverte asks for the source and the date of its last update: the
//! attribution carries the revision date of the IGN catalogue record `IGNF_PLAN-IGN`
//! ([`CSW_URL`], [`revision_date`]) and the dates the tiles were downloaded (cache).

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
/// Catalogue record of Plan IGN (ISO 19139), whose citation dates the data.
pub const CSW_URL: &str = "https://data.geopf.fr/csw?service=CSW&version=2.0.2&request=GetRecordById&id=IGNF_PLAN-IGN&outputSchema=http://www.isotc211.org/2005/gmd&elementSetName=full";
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

/// Attempts on a 404: the TMS answered 404 once for tile 16/33983/23609 on
/// 14/11/2026 and 200 to the three following identical requests (the IGN WMTS does the
/// same, see `skimap_terrain::ign_slopes`), so a 404 is retried before failing.
const NOT_FOUND_ATTEMPTS: u32 = 4;

/// MVT bytes of one tile (cache, then network); gzip bodies are inflated.
pub fn fetch_tile(dl: &Downloader, z: u8, x: u32, y: u32) -> Result<Vec<u8>> {
    Ok(fetch_tile_dated(dl, z, x, y)?.0)
}

/// [`fetch_tile`], with the time the body was downloaded (Unix seconds).
fn fetch_tile_dated(dl: &Downloader, z: u8, x: u32, y: u32) -> Result<(Vec<u8>, u64)> {
    let url = tile_url(z, x, y);
    let mut attempt = 0;
    let (body, fetched_at) = loop {
        match dl.fetch(&Request::get(SOURCE, &url)) {
            Ok(f) => break (f.bytes()?, f.meta.fetched_at_unix),
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
        Ok((out, fetched_at))
    } else {
        Ok((body, fetched_at))
    }
}

/// Every tile of the box for `minzoom..=maxzoom`, `(z, x, y, mvt)`; empty tiles skipped.
pub fn fetch_tiles(dl: &Downloader, bbox: BBox, minzoom: u8, maxzoom: u8) -> Result<Vec<TileData>> {
    Ok(fetch_tiles_dated(dl, bbox, minzoom, maxzoom)?.0)
}

/// [`fetch_tiles`], with the oldest and newest download times of the tiles (Unix s).
fn fetch_tiles_dated(
    dl: &Downloader,
    bbox: BBox,
    minzoom: u8,
    maxzoom: u8,
) -> Result<(Vec<TileData>, (u64, u64))> {
    let list = tiles_covering(bbox, minzoom, maxzoom);
    // Tiles fetched on several threads (the downloader bounds the requests in flight).
    let fetched = skimap_common::download::fetch_all(
        &list,
        skimap_common::download::FETCH_THREADS,
        |&(z, x, y)| fetch_tile_dated(dl, z, x, y),
    )?;
    tracing::info!(total = list.len(), "Plan IGN tiles");
    let mut out = Vec::with_capacity(list.len());
    let mut span = (u64::MAX, 0);
    for ((z, x, y), (t, at)) in list.iter().copied().zip(fetched) {
        span = (span.0.min(at), span.1.max(at));
        if !t.is_empty() {
            out.push(((z, x, y), t));
        }
    }
    Ok((out, span))
}

/// Date (`YYYY-MM-DD`) of the data in an ISO 19139 record: in the citation of the first
/// `identificationInfo`, the `revision` date, else `publication`, else `creation`.
pub fn revision_date(xml: &str) -> Option<String> {
    let ident = &xml[xml.find("identificationInfo")?..];
    let citation = &ident[..ident.find("</gmd:CI_Citation>")?];
    let mut dates: Vec<(&str, String)> = Vec::new();
    for block in citation.split("<gmd:CI_Date>").skip(1) {
        let block = block.split("</gmd:CI_Date>").next().unwrap_or(block);
        let value = ["<gco:Date>", "<gco:DateTime>"].iter().find_map(|tag| {
            let v = &block[block.find(tag)? + tag.len()..];
            let v = v.get(..10)?;
            chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").ok()?;
            Some(v.to_owned())
        });
        let kind = ["revision", "publication", "creation"]
            .into_iter()
            .find(|k| block.contains(&format!("codeListValue=\"{k}\"")));
        if let (Some(v), Some(k)) = (value, kind) {
            dates.push((k, v));
        }
    }
    ["revision", "publication", "creation"]
        .iter()
        .find_map(|k| {
            dates
                .iter()
                .filter(|(d, _)| d == k)
                .map(|(_, v)| v.clone())
                .max()
        })
}

/// `YYYY-MM-DD` (UTC) of a Unix time.
fn day(unix: u64) -> String {
    chrono::DateTime::from_timestamp(i64::try_from(unix).unwrap_or(i64::MAX), 0)
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// Attribution of the basemap: source, date of the last update of the data, and when the
/// tiles were downloaded (one day or a range).
pub fn attribution(revision: &str, fetched: (u64, u64)) -> String {
    let (a, b) = (day(fetched.0), day(fetched.1));
    let when = if a == b {
        format!("le {a}")
    } else {
        format!("du {a} au {b}")
    };
    format!(
        "© IGN – Plan IGN v2, mise à jour du {revision} (fiche IGNF_PLAN-IGN), tuiles téléchargées {when}, Licence Ouverte Etalab 2.0"
    )
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
    let record = dl.fetch(&Request::get(SOURCE, CSW_URL))?.text()?;
    let revision = revision_date(&record).ok_or_else(|| {
        Error::Pack("IGN catalogue record IGNF_PLAN-IGN without citation date".into())
    })?;
    let (tiles, fetched) = fetch_tiles_dated(dl, bbox, MINZOOM, maxzoom)?;
    let attribution = attribution(&revision, fetched);
    let metadata = json!({
        "name": "Plan IGN v2",
        "attribution": attribution,
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
        data_date: Some(revision.clone()),
        sources: vec![{
            let mut s = source(
                "Plan IGN v2, tuiles vectorielles (TMS PLAN.IGN) et style standard",
                TMS_URL,
                "Licence Ouverte 2.0 (Etalab), décision IGN 2021-295 ; stockage hors ligne : CGU muettes (ticket L2, #7)",
                &attribution,
            );
            s.edition = Some(revision.clone());
            s
        }],
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

    /// Excerpt of the record `IGNF_PLAN-IGN` fetched on 15/11/2026 (comments removed):
    /// the metadata date stamp comes before, and must not be taken.
    const RECORD: &str = r#"<gmd:MD_Metadata>
    <gmd:dateStamp><gco:DateTime>2026-10-08T15:26:30.739907Z</gco:DateTime></gmd:dateStamp>
    <gmd:identificationInfo>
      <gmd:MD_DataIdentification>
        <gmd:citation>
          <gmd:CI_Citation>
            <gmd:title><gco:CharacterString>Plan IGN</gco:CharacterString></gmd:title>
            <gmd:date>
              <gmd:CI_Date>
                <gmd:date><gco:Date>2024-12-01</gco:Date></gmd:date>
                <gmd:dateType>
                  <gmd:CI_DateTypeCode codeList="http://standards.iso.org/iso/19139/resources/gmxCodelists.xml#CI_DateTypeCode" codeListValue="publication" />
                </gmd:dateType>
              </gmd:CI_Date>
            </gmd:date>
            <gmd:date>
              <gmd:CI_Date>
                <gmd:date><gco:Date>2026-10-08</gco:Date></gmd:date>
                <gmd:dateType>
                  <gmd:CI_DateTypeCode codeList="http://standards.iso.org/iso/19139/resources/gmxCodelists.xml#CI_DateTypeCode" codeListValue="revision" />
                </gmd:dateType>
              </gmd:CI_Date>
            </gmd:date>
          </gmd:CI_Citation>
        </gmd:citation>
        <gmd:extent><gmd:CI_Date><gmd:date><gco:Date>2030-01-01</gco:Date></gmd:date>
          <gmd:CI_DateTypeCode codeListValue="revision" /></gmd:CI_Date></gmd:extent>
      </gmd:MD_DataIdentification>
    </gmd:identificationInfo>
</gmd:MD_Metadata>"#;

    #[test]
    fn data_date_from_the_catalogue_record() {
        assert_eq!(revision_date(RECORD).as_deref(), Some("2026-10-08"));
        // Without a revision date: the publication date.
        let no_rev = RECORD.replacen("codeListValue=\"revision\"", "codeListValue=\"x\"", 1);
        assert_eq!(revision_date(&no_rev).as_deref(), Some("2024-12-01"));
        assert_eq!(revision_date("<gmd:MD_Metadata/>"), None);
        // 2026-11-14 10:00 and 2026-11-15 09:00 UTC.
        let a = attribution("2026-10-08", (1_794_650_400, 1_794_733_200));
        assert_eq!(
            a,
            "© IGN – Plan IGN v2, mise à jour du 2026-10-08 (fiche IGNF_PLAN-IGN), tuiles téléchargées du 2026-11-14 au 2026-11-15, Licence Ouverte Etalab 2.0"
        );
        assert!(
            attribution("2026-10-08", (1_794_650_400, 1_794_650_500)).contains("le 2026-11-14,")
        );
    }

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
