//! P3 (#16): grouping pistes into cross-country ski areas ("domaines").
//!
//! OpenSkiMap grows areas by proximity of 0.5 km step by step; across a col this merges
//! two domains of neighbouring valleys into one area (report 07). Our grouping uses the
//! topology of the network, a shorter proximity threshold, the operators' own groupings
//! and a rule against long single liaisons:
//!
//! 1. **Network.** Graph of all pistes (nordic, hike, snowshoe) split at junctions; two
//!    networks closer than [`Params::gap_m`] (150 m) are joined by a connector.
//! 2. **Site seeds.** Each OSM `site=piste` relation with nordic content is a seed: its
//!    ways (those in no other site) start at distance 0. Two different sites are never
//!    merged.
//! 3. **Network cores.** A *bridge* is an edge whose removal disconnects the network. A
//!    bridge is a *liaison bridge* when both sides weigh at least [`Params::core_min_km`].
//!    Removing liaison bridges leaves pieces; pieces of at least `core_min_km` are cores.
//!    A core holding no site seed becomes a seed of its own.
//! 4. **Partition.** Multi-source Dijkstra over the network: every node goes to its
//!    nearest seed, so a liaison between two seeds is cut near its middle.
//! 5. **Merging.** Two neighbouring cells, not both sites, merge when the liaison between
//!    them is shorter than [`Params::liaison_min_km`] (6 km): a domain's own sectors are
//!    joined by short liaisons (≈ 4 km between two sectors of one village), whereas a pass
//!    between two valleys is a long single track (≈ 13 km over a col). Merges run shortest first and never put two sites together.
//! 6. **Relation coherence.** A piste relation of at most
//!    [`Params::coherence_max_km`] split between domains is moved whole to the domain
//!    holding most of its length, so the cut of a liaison falls between named pistes
//!    (a named piste up to a col stays whole; the cut lands at the col).
//!
//! The length rule stands in for terrain: once the relief (P5) is available, a liaison
//! should be cut at its highest point when it crosses a col.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};

use rstar::{RTree, primitives::GeomWithData};
use skimap_common::geom::LonLat;

use crate::graph::{Edge, EdgeKind, Graph, search_box};
use crate::pistes::PisteSet;

/// Parameters of the grouping.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Params {
    /// Largest gap bridged between two separate networks, m (OpenSkiMap: 500).
    pub gap_m: f64,
    /// Smallest network on each side of a bridge for it to be a liaison, km.
    pub core_min_km: f64,
    /// Liaisons at least this long separate two domains, km.
    pub liaison_min_km: f64,
    /// Longest piste relation kept whole by the coherence step, km.
    pub coherence_max_km: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            gap_m: 150.0,
            core_min_km: 5.0,
            liaison_min_km: 6.0,
            coherence_max_km: 25.0,
        }
    }
}

/// How a domain was seeded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Seed {
    /// An OSM `site=piste` relation.
    Site {
        /// Relation id.
        relation: i64,
        /// Relation name.
        name: Option<String>,
    },
    /// A network core without site relation, identified by its smallest way id.
    Network {
        /// Smallest OSM way id of the core.
        way: i64,
    },
}

/// A domain: a set of pistes.
#[derive(Debug, Clone, PartialEq)]
pub struct Domain {
    /// Stable identifier: `site-<relation>` or `net-<smallest way id>`.
    pub id: String,
    /// Seed of the cell that gave the domain its identity.
    pub seed: Seed,
    /// Other seeds merged into it (network cores joined by short liaisons).
    pub merged: Vec<Seed>,
    /// Indices of its pistes in the [`PisteSet`].
    pub pistes: Vec<usize>,
}

