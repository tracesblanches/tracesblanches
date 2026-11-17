//! Licence families of the data and where a piece of data may go (#169).
//!
//! One decision for every layer that mixes sources (docs/licences/l3-combinaison.md):
//!
//! - **ODbL** (OpenStreetMap and its derivatives) and the **attribution-only** licences
//!   (Licence Ouverte, CC BY, CC0, ODC-BY) may share a file: an attribution-only piece of
//!   data enters an ODbL database;
//! - **CC BY-SA** (refuges.info, Camptocamp) is share-alike and not compatible with the
//!   ODbL (and the reverse): its data goes into files of its own, with its licence and
//!   its attribution, never into an ODbL file;
//! - anything else (no licence shown, a licence still to confirm, a source marked
//!   `personal_use_only`) is **personal**: never in a published pack, only with
//!   `personal_sources`.
//!
//! [`classify`] reads the licence text a source publishes (`SourceRef::licence`, the
//! `licence` property of an itinerary) by its beginning: a text that starts with
//! « aucune licence » and later names a licence to confirm stays personal.

use crate::meta::SourceRef;

/// Licence family of a piece of data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// Open Database License (OpenStreetMap and its derivatives).
    Odbl,
    /// Attribution only: Licence Ouverte (Etalab), CC BY, CC0, ODC-BY, opendata.swiss
    /// « terms_by », SkiRadar; the code licences of a ported method (MIT, EUPL) and of
    /// the snowtools contours (CeCILL-C); links and facts cited (nothing reproduced).
    Attribution,
    /// Creative Commons share-alike (CC BY-SA, any version).
    ShareAlike,
    /// No known reuse licence, or personal use only.
    Personal,
}

impl Family {
    /// Whether data of this family may enter an ODbL database (the routing graph, the
    /// OSM-derived layers of a published pack).
    pub fn enters_odbl(self) -> bool {
        matches!(self, Self::Odbl | Self::Attribution)
    }

    /// Whether data of this family may go into a published pack (in a file of its group,
    /// [`Group`]).
    pub fn public(self) -> bool {
        self != Self::Personal
    }

    /// Whether data of this family may go into a file of `group`.
    pub fn fits(self, group: Group) -> bool {
        match group {
            Group::Odbl => self.enters_odbl(),
            Group::ShareAlike => matches!(self, Self::ShareAlike | Self::Attribution),
            Group::Personal => true,
        }
    }
}

/// Licence family of a licence text as a source publishes it.
pub fn classify(licence: &str) -> Family {
    let l = licence.trim().to_lowercase();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| l.starts_with(p));
    if starts(&["cc by-sa", "cc-by-sa", "cc by sa", "creative commons by-sa"]) {
        Family::ShareAlike
    } else if starts(&["odbl", "open database license"]) {
        Family::Odbl
    } else if starts(&[
        "licence ouverte",
        "etalab",
        "cc0",
        "odc-by",
        "cc by 2",
        "cc by 3",
        "cc by 4",
        "cc-by-2",
        "cc-by-3",
        "cc-by-4",
        // opendata.swiss « terms_by » : « Utilisation libre. Obligation d'indiquer la
        // source » (geo.admin.ch, swisstopo OGD).
        "opendata.swiss « terms_by »",
        "terms_by",
        // SkiRadar : « Données factuelles réutilisables avec attribution ».
        "données factuelles réutilisables avec attribution",
        "réutilisation avec attribution",
        // Code of a method ported (MIT, EUPL) and the snowtools massif contours
        // (CeCILL-C, whose copyleft covers the component itself, art. 5.3.3: only a
        // raster derived from them is delivered, docs/licences/f2-licence-code.md).
        "mit ",
        "eupl",
        "cecill-c",
        // Nothing reproduced: links, or facts cited with their source.
        "liens seulement",
        "plages de température citées (faits)",
    ]) || l == "mit"
    {
        Family::Attribution
    } else {
        Family::Personal
    }
}

/// Licence family of a source: [`Family::Personal`] when it is marked
/// `personal_use_only`, else [`classify`] of its licence.
pub fn of_source(s: &SourceRef) -> Family {
    if s.personal_use_only {
        Family::Personal
    } else {
        classify(&s.licence)
    }
}

/// Group of files of a pack a piece of data goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// ODbL file (ODbL and attribution-only data): published.
    Odbl,
    /// CC BY-SA file (share-alike and attribution-only data): published, apart, with its
    /// licence and attribution.
    ShareAlike,
    /// Personal file: only with `personal_sources`, never published.
    Personal,
}

impl Group {
    /// Suffix of the pack layer of the group (`srn-routes`, `srn-routes-cc-by-sa`,
    /// `srn-routes-personnel`).
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Odbl => "",
            Self::ShareAlike => "-cc-by-sa",
            Self::Personal => "-personnel",
        }
    }

    /// Licence of a file of the group, French.
    pub fn licence_fr(self) -> &'static str {
        match self {
            Self::Odbl => "ODbL 1.0 (OpenStreetMap et données sous licence d'attribution seule)",
            Self::ShareAlike => {
                "CC BY-SA (partage à l'identique, à part des données ODbL ; attribution de chaque source)"
            }
            Self::Personal => {
                "usage personnel : sources sans licence de réutilisation connue ou licences incompatibles mêlées"
            }
        }
    }

    /// Whether a file of the group may hold OSM (ODbL) data: the ODbL file, and a
    /// personal one (never published); not a CC BY-SA file (#171).
    pub fn fits_osm(self) -> bool {
        Family::Odbl.fits(self)
    }

    /// Every group, in pack order.
    pub const ALL: [Group; 3] = [Group::Odbl, Group::ShareAlike, Group::Personal];
}

