//! `skimap assist`: the assistant layer of a site (A1 to A9), `data/out/<site>/assist/`.

use std::path::PathBuf;

use anyhow::{Result, bail};
use chrono::NaiveDate;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

/// Options of `skimap assist`.
#[derive(Debug, Clone, Default)]
pub struct AssistArgs {
    /// First day (default: today, legal time).
    pub date: Option<NaiveDate>,
    /// Days after the first one.
    pub days: u32,
    /// Home point, for the distances of the briefing.
    pub home: Option<LonLat>,
    /// Planned route (GeoJSON with a LineString).
    pub route: Option<PathBuf>,
    /// The three suggestion requests of the site ([`skimap_assist::presets`]).
    pub suggest: bool,
    /// Outings on the core proof routes.
    pub outings: bool,
    /// Past days archived by `skimap history`: `past`, or `with-forecast`.
    pub history: Option<String>,
    /// `personal_sources` of the run: off (the default), the assistant reads only the
    /// layers whose sources may go into a published pack, so that it enters the public
    /// pack (#151); on, every layer.
    pub personal_sources: bool,
}

/// Parses `lon,lat`.
pub fn parse_lon_lat(s: &str) -> Result<LonLat> {
    let (lon, lat) = s
        .split_once(',')
        .ok_or_else(|| anyhow::anyhow!("`{s}` : attendu lon,lat"))?;
    Ok(LonLat::new(lon.trim().parse()?, lat.trim().parse()?))
}

/// Runs the assistant; returns the output and the lines to print (texts of the day).
pub fn assist(
    layout: &DataLayout,
    site: &Site,
    a: &AssistArgs,
) -> Result<(skimap_assist::Output, Vec<String>)> {
    let date = a
        .date
        .unwrap_or_else(|| skimap_core::ephemeris::local_time(chrono::Utc::now()).date());
    let mut opts = skimap_assist::Options {
        date: Some(date),
        extra_days: a.days,
        home: a.home,
        ..Default::default()
    };
    let mut lines = Vec::new();
    match a.history.as_deref() {
        None => {}
        Some(h @ ("past" | "with-forecast")) => {
            opts.paths = skimap_assist::presets::history_paths(layout, site, h == "with-forecast")?;
        }
        Some(other) => bail!("--history {other} : past ou with-forecast"),
    }
    opts.paths.route = a.route.clone();
    opts.paths.publishable_only = !a.personal_sources;
    if a.suggest {
        opts.requests = skimap_assist::presets::suggestion_requests(site, date);
    }
    if a.outings {
        let (outings, skipped) = skimap_assist::presets::proof_outings(layout, site, date)?;
        opts.outings = outings;
        lines.extend(skipped.into_iter().map(|s| format!("sortie ignorée : {s}")));
    }
    let out = skimap_assist::build(site, layout, &opts)?;
    for d in &out.briefing.days {
        lines.push(format!("--- {}", d.date));
        lines.extend(d.text.iter().cloned());
    }
    for w in &out.wax {
        lines.push(format!("Fartage {} {} : {}", w.date, w.point, w.text));
    }
    for s in &out.synthesis {
        lines.push(format!("Synthèse {} : {}", s.date, s.text()));
    }
    for s in &out.suggestions {
        lines.extend(s.text.iter().cloned());
    }
    for b in &out.best_times {
        lines.push(format!(
            "Départ {} {} : {}",
            b.request.date, b.point, b.text
        ));
    }
    for e in &out.equipment {
        lines.push(format!("Équipement : {}", e.text));
    }
    for f in &out.files {
        lines.push(format!("écrit {}", f.display()));
    }
    Ok((out, lines))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::disallowed_methods,
        reason = "the test writes its inputs directly"
    )]

    use std::path::Path;

    use super::*;
    use skimap_common::meta::LayerMeta;

    /// Copies a directory tree (the reference data set into a working copy).
    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap_or_else(|e| panic!("{}: {e}", from.display())) {
            let e = e.unwrap();
            let dest = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_tree(&e.path(), &dest);
            } else {
                std::fs::copy(e.path(), &dest).unwrap();
            }
        }
    }

    fn assist_meta(layout: &DataLayout, site: &Site) -> LayerMeta {
        LayerMeta::read(
            &layout
                .out_dir(site)
                .join("assist")
                .join(LayerMeta::file_name("assist")),
        )
        .unwrap()
    }

    /// #151: a public build (`personal_sources` off) whose zones layer still cites a
    /// source without a known reuse licence (metadata written before the licence review,
    /// or a source still to confirm) must give the assistant made from the publishable
    /// layers only: its metadata then lets it into the published pack, and the zones
    /// stay out of it.
    #[test]
    fn a_personal_input_leaves_the_public_assistant_publishable() {
        let tmp = tempfile::tempdir().unwrap();
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/data"),
            tmp.path(),
        );
        let layout = DataLayout::new(tmp.path());
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let zones_meta = layout.out_dir(site).join("zones").join("zones.meta.json");
        let mut zones = LayerMeta::read(&zones_meta).unwrap();
        zones.sources[0].licence = "non vérifiée".into();
        std::fs::write(&zones_meta, serde_json::to_vec_pretty(&zones).unwrap()).unwrap();
        assert!(skimap_pack::build::layers::restricted(&zones.sources));

        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let args = AssistArgs {
            date: Some(date),
            ..Default::default()
        };
        assist(&layout, site, &args).unwrap();
        let meta = assist_meta(&layout, site);
        let personal: Vec<&str> = meta
            .sources
            .iter()
            .filter(|s| skimap_pack::build::layers::unauthorised(s))
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            personal.is_empty(),
            "public assistant made of personal sources: {personal:?}"
        );
        assert!(
            !meta.sources.iter().any(|s| s.name == zones.sources[0].name),
            "the zones layer (personal) was read"
        );

        // The layer left out is said, in each language.
        let paths = skimap_assist::inputs::Paths {
            publishable_only: true,
            ..Default::default()
        };
        let data = skimap_assist::inputs::SiteData::load(&layout, site, &paths).unwrap();
        assert!(data.zones.is_none());
        use skimap_assist::text::Lang;
        assert!(
            data.missing_in(Lang::Fr).contains(
                &"zones et règles : source à usage personnel seulement, absente du pack public"
                    .to_owned()
            ),
            "{:?}",
            data.missing
        );
        assert!(
            data.missing_in(Lang::En).contains(
                &"zones and rules: personal-use-only source, not in the public pack".to_owned()
            ),
            "{:?}",
            data.missing_in(Lang::En)
        );

        // With `personal_sources`, every layer is read, the zones included.
        let args = AssistArgs {
            date: Some(date),
            personal_sources: true,
            ..Default::default()
        };
        assist(&layout, site, &args).unwrap();
        let meta = assist_meta(&layout, site);
        assert!(skimap_pack::build::layers::restricted(&meta.sources));
        assert!(meta.sources.iter().any(|s| s.name == zones.sources[0].name));
    }
}
