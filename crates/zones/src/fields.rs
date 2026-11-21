//! Simple fields of a zone derived from the rules, for maps (pack, styles): what applies
//! to bivouac and to access anywhere in the zone at some time of the year, with the
//! rules that decide it. The full evaluation at a position and an instant stays in the
//! core (`skimap_core::zone_eval`, C8); these fields only say what a map can show.
//!
//! A zone is governed by its own rules and by those of the jurisdictions it lies in
//! (country, département, canton, commune of the layer containing an interior point of
//! the zone): R111-32 of the French town-planning code (bivouac with the owner's consent)
//! applies inside a Natura 2000 site that has no rule of its own. The verdict is the
//! most restrictive applicable rule in force on the date or later, in the order of the
//! core: forbidden, forbidden in places (the rule covers an unmapped part of the zone),
//! tolerated, consent required, avoid recommended, allowed; `unknown` without any rule
//! (outside the jurisdictions of the layer, Italy).

use geo::{Area, Centroid};
use jiff::civil::Date;
use serde::Serialize;
use skimap_common::geom::LonLat;

use crate::layer::{Zone, ZoneKind, ZoneLayer};
use crate::rules::{Effect, Period, Rule, RuleSet, Topic};

/// Bivouac in a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bivouac {
    /// Forbidden in the whole zone (during [`ZoneFields::bivouac_period`]).
    Forbidden,
    /// Forbidden in parts of the zone the map does not draw (private land crossed by the
    /// pistes, near woods).
    ForbiddenInPlaces,
    /// Tolerated under conditions.
    Tolerated,
    /// With the consent of the owner (France: R111-32 everywhere else).
    ConsentRequired,
    /// Explicitly allowed.
    Allowed,
    /// No rule known.
    Unknown,
}

/// Access to a zone on skis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Entering is forbidden (during [`ZoneFields::access_period`]).
    Forbidden,
    /// Entering is forbidden in parts the map does not draw.
    ForbiddenInPlaces,
    /// Only on the marked routes: leaving them is forbidden.
    RoutesOnly,
    /// Leaving the marked routes is forbidden in parts the map does not draw (private
    /// land crossed by the pistes).
    RoutesOnlyInPlaces,
    /// Not a ban: the zone should be avoided (wildlife, Biodiv'sports).
    AvoidRecommended,
    /// Explicitly allowed.
    Allowed,
    /// No rule known.
    Unknown,
}

impl Bivouac {
    /// Value written in the layer.
    pub fn as_str(self) -> &'static str {
        match self {
            Bivouac::Forbidden => "forbidden",
            Bivouac::ForbiddenInPlaces => "forbidden_in_places",
            Bivouac::Tolerated => "tolerated",
            Bivouac::ConsentRequired => "consent_required",
            Bivouac::Allowed => "allowed",
            Bivouac::Unknown => "unknown",
        }
    }

    /// French label.
    pub fn label_fr(self) -> &'static str {
        match self {
            Bivouac::Forbidden => "bivouac interdit",
            Bivouac::ForbiddenInPlaces => "bivouac interdit par endroits",
            Bivouac::Tolerated => "bivouac toléré",
            Bivouac::ConsentRequired => "bivouac avec l'accord du propriétaire",
            Bivouac::Allowed => "bivouac autorisé",
            Bivouac::Unknown => "bivouac : règle inconnue",
        }
    }
}

impl Access {
    /// Value written in the layer.
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Forbidden => "forbidden",
            Access::ForbiddenInPlaces => "forbidden_in_places",
            Access::RoutesOnly => "routes_only",
            Access::RoutesOnlyInPlaces => "routes_only_in_places",
            Access::AvoidRecommended => "avoid_recommended",
            Access::Allowed => "allowed",
            Access::Unknown => "unknown",
        }
    }

    /// French label.
    pub fn label_fr(self) -> &'static str {
        match self {
            Access::Forbidden => "accès interdit",
            Access::ForbiddenInPlaces => "accès interdit par endroits",
            Access::RoutesOnly => "itinéraires balisés seulement",
            Access::RoutesOnlyInPlaces => "itinéraires balisés seulement par endroits",
            Access::AvoidRecommended => "à éviter (faune)",
            Access::Allowed => "accès autorisé",
            Access::Unknown => "accès : règle inconnue",
        }
    }
}

/// Every value of [`Bivouac`], for the style validator.
pub const BIVOUAC_VALUES: [Bivouac; 6] = [
    Bivouac::Forbidden,
    Bivouac::ForbiddenInPlaces,
    Bivouac::Tolerated,
    Bivouac::ConsentRequired,
    Bivouac::Allowed,
    Bivouac::Unknown,
];

