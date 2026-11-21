//! SR1 (#134): references, corridor profiles, evaluation, review, cost.
//!
//! ```sh
//! cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- refs
//! cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- profiles [--only TEXTE] [--limit N] [--force]
//! cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- evaluate
//! cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- review
//! cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- show TEXTE [--fresh CM] [--icy]
//! cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- bench TEXTE
//! ```

use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::site::SITES;
use skimap_srndiff::pipeline::corridor::{Corridor, SiteLayers};
use skimap_srndiff::pipeline::eval::{self, Case, MetricCache};
use skimap_srndiff::pipeline::refs::{self, RefRoute};
use skimap_srndiff::pipeline::{self as pl, out_dir, read_profile};
use skimap_srndiff::rules::Params;
use skimap_srndiff::{Conditions, Profile, assess};
use skimap_zones::jiff::Timestamp;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// A reference with its group (same route across sources).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    route: RefRoute,
    group: usize,
    selected: bool,
}

/// Instant used for the zone rules: 15/02/2026 09:00 UTC (as the C3 proofs).
fn at() -> Timestamp {
    skimap_common::dates::utc(skimap_zones::jiff::civil::date(2026, 2, 15).at(9, 0, 0, 0))
        .expect("valid date")
}

fn selected(r: &RefRoute) -> bool {
    let km: f64 = r
        .positions()
        .windows(2)
        .map(|w| w[0].haversine_m(w[1]))
        .sum::<f64>()
        / 1000.0;
    !r.ratings.is_empty()
        || r.provider == "c3"
        || (matches!(r.region.as_str(), "srn" | "easy_ski_touring") && km >= 1.0)
}

fn refs_path(layout: &DataLayout) -> Res<std::path::PathBuf> {
    Ok(out_dir(layout)?.join("references.json"))
}

fn load_refs(layout: &DataLayout) -> Res<Vec<Entry>> {
    let text = std::fs::read_to_string(refs_path(layout)?)?;
    Ok(serde_json::from_str(&text)?)
}

fn cmd_refs(layout: &DataLayout, dl: &Downloader) -> Res<()> {
    let t = Instant::now();
    let mut all = refs::g5_routes(layout)?;
    let n_g5 = all.len();
    let srn = refs::srn_com_routes(dl)?;
    let n_srn = srn.len();
    all.extend(srn);
    let c3 = refs::c3_routes(layout)?;
    let n_c3 = c3.len();
    all.extend(c3);
    let groups = refs::same_route_groups(&all);
    let entries: Vec<Entry> = all
        .into_iter()
        .zip(groups)
        .map(|(route, group)| Entry {
            selected: selected(&route),
            route,
            group,
        })
        .collect();
    skimap_common::output::write_json(&refs_path(layout)?, &entries)?;
    let rated = entries
        .iter()
        .filter(|e| e.route.ratings.iter().any(|r| r.level.is_some()))
        .count();
    println!(
        "{} references (G5 {n_g5}, skirandonneenordique.com {n_srn}, C3 {n_c3}), {} selected, {rated} with a mapped level, {:.1} s",
        entries.len(),
        entries.iter().filter(|e| e.selected).count(),
        t.elapsed().as_secs_f64()
    );
    let mut by_group: BTreeMap<usize, Vec<&Entry>> = BTreeMap::new();
    for e in &entries {
        by_group.entry(e.group).or_default().push(e);
    }
    for g in by_group.values().filter(|g| g.len() > 1) {
        let rated: Vec<String> = g
            .iter()
            .filter(|e| !e.route.ratings.is_empty())
            .map(|e| format!("{} [{}]", e.route.id, e.route.ratings[0].raw))
            .collect();
        if rated.len() > 1 {
            println!("same route: {}", rated.join(" | "));
        }
    }
    Ok(())
}

fn sites(layout: &DataLayout) -> Res<Vec<SiteLayers>> {
    SITES
        .iter()
        .map(|s| SiteLayers::load(layout, s).map_err(Into::into))
        .collect()
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1).cloned())
}