/// Group of a piece of data made of data of `families` (an SRN area built from the
/// itineraries of several sources): personal when one is personal or when ODbL and
/// share-alike data are mixed (no licence allows both), share-alike when one is, else
/// ODbL (nothing at all: ODbL).
pub fn group<I: IntoIterator<Item = Family>>(families: I) -> Group {
    let (mut odbl, mut sa) = (false, false);
    for f in families {
        match f {
            Family::Personal => return Group::Personal,
            Family::Odbl => odbl = true,
            Family::ShareAlike => sa = true,
            Family::Attribution => {}
        }
    }
    match (odbl, sa) {
        (true, true) => Group::Personal,
        (false, true) => Group::ShareAlike,
        _ => Group::Odbl,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_of_the_licences_met() {
        for (l, f) in [
            ("ODbL 1.0", Family::Odbl),
            (
                "ODbL 1.0 (OSM, OpenSkiMap) ; communes : Licence Ouverte 2.0",
                Family::Odbl,
            ),
            (
                "Licence Ouverte 2.0 (Etalab), via DATAtourisme",
                Family::Attribution,
            ),
            ("Licence Ouverte Etalab 2.0", Family::Attribution),
            ("CC BY 4.0", Family::Attribution),
            ("CC-BY-4.0, avec les usages interdits", Family::Attribution),
            ("CC0", Family::Attribution),
            ("CC BY-SA 3.0", Family::ShareAlike),
            ("CC BY-SA 2.0", Family::ShareAlike),
            (
                "CC BY-SA 4.0 (refuges.info CC BY-SA 2.0)",
                Family::ShareAlike,
            ),
            (
                "aucune licence affichée par l'instance ; le PNR publie sous Licence Ouverte 2.0 : à confirmer",
                Family::Personal,
            ),
            ("non indiquée", Family::Personal),
            ("non vérifiée (F5, #11)", Family::Personal),
            ("", Family::Personal),
        ] {
            assert_eq!(classify(l), f, "{l}");
        }
    }

    /// Licence texts of the sources of the published layers (#171): read on the source's
    /// page, attribution only or ODbL; a text still to confirm stays personal.
    #[test]
    fn families_of_the_sources_checked_for_171() {
        for (l, f) in [
            (
                "opendata.swiss « terms_by » (attribution obligatoire)",
                Family::Attribution,
            ),
            (
                "Données factuelles réutilisables avec attribution « skiradar.fr » et lien vers la fiche",
                Family::Attribution,
            ),
            (
                "réutilisation avec attribution « skiradar.fr » et lien vers la fiche",
                Family::Attribution,
            ),
            ("CeCILL-C (dépôt UMR-CNRM/snowtools)", Family::Attribution),
            ("MIT", Family::Attribution),
            ("EUPL-1.2 (AvaFrame) ; Flow-Py GPL-3.0", Family::Attribution),
            (
                "liens seulement, aucune donnée reprise",
                Family::Attribution,
            ),
            (
                "plages de température citées (faits), pages consultées le 15/11/2026",
                Family::Attribution,
            ),
            (
                "ODbL 1.0 (jeu data.gouv.fr « Contours administratifs »)",
                Family::Odbl,
            ),
            ("mitigée", Family::Personal),
            (
                "« other-open » selon data.gouv.fr, à confirmer (F5, #11)",
                Family::Personal,
            ),
        ] {
            assert_eq!(classify(l), f, "{l}");
        }
    }

    #[test]
    fn a_personal_source_is_personal_whatever_its_licence() {
        let mut s = SourceRef::new("Open-Meteo", "https://x", "CC BY 4.0", "Open-Meteo");
        assert_eq!(of_source(&s), Family::Attribution);
        s.personal_use_only = true;
        assert_eq!(of_source(&s), Family::Personal);
    }

    #[test]
    fn groups() {
        use Family::*;
        assert_eq!(group([Odbl, Attribution]), Group::Odbl);
        assert_eq!(group([Attribution]), Group::Odbl);
        assert_eq!(group([]), Group::Odbl);
        assert_eq!(group([ShareAlike, Attribution]), Group::ShareAlike);
        assert_eq!(group([ShareAlike, Odbl]), Group::Personal);
        assert_eq!(group([Odbl, Personal]), Group::Personal);
        assert!(Attribution.fits(Group::Odbl) && Attribution.fits(Group::ShareAlike));
        assert!(!ShareAlike.fits(Group::Odbl) && !Odbl.fits(Group::ShareAlike));
        assert!(!Personal.fits(Group::Odbl) && Personal.fits(Group::Personal));
        assert!(Odbl.enters_odbl() && !ShareAlike.enters_odbl() && !Personal.public());
    }
}
