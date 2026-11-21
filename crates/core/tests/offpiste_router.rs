//! C3 router on a synthetic terrain with varied costs (Q4, #104: `cargo mutants` left the
//! A* heuristic, the cell factors and the window arithmetic of the router untested):
//! A* must give the Dijkstra optimum on every pair, with fewer expansions; each cell
//! factor (groomed, marked, avoided zone, snow, overlay) must change the cost by exactly
//! its value on a uniform terrain.

use jiff::civil::date;
use skimap_common::crs::{Crs, from_lambert93};
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_core::offpiste::{
    CostParams, ELEVATION_NODATA, Heuristic, OffpisteGrid, Overlay, PISTE_GROOMED, PISTE_MARKED,
    PISTE_NONE, Router, SLOPE_NODATA, SnowGrid, Window, ZoneClass, ZonePolicy,
};

const X0: f64 = 998_000.0;
const Y0: f64 = 6_425_000.0;
const CELL: f64 = 5.0;

/// Deterministic pseudo-random numbers (xorshift64).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn t() -> GeoTransform {
    GeoTransform::north_up(X0, Y0, CELL)
}

fn classes(n: usize) -> Vec<ZoneClass> {
    (0..n)
        .map(|k| ZoneClass {
            zone_ids: if k == 0 {
                vec![]
            } else {
                vec![format!("z{k}")]
            },
            representative: LonLat::new(6.7, 44.85),
            cells: 1,
        })
        .collect()
}

/// `w × h` grid: elevation (decimetres), slope (half degrees), zones, pistes.
fn grid(
    w: usize,
    h: usize,
    elev: Vec<u16>,
    slope: Vec<u8>,
    zones: Vec<u8>,
    pistes: Vec<u8>,
) -> OffpisteGrid {
    let n = usize::from(*zones.iter().max().unwrap()) + 1;
    OffpisteGrid::new(
        Grid::from_vec(w, h, t(), Crs::Lambert93, elev).unwrap(),
        Grid::from_vec(w, h, t(), Crs::Lambert93, slope).unwrap(),
        Grid::from_vec(w, h, t(), Crs::Lambert93, zones).unwrap(),
        Grid::from_vec(w, h, t(), Crs::Lambert93, pistes).unwrap(),
        classes(n),
    )
    .unwrap()
}

/// Rough terrain: smooth hills plus noise, slopes up to 35° (some blocked), a few missing
/// cells, two zone classes, a groomed and a marked trail.
fn rough(w: usize, h: usize) -> OffpisteGrid {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut elev = Vec::with_capacity(w * h);
    let mut slope = Vec::with_capacity(w * h);
    let mut zones = Vec::with_capacity(w * h);
    let mut pistes = Vec::with_capacity(w * h);
    for r in 0..h {
        for c in 0..w {
            let (x, y) = (c as f64, r as f64);
            let z = 1500.0 + 40.0 * (x / 9.0).sin() * (y / 7.0).cos() + rng.below(30) as f64 * 0.1;
            let missing = rng.below(100) == 0;
            elev.push(if missing {
                ELEVATION_NODATA
            } else {
                (z * 10.0) as u16
            });
            slope.push(if missing {
                SLOPE_NODATA
            } else {
                rng.below(70) as u8
            });
            zones.push(u8::from(c > w / 3 && c < w / 2 && r > h / 4));
            pistes.push(if r == h / 2 {
                PISTE_GROOMED
            } else if c == w / 4 {
                PISTE_MARKED
            } else {
                PISTE_NONE
            });
        }
    }
    grid(w, h, elev, slope, zones, pistes)
}

fn router<'a>(g: &'a OffpisteGrid, policy: &'a ZonePolicy, params: &'a CostParams) -> Router<'a> {
    Router {
        grid: g,
        snow: None,
        overlays: &[],
        policy,
        params,
    }
}

