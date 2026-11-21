//! Cards of the map lines and areas the user touches: a piste section (with its area and
//! the trail pass of the place), a regulated zone, a zone without network, a known
//! avalanche path. Labels in French or English; values of the sources as published.

use serde_json::{Map, Value};

use crate::card::{Card, Fact, Lang, Link, link};

/// English of the French labels of these cards.
const EN: &[(&str, &str)] = &[
    ("Piste", "Trail"),
    ("Piste sans nom", "Unnamed trail"),
    ("Piste verte", "Green trail"),
    ("Piste bleue", "Blue trail"),
    ("Piste rouge", "Red trail"),
    ("Piste noire", "Black trail"),
    ("Hors piste balisé", "Marked backcountry route"),
    ("Raquettes et piétons", "Snowshoes and walkers"),
    ("Difficulté", "Difficulty"),
    ("très facile", "very easy"),
    ("facile", "easy"),
    ("moyenne", "intermediate"),
    ("difficile", "difficult"),
    ("inconnue", "unknown"),
    ("Technique", "Technique"),
    ("classique et skating", "classic and skating"),
    ("classique", "classic"),
    ("skating", "skating"),
    ("Longueur du tronçon", "Section length"),
    ("Sens", "Direction"),
    ("sens unique", "one way"),
    ("dans les deux sens", "both ways"),
    ("État", "Status"),
    ("ouverte", "open"),
    ("fermée", "closed"),
    ("inconnu", "unknown"),
    ("au", "on"),
    ("Itinéraires", "Routes"),
    ("Domaine", "Area"),
    ("Redevance", "Trail pass"),
    ("Zone réglementée", "Regulated area"),
    (
        "Arrêté de protection de biotope",
        "Biotope protection order",
    ),
    ("Réserve naturelle nationale", "National nature reserve"),
    ("Réserve naturelle régionale", "Regional nature reserve"),
    ("Parc naturel régional", "Regional nature park"),
    ("Parc national", "National park"),
    ("Site classé", "Listed site"),
    ("Site inscrit", "Registered site"),
    ("Site patrimonial remarquable", "Heritage site"),
    ("Monument historique", "Historic monument"),
    (
        "Abords d'un monument historique",
        "Surroundings of a historic monument",
    ),
    ("Protection de captage", "Water catchment protection"),
    ("Forêt de protection", "Protection forest"),
    ("Natura 2000 (habitats)", "Natura 2000 (habitats)"),
    ("Natura 2000 (oiseaux)", "Natura 2000 (birds)"),
    ("Réserve biologique", "Biological reserve"),
    ("Zone de quiétude de la faune", "Wildlife quiet zone"),
    (
        "Réserve de chasse (district franc)",
        "Hunting reserve (district franc)",
    ),
    ("Pays", "Country"),
    ("Département", "Department"),
    ("Canton", "Canton"),
    ("Commune", "Municipality"),
    ("Ski", "Skiing"),
    ("Bivouac", "Bivouac"),
    ("interdit", "forbidden"),
    ("interdit par endroits", "forbidden in places"),
    ("itinéraires balisés seulement", "marked routes only"),
    (
        "itinéraires balisés seulement, par endroits",
        "marked routes only, in places",
    ),
    ("à éviter (faune)", "avoid (wildlife)"),
    ("autorisé", "allowed"),
    ("toléré", "tolerated"),
    ("avec l'accord du propriétaire", "with the owner's consent"),
    ("Période", "Period"),
    ("Règles", "Rules"),
    ("Fiche de la zone", "Area page"),
    ("Zone sans réseau", "No network area"),
    ("Téléphone", "Phone"),
    ("aucun opérateur pour appeler", "no operator for calls"),
    ("Superficie", "Size"),
    ("Couloir d'avalanche connu", "Known avalanche path"),
    ("Risque", "Risk"),
    ("Voir le plan officiel", "See the official trail map"),
    ("Tracé", "Line"),
    (
        "Tracé incertain, d'après le plan de la station",
        "Uncertain line, from the resort's trail map",
    ),
    ("Longueur sur le plan", "Length on the map"),
    ("Départ", "Start"),
    (
        "Plan officiel de la station",
        "Official trail map of the resort",
    ),
    (
        "Tracé incertain, d'après un itinéraire publié du même nom",
        "Uncertain line, from a published route of the same name",
    ),
    (
        "Tracé incertain, routé sur les routes et chemins IGN",
        "Uncertain line, routed on the IGN roads and paths",
    ),
    ("Longueur annoncée", "Announced length"),
    ("Piste piétonne", "Walking trail"),
    ("Liaison", "Link"),
    ("Voir l'itinéraire repris", "See the route used"),
    ("Bulletin de l'exploitant", "Operator's bulletin"),
];

