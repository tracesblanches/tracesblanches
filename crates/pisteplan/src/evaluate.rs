//! Evaluation of the matching model by domain: plans grouped by the census domains they
//! draw (two plans sharing a domain are in the same group), groups split into folds, the
//! model trained on the other folds scores each fold (never a domain both in training and
//! in test). Decision per legend line: its best candidate, when the model's probability
//! reaches the threshold and leaves every other candidate of the line below it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::gbdt::{Gbdt, Params, auc};

/// A candidate pair as stored by the labelling (subset of `learn::Pair`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    /// Plan.
    pub plan: String,
    /// Legend line index.
    pub entry: usize,
    /// Legend name.
    pub entry_name: String,
    /// OSM relations of the unit.
    pub relations: Vec<i64>,
    /// OSM name of the unit.
    pub unit_name: Option<String>,
    /// Descriptors.
    pub x: Vec<f64>,
    /// Exact label.
    pub label: Option<u8>,
}

/// Groups of plans: plans sharing a domain are joined (union-find). Returns plan → group.
pub fn domain_groups(plans: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, usize> {
    let names: Vec<&String> = plans.keys().collect();
    let mut parent: Vec<usize> = (0..names.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut j = i;
        while p[j] != r {
            let n = p[j];
            p[j] = r;
            j = n;
        }
        r
    }
    let mut by_domain: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, n) in names.iter().enumerate() {
        for d in &plans[*n] {
            match by_domain.get(d.as_str()) {
                Some(&j) => {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    parent[a] = b;
                }
                None => {
                    by_domain.insert(d, i);
                }
            }
        }
    }
    let mut ids: BTreeMap<usize, usize> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (i, n) in names.iter().enumerate() {
        let r = find(&mut parent, i);
        let next = ids.len();
        let g = *ids.entry(r).or_insert(next);
        out.insert((*n).clone(), g);
    }
    out
}

/// Assigns groups to `k` folds, balancing the number of rows (largest groups first).
pub fn folds(group_sizes: &BTreeMap<usize, usize>, k: usize) -> BTreeMap<usize, usize> {
    let mut gs: Vec<(usize, usize)> = group_sizes.iter().map(|(g, n)| (*g, *n)).collect();
    gs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut load = vec![0usize; k];
    let mut out = BTreeMap::new();
    for (g, n) in gs {
        let f = (0..k).min_by_key(|&f| (load[f], f)).unwrap_or(0);
        load[f] += n;
        out.insert(g, f);
    }
    out
}

/// Decision on one legend line.
#[derive(Debug, Clone, Serialize)]
pub struct LineDecision {
    /// Plan.
    pub plan: String,
    /// Legend line.
    pub entry: usize,
    /// Best candidate (index into the rows).
    pub best: usize,
    /// Its probability.
    pub p: f64,
    /// Probability of the runner-up.
    pub second: f64,
    /// Whether the best candidate is the exact match (`None`: line without exact label).
    pub correct: Option<bool>,
}

/// Best candidate of every legend line.
pub fn line_decisions(rows: &[Row], p: &[f64]) -> Vec<LineDecision> {
    let mut by: BTreeMap<(String, usize), Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        by.entry((r.plan.clone(), r.entry)).or_default().push(i);
    }
    by.into_iter()
        .filter_map(|((plan, entry), idx)| {
            let best = *idx.iter().max_by(|&&a, &&b| p[a].total_cmp(&p[b]))?;
            let second = idx
                .iter()
                .filter(|&&i| i != best)
                .map(|&i| p[i])
                .fold(0.0, f64::max);
            let labelled = idx.iter().any(|&i| rows[i].label.is_some());
            let correct = labelled.then(|| rows[best].label == Some(1));
            Some(LineDecision {
                plan,
                entry,
                best,
                p: p[best],
                second,
                correct,
            })
        })
        .collect()
}

/// Precision and recall at a threshold over the labelled lines that have a positive
/// candidate.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PrPoint {
    /// Threshold.
    pub threshold: f64,
    /// Lines decided.
    pub decided: usize,
    /// Of which correct.
    pub correct: usize,
    /// Lines with a positive candidate.
    pub positives: usize,
    /// Precision.
    pub precision: f64,
    /// Wilson 95 % lower bound of the precision.
    pub precision_low95: f64,
    /// Recall.
    pub recall: f64,
}