fn cmd_profiles(layout: &DataLayout, dl: &Downloader, args: &[String]) -> Res<()> {
    let only = arg_value(args, "--only");
    let limit: usize = arg_value(args, "--limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let force = args.iter().any(|a| a == "--force");
    let mut entries = load_refs(layout)?;
    entries.retain(|e| {
        e.selected
            && only
                .as_ref()
                .is_none_or(|o| e.route.id.contains(o.as_str()))
    });
    entries.sort_by_key(|e| e.route.track.len());
    let sites = sites(layout)?;
    let c = Corridor::default();
    let workers: usize = arg_value(args, "--jobs")
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    if !force {
        let mut todo = Vec::new();
        for e in entries {
            if read_profile(layout, &e.route.id)?.is_none() {
                todo.push(e);
            }
        }
        entries = todo;
    }
    entries.truncate(limit);
    let t_all = Instant::now();
    // Several tracks at once: tile downloads (one request in flight per
    // source) and the avalanche model overlap.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let built = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(e) = entries.get(k) else { break };
                    profile_one(layout, dl, e, &sites, &c, force, &built);
                }
            });
        }
    });
    println!(
        "{} profiles built in {:.1} s",
        built.load(std::sync::atomic::Ordering::SeqCst),
        t_all.elapsed().as_secs_f64()
    );
    Ok(())
}

fn profile_one(
    layout: &DataLayout,
    dl: &Downloader,
    e: &Entry,
    sites: &[SiteLayers],
    c: &Corridor,
    force: bool,
    built: &std::sync::atomic::AtomicUsize,
) {
    {
        let t = Instant::now();
        match pl::profile_of(layout, dl, &e.route, sites, at(), c, force) {
            Ok((p, Some(st))) => {
                built.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                println!(
                    "{:<60} {:>6.1} km {:>6.1} s  chunks {:>2} dem {:>3} model {:>2} ({:.1} Mcells, {:.1} s) fetch {:.1} s",
                    e.route.id,
                    p.samples.last().map_or(0.0, |s| s.s) / 1000.0,
                    t.elapsed().as_secs_f64(),
                    st.chunks,
                    st.dem_tiles,
                    st.model_chunks,
                    st.model_cells as f64 / 1e6,
                    st.model_s,
                    st.fetch_s
                );
            }
            Ok((_, None)) => {}
            Err(err) => println!("{:<60} ERROR {err}", e.route.id),
        }
    }
}

/// Cached profile, with the segment fields of the track (profiles built before they
/// existed get them from the track).
fn load_profile(layout: &DataLayout, route: &RefRoute) -> Res<Option<Profile>> {
    let Some(mut p) = read_profile(layout, &route.id)? else {
        return Ok(None);
    };
    if p.samples.iter().all(|s| s.seg_len == 0.0) {
        skimap_srndiff::profile::annotate_segments(&route.positions(), &mut p.samples)?;
    }
    Ok(Some(p))
}

/// Adds the roads (BD TOPO, open in winter) to the profiles that do not have them yet,
/// or to all of them with `--force`.
fn cmd_roads(layout: &DataLayout, dl: &Downloader, args: &[String]) -> Res<()> {
    let force = args.iter().any(|a| a == "--force");
    let entries = load_refs(layout)?;
    let t0 = Instant::now();
    let mut done = 0;
    for e in entries.iter().filter(|e| e.selected) {
        let Some(mut p) = read_profile(layout, &e.route.id)? else {
            continue;
        };
        if p.inputs.roads && !force {
            continue;
        }
        let t = Instant::now();
        match pl::add_roads(dl, &e.route, &mut p) {
            Ok(n) => {
                pl::write_profile(layout, &e.route.id, &p)?;
                done += 1;
                println!(
                    "{:<60} {:>6} road points, {:>4} exit samples, {:.1} s",
                    e.route.id,
                    n,
                    p.samples.iter().filter(|s| s.exit).count(),
                    t.elapsed().as_secs_f64()
                );
            }
            Err(err) => println!("{:<60} ERROR {err}", e.route.id),
        }
    }
    println!(
        "{done} profiles with roads in {:.1} s",
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

fn find<'a>(entries: &'a [Entry], text: &str) -> Res<&'a Entry> {
    entries
        .iter()
        .find(|e| e.route.id == text)
        .or_else(|| entries.iter().find(|e| e.route.id.contains(text)))
        .ok_or_else(|| format!("no reference matches {text}").into())
}

