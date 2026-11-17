//! National rules and the status of local rules, for every site (G4, #120).
//!
//! National rules ([`Rule::is_national`]) select zones by kind or the whole country: the
//! codes (R111-32 and R111-33 of the town-planning code, L131-1 and R163-11 of the forest
//! code, Swiss federal ordinances). They apply automatically wherever the layer has the
//! zone kinds, i.e. on every site the pipeline builds.
//!
//! The other protected areas of the national inventories (APPB, réserves naturelles,
//! cœurs de parc national, réserves biologiques, captages) are each governed by their own
//! act, and territories (communes, départements, cantons) by their own decrees. A zone
//! with such an act gets a [`LocalStatus`]: [`LocalStatus::Read`] when a rule names it by
//! identifier (its act was read, see the rule's sources), [`LocalStatus::Partial`] for a
//! territory some of whose acts were read (never exhaustive), [`LocalStatus::Unverified`]
//! otherwise: « règles locales non vérifiées ». Municipal and prefectoral decrees of the
//! communes the layer does not hold are never listed: [`SiteLocalRules::municipal`] says
//! so for every French site.

use serde::Serialize;

use crate::layer::{Zone, ZoneKind, ZoneLayer};
use crate::rules::{Rule, RuleSet};

/// The act that governs a zone besides the national rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnAct {
    /// Arrêté préfectoral de protection de biotope (code de l'environnement, R411-15).
    ArretePrefectoralBiotope,
    /// Décret de classement de la réserve naturelle nationale (L332-2, L332-3).
    DecretReserveNationale,
    /// Délibération du conseil régional classant la réserve (L332-2-1).
    DeliberationReserveRegionale,
    /// Réglementation du cœur du parc national (décret de création, L331-4-1).
    ReglementationCoeurParc,
    /// Arrêté de création de la réserve biologique (code forestier, L212-2-1).
    ArreteReserveBiologique,
    /// Arrêté de déclaration d'utilité publique du captage (code de la santé publique,
    /// L1321-2), servitude AS1.
    ArreteCaptage,
    /// Arrêtés municipaux (police du maire).
    ArretesMunicipaux,
    /// Arrêtés préfectoraux du département.
    ArretesPrefectoraux,
    /// Cantonal law and decrees (Switzerland).
    DroitCantonal,
}

impl OwnAct {
    /// Value written in the layer and the metadata.
    pub fn as_str(self) -> &'static str {
        match self {
            OwnAct::ArretePrefectoralBiotope => "arrete_prefectoral_biotope",
            OwnAct::DecretReserveNationale => "decret_reserve_nationale",
            OwnAct::DeliberationReserveRegionale => "deliberation_reserve_regionale",
            OwnAct::ReglementationCoeurParc => "reglementation_coeur_parc",
            OwnAct::ArreteReserveBiologique => "arrete_reserve_biologique",
            OwnAct::ArreteCaptage => "arrete_captage",
            OwnAct::ArretesMunicipaux => "arretes_municipaux",
            OwnAct::ArretesPrefectoraux => "arretes_prefectoraux",
            OwnAct::DroitCantonal => "droit_cantonal",
        }
    }

    /// Whether the act is a body of decrees of a territory (never read exhaustively).
    pub fn is_territorial(self) -> bool {
        matches!(
            self,
            OwnAct::ArretesMunicipaux | OwnAct::ArretesPrefectoraux | OwnAct::DroitCantonal
        )
    }
}

/// Whether a parc national zone of INPN is its aire d'adhésion (INPN names it
/// « <parc> [Aire D'Adhésion] », the cœur has the bare name): the aire d'adhésion has a
/// charter, not a regulation.
fn is_aire_adhesion(z: &Zone) -> bool {
    z.props.name.to_lowercase().contains("adhésion")
}

/// The act governing `z` besides the national rules, `None` when the zone has none
/// (Natura 2000, parcs naturels régionaux, sites, monuments, Biodiv'sports, districts
/// francs fédéraux, countries: their rules are national).
pub fn own_act(z: &Zone) -> Option<OwnAct> {
    Some(match z.kind() {
        ZoneKind::Appb => OwnAct::ArretePrefectoralBiotope,
        ZoneKind::ReserveNaturelleNationale => OwnAct::DecretReserveNationale,
        ZoneKind::ReserveNaturelleRegionale => OwnAct::DeliberationReserveRegionale,
        ZoneKind::ParcNational if !is_aire_adhesion(z) => OwnAct::ReglementationCoeurParc,
        ZoneKind::ReserveBiologique => OwnAct::ArreteReserveBiologique,
        ZoneKind::CaptageProtection => OwnAct::ArreteCaptage,
        ZoneKind::Commune => OwnAct::ArretesMunicipaux,
        ZoneKind::Department => OwnAct::ArretesPrefectoraux,
        ZoneKind::Canton => OwnAct::DroitCantonal,
        _ => return None,
    })
}

