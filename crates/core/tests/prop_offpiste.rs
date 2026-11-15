//! Property tests of the off-piste router and of its file formats (Q3, #103): A* against an
//! independent Dijkstra of the documented cost model, validity of the returned path,
//! lossless SKR1 rasters (both cell layouts) and zone class tables.

use std::collections::BinaryHeap;

use proptest::prelude::*;
use skimap_common::crs::Crs;
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_core::offpiste::{
    ClassPolicy, CostParams, ELEVATION_NODATA, Encoding, Heuristic, OffpisteGrid, Overlay,
    OverlayKind, PISTE_NONE, RouteError, Router, SnowGrid, Window, ZoneClass, ZonePolicy,
    decode_raster, encode_raster_with, encode_zone_classes, parse_zone_classes,
};
use skimap_zones::chrono::{DateTime, Utc};

const CELL: f64 = 5.0;
// Lambert-93 origin near Chapelle-des-Bois.
const OX: f64 = 960_000.0;
const OY: f64 = 6_620_000.0;

fn grid_of<T: Copy>(w: usize, h: usize, cell: f64, data: Vec<T>) -> Grid<T> {
    Grid::from_vec(
        w,
        h,
        GeoTransform::north_up(OX, OY, cell),
        Crs::Lambert93,
        data,
    )
    .expect("shape")
}

#[derive(Debug, Clone)]
struct Site {
    w: usize,
    h: usize,
    elev: Vec<u16>,
    slope: Vec<u8>,
    zones: Vec<u8>,
    pistes: Vec<u8>,
    classes: Vec<(bool, bool, bool)>,
    /// Snow grid at 25 m (packed classes) covering part of the site.
    snow: Option<(usize, usize, Vec<u8>)>,
    /// Overlay at 10 m (classes 0..4) with its factors.
    overlay: Option<(usize, usize, Vec<u8>, [f32; 4])>,
}

fn site() -> impl Strategy<Value = Site> {
    (2usize..22, 2usize..22, 1usize..4).prop_flat_map(|(w, h, nc)| {
        let n = w * h;
        (
            // Elevation: smooth base plus noise, a few no-data cells.
            (
                1000.0f32..2500.0,
                -30.0f32..30.0,
                -30.0f32..30.0,
                prop::collection::vec(
                    prop_oneof![9 => (-3.0f32..3.0).prop_map(Some), 1 => Just(None)],
                    n,
                ),
            ),
            prop::collection::vec(
                prop_oneof![8 => 0u8..80, 1 => 50u8..=180, 1 => Just(u8::MAX)],
                n,
            ),
            prop::collection::vec(0..nc as u8, n),
            prop::collection::vec(
                prop_oneof![6 => Just(0u8), 1 => Just(1u8), 2 => Just(2u8)],
                n,
            ),
            prop::collection::vec(
                (
                    prop::bool::weighted(0.1),
                    prop::bool::weighted(0.3),
                    prop::bool::weighted(0.3),
                ),
                nc,
            ),
            prop::option::of((1usize..6, 1usize..6).prop_flat_map(|(sw, sh)| {
                (
                    Just(sw),
                    Just(sh),
                    prop::collection::vec(
                        (prop::option::of(0u8..3), prop::option::of(0u8..3))
                            .prop_map(|(a, b)| SnowGrid::pack(a, b)),
                        sw * sh,
                    ),
                )
            })),
            prop::option::of((1usize..12, 1usize..12).prop_flat_map(|(ow, oh)| {
                (
                    Just(ow),
                    Just(oh),
                    prop::collection::vec(0u8..4, ow * oh),
                    prop::array::uniform4(prop_oneof![
                        Just(1.0f32),
                        0.5f32..4.0,
                        Just(f32::INFINITY)
                    ]),
                )
            })),
        )
            .prop_map(
                move |((z0, gx, gy, noise), slope, zones, pistes, classes, snow, overlay)| {
                    let elev = (0..n)
                        .map(|i| {
                            let (c, r) = ((i % w) as f32, (i / w) as f32);
                            match noise[i] {
                                Some(e) => ((z0 + gx * c + gy * r + e) * 10.0).round() as u16,
                                None => ELEVATION_NODATA,
                            }
                        })
                        .collect();
                    Site {
                        w,
                        h,
                        elev,
                        slope,
                        zones,
                        pistes,
                        classes,
                        snow,
                        overlay,
                    }
                },
            )
    })
}

