//! C11 (#130): the arrival time learned per group against C5 alone, on synthetic outings
//! of both sites (`skimap_plan::pace_eval`): a family with children of 6 and 9 on a short
//! loop, a fast adult on the site's first loop; leave-future-out cross-validation (learn
//! on the outings before, test on the next). The example `pace_eval` prints the full
//! tables (three groups, eight days).
//!
//! Reads `data/out/<site>/` (fails if absent; `fixtures/` once Q1 is merged). Synthetic
//! traces only: the real validation waits for real outings (T1).

use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use skimap_core::calibration::{GroupProfile, Level};
use skimap_plan::pace_eval::{self, Case, Day};
use skimap_plan::plan::{self, PlanRequest};
use skimap_plan::presets;
use skimap_plan::scenario::Behaviour;

fn layout() -> DataLayout {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    let l = DataLayout::new(root);
    for s in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let g = l.out_dir(s).join("routes/piste_graph.json");
        assert!(g.is_file(), "données absentes : {}", g.display());
    }
    l
}

fn evaluate(
    site: &'static Site,
    req: PlanRequest,
    bh: Behaviour,
    profile: GroupProfile,
    days: u64,
) -> Vec<Day> {
    let l = layout();
    let (mut data, ctx) = presets::load(&l, site).expect("layers");
    let m = plan::morning(&data, &req);
    let p = plan::build(&mut data, &m, 0, &ctx).expect("plan");
    let g = data.graph.take().expect("graph");
    let bytes = plan::track(&p, &g).expect("track").to_bytes();
    let name = format!("{} — {}", site.id, profile.label_fr());
    let res = pace_eval::evaluate(&Case {
        name: name.clone(),
        plan: &p,
        track: &bytes,
        graph: &g,
        behaviour: bh,
        profile,
        days,
        seed: 900,
    })
    .expect("evaluate");
    for l in pace_eval::table(&name, &res, 0) {
        println!("{l}");
    }
    res
}

fn at(s: &[pace_eval::Score], frac: f64) -> pace_eval::Score {
    *s.iter()
        .find(|x| (x.frac - frac).abs() < 1e-9)
        .expect("frac")
}

#[test]
fn a_family_is_predicted_far_better_than_by_c5() {
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let res = evaluate(
            site,
            presets::family_request(site, presets::past_day(site)),
            Behaviour::family(),
            GroupProfile::family(2, &[6, 9], skimap_core::routing::Style::Classic),
            6,
        );
        assert!(
            res.iter().filter(|d| !d.switched).count() >= 5,
            "{}",
            site.id
        );
        let all = pace_eval::score(&res, 0);
        let learned = pace_eval::score(&res, 2);
        // Morning, learned on two outings or more: well under C5 alone (C5 misses the
        // family by more than an hour).
        let m = at(&learned, 0.0);
        assert!(m.mae_min[4] < 0.5 * m.mae_min[0], "{}: {m:?}", site.id);
        // During the outing: better than C5 for the rest at 25, 50 and 75 %, as good as the
        // pace extrapolated (A8, J2) at 50 % within 5 min.
        for f in [0.25, 0.5, 0.75] {
            let s = at(&all, f);
            assert!(s.mae_min[4] < s.mae_min[0], "{} {f}: {s:?}", site.id);
            assert!(s.mae_min[3] < s.mae_min[0], "{} {f}: {s:?}", site.id);
        }
        let h = at(&all, 0.5);
        assert!(h.mae_min[4] <= h.mae_min[2] + 5.0, "{}: {h:?}", site.id);
        // The ranges: the family's prior is wide and covers; learned, still 60 % or more.
        assert!(at(&all, 0.0).cover[0] >= 0.6, "{}", site.id);
        assert!(at(&all, 0.5).cover[1] >= 0.6, "{}", site.id);
    }
}

