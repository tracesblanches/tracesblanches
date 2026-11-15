//! Names of the built layers of both sites with their provenance (`data/out/<site>/`).
//! Ignored by default: needs `data/`.
//!
//! ```sh
//! cargo test -p skimap-voice --test real_layers -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;
use skimap_voice::names::name_clip_id;
use skimap_voice::sources::{SiteNames, personal, site_names};

fn layout() -> DataLayout {
    DataLayout::new(
        std::env::var_os("SKIMAP_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")),
    )
}

/// Biodiv'sports names are personal, INPN ones public; names given without provenance (as
/// `skimap voice` gives them to `build`) are classified as `site_names` does.
#[test]
#[ignore = "needs data/out/<site>"]
fn provenance_of_the_names_of_both_sites() {
    let l = layout();
    for site in SITES {
        let s = site_names(&l, site).unwrap();
        println!(
            "{}: public {} pistes, {} places, {} dates; personal {:?} {:?} {:?}",
            site.id,
            s.public.pistes.len(),
            s.public.places.len(),
            s.public.dates.len(),
            s.personal.pistes,
            s.personal.places,
            s.personal.dates
        );
        assert!(!s.personal.places.is_empty());
        assert!(s.public_sources.iter().all(|x| !personal(x)));
        assert!(s.personal_sources.iter().all(|x| x.personal_use_only));
        assert!(
            s.personal_sources
                .iter()
                .any(|x| x.name.starts_with("Biodiv'sports"))
        );
        let public: Vec<String> = s.public.places.iter().map(|p| name_clip_id(p)).collect();
        for p in &s.personal.places {
            assert!(!public.contains(&name_clip_id(p)), "{p}");
        }
        // Without provenance, as given by a caller: the same split.
        let sources: Vec<_> = s
            .public_sources
            .iter()
            .chain(&s.personal_sources)
            .cloned()
            .collect();
        let c = SiteNames::classify(&s.all(), &sources, &s);
        assert_eq!(c.public, s.public);
        assert_eq!(c.personal, s.personal);
    }
}
