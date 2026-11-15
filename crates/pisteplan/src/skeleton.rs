//! Skeleton of a line mask (Zhang–Suen thinning) and its graph: nodes at line ends and
//! junctions, edges along the one-pixel skeleton.

use crate::segment::Mask;

/// Zhang–Suen thinning (8-connected one-pixel skeleton).
pub fn thin(m: &Mask) -> Mask {
    let (w, h) = (m.w, m.h);
    let mut v = m.v.clone();
    // Work on the bounding box of the set pixels.
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if v[y * w + x] != 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 > x1 {
        return Mask { w, h, v };
    }
    let at = |v: &[u8], x: usize, y: usize, dx: i64, dy: i64| -> u8 {
        let (xx, yy) = (x as i64 + dx, y as i64 + dy);
        if xx < 0 || yy < 0 || xx as usize >= w || yy as usize >= h {
            0
        } else {
            v[yy as usize * w + xx as usize]
        }
    };
    let mut del = Vec::new();
    loop {
        let mut changed = false;
        for step in 0..2 {
            del.clear();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if v[y * w + x] == 0 {
                        continue;
                    }
                    // P2..P9 clockwise from north.
                    let n = [
                        at(&v, x, y, 0, -1),
                        at(&v, x, y, 1, -1),
                        at(&v, x, y, 1, 0),
                        at(&v, x, y, 1, 1),
                        at(&v, x, y, 0, 1),
                        at(&v, x, y, -1, 1),
                        at(&v, x, y, -1, 0),
                        at(&v, x, y, -1, -1),
                    ];
                    let b: u8 = n.iter().sum();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let a = (0..8).filter(|&i| n[i] == 0 && n[(i + 1) % 8] == 1).count();
                    if a != 1 {
                        continue;
                    }
                    let (p2, p4, p6, p8) = (n[0], n[2], n[4], n[6]);
                    let ok = if step == 0 {
                        p2 * p4 * p6 == 0 && p4 * p6 * p8 == 0
                    } else {
                        p2 * p4 * p8 == 0 && p2 * p6 * p8 == 0
                    };
                    if ok {
                        del.push(y * w + x);
                    }
                }
            }
            for &i in &del {
                v[i] = 0;
            }
            changed |= !del.is_empty();
        }
        if !changed {
            break;
        }
    }
    // Zhang–Suen leaves staircase corners (a pixel whose neighbours touch each other
    // diagonally): remove them in place, so that every line pixel has two neighbours.
    for y in y0..=y1 {
        for x in x0..=x1 {
            if v[y * w + x] == 0 {
                continue;
            }
            let nb: Vec<(i64, i64)> = N8
                .iter()
                .copied()
                .filter(|&(dx, dy)| at(&v, x, y, dx, dy) != 0)
                .collect();
            if nb.len() >= 2 && neighbour_groups(&nb) == 1 {
                v[y * w + x] = 0;
            }
        }
    }
    Mask { w, h, v }
}

/// Number of 8-connected groups among neighbour offsets of a pixel.
fn neighbour_groups(nb: &[(i64, i64)]) -> usize {
    let mut group: Vec<usize> = (0..nb.len()).collect();
    fn find(g: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while g[r] != r {
            r = g[r];
        }
        g[i] = r;
        r
    }
    for i in 0..nb.len() {
        for j in i + 1..nb.len() {
            if (nb[i].0 - nb[j].0).abs() <= 1 && (nb[i].1 - nb[j].1).abs() <= 1 {
                let (a, b) = (find(&mut group, i), find(&mut group, j));
                group[a] = b;
            }
        }
    }
    (0..nb.len()).filter(|&i| find(&mut group, i) == i).count()
}

/// Edge of a skeleton graph: polyline in pixel coordinates from node `a` to node `b`.
#[derive(Debug, Clone, PartialEq)]
pub struct SkelEdge {
    /// First node.
    pub a: usize,
    /// Last node.
    pub b: usize,
    /// Pixel centres from `a` to `b`, both included.
    pub pts: Vec<[f64; 2]>,
}

impl SkelEdge {
    /// Length in pixels.
    pub fn len(&self) -> f64 {
        self.pts
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
            .sum()
    }

    /// Whether the edge has no points.
    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }
}

/// Graph of a skeleton.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkelGraph {
    /// Node positions (pixel centres).
    pub nodes: Vec<[f64; 2]>,
    /// Edges.
    pub edges: Vec<SkelEdge>,
}