fn cmd_show(layout: &DataLayout, args: &[String]) -> Res<()> {
    let entries = load_refs(layout)?;
    let e = find(&entries, args.first().ok_or("show TEXTE")?)?;
    let p = load_profile(layout, &e.route)?.ok_or("profile not built")?;
    let c = Conditions {
        fresh_snow_cm: arg_value(args, "--fresh").and_then(|s| s.parse().ok()),
        icy: args.iter().any(|a| a == "--icy"),
        crust: args.iter().any(|a| a == "--crust"),
    };
    let cond = (c != Conditions::default()).then_some(c);
    let r = assess(&p, &Params::default(), cond.as_ref());
    println!("{}", r.text_fr(&title(&e.route)));
    Ok(())
}

fn title(r: &RefRoute) -> String {
    let rating = r
        .ratings
        .iter()
        .map(|x| format!("{} « {} »", scale_name(&x.scale), x.raw))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} ({}{})",
        r.name,
        r.provider,
        if rating.is_empty() {
            String::new()
        } else {
            format!(" ; cotation publiée : {rating}")
        }
    )
}

fn scale_name(s: &str) -> &str {
    match s {
        "srn_com" => "skirandonneenordique.com",
        "geotrek_vercors" => "PNR du Vercors",
        "osm" => "OSM piste:difficulty",
        "camptocamp" => "Camptocamp",
        other => other,
    }
}

#[derive(Serialize)]
struct CaseOut {
    id: String,
    name: String,
    group: usize,
    published: Vec<(String, String, Option<u8>)>,
    default_forward: u8,
    default_reverse: u8,
    technical: u8,
    effort: u8,
    literal_scale: u8,
    loo: u8,
    best: u8,
    best_without_avalanche_floor: u8,
    best_reason: String,
    length_km: f64,
    published_km: Option<f64>,
    ascent_m: f64,
    published_ascent_m: Option<f64>,
    max_climb_deg: f32,
    max_descent_deg: f32,
    exposed_km: f64,
    reasons: Vec<String>,
}

fn params_json(p: &Params) -> serde_json::Value {
    serde_json::to_value(p).unwrap_or_default()
}