/// Result of [`cluster`], with the intermediate figures.
#[derive(Debug, Clone)]
pub struct Clustering {
    /// Domains, ordered by id.
    pub domains: Vec<Domain>,
    /// Domain index of every piste.
    pub piste_domain: Vec<usize>,
    /// Connectors added between networks.
    pub connectors: usize,
    /// Liaison bridges found.
    pub liaison_bridges: usize,
    /// Liaisons kept as boundaries between two cells: (cell a, cell b, length km).
    pub separations: Vec<(String, String, f64)>,
    /// Merges done: (kept cell, merged cell, liaison length km).
    pub merges: Vec<(String, String, f64)>,
    /// Pistes moved by the relation coherence step.
    pub coherence_moves: usize,
}

type Pt = GeomWithData<[f64; 2], (usize, usize)>;

/// Joins separate networks closer than `gap_m` by connectors between the nearest graph
/// nodes of the two closest edges. Returns the number of connectors.
fn connect_components(g: &mut Graph, gap_m: f64) -> usize {
    let (comp, _) = g.components();
    let edge_comp: Vec<usize> = g.edges.iter().map(|e| comp[e.from]).collect();
    let tree: RTree<Pt> = RTree::bulk_load(
        g.edges
            .iter()
            .enumerate()
            .flat_map(|(ei, e)| {
                e.points
                    .iter()
                    .enumerate()
                    .map(move |(pi, p)| Pt::new([p.lon, p.lat], (ei, pi)))
            })
            .collect(),
    );
    // Closest pair of points per pair of components.
    // (distance, (edge, point) on the first component, (edge, point) on the second).
    type Closest = (f64, (usize, usize), (usize, usize));
    let mut best: HashMap<(usize, usize), Closest> = HashMap::new();
    for (ei, e) in g.edges.iter().enumerate() {
        let ca = edge_comp[ei];
        for (pi, p) in e.points.iter().enumerate() {
            for q in tree.locate_in_envelope(&search_box(*p, gap_m)) {
                let (ej, pj) = q.data;
                let cb = edge_comp[ej];
                if cb <= ca {
                    continue;
                }
                let d = p.haversine_m(g.edges[ej].points[pj]);
                if d > gap_m {
                    continue;
                }
                let entry = best.entry((ca, cb)).or_insert((f64::MAX, (0, 0), (0, 0)));
                if d < entry.0 {
                    *entry = (d, (ei, pi), (ej, pj));
                }
            }
        }
    }
    let mut pairs: Vec<_> = best.into_iter().collect();
    pairs.sort_by_key(|(k, _)| *k);
    let n = pairs.len();
    for (_, (d, (ea, pa), (eb, pb))) in pairs {
        let (na, la) = nearest_end(&g.edges[ea], pa);
        let (nb, lb) = nearest_end(&g.edges[eb], pb);
        let (p, q) = (g.nodes[na].pos, g.nodes[nb].pos);
        g.edges.push(Edge {
            kind: EdgeKind::Connector,
            from: na,
            to: nb,
            points: vec![p, q],
            length_m: d + la + lb,
            oneway: false,
        });
    }
    n
}

/// End node of `e` nearest (along the edge) to its point `pi`, and that distance.
fn nearest_end(e: &Edge, pi: usize) -> (usize, f64) {
    let along = |a: usize, b: usize| -> f64 {
        e.points[a.min(b)..=a.max(b)]
            .windows(2)
            .map(|w| w[0].haversine_m(w[1]))
            .sum()
    };
    let to_start = along(0, pi);
    let to_end = along(pi, e.points.len() - 1);
    if to_start <= to_end {
        (e.from, to_start)
    } else {
        (e.to, to_end)
    }
}

