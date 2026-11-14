//! Minimal GeoPackage reader (SQLite through `rusqlite`, no GDAL): one feature table,
//! spatial filter through the `gpkg_rtree_index` extension when present, otherwise
//! through the envelopes of the geometry headers.

use std::path::{Path, PathBuf};

use geo::{MultiPolygon, Rect};
use rusqlite::{Connection, OpenFlags, params};

use crate::error::{Error, Result};
use crate::wkb::{parse_wkb, split_gpkg};

/// An open GeoPackage with a single polygon feature table.
pub struct GeoPackage {
    conn: Connection,
    path: PathBuf,
    table: String,
    geom: String,
    pk: String,
    rtree: Option<String>,
    epsg: Option<u32>,
}

fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn overlaps(a: &Rect<f64>, b: &Rect<f64>) -> bool {
    a.min().x <= b.max().x
        && b.min().x <= a.max().x
        && a.min().y <= b.max().y
        && b.min().y <= a.max().y
}

impl GeoPackage {
    /// Opens read-only and reads the GeoPackage metadata.
    pub fn open(path: &Path) -> Result<Self> {
        let e = |err: rusqlite::Error| Error::gpkg(path, err);
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(e)?;
        let mut stmt = conn
            .prepare("SELECT table_name, column_name, srs_id FROM gpkg_geometry_columns")
            .map_err(e)?;
        let cols: Vec<(String, String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(e)?
            .collect::<std::result::Result<_, _>>()
            .map_err(e)?;
        drop(stmt);
        let [(table, geom, srs_id)] = <[_; 1]>::try_from(cols).map_err(|c| {
            Error::gpkg(
                path,
                format!("expected one feature table, found {}", c.len()),
            )
        })?;
        let (org, org_id, def): (Option<String>, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT organization, organization_coordsys_id, definition \
                 FROM gpkg_spatial_ref_sys WHERE srs_id = ?1",
                params![srs_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(e)?;
        let epsg = if org
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("EPSG"))
        {
            org_id.and_then(|i| u32::try_from(i).ok())
        } else {
            // ARCEP files use a private srs_id whose WKT ends with the EPSG authority.
            def.as_deref().and_then(|d| {
                let tail = d.rsplit("AUTHORITY[").next()?;
                let code = tail
                    .split(',')
                    .nth(1)?
                    .trim_matches(|c: char| !c.is_ascii_digit());
                code.parse().ok()
            })
        };
        let pk: String = conn
            .query_row(
                &format!(
                    "SELECT name FROM pragma_table_info({}) WHERE pk = 1",
                    quote_literal(&table)
                ),
                [],
                |r| r.get(0),
            )
            .map_err(e)?;
        let has_rtree: bool = conn
            .query_row(
                "SELECT count(*) FROM gpkg_extensions WHERE table_name = ?1 \
                 AND column_name = ?2 AND extension_name = 'gpkg_rtree_index'",
                params![table, geom],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false);
        let rtree = has_rtree.then(|| format!("rtree_{table}_{geom}"));
        Ok(Self {
            conn,
            path: path.to_owned(),
            table,
            geom,
            pk,
            rtree,
            epsg,
        })
    }

    /// EPSG code of the geometry column, when it can be read.
    pub fn epsg(&self) -> Option<u32> {
        self.epsg
    }

    /// Feature table name.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Whether the R-tree spatial index is used.
    pub fn has_rtree(&self) -> bool {
        self.rtree.is_some()
    }

    /// Features whose envelope intersects `rect`, with the text attribute `attr`.
    pub fn features_in(
        &self,
        rect: Rect<f64>,
        attr: &str,
    ) -> Result<Vec<(String, MultiPolygon<f64>)>> {
        let e = |err: rusqlite::Error| Error::gpkg(&self.path, err);
        let (t, g, pk, a) = (
            quote(&self.table),
            quote(&self.geom),
            quote(&self.pk),
            quote(attr),
        );
        let mut stmt;
        let mut rows = match &self.rtree {
            Some(rt) => {
                stmt = self
                    .conn
                    .prepare(&format!(
                        "SELECT t.{a}, t.{g} FROM {t} t JOIN {} r ON t.{pk} = r.id \
                         WHERE r.maxx >= ?1 AND r.minx <= ?2 AND r.maxy >= ?3 AND r.miny <= ?4",
                        quote(rt)
                    ))
                    .map_err(e)?;
                stmt.query(params![
                    rect.min().x,
                    rect.max().x,
                    rect.min().y,
                    rect.max().y
                ])
                .map_err(e)?
            }
            None => {
                stmt = self
                    .conn
                    .prepare(&format!("SELECT {a}, {g} FROM {t}"))
                    .map_err(e)?;
                stmt.query([]).map_err(e)?
            }
        };
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(e)? {
            let value: Option<String> = row.get(0).map_err(e)?;
            let blob: Option<Vec<u8>> = row.get(1).map_err(e)?;
            let Some(blob) = blob else { continue };
            let (env, body) = split_gpkg(&blob)?;
            if self.rtree.is_none() && env.is_some_and(|env| !overlaps(&env, &rect)) {
                continue;
            }
            let mp = parse_wkb(body)?;
            out.push((value.unwrap_or_default(), mp));
        }
        Ok(out)
    }
}

fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wkb::tests::{gpkg_blob, wkb_polygon};
    use geo::coord;

    /// Writes a tiny GeoPackage with the ARCEP layout (private srs_id, EPSG in the WKT).
    pub(crate) fn make_gpkg(path: &Path, features: &[(&str, [(f64, f64); 5])], rtree: bool) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            r#"CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT, srs_id INTEGER PRIMARY KEY, organization TEXT, organization_coordsys_id INTEGER, definition TEXT, description TEXT);
               INSERT INTO gpkg_spatial_ref_sys VALUES ('RGF93 / Lambert-93', 100000, 'NONE', 100000, 'PROJCS["RGF93 / Lambert-93",GEOGCS["RGF93",AUTHORITY["EPSG","4171"]],AUTHORITY["EPSG","2154"]]', NULL);
               CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT, srs_id INTEGER, z INTEGER, m INTEGER);
               INSERT INTO gpkg_geometry_columns VALUES ('2026_T2_couv', 'geom', 'GEOMETRY', 100000, 0, 0);
               CREATE TABLE gpkg_extensions (table_name TEXT, column_name TEXT, extension_name TEXT, definition TEXT, scope TEXT);
               CREATE TABLE "2026_T2_couv" (fid INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, geom BLOB, niveau TEXT);"#,
        )
        .unwrap();
        if rtree {
            c.execute_batch(
                r#"INSERT INTO gpkg_extensions VALUES ('2026_T2_couv', 'geom', 'gpkg_rtree_index', '', 'write-only');
                   CREATE VIRTUAL TABLE "rtree_2026_T2_couv_geom" USING rtree(id, minx, maxx, miny, maxy);"#,
            )
            .unwrap();
        }
        for (i, (level, ring)) in features.iter().enumerate() {
            let xs = ring.iter().map(|p| p.0);
            let ys = ring.iter().map(|p| p.1);
            let env = (
                xs.clone().fold(f64::INFINITY, f64::min),
                xs.fold(f64::NEG_INFINITY, f64::max),
                ys.clone().fold(f64::INFINITY, f64::min),
                ys.fold(f64::NEG_INFINITY, f64::max),
            );
            let blob = gpkg_blob(&wkb_polygon(ring, None, false, true), env);
            c.execute(
                r#"INSERT INTO "2026_T2_couv" (fid, geom, niveau) VALUES (?1, ?2, ?3)"#,
                params![i as i64 + 1, blob, level],
            )
            .unwrap();
            if rtree {
                c.execute(
                    r#"INSERT INTO "rtree_2026_T2_couv_geom" VALUES (?1, ?2, ?3, ?4, ?5)"#,
                    params![i as i64 + 1, env.0, env.1, env.2, env.3],
                )
                .unwrap();
            }
        }
    }

    pub(crate) fn square(x: f64, y: f64, d: f64) -> [(f64, f64); 5] {
        [(x, y), (x + d, y), (x + d, y + d), (x, y + d), (x, y)]
    }

    #[test]
    fn spatial_filter_with_and_without_rtree() {
        let dir = tempfile::tempdir().unwrap();
        for rtree in [true, false] {
            let p = dir.path().join(format!("t{rtree}.gpkg"));
            make_gpkg(
                &p,
                &[
                    ("TBC", square(0.0, 0.0, 10.0)),
                    ("BC", square(100.0, 100.0, 10.0)),
                    ("CL", square(5.0, 5.0, 10.0)),
                ],
                rtree,
            );
            let g = GeoPackage::open(&p).unwrap();
            assert_eq!(g.epsg(), Some(2154));
            assert_eq!(g.has_rtree(), rtree);
            let rect = Rect::new(coord! { x: 8.0, y: 8.0 }, coord! { x: 12.0, y: 12.0 });
            let mut levels: Vec<String> = g
                .features_in(rect, "niveau")
                .unwrap()
                .into_iter()
                .map(|f| f.0)
                .collect();
            levels.sort();
            assert_eq!(levels, ["CL", "TBC"]);
        }
    }
}