/// Wilson score interval, lower bound at 95 %.
pub fn wilson_low(k: usize, n: usize) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let (k, n, z) = (k as f64, n as f64, 1.96);
    let p = k / n;
    let d = 1.0 + z * z / n;
    (p + z * z / (2.0 * n) - z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt()) / d
}

/// Precision / recall of the line decisions at `t` (decided: best ≥ t and runner-up < t).
pub fn pr_at(ds: &[LineDecision], rows: &[Row], t: f64) -> PrPoint {
    let positives = ds
        .iter()
        .filter(|d| {
            rows.iter()
                .any(|r| r.plan == d.plan && r.entry == d.entry && r.label == Some(1))
        })
        .count();
    let decided: Vec<&LineDecision> = ds
        .iter()
        .filter(|d| d.correct.is_some() && d.p >= t && d.second < t)
        .collect();
    let correct = decided.iter().filter(|d| d.correct == Some(true)).count();
    PrPoint {
        threshold: t,
        decided: decided.len(),
        correct,
        positives,
        precision: if decided.is_empty() {
            1.0
        } else {
            correct as f64 / decided.len() as f64
        },
        precision_low95: wilson_low(correct, decided.len()),
        recall: correct as f64 / positives.max(1) as f64,
    }
}

/// Lowest decision threshold accepted, whatever the cross-validation: the measured
/// precision rests on a few hundred lines that the exact matching settled (the name is in
/// the labels), not on the lines the model is for.
pub const MIN_THRESHOLD: f64 = 0.9;

/// Result of the cross-validation.
#[derive(Debug, Clone, Serialize)]
pub struct CvReport {
    /// Folds.
    pub folds: usize,
    /// Groups of domains.
    pub groups: usize,
    /// Labelled rows, positives.
    pub rows: usize,
    /// Positive rows.
    pub positive_rows: usize,
    /// Area under the ROC curve of the out-of-fold scores.
    pub auc: f64,
    /// Precision / recall curve of the line decisions.
    pub curve: Vec<PrPoint>,
    /// Smallest threshold reaching a precision of 99 % (measured), if any.
    pub threshold_99: Option<f64>,
}

/// Out-of-fold probabilities of the labelled rows (folds by group of domains).
pub fn cross_validate(
    features: &[String],
    rows: &[Row],
    group_of: &BTreeMap<String, usize>,
    k: usize,
    params: Params,
) -> (Vec<f64>, CvReport) {
    let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
    for r in rows {
        *sizes.entry(group_of[&r.plan]).or_default() += 1;
    }
    let k = k.min(sizes.len()).max(2);
    let fold_of = folds(&sizes, k);
    let mut p = vec![0.0; rows.len()];
    for f in 0..k {
        let train: Vec<usize> = (0..rows.len())
            .filter(|&i| fold_of[&group_of[&rows[i].plan]] != f)
            .collect();
        let test: Vec<usize> = (0..rows.len())
            .filter(|&i| fold_of[&group_of[&rows[i].plan]] == f)
            .collect();
        if train.is_empty() || test.is_empty() {
            continue;
        }
        let x: Vec<Vec<f64>> = train.iter().map(|&i| rows[i].x.clone()).collect();
        let y: Vec<f64> = train
            .iter()
            .map(|&i| f64::from(rows[i].label.unwrap_or(0)))
            .collect();
        let m = Gbdt::fit(features, &x, &y, params);
        for &i in &test {
            p[i] = m.predict(&rows[i].x);
        }
    }
    let y: Vec<f64> = rows
        .iter()
        .map(|r| f64::from(r.label.unwrap_or(0)))
        .collect();
    let ds = line_decisions(rows, &p);
    let mut curve = Vec::new();
    for i in 0..=100 {
        let t = 0.5 + 0.5 * i as f64 / 100.0;
        curve.push(pr_at(&ds, rows, t.min(0.999)));
    }
    // Smallest threshold from which the measured precision stays at 99 % or more (every
    // higher threshold too), and no lower than MIN_THRESHOLD.
    let stable = (0..curve.len()).find(|&i| {
        curve[i..]
            .iter()
            .all(|p| p.decided == 0 || p.precision >= 0.99)
    });
    let threshold_99 = stable
        .filter(|&i| curve[i..].iter().any(|p| p.decided > 0))
        .map(|i| curve[i].threshold.max(MIN_THRESHOLD));
    let groups: BTreeSet<usize> = rows.iter().map(|r| group_of[&r.plan]).collect();
    let area = auc(&p, &y);
    (
        p,
        CvReport {
            folds: k,
            groups: groups.len(),
            rows: rows.len(),
            positive_rows: rows.iter().filter(|r| r.label == Some(1)).count(),
            auc: area,
            curve,
            threshold_99,
        },
    )
}

