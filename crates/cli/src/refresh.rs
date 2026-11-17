//! `skimap refresh`: the scheduled refresh of the layers that follow their sources (H1,
//! H2): station cards and access titles of France (`skimap_stations::refresh`), then the
//! hut cards of each site (`skimap_refuges::refresh`). Each part only runs when it is due
//! (daily from 1 November to 30 April, weekly otherwise; station cards weekly), so a
//! second run the same day does nothing. A failing source keeps the previous files; the
//! run writes `data/out/france/refresh/status.json` (state of every part) and
//! `changed_sites.txt` (the sites whose pack must be rebuilt, one identifier per line:
//! [`changed_sites`]), and fails when a source is broken.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::build::Ctx;

/// Layer directory of the summary, under `data/out/france/`.
pub const LAYER: &str = "refresh";

/// Refresh of the stations layer.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StationsState {
    /// Parts run (`fiches`, `titres`).
    pub ran: Vec<String>,
    /// Parts not due.
    pub skipped: Vec<String>,
    /// Changes written to the journal.
    pub changes: usize,
    /// Sources that failed (`id: status`).
    pub broken_sources: Vec<String>,
    /// Parts whose last run failed (`part: error`).
    pub failed_parts: Vec<String>,
    /// Titles kept from a previous run or withdrawn.
    pub stale_titres: Vec<String>,
    /// Error of the refresh (no previous files to keep).
    pub error: Option<String>,
}

/// Refresh of the hut layer of a site.
#[derive(Debug, Clone, Serialize)]
pub struct RefugesState {
    /// Site.
    pub site: String,
    /// Whether the layer was rebuilt.
    pub ran: bool,
    /// Changes written to the journal.
    pub changes: usize,
    /// Error of the sources (previous files kept) or of the refresh.
    pub error: Option<String>,
}

/// Summary of a run (`status.json`).
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Start of the run (RFC 3339).
    pub checked_at: String,
    /// Whether every source answered as expected.
    pub ok: bool,
    /// `personal_sources` of the run (titles only with it).
    pub personal_sources: bool,
    /// Stations layer of France.
    pub stations: StationsState,
    /// Hut layers, per site.
    pub refuges: Vec<RefugesState>,
    /// Everything that failed, one line each.
    pub failures: Vec<String>,
    /// Summary file.
    #[serde(skip)]
    pub path: PathBuf,
}