/// Liaison bridges: bridges with at least `core_min_m` of network on each side.
fn liaison_bridges(g: &Graph, core_min_m: f64) -> Vec<bool> {
    let is_bridge = g.bridges();
    let adj = g.adjacency();
    let n = g.nodes.len();
    // Blocks: components without bridges.
    let mut block = vec![usize::MAX; n];
    let mut nb = 0;
    for s in 0..n {
        if block[s] != usize::MAX {
            continue;
        }
        block[s] = nb;
        let mut st = vec![s];
        while let Some(u) = st.pop() {
            for &(e, v) in &adj[u] {
                if !is_bridge[e] && block[v] == usize::MAX {
                    block[v] = nb;
                    st.push(v);
                }
            }
        }
        nb += 1;
    }
    let mut weight = vec![0.0; nb];
    let mut tree: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nb];
    for (e, edge) in g.edges.iter().enumerate() {
        if is_bridge[e] {
            let (a, b) = (block[edge.from], block[edge.to]);
            tree[a].push((e, b));
            tree[b].push((e, a));
        } else {
            weight[block[edge.from]] += edge.length_m;
        }
    }
    // Subtree sums over the bridge forest (iterative post-order).
    let mut sub = vec![0.0; nb];
    let mut parent_edge = vec![usize::MAX; nb];
    let mut root_of = vec![usize::MAX; nb];
    let mut order = Vec::with_capacity(nb);
    for r in 0..nb {
        if root_of[r] != usize::MAX {
            continue;
        }
        root_of[r] = r;
        let mut st = vec![r];
        while let Some(u) = st.pop() {
            order.push(u);
            for &(e, v) in &tree[u] {
                if root_of[v] == usize::MAX {
                    root_of[v] = r;
                    parent_edge[v] = e;
                    st.push(v);
                }
            }
        }
    }
    for &u in order.iter().rev() {
        sub[u] += weight[u];
        let pe = parent_edge[u];
        if pe != usize::MAX {
            let edge = &g.edges[pe];
            let p = if block[edge.from] == u {
                block[edge.to]
            } else {
                block[edge.from]
            };
            let s = sub[u] + edge.length_m;
            sub[p] += s;
        }
    }
    let mut out = vec![false; g.edges.len()];
    for (u, &pe) in parent_edge.iter().enumerate() {
        if pe == usize::MAX {
            continue;
        }
        let total = sub[root_of[u]];
        let child = sub[u];
        let other = total - child - g.edges[pe].length_m;
        out[pe] = child >= core_min_m && other >= core_min_m;
    }
    out
}

struct UnionFind {
    parent: Vec<usize>,
    site: Vec<Option<usize>>,
}

impl UnionFind {
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
}