#[test]
fn a_fast_adult_is_learned() {
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let req = presets::requests(site, presets::past_day(site))[0].clone();
        let style = req.style;
        let res = evaluate(
            site,
            req,
            Behaviour::fast_adult(),
            GroupProfile::solo(style, Level::Intermediate),
            6,
        );
        let learned = pace_eval::score(&res, 2);
        let m = at(&learned, 0.0);
        // The prior expects the C5 pace with its stops: learning removes most of the bias.
        assert!(m.mae_min[4] < m.mae_min[0], "{}: {m:?}", site.id);
        assert!(m.mae_min[4] < m.mae_min[3], "{}: {m:?}", site.id);
        let h = at(&pace_eval::score(&res, 0), 0.5);
        assert!(h.mae_min[4] <= h.mae_min[0] + 2.0, "{}: {h:?}", site.id);
    }
}

/// The one testimony the priors of children of 8 and more are set on
/// (`calibration::TESTIMONY`, n = 1): a family with beginner children did
/// about 20 km a day up to red pistes, 4 h of skiing and « 6 h en tout peut-être ». The
/// prior of that group (the app's default profile) on the loops of 19 to 21.5 km up to red of
/// Chapelle-des-Bois: 3 h 30 to 4 h 30 moving, 5 h 30 to 6 h 30 in total (median), planned
/// on the slow side (80 %). Cervières (no loop of 20 km from Le Laus): printed.
#[test]
fn the_testimony_falls_in_the_prior_range() {
    use skimap_assist::suggest;
    use skimap_core::calibration::{ProfileModel, TESTIMONY, terrain_ratio};
    use skimap_core::graph::Difficulty;
    let l = layout();
    let fam = ProfileModel::new(GroupProfile::app_default());
    println!("{TESTIMONY}\n{}", fam.profile.label_fr());
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let (data, _) = presets::load(&l, site).expect("layers");
        let g = data.graph.as_ref().expect("graph");
        let base = presets::requests(site, presets::past_day(site)).remove(0);
        let mut seen = std::collections::BTreeSet::new();
        let mut near_20 = 0;
        for budget in [150.0, 180.0, 210.0, 240.0] {
            let r = suggest::Request {
                label: "témoignage".into(),
                start: base.start,
                start_name: base.start_name.clone(),
                start_time: base
                    .date
                    .and_time(chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
                budget_min: budget,
                style: fam.profile.style,
                return_before: None,
                max_difficulty: None,
                max_results: 10,
                lang: skimap_common::Lang::Fr,
            };
            for x in suggest::suggest(&data, &r).suggestions {
                let red = x
                    .legs
                    .iter()
                    .filter_map(|lg| g.edge(lg.edge).attrs.difficulty)
                    .all(|d| d <= Difficulty::Intermediate);
                if !(14.0..=24.0).contains(&x.length_km) || !seen.insert(x.label.clone()) {
                    continue;
                }
                let m = x.profile.model_by_terrain(terrain_ratio(&x.eta));
                let p = fam.params.predict(m);
                println!(
                    "{} : {:.1} km, D+ {:.0} m, {}, C5 {:.0} min ; famille : mouvement {:.0} min, arrêts {:.0}, médiane {:.0} ({:.0} à {:.0}), prévu (80 %) {:.0} min",
                    site.id,
                    x.length_km,
                    x.gain_m,
                    if red {
                        "jusqu'au rouge"
                    } else {
                        "avec du noir"
                    },
                    m.iter().sum::<f64>() / 60.0,
                    p.moving_s / 60.0,
                    p.stop_s / 60.0,
                    p.median_s / 60.0,
                    p.lo_s / 60.0,
                    p.hi_s / 60.0,
                    fam.plan_s(&p) / 60.0
                );
                if site.id == "chapelle-des-bois" && red && (19.0..=21.5).contains(&x.length_km) {
                    near_20 += 1;
                    assert!(
                        (210.0..=270.0).contains(&(p.moving_s / 60.0)),
                        "{}: moving {:.0} min",
                        x.label,
                        p.moving_s / 60.0
                    );
                    assert!(
                        (330.0..=390.0).contains(&(p.median_s / 60.0)),
                        "{}: total {:.0} min",
                        x.label,
                        p.median_s / 60.0
                    );
                    // The day of the testimony (about 6 h) is inside the range.
                    assert!(p.lo_s < 6.0 * 3600.0 && 6.0 * 3600.0 < p.hi_s);
                }
            }
        }
        if site.id == "chapelle-des-bois" {
            assert!(near_20 >= 1, "loops of about 20 km: {near_20}");
        }
    }
}