fn cmd_evaluate(layout: &DataLayout, args: &[String]) -> Res<()> {
    let t0 = Instant::now();
    let entries = load_refs(layout)?;
    let mut profiles: Vec<(usize, Profile)> = Vec::new();
    let mut missing = 0;
    for (i, e) in entries.iter().enumerate() {
        if !e.selected {
            continue;
        }
        match load_profile(layout, &e.route)? {
            Some(p) => profiles.push((i, p)),
            None => missing += 1,
        }
    }
    // Rated cases with terrain (more than half of the track inside the LiDAR HD cover).
    let mut cases: Vec<Case<'_>> = Vec::new();
    let mut out_of_france = Vec::new();
    for (i, p) in &profiles {
        let e = &entries[*i];
        if !e.route.ratings.iter().any(|r| r.level.is_some()) {
            continue;
        }
        let known = p.samples.iter().filter(|s| s.z.is_some()).count() as f64
            / p.samples.len().max(1) as f64;
        if known < 0.5 {
            out_of_france.push(e.route.id.clone());
            continue;
        }
        cases.push(Case {
            route: &e.route,
            profile: p,
            group: e.group,
        });
    }
    let grid = eval::grid();
    let mut all_params = grid.clone();
    all_params.push(Params::default());
    all_params.push(Params::scale());
    let cache = MetricCache::new(&cases, &all_params);
    let t_measure = t0.elapsed().as_secs_f64();
    let idx: Vec<usize> = (0..cases.len()).collect();
    let pred_default = eval::predict_all(&cache, &cases, &Params::default());
    let pred_scale = eval::predict_all(&cache, &cases, &Params::scale());
    let obj = match arg_value(args, "--objective").as_deref() {
        Some("kappa") => eval::Objective::Kappa,
        _ => eval::Objective::Mae,
    };
    let best_all = eval::best(&cache, &cases, &idx, &grid, obj);
    let pred_best = eval::predict_all(&cache, &cases, &best_all);
    // Ablation (evaluation only, the product always applies it): the same parameters
    // without the avalanche floor, to measure what « never easy across exposed terrain »
    // costs against the published ratings.
    let pred_noava: Vec<u8> = (0..cases.len())
        .map(|i| {
            let b = skimap_srndiff::rules::rate(cache.get(&best_all, i), &best_all, None);
            b.technical.max(b.effort_counted).max(b.other_floor).index()
        })
        .collect();
    let t1 = Instant::now();
    let (pred_loo, chosen) = eval::leave_one_route_out(&cache, &cases, &grid, obj);
    let t_loo = t1.elapsed().as_secs_f64();
    let ag = |pred: &[u8]| eval::agreements(&cases, pred);

    // Agreement between two sources rating the same route.
    let mut inter: Vec<serde_json::Value> = Vec::new();
    let mut by_group: BTreeMap<usize, Vec<&Entry>> = BTreeMap::new();
    for e in &entries {
        by_group.entry(e.group).or_default().push(e);
    }
    for g in by_group.values() {
        let rated: Vec<(&Entry, &refs::Published)> = g
            .iter()
            .flat_map(|e| e.route.ratings.iter().map(move |r| (*e, r)))
            .filter(|(_, r)| r.level.is_some())
            .collect();
        for a in 0..rated.len() {
            for b in a + 1..rated.len() {
                let (ea, eb) = (&rated[a].0.route, &rated[b].0.route);
                // Same route: each track lies at least half on the other (a stage inside
                // a traverse is not the same route).
                if rated[a].1.scale != rated[b].1.scale
                    && refs::overlap(ea, eb) >= 0.5
                    && refs::overlap(eb, ea) >= 0.5
                {
                    inter.push(serde_json::json!({
                        "overlap": [refs::overlap(ea, eb), refs::overlap(eb, ea)],
                        "a": rated[a].0.route.id, "a_scale": rated[a].1.scale, "a_raw": rated[a].1.raw,
                        "b": rated[b].0.route.id, "b_scale": rated[b].1.scale, "b_raw": rated[b].1.raw,
                    }));
                }
            }
        }
    }

    // Camptocamp ski 1.x (ski touring scale): predicted level and steepest descent.
    let mut c2c: BTreeMap<String, Vec<(u8, f32)>> = BTreeMap::new();
    let d = Params::default();
    for (i, p) in &profiles {
        let e = &entries[*i];
        for r in &e.route.ratings {
            if r.scale == "camptocamp" {
                let rep = assess(p, &d, None);
                c2c.entry(r.raw.clone()).or_default().push((
                    rep.forward.rating.level.index(),
                    rep.forward
                        .metrics
                        .max_descent_deg
                        .max(rep.forward.metrics.max_climb_deg),
                ));
            }
        }
    }

    // Distribution of the levels of every profiled track (default parameters).
    let mut dist: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (i, p) in &profiles {
        let e = &entries[*i];
        let r = assess(p, &d, None);
        *dist
            .entry(e.route.provider.clone())
            .or_default()
            .entry(r.forward.rating.level.label().to_owned())
            .or_default() += 1;
    }

    let case_out: Vec<CaseOut> = cases
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let r = assess(c.profile, &d, None);
            CaseOut {
                id: c.route.id.clone(),
                name: c.route.name.clone(),
                group: c.group,
                published: c
                    .route
                    .ratings
                    .iter()
                    .map(|x| (x.scale.clone(), x.raw.clone(), x.level))
                    .collect(),
                default_forward: pred_default[i],
                default_reverse: r.reverse.rating.level.index(),
                technical: r.forward.rating.technical.index(),
                effort: r.forward.rating.effort.index(),
                literal_scale: pred_scale[i],
                loo: pred_loo[i],
                best: pred_best[i],
                best_without_avalanche_floor: {
                    let b = skimap_srndiff::rules::rate(cache.get(&best_all, i), &best_all, None);
                    b.technical.max(b.effort_counted).max(b.other_floor).index()
                },
                best_reason: skimap_srndiff::rules::rate(cache.get(&best_all, i), &best_all, None)
                    .reasons
                    .first()
                    .map(|x| format!("[{}] {}", x.level.label(), x.text))
                    .unwrap_or_default(),
                length_km: r.forward.metrics.length_km,
                published_km: c.route.published_km,
                ascent_m: r.forward.metrics.ascent_m,
                published_ascent_m: c.route.published_ascent_m,
                max_climb_deg: r.forward.metrics.max_climb_deg,
                max_descent_deg: r.forward.metrics.max_descent_deg,
                exposed_km: r.forward.metrics.exposed_km + r.forward.metrics.high_exposed_km,
                reasons: r
                    .forward
                    .rating
                    .reasons
                    .iter()
                    .take(3)
                    .map(|x| format!("[{}] {}", x.level.label(), x.text))
                    .collect(),
            }
        })
        .collect();
    // Chosen parameters per left-out route: how often each value wins.
    let mut chosen_count: BTreeMap<String, usize> = BTreeMap::new();
    for p in &chosen {
        *chosen_count
            .entry(format!(
                "sustain {} m, forest {:?}, sidehill {:?}, effort {:?}, open {:?}",
                p.sustain_m, p.forest_cover_pct, p.sidehill_deg, p.effort, p.open_deg
            ))
            .or_default() += 1;
    }
    // Published vs computed length and ascent.
    let rel = |a: f64, b: Option<f64>| b.filter(|b| *b > 0.0).map(|b| (a - b) / b);
    let mut len_err: Vec<f64> = case_out
        .iter()
        .filter_map(|c| rel(c.length_km, c.published_km))
        .collect();
    let mut asc_err: Vec<f64> = case_out
        .iter()
        .filter_map(|c| rel(c.ascent_m, c.published_ascent_m))
        .collect();
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        if v.is_empty() {
            f64::NAN
        } else {
            v[v.len() / 2]
        }
    };
    let abs_med = |v: &[f64]| {
        let mut a: Vec<f64> = v.iter().map(|x| x.abs()).collect();
        median(&mut a)
    };
    let len_abs = abs_med(&len_err);
    let asc_abs = abs_med(&asc_err);
    let json = serde_json::json!({
        "objective": obj,
        "cases": cases.len(),
        "groups": cases.iter().map(|c| c.group).collect::<std::collections::BTreeSet<_>>().len(),
        "profiles": profiles.len(),
        "profiles_missing": missing,
        "excluded_without_terrain": out_of_france,
        "default_params": params_json(&Params::default()),
        "agreement_default": ag(&pred_default),
        "agreement_literal_scale": ag(&pred_scale),
        "best_in_sample_params": params_json(&best_all),
        "agreement_best_in_sample": ag(&pred_best),
        "agreement_best_without_avalanche_floor": ag(&pred_noava),
        "agreement_leave_one_route_out": ag(&pred_loo),
        "loo_chosen_params": chosen_count,
        "inter_source": inter,
        "camptocamp": c2c,
        "level_distribution": dist,
        "length_rel_error_median": median(&mut len_err),
        "length_abs_rel_error_median": len_abs,
        "ascent_rel_error_median": median(&mut asc_err),
        "ascent_abs_rel_error_median": asc_abs,
        "seconds": {"measure": t_measure, "loo": t_loo, "total": t0.elapsed().as_secs_f64()},
        "case_details": case_out,
    });
    let dir = out_dir(layout)?;
    let name = match obj {
        eval::Objective::Mae => "evaluation.json",
        eval::Objective::Kappa => "evaluation_kappa.json",
    };
    skimap_common::output::write_json(&dir.join(name), &json)?;
    for (name, key) in [
        ("par défaut", "agreement_default"),
        ("échelle 2025 littérale", "agreement_literal_scale"),
        ("meilleur sur tout (optimiste)", "agreement_best_in_sample"),
        (
            "idem sans le plancher avalanche (analyse)",
            "agreement_best_without_avalanche_floor",
        ),
        (
            "validation en laissant un itinéraire de côté",
            "agreement_leave_one_route_out",
        ),
    ] {
        println!("== {name}");
        for a in json[key].as_array().into_iter().flatten() {
            println!(
                "  {:<22} n={:>3} exact {:>5.1} %  ±1 {:>5.1} %  MAE {:.2}  kappa_w {:.2}  matrice {}",
                a["scale"].as_str().unwrap_or_default(),
                a["n"],
                100.0 * a["exact"].as_f64().unwrap_or(0.0),
                100.0 * a["within_one"].as_f64().unwrap_or(0.0),
                a["mae"].as_f64().unwrap_or(0.0),
                a["kappa_w"].as_f64().unwrap_or(0.0),
                a["matrix"]
            );
        }
    }
    println!("best in sample: {}", json["best_in_sample_params"]);
    println!(
        "LOO chosen: {}",
        serde_json::to_string_pretty(&chosen_count)?
    );
    println!("inter-source: {}", serde_json::to_string_pretty(&inter)?);
    println!("camptocamp: {}", serde_json::to_string(&c2c)?);
    println!("distribution: {}", serde_json::to_string(&dist)?);
    println!(
        "length rel. error median {:.3} (abs {:.3}), ascent {:.3} (abs {:.3})",
        json["length_rel_error_median"].as_f64().unwrap_or(f64::NAN),
        len_abs,
        json["ascent_rel_error_median"].as_f64().unwrap_or(f64::NAN),
        asc_abs
    );
    println!(
        "{} cases in {} groups; {} profiles ({} missing); excluded {:?}; {:.1} s (measure {:.1}, LOO {:.1})",
        json["cases"],
        json["groups"],
        profiles.len(),
        missing,
        out_of_france,
        t0.elapsed().as_secs_f64(),
        t_measure,
        t_loo
    );
    for c in &case_out {
        println!(
            "{:<58} pub {:<12} def {} rev {} tech {} eff {} lit {} loo {} best {} noava {} | {:>5.1} km ({:>5}) +{:>4.0} ({:>5}) climb {:>4.1}° desc {:>4.1}° exp {:.1} km | {}",
            c.id.chars().take(58).collect::<String>(),
            c.published
                .iter()
                .map(|p| p.1.clone())
                .collect::<Vec<_>>()
                .join(","),
            c.default_forward,
            c.default_reverse,
            c.technical,
            c.effort,
            c.literal_scale,
            c.loo,
            c.best,
            c.best_without_avalanche_floor,
            c.length_km,
            c.published_km.map_or("-".into(), |v| format!("{v:.1}")),
            c.ascent_m,
            c.published_ascent_m
                .map_or("-".into(), |v| format!("{v:.0}")),
            c.max_climb_deg,
            c.max_descent_deg,
            c.exposed_km,
            c.best_reason
        );
    }
    write_meta(layout, &json)?;
    Ok(())
}

