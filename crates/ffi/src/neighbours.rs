//! One outing over several installed packs (H2 #133): the packs of the neighbouring
//! areas whose box meets the box of the pack of the start are joined to it, so that a loop
//! can cross from one area into the next (each area has its own pack, #175).
//!
//! - The trail graphs are merged ([`merge_graphs`]): nodes by OSM node, edges by OSM way
//!   and end nodes; an edge a neighbour split differently (a junction with a trail only it
//!   has) is not doubled, and every edge is split at the nodes of the other packs that lie
//!   on it, so the trails of each pack meet at their real junctions.
//! - The GeoJSON layers of the pipeline view are merged feature by feature (deduplicated by
//!   identifier, OSM way, or geometry), the rules of the zones by rule identifier; the
//!   other layers (weather, snow, rasters) stay those of the pack of the start.
//!
//! The merge is deterministic: the morning, the tracking and the recap rebuild the same
//! graph from the same packs (`neighbours.txt` of the plan directory lists them).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use skimap_common::geom::LonLat;
use skimap_core::graph::{Edge, GraphData, Node, Profile};
use skimap_pack::Pack;

use crate::tracking::TrackingError;
use crate::tracking_day::PackView;

/// Plan directory file: the directories of the neighbouring packs joined to the pack of
/// the start, one per line (absent: the pack of the start alone).
pub const NEIGHBOURS_FILE: &str = "neighbours.txt";

/// Pack layer of the trail graph.
const GRAPH_LAYER: &str = "piste-graph";

fn invalid(e: impl std::fmt::Display) -> TrackingError {
    TrackingError::Invalid {
        detail: e.to_string(),
    }
}

fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// The packs among `dirs` that are joined to `primary`: another site, a box that meets
/// its box; in the order of their site identifiers (stable). Unreadable directories are
/// left out.
pub fn neighbours(primary: &Pack, dirs: &[String]) -> Vec<Pack> {
    let me = &primary.manifest().site;
    let mut out: Vec<Pack> = Vec::new();
    for d in dirs {
        let Ok(p) = Pack::open(PathBuf::from(d)) else {
            continue;
        };
        let s = &p.manifest().site;
        if s.id == me.id
            || !overlaps(s.bbox, me.bbox)
            || out.iter().any(|o| o.manifest().site.id == s.id)
        {
            continue;
        }
        out.push(p);
    }
    out.sort_by(|a, b| a.manifest().site.id.cmp(&b.manifest().site.id));
    out
}

/// Writes the neighbours of a plan into `plan_dir` (nothing when there is none).
pub fn write_neighbours(plan_dir: &Path, packs: &[Pack]) -> Result<(), TrackingError> {
    let path = plan_dir.join(NEIGHBOURS_FILE);
    if packs.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(invalid)?;
        }
        return Ok(());
    }
    let text: String = packs
        .iter()
        .map(|p| format!("{}\n", p.root().display()))
        .collect();
    skimap_common::output::write_text(&path, &text).map_err(invalid)
}