/// Groups the pistes of `set` into domains.
pub fn cluster(set: &PisteSet, params: &Params) -> Clustering {
    let mut g = Graph::build(set, 0.0);
    let connectors = connect_components(&mut g, params.gap_m);
    let adj = g.adjacency();
    let n = g.nodes.len();
    let liaison = liaison_bridges(&g, params.core_min_km * 1000.0);

    // Pieces: components without liaison bridges.
    let mut piece = vec![usize::MAX; n];
    let mut n_pieces = 0;
    for s in 0..n {
        if piece[s] != usize::MAX {
            continue;
        }
        piece[s] = n_pieces;
        let mut st = vec![s];
        while let Some(u) = st.pop() {
            for &(e, v) in &adj[u] {
                if !liaison[e] && piece[v] == usize::MAX {
                    piece[v] = n_pieces;
                    st.push(v);
                }
            }
        }
        n_pieces += 1;
    }
    let mut piece_m = vec![0.0; n_pieces];
    let mut piece_min_way = vec![i64::MAX; n_pieces];
    for e in &g.edges {
        if piece[e.from] == piece[e.to] {
            piece_m[piece[e.from]] += e.length_m;
        }
        if let EdgeKind::Piste { piste } = e.kind {
            let p = piece[e.from];
            piece_min_way[p] = piece_min_way[p].min(set.pistes[piste].way_id);
        }
    }

    // Site seeds: a way seeds the most specific site relation (with nordic content)
    // containing it; ways shared by unrelated sites seed none.
    let piste_of_way: HashMap<i64, usize> = set
        .pistes
        .iter()
        .enumerate()
        .map(|(i, p)| (p.way_id, i))
        .collect();
    let mut sites_of_piste: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut seeds: Vec<Seed> = Vec::new();
    for s in &set.sites {
        let members: Vec<usize> = s
            .ways
            .iter()
            .filter_map(|w| piste_of_way.get(w).copied())
            .collect();
        if !members.iter().any(|&i| set.pistes[i].is_nordic()) {
            continue;
        }
        let k = seeds.len();
        seeds.push(Seed::Site {
            relation: s.id,
            name: s.name.clone(),
        });
        for i in members {
            sites_of_piste.entry(i).or_default().push(k);
        }
    }
    let site_id: Vec<i64> = seeds
        .iter()
        .map(|s| match s {
            Seed::Site { relation, .. } => *relation,
            Seed::Network { .. } => 0,
        })
        .collect();
    let parents_of: HashMap<i64, &Vec<i64>> =
        set.sites.iter().map(|s| (s.id, &s.parents)).collect();
    for ks in sites_of_piste.values_mut() {
        let all = ks.clone();
        // Drop every site that encloses another site of the list.
        ks.retain(|&k| {
            !all.iter().any(|&j| {
                j != k
                    && parents_of
                        .get(&site_id[j])
                        .is_some_and(|p| p.contains(&site_id[k]))
            })
        });
    }
    let n_sites = seeds.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut label = vec![usize::MAX; n];
    let mut seeded_piece = vec![false; n_pieces];
    for e in &g.edges {
        let EdgeKind::Piste { piste } = e.kind else {
            continue;
        };
        if let Some(ks) = sites_of_piste.get(&piste)
            && ks.len() == 1
        {
            for v in [e.from, e.to] {
                dist[v] = 0.0;
                label[v] = ks[0];
                seeded_piece[piece[v]] = true;
            }
        }
    }
    // Core seeds.
    let mut core_seed = vec![usize::MAX; n_pieces];
    for p in 0..n_pieces {
        if !seeded_piece[p] && piece_m[p] >= params.core_min_km * 1000.0 {
            core_seed[p] = seeds.len();
            seeds.push(Seed::Network {
                way: piece_min_way[p],
            });
        }
    }
    for v in 0..n {
        let s = core_seed[piece[v]];
        if s != usize::MAX {
            dist[v] = 0.0;
            label[v] = s;
        }
    }

    // Multi-source Dijkstra.
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = (0..n)
        .filter(|&v| dist[v] == 0.0)
        .map(|v| Reverse((0, v)))
        .collect();
    let key = |d: f64| (d * 1000.0) as u64;
    while let Some(Reverse((dk, u))) = heap.pop() {
        if dk > key(dist[u]) {
            continue;
        }
        for &(e, v) in &adj[u] {
            let nd = dist[u] + g.edges[e].length_m;
            if nd < dist[v] {
                dist[v] = nd;
                label[v] = label[u];
                heap.push(Reverse((key(nd), v)));
            }
        }
    }
    // Unreached nodes: small isolated networks, one seed per component.
    let (comp, _) = g.components();
    let mut comp_seed: HashMap<usize, usize> = HashMap::new();
    for e in &g.edges {
        if label[e.from] != usize::MAX {
            continue;
        }
        let c = comp[e.from];
        let way = match e.kind {
            EdgeKind::Piste { piste } => set.pistes[piste].way_id,
            EdgeKind::Connector => i64::MAX,
        };
        match comp_seed.get(&c) {
            Some(&k) => {
                if let Seed::Network { way: w } = &mut seeds[k] {
                    *w = (*w).min(way);
                }
            }
            None => {
                comp_seed.insert(c, seeds.len());
                seeds.push(Seed::Network { way });
            }
        }
    }
    for v in 0..n {
        if label[v] == usize::MAX {
            label[v] = comp_seed[&comp[v]];
            dist[v] = 0.0;
        }
    }

    // Merging across short liaisons.
    let mut liaisons: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for e in &g.edges {
        let (a, b) = (label[e.from], label[e.to]);
        if a == b {
            continue;
        }
        let l = dist[e.from] + e.length_m + dist[e.to];
        let k = (a.min(b), a.max(b));
        let entry = liaisons.entry(k).or_insert(f64::INFINITY);
        *entry = entry.min(l);
    }
    let mut uf = UnionFind {
        parent: (0..seeds.len()).collect(),
        site: (0..seeds.len())
            .map(|k| (k < n_sites).then_some(k))
            .collect(),
    };
    let mut pairs: Vec<((usize, usize), f64)> = liaisons.into_iter().collect();
    pairs.sort_by(|x, y| x.1.total_cmp(&y.1).then(x.0.cmp(&y.0)));
    let cell_id = |k: usize, seeds: &[Seed]| seed_id(&seeds[k]);
    let mut merges = Vec::new();
    let mut separations = Vec::new();
    for ((a, b), l) in pairs {
        let (ra, rb) = (uf.find(a), uf.find(b));
        if ra == rb {
            continue;
        }
        let both_sites = uf.site[ra].is_some() && uf.site[rb].is_some();
        if both_sites || l >= params.liaison_min_km * 1000.0 {
            separations.push((cell_id(a, &seeds), cell_id(b, &seeds), l / 1000.0));
            continue;
        }
        // Keep the site (or the heavier identity) as root.
        let (keep, gone) = if uf.site[rb].is_some() && uf.site[ra].is_none() {
            (rb, ra)
        } else {
            (ra, rb)
        };
        uf.parent[gone] = keep;
        if uf.site[keep].is_none() {
            uf.site[keep] = uf.site[gone];
        }
        merges.push((cell_id(keep, &seeds), cell_id(gone, &seeds), l / 1000.0));
    }

    // Piste labels: a seed way keeps its site; other edges take the label of their nearer
    // end; a way goes to the majority of its edge length.
    let mut piste_votes: Vec<HashMap<usize, f64>> = vec![HashMap::new(); set.pistes.len()];
    for e in &g.edges {
        if let EdgeKind::Piste { piste } = e.kind {
            let lab = match sites_of_piste.get(&piste) {
                Some(ks) if ks.len() == 1 => ks[0],
                _ if dist[e.from] <= dist[e.to] => label[e.from],
                _ => label[e.to],
            };
            let root = uf.find(lab);
            *piste_votes[piste].entry(root).or_default() += e.length_m.max(1e-6);
        }
    }
    let mut piste_cell: Vec<usize> = piste_votes
        .iter()
        .map(|v| {
            v.iter()
                .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(k, _)| *k)
                .unwrap_or(usize::MAX)
        })
        .collect();

    // Relation coherence, longest relations first so that shorter ones win.
    let mut rels: Vec<(usize, f64)> = set
        .relations
        .iter()
        .enumerate()
        .map(|(ri, _)| {
            let km = set
                .pistes
                .iter()
                .filter(|p| p.relations.contains(&ri))
                .map(|p| p.length_m)
                .sum::<f64>()
                / 1000.0;
            (ri, km)
        })
        .filter(|(_, km)| *km > 0.0 && *km <= params.coherence_max_km)
        .collect();
    rels.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); set.relations.len()];
    for (pi, p) in set.pistes.iter().enumerate() {
        for &ri in &p.relations {
            members[ri].push(pi);
        }
    }
    let mut coherence_moves = 0;
    for (ri, _) in rels {
        let mut by_cell: BTreeMap<usize, f64> = BTreeMap::new();
        for &pi in &members[ri] {
            *by_cell.entry(piste_cell[pi]).or_default() += set.pistes[pi].length_m;
        }
        if by_cell.len() < 2 {
            continue;
        }
        let major = by_cell
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(k, _)| *k)
            .expect("non-empty");
        for &pi in &members[ri] {
            // Ways of exactly one site stay in that site.
            let pinned = sites_of_piste
                .get(&pi)
                .is_some_and(|ks| ks.len() == 1 && uf.find(ks[0]) != major);
            if piste_cell[pi] != major && !pinned {
                piste_cell[pi] = major;
                coherence_moves += 1;
            }
        }
    }

    // Domains.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (pi, &c) in piste_cell.iter().enumerate() {
        if c != usize::MAX {
            groups.entry(c).or_default().push(pi);
        }
    }
    let mut merged_into: BTreeMap<usize, Vec<Seed>> = BTreeMap::new();
    for (k, seed) in seeds.iter().enumerate() {
        let r = uf.find(k);
        if r != k {
            merged_into.entry(r).or_default().push(seed.clone());
        }
    }
    let mut domains: Vec<Domain> = groups
        .into_iter()
        .map(|(root, pistes)| {
            let seed = match &seeds[root] {
                Seed::Network { .. } => Seed::Network {
                    way: pistes
                        .iter()
                        .map(|&i| set.pistes[i].way_id)
                        .min()
                        .unwrap_or(i64::MAX),
                },
                s => s.clone(),
            };
            Domain {
                id: seed_id(&seed),
                merged: merged_into.remove(&root).unwrap_or_default(),
                seed,
                pistes,
            }
        })
        .collect();
    domains.sort_by(|a, b| a.id.cmp(&b.id));
    let mut piste_domain = vec![usize::MAX; set.pistes.len()];
    for (di, d) in domains.iter().enumerate() {
        for &pi in &d.pistes {
            piste_domain[pi] = di;
        }
    }
    Clustering {
        domains,
        piste_domain,
        connectors,
        liaison_bridges: liaison.iter().filter(|b| **b).count(),
        separations,
        merges,
        coherence_moves,
    }
}