/// The 21 tracks reviewed by hand (identifiers or unique parts of them).
const REVIEW: &[&str] = &[
    "datatourisme:6/80785b47",
    "geotrek:geo.pnr-vercors.fr:29411",
    "srn.com:correncon-en-vercors-col-du-rousset",
    "geotrek:geo.pnr-vercors.fr:29403",
    "geotrek:geo.pnr-vercors.fr:29401",
    "geotrek:geo.pnr-vercors.fr:29218",
    "srn.com:le-tour-de-la-corniche-nord-du-vercors",
    "osm:relation/6354784",
    "srn.com:vallee-de-saint-veran-le-grand-canal-refuge-de-la-blanche",
    "srn.com:cervieres-capanna-mautino",
    "srn.com:souliers-arvieux-cervieres",
    "srn.com:lac-de-lorceyrette",
    "osm:relation/19990223",
    "camptocamp:1279879",
    "srn.com:super-longuevilles-mont-dor-la-coquille",
    "srn.com:la-vattay-la-dole",
    "camptocamp:1722787",
    "srn.com:tour-crozet-le-poteau-chez-la-comtesse",
    "c3:cervieres:laus_izoard",
    "c3:cervieres:bourget_italie",
    "c3:chapelle-des-bois:risoux_fevrier",
];

