//! One outing over two installed packs of neighbouring areas (H2 #133), on real packs of
//! two massifs: the morning joins the neighbour's pack to the pack of the start, a loop
//! that crosses into the neighbouring area is validated, its plan says the area entered
//! and its trail pass at the crossing, and the outing opens for tracking on the merged
//! graph.
//!
//! ```sh
//! SKIMAP_DATA_DIR=~/projects/skimap/data \
//!   cargo test --release -p skimap-ffi --test two_areas -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use skimap_ffi::card::Lang;
use skimap_ffi::neighbours::{NEIGHBOURS_FILE, graph_data};
use skimap_ffi::tracking::open_tracking;
use skimap_ffi::tracking_day::{DayRequest, day_pack_starts, open_day_planner_with};
use skimap_pack::Pack;
use skimap_plan::plan::{DayPlan, KeyKind};

fn data() -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    PathBuf::from(root)
}

/// Links the packs of `sites` under `root` as the app installs them.
fn install(root: &Path, sites: &[&str]) -> Vec<String> {
    sites
        .iter()
        .map(|s| {
            let at = root.join(s);
            std::os::unix::fs::symlink(data().join("out").join(s).join("pack"), &at).unwrap();
            at.display().to_string()
        })
        .collect()
}

/// Tries the starts of `primary` and outing lengths until a validated loop enters the
/// neighbouring area; checks its plan and that it opens for tracking.
fn crossing(primary: &str, neighbour: &str) {
    let root = tempfile::tempdir().unwrap();
    let dirs = install(root.path(), &[primary, neighbour]);
    let work = tempfile::tempdir().unwrap();
    let t0 = Instant::now();
    let planner = open_day_planner_with(
        dirs[0].clone(),
        dirs.clone(),
        work.path().join("view").display().to_string(),
        Lang::Fr,
    )
    .unwrap();
    let open_ms = t0.elapsed().as_millis();
    assert_eq!(planner.neighbour_sites(), vec![neighbour.to_owned()]);
    let own: HashSet<Option<i64>> = graph_data(&Pack::open(PathBuf::from(&dirs[0])).unwrap())
        .unwrap()
        .edges
        .iter()
        .map(|e| e.attrs.osm_way_id)
        .collect();
    let starts = day_pack_starts(dirs[0].clone(), Lang::Fr);
    let mut found = None;
    'search: for budget in [180.0, 240.0, 300.0, 360.0] {
        for st in &starts {
            let req = DayRequest {
                label: "sortie".into(),
                start: st.clone(),
                date: "2026-11-21".into(),
                start_time: Some("09:00".into()),
                earliest: "08:00".into(),
                budget_min: budget,
                skating: false,
                return_before: None,
                max_difficulty: None,
            };
            let m = planner.morning(req, None).unwrap();
            for i in 0..m.loops.len() {
                let plan_dir = work.path().join(format!("plan-{budget}-{i}"));
                let v = planner
                    .validate(
                        u32::try_from(i).unwrap(),
                        None,
                        plan_dir.display().to_string(),
                    )
                    .unwrap();
                let json = std::fs::read_to_string(plan_dir.join("plan.json")).unwrap();
                let p = DayPlan::from_json(&json).unwrap();
                let entered: Vec<_> = p
                    .key_points
                    .iter()
                    .filter(|k| k.kind == KeyKind::DomainEntered)
                    .collect();
                let merged =
                    skimap_ffi::neighbours::load_graph_set(Path::new(&dirs[0]), &dirs).unwrap();
                let beyond_m: f64 = p
                    .route
                    .legs
                    .iter()
                    .filter(|l| !own.contains(&merged.edge(l.edge).attrs.osm_way_id))
                    .map(|l| (l.to_m - l.from_m).abs())
                    .sum();
                eprintln!(
                    "  {budget} min from « {} », loop {i}: {:.1} km, entered {:?}, {beyond_m:.0} m on the neighbour's own trails",
                    st.name,
                    v.length_km,
                    entered.iter().map(|k| k.name.clone()).collect::<Vec<_>>()
                );
                if entered.is_empty() || beyond_m < 100.0 {
                    continue;
                }
                found = Some((st.name.clone(), budget, v, p, plan_dir, beyond_m));
                break 'search;
            }
        }
    }
    let (start, budget, v, p, plan_dir, beyond_m) =
        found.unwrap_or_else(|| panic!("{primary}: no loop entering {neighbour}"));
    println!(
        "{primary} + {neighbour}: open {open_ms} ms; from « {start} », {budget} min: {} ({:.1} km, {:.1} km on trails only the neighbour's pack has)",
        v.label,
        v.length_km,
        beyond_m / 1000.0
    );
    for k in p
        .key_points
        .iter()
        .filter(|k| k.kind == KeyKind::DomainEntered)
    {
        println!(
            "  key point: km {:.2} {:?} {:?}",
            k.s / 1000.0,
            k.kind,
            k.name
        );
    }
    let said: Vec<_> = p
        .announcements
        .iter()
        .filter(|a| a.kind == "domain_entered")
        .collect();
    assert!(!said.is_empty(), "the crossing is said");
    for a in &said {
        println!("  said: {:?} « {} »", a.trigger, a.text);
        assert!(a.text.contains("redevance"), "{}", a.text);
    }
    assert!(
        v.key_points
            .iter()
            .any(|k| k.kind.contains("autre domaine nordique")),
        "{:?}",
        v.key_points
    );
    assert!(plan_dir.join(NEIGHBOURS_FILE).is_file());
    // The outing opens on the merged graph (the plan's fingerprint and edges checked).
    let session = open_tracking(plan_dir.display().to_string(), None, "fr".into())
        .unwrap_or_else(|e| panic!("{primary}: tracking: {e}"));
    drop(session);
}

