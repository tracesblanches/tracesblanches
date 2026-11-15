//! Station cards (H1, #132): for each census domain, the rescue numbers and posts, the
//! pass sale points, the contacts (station, tourist office, ski school) and the car parks.
//! Information only: no purchase in the app.

use serde::Serialize;
use skimap_refuges::privacy::Phone;

use crate::contacts::{Link, StationContacts};
use crate::domains::Domain;
use crate::pois::{Category, Poi};

/// An emergency number.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Number {
    /// Number to dial.
    pub number: String,
    /// French label.
    pub label: String,
}

/// European emergency number, always first in every rescue card.
pub fn emergency_numbers() -> Vec<Number> {
    vec![
        Number {
            number: "112".to_owned(),
            label: "urgences, numéro européen (fonctionne avec tout réseau mobile disponible)"
                .to_owned(),
        },
        Number {
            number: "15".to_owned(),
            label: "SAMU (urgence médicale)".to_owned(),
        },
        Number {
            number: "114".to_owned(),
            label: "urgences par SMS (personnes sourdes ou ne pouvant pas parler)".to_owned(),
        },
    ]
}

/// A point of a card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    /// Kind (see [`Poi::kind`]).
    pub kind: String,
    /// Name.
    pub name: Option<String>,
    /// Phones.
    pub phones: Vec<Phone>,
    /// Web site.
    pub website: Option<String>,
    /// Opening hours.
    pub opening_hours: Option<String>,
    /// Distance to the domain footprint, m (0 inside).
    pub distance_m: f64,
    /// Longitude, latitude.
    pub lon_lat: [f64; 2],
    /// OSM page.
    pub url: String,
}

/// A car park of a card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Parking {
    /// Name.
    pub name: Option<String>,
    /// Capacity.
    pub capacity: Option<String>,
    /// `fee`.
    pub fee: Option<String>,
    /// Distance to the footprint, m.
    pub distance_m: f64,
    /// Longitude, latitude.
    pub lon_lat: [f64; 2],
    /// OSM page.
    pub url: String,
}

/// Rescue part of a card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Secours {
    /// Numbers to dial, 112 first.
    pub numbers: Vec<Number>,
    /// Posts and equipment (OSM), nearest first.
    pub posts: Vec<Place>,
}

/// A station card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fiche {
    /// Domain id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Department.
    pub departement: String,
    /// Commune of the representative point.
    pub commune: String,
    /// Nordic km.
    pub km_nordic: f64,
    /// Rescue.
    pub secours: Secours,
    /// Pass sale points (OSM); information only.
    pub vente: Vec<Place>,
    /// Station web links (station, ski school, SkiRadar page).
    pub links: Vec<Link>,
    /// Station phones (OSM site relation).
    pub phones: Vec<Phone>,
    /// Operator (OSM site relation).
    pub operator: Option<String>,
    /// Tourist offices (OSM, 2 nearest within 6 km).
    pub offices_tourisme: Vec<Place>,
    /// Ski schools (OSM, 2 nearest within 6 km).
    pub ecoles_ski: Vec<Place>,
    /// Car parks within 300 m, nearest first.
    pub parkings: Vec<Parking>,
    /// What the card has: `secours` (a post beyond the numbers), `vente`, `telephone`
    /// (any phone), `site` (any web link), `parking`.
    pub has: Vec<String>,
}

fn place(p: &Poi, d: f64) -> Place {
    Place {
        kind: p.kind.clone(),
        name: p.name.clone(),
        phones: p.phones.clone(),
        website: p.website.clone(),
        opening_hours: p.opening_hours.clone(),
        distance_m: d,
        lon_lat: [p.pos.lon, p.pos.lat],
        url: p.url.clone(),
    }
}

/// Order of the rescue kinds on a card: staffed posts first, then equipment.
fn secours_rank(kind: &str) -> u8 {
    match kind {
        "patrol_post" | "first_aid" => 0,
        "mountain_rescue" => 1,
        "ambulance_station" | "fire_station" => 2,
        "defibrillator" => 3,
        "emergency_phone" => 4,
        _ => 5,
    }
}

/// Builds the card of a domain from the points attached to it.
pub fn fiche(d: &Domain, pois: &[Poi], contacts: Option<&StationContacts>) -> Fiche {
    let mine: Vec<(&Poi, f64)> = pois
        .iter()
        .filter_map(|p| {
            p.domains
                .iter()
                .find(|(id, _)| *id == d.id)
                .map(|(_, m)| (p, *m))
        })
        .collect();
    let of = |c: Category| {
        let mut v: Vec<Place> = mine
            .iter()
            .filter(|(p, _)| p.category == c)
            .map(|(p, m)| place(p, *m))
            .collect();
        v.sort_by(|a, b| {
            a.distance_m
                .total_cmp(&b.distance_m)
                .then(a.url.cmp(&b.url))
        });
        v
    };
    let mut posts = of(Category::Secours);
    posts.sort_by(|a, b| {
        secours_rank(&a.kind)
            .cmp(&secours_rank(&b.kind))
            .then(a.distance_m.total_cmp(&b.distance_m))
    });
    let mut parkings: Vec<Parking> = mine
        .iter()
        .filter(|(p, _)| p.category == Category::Parking)
        .map(|(p, m)| Parking {
            name: p.name.clone(),
            capacity: p.capacity.clone(),
            fee: p.fee.clone(),
            distance_m: *m,
            lon_lat: [p.pos.lon, p.pos.lat],
            url: p.url.clone(),
        })
        .collect();
    parkings.sort_by(|a, b| {
        a.distance_m
            .total_cmp(&b.distance_m)
            .then(a.url.cmp(&b.url))
    });
    let c = contacts.cloned().unwrap_or_default();
    let vente = of(Category::Vente);
    let offices_tourisme = of(Category::OfficeTourisme);
    let ecoles_ski = of(Category::EcoleSki);
    let mut has = Vec::new();
    if !posts.is_empty() {
        has.push("secours".to_owned());
    }
    if !vente.is_empty() {
        has.push("vente".to_owned());
    }
    let any_phone = !c.phones.is_empty()
        || [&posts, &vente, &offices_tourisme, &ecoles_ski]
            .iter()
            .any(|v| v.iter().any(|p| !p.phones.is_empty()));
    if any_phone {
        has.push("telephone".to_owned());
    }
    let any_site = !c.links.is_empty()
        || [&vente, &offices_tourisme, &ecoles_ski]
            .iter()
            .any(|v| v.iter().any(|p| p.website.is_some()));
    if any_site {
        has.push("site".to_owned());
    }
    if !parkings.is_empty() {
        has.push("parking".to_owned());
    }
    Fiche {
        id: d.id.clone(),
        name: d.name.clone(),
        departement: d.departement.clone(),
        commune: d.commune.clone(),
        km_nordic: d.km_nordic,
        secours: Secours {
            numbers: emergency_numbers(),
            posts,
        },
        vente,
        links: c.links,
        phones: c.phones,
        operator: c.operator,
        offices_tourisme,
        ecoles_ski,
        parkings,
        has,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_112_first() {
        assert_eq!(emergency_numbers()[0].number, "112");
    }

    #[test]
    fn rescue_order() {
        assert!(secours_rank("patrol_post") < secours_rank("defibrillator"));
        assert!(secours_rank("mountain_rescue") < secours_rank("emergency_phone"));
    }
}
