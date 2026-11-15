//! Learnt matching of the plan's pistes to OSM (P22, model step): candidate pairs (a piste
//! of the legend, drawn in its ink ↔ an OSM piste: relations of one name, connected), their
//! descriptors, and the training labels given by the exact matching.
//!
//! Descriptors of the drawing: share of the OSM geometry drawn in the piste's ink at four
//! radii and the same for shifted copies (permutation), other inks drawn there, colour of
//! the OSM piste against the ink, length against the printed length, distance to the
//! piste's departure box, map labels read near the geometry against the legend name, loop,
//! rank among the candidates of the same legend line, quality of the registration; and the
//! similarity of the legend name with the OSM name. The exact matching that labels the
//! pairs requires that name (key equality): the cross-validation measures the model on
//! lines the exact matching settled, its precision on the lines it left out (the ones the
//! model is for) is checked by hand on the two sites.

use std::collections::{BTreeSet, HashSet};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use skimap_common::names::name_key;

use crate::build::Registration;
use crate::labels::similarity;
use crate::transcribe::{
    self, Decision, InkDistance, PisteTest, PlanEntry, TestSamples, ink_samples, share_within,
    warped_samples,
};
use crate::transcript::PisteKind;
use crate::warp::{P2, dist};

/// Names of the descriptors, in the order of [`Pair::x`].
pub const FEATURES: &[&str] = &[
    "share_3pt",
    "share_5pt",
    "share_8pt",
    "share_12pt",
    "shift_3pt",
    "shift_5pt",
    "shift_8pt",
    "shift_12pt",
    "share_minus_shift_8pt",
    "other_ink_share_8pt",
    "colour_agrees",
    "colour_known",
    "log_length_ratio",
    "has_plan_length",
    "box_distance_km",
    "box_on_map",
    "on_map_fraction",
    "log_samples",
    "rank_8pt",
    "gap_to_best_8pt",
    "map_label_similarity",
    "is_loop",
    "same_ink_entries",
    "kind",
    "tie_p90_m",
    "icp_median_m",
    "name_similarity",
];

/// Name descriptor: similarity of the legend name with the unit's OSM name (best of the
/// folded names and of the leading words of the reading, as for an OCR reading; 0 for an
/// unnamed unit). Continuous, unlike the exact matching's key equality.
pub fn name_feature(entry: &str, unit: Option<&str>) -> f64 {
    unit.map_or(0.0, |n| {
        transcribe::name_sim(crate::spec::LabelSource::Ocr, entry, n)
    })
}

/// An OSM piste candidate: relations of one name key, connected (or one unnamed
/// relation).
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    /// Relation indices.
    pub relations: Vec<usize>,
    /// Way indices.
    pub ways: Vec<usize>,
    /// Name.
    pub name: Option<String>,
    /// Uses (union).
    pub nordic: bool,
}

/// Candidate units of a registered plan.
pub fn units(osm: &crate::osmref::OsmRef) -> Vec<Unit> {
    let mut seen = vec![false; osm.relations.len()];
    let mut out = Vec::new();
    for i in 0..osm.relations.len() {
        if seen[i] || osm.relations[i].ways.is_empty() {
            continue;
        }
        let key = osm.relations[i]
            .name
            .as_deref()
            .map(name_key)
            .unwrap_or_default();
        // Relations of the same (non-empty) key connected to this one.
        let mut group = vec![i];
        seen[i] = true;
        if !key.is_empty() {
            let mut nodes: HashSet<i64> = osm.relations[i]
                .ways
                .iter()
                .flat_map(|&w| osm.ways[w].nodes.iter().copied())
                .collect();
            loop {
                let next = (0..osm.relations.len()).find(|&j| {
                    !seen[j]
                        && !osm.relations[j].ways.is_empty()
                        && osm.relations[j]
                            .name
                            .as_deref()
                            .map(name_key)
                            .unwrap_or_default()
                            == key
                        && osm.relations[j]
                            .ways
                            .iter()
                            .any(|&w| osm.ways[w].nodes.iter().any(|n| nodes.contains(n)))
                });
                let Some(j) = next else { break };
                seen[j] = true;
                nodes.extend(
                    osm.relations[j]
                        .ways
                        .iter()
                        .flat_map(|&w| osm.ways[w].nodes.iter().copied()),
                );
                group.push(j);
            }
        }
        let ways: BTreeSet<usize> = group
            .iter()
            .flat_map(|&r| osm.relations[r].ways.iter().copied())
            .collect();
        out.push(Unit {
            nordic: group
                .iter()
                .any(|&r| osm.relations[r].uses.iter().any(|u| u == "nordic")),
            name: osm.relations[i].name.clone(),
            relations: group,
            ways: ways.into_iter().collect(),
        });
    }
    out
}