#[test]
fn astar_finds_the_dijkstra_optimum_with_fewer_expansions() {
    let (w, h) = (90, 70);
    let g = rough(w, h);
    let mut policy = ZonePolicy::open(
        2,
        skimap_common::dates::utc(date(2027, 1, 10).at(10, 0, 0, 0)).unwrap(),
    );
    policy.classes[1].avoid = true;
    let params = CostParams::default();
    let r = router(&g, &policy, &params);
    let win = Window::full(&g);
    let mut rng = Rng(42);
    let (mut routes, mut fewer) = (0, 0);
    let (mut astar_total, mut dijkstra_total) = (0usize, 0usize);
    for _ in 0..40 {
        let a = (rng.below(w as u64) as usize, rng.below(h as u64) as usize);
        let b = (rng.below(w as u64) as usize, rng.below(h as u64) as usize);
        let (x, y) = (
            r.route_cells(win, a, b, Heuristic::AStar),
            r.route_cells(win, a, b, Heuristic::Dijkstra),
        );
        match (x, y) {
            (Ok(x), Ok(y)) => {
                routes += 1;
                assert!(
                    (x.cost_s - y.cost_s).abs() <= 1e-4 * y.cost_s.max(1.0),
                    "{a:?} → {b:?}: A* {} vs Dijkstra {}",
                    x.cost_s,
                    y.cost_s
                );
                assert_eq!(x.cells.first(), Some(&a));
                assert_eq!(x.cells.last(), Some(&b));
                // Consecutive cells are neighbours.
                for p in x.cells.windows(2) {
                    let (dc, dr) = (p[0].0.abs_diff(p[1].0), p[0].1.abs_diff(p[1].1));
                    assert!(dc <= 1 && dr <= 1 && dc + dr > 0, "{p:?}");
                }
                assert!(x.stats.expanded <= y.stats.expanded);
                fewer += usize::from(x.stats.expanded < y.stats.expanded);
                astar_total += x.stats.expanded;
                dijkstra_total += y.stats.expanded;
                assert_eq!(x.stats.window_cells, w * h);
                assert_eq!(x.stats.passable_cells, y.stats.passable_cells);
                assert!(x.stats.pushed >= x.stats.expanded && x.stats.max_open > 0);
                assert!(x.stats.bytes > 0);
            }
            (Err(_), Err(_)) => {}
            (x, y) => panic!("{a:?} → {b:?}: {x:?} vs {y:?}"),
        }
    }
    assert!(routes >= 20, "{routes} routes");
    assert!(
        fewer * 2 >= routes,
        "A* expanded fewer cells on {fewer} of {routes}"
    );
    // The heuristic is informative (it is weak on this rough terrain: the smallest cell
    // factor bounds it): 83 % of Dijkstra's expansions when written, at most 90 %.
    assert!(
        astar_total * 100 <= dijkstra_total * 90,
        "A* {astar_total} vs Dijkstra {dijkstra_total}"
    );
    // Exploring the whole window settles every reachable cell.
    let e = r.explore(win, (w / 2, h / 2));
    assert_eq!(e.window_cells, w * h);
    assert!(e.expanded > w * h / 2 && e.expanded <= e.passable_cells);
}

/// Uniform flat terrain, 21 × 5 cells: one cell factor at a time on the middle row.
fn flat(pistes: u8, zone: u8) -> OffpisteGrid {
    let (w, h) = (21, 5);
    let n = w * h;
    let mut pist = vec![PISTE_NONE; n];
    let mut zones = vec![0u8; n];
    for c in 0..w {
        pist[2 * w + c] = pistes;
        zones[2 * w + c] = zone;
    }
    grid(w, h, vec![15_000; n], vec![0; n], zones, pist)
}

fn straight_cost(
    g: &OffpisteGrid,
    policy: &ZonePolicy,
    params: &CostParams,
    snow: Option<&SnowGrid>,
    overlays: &[Overlay],
) -> f64 {
    let r = Router {
        grid: g,
        snow,
        overlays,
        policy,
        params,
    };
    let route = r
        .route_cells(Window::full(g), (0, 2), (20, 2), Heuristic::AStar)
        .unwrap();
    assert_eq!(route.cells.len(), 21);
    assert!(route.cells.iter().all(|c| c.1 == 2));
    assert!((route.length_m - 100.0).abs() < 1e-6, "{}", route.length_m);
    route.cost_s
}