/// How far the local rules of a zone were read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalStatus {
    /// Not read: « règles locales non vérifiées ».
    Unverified,
    /// A territory some of whose acts were read (rules naming it); the others were not
    /// listed.
    Partial,
    /// The act of the zone was read: its rules name the zone (their sources say whether
    /// the signed text or a draft was read).
    Read,
}

impl LocalStatus {
    /// Value written in the layer and the metadata.
    pub fn as_str(self) -> &'static str {
        match self {
            LocalStatus::Unverified => "unverified",
            LocalStatus::Partial => "partial",
            LocalStatus::Read => "read",
        }
    }
}

impl Rule {
    /// A national rule selects zones only by kind or a whole country (`country:*`): it
    /// applies on every site where the layer has those zones.
    pub fn is_national(&self) -> bool {
        self.applies_to.iter().all(|s| {
            s.kind.is_some() || s.zone.as_deref().is_some_and(|z| z.starts_with("country:"))
        })
    }

    /// Whether the rule names the zone by identifier.
    pub fn names_zone(&self, id: &str) -> bool {
        self.applies_to
            .iter()
            .any(|s| s.zone.as_deref() == Some(id))
    }
}

/// Status of the local rules of `z`, `None` when it has no act of its own.
pub fn local_status(rules: &RuleSet, z: &Zone) -> Option<(OwnAct, LocalStatus)> {
    let act = own_act(z)?;
    let named = rules.rules.iter().any(|r| r.names_zone(z.id()));
    let status = match (named, act.is_territorial()) {
        (false, _) => LocalStatus::Unverified,
        (true, true) => LocalStatus::Partial,
        (true, false) => LocalStatus::Read,
    };
    Some((act, status))
}

/// A zone of the site with an act of its own.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZoneLocal {
    /// Zone identifier.
    pub id: String,
    /// Zone name.
    pub name: String,
    /// Zone kind.
    pub kind: ZoneKind,
    /// Its act.
    pub act: OwnAct,
    /// How far it was read.
    pub status: LocalStatus,
    /// Rules naming the zone.
    pub rules: Vec<String>,
    /// Page of the zone (INPN fiche with its acts), when known.
    pub url: Option<String>,
}

/// National rules and status of the local rules of a site.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteLocalRules {
    /// National rules applying to at least one zone of the site.
    pub national: Vec<String>,
    /// Zones with an act of their own.
    pub zones: Vec<ZoneLocal>,
    /// Municipal and prefectoral decrees of the communes the layer does not hold were not
    /// listed (`Unverified`, any French site); `None` outside France.
    pub municipal: Option<LocalStatus>,
    /// Overall: `Read` only when every act was read and no decree is left unlisted.
    pub status: LocalStatus,
}

impl SiteLocalRules {
    /// Number of zones with each status.
    pub fn count(&self, s: LocalStatus) -> usize {
        self.zones.iter().filter(|z| z.status == s).count()
    }
}