fn seed_id(s: &Seed) -> String {
    match s {
        Seed::Site { relation, .. } => format!("site-{relation}"),
        Seed::Network { way } => format!("net-{way}"),
    }
}

impl Domain {
    /// Total length of the pistes matching `f`, km.
    pub fn km(&self, set: &PisteSet, f: impl Fn(&crate::pistes::Piste) -> bool) -> f64 {
        self.pistes
            .iter()
            .map(|&i| &set.pistes[i])
            .filter(|p| f(p))
            .map(|p| p.length_m)
            .sum::<f64>()
            / 1000.0
            + 0.0
    }

    /// Point of the network nearest to the length-weighted centroid of its pistes.
    pub fn representative_point(&self, set: &PisteSet) -> LonLat {
        let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
        for &i in &self.pistes {
            let p = &set.pistes[i];
            for w in p.points.windows(2) {
                let l = w[0].1.haversine_m(w[1].1);
                sx += l * (w[0].1.lon + w[1].1.lon) / 2.0;
                sy += l * (w[0].1.lat + w[1].1.lat) / 2.0;
                sw += l;
            }
        }
        let first = set.pistes[self.pistes[0]].points[0].1;
        if sw == 0.0 {
            return first;
        }
        let c = LonLat::new(sx / sw, sy / sw);
        self.pistes
            .iter()
            .flat_map(|&i| set.pistes[i].points.iter().map(|(_, p)| *p))
            .min_by(|a, b| a.haversine_m(c).total_cmp(&b.haversine_m(c)))
            .unwrap_or(first)
    }