fn params() -> impl Strategy<Value = CostParams> {
    (
        (15.0f32..45.0, 0.3f32..3.0, 0.02f32..0.5),
        // Downhill knots: increasing angles, positive factors.
        prop::collection::vec((0.5f32..12.0, 0.2f32..6.0), 0..6),
        (0.0f32..40.0, 1.0f32..4.0),
        (
            prop::array::uniform3(prop_oneof![0.5f32..8.0, Just(f32::INFINITY)]),
            0.5f32..4.0,
        ),
        (0.2f32..2.0, 0.2f32..2.0, 1.0f32..4.0),
    )
        .prop_map(
            |(
                (max_slope, v, climb),
                knots,
                (steep_from, steep_max),
                (snow, unknown),
                (gf, mf, af),
            )| {
                let mut a = 0.0f32;
                let downhill = knots
                    .into_iter()
                    .map(|(da, f)| {
                        a += da;
                        (a, f)
                    })
                    .collect();
                CostParams {
                    max_slope_deg: max_slope,
                    flat_speed_mps: v,
                    climb_rate_mps: climb,
                    downhill,
                    steep_from_deg: steep_from,
                    steep_factor_at_max: steep_max,
                    snow_factor: snow,
                    snow_unknown_factor: unknown,
                    groomed_factor: gf,
                    marked_factor: mf,
                    avoid_factor: af,
                    ..CostParams::default()
                }
            },
        )
}

struct Built {
    grid: OffpisteGrid,
    snow: Option<SnowGrid>,
    overlays: Vec<Overlay>,
    policy: ZonePolicy,
}

fn at() -> DateTime<Utc> {
    "2026-02-15T10:00:00Z".parse().expect("date")
}

fn build(s: &Site) -> Built {
    let classes: Vec<ZoneClass> = s
        .classes
        .iter()
        .map(|_| ZoneClass {
            zone_ids: vec![],
            representative: LonLat::new(6.1, 46.6),
            cells: 0,
        })
        .collect();
    let grid = OffpisteGrid::new(
        grid_of(s.w, s.h, CELL, s.elev.clone()),
        grid_of(s.w, s.h, CELL, s.slope.clone()),
        grid_of(s.w, s.h, CELL, s.zones.clone()),
        grid_of(s.w, s.h, CELL, s.pistes.clone()),
        classes,
    )
    .expect("valid grid");
    let mut policy = ZonePolicy::open(s.classes.len(), at());
    for (p, &(blocked, off, avoid)) in policy.classes.iter_mut().zip(&s.classes) {
        *p = ClassPolicy {
            blocked,
            offpiste_blocked: off,
            avoid,
            ..ClassPolicy::open()
        };
    }
    let snow = s.snow.as_ref().map(|(sw, sh, v)| SnowGrid {
        grid: grid_of(*sw, *sh, 25.0, v.clone()),
    });
    let overlays = s
        .overlay
        .as_ref()
        .map(|(ow, oh, v, f)| {
            let mut factor = [1.0f32; 256];
            factor[..4].copy_from_slice(f);
            vec![Overlay {
                kind: OverlayKind::Forest,
                grid: grid_of(*ow, *oh, 10.0, v.clone()),
                factor,
            }]
        })
        .unwrap_or_default();
    Built {
        grid,
        snow,
        overlays,
        policy,
    }
}

