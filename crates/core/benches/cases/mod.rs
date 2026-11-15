//! The dominating phone-side cases (Q5, #105), shared by the instruction-count benchmarks
//! (`dominant`, under Valgrind) and the wall-clock benchmark (`wall`, native).

#![allow(dead_code)] // Each benchmark uses a subset.

use std::hint::black_box;
use std::io::Read;
use std::path::{Path, PathBuf};

use skimap_common::geom::LonLat;
use skimap_core::eta::EtaModel;
use skimap_core::graph::PisteGraph;
use skimap_core::matching::synth::{Noise, Rng, add_noise, sample_polyline};
use skimap_core::matching::{Fix, MatchParams, ROUTE_HYSTERESIS, RouteMonitor};
use skimap_core::offpiste::{
    CostParams, ExposureFactors, FILE_ELEVATION, FILE_EXPOSURE, FILE_PISTES, FILE_SLOPE,
    FILE_ZONE_CLASSES, FILE_ZONES, OffpisteGrid, Overlay, Router as GridRouter, SnowGrid, Window,
    ZonePolicy, decode_raster, parse_zone_classes, read_raster,
};
use skimap_core::routing::{Distance, Router, Style, StylePolicy};

/// Le Laus (hamlet) and the Col d'Izoard: the C3 proof route.
pub const LAUS: LonLat = LonLat::new(6.72687, 44.8552163);
/// Col d'Izoard (2361 m).
pub const IZOARD: LonLat = LonLat::new(6.734975, 44.8196595);
/// Start of the « Liaison Le Laus - Les Fonts » (the Cancel trails are another component).
pub const LAUS_LIAISON: LonLat = LonLat::new(6.725_006, 44.858_940);
/// Refuge des Fonts de Cervières.
pub const FONTS: LonLat = LonLat::new(6.8157, 44.84465);

/// A file of `benches/data`.
pub fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../benches/data")
        .join(name)
}

fn gunzip(path: &Path) -> Vec<u8> {
    let f = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(f)
        .read_to_end(&mut out)
        .expect("gzip");
    out
}

/// The off-piste inputs of a request, owned.
pub struct Offpiste {
    pub grid: OffpisteGrid,
    pub snow: SnowGrid,
    pub overlays: Vec<Overlay>,
    pub policy: ZonePolicy,
    pub params: CostParams,
}

impl Offpiste {
    fn new(grid: OffpisteGrid, snow: SnowGrid, exposure: skimap_common::grid::Grid<u8>) -> Self {
        let n = grid.classes.len();
        let at = "2026-02-15T09:00:00Z".parse().expect("date");
        Self {
            grid,
            snow,
            overlays: vec![ExposureFactors::default().overlay(exposure)],
            policy: ZonePolicy::open(n, at),
            params: CostParams::default(),
        }
    }

    /// The router of the request.
    pub fn router(&self) -> GridRouter<'_> {
        GridRouter {
            grid: &self.grid,
            snow: Some(&self.snow),
            overlays: &self.overlays,
            policy: &self.policy,
            params: &self.params,
        }
    }
}

/// The Laus → Izoard crop of `benches/data`.
pub fn setup_offpiste() -> Offpiste {
    let dir = data("cervieres_offpiste");
    let skr = |name: &str| gunzip(&dir.join(format!("{name}.gz")));
    let classes = parse_zone_classes(
        &std::fs::read_to_string(dir.join(FILE_ZONE_CLASSES)).expect("zone classes"),
    )
    .expect("zone classes");
    let grid = OffpisteGrid::new(
        decode_raster(&skr(FILE_ELEVATION)).expect("elevation"),
        decode_raster(&skr(FILE_SLOPE)).expect("slope"),
        decode_raster(&skr(FILE_ZONES)).expect("zones"),
        decode_raster(&skr(FILE_PISTES)).expect("pistes"),
        classes,
    )
    .expect("grid");
    let snow = SnowGrid {
        grid: decode_raster(&skr("snow_2026-02-15.skr")).expect("snow"),
    };
    let exposure = decode_raster(&skr(FILE_EXPOSURE)).expect("exposure");
    Offpiste::new(grid, snow, exposure)
}

/// The whole prepared grid of Cervières (`<data>/out/cervieres/offpiste`), if present.
pub fn setup_offpiste_full(data_dir: &Path) -> Option<Offpiste> {
    let dir = data_dir.join("out/cervieres/offpiste");
    let grid = OffpisteGrid::load(&dir).ok()?;
    let snow = SnowGrid::load(&dir.join("snow_2026-02-15.skr")).ok()?;
    let exposure = read_raster(&dir.join(FILE_EXPOSURE)).ok()?;
    Some(Offpiste::new(grid, snow, exposure))
}