    /// Distinct names of the piste relations of the domain.
    pub fn relation_names(&self, set: &PisteSet) -> BTreeSet<String> {
        self.pistes
            .iter()
            .flat_map(|&i| set.pistes[i].relations.iter())
            .filter_map(|&ri| set.relations[ri].name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::M_PER_DEG_LAT;
    use crate::parse::{Member, MemberKind, OsmData, Relation, Tags, Way};
    use crate::pistes::classify;

    /// A builder of synthetic networks on a metric grid near 45° N.
    struct Net {
        nodes: Vec<(i64, LonLat)>,
        ways: Vec<Way>,
        relations: Vec<Relation>,
    }

    impl Net {
        fn new() -> Self {
            Net {
                nodes: Vec::new(),
                ways: Vec::new(),
                relations: Vec::new(),
            }
        }

        /// Node at (x, y) metres from the origin.
        fn node(&mut self, id: i64, x: f64, y: f64) {
            let lat = 45.0 + y / M_PER_DEG_LAT;
            let lon = 6.0 + x / (M_PER_DEG_LAT * 45f64.to_radians().cos());
            self.nodes.push((id, LonLat::new(lon, lat)));
        }

        fn way(&mut self, id: i64, nodes: &[i64]) {
            self.ways.push(Way {
                id,
                nodes: nodes.to_vec(),
                tags: Tags(vec![("piste:type".into(), "nordic".into())]),
            });
        }

        /// A square loop of side `s` metres at (x, y), node ids from `id0`, way `wid`.
        fn square(&mut self, id0: i64, wid: i64, x: f64, y: f64, s: f64) {
            self.node(id0, x, y);
            self.node(id0 + 1, x + s, y);
            self.node(id0 + 2, x + s, y + s);
            self.node(id0 + 3, x, y + s);
            self.way(wid, &[id0, id0 + 1, id0 + 2, id0 + 3, id0]);
        }

        fn site(&mut self, id: i64, ways: &[i64], name: &str) {
            self.relations.push(Relation {
                id,
                members: ways
                    .iter()
                    .map(|&w| Member {
                        kind: MemberKind::Way,
                        id: w,
                        role: String::new(),
                    })
                    .collect(),
                tags: Tags(vec![
                    ("type".into(), "site".into()),
                    ("site".into(), "piste".into()),
                    ("name".into(), name.into()),
                ]),
            });
        }

        fn data(self) -> OsmData {
            OsmData::from_elements(self.nodes, self.ways, self.relations)
        }
    }

    /// Two 8 km loops (2 km squares) joined by a straight liaison of `liaison_m`.
    fn two_cores(liaison_m: f64) -> OsmData {
        let mut n = Net::new();
        n.square(1, 1, 0.0, 0.0, 2000.0);
        n.square(11, 2, 2000.0 + liaison_m, 0.0, 2000.0);
        // Liaison from node 2 (2000, 0) to node 11.
        n.way(3, &[2, 11]);
        n.data()
    }

    #[test]
    fn long_liaison_separates_short_one_joins() {
        let p = Params::default();
        let set = classify(&two_cores(13_000.0));
        let c = cluster(&set, &p);
        assert_eq!(c.domains.len(), 2, "{:?}", c.separations);
        assert_eq!(c.liaison_bridges, 1);
        assert_eq!(c.separations.len(), 1);
        assert!((c.separations[0].2 - 13.0).abs() < 0.05);
        // The liaison way goes to one side; each loop stays with its own domain.
        assert_ne!(c.piste_domain[0], c.piste_domain[1]);

        let set = classify(&two_cores(4_000.0));
        let c = cluster(&set, &p);
        assert_eq!(c.domains.len(), 1);
        assert_eq!(c.merges.len(), 1);
        assert_eq!(c.domains[0].merged.len(), 1);
        assert_eq!(c.domains[0].pistes.len(), 3);
    }

    #[test]
    fn gaps_shorter_than_threshold_are_bridged() {
        let mut n = Net::new();
        n.square(1, 1, 0.0, 0.0, 1000.0);
        n.square(11, 2, 1100.0, 0.0, 1000.0); // 100 m gap
        n.square(21, 3, 2400.0, 0.0, 1000.0); // 300 m gap
        let set = classify(&n.data());
        let c = cluster(&set, &Params::default());
        assert_eq!(c.connectors, 1);
        assert_eq!(c.domains.len(), 2);
        assert_eq!(c.piste_domain[0], c.piste_domain[1]);
        assert_ne!(c.piste_domain[1], c.piste_domain[2]);
    }

    #[test]
    fn two_sites_never_merge_and_split_a_shared_network() {
        // Three adjacent loops sharing edges: site A holds loop 1, site B loop 3, loop 2 in
        // between belongs to no site and is nearer to B.
        let mut n = Net::new();
        n.square(1, 1, 0.0, 0.0, 1000.0);
        n.square(11, 2, 1000.0, 0.0, 1000.0);
        n.square(21, 3, 2000.0, 0.0, 500.0);
        // Join the loops at shared nodes.
        n.way(4, &[2, 11]);
        n.way(5, &[12, 21]);
        n.site(100, &[1], "A");
        n.site(200, &[3], "B");
        let set = classify(&n.data());
        let c = cluster(&set, &Params::default());
        let ids: Vec<&str> = c.domains.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec!["site-100", "site-200"]);
        let a = c.piste_domain[0];
        let b = c.piste_domain[2];
        assert_ne!(a, b);
        assert!(matches!(&c.domains[a].seed, Seed::Site { name: Some(x), .. } if x == "A"));
        assert!(
            c.separations
                .iter()
                .any(|s| s.0 == "site-100" && s.1 == "site-200")
        );
    }

    #[test]
    fn sub_site_keeps_its_ways() {
        // Site B lists loops 1 and 2; site A lists loop 1 only: A is a sub-site of B, so
        // loop 1 seeds A and loop 2 seeds B. The loops touch (shared node), the two sites
        // stay apart.
        let mut n = Net::new();
        n.square(1, 1, 0.0, 0.0, 2000.0);
        n.square(11, 2, 2000.0, 0.0, 2000.0);
        n.way(3, &[2, 11]);
        n.site(100, &[1], "A");
        n.site(200, &[1, 2, 3], "B");
        let set = classify(&n.data());
        assert_eq!(set.sites[0].parents, vec![200]);
        let c = cluster(&set, &Params::default());
        let ids: Vec<&str> = c.domains.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec!["site-100", "site-200"]);
        assert_eq!(c.domains[c.piste_domain[0]].id, "site-100");
        assert_eq!(c.domains[c.piste_domain[1]].id, "site-200");
    }

