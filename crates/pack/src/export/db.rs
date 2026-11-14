//! Raster overlays as SQLite tile databases: MBTiles 1.3 (Locus Map) and the OsmAnd
//! SQLiteDB format (`tiles(x, y, z, s, image)`, `info` table, simple numbering).

use std::path::Path;

use rusqlite::{Connection, params};

use crate::error::{Error, Result};

/// A tile `(z, x, y, png)` (XYZ rows).
pub type DbTile = (u8, u32, u32, Vec<u8>);

/// Description of a raster overlay.
#[derive(Debug, Clone)]
pub struct Overlay<'a> {
    /// Name shown by the app.
    pub name: &'a str,
    /// Description.
    pub description: &'a str,
    /// Attribution.
    pub attribution: &'a str,
    /// Bounds `[west, south, east, north]`.
    pub bounds: [f64; 4],
    /// Lowest zoom.
    pub minzoom: u8,
    /// Highest zoom.
    pub maxzoom: u8,
}

fn sql(path: &Path) -> impl Fn(rusqlite::Error) -> Error + '_ {
    move |source| Error::Sqlite {
        path: path.to_path_buf(),
        source,
    }
}

fn fresh(path: &Path) -> Result<Connection> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| Error::io(path, e))?;
    }
    if let Some(p) = path.parent() {
        crate::build::create_dir(p)?;
    }
    Connection::open(path).map_err(sql(path))
}

/// Writes an MBTiles 1.3 file (TMS rows: `tile_row = 2^z - 1 - y`). `tiles`: `(z, x, y, png)`.
pub fn write_mbtiles(path: &Path, o: &Overlay<'_>, tiles: &[DbTile]) -> Result<u64> {
    let mut c = fresh(path)?;
    let e = sql(path);
    c.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE UNIQUE INDEX name ON metadata (name);
         CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);
         CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);",
    )
    .map_err(&e)?;
    let tx = c.transaction().map_err(&e)?;
    let [w, s, ea, n] = o.bounds;
    let center_z = o.maxzoom.min(13).max(o.minzoom);
    let meta = [
        ("name", o.name.to_owned()),
        ("format", "png".to_owned()),
        ("type", "overlay".to_owned()),
        ("version", "1".to_owned()),
        ("description", o.description.to_owned()),
        ("attribution", o.attribution.to_owned()),
        ("bounds", format!("{w},{s},{ea},{n}")),
        (
            "center",
            format!("{},{},{center_z}", (w + ea) / 2.0, (s + n) / 2.0),
        ),
        ("minzoom", o.minzoom.to_string()),
        ("maxzoom", o.maxzoom.to_string()),
        ("scheme", "tms".to_owned()),
    ];
    for (k, v) in meta {
        tx.execute(
            "INSERT INTO metadata (name, value) VALUES (?1, ?2)",
            params![k, v],
        )
        .map_err(&e)?;
    }
    {
        let mut st = tx
            .prepare("INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)")
            .map_err(&e)?;
        for (z, x, y, png) in tiles {
            let row = (1u32 << z) - 1 - y;
            st.execute(params![z, x, row, png]).map_err(&e)?;
        }
    }
    tx.commit().map_err(&e)?;
    drop(c);
    Ok(std::fs::metadata(path)
        .map_err(|e| Error::io(path, e))?
        .len())
}

/// Writes an OsmAnd `.sqlitedb` overlay: simple (non-inverted) zoom numbering, XYZ rows,
/// no download URL (offline only), no time column.
pub fn write_sqlitedb(path: &Path, o: &Overlay<'_>, tiles: &[DbTile]) -> Result<u64> {
    let mut c = fresh(path)?;
    let e = sql(path);
    c.execute_batch(
        "CREATE TABLE tiles (x INT, y INT, z INT, s INT, image BLOB, PRIMARY KEY (x, y, z, s));
         CREATE TABLE info (title TEXT, minzoom INT, maxzoom INT, tilenumbering TEXT, timecolumn TEXT,
                            expireminutes INT, ellipsoid INT, inverted_y INT, tilesize INT);
         CREATE TABLE android_metadata (locale TEXT);
         INSERT INTO android_metadata VALUES ('fr_FR');",
    )
    .map_err(&e)?;
    let tx = c.transaction().map_err(&e)?;
    tx.execute(
        "INSERT INTO info (title, minzoom, maxzoom, tilenumbering, timecolumn, expireminutes, ellipsoid, inverted_y, tilesize)
         VALUES (?1, ?2, ?3, 'simple', 'no', 0, 0, 0, 256)",
        params![o.name, o.minzoom, o.maxzoom],
    )
    .map_err(&e)?;
    {
        let mut st = tx
            .prepare("INSERT INTO tiles (x, y, z, s, image) VALUES (?1, ?2, ?3, 0, ?4)")
            .map_err(&e)?;
        for (z, x, y, png) in tiles {
            st.execute(params![x, y, z, png]).map_err(&e)?;
        }
    }
    tx.commit().map_err(&e)?;
    drop(c);
    Ok(std::fs::metadata(path)
        .map_err(|e| Error::io(path, e))?
        .len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_formats_hold_the_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let o = Overlay {
            name: "Pentes",
            description: "d",
            attribution: "a",
            bounds: [6.0, 44.0, 7.0, 45.0],
            minzoom: 10,
            maxzoom: 11,
        };
        let tiles = vec![(10, 530, 370, vec![1, 2, 3]), (11, 1061, 741, vec![4])];
        let m = dir.path().join("t.mbtiles");
        write_mbtiles(&m, &o, &tiles).unwrap();
        let c = Connection::open(&m).unwrap();
        let row: u32 = c
            .query_row(
                "SELECT tile_row FROM tiles WHERE zoom_level = 10",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(row, 1023 - 370);
        let maxz: String = c
            .query_row(
                "SELECT value FROM metadata WHERE name = 'maxzoom'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(maxz, "11");
        let s = dir.path().join("t.sqlitedb");
        write_sqlitedb(&s, &o, &tiles).unwrap();
        let c = Connection::open(&s).unwrap();
        let (y, img): (u32, Vec<u8>) = c
            .query_row("SELECT y, image FROM tiles WHERE z = 11", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((y, img), (741, vec![4]));
        let num: String = c
            .query_row("SELECT tilenumbering FROM info", [], |r| r.get(0))
            .unwrap();
        assert_eq!(num, "simple");
    }
}
