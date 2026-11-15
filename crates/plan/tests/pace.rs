//! C11 (#130): the arrival time learned per group against C5 alone, on synthetic outings
//! of both sites (`skimap_plan::pace_eval`): a family with children of 6 and 9 on a short
//! loop, a fast adult on the site's first loop; leave-future-out cross-validation (learn
//! on the outings before, test on the next). The example `pace_eval` prints the full
//! tables (three groups, eight days).
//!
//! Reads `data/out/<site>/` (fails if absent; `fixtures/` once Q1 is merged). Synthetic
//! traces only: the real validation waits for real outings (T1).

use skimap_common::layout::DataLayout;
use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS, Site};
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