/// Downhill factor of the documented model: piecewise linear in grade between the knots
/// (angles in degrees), constant outside.
fn down(knots: &[(f32, f32)], grade: f64) -> f64 {
    let k: Vec<(f64, f64)> = knots
        .iter()
        .map(|&(a, f)| (f64::from(a).to_radians().tan(), f64::from(f)))
        .collect();
    match k.as_slice() {
        [] => 1.0,
        [(g0, f0), ..] if grade <= *g0 => *f0,
        _ => {
            for w in k.windows(2) {
                let ((g0, f0), (g1, f1)) = (w[0], w[1]);
                if grade <= g1 {
                    return f0 + (f1 - f0) * (grade - g0) / (g1 - g0);
                }
            }
            k[k.len() - 1].1
        }
    }
}

/// Cost of the move from cell `a` to the neighbouring cell `b` (documented model), `None`
/// if not allowed.
fn move_cost(
    r: &Router<'_>,
    f: &[f64],
    w: usize,
    a: (usize, usize),
    b: (usize, usize),
) -> Option<f64> {
    let idx = |c: (usize, usize)| c.1 * w + c.0;
    let (fa, fb) = (f[idx(a)], f[idx(b)]);
    if !fa.is_finite() || !fb.is_finite() {
        return None;
    }
    let diagonal = a.0 != b.0 && a.1 != b.1;
    if diagonal && (!f[idx((b.0, a.1))].is_finite() || !f[idx((a.0, b.1))].is_finite()) {
        return None;
    }
    let p = r.params;
    let len = if diagonal {
        CELL * std::f64::consts::SQRT_2
    } else {
        CELL
    };
    let z = |c: (usize, usize)| f64::from(r.grid.elevation.data()[idx(c)]) * 0.1;
    let dz = z(b) - z(a);
    let v = f64::from(p.flat_speed_mps);
    let pace = if dz >= 0.0 {
        len / v + dz / f64::from(p.climb_rate_mps)
    } else {
        len / v * down(&p.downhill, -dz / len)
    };
    Some(pace * 0.5 * (fa + fb))
}

/// Reference Dijkstra over the whole grid (f64, binary heap of `(cost, cell)`).
fn reference(r: &Router<'_>, start: (usize, usize), goal: (usize, usize)) -> Option<f64> {
    let (w, h) = (r.grid.width(), r.grid.height());
    let f: Vec<f64> = (0..w * h)
        .map(|i| f64::from(r.cell_factor(i % w, i / w)))
        .collect();
    if !f[start.1 * w + start.0].is_finite() || !f[goal.1 * w + goal.0].is_finite() {
        return None;
    }
    let mut dist = vec![f64::INFINITY; w * h];
    let mut heap = BinaryHeap::new();
    let key = |c: f64| std::cmp::Reverse(c.to_bits());
    dist[start.1 * w + start.0] = 0.0;
    heap.push((key(0.0), start.0, start.1));
    while let Some((std::cmp::Reverse(bits), c, rw)) = heap.pop() {
        let d = f64::from_bits(bits);
        if d > dist[rw * w + c] {
            continue;
        }
        if (c, rw) == goal {
            return Some(d);
        }
        for dc in -1i64..=1 {
            for dr in -1i64..=1 {
                if dc == 0 && dr == 0 {
                    continue;
                }
                let (nc, nr) = (c as i64 + dc, rw as i64 + dr);
                if nc < 0 || nr < 0 || nc >= w as i64 || nr >= h as i64 {
                    continue;
                }
                let b = (nc as usize, nr as usize);
                if let Some(m) = move_cost(r, &f, w, (c, rw), b) {
                    let nd = d + m;
                    if nd < dist[b.1 * w + b.0] {
                        dist[b.1 * w + b.0] = nd;
                        heap.push((key(nd), b.0, b.1));
                    }
                }
            }
        }
    }
    None
}

