//! Gradient boosted decision trees for binary classification (logistic loss, second-order
//! leaf values, depth-limited trees, exact split search on pre-sorted features). Small,
//! dependency-free and deterministic: training and inference of the matching model
//! (a few thousand candidate pairs, twenty descriptors) take milliseconds.

use serde::{Deserialize, Serialize};

/// Training parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Params {
    /// Number of trees.
    pub trees: usize,
    /// Depth of each tree.
    pub depth: usize,
    /// Learning rate (shrinkage).
    pub learning_rate: f64,
    /// Smallest sum of hessians in a leaf.
    pub min_child_weight: f64,
    /// L2 regularisation of the leaf values.
    pub lambda: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            trees: 200,
            depth: 3,
            learning_rate: 0.1,
            min_child_weight: 1.0,
            lambda: 1.0,
        }
    }
}

/// A node of a tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// `x[feature] < threshold` goes left.
    Split {
        /// Feature index.
        feature: usize,
        /// Threshold.
        threshold: f64,
        /// Left child.
        left: usize,
        /// Right child.
        right: usize,
    },
    /// Leaf value (log-odds increment).
    Leaf(f64),
}

/// A regression tree on the log-odds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    /// Nodes, root first.
    pub nodes: Vec<Node>,
}

impl Tree {
    /// Value of the leaf reached by `x`.
    pub fn eval(&self, x: &[f64]) -> f64 {
        let mut i = 0;
        loop {
            match &self.nodes[i] {
                Node::Leaf(v) => return *v,
                Node::Split {
                    feature,
                    threshold,
                    left,
                    right,
                } => {
                    i = if x[*feature] < *threshold {
                        *left
                    } else {
                        *right
                    }
                }
            }
        }
    }
}

/// A trained model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gbdt {
    /// Names of the features, in order.
    pub features: Vec<String>,
    /// Initial log-odds.
    pub base: f64,
    /// Learning rate.
    pub learning_rate: f64,
    /// Trees.
    pub trees: Vec<Tree>,
    /// Parameters used.
    pub params: Params,
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

impl Gbdt {
    /// Probability of the positive class.
    pub fn predict(&self, x: &[f64]) -> f64 {
        let z = self.base + self.learning_rate * self.trees.iter().map(|t| t.eval(x)).sum::<f64>();
        sigmoid(z)
    }

    /// Trains on rows `x` (all of the same length as `features`) and labels `y` (0 or 1).
    pub fn fit(features: &[String], x: &[Vec<f64>], y: &[f64], p: Params) -> Gbdt {
        let n = x.len();
        let nf = features.len();
        let pos = y.iter().sum::<f64>().clamp(0.5, n as f64 - 0.5);
        let base = (pos / (n as f64 - pos)).ln();
        // Pre-sorted indices per feature.
        let order: Vec<Vec<usize>> = (0..nf)
            .map(|f| {
                let mut o: Vec<usize> = (0..n).collect();
                o.sort_by(|&a, &b| x[a][f].total_cmp(&x[b][f]).then(a.cmp(&b)));
                o
            })
            .collect();
        let mut score = vec![base; n];
        let mut trees = Vec::with_capacity(p.trees);
        for _ in 0..p.trees {
            let g: Vec<f64> = (0..n).map(|i| sigmoid(score[i]) - y[i]).collect();
            let h: Vec<f64> = (0..n)
                .map(|i| {
                    let s = sigmoid(score[i]);
                    (s * (1.0 - s)).max(1e-12)
                })
                .collect();
            let mut tree = Tree { nodes: Vec::new() };
            let mut member = vec![true; n];
            grow(&mut tree, x, &g, &h, &order, &mut member, 0, &p);
            for (i, s) in score.iter_mut().enumerate() {
                *s += p.learning_rate * tree.eval(&x[i]);
            }
            trees.push(tree);
        }
        Gbdt {
            features: features.to_vec(),
            base,
            learning_rate: p.learning_rate,
            trees,
            params: p,
        }
    }
}