#[test]
fn each_cell_factor_scales_the_cost() {
    let at = skimap_common::dates::utc(date(2027, 1, 10).at(10, 0, 0, 0)).unwrap();
    let params = CostParams::default();
    let open = ZonePolicy::open(1, at);
    // 100 m on the flat at 1.1 m/s, unknown snow (no snow grid): × 2.
    let base = straight_cost(&flat(PISTE_NONE, 0), &open, &params, None, &[]);
    let walk = 100.0 / f64::from(params.flat_speed_mps) * f64::from(params.snow_unknown_factor);
    assert!((base - walk).abs() < 1e-3, "{base} vs {walk}");
    let ratio = |c: f64| c / base;
    let groomed = straight_cost(&flat(PISTE_GROOMED, 0), &open, &params, None, &[]);
    assert!((ratio(groomed) - f64::from(params.groomed_factor)).abs() < 1e-5);
    let marked = straight_cost(&flat(PISTE_MARKED, 0), &open, &params, None, &[]);
    assert!((ratio(marked) - f64::from(params.marked_factor)).abs() < 1e-5);
    let mut avoid = ZonePolicy::open(2, at);
    avoid.classes[1].avoid = true;
    let g = flat(PISTE_NONE, 1);
    // The whole row is avoided but its neighbours are not: the route leaves the row.
    let r = Router {
        grid: &g,
        snow: None,
        overlays: &[],
        policy: &avoid,
        params: &params,
    };
    let route = r
        .route_cells(Window::full(&g), (0, 2), (20, 2), Heuristic::AStar)
        .unwrap();
    assert!(route.cells.iter().filter(|c| c.1 == 2).count() < 21);
    assert_eq!(route.zone_classes, vec![0, 1]);
    // Blocked off-piste: the route must stay on the (groomed) row.
    let mut off = ZonePolicy::open(2, at);
    for k in 0..2 {
        off.classes[k].offpiste_blocked = true;
    }
    let on_piste = straight_cost(&flat(PISTE_GROOMED, 1), &off, &params, None, &[]);
    assert!((on_piste - groomed).abs() < 1e-3);
    // Snow of the day: sufficient snow (class 2) off the pistes, factor 1.
    let snow = SnowGrid {
        grid: Grid::from_vec(
            5,
            1,
            GeoTransform::north_up(X0, Y0, 25.0),
            Crs::Lambert93,
            vec![SnowGrid::pack(Some(2), Some(1)); 5],
        )
        .unwrap(),
    };
    let snowy = straight_cost(&flat(PISTE_NONE, 0), &open, &params, Some(&snow), &[]);
    assert!((ratio(snowy) - 1.0 / f64::from(params.snow_unknown_factor)).abs() < 1e-5);
    // On a groomed piste the groomed snow class counts (1: limit, × 1.5).
    let g_snow = straight_cost(&flat(PISTE_GROOMED, 0), &open, &params, Some(&snow), &[]);
    let want =
        f64::from(params.groomed_factor * params.snow_factor[1] / params.snow_unknown_factor);
    assert!((ratio(g_snow) - want).abs() < 1e-5, "{}", ratio(g_snow));
    assert_eq!(SnowGrid::srn(SnowGrid::pack(Some(2), Some(1))), 2);
    assert_eq!(SnowGrid::groomed(SnowGrid::pack(Some(2), Some(1))), 1);
    assert_eq!(SnowGrid::pack(None, None), 0xFF);
    // An overlay factor of 3 on class 1 cells.
    let ov = skimap_core::offpiste::ExposureFactors::default()
        .overlay(Grid::from_vec(21, 5, t(), Crs::Lambert93, vec![2u8; 105]).unwrap());
    let exposed = straight_cost(&flat(PISTE_NONE, 0), &open, &params, None, &[ov]);
    assert!((ratio(exposed) - 3.0).abs() < 1e-5, "{}", ratio(exposed));
}

