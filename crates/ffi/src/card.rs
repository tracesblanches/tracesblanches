//! Cards of the map features the user touches (refuges today, stations once the pack has
//! them): a title, facts in the language of the app (French or English, VO6 #129; values
//! from the sources stay as published), phone numbers to dial, links, the source and the
//! date of the information. One call per touch.
//!
//! Station cards read a pack layer `stations` (GeoJSON points) whose properties follow the
//! schema documented in `android/docs/telechargement.md` (crate `stations` to come): `name`,
//! `emergency` (`[{label, phone}]`, 112 always shown first), `fee_outlets`
//! (`[{name, address, phone}]`), `prices` (`[{label, price_eur, date}]`), `phones`
//! (`[{label, phone}]`), `websites` (`[{label, url}]`), `source`, `updated`.

use serde_json::{Map, Value};

/// One line of a card.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Fact {
    /// Label in the language asked (`Places`).
    pub label: String,
    /// Value (`16`); words of the source as published.
    pub value: String,
}

/// A phone number the user can dial (the app opens the dialler, never calls itself).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Phone {
    /// What it is (`Refuge`, `Secours`).
    pub label: String,
    /// Number as displayed.
    pub display: String,
    /// Number for a `tel:` URI: digits and a leading `+` only.
    pub dial: String,
}

/// A web link.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Link {
    /// Label, French.
    pub label: String,
    /// URL (`https://...`).
    pub url: String,
}

/// The card of a map feature.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Card {
    /// Kind (`Cabane non gardée`, `Station`): the source's words, else in the language
    /// asked.
    pub kind: String,
    /// Name.
    pub title: String,
    /// Facts, most useful first.
    pub facts: Vec<Fact>,
    /// Numbers to dial.
    pub phones: Vec<Phone>,
    /// Links (the source page last).
    pub links: Vec<Link>,
    /// Source and licence.
    pub source: String,
    /// Date of the information (`AAAA-MM-JJ`), when known.
    pub updated: Option<String>,
}

/// Language of the card labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Lang {
    /// French.
    Fr,
    /// English (the default of the app outside French-speaking phones).
    En,
}

/// English of every French label of the cards (labels and the app's own words; values
/// from the sources are not translated).
const EN: &[(&str, &str)] = &[
    ("Refuge", "Hut"),
    ("Gardiennage", "Warden"),
    ("gardé", "wardened"),
    ("non gardé", "not wardened"),
    ("Places", "Beds"),
    ("État", "Condition"),
    ("poêle", "stove"),
    ("cheminée", "fireplace"),
    ("couvertures", "blankets"),
    ("latrines", "latrines"),
    ("Équipements", "Equipment"),
    ("Eau à moins de 100 m", "Water within 100 m"),
    ("oui", "yes"),
    ("non", "no"),
    ("Altitude", "Elevation"),
    ("Accès", "Access"),
    ("Dernier commentaire", "Latest comment"),
    ("Remarque", "Note"),
    ("Fiche refuges.info", "refuges.info page"),
    ("Refuge sans nom", "Unnamed hut"),
    ("Cabane non gardée", "Unwardened hut"),
    ("Abri", "Shelter"),
    ("Abri ou refuge", "Shelter or hut"),
    ("Cheminée", "Fireplace"),
    ("Payant", "Fee"),
    ("Description", "Description"),
    ("Téléphone", "Phone"),
    ("Site web", "Website"),
    ("Fiche OpenStreetMap", "OpenStreetMap page"),
    ("Sans nom", "Unnamed"),
    ("Gîte", "Lodge"),
    ("Bivouac", "Bivouac"),
    ("toujours accessible", "always open"),
    ("accessible pendant le gardiennage", "open when wardened"),
    ("clés à demander", "keys on request"),
    ("jamais gardé", "never wardened"),
    ("Places gardées", "Beds when wardened"),
    ("Places hors gardiennage", "Beds when not wardened"),
    ("Période d'accès", "Access period"),
    ("chauffage", "heating"),
    ("matelas", "mattresses"),
    ("gaz", "gas"),
    (
        "Équipements hors gardiennage",
        "Equipment when not wardened",
    ),
    ("Fiche Camptocamp", "Camptocamp page"),
    ("Secours (urgence)", "Rescue (emergency)"),
    ("Secours", "Rescue"),
    ("Vente de la redevance", "Trail pass sold at"),
    ("Point de vente", "Sales point"),
    ("Tarif", "Price"),
    ("Station", "Ski area"),
    ("Voir le plan officiel", "See the official trail map"),
];