/// The neighbouring packs of the plan in `plan_dir` (none without the file).
pub fn read_neighbours(plan_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(plan_dir.join(NEIGHBOURS_FILE))
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The graph data of a pack.
pub fn graph_data(pack: &Pack) -> Result<GraphData, TrackingError> {
    let l = pack.layer(GRAPH_LAYER).map_err(invalid)?;
    let text = std::fs::read_to_string(pack.path(&l.file).map_err(invalid)?).map_err(invalid)?;
    serde_json::from_str(&text).map_err(invalid)
}

fn node_key(n: &Node) -> (i64, u64, u64) {
    match n.osm_node_id {
        Some(id) => (id, 0, 0),
        None => (i64::MIN, n.pos.lon.to_bits(), n.pos.lat.to_bits()),
    }
}

fn pos_key(p: LonLat) -> (u64, u64) {
    (p.lon.to_bits(), p.lat.to_bits())
}

/// Splits a profile at `d` metres from its start: (before, after), each rebased from 0;
/// a part with fewer than two stations has none.
fn split_profile(p: &Profile, d: f64) -> (Option<Profile>, Option<Profile>) {
    let z = p.elevation_at(d) as f32;
    let df = d as f32;
    let mut a = Profile {
        dist_m: Vec::new(),
        elev_m: Vec::new(),
        filled: 0,
    };
    let mut b = a.clone();
    for (&x, &e) in p.dist_m.iter().zip(&p.elev_m) {
        if x < df - 1e-3 {
            a.dist_m.push(x);
            a.elev_m.push(e);
        }
    }
    a.dist_m.push(df);
    a.elev_m.push(z);
    b.dist_m.push(0.0);
    b.elev_m.push(z);
    for (&x, &e) in p.dist_m.iter().zip(&p.elev_m) {
        if x > df + 1e-3 {
            b.dist_m.push(x - df);
            b.elev_m.push(e);
        }
    }
    let ok = |q: Profile| (q.dist_m.len() >= 2 && q.dist_m[0] == 0.0).then_some(q);
    (ok(a), ok(b))
}

/// `e` split at the interior points `cuts` (indices into its points, increasing) that are
/// the nodes `nodes`.
fn split_edge(e: &Edge, cuts: &[(usize, usize)]) -> Vec<Edge> {
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut start = 0;
    let mut from = e.from;
    let mut profile = e.profile.clone();
    let mut done_m = 0.0;
    let mut cum = vec![0.0];
    for w in e.points.windows(2) {
        let last = *cum.last().expect("first");
        cum.push(last + w[0].haversine_m(w[1]));
    }
    for &(i, node) in cuts {
        let (before, after) = match &profile {
            Some(p) => split_profile(p, cum[i] - done_m),
            None => (None, None),
        };
        out.push(Edge {
            from,
            to: node,
            oneway: e.oneway,
            points: e.points[start..=i].to_vec(),
            attrs: e.attrs.clone(),
            profile: before,
        });
        profile = after;
        done_m = cum[i];
        start = i;
        from = node;
    }
    out.push(Edge {
        from,
        to: e.to,
        oneway: e.oneway,
        points: e.points[start..].to_vec(),
        attrs: e.attrs.clone(),
        profile,
    });
    out
}

/// The trail graph of `primary` joined with `others` (see the module documentation). The
/// site and the terrain source are the primary's.
pub fn merge_graphs(primary: &GraphData, others: &[GraphData]) -> GraphData {
    let mut g = primary.clone();
    if others.is_empty() {
        return g;
    }
    let mut nodes: HashMap<(i64, u64, u64), usize> = HashMap::new();
    for (i, n) in g.nodes.iter().enumerate() {
        nodes.entry(node_key(n)).or_insert(i);
    }
    let ekey = |g: &GraphData, e: &Edge| {
        let (a, b) = (node_key(&g.nodes[e.from]), node_key(&g.nodes[e.to]));
        (e.attrs.osm_way_id, a.min(b), a.max(b))
    };
    let mut edges: HashSet<_> = g.edges.iter().map(|e| ekey(&g, e)).collect();
    for o in others {
        let map: Vec<usize> = o
            .nodes
            .iter()
            .map(|n| {
                *nodes.entry(node_key(n)).or_insert_with(|| {
                    g.nodes.push(n.clone());
                    g.nodes.len() - 1
                })
            })
            .collect();
        // Points of the edges already in the graph, per OSM way: an edge of `o` most of
        // whose points are already there is the same way split at other nodes.
        let mut by_way: HashMap<i64, HashSet<(u64, u64)>> = HashMap::new();
        for e in &g.edges {
            if let Some(w) = e.attrs.osm_way_id {
                by_way
                    .entry(w)
                    .or_default()
                    .extend(e.points.iter().map(|p| pos_key(*p)));
            }
        }
        for e in &o.edges {
            let mut e2 = e.clone();
            e2.from = map[e.from];
            e2.to = map[e.to];
            let k = ekey(&g, &e2);
            if edges.contains(&k) {
                continue;
            }
            if let Some(seen) = e.attrs.osm_way_id.and_then(|w| by_way.get(&w)) {
                let known = e
                    .points
                    .iter()
                    .filter(|p| seen.contains(&pos_key(**p)))
                    .count();
                if 2 * known > e.points.len() {
                    continue;
                }
            }
            edges.insert(k);
            g.edges.push(e2);
        }
    }
    // Every edge is split at the end nodes of other edges that lie on its interior points.
    let mut ends: HashMap<(u64, u64), usize> = HashMap::new();
    for e in &g.edges {
        for n in [e.from, e.to] {
            ends.entry(pos_key(g.nodes[n].pos)).or_insert(n);
        }
    }
    let mut out = Vec::with_capacity(g.edges.len());
    for e in &g.edges {
        let cuts: Vec<(usize, usize)> = (1..e.points.len().saturating_sub(1))
            .filter_map(|i| {
                ends.get(&pos_key(e.points[i]))
                    .copied()
                    .filter(|n| *n != e.from && *n != e.to)
                    .map(|n| (i, n))
            })
            .collect();
        if cuts.is_empty() {
            out.push(e.clone());
        } else {
            out.extend(split_edge(e, &cuts));
        }
    }
    g.edges = out;
    g
}

/// Key of a GeoJSON feature for the merge: its identifier, else its OSM way, else its
/// geometry.
fn feature_key(f: &Value) -> String {
    let p = &f["properties"];
    for k in ["id", "fee_id", "osm_way_id"] {
        match &p[k] {
            Value::String(s) if !s.is_empty() => return format!("{k}:{s}"),
            Value::Number(n) => return format!("{k}:{n}"),
            _ => {}
        }
    }
    format!("g:{}", f["geometry"])
}

fn read_json(path: &Path) -> Result<Value, TrackingError> {
    let text = std::fs::read_to_string(path).map_err(invalid)?;
    serde_json::from_str(&text).map_err(invalid)
}

/// Replaces the link of the view at `at` by `value` (written atomically).
fn replace(at: &Path, value: &Value) -> Result<(), TrackingError> {
    if at.symlink_metadata().is_ok() {
        std::fs::remove_file(at).map_err(invalid)?;
    }
    skimap_common::output::write_json(at, value).map_err(invalid)
}

/// The pipeline view of `primary` joined with `others` under `root` (see the module
/// documentation).
pub fn build_view(primary: &Pack, others: &[Pack], root: &Path) -> Result<PackView, TrackingError> {
    let view = PackView::build(primary, root).map_err(invalid)?;
    if others.is_empty() {
        return Ok(view);
    }
    let base = root.join("out").join(&primary.manifest().site.id);
    // Layers by origin, the primary's first.
    let mut by_origin: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for p in std::iter::once(primary).chain(others) {
        for l in &p.manifest().layers {
            let ext = |s: &str| Path::new(s).extension().map(|e| e.to_ascii_lowercase());
            if ext(&l.file) != ext(&l.origin) {
                continue;
            }
            if let Ok(path) = p.path(&l.file) {
                by_origin.entry(l.origin.clone()).or_default().push(path);
            }
        }
    }
    for (origin, files) in &by_origin {
        let at = base.join(origin);
        if l_is(origin, ".geojson") {
            let mut seen: HashSet<String> = HashSet::new();
            let mut feats: Vec<Value> = Vec::new();
            for f in files {
                let v = read_json(f)?;
                for feat in v["features"].as_array().into_iter().flatten() {
                    if seen.insert(feature_key(feat)) {
                        feats.push(feat.clone());
                    }
                }
            }
            if files.len() > 1 {
                if at.symlink_metadata().is_ok() {
                    std::fs::remove_file(&at).map_err(invalid)?;
                }
                skimap_common::output::write_geojson(&at, &feats).map_err(invalid)?;
            } else if at.symlink_metadata().is_err() {
                // A layer only a neighbour has.
                skimap_common::output::write_geojson(&at, &feats).map_err(invalid)?;
            }
        } else if origin.ends_with("piste_graph.json") && files.len() > 1 {
            let mut datas = Vec::with_capacity(files.len());
            for f in files {
                datas.push(serde_json::from_value::<GraphData>(read_json(f)?).map_err(invalid)?);
            }
            let merged = merge_graphs(&datas[0], &datas[1..]);
            replace(&at, &serde_json::to_value(&merged).map_err(invalid)?)?;
        } else if origin.ends_with("rules.json") && files.len() > 1 {
            let mut v = read_json(&files[0])?;
            let mut seen: HashSet<String> = HashSet::new();
            let mut rules: Vec<Value> = Vec::new();
            for f in files {
                let o = read_json(f)?;
                for r in o["rules"].as_array().into_iter().flatten() {
                    if seen.insert(r["id"].to_string()) {
                        rules.push(r.clone());
                    }
                }
            }
            v["rules"] = Value::Array(rules);
            replace(&at, &v)?;
        }
    }
    Ok(view)
}

fn l_is(origin: &str, ext: &str) -> bool {
    origin.to_ascii_lowercase().ends_with(ext)
}

/// The trail graph of the pack in `pack_dir` joined with the packs `others` (directories).
pub fn load_graph_set(
    pack_dir: &Path,
    others: &[String],
) -> Result<skimap_core::graph::PisteGraph, TrackingError> {
    let pack = Pack::open(pack_dir.to_path_buf()).map_err(invalid)?;
    let ns = neighbours(&pack, others);
    let primary = graph_data(&pack)?;
    let rest: Vec<GraphData> = ns.iter().filter_map(|p| graph_data(p).ok()).collect();
    skimap_core::graph::PisteGraph::new(merge_graphs(&primary, &rest)).map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_core::graph::{EdgeAttrs, EdgeKind};

    fn node(id: i64, lon: f64) -> Node {
        Node {
            pos: LonLat::new(lon, 46.0),
            osm_node_id: Some(id),
        }
    }

    fn edge(from: usize, to: usize, way: i64, lons: &[f64]) -> Edge {
        Edge {
            from,
            to,
            oneway: false,
            points: lons.iter().map(|l| LonLat::new(*l, 46.0)).collect(),
            attrs: EdgeAttrs {
                kind: EdgeKind::Piste,
                name: None,
                osm_way_id: Some(way),
                classic: None,
                skating: None,
                backcountry: false,
                nordic: true,
                hike: false,
                difficulty: None,
                difficulty_max: None,
            },
            profile: Some(Profile {
                dist_m: vec![0.0, 1000.0, 2000.0],
                elev_m: vec![1000.0, 1100.0, 1200.0],
                filled: 0,
            }),
        }
    }

    /// Pack A has way 1 whole (0 → 0.02); pack B has way 1 split at 0.01 where its own
    /// way 2 starts. Merged: way 1 split once at the shared node, way 2 attached there,
    /// no doubled edge, profiles cut at the split.
    #[test]
    fn neighbour_trail_joins_at_its_junction() {
        let a = GraphData {
            site: "a".into(),
            nodes: vec![node(10, 0.0), node(12, 0.02)],
            edges: vec![edge(0, 1, 1, &[0.0, 0.01, 0.02])],
            elevation_source: None,
        };
        let b = GraphData {
            site: "b".into(),
            nodes: vec![
                node(10, 0.0),
                node(11, 0.01),
                node(12, 0.02),
                node(13, 0.03),
            ],
            edges: vec![
                edge(0, 1, 1, &[0.0, 0.01]),
                edge(1, 2, 1, &[0.01, 0.02]),
                edge(1, 3, 2, &[0.01, 0.02, 0.03]),
            ],
            elevation_source: None,
        };
        let m = merge_graphs(&a, &[b]);
        assert_eq!(m.nodes.len(), 4);
        let way1: Vec<&Edge> = m
            .edges
            .iter()
            .filter(|e| e.attrs.osm_way_id == Some(1))
            .collect();
        assert_eq!(way1.len(), 2, "way 1 split once, not doubled");
        let mid = m
            .nodes
            .iter()
            .position(|n| n.osm_node_id == Some(11))
            .unwrap();
        assert!(way1.iter().any(|e| e.to == mid) && way1.iter().any(|e| e.from == mid));
        assert!(
            m.edges
                .iter()
                .any(|e| e.attrs.osm_way_id == Some(2) && e.from == mid)
        );
        let first = way1.iter().find(|e| e.to == mid).unwrap();
        let p = first.profile.as_ref().unwrap();
        assert_eq!(p.dist_m[0], 0.0);
        assert!(p.dist_m.windows(2).all(|w| w[1] > w[0]));
        skimap_core::graph::PisteGraph::new(m).expect("valid merged graph");
    }

    #[test]
    fn alone_the_graph_is_unchanged() {
        let a = GraphData {
            site: "a".into(),
            nodes: vec![node(10, 0.0), node(12, 0.02)],
            edges: vec![edge(0, 1, 1, &[0.0, 0.01, 0.02])],
            elevation_source: None,
        };
        assert_eq!(merge_graphs(&a, &[]), a);
    }
}