/// A candidate pair with its descriptors and label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    /// Plan identifier.
    pub plan: String,
    /// Index of the legend line.
    pub entry: usize,
    /// Legend name.
    pub entry_name: String,
    /// Ink name.
    pub ink: String,
    /// OSM relation ids of the unit.
    pub relations: Vec<i64>,
    /// OSM way ids of the unit.
    pub ways: Vec<i64>,
    /// OSM name of the unit.
    pub unit_name: Option<String>,
    /// Descriptors ([`FEATURES`]).
    pub x: Vec<f64>,
    /// 1: the exact matching carried this line onto this unit; 0: it carried it onto
    /// another unit; `None`: the line has no exact match (unlabelled).
    pub label: Option<u8>,
    /// Similarity of the legend name with the unit's OSM name (not a descriptor: for the
    /// report and the decision rules only).
    pub name_similarity: f64,
}

/// Candidate pairs of a registered plan, with the exact decisions as labels. A unit is a
/// candidate of a legend line when it is nordic for a ski ink (non-nordic otherwise), has
/// ten samples on the map at least and the line's ink is drawn along at least 10 % of it
/// at 12 pt.
pub fn pairs(reg: &Registration, dmap: &InkDistance, decisions: &[Decision]) -> Vec<Pair> {
    let entries: Vec<PlanEntry> = transcribe::entries(reg);
    let us = units(&reg.osm);
    // Samples of every unit and of its shifted copies, carried onto the plan once (in
    // parallel; the warp is the costly part), for the units with ten samples on the map.
    let unit_samples: Vec<Option<TestSamples>> = us
        .par_iter()
        .map(|u| {
            let lines: Vec<&[P2]> = u
                .ways
                .iter()
                .map(|&w| reg.osm.ways[w].xy.as_slice())
                .collect();
            (warped_samples(reg, &lines).len() >= 10).then(|| TestSamples::new(reg, &u.ways))
        })
        .collect();
    // Tests of every (unit, ink): units with ten samples, ink drawn along 10 % of the
    // unit at 12 pt; and the share of each ink at 8 pt (other inks drawn there). Shared by
    // the legend lines of one ink, in parallel.
    let n_inks = reg.spec.inks.len();
    let share8: Vec<Vec<f64>> = unit_samples
        .par_iter()
        .map(|ts| {
            (0..n_inks)
                .map(|k| {
                    ts.as_ref().map_or(0.0, |ts| {
                        share_within(&ink_samples(dmap, &ts.truth, k), 8.0)
                    })
                })
                .collect()
        })
        .collect();
    let tests: Vec<Vec<Option<(PisteTest, usize)>>> = unit_samples
        .par_iter()
        .map(|ts| {
            (0..n_inks)
                .map(|k| {
                    let ts = ts.as_ref()?;
                    let samples = ink_samples(dmap, &ts.truth, k);
                    (share_within(&samples, 12.0) >= 0.1)
                        .then(|| (transcribe::piste_test_warped(dmap, ts, k), samples.len()))
                })
                .collect()
        })
        .collect();
    // Map labels carried onto the map once.
    let label_map: Vec<(P2, &str)> = reg
        .map_labels
        .iter()
        .map(|l| (reg.warp.to_map(l.center()), l.text.as_str()))
        .collect();
    let osm = &reg.osm;
    // Exact decisions: relation sets per (folded name, ink).
    let exact: Vec<(String, Option<String>, BTreeSet<usize>)> = decisions
        .iter()
        .filter(|d| d.status.is_some() && !d.relations.is_empty())
        .map(|d| {
            (
                crate::labels::fold(&d.entry.name_read),
                d.ink.clone(),
                d.relations.iter().copied().collect(),
            )
        })
        .collect();
    let ties: Vec<f64> = reg.ties.iter().map(|t| t.residual_m).collect();
    let tie_p90 = crate::warp::quantile(&ties, 0.9).unwrap_or(500.0);
    let icp_med = reg.steps.last().map_or(500.0, |s| s.median_m);
    let a = reg.spec.map_area;
    let same_ink = |ink: usize| entries.iter().filter(|e| e.ink == Some(ink)).count();
    let mut out = Vec::new();
    for (ei, e) in entries.iter().enumerate() {
        let Some(ink) = e.ink else { continue };
        let spec_ink = &reg.spec.inks[ink];
        let ski = matches!(
            spec_ink.kind,
            PisteKind::Ski | PisteKind::Liaison | PisteKind::MultiActivity
        );
        let pos = exact
            .iter()
            .find(|(n, i, _)| {
                *n == crate::labels::fold(&e.name_read)
                    && i.as_deref() == Some(spec_ink.name.as_str())
            })
            .map(|x| &x.2);
        let centre = [(e.rect[0] + e.rect[2]) / 2.0, (e.rect[1] + e.rect[3]) / 2.0];
        let box_map = reg.warp.to_map(centre);
        let box_on_map =
            centre[0] >= a[0] && centre[0] < a[2] && centre[1] >= a[1] && centre[1] < a[3];
        let mut rows: Vec<(usize, PisteTest, Vec<f64>)> = Vec::new();
        for (ui, u) in us.iter().enumerate() {
            if u.nordic != ski {
                continue;
            }
            let lines: Vec<&[P2]> = u.ways.iter().map(|&w| osm.ways[w].xy.as_slice()).collect();
            let Some((t, n_samples)) = &tests[ui][ink] else {
                continue;
            };
            let (t, n_samples) = (t.clone(), *n_samples);
            // Other inks drawn along the unit.
            let other = (0..reg.spec.inks.len())
                .filter(|&k| k != ink)
                .map(|k| share8[ui][k])
                .fold(0.0, f64::max);
            let len_m: f64 = u.ways.iter().map(|&w| osm.ways[w].length_m).sum();
            let total_len: f64 = lines
                .iter()
                .flat_map(|l| l.windows(2))
                .map(|s| dist(s[0], s[1]))
                .sum();
            let total_samples = (total_len / transcribe::SHARE_STEP_M).max(1.0);
            let osm_colours: BTreeSet<&str> = u
                .relations
                .iter()
                .filter_map(|&r| osm.relations[r].colour.as_deref())
                .collect();
            let (agree, known) = match (spec_ink.difficulty.as_deref(), osm_colours.is_empty()) {
                (Some(c), false) => (f64::from(osm_colours.contains(c)), 1.0),
                _ => (0.0, 0.0),
            };
            let plan_km = e.lengths_km.first().copied();
            let box_km = u
                .ways
                .iter()
                .flat_map(|&w| osm.ways[w].xy.iter())
                .map(|p| dist(*p, box_map))
                .fold(f64::INFINITY, f64::min)
                / 1000.0;
            // Map labels within 400 m of the unit, against the legend name.
            let label_sim = label_map
                .iter()
                .filter(|(q, _)| {
                    u.ways
                        .iter()
                        .flat_map(|&w| osm.ways[w].xy.iter())
                        .any(|p| dist(*p, *q) < 400.0)
                })
                .map(|(_, text)| similarity(text, &e.name_read))
                .fold(0.0, f64::max);
            let i8 = 2;
            let x = vec![
                t.share[0],
                t.share[1],
                t.share[2],
                t.share[3],
                t.shift_max[0],
                t.shift_max[1],
                t.shift_max[2],
                t.shift_max[3],
                t.share[i8] - t.shift_max[i8],
                other,
                agree,
                known,
                plan_km.map_or(0.0, |km| (len_m / (1000.0 * km).max(1.0)).ln()),
                f64::from(u8::from(plan_km.is_some())),
                box_km.min(50.0),
                f64::from(u8::from(box_on_map)),
                (n_samples as f64 / total_samples).min(1.0),
                (n_samples as f64).ln(),
                0.0,
                0.0,
                label_sim,
                f64::from(u8::from(transcribe::is_loop(osm, &u.ways))),
                same_ink(ink) as f64,
                match spec_ink.kind {
                    PisteKind::Ski => 0.0,
                    PisteKind::Liaison => 1.0,
                    _ => 2.0,
                },
                tie_p90.min(1000.0),
                icp_med.min(1000.0),
                name_feature(&e.name_read, u.name.as_deref()),
            ];
            debug_assert_eq!(x.len(), FEATURES.len());
            rows.push((ui, t, x));
        }
        // Rank and gap of the 8 pt share among the line's candidates.
        let mut shares: Vec<f64> = rows.iter().map(|r| r.1.share[2]).collect();
        shares.sort_by(|a, b| b.total_cmp(a));
        for (ui, t, mut x) in rows {
            let s = t.share[2];
            let rank = shares.iter().filter(|&&o| o > s).count();
            let best_other = shares.iter().copied().find(|&o| o != s).unwrap_or(0.0).max(
                if shares.iter().filter(|&&o| o == s).count() > 1 {
                    s
                } else {
                    0.0
                },
            );
            x[18] = rank as f64;
            x[19] = if rank == 0 {
                s - best_other
            } else {
                s - shares[0]
            };
            let u = &us[ui];
            let label = pos.map(|p| u8::from(u.relations.iter().any(|r| p.contains(r))));
            out.push(Pair {
                plan: reg.spec.id.clone(),
                entry: ei,
                entry_name: e.name_read.clone(),
                ink: spec_ink.name.clone(),
                relations: u.relations.iter().map(|&r| osm.relations[r].id).collect(),
                ways: u.ways.iter().map(|&w| osm.ways[w].id).collect(),
                unit_name: u.name.clone(),
                x,
                label,
                name_similarity: u.name.as_deref().map_or(0.0, |n| {
                    transcribe::name_sim(reg.spec.labels, &e.name_read, n)
                }),
            });
        }
    }
    out
}

