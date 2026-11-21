//! The day plan in English (VO6, #129): the morning (best time, wax, equipment, notes),
//! the plan of every loop proposed (its text, plans B, synthesis, the group's time) and
//! the start names, built in English on the archived day of both sites, without any
//! French left once the proper names of the data are taken out; the French plan is the
//! same as before (`DayPlan::text_fr`). The texts the data does not reach (pace
//! proposals, recap of the outing, every key point) are checked on their closed lists.
//!
//! Reads `data/out/<site>/` (`SKIMAP_DATA_DIR`, as the other tests of this crate).

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use jiff::civil::Date;
use skimap_assist::inputs::SiteData;
use skimap_common::Lang;
use skimap_common::layout::DataLayout;
use skimap_core::calibration::{GroupProfile, ProfileModel};
use skimap_plan::pace::{self, PaceReason, ProposalKind};
use skimap_plan::plan::{self, KeyKind};
use skimap_plan::presets;
use skimap_plan::session::End;

fn layout() -> DataLayout {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    DataLayout::new(root)
}

/// Proper names of the data of a site: they stay as published.
fn names(d: &SiteData, ctx: &plan::Context) -> Vec<String> {
    let mut v = vec![d.site.name.to_owned()];
    v.extend(d.domains.iter().map(|x| x.name.clone()));
    // The clips of the area names (said when a route enters another area, H2 #133), listed
    // by identifier when the pack lacks them.
    v.extend(
        d.domains
            .iter()
            .map(|x| format!("name.{}", skimap_voice::names::slug(&x.name))),
    );
    if let Some(s) = &d.status {
        v.extend(s.itineraries.iter().map(|i| i.name.clone()));
        v.extend(s.messages.iter().cloned());
        v.extend(s.conflicts.iter().cloned());
    }
    for p in &d.pistes {
        v.extend(p.name.iter().cloned());
        v.extend(p.relations.iter().cloned());
    }
    if let Some(w) = &d.weather {
        v.extend(w.points.iter().map(|p| p.name.clone()));
        v.extend(w.points.iter().map(|p| p.id.clone()));
    }
    if let Some((l, _)) = &d.zones {
        v.extend(l.zones.iter().map(|z| z.props.name.clone()));
    }
    for r in &ctx.refuges {
        v.extend(r.name.iter().cloned());
        v.push(r.kind.clone());
    }
    // The reasons of the layers that could not be read (local paths, reader errors).
    for m in d.missing.iter().chain(&ctx.missing) {
        if let Some((_, why)) = m.split_once(" : ") {
            v.push(why.to_owned());
        }
    }
    v
}

fn check(id: &str, what: &str, text: &str, names: &[String]) {
    let left = fixtures::french::french_in(text, names);
    assert!(
        left.is_empty(),
        "{id} {what}: French {left:?} in « {text} »"
    );
    assert!(
        !text.to_lowercase().contains(" safe"),
        "{id} {what}: « {text} »"
    );
}

#[test]
fn the_day_plan_in_english() {
    let l = layout();
    let model = ProfileModel::new(GroupProfile::app_default());
    for id in ["cervieres", "chapelle-des-bois"] {
        let site = skimap_common::site::site(id).expect("evaluation site");
        let (mut data, ctx) = presets::load(&l, site).expect("layers");
        let mut names = names(&data, &ctx);
        // The name of a group is the skier's.
        names.push(model.profile.name.clone());
        // Starts without an English phrase are proper names (« Le Laus »).
        for st in skimap_common::site::starts(site) {
            if st.phrase_en.is_none() {
                names.push(st.phrase.clone());
                names.push(st.name.clone());
            }
        }
        let day = presets::past_day(site);
        let mut reqs = presets::requests_in(site, day, Lang::En);
        let mut family = presets::family_request(site, day);
        family.start_name = reqs[0].start_name.clone();
        reqs.push(family);
        // The request labels are the presets of the site table (the app writes its own).
        names.extend(reqs.iter().map(|r| r.label.clone()));
        let fr = presets::requests(site, day);
        for (r, f) in reqs.iter().zip(&fr) {
            check(id, "start", &r.start_name, &names);
            if f.start_name.starts_with("le départ") {
                assert_ne!(r.start_name, f.start_name);
            }
        }
        let mut built = 0;
        for date in [day, Date::new(2027, 2, 20).expect("date")] {
            for r in &reqs {
                let mut r = r.clone();
                r.date = date;
                for group in [None, Some(&model)] {
                    let m = plan::morning_in(&data, &r, group, Lang::En);
                    let mut texts: Vec<String> = m.notes.clone();
                    if let Some(b) = &m.best_time {
                        texts.push(b.text.clone());
                    }
                    if let Some(w) = &m.wax {
                        texts.push(w.text.clone());
                        texts.extend(w.morning.reasons.iter().cloned());
                        texts.extend(w.afternoon.reasons.iter().cloned());
                    }
                    texts.extend(m.equipment.iter().map(|e| e.text.clone()));
                    for t in &texts {
                        check(id, "morning", t, &names);
                    }
                    // The loop labels are made of trail names.
                    let mut names = names.clone();
                    names.extend(m.suggestions.suggestions.iter().map(|s| s.label.clone()));
                    for c in 0..m.suggestions.suggestions.len() {
                        let mut p = plan::build(&mut data, &m, c, &ctx).expect("plan");
                        assert_eq!(p.lang, Lang::En);
                        if group.is_none() {
                            let g = data.graph.as_ref().expect("graph");
                            pace::with_pace(&mut p, g, &model).expect("pace");
                        }
                        built += 1;
                        for line in p.text().lines() {
                            // The announcements say the trail names as the voice says them.
                            if line.contains('“') {
                                continue;
                            }
                            check(id, "plan", line, &names);
                        }
                        for b in &p.plans_b {
                            check(id, "plan B", &b.fallback.text, &names);
                        }
                        let pt = &p.pace.as_ref().expect("group time").text;
                        check(id, "pace", pt, &names);
                        // The French text of the same plan keeps its French headings.
                        assert!(p.text_fr().contains("Points clés"));
                    }
                }
            }
        }
        assert!(built > 0, "{id}: plans built");
    }
}

