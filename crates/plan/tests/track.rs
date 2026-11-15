//! X13 (#147): the flat table of every plan of both sites, its plans B written on the
//! loop's block (shared head and tail) against the same plans B written whole: size, and
//! the same route read through the view (geometry, model times, A8 profile, projections).
//!
//! Reads `data/out/<site>/` (fails if absent; `fixtures/` once Q1 is merged).
//!
//! ```sh
//! cargo test --release -p skimap-plan --test track -- --nocapture
//! ```

use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;
use skimap_plan::course::Course;
use skimap_plan::plan::{self, PlannedAnnouncement};
use skimap_plan::presets;
use skimap_plan::track::{RouteTable, SIMPLIFY_M, Track, TrackView};

#[test]
fn plans_b_on_the_loop_are_smaller_and_read_the_same() {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    let layout = DataLayout::new(root);
    let mut plans = 0;
    for site in SITES {
        assert!(
            layout
                .out_dir(site)
                .join("routes/piste_graph.json")
                .is_file(),
            "données absentes : {}",
            layout.out_dir(site).display()
        );
        let (mut data, ctx) = presets::load(&layout, site).expect("layers");
        for req in presets::requests(site, presets::past_day(site)) {
            let m = plan::morning(&data, &req);
            if m.suggestions.suggestions.is_empty() {
                continue;
            }
            let p = plan::build(&mut data, &m, 0, &ctx).expect("plan");
            let g = data.graph.as_ref().expect("graph");
            let t = plan::track(&p, g).expect("track");
            // The same table with every plan B written whole (the format before X13).
            let course = Course::new(g, &p.route.legs, &p.eta).unwrap();
            let origin = course.line().point_at(0.0);
            let plane = skimap_core::graph::LocalPlane::new(origin);
            let mut whole = Track {
                origin,
                routes: vec![t.routes[0].clone()],
            };
            for r in &t.routes[1..] {
                let bi = r.id as usize - 1;
                let b = &p.plans_b[bi];
                let f = &b.fallback;
                let cb = Course::new(g, &f.full_legs(&p.route.legs), &p.eta).unwrap();
                // The announcements of `plan::track`: the loop's before the divergence,
                // then the plan B's.
                let mut anns: Vec<(u32, u32, &PlannedAnnouncement)> = p
                    .announcements
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        matches!(a.trigger, plan::Trigger::At { s, .. } if s < f.s_div - plan::TURN_LEAD_M)
                    })
                    .map(|(i, a)| (0, i as u32, a))
                    .collect();
                anns.extend(
                    b.announcements
                        .iter()
                        .enumerate()
                        .map(|(i, a)| ((bi + 1) as u32, i as u32, a)),
                );
                whole
                    .routes
                    .push(RouteTable::new(r.id, f.s_div, &cb, &plane, &anns));
            }
            let (sb, wb) = (t.to_bytes(), whole.to_bytes());
            let (sv, wv) = (TrackView::new(&sb).unwrap(), TrackView::new(&wb).unwrap());
            assert_eq!(sv.len(), wv.len());
            let shared = t.routes[1..].iter().filter(|r| r.shared.head > 0).count();
            let (mut dmax, mut ea, mut eb, mut pmax) = (0.0f32, 0.0f32, 0.0f32, 0.0f64);
            for i in 1..sv.len() {
                let (a, b) = (sv.route(i).unwrap(), wv.route(i).unwrap());
                assert_eq!(a.id(), b.id());
                let f = &p.plans_b[a.id() as usize - 1].fallback;
                let cb = Course::new(g, &f.full_legs(&p.route.legs), &p.eta).unwrap();
                assert!((a.length() - b.length()).abs() < 1e-2);
                let mut s = 0.0f32;
                while s <= b.length() {
                    let (pa, pb) = (a.point_at(s), b.point_at(s));
                    dmax = dmax.max((pa[0] - pb[0]).hypot(pa[1] - pb[1]));
                    // Model time against the course's (the profile is sampled every 50 m
                    // in both).
                    let truth = cb.model_s_at(f64::from(s)) as f32;
                    ea = ea.max((a.model_s_at(s) - truth).abs());
                    eb = eb.max((b.model_s_at(s) - truth).abs());
                    // Projections of a point 20 m off the route.
                    let q = [pa[0] + 20.0, pa[1]];
                    let (x, y) = (
                        a.project_window(q, s - 100.0, s + 100.0),
                        b.project_window(q, s - 100.0, s + 100.0),
                    );
                    let ((xs, xd), (ys, yd)) = (x.unwrap(), y.unwrap());
                    assert!(
                        (xd - yd).abs() < 2.5 * SIMPLIFY_M as f32,
                        "{}: {s}: {x:?} {y:?}",
                        site.id
                    );
                    let _ = (xs, ys);
                    let (x, y) = (a.project_grid(q, s), b.project_grid(q, s));
                    assert_eq!(x.is_some(), y.is_some(), "{}: {s}", site.id);
                    // The nearest pass, the one nearest to the cursor on a tie of 5 m: a
                    // pass a few metres nearer or farther after the simplification may win.
                    if let (Some((_, xd)), Some((_, yd))) = (x, y) {
                        assert!(
                            (xd - yd).abs() < 5.0 + 2.5 * SIMPLIFY_M as f32,
                            "{} route {}: {s}: {x:?} {y:?}",
                            site.id,
                            a.id()
                        );
                    }
                    s += 20.0;
                }
                let (pa, pb) = (a.to_profile(), b.to_profile());
                for (u, v) in [
                    (pa.length_m(), pb.length_m()),
                    (pa.total_s(), pb.total_s()),
                    (*pa.back_s.last().unwrap(), *pb.back_s.last().unwrap()),
                ] {
                    pmax = pmax.max((u - v).abs());
                }
            }
            println!(
                "{} {} : table {} Ko au lieu de {} Ko ({:.1} fois moins), {} plans B sur la boucle sur {} ; écarts au plan B entier : géométrie {dmax:.2} m, fin du profil {pmax:.3} ; temps du modèle à 20 m près, écart maximal au profil complet {ea:.1} s (plan B entier {eb:.1} s)",
                site.id,
                req.label,
                sb.len() / 1024,
                wb.len() / 1024,
                wb.len() as f64 / sb.len() as f64,
                shared,
                t.routes.len() - 1
            );
            assert_eq!(shared, t.routes.len() - 1, "every plan B on the loop");
            assert!(sb.len() < wb.len());
            assert!(dmax <= 2.0 * SIMPLIFY_M as f32, "{dmax}");
            assert!(ea <= eb + 2.0, "{ea} vs {eb}");
            assert!(pmax < 0.1, "{pmax}");
            plans += 1;
        }
    }
    assert_eq!(plans, 4);
}
