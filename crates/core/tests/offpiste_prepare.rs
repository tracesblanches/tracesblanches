//! C3 pipeline side (`offpiste::prepare`) on a synthetic site small enough to check every
//! cell (Q4, #104: the preparation had no test of its outputs; `cargo mutants` left 396
//! of its mutants alive): quantised elevation and slope, missing terrain, zone classes
//! against `ZoneLayer::containing` (holes and overlaps included), piste rasterisation
//! (groomed over marked at crossings), avalanche exposure resampled to the 5 m grid, snow
//! classes, statistics and files of `offpiste.meta.json`.

#![cfg(feature = "pipeline")]

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;
use skimap_common::crs::{Crs, from_lambert93};
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_common::layout::DataLayout;
use skimap_common::meta::LayerMeta;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
use skimap_core::offpiste::{
    self, ELEVATION_NODATA, OffpisteGrid, PISTE_GROOMED, PISTE_MARKED, PISTE_NONE, SLOPE_NODATA,
    SnowGrid,
};
use skimap_zones::geo::{LineString, MultiPolygon, Polygon};
use skimap_zones::layer::{Zone, ZoneKind, ZoneLayer, ZoneProps};

const X0: f64 = 998_000.0;
const Y0: f64 = 6_425_000.0;
const W: usize = 80;
const H: usize = 60;
const CELL: f64 = 5.0;

fn centre(c: usize, r: usize) -> (f64, f64) {
    (X0 + (c as f64 + 0.5) * CELL, Y0 - (r as f64 + 0.5) * CELL)
}

fn wgs(x: f64, y: f64) -> LonLat {
    from_lambert93(x, y).unwrap()
}