#[test]
#[ignore = "full-data: needs data/out/<site>/pack of the Jura and the Vosges"]
fn jura_outing_over_two_packs() {
    crossing("chapelle-des-bois", "site-nordique-de-mouthe");
}

/// The Vosges: the two packs are joined and their graphs meet (no loop of the morning up
/// to 6 h from the starts of Lac Blanc or Gérardmer crosses into the other area: the
/// crossing itself is proved on the Jura).
#[test]
#[ignore = "full-data: needs data/out/<site>/pack of the Jura and the Vosges"]
fn vosges_packs_joined() {
    let root = tempfile::tempdir().unwrap();
    let dirs = install(root.path(), &["lac-blanc", "gerardmer"]);
    let work = tempfile::tempdir().unwrap();
    let planner = open_day_planner_with(
        dirs[0].clone(),
        dirs.clone(),
        work.path().join("view").display().to_string(),
        Lang::Fr,
    )
    .unwrap();
    assert_eq!(planner.neighbour_sites(), vec!["gerardmer".to_owned()]);
    let a = graph_data(&Pack::open(PathBuf::from(&dirs[0])).unwrap()).unwrap();
    let b = graph_data(&Pack::open(PathBuf::from(&dirs[1])).unwrap()).unwrap();
    let t0 = Instant::now();
    let m = skimap_ffi::neighbours::merge_graphs(&a, std::slice::from_ref(&b));
    let merge_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let ways: HashSet<_> = m.edges.iter().map(|e| e.attrs.osm_way_id).collect();
    let all: HashSet<_> = a
        .edges
        .iter()
        .chain(&b.edges)
        .map(|e| e.attrs.osm_way_id)
        .collect();
    println!(
        "lac-blanc + gerardmer: {} + {} edges → {} edges, {} nodes, every way kept: {}, merge {merge_ms:.1} ms",
        a.edges.len(),
        b.edges.len(),
        m.edges.len(),
        m.nodes.len(),
        ways == all
    );
    assert_eq!(ways, all, "every OSM way of both packs");
    assert!(
        m.edges.len() < a.edges.len() + b.edges.len(),
        "shared ways not doubled"
    );
    skimap_core::graph::PisteGraph::new(m).unwrap();
}