    #[test]
    fn coherence_keeps_named_piste_whole() {
        // A 10 km liaison between cores A (x <= 2 km) and B (x >= 12 km): a named piste of
        // two ways (4.5 km then 4 km) from A, then 1.5 km of unnamed road to B. The
        // nearest-seed cut gives the second way of the piste to B; coherence moves it
        // back so that the boundary falls at the end of the named piste.
        let mut n = Net::new();
        n.square(1, 1, 0.0, 0.0, 2000.0);
        n.square(11, 2, 12_000.0, 0.0, 2000.0);
        n.node(50, 6500.0, 0.0);
        n.node(51, 10_500.0, 0.0);
        n.way(3, &[2, 50]);
        n.way(5, &[50, 51]);
        n.way(4, &[51, 11]);
        let member = |id| Member {
            kind: MemberKind::Way,
            id,
            role: String::new(),
        };
        n.relations.push(Relation {
            id: 300,
            members: vec![member(3), member(5)],
            tags: Tags(vec![
                ("type".into(), "route".into()),
                ("route".into(), "piste".into()),
                ("piste:type".into(), "nordic".into()),
                ("name".into(), "Izoard".into()),
            ]),
        });
        let set = classify(&n.data());
        let c = cluster(&set, &Params::default());
        assert_eq!(c.domains.len(), 2);
        // Piste order follows the ways: 1, 2, 3, 5, 4.
        let ids: Vec<i64> = set.pistes.iter().map(|p| p.way_id).collect();
        assert_eq!(ids, vec![1, 2, 3, 5, 4]);
        let d = &c.piste_domain;
        assert_ne!(d[0], d[1]);
        assert_eq!(d[2], d[0], "first way of the piste");
        assert_eq!(d[3], d[0], "second way moved back by coherence");
        assert_eq!(d[4], d[1], "road goes to B");
        assert_eq!(c.coherence_moves, 1);
    }
}