/// Mountain range of a French département (the nordic départements; the national split
/// into massifs of the mountain law, Ain with the Jura, Drôme with the Northern Alps).
pub fn massif_of_departement(dep: &str) -> Option<&'static str> {
    Some(match dep {
        "01" | "25" | "39" => "Jura",
        "26" | "38" | "73" | "74" => "Alpes du Nord",
        "04" | "05" | "06" => "Alpes du Sud",
        "54" | "57" | "67" | "68" | "70" | "88" | "90" => "Vosges",
        "03" | "07" | "12" | "15" | "19" | "23" | "30" | "34" | "42" | "43" | "48" | "63"
        | "81" | "87" => "Massif central",
        "09" | "11" | "31" | "64" | "65" | "66" => "Pyrénées",
        "2A" | "2B" => "Corse",
        _ => return None,
    })
}

/// Massif of a plan: the most frequent massif of its domains (ties: the first in name
/// order); `None` when none of them has one.
pub fn plan_massif(departements: &[&str]) -> Option<&'static str> {
    let mut n: BTreeMap<&'static str, usize> = BTreeMap::new();
    for d in departements {
        if let Some(m) = massif_of_departement(d) {
            *n.entry(m).or_default() += 1;
        }
    }
    n.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
        .map(|(m, _)| m)
}

/// Precision and recall of one massif held out.
#[derive(Debug, Clone, Serialize)]
pub struct MassifReport {
    /// Massif.
    pub massif: String,
    /// Plans of the massif (with labelled rows).
    pub plans: usize,
    /// Labelled rows, positive rows.
    pub rows: usize,
    /// Positive rows.
    pub positive_rows: usize,
    /// Leave-one-massif-out: the model trained on the other massifs only, at the
    /// threshold.
    pub held_out: PrPoint,
    /// Out-of-fold scores of the grouped cross-validation (folds by group of domains), at
    /// the threshold, restricted to the massif.
    pub cross_validated: PrPoint,
}

/// Columns of `features` kept, in order (the others dropped: a model without some
/// descriptors).
pub fn select_columns(rows: &[Row], features: &[String], drop: &[&str]) -> (Vec<String>, Vec<Row>) {
    let keep: Vec<usize> = (0..features.len())
        .filter(|&i| !drop.contains(&features[i].as_str()))
        .collect();
    let names = keep.iter().map(|&i| features[i].clone()).collect();
    let rows = rows
        .iter()
        .map(|r| Row {
            x: keep.iter().map(|&i| r.x[i]).collect(),
            ..r.clone()
        })
        .collect();
    (names, rows)
}

