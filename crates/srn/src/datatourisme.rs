//! DATAtourisme tours (Licence Ouverte 2.0, « paternité » = `hasBeenCreatedBy`).
//!
//! The daily `datatourisme-tour.csv` (data.gouv.fr) lists every tour as a point with its
//! linked-data URI. Tours whose name says SRN or ski touring are dereferenced
//! (`<URI>?format=jsonld`: themes, distance, ascent, representations), and the GPX the
//! record points to (`hasRepresentation` → `ebucore:hasRelatedResource` →
//! `ebucore:locator`, MIME type `GPX`) gives the itinerary. Snowshoe tours are only
//! counted (several hundred requests for attached-only itineraries).

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::LonLat;

use crate::error::{Result, SrnError};
use crate::route::{Activity, Hint, Provider, Route};

/// Downloader source of the records (1 request/s, 1000/h).
pub const SOURCE: &str = "datatourisme";
/// Downloader source of the GPX files.
pub const GPX_SOURCE: &str = "datatourisme-gpx";
/// Dataset page on data.gouv.fr (API), listing the daily CSV files.
pub const DATASET_API: &str = "https://www.data.gouv.fr/api/1/datasets/datatourisme-la-plateforme-nationale-des-donnees-touristiques/";
/// Title of the tour file in the dataset.
pub const TOUR_FILE: &str = "datatourisme-tour.csv";
/// Licence.
pub const LICENCE: &str = "Licence Ouverte 2.0 (Etalab), via DATAtourisme";
/// Cache lifetime of the records and GPX.
pub const MAX_AGE: Duration = Duration::from_secs(30 * 86_400);
const CORE: &str = "https://www.datatourisme.fr/ontology/core#";
const EBU: &str = "http://www.ebu.ch/metadata/ontologies/ebucore/ebucore#";
const THEME: &str = "https://www.datatourisme.fr/resource/core#";

/// A tour of the CSV.
#[derive(Debug, Clone, PartialEq)]
pub struct Tour {
    /// Name.
    pub name: String,
    /// Point of the tour.
    pub pos: Option<LonLat>,
    /// Postcode and commune (`25240#Mouthe`).
    pub commune: String,
    /// Producer (`Createur_de_la_donnee`).
    pub producer: String,
    /// Last update by the producer.
    pub updated: String,
    /// Linked-data URI.
    pub uri: String,
}

/// Class a tour name may have before its themes are read, `None` when not a candidate.
pub fn name_candidate(name: &str) -> Option<Activity> {
    let n = name.to_lowercase();
    let excluded = [
        "marche nordique",
        "vtt",
        "vélo",
        "velo",
        "cyclo",
        "équestre",
        "trail",
        "e-bike",
        "pédestre",
        "à pied",
        "gravel",
        "ski de fond",
        "domaine nordique",
        "site nordique",
        "foyer nordique",
        "centre nordique",
        "espace nordique",
        "pistes nordiques",
    ];
    if excluded.iter().any(|x| n.contains(x)) {
        return None;
    }
    if n.contains("nordique") {
        Some(Activity::Srn)
    } else if n.contains("raquette") {
        Some(Activity::Snowshoe)
    } else if n.contains("ski de rando") || n.contains("ski rando") || n.contains("skitour") {
        Some(Activity::SkiTouring)
    } else {
        None
    }
}

/// Final class from the name class and the record themes: SRN and ski touring need the
/// `SkiTouring` theme (« randonnée nordique » alone is Nordic walking in the
/// Dordogne, walking in the Vosges).
pub fn classify(name_class: Activity, themes: &[String]) -> Option<Activity> {
    let ski = themes.iter().any(|t| t == "SkiTouring");
    match name_class {
        Activity::Srn | Activity::SkiTouring if ski => Some(name_class),
        Activity::Snowshoe => Some(Activity::Snowshoe),
        _ => None,
    }
}

