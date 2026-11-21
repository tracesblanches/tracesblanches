//! E1 (#125): the tracker allocates nothing for a fix where all goes well. A counting
//! global allocator (the only `unsafe` of the crate, test only) counts the allocations of
//! every `Tracker::push` while a normal outing of each site is replayed, and a slow family
//! outing with stops and detours on a plan with the family's profile (C11 #130: the pace
//! of the day updated on every fix, the arrival and the proposals checked every minute; its
//! phone counts the steps, X10: their reports are counted too), with the terrain slope read
//! on every fix and the layers of the day (K6 #56: the protected zones tested off the
//! route).
//!
//! Reads `data/out/<site>/` (fails if absent; `fixtures/` once Q1 is merged).

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

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
use skimap_core::calibration::{GroupProfile, ProfileModel};
use skimap_plan::pace;
use skimap_plan::plan;
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Scenario, Step};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Input, Output, Tracker, TrackerParams};

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged, only counting them.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn site_counts(site: &'static Site, family: bool, zone: bool) -> (usize, usize, usize, usize) {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    let layout = DataLayout::new(root);
    assert!(
        layout
            .out_dir(site)
            .join("routes/piste_graph.json")
            .is_file(),
        "données absentes : {} (ou fixtures/ une fois Q1 fusionné)",
        layout.out_dir(site).display()
    );
    let (mut data, ctx) = presets::load(&layout, site).expect("layers");
    let req = presets::requests(site, presets::past_day(site))[0].clone();
    let m = plan::morning(&data, &req);
    let mut p = plan::build(&mut data, &m, 0, &ctx).expect("plan");
    let g = data.graph.take().expect("graph");
    if family {
        let model = ProfileModel::new(GroupProfile::family(2, &[6, 9], req.style));
        pace::with_pace(&mut p, &g, &model).expect("pace");
    }
    let bytes = plan::track(&p, &g).expect("track").to_bytes();
    let view = TrackView::new(&bytes).unwrap();
    let mut sc = Scenario {
        name: "alloc".into(),
        start: scenario::departure_utc(&p),
        slowdown: 1.0,
        steps: vec![Step::WaitAtStart { secs: 60.0 }],
        sigma_m: 10.0,
        forest: true,
        seed: 3,
        tail_s: 300.0,
        behaviour: family.then(Behaviour::family),
        // The family's phone counts the steps (X10): their reports allocate nothing either.
        step_counter: family,
    };
    if zone {
        // Off the trails into a protected zone and back (K6 #56): the fixes in the zone
        // after its entry is said are quiet too.
        let scenario::ZoneExcursion { at_s, to, .. } =
            scenario::zone_excursion(&p, &g, &data, 40.0)
                .unwrap()
                .expect("a protected zone off the trails");
        sc.steps.push(Step::Excursion {
            at_s,
            to,
            speed_mps: 0.8,
        });
    }
    // With the terrain slope (K6 #56): read on every fix; and the layers of the day (the
    // protected zones, K6 #56).
    let slope: skimap_common::grid::Grid<u8> = skimap_core::offpiste::read_raster(
        &layout
            .out_dir(site)
            .join("offpiste")
            .join(skimap_core::offpiste::FILE_SLOPE),
    )
    .expect("slope raster");
    let run = scenario::run_with(
        &sc,
        &p,
        view,
        &g,
        TrackerParams::default(),
        Some(&data),
        Some(&slope),
    )
    .expect("run");
    let mut tr = Tracker::new(&p, view, &g, TrackerParams::default())
        .unwrap()
        .with_layers(&data)
        .with_slope(&slope);
    let mut out: Vec<Output> = Vec::with_capacity(64);
    let (mut quiet, mut quiet_alloc, mut eventful, mut fixes) = (0, 0, 0, 0);
    for i in &run.inputs {
        out.clear();
        let (pending, dropped) = (tr.speech().pending().len(), tr.speech().dropped().len());
        let before = ALLOCS.load(Ordering::Relaxed);
        tr.push(i, &mut out);
        let n = ALLOCS.load(Ordering::Relaxed) - before;
        if matches!(i, Input::Fix { .. } | Input::Steps { .. }) {
            fixes += 1;
            // Quiet: nothing out, nothing offered to the speech policy.
            let offered =
                tr.speech().pending().len() != pending || tr.speech().dropped().len() != dropped;
            if out.is_empty() && !offered {
                quiet += 1;
                quiet_alloc += n;
            } else {
                eventful += 1;
            }
        }
    }
    println!(
        "{}{}{}: {fixes} fixes et relevés de pas, {quiet} sans rien à dire ni décider ({quiet_alloc} allocations), {eventful} avec une sortie ou une annonce proposée",
        site.id,
        if family { ", famille" } else { "" },
        if zone {
            ", dans une zone protégée"
        } else {
            ""
        }
    );
    (fixes, quiet, quiet_alloc, eventful)
}

#[test]
fn a_quiet_fix_allocates_nothing() {
    for (site, family, zone) in [
        (&CERVIERES, false, false),
        (&CHAPELLE_DES_BOIS, false, false),
        (&CERVIERES, true, false),
        (&CHAPELLE_DES_BOIS, true, false),
        (&CHAPELLE_DES_BOIS, false, true),
    ] {
        let (fixes, quiet, allocs, _) = site_counts(site, family, zone);
        assert!(quiet > fixes * 9 / 10, "{quiet} of {fixes}");
        assert_eq!(
            allocs, 0,
            "{}: {allocs} allocations on {quiet} quiet fixes",
            site.id
        );
    }
}