proptest! {
    /// A* is optimal: same cost as an independent Dijkstra of the documented cost model
    /// (f64) on random terrain, slopes, zones and policies, pistes, snow and overlay grids
    /// of other resolutions and random cost parameters; the returned path is a chain of
    /// allowed moves (no corner cutting) whose cost is the reported one; A* and the
    /// router's own Dijkstra agree.
    #[test]
    fn astar_matches_reference_dijkstra(
        s in site(),
        p in params(),
        a in (any::<prop::sample::Index>(), any::<prop::sample::Index>()),
        b in (any::<prop::sample::Index>(), any::<prop::sample::Index>()),
    ) {
        let built = build(&s);
        let r = Router {
            grid: &built.grid,
            snow: built.snow.as_ref(),
            overlays: &built.overlays,
            policy: &built.policy,
            params: &p,
        };
        let start = (a.0.index(s.w), a.1.index(s.h));
        let goal = (b.0.index(s.w), b.1.index(s.h));
        let want = reference(&r, start, goal);
        let got = r.route_cells(Window::full(&built.grid), start, goal, Heuristic::AStar);
        let dij = r.route_cells(Window::full(&built.grid), start, goal, Heuristic::Dijkstra);
        match (&got, want) {
            (Ok(route), Some(want)) => {
                let tol = 1e-4 * want.max(1.0);
                prop_assert!((route.cost_s - want).abs() <= tol, "A* {} vs reference {want}", route.cost_s);
                let dij = dij.as_ref().expect("Dijkstra finds the route too");
                prop_assert!((dij.cost_s - want).abs() <= tol, "Dijkstra {} vs reference {want}", dij.cost_s);
                // The path: from start to goal, allowed moves, cost = sum of moves.
                prop_assert_eq!(route.cells.first().copied(), Some(start));
                prop_assert_eq!(route.cells.last().copied(), Some(goal));
                let f: Vec<f64> = (0..s.w * s.h)
                    .map(|i| f64::from(r.cell_factor(i % s.w, i / s.w)))
                    .collect();
                let mut sum = 0.0;
                for m in route.cells.windows(2) {
                    let (x, y) = (m[0], m[1]);
                    prop_assert!(x.0.abs_diff(y.0) <= 1 && x.1.abs_diff(y.1) <= 1 && x != y, "jump {x:?} -> {y:?}");
                    let c = move_cost(&r, &f, s.w, x, y);
                    prop_assert!(c.is_some(), "forbidden move {x:?} -> {y:?}");
                    sum += c.unwrap_or_default();
                }
                prop_assert!((sum - route.cost_s).abs() <= tol, "path cost {sum} vs reported {}", route.cost_s);
                prop_assert_eq!(route.piste.len(), route.cells.len());
                prop_assert!(route.length_m >= 0.0 && route.ascent_m >= 0.0 && route.descent_m >= 0.0);
            }
            (Err(RouteError::NoRoute { .. } | RouteError::NoPassableCell { .. }), None) => {
                prop_assert!(dij.is_err());
            }
            (got, want) => prop_assert!(false, "A* {got:?} vs reference {want:?}"),
        }
    }

    /// Blocked cells never appear on a route and the zone policy holds: no cell of a
    /// blocked class, no off-piste cell of a class closed off the pistes.
    #[test]
    fn routes_avoid_blocked_cells(
        s in site(),
        p in params(),
        a in (any::<prop::sample::Index>(), any::<prop::sample::Index>()),
        b in (any::<prop::sample::Index>(), any::<prop::sample::Index>()),
    ) {
        let built = build(&s);
        let r = Router {
            grid: &built.grid,
            snow: built.snow.as_ref(),
            overlays: &built.overlays,
            policy: &built.policy,
            params: &p,
        };
        let start = (a.0.index(s.w), a.1.index(s.h));
        let goal = (b.0.index(s.w), b.1.index(s.h));
        if let Ok(route) = r.route_cells(Window::full(&built.grid), start, goal, Heuristic::AStar) {
            for &(c, rw) in &route.cells {
                let i = rw * s.w + c;
                prop_assert!(r.cell_factor(c, rw).is_finite());
                prop_assert!(s.elev[i] != ELEVATION_NODATA);
                prop_assert!(!p.slope_blocked(s.slope[i]));
                let (blocked, off, _) = s.classes[usize::from(s.zones[i])];
                prop_assert!(!blocked);
                prop_assert!(!(off && s.pistes[i] == PISTE_NONE));
            }
        }
    }

    /// SKR1 rasters are lossless for both cell layouts and both cell types, and keep the
    /// georeferencing and the no-data value.
    #[test]
    fn skr1_round_trip(
        (w, h) in (0usize..40, 0usize..40),
        seed in any::<u64>(),
        nodata16 in any::<u16>(),
        nodata8 in any::<u8>(),
        origin in (-1e6f64..1e7, -1e6f64..1e7),
        cell in prop_oneof![Just(5.0f64), Just(25.0), 0.1f64..1000.0],
        row_delta in any::<bool>(),
    ) {
        let mut x = seed | 1;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let v16: Vec<u16> = (0..w * h).map(|_| next() as u16).collect();
        let v8: Vec<u8> = (0..w * h).map(|_| next() as u8).collect();
        let gt = GeoTransform::north_up(origin.0, origin.1, cell);
        let g16 = Grid::from_vec(w, h, gt, Crs::Lambert93, v16).expect("shape");
        let g8 = Grid::from_vec(w, h, gt, Crs::WebMercator, v8).expect("shape");
        let enc = if row_delta { Encoding::RowDelta } else { Encoding::Raw };
        let b16 = encode_raster_with(&g16, nodata16, enc).expect("encode");
        let b8 = encode_raster_with(&g8, nodata8, enc).expect("encode");
        prop_assert_eq!(b16.len(), 72 + 2 * w * h);
        prop_assert_eq!(b8.len(), 72 + w * h);
        let d16: Grid<u16> = decode_raster(&b16).expect("decode");
        let d8: Grid<u8> = decode_raster(&b8).expect("decode");
        prop_assert_eq!(&d16, &g16.clone().with_nodata(nodata16));
        prop_assert_eq!(&d8, &g8.clone().with_nodata(nodata8));
        // A u16 raster is not a u8 raster and the other way round.
        prop_assert!(decode_raster::<u8>(&b16).is_err());
        prop_assert!(decode_raster::<u16>(&b8).is_err());
        // Any truncation is an error, never a panic.
        if !b16.is_empty() {
            let cut = (seed as usize) % b16.len();
            prop_assert!(decode_raster::<u16>(&b16[..cut]).is_err());
        }
    }

    /// The zone class table round-trips (coordinates at 1e-7°) and refuses unencodable ids.
    #[test]
    fn zone_classes_round_trip(
        classes in prop::collection::vec(
            (
                prop::collection::vec("[a-z]{1,8}:[A-Za-z0-9_.-]{1,12}", 0..5),
                (-180.0f64..180.0, -90.0f64..90.0),
                any::<u64>(),
            ),
            0..40,
        ),
    ) {
        let classes: Vec<ZoneClass> = classes
            .into_iter()
            .map(|(mut ids, (lon, lat), cells)| {
                ids.sort();
                ZoneClass { zone_ids: ids, representative: LonLat::new(lon, lat), cells }
            })
            .collect();
        let text = encode_zone_classes(&classes).expect("encodable");
        let back = parse_zone_classes(&text).expect("parse");
        prop_assert_eq!(back.len(), classes.len());
        for (a, b) in classes.iter().zip(&back) {
            prop_assert_eq!(&a.zone_ids, &b.zone_ids);
            prop_assert_eq!(a.cells, b.cells);
            prop_assert!((a.representative.lon - b.representative.lon).abs() <= 5e-8);
            prop_assert!((a.representative.lat - b.representative.lat).abs() <= 5e-8);
        }
        let mut bad = classes.clone();
        if let Some(c) = bad.first_mut() {
            c.zone_ids.push("a,b".into());
            prop_assert!(encode_zone_classes(&bad).is_err());
        }
    }
}