/// Off-piste route Le Laus → Col d'Izoard (snapping, window, A*, route geometry).
pub fn run_offpiste_route(o: &Offpiste) -> f64 {
    let route = o
        .router()
        .route(black_box(LAUS), black_box(IZOARD))
        .expect("route");
    route.cost_s
}

/// Exhaustive search from Le Laus over the whole grid (unreachable destination).
pub fn run_offpiste_explore(o: &Offpiste) -> usize {
    let start = o.grid.cell_of(LAUS).expect("crs").expect("inside");
    o.router()
        .explore(Window::full(&o.grid), black_box(start))
        .expanded
}

/// A whole outing: the graph, the planned route and the noisy fixes.
pub struct Outing {
    pub graph: &'static PisteGraph,
    pub plan: Vec<LonLat>,
    pub fixes: Vec<Fix>,
}

/// The trail graph of a site in `benches/data` (leaked: routers and matchers borrow it).
pub fn graph(site: &str) -> &'static PisteGraph {
    let json = gunzip(&data(&format!("{site}_piste_graph.json.gz")));
    let g = PisteGraph::from_json(std::str::from_utf8(&json).expect("UTF-8")).expect("graph");
    Box::leak(Box::new(g))
}

/// Le Laus → Refuge des Fonts and back at 3 m/s, one fix per second, 10 m Gauss–Markov
/// noise and 30 m forest outliers.
pub fn setup_outing() -> Outing {
    let g = graph("cervieres");
    let r = Router::new(
        g,
        Distance {
            style: Style::Classic,
            policy: StylePolicy::default(),
        },
    );
    let out = r.route(LAUS_LIAISON, FONTS, 200.0).expect("outbound route");
    let back = r.route(FONTS, LAUS_LIAISON, 200.0).expect("return route");
    let mut plan = out.polyline(g);
    plan.extend(back.polyline(g).into_iter().skip(1));
    let truth = sample_polyline(&plan, 1.0, |_| 3.0);
    let noise = Noise {
        sigma_m: 10.0,
        tau_s: 20.0,
        outlier_rate_hz: 0.02,
        outlier_m: 30.0,
        outlier_len_s: (2.0, 10.0),
    };
    let fixes = add_noise(&truth, &noise, &mut Rng(0x5EED));
    Outing {
        graph: g,
        plan,
        fixes,
    }
}

/// The outing matched fix by fix (route monitor), then decoded (Viterbi).
pub fn run_matching(o: &Outing) -> usize {
    let mut m = RouteMonitor::new(o.graph, &o.plan, MatchParams::default(), ROUTE_HYSTERESIS);
    let mut alerts = 0;
    for f in &o.fixes {
        alerts += usize::from(m.push(black_box(*f)).event.is_some());
    }
    m.matcher().decode().len() + alerts
}

/// The whole graph of a site and node pairs spread over it.
pub struct Network {
    pub graph: &'static PisteGraph,
    pub pairs: Vec<(LonLat, LonLat)>,
}

/// 40 deterministic node pairs of the largest component of the Chapelle-des-Bois graph.
pub fn setup_network() -> Network {
    let g = graph("chapelle-des-bois");
    let (comp, _) = g.components();
    let mut size = std::collections::BTreeMap::new();
    for c in &comp {
        *size.entry(*c).or_insert(0usize) += 1;
    }
    let main_c = size
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(c, _)| *c)
        .expect("nodes");
    let nodes: Vec<LonLat> = g
        .nodes()
        .iter()
        .zip(&comp)
        .filter(|(_, c)| **c == main_c)
        .map(|(n, _)| n.pos)
        .collect();
    let mut rng = Rng(42);
    let pick = |rng: &mut Rng| nodes[(rng.next_u64() % nodes.len() as u64) as usize];
    let pairs = (0..40).map(|_| (pick(&mut rng), pick(&mut rng))).collect();
    Network { graph: g, pairs }
}

/// Router precomputation (C5 time model) and the 40 fastest routes.
pub fn run_routing(n: &Network) -> f64 {
    let r = Router::new(n.graph, EtaModel::new(Style::Classic));
    let mut total = 0.0;
    for &(a, b) in &n.pairs {
        if let Ok(route) = r.route(black_box(a), black_box(b), 50.0) {
            total += route.cost;
        }
    }
    total
}