/// `fr` in `lang`.
fn t(lang: Lang, fr: &'static str) -> &'static str {
    match lang {
        Lang::Fr => fr,
        Lang::En => EN.iter().find(|(f, _)| *f == fr).map_or(fr, |(_, e)| *e),
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

/// An array property, or the same array encoded as a string (maps flatten nested values).
fn list(props: &Map<String, Value>, key: &str) -> Vec<Value> {
    match props.get(key) {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_default(),
        _ => Vec::new(),
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

fn word(facts: &mut Vec<Fact>, lang: Lang, label: &'static str, value: Option<&'static str>) {
    fact(facts, t(lang, label), value.map(|w| t(lang, w).to_owned()));
}

/// Card of a piste section (pack layer `pistes`) from its properties, with the name of the
/// area it belongs to (`domain`) and the trail-pass zones under the finger (`fees_json`:
/// properties of `fee-zones` features), labels in `lang`. `None`: the properties are not
/// a JSON object.
#[uniffi::export]
pub fn piste_card(
    properties_json: String,
    domain: Option<String>,
    fees_json: Vec<String>,
    lang: Lang,
) -> Option<Card> {
    let props: Map<String, Value> = serde_json::from_str(&properties_json).ok()?;
    let fees: Vec<Map<String, Value>> = fees_json
        .iter()
        .filter_map(|f| serde_json::from_str(f).ok())
        .collect();
    Some(piste(&props, domain.as_deref(), &fees, lang))
}

/// Card of a regulated zone, a zone without network or a known avalanche path.
pub(crate) fn area_card(pack_layer: &str, props: &Map<String, Value>, lang: Lang) -> Option<Card> {
    match pack_layer {
        "zones" => Some(zone(props, lang)),
        "no-network" => Some(no_network(props, lang)),
        "avalanche-areas" => Some(avalanche_area(props, lang)),
        "pistes-estimees" => Some(estimated_piste(props, lang)),
        _ => None,
    }
}

/// Names of the itineraries of a piste section (`relations[].name`), without repeats.
fn route_names(props: &Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in list(props, "relations") {
        if let Some(n) = text(r.get("name"))
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    out
}

/// `323 m`, `1,2 km` (`1.2 km` in English).
fn length_text(m: f64, lang: Lang) -> String {
    if m < 1000.0 {
        format!("{} m", m.round())
    } else {
        let km = format!("{:.1} km", m / 1000.0);
        match lang {
            Lang::Fr => km.replace('.', ","),
            Lang::En => km,
        }
    }
}

/// `Nordic Pass saison : 205 € (Adulte, 2026-2027)`.
fn fee_text(f: &Map<String, Value>) -> Option<String> {
    let name = text(f.get("name"));
    let price = list(f, "prices").into_iter().find_map(|p| {
        let eur = num(p.get("price_eur"))?;
        let detail: Vec<String> = [text(p.get("label")), text(p.get("date"))]
            .into_iter()
            .flatten()
            .collect();
        Some(if detail.is_empty() {
            format!("{eur} €")
        } else {
            format!("{eur} € ({})", detail.join(", "))
        })
    });
    match (name, price) {
        (Some(n), Some(p)) => Some(format!("{n} : {p}")),
        (Some(n), None) => Some(n),
        (None, p) => p,
    }
}

fn piste(
    props: &Map<String, Value>,
    domain: Option<&str>,
    fees: &[Map<String, Value>],
    lang: Lang,
) -> Card {
    let routes = route_names(props);
    let title = text(props.get("name"))
        .or_else(|| (!routes.is_empty()).then(|| routes.join(" · ")))
        .unwrap_or_else(|| t(lang, "Piste sans nom").to_owned());
    let difficulty = props.get("difficulty").and_then(Value::as_str);
    let uses = list(props, "uses");
    let walk = !uses.is_empty()
        && uses
            .iter()
            .all(|u| matches!(u.as_str(), Some("hike" | "snowshoe" | "walk")));
    let kind = if yes(props.get("backcountry")) == Some(true) {
        "Hors piste balisé"
    } else if walk {
        "Raquettes et piétons"
    } else {
        match difficulty {
            Some("novice") => "Piste verte",
            Some("easy") => "Piste bleue",
            Some("intermediate") => "Piste rouge",
            Some("advanced" | "expert") => "Piste noire",
            _ => "Piste",
        }
    };
    let mut facts = Vec::new();
    let state = props.get("state").and_then(Value::as_str);
    let state_word = match state {
        Some("open") => "ouverte",
        Some("closed") => "fermée",
        _ => "inconnu",
    };
    let state_at = text(props.get("state_at"));
    let state_value = match (&state_at, state) {
        (Some(d), Some("open" | "closed")) => {
            format!("{} ({} {d})", t(lang, state_word), t(lang, "au"))
        }
        _ => t(lang, state_word).to_owned(),
    };
    fact(&mut facts, t(lang, "État"), Some(state_value));
    let difficulty_word = match difficulty {
        Some("novice") => "très facile",
        Some("easy") => "facile",
        Some("intermediate") => "moyenne",
        Some("advanced" | "expert") => "difficile",
        _ => "inconnue",
    };
    word(&mut facts, lang, "Difficulté", Some(difficulty_word));
    let technique = match (
        yes(props.get("classic")) == Some(true),
        yes(props.get("skating")) == Some(true),
    ) {
        (true, true) => Some("classique et skating"),
        (true, false) => Some("classique"),
        (false, true) => Some("skating"),
        (false, false) => None,
    };
    word(&mut facts, lang, "Technique", technique);
    fact(
        &mut facts,
        t(lang, "Longueur du tronçon"),
        num(props.get("length_m")).map(|m| length_text(m, lang)),
    );
    let oneway = yes(props.get("oneway")).map(|o| {
        if o {
            "sens unique"
        } else {
            "dans les deux sens"
        }
    });
    word(&mut facts, lang, "Sens", oneway);
    if routes.len() > 1 || (routes.len() == 1 && routes[0] != title) {
        fact(&mut facts, t(lang, "Itinéraires"), Some(routes.join(", ")));
    }
    fact(&mut facts, t(lang, "Domaine"), domain.map(str::to_owned));
    for f in fees {
        fact(&mut facts, t(lang, "Redevance"), fee_text(f));
    }
    let mut source = String::from("OpenStreetMap (ODbL)");
    if let Some(s) = text(props.get("state_source")) {
        source.push_str(" ; ");
        source.push_str(&s);
    }
    let mut links = Vec::new();
    link(
        &mut links,
        t(lang, "Voir le plan officiel"),
        text(props.get("plan_url")),
    );
    Card {
        kind: t(lang, kind).to_owned(),
        title,
        facts,
        phones: Vec::new(),
        links,
        source,
        updated: state_at,
    }
}

/// Card of a piste known but absent from OSM (pack layer `pistes-estimees`, G1): its
/// line is estimated (official plan, published route of the same name, or IGN roads and
/// paths), never shown as sure.
fn estimated_piste(props: &Map<String, Value>, lang: Lang) -> Card {
    let method = props.get("method").and_then(Value::as_str).unwrap_or("");
    let kind = match (
        props.get("difficulty").and_then(Value::as_str),
        props.get("kind").and_then(Value::as_str),
    ) {
        (Some("novice"), _) => "Piste verte",
        (Some("easy"), _) => "Piste bleue",
        (Some("intermediate"), _) => "Piste rouge",
        (Some("advanced" | "expert"), _) => "Piste noire",
        (_, Some("pedestrian")) => "Piste piétonne",
        (_, Some("liaison")) => "Liaison",
        _ => "Piste",
    };
    let line = match method {
        "geotrek" => "Tracé incertain, d'après un itinéraire publié du même nom",
        "estimated_route" => "Tracé incertain, routé sur les routes et chemins IGN",
        _ => "Tracé incertain, d'après le plan de la station",
    };
    let mut facts = Vec::new();
    word(&mut facts, lang, "Tracé", Some(line));
    fact(
        &mut facts,
        t(lang, "Longueur annoncée"),
        num(props.get("length_km_announced")).map(|k| length_text(k * 1000.0, lang)),
    );
    let km: Vec<String> = list(props, "lengths_km_plan")
        .iter()
        .filter_map(|v| num(Some(v)))
        .map(|k| length_text(k * 1000.0, lang))
        .collect();
    fact(
        &mut facts,
        t(lang, "Longueur sur le plan"),
        (!km.is_empty()).then(|| km.join(", ")),
    );
    let departures: Vec<String> = list(props, "departures")
        .iter()
        .filter_map(|v| text(Some(v)))
        .collect();
    fact(
        &mut facts,
        t(lang, "Départ"),
        (!departures.is_empty()).then(|| departures.join(", ")),
    );
    let mut links = Vec::new();
    link(
        &mut links,
        t(lang, "Voir le plan officiel"),
        text(props.get("plan_url")),
    );
    link(
        &mut links,
        t(lang, "Voir l'itinéraire repris"),
        text(props.get("source_url")),
    );
    let named_by_plan = props
        .get("named_by")
        .and_then(Value::as_str)
        .is_none_or(|n| n == "plan");
    let mut source = if named_by_plan {
        t(lang, "Plan officiel de la station").to_owned()
    } else {
        t(lang, "Bulletin de l'exploitant").to_owned()
    };
    match method {
        "estimated_paths" | "estimated_route" => source.push_str(" ; IGN BD TOPO"),
        "geotrek" => {
            source.push_str(" ; Geotrek");
            if let Some(n) = text(props.get("source_name")) {
                source.push_str(&format!(" (« {n} »)"));
            }
        }
        _ => {}
    }
    Card {
        kind: t(lang, kind).to_owned(),
        title: text(props.get("name")).unwrap_or_else(|| t(lang, "Piste sans nom").to_owned()),
        facts,
        phones: Vec::new(),
        links,
        source,
        updated: None,
    }
}

/// Words of a zone kind (`kind` of `skimap-zones`).
fn zone_kind(kind: &str) -> &'static str {
    match kind {
        "appb" => "Arrêté de protection de biotope",
        "reserve_naturelle_nationale" => "Réserve naturelle nationale",
        "reserve_naturelle_regionale" => "Réserve naturelle régionale",
        "parc_naturel_regional" => "Parc naturel régional",
        "parc_national" => "Parc national",
        "site_classe" => "Site classé",
        "site_inscrit" => "Site inscrit",
        "spr" => "Site patrimonial remarquable",
        "monument_historique" => "Monument historique",
        "monument_abords" => "Abords d'un monument historique",
        "captage_protection" => "Protection de captage",
        "foret_protection" => "Forêt de protection",
        "natura2000_sic" => "Natura 2000 (habitats)",
        "natura2000_zps" => "Natura 2000 (oiseaux)",
        "reserve_biologique" => "Réserve biologique",
        "biodiv_sports" => "Zone de quiétude de la faune",
        "district_franc" => "Réserve de chasse (district franc)",
        "country" => "Pays",
        "department" => "Département",
        "canton" => "Canton",
        "commune" => "Commune",
        _ => "Zone réglementée",
    }
}

fn zone(props: &Map<String, Value>, lang: Lang) -> Card {
    let kind = t(
        lang,
        zone_kind(props.get("kind").and_then(Value::as_str).unwrap_or("")),
    );
    let mut facts = Vec::new();
    let access = match props.get("access").and_then(Value::as_str) {
        Some("forbidden") => Some("interdit"),
        Some("forbidden_in_places") => Some("interdit par endroits"),
        Some("routes_only") => Some("itinéraires balisés seulement"),
        Some("routes_only_in_places") => Some("itinéraires balisés seulement, par endroits"),
        Some("avoid_recommended") => Some("à éviter (faune)"),
        Some("allowed") => Some("autorisé"),
        _ => None,
    };
    word(&mut facts, lang, "Ski", access);
    fact(
        &mut facts,
        t(lang, "Période"),
        text(props.get("access_period")),
    );
    let bivouac = match props.get("bivouac").and_then(Value::as_str) {
        Some("forbidden") => Some("interdit"),
        Some("forbidden_in_places") => Some("interdit par endroits"),
        Some("tolerated") => Some("toléré"),
        Some("consent_required") => Some("avec l'accord du propriétaire"),
        Some("allowed") => Some("autorisé"),
        _ => None,
    };
    word(&mut facts, lang, "Bivouac", bivouac);
    fact(&mut facts, t(lang, "Règles"), text(props.get("rules_text")));
    let links = text(props.get("url"))
        .map(|url| Link {
            label: t(lang, "Fiche de la zone").to_owned(),
            url,
        })
        .into_iter()
        .collect();
    Card {
        kind: kind.to_owned(),
        title: text(props.get("name")).unwrap_or_else(|| kind.to_owned()),
        facts,
        phones: Vec::new(),
        links,
        source: text(props.get("source")).unwrap_or_default(),
        updated: text(props.get("designated")),
    }
}

fn no_network(props: &Map<String, Value>, lang: Lang) -> Card {
    let mut facts = Vec::new();
    if props.get("kind").and_then(Value::as_str) == Some("no_voice_network_any_operator") {
        word(
            &mut facts,
            lang,
            "Téléphone",
            Some("aucun opérateur pour appeler"),
        );
    }
    fact(
        &mut facts,
        t(lang, "Superficie"),
        num(props.get("area_ha")).map(|h| format!("{h} ha")),
    );
    let title = t(lang, "Zone sans réseau");
    Card {
        kind: title.to_owned(),
        title: title.to_owned(),
        facts,
        phones: Vec::new(),
        links: Vec::new(),
        source: "ARCEP".to_owned(),
        updated: text(props.get("release")),
    }
}

fn avalanche_area(props: &Map<String, Value>, lang: Lang) -> Card {
    let mut facts = Vec::new();
    fact(&mut facts, t(lang, "Risque"), text(props.get("risque")));
    fact(&mut facts, t(lang, "Commune"), text(props.get("commune")));
    let title = t(lang, "Couloir d'avalanche connu");
    Card {
        kind: title.to_owned(),
        title: title.to_owned(),
        facts,
        phones: Vec::new(),
        links: Vec::new(),
        source: text(props.get("source")).unwrap_or_else(|| "CLPA (INRAE)".to_owned()),
        updated: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real section of Cervières (`layers/pistes.geojson`), closed with a date.
    const SOULIERS: &str = r#"{"backcountry":false,"classic":true,"difficulty":"intermediate",
        "length_m":322.9,"name":"Souliers","oneway":false,
        "relations":[{"colour":"red","name":"Souliers"},{"name":"Izoard"}],
        "skating":true,"state":"closed","state_at":"2026-04-06","state_source":"nordicfrance",
        "uses":["nordic"]}"#;

    const FEE: &str = r#"{"fee_id":"nads","name":"Nordic Pass saison Alpes du Sud",
        "prices":[{"date":"2026-2027","label":"Adulte, plein tarif","price_eur":205.0}]}"#;

    fn value<'a>(c: &'a Card, label: &str) -> Option<&'a str> {
        c.facts
            .iter()
            .find(|f| f.label == label)
            .map(|f| f.value.as_str())
    }

    #[test]
    fn piste_card_has_every_fact_in_french() {
        let c = piste_card(
            SOULIERS.into(),
            Some("Cervières".into()),
            vec![FEE.into()],
            Lang::Fr,
        )
        .unwrap();
        assert_eq!(c.kind, "Piste rouge");
        assert_eq!(c.title, "Souliers");
        assert_eq!(value(&c, "État"), Some("fermée (au 2026-04-06)"));
        assert_eq!(value(&c, "Difficulté"), Some("moyenne"));
        assert_eq!(value(&c, "Technique"), Some("classique et skating"));
        assert_eq!(value(&c, "Longueur du tronçon"), Some("323 m"));
        assert_eq!(value(&c, "Sens"), Some("dans les deux sens"));
        assert_eq!(value(&c, "Itinéraires"), Some("Souliers, Izoard"));
        assert_eq!(value(&c, "Domaine"), Some("Cervières"));
        assert_eq!(
            value(&c, "Redevance"),
            Some("Nordic Pass saison Alpes du Sud : 205 € (Adulte, plein tarif, 2026-2027)")
        );
        assert_eq!(c.updated.as_deref(), Some("2026-04-06"));
        assert_eq!(c.source, "OpenStreetMap (ODbL) ; nordicfrance");
    }

    #[test]
    fn piste_card_in_english_and_unknown_state() {
        let c = piste_card(
            r#"{"difficulty":"novice","classic":true,"skating":false,"oneway":true,
                "length_m":1234.0,"state":"unknown"}"#
                .into(),
            None,
            vec![],
            Lang::En,
        )
        .unwrap();
        assert_eq!(c.kind, "Green trail");
        assert_eq!(c.title, "Unnamed trail");
        assert_eq!(value(&c, "Status"), Some("unknown"));
        assert_eq!(value(&c, "Technique"), Some("classic"));
        assert_eq!(value(&c, "Section length"), Some("1.2 km"));
        assert_eq!(value(&c, "Direction"), Some("one way"));
        assert_eq!(value(&c, "Area"), None);
        assert!(piste_card("[]".into(), None, vec![], Lang::En).is_none());
    }

    #[test]
    fn estimated_piste_card_says_uncertain_and_links_the_plan() {
        let p: Map<String, Value> = serde_json::from_str(
            r#"{"name": "Le Gy", "difficulty": "novice", "certainty": "incertain",
                "method": "estimated_paths", "lengths_km_plan": [3.5], "departures": ["Chapelle-des-Bois"],
                "plan_url": "https://example.org/plan.pdf"}"#,
        )
        .unwrap();
        let c = area_card("pistes-estimees", &p, Lang::Fr).unwrap();
        assert_eq!(c.kind, "Piste verte");
        assert_eq!(c.title, "Le Gy");
        assert_eq!(
            value(&c, "Tracé"),
            Some("Tracé incertain, d'après le plan de la station")
        );
        assert_eq!(value(&c, "Longueur sur le plan"), Some("3,5 km"));
        assert_eq!(value(&c, "Départ"), Some("Chapelle-des-Bois"));
        assert_eq!(c.links.len(), 1);
        assert_eq!(c.links[0].label, "Voir le plan officiel");
        assert_eq!(c.links[0].url, "https://example.org/plan.pdf");
        assert_eq!(c.source, "Plan officiel de la station ; IGN BD TOPO");
        let e = area_card("pistes-estimees", &p, Lang::En).unwrap();
        assert_eq!(
            value(&e, "Line"),
            Some("Uncertain line, from the resort's trail map")
        );
        assert_eq!(e.links[0].label, "See the official trail map");
        // A piste of the operator's bulletin traced from a published route of its name.
        let g: Map<String, Value> = serde_json::from_str(
            r#"{"name": "Piéton le Val", "kind": "pedestrian", "certainty": "incertain",
                "method": "geotrek", "named_by": "nordicfrance", "length_km_announced": 6.0,
                "source_name": "Le Val en raquettes", "source_url": "https://geotrek.example/trek/1/"}"#,
        )
        .unwrap();
        let c = area_card("pistes-estimees", &g, Lang::Fr).unwrap();
        assert_eq!(c.kind, "Piste piétonne");
        assert_eq!(
            value(&c, "Tracé"),
            Some("Tracé incertain, d'après un itinéraire publié du même nom")
        );
        assert_eq!(value(&c, "Longueur annoncée"), Some("6,0 km"));
        assert_eq!(c.links[0].label, "Voir l'itinéraire repris");
        assert_eq!(
            c.source,
            "Bulletin de l'exploitant ; Geotrek (« Le Val en raquettes »)"
        );
        let r: Map<String, Value> = serde_json::from_str(
            r#"{"name": "Liaison le Bas", "kind": "liaison", "certainty": "incertain",
                "method": "estimated_route", "named_by": "nordicfrance"}"#,
        )
        .unwrap();
        let e = area_card("pistes-estimees", &r, Lang::En).unwrap();
        assert_eq!(e.kind, "Link");
        assert_eq!(
            value(&e, "Line"),
            Some("Uncertain line, routed on the IGN roads and paths")
        );
        assert_eq!(e.source, "Operator's bulletin ; IGN BD TOPO");
        // A piste of OSM in a domain with a plan links it too.
        let mut q = p.clone();
        q.remove("certainty");
        let c = piste_card(
            serde_json::to_string(&q).unwrap(),
            None,
            Vec::new(),
            Lang::Fr,
        )
        .unwrap();
        assert_eq!(c.links[0].url, "https://example.org/plan.pdf");
    }

    #[test]
    fn zone_cards() {
        let z: Map<String, Value> = serde_json::from_str(
            r#"{"kind":"appb","access":"routes_only","bivouac":"forbidden","name":"Mont Pelé",
                "url":"https://inpn.mnhn.fr/x","source":"inpn","designated":"1990-01-01"}"#,
        )
        .unwrap();
        let c = area_card("zones", &z, Lang::Fr).unwrap();
        assert_eq!(c.kind, "Arrêté de protection de biotope");
        assert_eq!(c.title, "Mont Pelé");
        assert_eq!(value(&c, "Ski"), Some("itinéraires balisés seulement"));
        assert_eq!(value(&c, "Bivouac"), Some("interdit"));
        assert_eq!(c.links[0].url, "https://inpn.mnhn.fr/x");
        let c = area_card("zones", &z, Lang::En).unwrap();
        assert_eq!(value(&c, "Skiing"), Some("marked routes only"));
        let n: Map<String, Value> = serde_json::from_str(
            r#"{"area_ha":2.25,"kind":"no_voice_network_any_operator","release":"2026_T2"}"#,
        )
        .unwrap();
        let c = area_card("no-network", &n, Lang::Fr).unwrap();
        assert_eq!(c.title, "Zone sans réseau");
        assert_eq!(value(&c, "Téléphone"), Some("aucun opérateur pour appeler"));
        let a: Map<String, Value> =
            serde_json::from_str(r#"{"risque":"niveau fort","commune":"MONTGENEVRE"}"#).unwrap();
        let c = area_card("avalanche-areas", &a, Lang::Fr).unwrap();
        assert_eq!(value(&c, "Risque"), Some("niveau fort"));
        assert!(area_card("pistes", &a, Lang::Fr).is_none());
    }
}