/// Every value of [`Access`], for the style validator.
pub const ACCESS_VALUES: [Access; 7] = [
    Access::Forbidden,
    Access::ForbiddenInPlaces,
    Access::RoutesOnly,
    Access::RoutesOnlyInPlaces,
    Access::AvoidRecommended,
    Access::Allowed,
    Access::Unknown,
];

/// The fields of one zone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZoneFields {
    /// Bivouac.
    pub bivouac: Bivouac,
    /// When the deciding bivouac rule applies, French; `None` all year.
    pub bivouac_period: Option<String>,
    /// Deciding bivouac rules.
    pub bivouac_rules: Vec<String>,
    /// Access.
    pub access: Access,
    /// When the deciding access rule applies, French; `None` all year.
    pub access_period: Option<String>,
    /// Deciding access rules.
    pub access_rules: Vec<String>,
}

fn bivouac_of(r: &Rule) -> Option<Bivouac> {
    (r.topic == Topic::Bivouac).then_some(match r.effect {
        Effect::Forbidden if r.only_where.is_some() => Bivouac::ForbiddenInPlaces,
        Effect::Forbidden => Bivouac::Forbidden,
        Effect::Tolerated => Bivouac::Tolerated,
        Effect::ConsentRequired => Bivouac::ConsentRequired,
        // No bivouac rule says « avoid »: the softest ban is consent.
        Effect::AvoidRecommended => Bivouac::ConsentRequired,
        Effect::Allowed => Bivouac::Allowed,
    })
}

fn access_of(r: &Rule) -> Option<Access> {
    let in_places = r.only_where.is_some();
    match (r.topic, r.effect) {
        (Topic::Access, Effect::Forbidden) if in_places => Some(Access::ForbiddenInPlaces),
        (Topic::Access, Effect::Forbidden) => Some(Access::Forbidden),
        (Topic::LeaveRoutes, Effect::Forbidden) if in_places => Some(Access::RoutesOnlyInPlaces),
        (Topic::LeaveRoutes, Effect::Forbidden) => Some(Access::RoutesOnly),
        (Topic::Access | Topic::LeaveRoutes, Effect::AvoidRecommended) => {
            Some(Access::AvoidRecommended)
        }
        (Topic::Access | Topic::LeaveRoutes, Effect::Allowed) => Some(Access::Allowed),
        // Tolerated or with consent: the zone can be entered under conditions.
        (Topic::Access | Topic::LeaveRoutes, Effect::Tolerated | Effect::ConsentRequired) => {
            Some(Access::AvoidRecommended)
        }
        _ => None,
    }
}

/// Level of a jurisdiction: country 0, département or canton 1, commune 2; `None` for
/// the other zones.
fn jurisdiction_rank(k: ZoneKind) -> Option<u8> {
    match k {
        ZoneKind::Country => Some(0),
        ZoneKind::Department | ZoneKind::Canton => Some(1),
        ZoneKind::Commune => Some(2),
        _ => None,
    }
}

/// Months of a zone, French (« décembre à juin »).
fn months_fr(z: &Zone) -> String {
    const M: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    match z.props.months {
        Some(m) if !m.is_empty() => m
            .numbers()
            .iter()
            .map(|n| M[(*n as usize).saturating_sub(1) % 12])
            .collect::<Vec<_>>()
            .join(", "),
        _ => "toute l'année".to_owned(),
    }
}

fn period_of(r: &Rule, z: &Zone) -> Option<String> {
    match &r.period {
        Period::AllYear => None,
        Period::ZoneMonths => Some(format!("mois sensibles : {}", months_fr(z))),
        p => Some(p.describe_fr()),
    }
}

