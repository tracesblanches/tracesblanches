//! Trains and evaluates the matching model on the labelled pairs of `train_pairs`:
//! cross-validation by group of domains (never a domain both in training and in test),
//! precision / recall of the line decisions, threshold for a precision of 99 %, then the
//! model trained on every training plan applied to the two sites (test only). Writes
//! `data/out/france/plan_training/model.json` and `evaluation.json`; with a path argument,
//! also the rows as CSV (for the Python oracle, outside the repository).
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example train_model [rows.csv]`

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use serde_json::{Value, json};
use skimap_common::config::Config;
use skimap_pisteplan::evaluate::{self, Row, cross_validate, line_decisions, pr_at};
use skimap_pisteplan::gbdt::{Gbdt, Params};
use skimap_pisteplan::{Error, collect, learn};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let csv = std::env::args().nth(1);
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let census = collect::load_census(&layout)?;
    let dir = layout.france_layer_dir("plan_training")?;
    // Test domains: those of the two sites (centre inside the site's box).
    let mut test_domains: BTreeSet<String> = BTreeSet::new();
    for s in skimap_common::site::SITES {
        for d in &census {
            if s.bbox.contains(d.bbox.center()) {
                test_domains.insert(d.id.clone());
            }
        }
    }
    let mut plans: Vec<Value> = Vec::new();
    for e in std::fs::read_dir(dir.join("plans"))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "json") {
            let v: Value = serde_json::from_slice(&std::fs::read(&p)?)?;
            if v["ok"].as_bool() == Some(true) {
                plans.push(v);
            }
        }
    }
    plans.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    // And the domains the test plans draw (a training plan sharing one would put a domain
    // both in training and in test).
    for v in plans.iter().filter(|v| v["test"].as_bool() == Some(true)) {
        for d in v["domains_drawn"].as_array().into_iter().flatten() {
            if let Some(d) = d.as_str() {
                test_domains.insert(d.to_owned());
            }
        }
    }
    let doms = |v: &Value| -> Vec<String> {
        v["domains_drawn"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d.as_str().map(str::to_owned))
            .collect()
    };
    let is_test = |v: &Value| {
        v["test"].as_bool() == Some(true) || doms(v).iter().any(|d| test_domains.contains(d))
    };
    // Rows written before the name descriptor was added get it from their names (the
    // same function as `learn::pairs`).
    let rows_of = |v: &Value| -> Vec<Row> {
        let mut rows: Vec<Row> = serde_json::from_value(v["pairs"].clone()).unwrap_or_default();
        for r in &mut rows {
            if r.x.len() + 1 == learn::FEATURES.len() {
                r.x.push(learn::name_feature(&r.entry_name, r.unit_name.as_deref()));
            }
        }
        rows.retain(|r| r.x.len() == learn::FEATURES.len());
        rows
    };
    let mut train: Vec<Row> = Vec::new();
    let mut plan_domains: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut excluded = Vec::new();
    for v in &plans {
        let id = v["id"].as_str().unwrap_or_default().to_owned();
        if is_test(v) {
            excluded.push(id);
            continue;
        }
        let rows: Vec<Row> = rows_of(v)
            .into_iter()
            .filter(|r| r.label.is_some())
            .collect();
        if !rows.iter().any(|r| r.label == Some(1)) {
            continue;
        }
        let mut d = doms(v);
        if d.is_empty() {
            d.push(format!("plan:{id}"));
        }
        plan_domains.insert(id, d);
        train.extend(rows);
    }
    let group_of = evaluate::domain_groups(&plan_domains);
    let features: Vec<String> = learn::FEATURES.iter().map(|s| (*s).to_owned()).collect();
    let params = Params::default();
    println!(
        "{} registered plans; {} excluded (test sites); {} training plans, {} groups of domains, {} labelled pairs ({} positive)",
        plans.len(),
        excluded.len(),
        plan_domains.len(),
        group_of.values().collect::<BTreeSet<_>>().len(),
        train.len(),
        train.iter().filter(|r| r.label == Some(1)).count()
    );
    let (oof, cv) = cross_validate(&features, &train, &group_of, 5, params);
    println!(
        "cross-validation: {} folds, AUC {:.4}; threshold for 99 % precision: {:?}",
        cv.folds, cv.auc, cv.threshold_99
    );
    for t in [0.5, 0.7, 0.8, 0.9, 0.95, 0.98, 0.99] {
        let ds = line_decisions(&train, &oof);
        let p = pr_at(&ds, &train, t);
        println!(
            "  t {t:.2}: {} decided, {} right, precision {:.3} (95 % low {:.3}), recall {:.3}",
            p.decided, p.correct, p.precision, p.precision_low95, p.recall
        );
    }
    if let Some(path) = &csv {
        let mut f = std::fs::File::create(path)?;
        writeln!(f, "plan,group,entry,label,oof,{}", features.join(","))?;
        for (r, p) in train.iter().zip(&oof) {
            writeln!(
                f,
                "{},{},{},{},{},{}",
                r.plan,
                group_of[&r.plan],
                r.entry,
                r.label.unwrap_or(0),
                p,
                r.x.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
        }
    }
    // Final model on every training plan.
    let x: Vec<Vec<f64>> = train.iter().map(|r| r.x.clone()).collect();
    let y: Vec<f64> = train
        .iter()
        .map(|r| f64::from(r.label.unwrap_or(0)))
        .collect();
    let model = Gbdt::fit(&features, &x, &y, params);
    let threshold = cv.threshold_99.unwrap_or(1.0);
    // The two sites.
    let mut sites = Vec::new();
    for v in plans.iter().filter(|v| v["test"].as_bool() == Some(true)) {
        let rows = rows_of(v);
        let p: Vec<f64> = rows.iter().map(|r| model.predict(&r.x)).collect();
        let ds = line_decisions(&rows, &p);
        let mut lines = Vec::new();
        for d in &ds {
            let r = &rows[d.best];
            let exact = rows
                .iter()
                .any(|o| o.plan == r.plan && o.entry == r.entry && o.label == Some(1));
            let decided = d.p >= threshold && d.second < threshold;
            println!(
                "  {:28} {:32} → {:32} p {:.3} (2nd {:.3}) {}{}",
                v["id"].as_str().unwrap_or_default(),
                r.entry_name,
                r.unit_name.as_deref().unwrap_or("(sans nom)"),
                d.p,
                d.second,
                if decided { "ML " } else { "   " },
                match d.correct {
                    Some(true) => "= exact",
                    Some(false) => "≠ exact",
                    None if exact => "",
                    None => "(pas d'exact)",
                }
            );
            lines.push(json!({
                "entry": r.entry_name, "unit": r.unit_name, "relations": r.relations,
                "p": d.p, "second": d.second, "decided": decided, "agrees_with_exact": d.correct,
            }));
        }
        sites.push(json!({"plan": v["id"], "lines": lines}));
    }
    skimap_common::output::write_json(
        &dir.join("model.json"),
        &json!({"model": model, "threshold": threshold, "features": features}),
    )?;
    skimap_common::output::write_json(
        &dir.join("evaluation.json"),
        &json!({
            "training_plans": plan_domains.keys().collect::<Vec<_>>(),
            "excluded_test_plans": excluded,
            "test_domains": test_domains,
            "cross_validation": cv,
            "threshold_99": threshold,
            "sites": sites,
        }),
    )?;
    Ok(())
}