/// A trained model and its decision threshold (`data/out/france/plan_training/model.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    /// The boosted trees.
    pub model: crate::gbdt::Gbdt,
    /// Probability for a precision of 99 % in the cross-validation by domain.
    pub threshold: f64,
    /// Descriptor names (must equal [`FEATURES`]).
    pub features: Vec<String>,
}

impl Model {
    /// Reads a model file; `None` when absent or made for other descriptors.
    pub fn read(path: &std::path::Path) -> Option<Model> {
        let m: Model = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        (m.features
            .iter()
            .map(String::as_str)
            .eq(FEATURES.iter().copied()))
        .then_some(m)
    }
}

/// Learnt matches for the legend lines the exact matching left out: the line's best
/// candidate when the model's probability reaches the threshold and leaves every other
/// candidate of the line below it. The decision gets method `ml`, score = probability,
/// the unit's relations and ways, and a status: as an exact match when the unit carries
/// the line's name, `name_missing` otherwise (OSM has the piste under another name or
/// none).
pub fn apply_model(
    reg: &Registration,
    dmap: &InkDistance,
    decisions: &mut [Decision],
    model: &Model,
) -> usize {
    use crate::transcript::{MatchMethod, OsmStatus};
    let ps = pairs(reg, dmap, decisions);
    let mut by_entry: std::collections::BTreeMap<usize, Vec<(f64, &Pair)>> = Default::default();
    for p in &ps {
        by_entry
            .entry(p.entry)
            .or_default()
            .push((model.model.predict(&p.x), p));
    }
    let entries = transcribe::entries(reg);
    let osm = &reg.osm;
    let mut n = 0;
    for (ei, mut cands) in by_entry {
        cands.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (p, best) = cands[0];
        let second = cands.get(1).map_or(0.0, |c| c.0);
        if p < model.threshold || second >= model.threshold {
            continue;
        }
        let e = &entries[ei];
        let ink = e.ink.map(|i| reg.spec.inks[i].name.clone());
        let Some(d) = decisions.iter_mut().find(|d| {
            d.status.is_none()
                && crate::labels::fold(&d.entry.name_read) == crate::labels::fold(&e.name_read)
                && d.ink == ink
        }) else {
            continue;
        };
        let relations: Vec<usize> = best
            .relations
            .iter()
            .filter_map(|id| osm.relations.iter().position(|r| r.id == *id))
            .collect();
        let ways: Vec<usize> = best
            .ways
            .iter()
            .filter_map(|id| osm.ways.iter().position(|w| w.id == *id))
            .collect();
        let named = relations.iter().any(|&r| {
            osm.relations[r]
                .name
                .as_deref()
                .is_some_and(|n| name_key(n) == name_key(&e.name_read))
        });
        let osm_colours: BTreeSet<&str> = relations
            .iter()
            .filter_map(|&r| osm.relations[r].colour.as_deref())
            .collect();
        d.status = Some(if !named {
            OsmStatus::NameMissing
        } else {
            match (d.colour.as_deref(), osm_colours.len()) {
                (None, _) => OsmStatus::Match,
                (Some(_), 0) => OsmStatus::ColourMissing,
                (Some(c), _) if osm_colours.contains(c) => OsmStatus::Match,
                _ => OsmStatus::ColourDiffers,
            }
        });
        d.method = Some(MatchMethod::Ml);
        d.score = p;
        d.relations = relations;
        d.ways = ways;
        d.named_ways_only = false;
        d.reason = format!(
            "modèle appris : p = {p:.3} (seuil {:.3}, candidat suivant {second:.3}){}",
            model.threshold,
            if named {
                String::new()
            } else {
                format!(
                    " ; piste OSM « {} »",
                    best.unit_name.as_deref().unwrap_or("sans nom")
                )
            }
        );
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osmref::{OsmRef, OsmRelation, OsmWay};
    use skimap_osm::pistes::Oneway;

    fn way(id: i64, nodes: &[i64]) -> OsmWay {
        OsmWay {
            id,
            nodes: nodes.to_vec(),
            ll: Vec::new(),
            xy: Vec::new(),
            uses: vec!["nordic".into()],
            name: None,
            relations: Vec::new(),
            colour: None,
            oneway: Oneway::Unknown,
            length_m: 0.0,
        }
    }

    fn rel(id: i64, name: Option<&str>, ways: &[usize]) -> OsmRelation {
        OsmRelation {
            id,
            name: name.map(str::to_owned),
            ref_: None,
            colour: None,
            colour_raw: None,
            uses: vec!["nordic".into()],
            ways: ways.to_vec(),
        }
    }

    #[test]
    fn units_join_connected_relations_of_one_name() {
        let osm = OsmRef {
            ways: vec![
                way(1, &[1, 2]),
                way(2, &[2, 3]),
                way(3, &[10, 11]),
                way(4, &[3, 4]),
            ],
            relations: vec![
                rel(100, Some("La Loge"), &[0]),
                rel(101, Some("Piste de la Loge"), &[1]),
                rel(102, Some("La Loge"), &[2]),
                rel(103, None, &[3]),
                rel(104, Some("Le Gy"), &[]),
            ],
            ..Default::default()
        };
        let u = units(&osm);
        // « La Loge » and « Piste de la Loge » share a key and a node: one unit; the
        // unconnected homonym is another; the unnamed relation alone; no way, no unit.
        let rels: Vec<Vec<usize>> = u.iter().map(|x| x.relations.clone()).collect();
        assert_eq!(rels, vec![vec![0, 1], vec![2], vec![3]]);
        assert_eq!(u[0].ways, vec![0, 1]);
        assert!(u.iter().all(|x| x.nordic));
    }
}