/// Parses the tour CSV.
pub fn parse_tours(body: &[u8]) -> Result<Vec<Tour>> {
    let mut r = csv::Reader::from_reader(body);
    let headers = r
        .headers()
        .map_err(|e| SrnError::format(SOURCE, e.to_string()))?
        .clone();
    let col = |name: &str| -> Result<usize> {
        headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| SrnError::format(SOURCE, format!("column {name} missing")))
    };
    let (n, lat, lon, cp, prod, date, uri) = (
        col("Nom_du_POI")?,
        col("Latitude")?,
        col("Longitude")?,
        col("Code_postal_et_commune")?,
        col("Createur_de_la_donnee")?,
        col("Date_de_mise_a_jour")?,
        col("URI_ID_du_POI")?,
    );
    let mut out = Vec::new();
    for rec in r.records() {
        let rec = rec.map_err(|e| SrnError::format(SOURCE, e.to_string()))?;
        let f = |i: usize| rec.get(i).unwrap_or_default().trim().to_owned();
        let pos = match (f(lon).parse::<f64>(), f(lat).parse::<f64>()) {
            (Ok(x), Ok(y)) => Some(LonLat::new(x, y)),
            _ => None,
        };
        out.push(Tour {
            name: f(n),
            pos,
            commune: f(cp),
            producer: f(prod),
            updated: f(date),
            uri: f(uri),
        });
    }
    Ok(out)
}

fn node<'a>(doc: &'a Value, id: &str) -> Option<&'a Value> {
    doc.as_array()?
        .iter()
        .find(|n| n.get("@id").and_then(Value::as_str) == Some(id))
}

fn ids(n: &Value, key: &str) -> Vec<String> {
    n.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("@id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn value_f64(n: &Value, key: &str) -> Option<f64> {
    n.get(key)?
        .as_array()?
        .first()?
        .get("@value")?
        .as_str()?
        .parse()
        .ok()
}

/// Polylines of a GPX file (tracks, then routes).
pub fn parse_gpx(text: &str) -> Result<Vec<Vec<LonLat>>> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut lines: Vec<Vec<LonLat>> = Vec::new();
    let mut cur: Vec<LonLat> = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|e| SrnError::format(GPX_SOURCE, e.to_string()))?
        {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "trkpt" | "rtept" => {
                    let (mut lat, mut lon) = (None, None);
                    for a in e.attributes().flatten() {
                        let v = a.value.trim().parse().ok();
                        match a.key.local_name().as_ref() {
                            "lat" => lat = v,
                            "lon" => lon = v,
                            _ => {}
                        }
                    }
                    if let (Some(lat), Some(lon)) = (lat, lon) {
                        cur.push(LonLat::new(lon, lat));
                    }
                }
                "trkseg" | "rte" => {
                    if cur.len() >= 2 {
                        lines.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
                _ => {}
            },
            Event::End(e) if matches!(e.local_name().as_ref(), "trkseg" | "rte") => {
                if cur.len() >= 2 {
                    lines.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
            _ => {}
        }
    }
    if cur.len() >= 2 {
        lines.push(cur);
    }
    Ok(lines)
}

/// What [`fetch`] found.
#[derive(Debug, Clone, Default)]
pub struct Fetch {
    /// Itineraries with a GPX.
    pub routes: Vec<Route>,
    /// The tour CSV as fetched.
    pub csv: Option<Fetched>,
    /// Tours in the CSV.
    pub tours: usize,
    /// Candidates by name class.
    pub candidates: BTreeMap<String, usize>,
    /// Dereferenced candidates without a GPX (points to complete).
    pub without_track: Vec<Hint>,
    /// Candidates whose themes reject them: name, URI, reason.
    pub rejected: Vec<(String, String, String)>,
}

/// Downloads the tour CSV, dereferences the SRN and ski touring candidates and reads
/// their GPX.
pub fn fetch(dl: &Downloader) -> Result<Fetch> {
    let ds: Value = dl
        .fetch(&Request::get("datagouv", DATASET_API).max_age(Duration::from_secs(86_400)))?
        .json()?;
    let url = ds
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|r| r.get("title").and_then(Value::as_str) == Some(TOUR_FILE))
        .and_then(|r| r.get("url").and_then(Value::as_str))
        .ok_or_else(|| SrnError::format("datagouv", format!("{TOUR_FILE} not in the dataset")))?
        .to_owned();
    let csv = dl.fetch(&Request::get("datagouv", &url).max_age(Duration::from_secs(7 * 86_400)))?;
    let tours = parse_tours(&csv.bytes()?)?;
    let mut out = Fetch {
        tours: tours.len(),
        ..Default::default()
    };
    for t in &tours {
        let Some(class) = name_candidate(&t.name) else {
            continue;
        };
        *out.candidates.entry(class.as_str().to_owned()).or_default() += 1;
        if class == Activity::Snowshoe || !t.uri.starts_with("https://data.datatourisme.fr/") {
            continue;
        }
        match tour_route(dl, t, class)? {
            Outcome::Route(r) => out.routes.push(*r),
            Outcome::NoTrack(activity, why) => {
                if let Some(pos) = t.pos {
                    out.without_track.push(Hint {
                        id: route_id(&t.uri),
                        name: t.name.clone(),
                        provider: Provider::Datatourisme,
                        url: t.uri.clone(),
                        activity,
                        pos,
                        reason: why,
                    });
                }
            }
            Outcome::Rejected(why) => out.rejected.push((t.name.clone(), t.uri.clone(), why)),
        }
    }
    out.csv = Some(csv);
    Ok(out)
}