#[test]
fn windows_and_snapping() {
    let g = rough(60, 40);
    let w = Window::around(&g, (10, 5), (20, 30), 3);
    assert_eq!((w.col0, w.row0), (7, 2));
    assert_eq!((w.width, w.height), (17, 32));
    assert_eq!(w.cells(), 17 * 32);
    assert!(w.contains((7, 2)) && w.contains((23, 33)));
    assert!(
        !w.contains((6, 2))
            && !w.contains((7, 1))
            && !w.contains((24, 10))
            && !w.contains((10, 34))
    );
    // Clamped to the grid.
    let w = Window::around(&g, (1, 1), (58, 38), 10);
    assert_eq!((w.col0, w.row0, w.width, w.height), (0, 0, 60, 40));
    let full = Window::full(&g);
    assert_eq!(full.cells(), 60 * 40);
    // Cell of a position, and back.
    let (x, y) = (X0 + 12.5 * CELL, Y0 - 7.5 * CELL);
    let p = from_lambert93(x, y).unwrap();
    assert_eq!(g.cell_of(p).unwrap(), Some((12, 7)));
    let c = g.center(12, 7).unwrap();
    assert!(c.haversine_m(p) < 0.01);
    assert_eq!(g.cell_of(LonLat::new(5.0, 44.0)).unwrap(), None);
    let policy = ZonePolicy::open(
        2,
        skimap_common::dates::utc(date(2027, 1, 10).at(10, 0, 0, 0)).unwrap(),
    );
    let params = CostParams::default();
    let r = router(&g, &policy, &params);
    let passable = r.passable(full);
    assert_eq!(passable.len(), 60 * 40);
    assert!(passable.iter().any(|p| *p) && passable.iter().any(|p| !*p));
    // A routed request between two positions snaps to passable cells nearby.
    let a = g.center(5, 5).unwrap();
    let b = g.center(50, 30).unwrap();
    let route = r.route(a, b).unwrap();
    assert!(route.from_snap_m <= params.snap_radius_m && route.to_snap_m <= params.snap_radius_m);
    assert!(
        route.from_snap_m < 15.0 && route.to_snap_m < 15.0,
        "{route:?}"
    );
    assert!(passable[route.cells[0].1 * 60 + route.cells[0].0]);
}

/// Reference optimum written from the cost model of the module documentation, in f64:
/// cell factor = slope factor × zone avoidance × piste factor × snow factor (no snow grid:
/// unknown); a move costs `len / v + dz / climb` uphill, `len / v × down(−dz / len)`
/// downhill, times the mean factor of its two cells; a diagonal needs both orthogonal
/// neighbours passable.
fn reference_cost(
    g: &OffpisteGrid,
    policy: &ZonePolicy,
    p: &CostParams,
    a: (usize, usize),
    b: (usize, usize),
) -> Option<f64> {
    let (w, h) = (g.width(), g.height());
    let n = w * h;
    let factor: Vec<f64> = (0..n)
        .map(|i| {
            if g.elevation.data()[i] == ELEVATION_NODATA {
                return f64::INFINITY;
            }
            let cl = &policy.classes[usize::from(g.zones.data()[i])];
            let piste = g.pistes.data()[i];
            if cl.blocked || (cl.offpiste_blocked && piste == PISTE_NONE) {
                return f64::INFINITY;
            }
            let mut f = f64::from(p.slope_factor(g.slope.data()[i]));
            if cl.avoid {
                f *= f64::from(p.avoid_factor);
            }
            f *= match piste {
                PISTE_GROOMED => f64::from(p.groomed_factor),
                PISTE_MARKED => f64::from(p.marked_factor),
                _ => 1.0,
            };
            f * f64::from(p.snow_unknown_factor)
        })
        .collect();
    let knots: Vec<(f64, f64)> = p
        .downhill
        .iter()
        .map(|(d, f)| (f64::from(*d).to_radians().tan(), f64::from(*f)))
        .collect();
    let down = |grade: f64| -> f64 {
        if grade <= knots[0].0 {
            return knots[0].1;
        }
        for k in knots.windows(2) {
            if grade <= k[1].0 {
                return k[0].1 + (k[1].1 - k[0].1) * (grade - k[0].0) / (k[1].0 - k[0].0);
            }
        }
        knots[knots.len() - 1].1
    };
    let z = |i: usize| f64::from(g.elevation.data()[i]) * 0.1;
    let (v, climb) = (f64::from(p.flat_speed_mps), f64::from(p.climb_rate_mps));
    let idx = |c: usize, r: usize| r * w + c;
    let mut cost = vec![f64::INFINITY; n];
    let mut done = vec![false; n];
    let s = idx(a.0, a.1);
    if !factor[s].is_finite() || !factor[idx(b.0, b.1)].is_finite() {
        return None;
    }
    cost[s] = 0.0;
    let mut heap = std::collections::BinaryHeap::new();
    heap.push(std::cmp::Reverse((0u64, s)));
    while let Some(std::cmp::Reverse((_, i))) = heap.pop() {
        if done[i] {
            continue;
        }
        done[i] = true;
        if i == idx(b.0, b.1) {
            return Some(cost[i]);
        }
        let (c, r) = ((i % w) as i64, (i / w) as i64);
        for (dc, dr) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let (cj, rj) = (c + dc, r + dr);
            if cj < 0 || rj < 0 || cj >= w as i64 || rj >= h as i64 {
                continue;
            }
            let j = idx(cj as usize, rj as usize);
            if !factor[j].is_finite() || done[j] {
                continue;
            }
            let diagonal = dc != 0 && dr != 0;
            if diagonal
                && (!factor[idx(cj as usize, r as usize)].is_finite()
                    || !factor[idx(c as usize, rj as usize)].is_finite())
            {
                continue;
            }
            let len = if diagonal { CELL * 2f64.sqrt() } else { CELL };
            let dz = z(j) - z(i);
            let pace = if dz >= 0.0 {
                len / v + dz / climb
            } else {
                len / v * down(-dz / len)
            };
            let nc = cost[i] + pace * 0.5 * (factor[i] + factor[j]);
            if nc < cost[j] {
                cost[j] = nc;
                heap.push(std::cmp::Reverse((nc.to_bits(), j)));
            }
        }
    }
    None
}

