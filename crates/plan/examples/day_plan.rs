//! Day plans of the two sites on archived past days (J1, J2): writes
//! `data/out/<site>/plan/plan_<date>_<rank>.json` and `.txt` and prints the timings.
//!
//! ```sh
//! cargo run --release -p skimap-plan --example day_plan            # both sites
//! cargo run --release -p skimap-plan --example day_plan -- <site>
//! ```

use std::time::Instant;

use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;
use skimap_plan::plan::{self, DayPlan};
use skimap_plan::presets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let only: Vec<String> = std::env::args().skip(1).collect();
    let layout =
        DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into()));
    for site in SITES
        .iter()
        .filter(|s| only.is_empty() || only.iter().any(|o| o == s.id))
    {
        let t0 = Instant::now();
        let (mut data, ctx) = presets::load(&layout, site)?;
        let load = t0.elapsed().as_secs_f64();
        let date = presets::past_day(site);
        for req in presets::requests(site, date) {
            let t1 = Instant::now();
            let m = plan::morning(&data, &req);
            let morning = t1.elapsed().as_secs_f64();
            if m.suggestions.suggestions.is_empty() {
                println!(
                    "{} {} : aucune boucle ({})",
                    site.id,
                    req.label,
                    m.notes.join(" ; ")
                );
                continue;
            }
            let t2 = Instant::now();
            let p = plan::build(&mut data, &m, 0, &ctx)?;
            let build = t2.elapsed().as_secs_f64();
            let json = p.to_json()?;
            let back = DayPlan::from_json(&json)?;
            assert!(
                plan::json_close(
                    &serde_json::to_value(&back)?,
                    &serde_json::to_value(&p)?,
                    1e-12
                ),
                "JSON round trip"
            );
            let files = plan::write(&layout, site, &p, &data)?;
            let trk = std::fs::read(&files[2])?;
            let view = skimap_plan::track::TrackView::new(&trk)?;
            let pts: usize = (0..view.len())
                .map(|i| view.route(i).map_or(0, |r| r.points()))
                .sum();
            println!(
                "{} {} : lecture des couches {:.2} s, matin (A1, A2, A9, A5, A6) {:.3} s, plan {:.3} s ; {} plans B, {} annonces prévues, {} clips dont {} absents ; JSON {} Ko, table {} Ko ({} itinéraires, {} sommets) ; {}",
                site.id,
                req.label,
                load,
                morning,
                build,
                p.plans_b.len(),
                p.announcements.len(),
                p.clips.required.len(),
                p.clips.missing.len(),
                json.len() / 1024,
                trk.len() / 1024,
                view.len(),
                pts,
                files[1].display()
            );
        }
    }
    Ok(())
}
