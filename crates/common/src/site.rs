//! The two evaluation sites of the v1 proof of concept.
//!
//! Bounding boxes are WGS84 and extend beyond France (Italy near Cervières, Switzerland
//! near Chapelle-des-Bois): crates that need French-only data clip with commune or
//! department outlines.
//!
//! Météo-France massif codes are those of the snowpack simulation and of the BERA,
//! verified by P6 (#19) on the snowtools massif polygons (`UMR-CNRM/snowtools`,
//! `massifs.shp`) against the commune outlines (geo.api.gouv.fr):
//! - Cervières: Queyras (17) covers 100 % of the commune.
//! - Chapelle-des-Bois: Haut-Doubs (55) covers 93.2 % of the commune and the village;
//!   Haut-Jura (56) covers the Risoux side (6.0 %, south-west edge). The Jura has no BERA.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::geom::{BBox, LonLat};

/// A Météo-France massif (snowpack simulation, BERA).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Massif {
    /// Météo-France massif number.
    pub code: u16,
    /// Massif name.
    pub name: &'static str,
}

/// How sure we are of the massif attachment of a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MassifStatus {
    /// Verified on the snowtools massif polygons (UMR-CNRM/snowtools `massifs.shp`) and
    /// the commune outline; the first massif covers most of the commune.
    Verified,
    /// Single massif, assumed from geography, not verified against Météo-France.
    Assumed,
    /// Several candidates, in order of preference; to be confirmed.
    ToConfirm,
}

/// An evaluation site.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Site {
    /// Stable identifier used in paths and on the command line.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// French department code.
    pub department: &'static str,
    /// INSEE code of the main commune (geo.api.gouv.fr).
    pub insee: &'static str,
    /// Centre of the main commune (geo.api.gouv.fr), WGS84.
    pub commune_center: LonLat,
    /// Working area, WGS84.
    pub bbox: BBox,
    /// Météo-France massifs covering the commune, the main one first.
    pub massifs: &'static [Massif],
    /// Confidence in [`Site::massifs`].
    pub massif_status: MassifStatus,
    /// Whether a BERA (avalanche bulletin) exists for the massif.
    pub has_bera: bool,
}

impl Site {
    /// Preferred massif.
    pub fn primary_massif(&self) -> Massif {
        self.massifs[0]
    }
}

/// Cervières (Hautes-Alpes).
pub const CERVIERES: Site = Site {
    id: "cervieres",
    name: "Cervières",
    department: "05",
    insee: "05027",
    commune_center: LonLat::new(6.7746, 44.8586),
    bbox: BBox::new(44.77, 6.66, 44.92, 6.86),
    massifs: &[Massif {
        code: 17,
        name: "Queyras",
    }],
    massif_status: MassifStatus::Verified,
    has_bera: true,
};

/// Chapelle-des-Bois (Doubs).
pub const CHAPELLE_DES_BOIS: Site = Site {
    id: "chapelle-des-bois",
    name: "Chapelle-des-Bois",
    department: "25",
    insee: "25121",
    commune_center: LonLat::new(6.1118, 46.5952),
    bbox: BBox::new(46.54, 6.02, 46.68, 6.20),
    massifs: &[
        Massif {
            code: 55,
            name: "Haut-Doubs",
        },
        Massif {
            code: 56,
            name: "Haut-Jura",
        },
    ],
    massif_status: MassifStatus::Verified,
    has_bera: false,
};

/// All sites.
pub const SITES: &[Site] = &[CERVIERES, CHAPELLE_DES_BOIS];

/// Site by identifier.
pub fn site(id: &str) -> Result<&'static Site> {
    SITES
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| Error::UnknownSite(id.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup() {
        assert_eq!(site("cervieres").unwrap().insee, "05027");
        assert_eq!(site("chapelle-des-bois").unwrap().primary_massif().code, 55);
        assert!(matches!(site("mouthe"), Err(Error::UnknownSite(_))));
    }

    #[test]
    fn bboxes_are_consistent() {
        for s in SITES {
            assert!(
                s.bbox.south < s.bbox.north && s.bbox.west < s.bbox.east,
                "{}",
                s.id
            );
            assert!(s.bbox.contains(s.commune_center), "{}", s.id);
            assert!(!s.massifs.is_empty());
            assert_eq!(s.massif_status, MassifStatus::Verified, "{}", s.id);
        }
        let codes: Vec<_> = CHAPELLE_DES_BOIS.massifs.iter().map(|m| m.code).collect();
        assert_eq!(codes, [55, 56], "Haut-Doubs first (93.2 % of the commune)");
        let ids: Vec<_> = SITES.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["cervieres", "chapelle-des-bois"]);
    }
}