#[test]
fn closed_lists_in_english() {
    let t = skimap_plan::text::of(Lang::En);
    let at = Date::new(2027, 2, 20).expect("date").at(14, 3, 0, 0);
    let mut all: Vec<String> = Vec::new();
    for r in [
        PaceReason::Slower,
        PaceReason::Pauses,
        PaceReason::SlowerAndPauses,
    ] {
        for k in [
            ProposalKind::ShortLoop,
            ProposalKind::NearestExit,
            ProposalKind::Keep,
        ] {
            for s in [None, Some(25.0)] {
                all.push(pace::proposal_text_in(
                    r,
                    35.0,
                    k,
                    (at, at, at),
                    s,
                    Lang::En,
                ));
            }
        }
    }
    for end in [End::Arrived, End::Abandoned] {
        all.push(t.recap(
            end,
            12.3,
            95.0,
            12.0,
            80.0,
            14.0,
            Some(1.25),
            1,
            7,
            &["X".into()],
        ));
        all.push(t.recap(end, 12.3, 95.0, 12.0, 80.0, 14.0, None, 0, 0, &[]));
    }
    for k in [
        KeyKind::Start,
        KeyKind::Turn,
        KeyKind::Junction,
        KeyKind::Refuge,
        KeyKind::NoNetworkStart,
        KeyKind::NoNetworkEnd,
        KeyKind::AvalancheStart,
        KeyKind::AvalancheEnd,
        KeyKind::Finish,
    ] {
        all.push(t.p_key_kind(k).into());
    }
    let closed = t.fb_closed(t.fb_unnamed_trail(), Some((1200.0, 2500.0)));
    all.extend([
        t.p_turnaround(Some(at), true),
        t.p_turnaround(Some(at), false),
        t.p_turnaround(None, false),
        t.p_arrival(at, at, Some(at), Some(at)),
        t.p_when(None, false),
        t.p_when(None, true),
        t.p_when(Some(1500.0), false),
        t.p_latest_exit(1500.0, at),
        t.p_unnamed_shelter("X"),
        t.fb_exit(&t.fb_at(Some(1500.0)), 8000.0, 12000.0, 1800.0, "X"),
        t.fb_no_detour(&closed),
        t.fb_detour(&closed, &t.fb_at(None), Some(3000.0), 9000.0, -300.0, "X"),
        t.fb_detour(&closed, &t.fb_at(None), None, 9000.0, 300.0, "X"),
        t.pc_text(
            "X",
            &t.pc_learned(0),
            &t.pc_duration(None, 3600.0, 5400.0),
            &t.pc_back_between(at, at),
            3600.0,
            600.0,
            4000.0,
        ),
        t.pc_text(
            "X",
            &t.pc_learned(3),
            &t.pc_duration(Some(4000.0), 3600.0, 5400.0),
            &t.pc_slow_side(4000.0, 4500.0, 8.0, at),
            3600.0,
            600.0,
            4000.0,
        ),
        t.pc_back_around(at),
        t.never_safe().into(),
        t.no_hourly_weather().into(),
        t.no_best_time(at),
    ]);
    for s in &all {
        check("closed lists", "text", s, &["X".into()]);
    }
}