const N8: [(i64, i64); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Graph of a one-pixel skeleton. Junction pixels that touch each other form one node;
/// closed loops without junction get one node.
pub fn trace(s: &Mask) -> SkelGraph {
    let (w, h) = (s.w, s.h);
    let deg = |x: i64, y: i64| {
        N8.iter()
            .filter(|(dx, dy)| s.at(x + dx, y + dy) != 0)
            .count()
    };
    // Node pixels: degree != 2.
    let mut node_of = vec![u32::MAX; w * h];
    let mut g = SkelGraph::default();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if s.v[i] == 0 || node_of[i] != u32::MAX || deg(x as i64, y as i64) == 2 {
                continue;
            }
            // Flood the cluster of touching node pixels.
            let id = members.len() as u32;
            let mut cl = vec![i];
            node_of[i] = id;
            let mut k = 0;
            while k < cl.len() {
                let (cx, cy) = ((cl[k] % w) as i64, (cl[k] / w) as i64);
                for (dx, dy) in N8 {
                    let (xx, yy) = (cx + dx, cy + dy);
                    if s.at(xx, yy) != 0 {
                        let j = yy as usize * w + xx as usize;
                        if node_of[j] == u32::MAX && deg(xx, yy) != 2 {
                            node_of[j] = id;
                            cl.push(j);
                        }
                    }
                }
                k += 1;
            }
            let n = cl.len() as f64;
            let cx = cl.iter().map(|&j| (j % w) as f64).sum::<f64>() / n + 0.5;
            let cy = cl.iter().map(|&j| (j / w) as f64).sum::<f64>() / n + 0.5;
            g.nodes.push([cx, cy]);
            members.push(cl);
        }
    }
    let mut visited = vec![false; w * h];
    let px = |i: usize| [(i % w) as f64 + 0.5, (i / w) as f64 + 0.5];
    // Walk from every node pixel along each unvisited path pixel.
    let walk = |start_node: usize,
                first: usize,
                from: usize,
                visited: &mut Vec<bool>,
                node_of: &Vec<u32>|
     -> Option<SkelEdge> {
        let mut pts = vec![px(from), px(first)];
        let (mut prev, mut cur) = (from, first);
        loop {
            if node_of[cur] != u32::MAX {
                let b = node_of[cur] as usize;
                pts.pop();
                pts.push(g_node_placeholder(b));
                return Some(SkelEdge {
                    a: start_node,
                    b,
                    pts,
                });
            }
            visited[cur] = true;
            let (cx, cy) = ((cur % w) as i64, (cur / w) as i64);
            // Prefer a node pixel, then 4-neighbours, then diagonals.
            let mut cands: Vec<usize> = Vec::new();
            for (dx, dy) in N8 {
                let (xx, yy) = (cx + dx, cy + dy);
                if s.at(xx, yy) == 0 {
                    continue;
                }
                let j = yy as usize * w + xx as usize;
                if j == prev || (node_of[j] == u32::MAX && visited[j]) {
                    continue;
                }
                if node_of[j] != u32::MAX && node_of[j] as usize == start_node && pts.len() < 3 {
                    continue;
                }
                cands.push(j);
            }
            cands.sort_by_key(|&j| {
                let d = (j % w) as i64 - cx + ((j / w) as i64 - cy);
                (node_of[j] == u32::MAX, d.abs() != 1)
            });
            let j = *cands.first()?;
            pts.push(px(j));
            prev = cur;
            cur = j;
        }
    };
    for (nid, cl) in members.iter().enumerate() {
        for &p in cl {
            let (x, y) = ((p % w) as i64, (p / w) as i64);
            for (dx, dy) in N8 {
                let (xx, yy) = (x + dx, y + dy);
                if s.at(xx, yy) == 0 {
                    continue;
                }
                let j = yy as usize * w + xx as usize;
                if node_of[j] != u32::MAX || visited[j] {
                    continue;
                }
                if let Some(mut e) = walk(nid, j, p, &mut visited, &node_of) {
                    e.pts[0] = g.nodes[nid];
                    let last = e.pts.len() - 1;
                    e.pts[last] = g.nodes[e.b];
                    g.edges.push(e);
                }
            }
        }
    }
    // Loops without any node.
    for i in 0..w * h {
        if s.v[i] == 0 || visited[i] || node_of[i] != u32::MAX {
            continue;
        }
        let nid = g.nodes.len();
        g.nodes.push(px(i));
        node_of[i] = nid as u32;
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        let first = N8.iter().find_map(|(dx, dy)| {
            let (xx, yy) = (x + dx, y + dy);
            (s.at(xx, yy) != 0).then(|| yy as usize * w + xx as usize)
        });
        if let Some(j) = first
            && let Some(mut e) = walk(nid, j, i, &mut visited, &node_of)
        {
            e.pts[0] = g.nodes[nid];
            let last = e.pts.len() - 1;
            e.pts[last] = g.nodes[e.b];
            g.edges.push(e);
        }
    }
    g
}