fn cmd_review(layout: &DataLayout) -> Res<()> {
    let entries = load_refs(layout)?;
    let mut text = String::new();
    for id in REVIEW {
        let e = find(&entries, id)?;
        let Some(p) = load_profile(layout, &e.route)? else {
            text.push_str(&format!("{} : profil absent\n\n", e.route.id));
            continue;
        };
        let r = assess(&p, &Params::default(), None);
        text.push_str(&format!(
            "### {}\n{}\n\n",
            e.route.id,
            r.text_fr(&title(&e.route))
        ));
    }
    let path = out_dir(layout)?.join("review.txt");
    skimap_common::output::write_text(&path, &text)?;
    println!("{text}");
    println!("-> {}", path.display());
    Ok(())
}

fn cmd_bench(layout: &DataLayout, dl: &Downloader, args: &[String]) -> Res<()> {
    use skimap_core::offpiste::{self, OffpisteGrid};
    use skimap_srndiff::field::{Classes, ElevationDm, SlopeHalfDeg};
    let entries = load_refs(layout)?;
    let e = find(&entries, args.first().ok_or("bench TEXTE")?)?;
    let full = load_profile(layout, &e.route)?.ok_or("profile not built")?;
    let n_full = full.samples.len();
    let runs = 20;

    // 1. Phone side, sampling: SKR1 rasters of Chapelle-des-Bois in memory (what the
    // phone loads from the pack), on the part of the track inside the site grid.
    let site = skimap_common::site::site("chapelle-des-bois").expect("evaluation site");
    let dir = layout.layer_dir(site, "offpiste")?;
    let g = OffpisteGrid::load(&dir)?;
    let exp: skimap_common::grid::Grid<u8> =
        offpiste::read_raster(&dir.join(offpiste::FILE_EXPOSURE))?;
    let inside: Vec<skimap_common::geom::LonLat> = e
        .route
        .positions()
        .into_iter()
        .filter(|p| g.cell_of(*p).ok().flatten().is_some() && site.bbox.contains(*p))
        .collect();
    let elev = ElevationDm(&g.elevation);
    let slope = SlopeHalfDeg(&g.slope);
    let ex = Classes {
        grid: &exp,
        nodata: offpiste::EXPOSURE_NODATA,
    };
    let pistes = Classes {
        grid: &g.pistes,
        nodata: u8::MAX,
    };
    let layers = skimap_srndiff::Layers {
        elevation: Some(&elev),
        slope: Some(&slope),
        // The canopy height is not in the SKR1 set yet: the elevation stands in for it,
        // same number of reads (25 per sample).
        canopy_height: Some(&elev),
        exposure: Some(&ex),
        pistes: Some(&pistes),
        ..Default::default()
    };
    let mut ts = Vec::new();
    let mut n_inside = 0;
    for _ in 0..runs {
        let t = Instant::now();
        let p = skimap_srndiff::profile::build(&inside, &layers, None)?;
        n_inside = p.samples.len();
        ts.push(t.elapsed().as_secs_f64());
    }
    ts.sort_by(f64::total_cmp);
    let per_sample = ts[runs / 2] / n_inside.max(1) as f64;
    println!(
        "sampling on the SKR1 rasters of {}: {} samples of the track inside the site, median {:.2} ms ({:.2} µs per sample)",
        site.name,
        n_inside,
        1000.0 * ts[runs / 2],
        1e6 * per_sample
    );

    // 2. Phone side, assessment of the whole track: both directions, advice, text.
    let mut ta = Vec::new();
    let mut text_len = 0;
    for _ in 0..runs {
        let t = Instant::now();
        let r = assess(&full, &Params::default(), None);
        text_len = r.text_fr(&e.route.name).len();
        ta.push(t.elapsed().as_secs_f64());
    }
    ta.sort_by(f64::total_cmp);
    println!(
        "assessment of the whole track ({} samples, {:.1} km): median {:.1} ms, max {:.1} ms, text {} bytes",
        n_full,
        full.samples.last().map_or(0.0, |s| s.s) / 1000.0,
        1000.0 * ta[runs / 2],
        1000.0 * ta[runs - 1],
        text_len
    );
    println!(
        "phone side, whole track: {:.1} ms (sampling {:.1} ms estimated from the per-sample cost + assessment)",
        1000.0 * (per_sample * n_full as f64 + ta[runs / 2]),
        1000.0 * per_sample * n_full as f64
    );

    // 3. Pipeline: corridor terrain (cache filled), avalanche model, site layers.
    if args.iter().any(|a| a == "--pipeline") {
        let sites = sites(layout)?;
        let t = Instant::now();
        let (p, st) = skimap_srndiff::pipeline::corridor::build_profile(
            &e.route.positions(),
            dl,
            &sites,
            at(),
            None,
            &Corridor::default(),
        )?;
        println!(
            "pipeline (tiles from the cache): {:.1} s for {} samples: {:?}",
            t.elapsed().as_secs_f64(),
            p.samples.len(),
            st
        );
    }
    Ok(())
}