#[test]
fn router_costs_equal_the_reference_model() {
    let (w, h) = (40, 30);
    let g = rough(w, h);
    let mut policy = ZonePolicy::open(
        2,
        skimap_common::dates::utc(date(2027, 1, 10).at(10, 0, 0, 0)).unwrap(),
    );
    policy.classes[1].avoid = true;
    let params = CostParams::default();
    let r = router(&g, &policy, &params);
    let mut rng = Rng(7);
    let mut compared = 0;
    for _ in 0..30 {
        let a = (rng.below(w as u64) as usize, rng.below(h as u64) as usize);
        let b = (rng.below(w as u64) as usize, rng.below(h as u64) as usize);
        let want = reference_cost(&g, &policy, &params, a, b);
        let got = r.route_cells(Window::full(&g), a, b, Heuristic::AStar).ok();
        match (want, got) {
            (Some(c), Some(route)) => {
                compared += 1;
                assert!(
                    (route.cost_s - c).abs() <= 1e-4 * c.max(1.0),
                    "{a:?} → {b:?}: router {} vs reference {c}",
                    route.cost_s
                );
            }
            (None, None) => {}
            (x, y) => panic!(
                "{a:?} → {b:?}: reference {x:?}, router {:?}",
                y.map(|r| r.cost_s)
            ),
        }
    }
    assert!(compared >= 15, "{compared}");
    // Diagonal on the flat: 4 √2 cells.
    let flat = flat(PISTE_NONE, 0);
    let open = ZonePolicy::open(
        1,
        skimap_common::dates::utc(date(2027, 1, 10).at(10, 0, 0, 0)).unwrap(),
    );
    let rr = router(&flat, &open, &params);
    let d = rr
        .route_cells(Window::full(&flat), (0, 0), (4, 4), Heuristic::AStar)
        .unwrap();
    let want = 4.0 * CELL * 2f64.sqrt() / f64::from(params.flat_speed_mps)
        * f64::from(params.snow_unknown_factor);
    assert!((d.cost_s - want).abs() < 1e-3, "{} vs {want}", d.cost_s);
    assert_eq!(d.cells.len(), 5);
}

#[test]
fn slope_factors() {
    let p = CostParams::default();
    // Raw slope: half degrees. Up to steep_from_deg: 1; linear to steep_factor_at_max at
    // the maximum; blocked when the upper bound of the bin exceeds the maximum.
    assert_eq!(p.slope_factor(0), 1.0);
    assert_eq!(p.slope_factor(40), 1.0);
    let mid = p.slope_factor(50); // 25°: halfway between 20° and 30°.
    assert!(
        (mid - (1.0 + 0.5 * (p.steep_factor_at_max - 1.0))).abs() < 1e-6,
        "{mid}"
    );
    assert!((p.slope_factor(59) - (1.0 + 0.95 * (p.steep_factor_at_max - 1.0))).abs() < 1e-6);
    assert!(!p.slope_blocked(59) && p.slope_blocked(60) && p.slope_blocked(SLOPE_NODATA));
    assert_eq!(p.slope_factor(60), f32::INFINITY);
    let flat = CostParams {
        steep_from_deg: 30.0,
        ..CostParams::default()
    };
    assert_eq!(flat.slope_factor(59), 1.0);
}