/// Sites refreshed by default: those whose hut layer exists (built once).
pub fn default_sites(layout: &DataLayout, sites: &[&'static Site]) -> Vec<&'static Site> {
    sites
        .iter()
        .copied()
        .filter(|s| {
            layout
                .out_dir(s)
                .join(skimap_refuges::LAYER)
                .join(format!("{}.meta.json", skimap_refuges::LAYER))
                .exists()
        })
        .collect()
}

fn stations_state(ctx: &Ctx, now: DateTime<Utc>) -> StationsState {
    match skimap_stations::refresh(&ctx.layout, &ctx.dl, ctx.cfg.personal_sources, now, false) {
        Ok(r) => {
            let state = skimap_stations::refresh::State::read(&r.dir.join("refresh_state.json"));
            StationsState {
                failed_parts: state
                    .parts
                    .iter()
                    .filter_map(|(k, p)| p.error.as_ref().map(|e| format!("{k}: {e}")))
                    .collect(),
                ran: r.ran,
                skipped: r.skipped,
                changes: r.changes,
                broken_sources: r.broken_sources,
                stale_titres: r.stale_titres,
                error: None,
            }
        }
        Err(e) => StationsState {
            error: Some(e.to_string()),
            ..Default::default()
        },
    }
}

fn refuges_state(ctx: &Ctx, site: &Site, now: DateTime<Utc>) -> RefugesState {
    match skimap_refuges::refresh(site, &ctx.layout, &ctx.dl, now, false) {
        Ok(r) => RefugesState {
            site: site.id.to_owned(),
            ran: r.ran,
            changes: r.changes,
            error: r.error,
        },
        Err(e) => RefugesState {
            site: site.id.to_owned(),
            ran: true,
            changes: 0,
            error: Some(e.to_string()),
        },
    }
}

/// Every failure of a run, one line each.
fn failures(st: &StationsState, refuges: &[RefugesState]) -> Vec<String> {
    let mut f: Vec<String> = Vec::new();
    f.extend(st.error.iter().map(|e| format!("stations : {e}")));
    f.extend(
        st.failed_parts
            .iter()
            .map(|e| format!("stations, partie {e}")),
    );
    f.extend(
        st.broken_sources
            .iter()
            .map(|e| format!("stations, source {e}")),
    );
    for r in refuges {
        if let Some(e) = &r.error {
            f.push(format!("refuges {} : {e}", r.site));
        }
    }
    f
}

/// Refreshes the stations layer, then the hut layer of `sites` one after the other (the
/// downloader serialises the requests to each source anyway), and writes the summary.
pub fn refresh(ctx: &Ctx, sites: &[&Site], now: DateTime<Utc>) -> Result<Summary> {
    let stations = stations_state(ctx, now);
    let refuges: Vec<RefugesState> = sites.iter().map(|s| refuges_state(ctx, s, now)).collect();
    let failures = failures(&stations, &refuges);
    let dir = ctx.layout.france_layer_dir(LAYER)?;
    let path = dir.join("status.json");
    let summary = Summary {
        checked_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ok: failures.is_empty(),
        personal_sources: ctx.cfg.personal_sources,
        stations,
        refuges,
        failures,
        path,
    };
    skimap_common::output::write_json(&summary.path, &summary)?;
    let changed = changed_sites(&summary);
    let mut text = changed.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    let list = dir.join(CHANGED_FILE);
    std::fs::write(&list, text).map_err(|e| anyhow::anyhow!("writing {}: {e}", list.display()))?;
    Ok(summary)
}

/// File of [`changed_sites`], next to `status.json` (read by `deploy/skimap-job`).
pub const CHANGED_FILE: &str = "changed_sites.txt";

/// Sites whose pack carries something the run changed: every refreshed site when the
/// station cards or titles of France changed, otherwise the sites whose hut layer changed.
pub fn changed_sites(s: &Summary) -> Vec<String> {
    s.refuges
        .iter()
        .filter(|r| s.stations.changes > 0 || (r.ran && r.changes > 0))
        .map(|r| r.site.clone())
        .collect()
}

/// French lines of a summary.
pub fn describe(s: &Summary) -> Vec<String> {
    let list = |v: &[String]| {
        if v.is_empty() {
            "aucune".to_owned()
        } else {
            v.join(", ")
        }
    };
    let st = &s.stations;
    let mut l = vec![format!(
        "stations : parties refaites : {} ; à jour : {} ; {} changements",
        list(&st.ran),
        list(&st.skipped),
        st.changes
    )];
    for r in &s.refuges {
        l.push(format!(
            "refuges {:<28} {}",
            r.site,
            match (r.ran, &r.error) {
                (_, Some(_)) => "ÉCHEC, fichiers précédents gardés".to_owned(),
                (true, None) => format!("refait, {} changements", r.changes),
                (false, None) => "à jour".to_owned(),
            }
        ));
    }
    let done = st.ran.len() + s.refuges.iter().filter(|r| r.ran).count();
    l.push(if s.ok {
        format!(
            "{} : {done} parties refaites, aucune source en échec ({})",
            if done == 0 {
                "rien à faire"
            } else {
                "rafraîchi"
            },
            s.path.display()
        )
    } else {
        format!(
            "{} échecs ({}) : {}",
            s.failures.len(),
            s.path.display(),
            s.failures.join(" | ")
        )
    });
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_list_every_broken_part() {
        let st = StationsState {
            failed_parts: vec!["fiches: overpass unreachable".into()],
            broken_sources: vec!["enj-faq: broken".into()],
            ..Default::default()
        };
        let refuges = vec![
            RefugesState {
                site: "cervieres".into(),
                ran: true,
                changes: 2,
                error: None,
            },
            RefugesState {
                site: "chapelle-des-bois".into(),
                ran: true,
                changes: 0,
                error: Some("HTTP 502".into()),
            },
        ];
        let f = failures(&st, &refuges);
        assert_eq!(
            f,
            [
                "stations, partie fiches: overpass unreachable",
                "stations, source enj-faq: broken",
                "refuges chapelle-des-bois : HTTP 502"
            ]
        );
        assert!(failures(&StationsState::default(), &refuges[..1]).is_empty());
    }

    #[test]
    fn changed_sites_follow_the_changes() {
        let r = |site: &str, ran, changes| RefugesState {
            site: site.into(),
            ran,
            changes,
            error: None,
        };
        let mut s = Summary {
            checked_at: "2026-11-15T14:00:00Z".into(),
            ok: true,
            personal_sources: false,
            stations: StationsState::default(),
            refuges: vec![r("a", true, 3), r("b", true, 0), r("c", false, 0)],
            failures: vec![],
            path: PathBuf::new(),
        };
        assert_eq!(changed_sites(&s), ["a"]);
        s.stations.changes = 1;
        assert_eq!(changed_sites(&s), ["a", "b", "c"]);
    }

    #[test]
    fn default_sites_are_the_built_ones() {
        let d = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(d.path());
        let sites: Vec<&'static Site> = skimap_common::site::SITES.iter().collect();
        assert!(default_sites(&layout, &sites).is_empty());
        let dir = layout.layer_dir(sites[1], skimap_refuges::LAYER).unwrap();
        DataLayout::ensure(&dir).unwrap();
        std::fs::write(dir.join("refuges.meta.json"), "{}").unwrap();
        let picked = default_sites(&layout, &sites);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, sites[1].id);
    }

    #[test]
    fn describe_says_nothing_to_do() {
        let s = Summary {
            checked_at: "2026-11-15T14:00:00Z".into(),
            ok: true,
            personal_sources: false,
            stations: StationsState {
                skipped: vec!["fiches".into()],
                ..Default::default()
            },
            refuges: vec![RefugesState {
                site: "cervieres".into(),
                ran: false,
                changes: 0,
                error: None,
            }],
            failures: Vec::new(),
            path: PathBuf::from("data/out/france/refresh/status.json"),
        };
        let l = describe(&s);
        assert!(
            l.last().unwrap().starts_with("rien à faire : 0 parties"),
            "{l:?}"
        );
        assert!(l[1].ends_with("à jour"), "{l:?}");
    }
}