fn write_meta(layout: &DataLayout, ev: &serde_json::Value) -> Res<()> {
    let mut m = LayerMeta::new(pl::LAYER, skimap_common::layout::FRANCE);
    m.tickets = vec!["SR1 #134".into()];
    m.crs = Some(
        "WGS84 (EPSG:4326) pour les tracés, Lambert-93 (EPSG:2154) pour les échantillons".into(),
    );
    m.validity = Some("jusqu'à la prochaine évaluation".into());
    m.sources = vec![
        SourceRef::new(
            "Échelle de cotation SRN des clubs alpins (octobre 2025)",
            "https://www.skirandonneenordique.com/actus-outdoor/un-nouveau-systeme-de-cotation-pour-le-srn/",
            "non indiquée (citée)",
            "skirandonneenordique.com",
        ),
        SourceRef::new(
            "Topos et cotations de skirandonneenordique.com",
            refs::SRN_TOPOS,
            "non indiquée : traces gardées dans le cache, non redistribuées",
            "skirandonneenordique.com",
        ),
        SourceRef::new(
            "Itinéraires SRN de la couche G5",
            "data/out/france/srn/srn_routes.geojson",
            "selon l'itinéraire (ODbL, CC BY-SA 3.0, Licence Ouverte 2.0, PNR du Vercors à confirmer)",
            "voir srn.meta.json",
        ),
        SourceRef::new(
            "IGN LiDAR HD MNT et MNH (WMS-R, 5 m)",
            skimap_terrain::dem::WMS_URL,
            "Licence Ouverte 2.0",
            "IGN",
        ),
        SourceRef::new(
            "IGN BD TOPO, tronçons de route (WFS)",
            pl::roads::WFS_URL,
            "Licence Ouverte 2.0",
            "IGN BD TOPO",
        ),
    ];
    m.files = vec![
        FileMeta::new(
            "references.json",
            "tracés de référence, cotations publiées, groupes d'itinéraires",
        ),
        FileMeta::new(
            "profiles/<id>.json",
            "profil de chaque tracé tous les 10 m (cache)",
        ),
        FileMeta::new(
            "evaluation.json",
            "accord avec les cotations publiées, matrices, calibrage, validation en laissant un itinéraire de côté",
        ),
        FileMeta::new("review.txt", "textes des tracés relus à la main"),
    ];
    m.parameters = serde_json::json!({
        "default": params_json(&Params::default()),
        "corridor": Corridor::default(),
        "zones_at": skimap_common::dates::rfc3339(at()),
    });
    m.stats = serde_json::json!({
        "cases": ev["cases"],
        "groups": ev["groups"],
        "agreement_leave_one_route_out": ev["agreement_leave_one_route_out"],
    });
    m.notes = vec![
        "Niveau calculé, jamais une garantie ; jamais « facile » à travers du terrain exposé aux avalanches (modèle).".into(),
    ];
    m.write(&out_dir(layout)?)?;
    Ok(())
}

fn main() -> Res<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("refs") => cmd_refs(&layout, &dl),
        Some("profiles") => cmd_profiles(&layout, &dl, rest),
        Some("roads") => cmd_roads(&layout, &dl, rest),
        Some("evaluate") => cmd_evaluate(&layout, rest),
        Some("review") => cmd_review(&layout),
        Some("show") => cmd_show(&layout, rest),
        Some("bench") => cmd_bench(&layout, &dl, rest),
        _ => Err("usage: srndiff refs|profiles|roads|evaluate|review|show|bench".into()),
    }
}
