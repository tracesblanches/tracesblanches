//! References of research report 07 per site (`data/report07.json`): pre-filter of the
//! exposure and lengths of the nordic network inside the CLPA footprints. A site the
//! report did not measure has none.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;
use skimap_common::geom::BBox;
use skimap_common::site::Site;

/// Pre-filter figures of report 07: box and shares reached at α = 18° and 30°.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Prefilter {
    /// Box of the pre-filter, WGS84.
    pub bbox: BBox,
    /// Share reached at α = 18°.
    pub alpha18: f64,
    /// Share reached at α = 30°.
    pub alpha30: f64,
}

/// References of one site.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Report07Site {
    /// Pre-filter of the exposure.
    #[serde(default)]
    pub prefilter: Option<Prefilter>,
    /// Network length and length inside the CLPA footprints (section B4), km: `total`,
    /// `inside`, then per piste name.
    #[serde(default)]
    pub clpa_km: Vec<(String, f64)>,
}

#[derive(Deserialize)]
struct Table {
    sites: BTreeMap<String, Report07Site>,
}

static TABLE: LazyLock<BTreeMap<String, Report07Site>> = LazyLock::new(|| {
    serde_json::from_str::<Table>(include_str!("../data/report07.json"))
        .expect("data/report07.json")
        .sites
});

/// References of report 07 for a site, if the report measured it.
pub fn for_site(site: &Site) -> Option<&'static Report07Site> {
    TABLE.get(site.id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn table_parses_and_is_consistent() {
        assert!(!super::TABLE.is_empty());
        for (id, r) in super::TABLE.iter() {
            if let Some(p) = r.prefilter {
                assert!(
                    p.bbox.south < p.bbox.north && p.bbox.west < p.bbox.east,
                    "{id}"
                );
                assert!(
                    (0.0..=1.0).contains(&p.alpha18) && p.alpha30 <= p.alpha18,
                    "{id}"
                );
            }
            if !r.clpa_km.is_empty() {
                assert_eq!(r.clpa_km[0].0, "total", "{id}");
                assert_eq!(r.clpa_km[1].0, "inside", "{id}");
            }
        }
    }
}