fn jsonld(dl: &Downloader, uri: &str) -> Result<Value> {
    Ok(dl
        .fetch(&Request::get(SOURCE, &format!("{uri}?format=jsonld")).max_age(MAX_AGE))?
        .json()?)
}

/// Result of dereferencing one candidate.
enum Outcome {
    /// An itinerary.
    Route(Box<Route>),
    /// A ski tour without a usable track (class, reason).
    NoTrack(Activity, String),
    /// Not a ski tour after all.
    Rejected(String),
}

fn route_id(uri: &str) -> String {
    format!(
        "datatourisme:{}",
        uri.trim_start_matches("https://data.datatourisme.fr/")
    )
}

/// Itinerary of one tour, or why there is none.
fn tour_route(dl: &Downloader, t: &Tour, class: Activity) -> Result<Outcome> {
    let doc = jsonld(dl, &t.uri)?;
    let Some(rec) = node(&doc, &t.uri) else {
        return Ok(Outcome::Rejected("fiche sans nœud principal".into()));
    };
    let themes: Vec<String> = ids(rec, &format!("{CORE}hasTheme"))
        .into_iter()
        .filter_map(|i| i.strip_prefix(THEME).map(str::to_owned))
        .collect();
    let Some(activity) = classify(class, &themes) else {
        return Ok(Outcome::Rejected(format!("thèmes {themes:?} : pas du ski")));
    };
    let mut gpx = None;
    'reps: for rep in ids(rec, &format!("{CORE}hasRepresentation")) {
        let d = jsonld(dl, &rep)?;
        let Some(n) = node(&d, &rep) else { continue };
        for res in ids(n, &format!("{EBU}hasRelatedResource")) {
            let d = jsonld(dl, &res)?;
            let Some(r) = node(&d, &res) else { continue };
            let is_gpx = ids(r, &format!("{EBU}hasMimeType"))
                .iter()
                .any(|m| m.ends_with("#GPX"));
            let locator = r
                .get(format!("{EBU}locator"))
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|v| v.get("@value"))
                .and_then(Value::as_str);
            if let (true, Some(l)) = (is_gpx, locator) {
                gpx = Some(l.to_owned());
                break 'reps;
            }
        }
    }
    let Some(gpx) = gpx else {
        return Ok(Outcome::NoTrack(activity, "aucun GPX dans la fiche".into()));
    };
    let text = dl
        .fetch(&Request::get(GPX_SOURCE, &gpx).max_age(MAX_AGE))?
        .text()?;
    let lines = parse_gpx(&text)?;
    if lines.is_empty() {
        return Ok(Outcome::NoTrack(activity, format!("GPX vide : {gpx}")));
    }
    let length_km = Route::lines_length_km(&lines);
    let shape = if ids(rec, &format!("{CORE}hasTourType"))
        .iter()
        .any(|x| x.ends_with("#OpenJaw"))
    {
        crate::route::Shape::Traverse
    } else {
        Route::shape_from_ends(&lines)
    };
    Ok(Outcome::Route(Box::new(Route {
        id: route_id(&t.uri),
        name: Some(t.name.clone()),
        provider: Provider::Datatourisme,
        publisher: t.producer.clone(),
        url: t.uri.clone(),
        licence: LICENCE.into(),
        attribution: format!(
            "{} via DATAtourisme, mise à jour du {}",
            t.producer, t.updated
        ),
        activity,
        shape,
        lines,
        length_km,
        published_length_km: value_f64(rec, &format!("{CORE}tourDistance")).map(|m| m / 1000.0),
        ascent_m: value_f64(rec, &format!("{CORE}positiveCumulDifference")),
        difficulty: None,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_and_themes() {
        let srn = "La Grande traversée du Jura à ski de randonnée nordique - GTJ à ski de randonnée nordique";
        assert_eq!(name_candidate(srn), Some(Activity::Srn));
        assert_eq!(name_candidate("Marche Nordique - Parcours vert"), None);
        assert_eq!(
            name_candidate("La Grande traversée du Jura à ski de fond - GTJ à ski de fond"),
            None
        );
        assert_eq!(name_candidate("Domaine Nordique du Meygal"), None);
        assert_eq!(
            name_candidate("Ski de randonnée : Circuit 1 Le Gaschney"),
            Some(Activity::SkiTouring)
        );
        assert_eq!(
            name_candidate("La Grande traversée du Jura à raquette - GTJ raquette"),
            Some(Activity::Snowshoe)
        );
        let ski = vec!["SkiTouring".to_owned(), "Sport".to_owned()];
        let walk = vec!["Pedestrian".to_owned()];
        assert_eq!(classify(Activity::Srn, &ski), Some(Activity::Srn));
        assert_eq!(classify(Activity::Srn, &walk), None);
        assert_eq!(classify(Activity::SkiTouring, &walk), None);
    }

    #[test]
    fn gpx_and_csv() {
        let gpx = r#"<?xml version="1.0"?><gpx xmlns="http://www.topografix.com/GPX/1/1"><trk><trkseg>
<trkpt lat="46.70679" lon="6.20926"><ele>941</ele></trkpt><trkpt lat="46.70624" lon="6.20824"/>
</trkseg><trkseg><trkpt lat="46.6" lon="6.1"/><trkpt lat="46.61" lon="6.1"/><trkpt lat="46.62" lon="6.1"/></trkseg></trk></gpx>"#;
        let l = parse_gpx(gpx).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0][0], LonLat::new(6.20926, 46.70679));
        assert_eq!(l[1].len(), 3);
        let csv = "Nom_du_POI,Categories_de_POI,Latitude,Longitude,Adresse_postale,Code_postal_et_commune,Covid19_mesures_specifiques,Createur_de_la_donnee,SIT_diffuseur,Date_de_mise_a_jour,Contacts_du_POI,Classements_du_POI,Description,URI_ID_du_POI\n\
GTJ,x,46.7061744,6.20833141,,25240#Mouthe,,JURA TOURISME,BFC,2024-01-19,,,,https://data.datatourisme.fr/6/80785b47\n";
        let t = parse_tours(csv.as_bytes()).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].producer, "JURA TOURISME");
        assert_eq!(t[0].pos, Some(LonLat::new(6.20833141, 46.7061744)));
        assert_eq!(t[0].uri, "https://data.datatourisme.fr/6/80785b47");
    }

    #[test]
    fn jsonld_nodes() {
        let doc: Value = serde_json::from_str(
            r#"[{"@id":"a","https://www.datatourisme.fr/ontology/core#tourDistance":[{"@value":"100000.0"}],
                "https://www.datatourisme.fr/ontology/core#hasTheme":[{"@id":"https://www.datatourisme.fr/resource/core#SkiTouring"}]}]"#,
        )
        .unwrap();
        let n = node(&doc, "a").unwrap();
        assert_eq!(
            value_f64(n, &format!("{CORE}tourDistance")),
            Some(100_000.0)
        );
        assert_eq!(ids(n, &format!("{CORE}hasTheme")).len(), 1);
    }
}