/// National rules and local status of a site's layer.
pub fn site_local_rules(layer: &ZoneLayer, rules: &RuleSet) -> SiteLocalRules {
    let national = rules
        .rules
        .iter()
        .filter(|r| r.is_national() && r.matching_zones(layer).next().is_some())
        .map(|r| r.id.clone())
        .collect();
    let zones: Vec<ZoneLocal> = layer
        .zones
        .iter()
        .filter_map(|z| {
            let (act, status) = local_status(rules, z)?;
            Some(ZoneLocal {
                id: z.id().to_owned(),
                name: z.props.name.clone(),
                kind: z.kind(),
                act,
                status,
                rules: rules
                    .rules
                    .iter()
                    .filter(|r| r.names_zone(z.id()))
                    .map(|r| r.id.clone())
                    .collect(),
                url: z.props.url.clone(),
            })
        })
        .collect();
    let in_france = layer.zones.iter().any(|z| z.kind() == ZoneKind::Country);
    let municipal = in_france.then_some(LocalStatus::Unverified);
    let status = zones
        .iter()
        .map(|z| z.status)
        .chain(municipal)
        .min()
        .unwrap_or(LocalStatus::Read);
    SiteLocalRules {
        national,
        zones,
        municipal,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::bbox_polygon;
    use crate::layer::ZoneProps;
    use geo::MultiPolygon;
    use skimap_common::geom::BBox;

    fn zone(id: &str, kind: ZoneKind, name: &str) -> Zone {
        Zone::new(
            ZoneProps {
                id: id.into(),
                kind,
                name: name.into(),
                source: "test".into(),
                url: None,
                months: None,
                practices: vec![],
                designated: None,
                area_ha: 1.0,
                clipped: false,
                note: None,
            },
            MultiPolygon::new(vec![bbox_polygon(BBox::new(45.0, 6.0, 45.1, 6.1), 1)]),
        )
    }

    #[test]
    fn national_rules_are_the_codes() {
        let rs = RuleSet::embedded().unwrap();
        let national: Vec<&str> = rs
            .rules
            .iter()
            .filter(|r| r.is_national())
            .map(|r| r.id.as_str())
            .collect();
        for id in [
            "fr-r111-32-bivouac-consent",
            "fr-r111-33-bivouac",
            "fr-l131-1-fire",
            "fr-r163-11-foret-protection-camping",
            "ch-odf-camping",
        ] {
            assert!(national.contains(&id), "{id} should be national");
        }
        for id in ["appb-haut-jura-bivouac", "ch-vd-lccr-27-camping"] {
            assert!(!national.contains(&id), "{id} is local");
        }
    }

    #[test]
    fn statuses_of_the_zone_kinds() {
        let rs = RuleSet::embedded().unwrap();
        let st = |z: &Zone| local_status(&rs, z).map(|(_, s)| s);
        // An APPB whose act was read, another one that was not.
        assert_eq!(
            st(&zone("inpn:FR3800988", ZoneKind::Appb, "Haut-Jura")),
            Some(LocalStatus::Read)
        );
        assert_eq!(
            st(&zone("inpn:FR0000000", ZoneKind::Appb, "x")),
            Some(LocalStatus::Unverified)
        );
        // Parc national: the cœur has a regulation, the aire d'adhésion does not.
        let coeur = zone("inpn:FR3300001", ZoneKind::ParcNational, "Vanoise");
        assert_eq!(
            local_status(&rs, &coeur),
            Some((OwnAct::ReglementationCoeurParc, LocalStatus::Unverified))
        );
        let aa = zone(
            "inpn:FR3400005",
            ZoneKind::ParcNational,
            "Ecrins [Aire D'Adhésion]",
        );
        assert_eq!(local_status(&rs, &aa), None);
        // Territories: partial when some of their acts were read, never « read ».
        assert_eq!(
            st(&zone("commune:05027", ZoneKind::Commune, "Cervières")),
            Some(LocalStatus::Partial)
        );
        assert_eq!(
            st(&zone("dept:73", ZoneKind::Department, "Savoie")),
            Some(LocalStatus::Unverified)
        );
        // National categories without act of their own.
        for k in [
            ZoneKind::Natura2000Sic,
            ZoneKind::SiteClasse,
            ZoneKind::Country,
            ZoneKind::ParcNaturelRegional,
        ] {
            assert_eq!(st(&zone("x", k, "x")), None, "{k:?}");
        }
    }

    #[test]
    fn site_summary() {
        let rs = RuleSet::embedded().unwrap();
        let layer = ZoneLayer {
            zones: vec![
                zone("country:fr", ZoneKind::Country, "France"),
                zone("dept:73", ZoneKind::Department, "Savoie"),
                zone("inpn:FR3300001", ZoneKind::ParcNational, "Vanoise"),
                zone("sc", ZoneKind::SiteClasse, "site"),
            ],
        };
        let s = site_local_rules(&layer, &rs);
        assert!(
            s.national
                .contains(&"fr-r111-32-bivouac-consent".to_owned())
        );
        assert!(s.national.contains(&"fr-r111-33-bivouac".to_owned()));
        assert!(!s.national.contains(&"ch-odf-camping".to_owned()));
        assert_eq!(s.zones.len(), 2);
        assert_eq!(s.count(LocalStatus::Unverified), 2);
        assert_eq!(s.municipal, Some(LocalStatus::Unverified));
        assert_eq!(s.status, LocalStatus::Unverified);
    }
}