/// The morning of the app's default family with 6 h, classic, up to red, from 9 h, on
/// both sites: loops planned on the slow side of the range (9 to 16 km), the loop of
/// about 20 km only as the last option « si tout va bien »; an hour of light kept.
#[test]
fn a_family_morning_is_cautious() {
    use skimap_core::calibration::ProfileModel;
    use skimap_core::graph::Difficulty;
    let l = layout();
    let fam = ProfileModel::new(GroupProfile::app_default());
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let (mut data, ctx) = presets::load(&l, site).expect("layers");
        let base = presets::requests(site, presets::past_day(site)).remove(0);
        let req = PlanRequest {
            label: "famille avec enfants débutants, 6 h, classique, jusqu'au rouge, départ 9 h"
                .into(),
            start_time: Some(chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
            budget_min: 360.0,
            style: fam.profile.style,
            return_before: None,
            max_difficulty: Some(Difficulty::Intermediate),
            max_results: 3,
            ..base
        };
        let m = plan::morning_for(&data, &req, Some(&fam));
        for t in &m.suggestions.text {
            println!("{t}");
        }
        let s = &m.suggestions.suggestions;
        assert!(!s.is_empty(), "{}", site.id);
        let (hopeful, cautious): (Vec<_>, Vec<_>) = s.iter().partition(|x| x.if_all_goes_well);
        assert!(!cautious.is_empty());
        for x in &cautious {
            let gt = x.group.as_ref().unwrap();
            // 9 km and more: since X16 (#150) real loops come before out-and-backs, and
            // Le Laus has only 23.5 km of trails in cycles: its cautious loop is a real
            // one of 9.8 km (4 h 48 planned on the slow side, back about 13 h 50 for a
            // limit at 16 h 43), no longer an out-and-back of 12 to 16 km.
            assert!(
                (9.0..=16.0).contains(&x.length_km),
                "{}: {:.1} km",
                site.id,
                x.length_km
            );
            // Planned on the slow side: the time asked holds it, the median is well under.
            assert!(gt.plan_min > gt.median_min && gt.plan_min <= 1.1 * 360.0);
        }
        // « Si tout va bien »: never first, longer, fits at the median only.
        assert!(hopeful.len() <= 1);
        if let Some(h) = hopeful.first() {
            assert!(s.last().unwrap().if_all_goes_well);
            let gt = h.group.as_ref().unwrap();
            assert!(h.length_km > cautious.iter().map(|x| x.length_km).fold(0.0, f64::max));
            assert!(gt.median_min <= 1.1 * 360.0 && gt.plan_min > 1.1 * 360.0);
            println!("si tout va bien : {:.1} km", h.length_km);
        }
        if site.id == "chapelle-des-bois" {
            let h = hopeful.first().expect("an option « si tout va bien »");
            assert!(h.length_km >= 18.0, "{:.1} km", h.length_km);
        }
        // The plan of the first loop: the group's time and an hour of light.
        let p = plan::build(&mut data, &m, 0, &ctx).expect("plan");
        let pc = p.pace.as_ref().expect("the group of the morning");
        println!("{}", pc.text);
        assert!(pc.planned > pc.arrival);
        let tp = skimap_core::turnaround::TurnaroundParams::default();
        let dl = skimap_core::turnaround::deadline(p.date, p.request.start, &tp)
            .map(|d| skimap_core::ephemeris::local_time(d.deadline))
            .unwrap();
        assert_eq!(
            p.turnaround.deadline.unwrap(),
            dl - chrono::Duration::minutes(30)
        );
        assert!(pc.planned <= p.turnaround.limit.unwrap());
    }
}

