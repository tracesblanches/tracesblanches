//! `skimap voice`: voice layer of a site (VO3) for the names of its built layers. The
//! names and their provenance (publishable or personal use only: nordic pistes, huts and
//! shelters, protected zones and the end dates of their seasonal periods, names read on
//! the official piste plan) come from `skimap_voice::site_names`; this module keeps the
//! reference test of the name clips.

#[cfg(test)]
mod tests {
    /// Q2 (#102): voice clips of the names of both sites from the reference layers
    /// (`fixtures/`, without Biodiv'sports): every name clip with its spoken text, and
    /// the number of clips by kind, for the publishable names
    /// (`skimap_voice::site_names(..).public`, what a published pack says). Exact: fixed
    /// layers, deterministic text rules. Update with
    /// `SKIMAP_BLESS=1 cargo test -p skimap-cli voice` and review the diff.
    #[test]
    fn name_clips_of_the_fixtures_golden() {
        use crate::test_fixtures as fixtures;
        use skimap_common::layout::DataLayout;
        use skimap_voice::manifest::{ClipKind, manifest};
        let layout = DataLayout::new(fixtures::data());
        for site in skimap_common::site::SITES {
            let split = skimap_voice::site_names(&layout, site).unwrap();
            assert!(split.public_sources.iter().all(|s| !s.personal_use_only));
            let names = &split.public;
            let m = manifest(names);
            let mut k = fixtures::golden::Golden::new(&format!("voice/{}", site.id));
            k.count("pistes", names.pistes.len())
                .count("places", names.places.len())
                .count("dates", names.dates.len());
            for kind in [ClipKind::Fixed, ClipKind::Value, ClipKind::Name] {
                k.count(&format!("clips.{kind:?}"), m.count(kind));
            }
            for (id, (kind, text)) in &m.clips {
                if *kind == ClipKind::Name {
                    k.text(id, text.as_str());
                }
            }
            k.check();
        }
    }
}