fn g_node_placeholder(_b: usize) -> [f64; 2] {
    // Replaced by the node position once the walk returns.
    [f64::NAN, f64::NAN]
}

impl SkelGraph {
    /// Degree of each node.
    pub fn degrees(&self) -> Vec<usize> {
        let mut d = vec![0; self.nodes.len()];
        for e in &self.edges {
            d[e.a] += 1;
            d[e.b] += 1;
        }
        d
    }

    /// Removes spurs (edges to a degree-1 node) shorter than `min_px`, once, then merges
    /// the degree-2 nodes left behind.
    pub fn pruned(mut self, min_px: f64) -> SkelGraph {
        let d = self.degrees();
        // A spur joins a line end to a junction; isolated edges (two ends) stay.
        self.edges
            .retain(|e| !((d[e.a] == 1) != (d[e.b] == 1) && e.len() < min_px));
        self.merge_degree_two()
    }

    /// Joins the edges through every node of degree 2.
    pub fn merge_degree_two(mut self) -> SkelGraph {
        loop {
            let d = self.degrees();
            let Some(n) = (0..self.nodes.len()).find(|&n| {
                d[n] == 2
                    && self.edges.iter().filter(|e| e.a == n || e.b == n).count() == 2
                    && !self.edges.iter().any(|e| e.a == n && e.b == n)
            }) else {
                break;
            };
            let idx: Vec<usize> = (0..self.edges.len())
                .filter(|&i| self.edges[i].a == n || self.edges[i].b == n)
                .collect();
            let (i, j) = (idx[0], idx[1]);
            let mut e1 = self.edges[i].clone();
            let mut e2 = self.edges[j].clone();
            if e1.b != n {
                e1.pts.reverse();
                std::mem::swap(&mut e1.a, &mut e1.b);
            }
            if e2.a != n {
                e2.pts.reverse();
                std::mem::swap(&mut e2.a, &mut e2.b);
            }
            let mut pts = e1.pts;
            pts.extend_from_slice(&e2.pts[1..]);
            let merged = SkelEdge {
                a: e1.a,
                b: e2.b,
                pts,
            };
            let (hi, lo) = (i.max(j), i.min(j));
            self.edges.swap_remove(hi);
            self.edges.swap_remove(lo);
            self.edges.push(merged);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_mask(w: usize, h: usize, rects: &[(usize, usize, usize, usize)]) -> Mask {
        let mut m = Mask::new(w, h);
        for &(x0, y0, x1, y1) in rects {
            for y in y0..y1 {
                for x in x0..x1 {
                    m.v[y * w + x] = 1;
                }
            }
        }
        m
    }

    #[test]
    fn thick_bar_thins_to_a_single_edge() {
        let m = rect_mask(60, 15, &[(5, 5, 55, 10)]);
        let s = thin(&m);
        // One pixel wide: no column has more than one or two pixels.
        for x in 8..52 {
            let n: u8 = (0..15).map(|y| s.v[y * 60 + x]).sum();
            assert!(n == 1, "column {x}: {n}");
        }
        let g = trace(&s);
        assert_eq!(g.edges.len(), 1, "{g:?}");
        assert!(g.edges[0].len() > 40.0);
    }

    #[test]
    fn cross_has_one_junction_and_four_branches() {
        let m = rect_mask(61, 61, &[(5, 28, 56, 33), (28, 5, 33, 56)]);
        let g = trace(&thin(&m)).pruned(5.0);
        let d = g.degrees();
        assert_eq!(d.iter().filter(|&&k| k >= 3).count(), 1, "{d:?}");
        assert_eq!(d.iter().filter(|&&k| k == 1).count(), 4, "{d:?}");
        assert_eq!(g.edges.len(), 4);
        let j = d.iter().position(|&k| k >= 3).unwrap();
        let p = g.nodes[j];
        assert!(
            (p[0] - 30.5).abs() < 2.5 && (p[1] - 30.5).abs() < 2.5,
            "{p:?}"
        );
    }

    #[test]
    fn ring_is_one_closed_edge() {
        let mut m = Mask::new(50, 50);
        for y in 0..50 {
            for x in 0..50 {
                let r = ((x as f64 - 25.0).powi(2) + (y as f64 - 25.0).powi(2)).sqrt();
                if (15.0..19.0).contains(&r) {
                    m.v[y * 50 + x] = 1;
                }
            }
        }
        let g = trace(&thin(&m));
        assert_eq!(g.edges.len(), 1, "{g:?}");
        assert_eq!(g.edges[0].a, g.edges[0].b);
        assert!(g.edges[0].len() > 90.0);
    }
}