/// X14 (#148): a group nobody described (a plan without a profile). During the outing, the
/// stops still to come are those of the profiles the day fits (`pace::day_fit`, mixture
/// of `pace::profile_presets`), not those of a lone adult: the family of 6 and 9 is no
/// longer predicted back half an hour too early at a quarter of the route, and adults are
/// not predicted with a family's stops.
#[test]
fn a_group_without_profile_is_predicted_at_its_own_stops() {
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let req = presets::family_request(site, presets::past_day(site));
        let res = evaluate(
            site,
            req,
            Behaviour::family(),
            GroupProfile::family(2, &[6, 9], skimap_core::routing::Style::Classic),
            20,
        );
        let q = at(&pace_eval::score(&res, 0), 0.25);
        let (mae, bias, cover) = q.unknown;
        println!(
            "{} famille sans profil à 25 % : erreur {mae:.1} min, biais {bias:+.1} min, couverture {:.0} % ({} jours)",
            site.id,
            cover * 100.0,
            q.n
        );
        assert!(q.n >= 15, "{}: {q:?}", site.id);
        assert!(bias > -15.0, "{}: biais {bias:+.1} min", site.id);
        assert!(cover >= 0.7, "{}: couverture {cover:.2}", site.id);
        let req = presets::requests(site, presets::past_day(site))[0].clone();
        let style = req.style;
        for bh in [Behaviour::normal_adult(), Behaviour::fast_adult()] {
            let res = evaluate(
                site,
                req.clone(),
                bh,
                GroupProfile::solo(style, Level::Intermediate),
                20,
            );
            let q = at(&pace_eval::score(&res, 0), 0.25);
            let (mae, bias, cover) = q.unknown;
            println!(
                "{} adulte sans profil à 25 % : erreur {mae:.1} min, biais {bias:+.1} min, couverture {:.0} %",
                site.id,
                cover * 100.0
            );
            assert!(mae <= 12.0 && bias.abs() <= 10.0, "{}: {q:?}", site.id);
            assert!(cover >= 0.75, "{}: couverture {cover:.2}", site.id);
        }
    }
}

/// X14 (#148): the range during the outing of a group learned on its outings is
/// calibrated on them (`calibration::Spread::day_factor`, each outing replayed from its
/// checkpoints at a quarter, half and three quarters of the route): it covers the arrival
/// about 8 times in 10, not every time (21/11/2026, 24 days, 252 checkpoints: 97 % before,
/// 90 % after; with 40 days `pace_eval` gives 79 to 93 %, the range of the first outings
/// still drawn to the model's own).
#[test]
fn the_range_during_the_outing_is_calibrated_on_the_groups_outings() {
    let (mut inside, mut n) = (0.0, 0usize);
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let req = presets::requests(site, presets::past_day(site))[0].clone();
        let style = req.style;
        for bh in [Behaviour::normal_adult(), Behaviour::fast_adult()] {
            let res = evaluate(
                site,
                req.clone(),
                bh,
                GroupProfile::solo(style, Level::Intermediate),
                24,
            );
            for f in [0.25, 0.5, 0.75] {
                let q = at(&pace_eval::score(&res, 3), f);
                println!(
                    "{} {f} : couverture {:.0} % ({} jours)",
                    site.id,
                    q.cover[1] * 100.0,
                    q.n
                );
                inside += q.cover[1] * q.n as f64;
                n += q.n;
            }
        }
    }
    let cover = inside / n as f64;
    println!(
        "adultes, pendant la sortie : couverture {:.0} % sur {n}",
        cover * 100.0
    );
    assert!(n >= 200, "{n}");
    assert!((0.72..=0.92).contains(&cover), "couverture {cover:.2}");
}