/// `fr` in `lang`.
fn t(lang: Lang, fr: &'static str) -> &'static str {
    match lang {
        Lang::Fr => fr,
        Lang::En => EN.iter().find(|(f, _)| *f == fr).map_or(fr, |(_, e)| *e),
    }
}

/// Card of a feature of `pack_layer` from its properties (JSON object as given by the
/// map), labels in `lang`. `None`: not a layer with cards.
#[uniffi::export]
pub fn feature_card(pack_layer: String, properties_json: String, lang: Lang) -> Option<Card> {
    let props: Map<String, Value> = serde_json::from_str(&properties_json).ok()?;
    match pack_layer.as_str() {
        "refuges-info" => Some(refuges_info(&props, lang)),
        "refuges-osm" => Some(refuges_osm(&props, lang)),
        "refuges-camptocamp" => Some(camptocamp(&props, lang)),
        "stations" => Some(station(&props, lang)),
        other => crate::card_map::area_card(other, &props, lang),
    }
}

/// A property that may be a nested object or the same object encoded as a string (maps
/// flatten nested values to strings).
fn nested(props: &Map<String, Value>, key: &str) -> Option<Map<String, Value>> {
    match props.get(key)? {
        Value::Object(o) => Some(o.clone()),
        Value::String(s) => serde_json::from_str(s).ok(),
        _ => None,
    }
}

fn list(props: &Map<String, Value>, key: &str) -> Vec<Value> {
    match props.get(key) {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn yes(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.as_str() {
            "yes" | "true" => Some(true),
            "no" | "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn fact(facts: &mut Vec<Fact>, label: &str, value: Option<String>) {
    if let Some(value) = value.filter(|v| !v.is_empty()) {
        facts.push(Fact {
            label: label.to_owned(),
            value,
        });
    }
}

/// `+33 4 00 00 05 04` → `+33400000504`; `None` when there are not enough digits.
pub fn dial_number(display: &str) -> Option<String> {
    let mut out = String::new();
    for (i, c) in display.trim().chars().enumerate() {
        if c.is_ascii_digit() || (c == '+' && i == 0) {
            out.push(c);
        }
    }
    (out.chars().filter(char::is_ascii_digit).count() >= 3).then_some(out)
}

fn phones_from(raw: &str, label: &str, out: &mut Vec<Phone>) {
    for part in raw.split([';', ',']) {
        if let Some(dial) = dial_number(part) {
            out.push(Phone {
                label: label.to_owned(),
                display: part.trim().to_owned(),
                dial,
            });
        }
    }
}

pub(crate) fn link(links: &mut Vec<Link>, label: &str, url: Option<String>) {
    if let Some(url) = url.filter(|u| u.starts_with("https://") || u.starts_with("http://")) {
        links.push(Link {
            label: label.to_owned(),
            url,
        });
    }
}

fn day(s: Option<String>) -> Option<String> {
    s.map(|s| s.chars().take(10).collect())
}

fn metres(v: Option<f64>) -> Option<String> {
    v.map(|m| format!("{} m", m.round()))
}

fn refuges_info(p: &Map<String, Value>, l: Lang) -> Card {
    let kind = text(p.get("type_label")).unwrap_or_else(|| t(l, "Refuge").to_owned());
    let mut facts = Vec::new();
    let guarded = kind.contains("gardé") && !kind.contains("non gardé");
    fact(
        &mut facts,
        t(l, "Gardiennage"),
        Some(
            if guarded {
                t(l, "gardé")
            } else {
                t(l, "non gardé")
            }
            .to_owned(),
        ),
    );
    let places = num(p.get("places")).filter(|n| *n > 0.0);
    let mattress = num(p.get("places_mattress")).filter(|n| *n > 0.0);
    fact(
        &mut facts,
        t(l, "Places"),
        match (places, mattress) {
            (Some(n), Some(m)) => Some(match l {
                Lang::Fr => format!("{n}, dont {m} sur matelas"),
                Lang::En => format!("{n}, {m} with mattresses"),
            }),
            (Some(n), None) => Some(n.to_string()),
            _ => None,
        },
    );
    fact(&mut facts, t(l, "État"), text(p.get("state")));
    let equipment: Vec<&str> = [
        ("stove", t(l, "poêle")),
        ("fireplace", t(l, "cheminée")),
        ("blankets", t(l, "couvertures")),
        ("latrines", t(l, "latrines")),
    ]
    .iter()
    .filter(|(k, _)| yes(p.get(*k)) == Some(true))
    .map(|(_, l)| *l)
    .collect();
    fact(
        &mut facts,
        t(l, "Équipements"),
        (!equipment.is_empty()).then(|| equipment.join(", ")),
    );
    fact(
        &mut facts,
        t(l, "Eau à moins de 100 m"),
        yes(p.get("water_100m")).map(|w| if w { t(l, "oui") } else { t(l, "non") }.to_owned()),
    );
    fact(&mut facts, t(l, "Altitude"), metres(num(p.get("alt"))));
    fact(&mut facts, t(l, "Accès"), text(p.get("access")));
    if let Some(c) = nested(p, "last_comment") {
        let date = day(text(c.get("date")));
        fact(
            &mut facts,
            &format!(
                "{}{}",
                t(l, "Dernier commentaire"),
                date.map(|d| format!(" ({d})")).unwrap_or_default()
            ),
            text(c.get("text")),
        );
    }
    fact(&mut facts, t(l, "Remarque"), text(p.get("remark")));
    let mut links = Vec::new();
    link(&mut links, t(l, "Fiche refuges.info"), text(p.get("url")));
    Card {
        kind,
        title: text(p.get("name")).unwrap_or_else(|| t(l, "Refuge sans nom").to_owned()),
        facts,
        phones: Vec::new(),
        links,
        source: "refuges.info (CC BY-SA 2.0)".to_owned(),
        updated: day(text(p.get("modified"))),
    }
}

fn refuges_osm(p: &Map<String, Value>, l: Lang) -> Card {
    let tags = nested(p, "tags").unwrap_or_default();
    let kind = match text(p.get("kind")).as_deref() {
        Some("alpine_hut") => t(l, "Refuge"),
        Some("wilderness_hut") => t(l, "Cabane non gardée"),
        Some("shelter") => t(l, "Abri"),
        _ => t(l, "Abri ou refuge"),
    }
    .to_owned();
    let mut facts = Vec::new();
    fact(&mut facts, t(l, "Places"), text(tags.get("beds")));
    fact(
        &mut facts,
        t(l, "Cheminée"),
        yes(tags.get("fireplace")).map(|f| if f { t(l, "oui") } else { t(l, "non") }.to_owned()),
    );
    fact(&mut facts, t(l, "Payant"), text(tags.get("fee")));
    fact(&mut facts, t(l, "Accès"), text(tags.get("access")));
    fact(&mut facts, t(l, "Altitude"), metres(num(tags.get("ele"))));
    fact(
        &mut facts,
        t(l, "Description"),
        text(tags.get("description")),
    );
    let mut phones = Vec::new();
    if let Some(raw) = text(tags.get("phone")) {
        phones_from(&raw, t(l, "Téléphone"), &mut phones);
    }
    let mut links = Vec::new();
    link(&mut links, t(l, "Site web"), text(tags.get("website")));
    if let (Some(kind_osm), Some(id)) = (text(p.get("osm_type")), text(p.get("osm_id"))) {
        link(
            &mut links,
            t(l, "Fiche OpenStreetMap"),
            Some(format!("https://www.openstreetmap.org/{kind_osm}/{id}")),
        );
    }
    Card {
        kind,
        title: text(tags.get("name")).unwrap_or_else(|| t(l, "Sans nom").to_owned()),
        facts,
        phones,
        links,
        source: "© les contributeurs d'OpenStreetMap (ODbL)".to_owned(),
        updated: None,
    }
}

fn camptocamp(p: &Map<String, Value>, l: Lang) -> Card {
    let kind = match text(p.get("waypoint_type")).as_deref() {
        Some("hut") => t(l, "Refuge"),
        Some("gite") => t(l, "Gîte"),
        Some("shelter") => t(l, "Abri"),
        Some("bivouac") => t(l, "Bivouac"),
        _ => t(l, "Refuge"),
    }
    .to_owned();
    let mut facts = Vec::new();
    fact(
        &mut facts,
        t(l, "Gardiennage"),
        text(p.get("custodianship")).map(|c| {
            match c.as_str() {
                "always_accessible" => t(l, "toujours accessible"),
                "accessible_when_wardened" => t(l, "accessible pendant le gardiennage"),
                "keys_needed" => t(l, "clés à demander"),
                "no_warden" => t(l, "jamais gardé"),
                other => other,
            }
            .to_owned()
        }),
    );
    fact(
        &mut facts,
        t(l, "Places gardées"),
        text(p.get("capacity_staffed")),
    );
    fact(
        &mut facts,
        t(l, "Places hors gardiennage"),
        text(p.get("capacity_unstaffed")),
    );
    fact(
        &mut facts,
        t(l, "Période d'accès"),
        text(p.get("access_period")),
    );
    let equipment: Vec<&str> = [
        ("heating_unstaffed", t(l, "chauffage")),
        ("blanket_unstaffed", t(l, "couvertures")),
        ("mattress_unstaffed", t(l, "matelas")),
        ("gas_unstaffed", t(l, "gaz")),
    ]
    .iter()
    .filter(|(k, _)| yes(p.get(*k)) == Some(true))
    .map(|(_, l)| *l)
    .collect();
    fact(
        &mut facts,
        t(l, "Équipements hors gardiennage"),
        (!equipment.is_empty()).then(|| equipment.join(", ")),
    );
    fact(
        &mut facts,
        t(l, "Altitude"),
        metres(num(p.get("elevation"))),
    );
    let mut links = Vec::new();
    link(&mut links, t(l, "Fiche Camptocamp"), text(p.get("url")));
    Card {
        kind,
        title: text(p.get("title")).unwrap_or_else(|| t(l, "Sans nom").to_owned()),
        facts,
        phones: Vec::new(),
        links,
        source: "© les contributeurs de Camptocamp.org (CC BY-SA 3.0)".to_owned(),
        updated: None,
    }
}

fn station(p: &Map<String, Value>, l: Lang) -> Card {
    let mut phones = vec![Phone {
        label: t(l, "Secours (urgence)").to_owned(),
        display: "112".to_owned(),
        dial: "112".to_owned(),
    }];
    for e in list(p, "emergency") {
        if let Some(raw) = text(e.get("phone")) {
            phones_from(
                &raw,
                &text(e.get("label")).unwrap_or_else(|| t(l, "Secours").to_owned()),
                &mut phones,
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    phones.retain(|p| seen.insert(p.dial.clone()));
    let mut facts = Vec::new();
    for o in list(p, "fee_outlets") {
        let what = [text(o.get("name")), text(o.get("address"))]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
        fact(&mut facts, t(l, "Vente de la redevance"), Some(what));
        if let Some(raw) = text(o.get("phone")) {
            phones_from(
                &raw,
                &text(o.get("name")).unwrap_or_else(|| t(l, "Point de vente").to_owned()),
                &mut phones,
            );
        }
    }
    for pr in list(p, "prices") {
        let price = num(pr.get("price_eur")).map(|e| format!("{e:.2} €").replace('.', ","));
        let date = day(text(pr.get("date")));
        fact(
            &mut facts,
            &text(pr.get("label")).unwrap_or_else(|| t(l, "Tarif").to_owned()),
            price.map(|p| match (date, l) {
                (Some(d), Lang::Fr) => format!("{p} (prix indicatif du {d})"),
                (None, Lang::Fr) => format!("{p} (prix indicatif)"),
                (Some(d), Lang::En) => format!("{p} (indicative price, {d})"),
                (None, Lang::En) => format!("{p} (indicative price)"),
            }),
        );
    }
    for ph in list(p, "phones") {
        if let Some(raw) = text(ph.get("phone")) {
            phones_from(
                &raw,
                &text(ph.get("label")).unwrap_or_else(|| t(l, "Téléphone").to_owned()),
                &mut phones,
            );
        }
    }
    let mut links = Vec::new();
    for w in list(p, "websites") {
        link(
            &mut links,
            &text(w.get("label")).unwrap_or_else(|| t(l, "Site web").to_owned()),
            text(w.get("url")),
        );
    }
    // The official piste plan of the resort (pack property `plan_url`): a link only.
    link(
        &mut links,
        t(l, "Voir le plan officiel"),
        text(p.get("plan_url")),
    );
    Card {
        kind: t(l, "Station").to_owned(),
        title: text(p.get("name")).unwrap_or_else(|| t(l, "Station").to_owned()),
        facts,
        phones,
        links,
        source: text(p.get("source")).unwrap_or_default(),
        updated: day(text(p.get("updated"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_to_dial() {
        assert_eq!(
            dial_number("+33 4 00 00 05 04").as_deref(),
            Some("+33400000504")
        );
        assert_eq!(dial_number("04.00.00-05-04").as_deref(), Some("0400000504"));
        assert_eq!(dial_number("n/a"), None);
    }

    #[test]
    fn refuges_info_card() {
        let c = feature_card(
            "refuges-info".into(),
            r#"{"name": "Cabane du Lac", "type_label": "cabane non gardée", "places": 6,
                "places_mattress": 4, "stove": true, "blankets": "true", "fireplace": false,
                "water_100m": false, "alt": 2012.4, "access": "Par le col en hiver",
                "last_comment": "{\"date\": \"2026-02-03 10:00\", \"text\": \"Bois sec\"}",
                "modified": "2025-12-01 08:00:00", "url": "https://www.refuges.info/point/1"}"#
                .into(),
            Lang::Fr,
        )
        .unwrap();
        assert_eq!(c.title, "Cabane du Lac");
        assert_eq!(c.kind, "cabane non gardée");
        let get = |l: &str| {
            c.facts
                .iter()
                .find(|f| f.label == l)
                .map(|f| f.value.as_str())
        };
        assert_eq!(get("Gardiennage"), Some("non gardé"));
        assert_eq!(get("Places"), Some("6, dont 4 sur matelas"));
        assert_eq!(get("Équipements"), Some("poêle, couvertures"));
        assert_eq!(get("Eau à moins de 100 m"), Some("non"));
        assert_eq!(get("Altitude"), Some("2012 m"));
        assert_eq!(get("Accès"), Some("Par le col en hiver"));
        assert_eq!(get("Dernier commentaire (2026-02-03)"), Some("Bois sec"));
        assert_eq!(c.updated.as_deref(), Some("2025-12-01"));
        assert_eq!(c.links[0].url, "https://www.refuges.info/point/1");
        assert!(c.phones.is_empty());
    }

    #[test]
    fn osm_and_camptocamp_cards() {
        let c = feature_card(
            "refuges-osm".into(),
            r#"{"kind": "alpine_hut", "osm_type": "node", "osm_id": 42,
                "tags": {"name": "La Jaique", "phone": "+33 3 81 00 00 00; 06 00 00 00 00",
                         "website": "https://example.org", "ele": "1370"}}"#
                .into(),
            Lang::Fr,
        )
        .unwrap();
        assert_eq!(c.kind, "Refuge");
        assert_eq!(c.title, "La Jaique");
        let dials: Vec<&str> = c.phones.iter().map(|p| p.dial.as_str()).collect();
        assert_eq!(dials, ["+33381000000", "0600000000"]);
        assert_eq!(c.links[1].url, "https://www.openstreetmap.org/node/42");
        let c = feature_card(
            "refuges-camptocamp".into(),
            r#"{"title": "Chalet Gaillard", "waypoint_type": "hut", "custodianship": "always_accessible",
                "capacity_staffed": 19, "access_period": "Toute l'année", "url": "https://www.camptocamp.org/waypoints/552665"}"#
                .into(), Lang::Fr,
        )
        .unwrap();
        assert_eq!(c.facts[0].value, "toujours accessible");
        assert_eq!(c.facts[1].value, "19");
        assert!(feature_card("pistes".into(), "{}".into(), Lang::Fr).is_none());
        assert!(feature_card("refuges-info".into(), "not json".into(), Lang::Fr).is_none());
    }

    #[test]
    fn station_card_puts_112_first() {
        let c = feature_card(
            "stations".into(),
            r#"{"name": "Chapelle-des-Bois", "emergency": [{"label": "Pisteurs", "phone": "03 00 00 00 00"},
                {"label": "Urgence", "phone": "112"}],
                "fee_outlets": [{"name": "Foyer de ski de fond", "address": "Village", "phone": "03 00 00 11 11"}],
                "prices": [{"label": "Redevance journée", "price_eur": 9.5, "date": "2026-11-01"}],
                "websites": [{"label": "Office de tourisme", "url": "https://example.org/ot"}],
                "plan_url": "https://example.org/plan.pdf",
                "source": "Espace Nordique Jurassien", "updated": "2026-11-10"}"#
                .into(), Lang::Fr,
        )
        .unwrap();
        let dials: Vec<&str> = c.phones.iter().map(|p| p.dial.as_str()).collect();
        assert_eq!(dials, ["112", "0300000000", "0300001111"]);
        assert_eq!(c.phones[0].label, "Secours (urgence)");
        assert_eq!(c.facts[0].value, "Foyer de ski de fond, Village");
        assert_eq!(c.facts[1].value, "9,50 € (prix indicatif du 2026-11-01)");
        assert_eq!(c.updated.as_deref(), Some("2026-11-10"));
        let links: Vec<(&str, &str)> = c
            .links
            .iter()
            .map(|l| (l.label.as_str(), l.url.as_str()))
            .collect();
        assert_eq!(
            links,
            [
                ("Office de tourisme", "https://example.org/ot"),
                ("Voir le plan officiel", "https://example.org/plan.pdf")
            ]
        );
    }

    #[test]
    fn english_labels() {
        let props = r#"{"name": "Cabane du Lac", "type_label": "cabane non gardée", "places": 6,
            "places_mattress": 4, "stove": true, "water_100m": true, "alt": 2012.4,
            "last_comment": {"date": "2026-02-03", "text": "Bois sec"}}"#;
        let c = feature_card("refuges-info".into(), props.into(), Lang::En).unwrap();
        let labels: Vec<&str> = c.facts.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Warden",
                "Beds",
                "Equipment",
                "Water within 100 m",
                "Elevation",
                "Latest comment (2026-02-03)"
            ]
        );
        assert_eq!(c.facts[0].value, "not wardened");
        assert_eq!(c.facts[1].value, "6, 4 with mattresses");
        assert_eq!(c.facts[3].value, "yes");
        // Words of the source stay as published.
        assert_eq!(c.kind, "cabane non gardée");
        assert_eq!(c.facts[5].value, "Bois sec");
        let s = feature_card(
            "stations".into(),
            r#"{"prices": [{"price_eur": 9.5}]}"#.into(),
            Lang::En,
        )
        .unwrap();
        assert_eq!(s.kind, "Ski area");
        assert_eq!(s.phones[0].label, "Rescue (emergency)");
        assert_eq!(s.facts[0].label, "Price");
        assert_eq!(s.facts[0].value, "9,50 € (indicative price)");
        // Every French label has its own English word.
        let mut fr: Vec<&str> = EN.iter().map(|(f, _)| *f).collect();
        fr.sort_unstable();
        fr.dedup();
        assert_eq!(fr.len(), EN.len());
    }
}