/// Precision and recall per massif at threshold `t`: for each massif, the model trained
/// on the rows of the other massifs scores the massif's rows (never a massif in training
/// and in test), and the grouped cross-validation's out-of-fold scores (`oof`) are
/// restricted to it. `massif_of`: plan → massif.
pub fn by_massif(
    features: &[String],
    rows: &[Row],
    oof: &[f64],
    massif_of: &BTreeMap<String, String>,
    t: f64,
    params: Params,
) -> Vec<MassifReport> {
    let massifs: BTreeSet<&String> = rows.iter().filter_map(|r| massif_of.get(&r.plan)).collect();
    let mut out = Vec::new();
    for m in massifs {
        let inside = |r: &Row| massif_of.get(&r.plan) == Some(m);
        let test: Vec<usize> = (0..rows.len()).filter(|&i| inside(&rows[i])).collect();
        let train: Vec<usize> = (0..rows.len()).filter(|&i| !inside(&rows[i])).collect();
        let test_rows: Vec<Row> = test.iter().map(|&i| rows[i].clone()).collect();
        let held_p: Vec<f64> = if train.iter().any(|&i| rows[i].label == Some(1)) {
            let x: Vec<Vec<f64>> = train.iter().map(|&i| rows[i].x.clone()).collect();
            let y: Vec<f64> = train
                .iter()
                .map(|&i| f64::from(rows[i].label.unwrap_or(0)))
                .collect();
            let model = Gbdt::fit(features, &x, &y, params);
            test_rows.iter().map(|r| model.predict(&r.x)).collect()
        } else {
            vec![0.0; test_rows.len()]
        };
        let oof_m: Vec<f64> = test.iter().map(|&i| oof[i]).collect();
        let plans: BTreeSet<&str> = test_rows.iter().map(|r| r.plan.as_str()).collect();
        out.push(MassifReport {
            massif: m.clone(),
            plans: plans.len(),
            rows: test_rows.len(),
            positive_rows: test_rows.iter().filter(|r| r.label == Some(1)).count(),
            held_out: pr_at(&line_decisions(&test_rows, &held_p), &test_rows, t),
            cross_validated: pr_at(&line_decisions(&test_rows, &oof_m), &test_rows, t),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn massifs_of_departements_and_plans() {
        assert_eq!(massif_of_departement("25"), Some("Jura"));
        assert_eq!(massif_of_departement("05"), Some("Alpes du Sud"));
        assert_eq!(massif_of_departement("75"), None);
        assert_eq!(plan_massif(&["25", "39", "74"]), Some("Jura"));
        assert_eq!(plan_massif(&["75"]), None);
        let rows = vec![Row {
            plan: "p".into(),
            entry: 0,
            entry_name: "x".into(),
            relations: vec![],
            unit_name: None,
            x: vec![1.0, 2.0, 3.0],
            label: Some(1),
        }];
        let f: Vec<String> = ["a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
        let (names, r) = select_columns(&rows, &f, &["b"]);
        assert_eq!(names, vec!["a".to_owned(), "c".to_owned()]);
        assert_eq!(r[0].x, vec![1.0, 3.0]);
    }

    #[test]
    fn plans_sharing_a_domain_are_one_group() {
        let mut m = BTreeMap::new();
        m.insert("a".to_owned(), vec!["d1".to_owned()]);
        m.insert("b".to_owned(), vec!["d2".to_owned(), "d1".to_owned()]);
        m.insert("c".to_owned(), vec!["d3".to_owned()]);
        let g = domain_groups(&m);
        assert_eq!(g["a"], g["b"]);
        assert_ne!(g["a"], g["c"]);
        let sizes: BTreeMap<usize, usize> = [(0, 10), (1, 7), (2, 3)].into_iter().collect();
        let f = folds(&sizes, 2);
        assert_ne!(f[&0], f[&1]);
        assert_eq!(f[&1], f[&2]);
    }

    #[test]
    fn line_decisions_need_the_threshold_and_no_rival() {
        let row = |plan: &str, entry, label| Row {
            plan: plan.into(),
            entry,
            entry_name: "x".into(),
            relations: vec![],
            unit_name: None,
            x: vec![],
            label,
        };
        let rows = vec![
            row("p", 0, Some(1)),
            row("p", 0, Some(0)),
            row("p", 1, Some(0)),
            row("p", 1, Some(1)),
            row("p", 2, None),
        ];
        let p = [0.95, 0.1, 0.92, 0.91, 0.99];
        let ds = line_decisions(&rows, &p);
        assert_eq!(ds.len(), 3);
        // Line 0 right, line 1 wrong (and its rival is above 0.9), line 2 unlabelled.
        let at = pr_at(&ds, &rows, 0.9);
        assert_eq!((at.decided, at.correct, at.positives), (1, 1, 2));
        let at = pr_at(&ds, &rows, 0.915);
        assert_eq!((at.decided, at.correct), (2, 1));
        assert!((wilson_low(99, 100) - 0.9455).abs() < 1e-3);
    }
}