/// Grows the subtree of the rows in `member` and returns the index of its root.
#[allow(clippy::too_many_arguments)]
fn grow(
    tree: &mut Tree,
    x: &[Vec<f64>],
    g: &[f64],
    h: &[f64],
    order: &[Vec<usize>],
    member: &mut [bool],
    depth: usize,
    p: &Params,
) -> usize {
    let (gs, hs): (f64, f64) = (0..x.len())
        .filter(|&i| member[i])
        .fold((0.0, 0.0), |a, i| (a.0 + g[i], a.1 + h[i]));
    let leaf = -gs / (hs + p.lambda);
    let id = tree.nodes.len();
    tree.nodes.push(Node::Leaf(leaf));
    if depth >= p.depth || hs < 2.0 * p.min_child_weight {
        return id;
    }
    let gain_of = |gl: f64, hl: f64| {
        let (gr, hr) = (gs - gl, hs - hl);
        gl * gl / (hl + p.lambda) + gr * gr / (hr + p.lambda) - gs * gs / (hs + p.lambda)
    };
    let mut best: Option<(f64, usize, f64)> = None;
    for (f, o) in order.iter().enumerate() {
        let (mut gl, mut hl) = (0.0, 0.0);
        let rows: Vec<usize> = o.iter().copied().filter(|&i| member[i]).collect();
        for w in rows.windows(2) {
            let (a, b) = (w[0], w[1]);
            gl += g[a];
            hl += h[a];
            if x[a][f] == x[b][f] {
                continue;
            }
            if hl < p.min_child_weight || hs - hl < p.min_child_weight {
                continue;
            }
            let gain = gain_of(gl, hl);
            if gain > 1e-9 && best.is_none_or(|bst| gain > bst.0) {
                best = Some((gain, f, (x[a][f] + x[b][f]) / 2.0));
            }
        }
    }
    let Some((_, feature, threshold)) = best else {
        return id;
    };
    let rows: Vec<usize> = (0..x.len()).filter(|&i| member[i]).collect();
    let mut left_m = vec![false; x.len()];
    let mut right_m = vec![false; x.len()];
    for &i in &rows {
        if x[i][feature] < threshold {
            left_m[i] = true;
        } else {
            right_m[i] = true;
        }
    }
    let left = grow(tree, x, g, h, order, &mut left_m, depth + 1, p);
    let right = grow(tree, x, g, h, order, &mut right_m, depth + 1, p);
    tree.nodes[id] = Node::Split {
        feature,
        threshold,
        left,
        right,
    };
    id
}

/// Area under the ROC curve of scores against labels (ties counted half).
pub fn auc(scores: &[f64], labels: &[f64]) -> f64 {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]));
    let (mut rank_sum, mut i) = (0.0, 0);
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && scores[idx[j + 1]] == scores[idx[i]] {
            j += 1;
        }
        let r = (i + j) as f64 / 2.0 + 1.0;
        for k in i..=j {
            if labels[idx[k]] > 0.5 {
                rank_sum += r;
            }
        }
        i = j + 1;
    }
    let np = labels.iter().filter(|&&l| l > 0.5).count() as f64;
    let nn = labels.len() as f64 - np;
    if np == 0.0 || nn == 0.0 {
        return f64::NAN;
    }
    (rank_sum - np * (np + 1.0) / 2.0) / (np * nn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_an_interaction_no_line_can_separate() {
        // XOR of two thresholds with noise-free labels.
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..400 {
            let a = (i % 20) as f64 / 20.0;
            let b = (i / 20) as f64 / 20.0;
            x.push(vec![a, b, 0.5]);
            y.push(f64::from((a > 0.5) != (b > 0.5)));
        }
        let names: Vec<String> = ["a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
        let m = Gbdt::fit(&names, &x, &y, Params::default());
        let acc = x
            .iter()
            .zip(&y)
            .filter(|(r, l)| (m.predict(r) > 0.5) == (**l > 0.5))
            .count();
        assert_eq!(acc, 400);
        let s: Vec<f64> = x.iter().map(|r| m.predict(r)).collect();
        assert!(auc(&s, &y) > 0.999);
        // Round trip through JSON.
        let back: Gbdt = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back.predict(&[0.9, 0.1, 0.5]), m.predict(&[0.9, 0.1, 0.5]));
    }

    #[test]
    fn auc_of_known_rankings() {
        assert_eq!(auc(&[0.1, 0.2, 0.8, 0.9], &[0.0, 0.0, 1.0, 1.0]), 1.0);
        assert_eq!(auc(&[0.9, 0.8, 0.2, 0.1], &[0.0, 0.0, 1.0, 1.0]), 0.0);
        assert_eq!(auc(&[0.5, 0.5], &[0.0, 1.0]), 0.5);
    }
}