/// Ring through Lambert-93 corners (cell edges), in WGS84, densified every 5 m so that
/// its edges follow the grid lines.
fn ring(c0: usize, r0: usize, c1: usize, r1: usize) -> LineString<f64> {
    let (xa, ya) = (X0 + c0 as f64 * CELL, Y0 - r0 as f64 * CELL);
    let (xb, yb) = (X0 + c1 as f64 * CELL, Y0 - r1 as f64 * CELL);
    let mut pts = Vec::new();
    let side = |a: (f64, f64), b: (f64, f64), pts: &mut Vec<(f64, f64)>| {
        let n = (((b.0 - a.0).abs() + (b.1 - a.1).abs()) / CELL)
            .ceil()
            .max(1.0) as usize;
        for k in 0..n {
            let t = k as f64 / n as f64;
            let p = wgs(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            pts.push((p.lon, p.lat));
        }
    };
    side((xa, ya), (xb, ya), &mut pts);
    side((xb, ya), (xb, yb), &mut pts);
    side((xb, yb), (xa, yb), &mut pts);
    side((xa, yb), (xa, ya), &mut pts);
    pts.push(pts[0]);
    LineString::from(pts)
}

fn zone(id: &str, poly: Polygon<f64>) -> Zone {
    Zone::new(
        ZoneProps {
            id: id.into(),
            kind: ZoneKind::Appb,
            name: id.into(),
            source: "test".into(),
            url: None,
            months: None,
            practices: vec![],
            designated: None,
            area_ha: 1.0,
            clipped: false,
            note: None,
        },
        MultiPolygon::new(vec![poly]),
    )
}

fn meta(dir: &Path, layer: &str) {
    std::fs::create_dir_all(dir).unwrap();
    LayerMeta::new(layer, CERVIERES.id).write(dir).unwrap();
}

/// Writes the inputs; returns the DEM, slope, zone layer and exposure grid.
fn site(root: &Path) -> (Grid<f32>, Grid<f32>, ZoneLayer, Grid<u8>) {
    let layout = DataLayout::new(root);
    let t = GeoTransform::north_up(X0, Y0, CELL);
    // Terrain: elevation rises 1 m per column, slope 0.5° per column; no terrain in the
    // top-left 5 × 5 block, no slope in one more cell.
    let dem: Vec<f32> = (0..W * H)
        .map(|i| {
            let (c, r) = (i % W, i / W);
            if c < 5 && r < 5 {
                f32::NAN
            } else {
                1500.0 + c as f32 + 0.123
            }
        })
        .collect();
    let slope: Vec<f32> = (0..W * H)
        .map(|i| {
            if i == 50 * W + 70 {
                f32::NAN
            } else {
                (i % W) as f32 * 0.5 + 0.2
            }
        })
        .collect();
    let dem = Grid::from_vec(W, H, t, Crs::Lambert93, dem).unwrap();
    let slope = Grid::from_vec(W, H, t, Crs::Lambert93, slope).unwrap();
    let terrain = layout.layer_dir(&CERVIERES, "terrain").unwrap();
    meta(&terrain, "terrain");
    skimap_common::geotiff::write_f32(&terrain.join("dem_5m.tif"), &dem).unwrap();
    skimap_common::geotiff::write_f32(&terrain.join("slope_5m.tif"), &slope).unwrap();
    // Zones: A (cols 10–40), B (cols 30–60, with a hole cols 45–50 × rows 20–30), C
    // outside the grid.
    let layer = ZoneLayer {
        zones: vec![
            zone("a", Polygon::new(ring(10, 5, 40, 55), vec![])),
            zone(
                "b",
                Polygon::new(ring(30, 10, 60, 50), vec![ring(45, 20, 50, 30)]),
            ),
            zone("c", Polygon::new(ring(200, 200, 210, 210), vec![])),
        ],
    };
    let zdir = layout.layer_dir(&CERVIERES, "zones").unwrap();
    meta(&zdir, "zones");
    std::fs::write(
        zdir.join("zones.geojson"),
        layer.to_geojson_string().unwrap(),
    )
    .unwrap();
    // Pistes: a groomed nordic trail along row 10 (cols 5–75), a marked snowshoe trail
    // along column 20 (rows 0–59), crossing at (20, 10).
    let line = |pts: [(usize, usize); 2]| {
        pts.iter()
            .map(|&(c, r)| {
                let (x, y) = centre(c, r);
                let p = wgs(x, y);
                json!([p.lon, p.lat])
            })
            .collect::<Vec<_>>()
    };
    let edges = json!({"type": "FeatureCollection", "features": [
        {"type": "Feature", "geometry": {"type": "LineString", "coordinates": line([(5, 10), (75, 10)])},
         "properties": {"kind": "piste", "uses": ["nordic"], "backcountry": false}},
        {"type": "Feature", "geometry": {"type": "LineString", "coordinates": line([(20, 0), (20, 59)])},
         "properties": {"kind": "piste", "uses": ["snowshoe"]}},
    ]});
    let pdir = layout.layer_dir(&CERVIERES, "pistes").unwrap();
    meta(&pdir, "pistes");
    std::fs::write(pdir.join("graph_edges.geojson"), edges.to_string()).unwrap();
    // Snow: 25 m grid, SRN class = column / 4 (0..3, 3 = unknown → NaN), groomed = row
    // parity.
    let sdir = layout.layer_dir(&CERVIERES, "snow").unwrap();
    let mut m = LayerMeta::new("snow", CERVIERES.id);
    m.valid_from = Some("2027-01-10T06:00:00Z".into());
    std::fs::create_dir_all(&sdir).unwrap();
    m.write(&sdir).unwrap();
    let st = GeoTransform::north_up(X0, Y0, 25.0);
    let (sw, sh) = (W / 5, H / 5);
    let srn: Vec<f32> = (0..sw * sh)
        .map(|i| match (i % sw) / 4 {
            3 => f32::NAN,
            k => k as f32,
        })
        .collect();
    let groomed: Vec<f32> = (0..sw * sh).map(|i| ((i / sw) % 2) as f32).collect();
    for (name, d) in [("class_srn.tif", srn), ("class_groomed.tif", groomed)] {
        let g = Grid::from_vec(sw, sh, st, Crs::Lambert93, d).unwrap();
        skimap_common::geotiff::write_f32(&sdir.join(name), &g).unwrap();
    }
    // Exposure: 10 m cells, classes 1..3 by column band, 255 on the last band.
    let et = GeoTransform::north_up(X0, Y0, 10.0);
    let ex: Vec<u8> = (0..(W / 2) * (H / 2))
        .map(|i| match (i % (W / 2)) / 10 {
            3 => 255,
            k => k as u8 + 1,
        })
        .collect();
    let exposure = Grid::from_vec(W / 2, H / 2, et, Crs::Lambert93, ex).unwrap();
    (dem, slope, layer, exposure)
}

fn ids_at(layer: &ZoneLayer, c: usize, r: usize) -> Vec<String> {
    let (x, y) = centre(c, r);
    let mut v: Vec<String> = layer
        .containing(wgs(x, y))
        .map(|z| z.id().to_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn prepared_rasters_match_their_inputs_cell_by_cell() {
    let tmp = tempfile::tempdir().unwrap();
    let (dem, slope, layer, exposure) = site(tmp.path());
    let layout = DataLayout::new(tmp.path());
    let snow_dir = layout.layer_dir(&CERVIERES, "snow").unwrap();
    let m = offpiste::prepare::prepare_with_exposure(
        &CERVIERES,
        &layout,
        std::slice::from_ref(&snow_dir),
        Some(&exposure),
    )
    .unwrap();
    let out = layout.layer_dir(&CERVIERES, offpiste::LAYER).unwrap();
    let g = OffpisteGrid::load(&out).unwrap();
    assert_eq!((g.width(), g.height(), g.cell_size()), (W, H, CELL));

    // Terrain: decimetres and half degrees, missing where either input is missing.
    let mut missing = 0;
    let mut steep = 0;
    for r in 0..H {
        for c in 0..W {
            let i = r * W + c;
            let (z, s) = (dem.data()[i], slope.data()[i]);
            let (ze, se) = (g.elevation.data()[i], g.slope.data()[i]);
            if z.is_nan() || s.is_nan() {
                missing += 1;
                assert_eq!((ze, se), (ELEVATION_NODATA, SLOPE_NODATA), "({c}, {r})");
                continue;
            }
            assert_eq!(Some(ze), offpiste::elevation_raw(z), "({c}, {r})");
            assert_eq!(se, offpiste::slope_raw(s), "({c}, {r})");
            assert_eq!(se, (2.0 * s).floor() as u8);
            steep += usize::from(se >= 60);
        }
    }
    assert_eq!(missing, 26);
    let st = &m.stats;
    assert_eq!(st["grid"]["size_cells"], json!([W, H]));
    assert_eq!(st["grid"]["cell_size"], CELL);
    assert_eq!(st["grid"]["origin"], json!([X0, Y0]));
    assert_eq!(st["grid"]["cells_without_terrain"], missing);
    assert_eq!(st["grid"]["cells_slope_ge_30"], steep);
    let share = st["grid"]["share_slope_ge_30_of_valid"].as_f64().unwrap();
    assert!((share - steep as f64 / (W * H - missing) as f64).abs() < 1e-12);
    let share = st["grid"]["share_without_terrain"].as_f64().unwrap();
    assert!((share - missing as f64 / (W * H) as f64).abs() < 1e-12);

    // Zones: every cell's class is the set of zones containing its centre.
    let classes = &g.classes;
    let mut seen = BTreeSet::new();
    let mut cells = vec![0u64; classes.len()];
    for r in 0..H {
        for c in 0..W {
            let k = usize::from(g.zones.data()[r * W + c]);
            assert_eq!(classes[k].zone_ids, ids_at(&layer, c, r), "({c}, {r})");
            seen.insert(classes[k].zone_ids.clone());
            cells[k] += 1;
        }
    }
    // Outside (and B's hole), A, A ∩ B, B.
    let want: BTreeSet<Vec<String>> = [vec![], vec!["a"], vec!["a", "b"], vec!["b"]]
        .into_iter()
        .map(|v| v.into_iter().map(String::from).collect())
        .collect();
    assert_eq!(seen, want);
    for (k, cl) in classes.iter().enumerate() {
        assert_eq!(cl.cells, cells[k], "class {k}");
        let rep = cl.representative;
        let mut got: Vec<String> = layer.containing(rep).map(|z| z.id().to_owned()).collect();
        got.sort();
        assert_eq!(got, cl.zone_ids, "representative of class {k}");
    }
    assert_eq!(st["zones"]["zones"], 3);
    assert_eq!(st["zones"]["classes"], classes.len());
    assert_eq!(st["zones"]["check_mismatches"], 0);
    assert_eq!(st["zones"]["check_sample_cells"], 20_000);
    let text = std::fs::read_to_string(out.join(offpiste::FILE_ZONE_CLASSES)).unwrap();
    assert_eq!(offpiste::parse_zone_classes(&text).unwrap(), *classes);
    // Inside the hole of B (and outside A): no zone; around the hole: B.
    assert!(ids_at(&layer, 47, 25).is_empty());
    assert_eq!(ids_at(&layer, 47, 35), vec!["b".to_owned()]);
    assert_eq!(ids_at(&layer, 35, 25), vec!["a".to_owned(), "b".to_owned()]);

    // Pistes: every cell crossed, groomed over marked at the crossing.
    let piste = |c: usize, r: usize| g.pistes.data()[r * W + c];
    for c in 5..=75 {
        assert_eq!(piste(c, 10), PISTE_GROOMED, "row 10, col {c}");
    }
    for r in (0..H).filter(|r| *r != 10) {
        assert_eq!(piste(20, r), PISTE_MARKED, "col 20, row {r}");
    }
    let groomed = g
        .pistes
        .data()
        .iter()
        .filter(|v| **v == PISTE_GROOMED)
        .count();
    let marked = g
        .pistes
        .data()
        .iter()
        .filter(|v| **v == PISTE_MARKED)
        .count();
    assert_eq!((groomed, marked), (71, 59));
    assert_eq!(piste(4, 10), PISTE_NONE);
    assert_eq!(piste(76, 10), PISTE_NONE);
    assert_eq!(st["pistes"]["edges"], 2);
    assert_eq!(st["pistes"]["cells_groomed"], groomed);
    assert_eq!(st["pistes"]["cells_marked"], marked);

    // Exposure: nearest cell of the 10 m grid.
    let e: Grid<u8> = offpiste::read_raster(&out.join(offpiste::FILE_EXPOSURE)).unwrap();
    let mut counts = [0u64; 4];
    for r in 0..H {
        for c in 0..W {
            let v = e.data()[r * W + c];
            assert_eq!(v, exposure.data()[(r / 2) * (W / 2) + c / 2], "({c}, {r})");
            counts[if (1..=3).contains(&v) {
                usize::from(v)
            } else {
                0
            }] += 1;
        }
    }
    let ex = &st["exposure"];
    for (key, n) in ["cells_nodata", "cells_low", "cells_exposed", "cells_high"]
        .iter()
        .zip(counts)
    {
        assert_eq!(ex[key], n, "{key}");
    }
    assert!(counts.iter().all(|n| *n > 0), "{counts:?}");

    // Snow: one file for the date, classes packed.
    let snow = SnowGrid::load(&out.join("snow_2027-01-10.skr")).unwrap();
    for (c, r) in [(0, 0), (5, 2), (12, 7), (15, 11)] {
        let (x, y) = (X0 + (c as f64 + 0.5) * 25.0, Y0 - (r as f64 + 0.5) * 25.0);
        let v = snow.at(x, y);
        let srn = if c / 4 == 3 {
            offpiste::SNOW_UNKNOWN
        } else {
            (c / 4) as u8
        };
        assert_eq!(SnowGrid::srn(v), srn, "({c}, {r})");
        assert_eq!(SnowGrid::groomed(v), (r % 2) as u8, "({c}, {r})");
    }
    let s0 = &st["snow"][0];
    assert_eq!(s0["name"], "snow_2027-01-10.skr");
    assert_eq!(s0["size_cells"], json!([W / 5, H / 5]));
    let k = &s0["srn_cells"];
    assert_eq!(k["insufficient"], 4 * (H / 5));
    assert_eq!(k["unknown"], 4 * (H / 5));
    assert_eq!(m.valid_from.as_deref(), Some("2027-01-10T06:00:00Z"));

    // Files: every listed file exists with its size.
    for f in &m.files {
        let p = out.join(&f.name);
        assert_eq!(
            Some(std::fs::metadata(&p).unwrap().len()),
            f.bytes,
            "{}",
            f.name
        );
        assert!(!f.description.is_empty(), "{}", f.name);
    }
    let names: BTreeSet<&str> = m.files.iter().map(|f| f.name.as_str()).collect();
    for n in [
        offpiste::FILE_ELEVATION,
        offpiste::FILE_SLOPE,
        offpiste::FILE_ZONES,
        offpiste::FILE_ZONE_CLASSES,
        offpiste::FILE_PISTES,
        offpiste::FILE_EXPOSURE,
        "snow_2027-01-10.skr",
    ] {
        assert!(names.contains(n), "{n} not listed");
    }
    assert_eq!(
        LayerMeta::read(&out.join("offpiste.meta.json"))
            .unwrap()
            .stats,
        m.stats
    );

    // Without the exposure layer, a stale exposure raster is removed.
    let m2 = offpiste::prepare::prepare(&CERVIERES, &layout, &[snow_dir]).unwrap();
    assert!(!out.join(offpiste::FILE_EXPOSURE).exists());
    assert!(m2.stats["exposure"].is_null());
    assert_eq!(m2.stats["snow_retention"]["previous_meta"], "lue");
}

#[test]
fn mismatched_terrain_grids_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    site(tmp.path());
    let layout = DataLayout::new(tmp.path());
    let terrain = layout.layer_dir(&CERVIERES, "terrain").unwrap();
    let t = GeoTransform::north_up(X0 + 5.0, Y0, CELL);
    let g = Grid::from_vec(W, H, t, Crs::Lambert93, vec![1.0f32; W * H]).unwrap();
    skimap_common::geotiff::write_f32(&terrain.join("slope_5m.tif"), &g).unwrap();
    let e = offpiste::prepare::prepare(&CERVIERES, &layout, &[]).unwrap_err();
    assert!(e.to_string().contains("differ"), "{e}");
}

#[test]
fn segment_distance_cases() {
    use skimap_core::offpiste::prepare::segment_distance as d;
    // Perpendicular foot inside the segment, before it, after it; degenerate segment.
    assert!((d((1.0, 2.0), (0.0, 0.0), (4.0, 0.0)) - 2.0).abs() < 1e-12);
    assert!((d((-3.0, 4.0), (0.0, 0.0), (4.0, 0.0)) - 5.0).abs() < 1e-12);
    assert!((d((7.0, -4.0), (0.0, 0.0), (4.0, 0.0)) - 5.0).abs() < 1e-12);
    assert!((d((3.0, 4.0), (0.0, 0.0), (0.0, 0.0)) - 5.0).abs() < 1e-12);
    // Oblique segment.
    let s = d((0.0, 2.0), (-1.0, -1.0), (3.0, 3.0));
    assert!((s - 2f64.sqrt()).abs() < 1e-12, "{s}");
    assert!((d((2.0, 1.0), (3.0, 3.0), (-1.0, -1.0)) - 0.5f64.sqrt()).abs() < 1e-12);
}

/// Cells crossed by a segment between two cell-coordinate points, by dense sampling.
fn crossed(a: (f64, f64), b: (f64, f64)) -> BTreeSet<(usize, usize)> {
    let n = 20_000;
    (0..=n)
        .map(|k| {
            let t = f64::from(k) / f64::from(n);
            let (c, r) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            (c.floor() as usize, r.floor() as usize)
        })
        .collect()
}

/// Diagonal pistes in all four directions and a zone with long oblique edges, partly
/// outside the grid: the rasterised pistes are exactly the cells the lines cross, the
/// zone classes still match `ZoneLayer::containing` everywhere, and the cells decided by
/// `containing` are those within 1 m of an edge.
#[test]
fn diagonal_pistes_and_oblique_zone_edges() {
    let tmp = tempfile::tempdir().unwrap();
    site(tmp.path());
    let layout = DataLayout::new(tmp.path());
    let px = |c: f64, r: f64| (X0 + c * CELL, Y0 - r * CELL);
    let lonlat = |c: f64, r: f64| {
        let (x, y) = px(c, r);
        let p = wgs(x, y);
        json!([p.lon, p.lat])
    };
    let lines = [
        ((3.3, 40.7), (77.9, 3.1)),
        ((70.2, 55.6), (8.4, 22.9)),
        ((12.6, 2.2), (31.7, 58.3)),
        ((60.1, 41.9), (75.3, 12.2)),
    ];
    let features: Vec<_> = lines
        .iter()
        .map(|(a, b)| {
            json!({"type": "Feature", "geometry": {"type": "LineString",
                   "coordinates": [lonlat(a.0, a.1), lonlat(b.0, b.1)]},
                   "properties": {"kind": "piste", "uses": ["nordic"]}})
        })
        .collect();
    let pdir = layout.layer_dir(&CERVIERES, "pistes").unwrap();
    std::fs::write(
        pdir.join("graph_edges.geojson"),
        json!({"type": "FeatureCollection", "features": features}).to_string(),
    )
    .unwrap();
    // A quadrilateral with four vertices, two of them outside the grid.
    let quad: Vec<(f64, f64)> = [(-15.0, 12.3), (52.7, -9.1), (95.4, 47.6), (21.9, 50.2)]
        .iter()
        .map(|&(c, r)| {
            let (x, y) = px(c, r);
            let p = wgs(x, y);
            (p.lon, p.lat)
        })
        .collect();
    let mut ring = quad.clone();
    ring.push(quad[0]);
    let layer = ZoneLayer {
        zones: vec![zone("q", Polygon::new(LineString::from(ring), vec![]))],
    };
    let zdir = layout.layer_dir(&CERVIERES, "zones").unwrap();
    std::fs::write(
        zdir.join("zones.geojson"),
        layer.to_geojson_string().unwrap(),
    )
    .unwrap();

    let m = offpiste::prepare::prepare(&CERVIERES, &layout, &[]).unwrap();
    let out = layout.layer_dir(&CERVIERES, offpiste::LAYER).unwrap();
    let g = OffpisteGrid::load(&out).unwrap();
    let mut want = BTreeSet::new();
    for (a, b) in lines {
        want.extend(crossed(a, b));
    }
    let got: BTreeSet<(usize, usize)> = (0..W * H)
        .filter(|i| g.pistes.data()[*i] == PISTE_GROOMED)
        .map(|i| (i % W, i / W))
        .collect();
    assert_eq!(got, want);
    assert_eq!(m.stats["pistes"]["cells_groomed"], want.len());
    for r in 0..H {
        for c in 0..W {
            let k = usize::from(g.zones.data()[r * W + c]);
            assert_eq!(g.classes[k].zone_ids, ids_at(&layer, c, r), "({c}, {r})");
        }
    }
    assert_eq!(m.stats["zones"]["check_mismatches"], 0);
    // Cells whose centre is within 1 m of an edge (projected vertex by vertex: the edges
    // are short enough for the projection to be straight within millimetres).
    let corners: Vec<(f64, f64)> = quad
        .iter()
        .map(|&(lon, lat)| skimap_common::crs::to_lambert93(LonLat::new(lon, lat)).unwrap())
        .collect();
    let mut near = 0;
    for r in 0..H {
        for c in 0..W {
            let ctr = centre(c, r);
            let close = (0..4).any(|k| {
                offpiste::prepare::segment_distance(ctr, corners[k], corners[(k + 1) % 4]) < 1.0
            });
            near += usize::from(close);
        }
    }
    assert!(near > 10, "{near}");
    assert_eq!(
        m.stats["zones"]["boundary_cells_decided_by_containing"],
        near
    );
}

#[test]
fn at_most_128_zones() {
    let tmp = tempfile::tempdir().unwrap();
    site(tmp.path());
    let layout = DataLayout::new(tmp.path());
    let zdir = layout.layer_dir(&CERVIERES, "zones").unwrap();
    let write = |n: usize| {
        let zones = (0..n)
            .map(|k| {
                zone(
                    &format!("z{k}"),
                    Polygon::new(ring(100 + 2 * k, 100, 101 + 2 * k, 101), vec![]),
                )
            })
            .collect();
        std::fs::write(
            zdir.join("zones.geojson"),
            ZoneLayer { zones }.to_geojson_string().unwrap(),
        )
        .unwrap();
    };
    write(128);
    let m = offpiste::prepare::prepare(&CERVIERES, &layout, &[]).unwrap();
    assert_eq!(m.stats["zones"]["zones"], 128);
    write(129);
    let e = offpiste::prepare::prepare(&CERVIERES, &layout, &[]).unwrap_err();
    assert!(e.to_string().contains("at most 128"), "{e}");
}