/// A point inside the zone: the centroid of its largest polygon when inside it, else the
/// middle of the first span of the horizontal line through that polygon's centroid.
/// (geo's `InteriorPoint` goes through `relate`, which logs warnings on the slivers some
/// sources publish.)
pub fn inner_point(z: &Zone) -> Option<LonLat> {
    let poly = z
        .geometry
        .0
        .iter()
        .max_by(|a, b| a.unsigned_area().total_cmp(&b.unsigned_area()))?;
    let c = poly.centroid()?;
    let at = LonLat::new(c.x(), c.y());
    if z.contains(at) {
        return Some(at);
    }
    let y = c.y();
    let mut xs: Vec<f64> = Vec::new();
    for ring in std::iter::once(poly.exterior()).chain(poly.interiors()) {
        for s in ring.lines() {
            let (a, b) = (s.start, s.end);
            if (a.y <= y) != (b.y <= y) {
                xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    let p = xs
        .as_chunks::<2>()
        .0
        .first()
        .map(|[a, b]| LonLat::new((a + b) / 2.0, y))?;
    z.contains(p).then_some(p)
}

/// Zones whose rules govern `z`: itself and the jurisdictions containing an interior
/// point of it ([`inner_point`]).
pub fn governing<'a>(layer: &'a ZoneLayer, z: &'a Zone) -> Vec<&'a Zone> {
    let mut v = vec![z];
    if let Some(at) = inner_point(z) {
        // A jurisdiction is governed by the larger ones only (a country is not governed
        // by its départements).
        let rank = jurisdiction_rank(z.kind());
        v.extend(layer.containing(at).filter(|j| {
            j.id() != z.id()
                && jurisdiction_rank(j.kind()).is_some_and(|r| rank.is_none_or(|zr| r < zr))
        }));
    }
    v
}

/// Fields of the zone `z` of `layer`, rules in force on `date` or later.
pub fn zone_fields(layer: &ZoneLayer, rules: &RuleSet, z: &Zone, date: Date) -> ZoneFields {
    let zones = governing(layer, z);
    let applicable: Vec<(&Rule, &Zone)> = rules
        .rules
        .iter()
        .filter(|r| r.in_force.until.is_none_or(|u| u >= date))
        .filter_map(|r| zones.iter().find(|g| r.applies_to_zone(g)).map(|g| (r, *g)))
        .collect();
    let pick = |f: &dyn Fn(&Rule) -> Option<u8>| -> Option<(u8, Vec<&(&Rule, &Zone)>)> {
        let best = applicable.iter().filter_map(|a| f(a.0)).min()?;
        Some((
            best,
            applicable.iter().filter(|a| f(a.0) == Some(best)).collect(),
        ))
    };
    let b = pick(&|r| bivouac_of(r).map(|v| v as u8));
    let a = pick(&|r| access_of(r).map(|v| v as u8));
    let bivouac = b
        .as_ref()
        .map_or(Bivouac::Unknown, |(v, _)| BIVOUAC_VALUES[usize::from(*v)]);
    let access = a
        .as_ref()
        .map_or(Access::Unknown, |(v, _)| ACCESS_VALUES[usize::from(*v)]);
    let period = |d: &Option<(u8, Vec<&(&Rule, &Zone)>)>| {
        d.as_ref().and_then(|(_, rs)| {
            // All year if any deciding rule is; else the first rule's period.
            if rs.iter().any(|(r, _)| r.period == Period::AllYear) {
                None
            } else {
                rs.first().and_then(|(r, g)| period_of(r, g))
            }
        })
    };
    let ids = |d: &Option<(u8, Vec<&(&Rule, &Zone)>)>| {
        d.as_ref()
            .map(|(_, rs)| rs.iter().map(|(r, _)| r.id.clone()).collect())
            .unwrap_or_default()
    };
    ZoneFields {
        bivouac,
        bivouac_period: period(&b),
        bivouac_rules: ids(&b),
        access,
        access_period: period(&a),
        access_rules: ids(&a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::bbox_polygon;
    use crate::layer::{Months, ZoneProps};
    use geo::MultiPolygon;
    use jiff::civil::date;
    use skimap_common::geom::BBox;

    fn zone(id: &str, kind: ZoneKind, b: BBox) -> Zone {
        Zone::new(
            ZoneProps {
                id: id.into(),
                kind,
                name: id.into(),
                source: "test".into(),
                url: None,
                months: None,
                practices: vec![],
                designated: None,
                area_ha: 1.0,
                clipped: false,
                note: None,
            },
            MultiPolygon::new(vec![bbox_polygon(b, 1)]),
        )
    }

    fn d() -> Date {
        date(2026, 11, 15)
    }

    /// The real rules (rules.json) on zones named as in the two sites.
    #[test]
    fn real_rules_give_the_expected_fields() {
        let rules = RuleSet::embedded().unwrap();
        let fr = BBox::new(44.0, 5.0, 47.5, 7.0);
        let vd = BBox::new(46.5, 6.45, 47.0, 6.9);
        let mut biodiv = zone(
            "biodiv:1",
            ZoneKind::BiodivSports,
            BBox::new(44.80, 6.70, 44.82, 6.72),
        );
        biodiv.props.practices = vec!["terrestre".into()];
        biodiv.props.months = Some(Months::from_numbers([12, 1, 2, 3]).unwrap());
        let layer = ZoneLayer {
            zones: vec![
                zone("country:fr", ZoneKind::Country, fr),
                zone(
                    "dept:25",
                    ZoneKind::Department,
                    BBox::new(46.5, 5.9, 47.4, 6.4),
                ),
                zone(
                    "dept:05",
                    ZoneKind::Department,
                    BBox::new(44.5, 6.3, 45.1, 7.0),
                ),
                zone(
                    "commune:05007",
                    ZoneKind::Commune,
                    BBox::new(44.70, 6.70, 44.78, 6.80),
                ),
                zone("canton:vd", ZoneKind::Canton, vd),
                zone(
                    "commune:ch-5872",
                    ZoneKind::Commune,
                    BBox::new(46.60, 6.50, 46.65, 6.60),
                ),
                zone(
                    "inpn:FR3800988",
                    ZoneKind::Appb,
                    BBox::new(46.60, 6.00, 46.62, 6.02),
                ),
                zone(
                    "inpn:FR3800137",
                    ZoneKind::Appb,
                    BBox::new(46.63, 6.05, 46.64, 6.06),
                ),
                zone(
                    "n2000",
                    ZoneKind::Natura2000Sic,
                    BBox::new(44.83, 6.73, 44.84, 6.74),
                ),
                zone(
                    "sc",
                    ZoneKind::SiteClasse,
                    BBox::new(44.85, 6.75, 44.86, 6.76),
                ),
                zone(
                    "ch-jbg:1",
                    ZoneKind::DistrictFranc,
                    BBox::new(46.61, 6.70, 46.62, 6.71),
                ),
                biodiv,
            ],
        };
        let f = |id: &str| zone_fields(&layer, &rules, layer.get(id).unwrap(), d());
        // France: R111-32 everywhere without a rule of its own.
        let n = f("n2000");
        assert_eq!(n.bivouac, Bivouac::ConsentRequired);
        assert_eq!(n.bivouac_rules, ["fr-r111-32-bivouac-consent"]);
        assert_eq!(n.access, Access::Unknown);
        assert_eq!(f("country:fr").bivouac, Bivouac::ConsentRequired);
        // Site classé: R111-33.
        let sc = f("sc");
        assert_eq!(sc.bivouac, Bivouac::Forbidden);
        assert!(sc.bivouac_rules.contains(&"fr-r111-33-bivouac".to_owned()));
        assert_eq!(sc.bivouac_period, None);
        // APPB Haut-Jura: bivouac forbidden and routes only from 15 December to 30 June.
        let hj = f("inpn:FR3800988");
        assert_eq!(hj.bivouac, Bivouac::Forbidden);
        assert_eq!(
            hj.bivouac_period.as_deref(),
            Some("du 15 décembre au 30 juin")
        );
        assert_eq!(hj.access, Access::RoutesOnly);
        assert_eq!(hj.access_rules, ["appb-haut-jura-leave-routes"]);
        // APPB Forêt du Paradis: bivouac forbidden all year.
        assert_eq!(f("inpn:FR3800137").bivouac, Bivouac::Forbidden);
        // Doubs: private land crossed by the pistes (no outline).
        let dept = f("dept:25");
        assert_eq!(dept.bivouac, Bivouac::ForbiddenInPlaces);
        assert_eq!(dept.access, Access::RoutesOnlyInPlaces);
        // Arvieux tolerates bivouac (more restrictive than consent in the core's order).
        assert_eq!(f("commune:05007").bivouac, Bivouac::Tolerated);
        // Vaud: consent; Le Chenit: forbidden; district franc: forbidden, routes only;
        // cantons: access allowed.
        assert_eq!(f("canton:vd").bivouac, Bivouac::ConsentRequired);
        assert_eq!(f("canton:vd").access, Access::Allowed);
        assert_eq!(f("commune:ch-5872").bivouac, Bivouac::Forbidden);
        let df = f("ch-jbg:1");
        assert_eq!(df.bivouac, Bivouac::Forbidden);
        assert_eq!(df.access, Access::RoutesOnly);
        // Biodiv'sports terrestrial: avoid during its months.
        let b = f("biodiv:1");
        assert_eq!(b.access, Access::AvoidRecommended);
        assert_eq!(
            b.access_period.as_deref(),
            Some("mois sensibles : janvier, février, mars, décembre")
        );
        assert_eq!(b.bivouac, Bivouac::ConsentRequired);
    }

    #[test]
    fn outside_every_jurisdiction_is_unknown() {
        let rules = RuleSet::embedded().unwrap();
        let layer = ZoneLayer {
            zones: vec![zone(
                "x",
                ZoneKind::Natura2000Sic,
                BBox::new(44.8, 7.0, 44.9, 7.1),
            )],
        };
        let f = zone_fields(&layer, &rules, &layer.zones[0], d());
        assert_eq!((f.bivouac, f.access), (Bivouac::Unknown, Access::Unknown));
        assert!(f.bivouac_rules.is_empty());
    }
}
