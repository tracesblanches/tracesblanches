//! Census of the piste graphs of every buildable site of the registry (B166, #166): number
//! of connected components, size of the largest one, connectors and their lengths, and the
//! gaps left between components (dead end to another component), and the edges of zero
//! length (two nodes at one place: an invalid elevation profile), so that a change of the
//! graph construction is measured on the whole of France, before and after.
//!
//! Reads the planet extract from a file and the registry of the data directory; writes
//! nothing.
//!
//! ```sh
//! SKIMAP_DATA_DIR=… cargo run --release -p skimap-osm --example graph_census -- \
//!     <planet_pistes.osm.gz> [<connectors.tsv>]
//! ```

use std::io::Write;
use std::time::Instant;

use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Registry;
use skimap_osm::extract::{CONNECTOR_MAX_M, france_aoi, site_pistes};
use skimap_osm::graph::{EdgeKind, Graph};
use skimap_osm::parse::read_file;
use skimap_osm::pistes::classify;

/// Distance from `p` to the segment `a b`, metres (local plane).
fn seg_dist(p: LonLat, a: LonLat, b: LonLat) -> f64 {
    let k = p.lat.to_radians().cos();
    let (px, py) = (p.lon * k, p.lat);
    let (ax, ay) = (a.lon * k, a.lat);
    let (bx, by) = (b.lon * k, b.lat);
    let (dx, dy) = (bx - ax, by - ay);
    let l = dx * dx + dy * dy;
    let t = if l == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / l).clamp(0.0, 1.0)
    };
    let (qx, qy) = (ax + t * dx, ay + t * dy);
    ((px - qx).powi(2) + (py - qy).powi(2)).sqrt() * 111_195.0
}

/// Whether the segments `a b` and `c d` cross at a point inside both.
fn proper_cross(a: LonLat, b: LonLat, c: LonLat, d: LonLat) -> bool {
    let o = |p: LonLat, q: LonLat, r: LonLat| {
        (q.lon - p.lon) * (r.lat - p.lat) - (q.lat - p.lat) * (r.lon - p.lon)
    };
    let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let planet = args
        .get(1)
        .ok_or("usage: graph_census <planet_pistes.osm.gz>")?;
    let mut conn_out = match args.get(2) {
        Some(p) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
        None => None,
    };
    let mut gap_out = match args.get(3) {
        Some(p) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
        None => None,
    };
    let data_dir = std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into());
    let registry = Registry::load(&DataLayout::new(data_dir))?;
    let t = Instant::now();
    let data = read_file(std::path::Path::new(planet), &france_aoi())?;
    let all = classify(&data);
    eprintln!(
        "planet: {} pistes, {:.1} s",
        all.pistes.len(),
        t.elapsed().as_secs_f64()
    );
    println!(
        "site\tnodes\tedges\tcomponents\tlargest\tconnectors\tconnector_max_m\tgaps_le_25m\tcrossings\tzero_length_edges"
    );
    let (mut tot_comp, mut tot_nodes, mut tot_largest, mut tot_conn) = (0, 0, 0, 0);
    let mut tot_zero = 0;
    let sites = registry.buildable();
    for site in &sites {
        let (set, _) = site_pistes(&all, &site.bbox);
        let g = Graph::build(&set, CONNECTOR_MAX_M);
        let (comp, n) = g.components();
        let mut sizes = vec![0usize; n];
        for &c in &comp {
            sizes[c] += 1;
        }
        let largest = sizes.iter().copied().max().unwrap_or(0);
        let mut conn_max: f64 = 0.0;
        let mut conns = 0;
        for e in &g.edges {
            if e.kind == EdgeKind::Connector {
                conns += 1;
                conn_max = conn_max.max(e.length_m);
                if let Some(w) = conn_out.as_mut() {
                    writeln!(
                        w,
                        "{}\t{}\t{}\t{:.1}\t{:.7},{:.7}\t{:.7},{:.7}",
                        site.id,
                        g.nodes[e.from].osm_id,
                        g.nodes[e.to].osm_id,
                        e.length_m,
                        e.points[0].lat,
                        e.points[0].lon,
                        e.points[1].lat,
                        e.points[1].lon
                    )?;
                }
            }
        }
        // Crossings: segments of edges of different components that cross.
        let segs: Vec<(usize, LonLat, LonLat)> = g
            .edges
            .iter()
            .flat_map(|e| {
                let c = comp[e.from];
                e.points.windows(2).map(move |w| (c, w[0], w[1]))
            })
            .collect();
        let tree: RTree<GeomWithData<Rectangle<[f64; 2]>, usize>> = RTree::bulk_load(
            segs.iter()
                .enumerate()
                .map(|(i, (_, a, b))| {
                    GeomWithData::new(Rectangle::from_corners([a.lon, a.lat], [b.lon, b.lat]), i)
                })
                .collect(),
        );
        let mut crossings = 0;
        for (i, (c, a, b)) in segs.iter().enumerate() {
            let env = AABB::from_corners([a.lon, a.lat], [b.lon, b.lat]);
            for r in tree.locate_in_envelope_intersecting(&env) {
                let j = r.data;
                if j <= i || segs[j].0 == *c {
                    continue;
                }
                if proper_cross(*a, *b, segs[j].1, segs[j].2) {
                    crossings += 1;
                    if let Some(w) = gap_out.as_mut() {
                        writeln!(
                            w,
                            "{}\t-\t0.0\tcrossing\t-\t{:.7},{:.7}",
                            site.id, a.lat, a.lon
                        )?;
                    }
                }
            }
        }
        // Gaps: dead ends within 25 m of an edge of another component.
        let adj = g.adjacency();
        let mut gaps = 0;
        for (u, a) in adj.iter().enumerate() {
            if a.len() != 1 {
                continue;
            }
            let p = g.nodes[u].pos;
            // Nearest point of another component: (distance, target is a dead end).
            let mut best: Option<(f64, bool, i64)> = None;
            for e in &g.edges {
                if comp[e.from] == comp[u] {
                    continue;
                }
                for w in e.points.windows(2) {
                    let d = seg_dist(p, w[0], w[1]);
                    if best.is_none_or(|b| d < b.0) {
                        let end = [e.from, e.to].into_iter().find(|&v| {
                            adj[v].len() == 1 && g.nodes[v].pos.haversine_m(p) <= d + 0.5
                        });
                        best = Some((d, end.is_some(), end.map_or(0, |v| g.nodes[v].osm_id)));
                    }
                }
            }
            if let Some((d, end, other)) = best
                && d <= 50.0
            {
                if d <= 25.0 {
                    gaps += 1;
                }
                if let Some(w) = gap_out.as_mut() {
                    writeln!(
                        w,
                        "{}\t{}\t{:.1}\t{}\t{}\t{:.7},{:.7}",
                        site.id,
                        g.nodes[u].osm_id,
                        d,
                        if end { "end-end" } else { "end-edge" },
                        other,
                        p.lat,
                        p.lon
                    )?;
                }
            }
        }
        let zero = g.edges.iter().filter(|e| e.length_m == 0.0).count();
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}",
            site.id,
            g.nodes.len(),
            g.edges.len(),
            n,
            largest,
            conns,
            conn_max,
            gaps,
            crossings,
            zero
        );
        tot_zero += zero;
        tot_comp += n;
        tot_nodes += g.nodes.len();
        tot_largest += largest;
        tot_conn += conns;
    }
    println!(
        "TOTAL {} sites\t{tot_nodes}\t-\t{tot_comp}\t{tot_largest}\t{tot_conn}\t-\t-\t-\t{tot_zero}",
        sites.len()
    );
    Ok(())
}
