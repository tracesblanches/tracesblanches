//! Points of interest of the stations from OpenStreetMap (ODbL), through the public
//! Overpass API: rescue posts and emergency equipment, pass sale points, tourist offices,
//! ski schools, car parks. One query per cluster of neighbouring domains (rate-limited by
//! the shared downloader, cached a day), then each point is attached to the domains whose
//! footprint is close enough for its category.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};
use skimap_refuges::privacy::{self, Phone};

use crate::domains::Domain;
use crate::error::{Result, StationsError};

/// Downloader source name.
pub const SOURCE: &str = "overpass";
/// Public Overpass endpoint.
pub const ENDPOINT: &str = "https://overpass-api.de/api/interpreter";
/// Licence.
pub const LICENCE: &str = "ODbL 1.0";
/// Attribution.
pub const ATTRIBUTION: &str = "© les contributeurs d'OpenStreetMap (ODbL)";
/// Margin around each domain footprint for the query, degrees (about 6 km).
pub const QUERY_MARGIN_DEG: f64 = 0.055;
/// Largest cluster box, degrees of latitude and longitude.
pub const CLUSTER_MAX_DEG: (f64, f64) = (0.7, 1.0);

/// Category of a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Rescue post, patrol post, mountain rescue, ambulance, defibrillator, emergency
    /// phone, rescue meeting point.
    Secours,
    /// Pass sale point: ticket desk, nordic lodge (« foyer »), ski-pass machine.
    Vente,
    /// Tourist office.
    OfficeTourisme,
    /// Ski school.
    EcoleSki,
    /// Car park.
    Parking,
}

impl Category {
    /// Largest distance to the footprint for a point to belong to a domain, m.
    pub fn reach_m(self, kind: &str) -> f64 {
        match (self, kind) {
            (Category::Secours, "mountain_rescue" | "ambulance_station" | "fire_station") => 3000.0,
            (Category::Secours, "defibrillator") => 500.0,
            (Category::Secours, "emergency_phone") => 200.0,
            (Category::Secours, "access_point") => 100.0,
            (Category::Secours, _) => 1000.0,
            (Category::Vente, _) => 1000.0,
            (Category::OfficeTourisme | Category::EcoleSki, _) => 6000.0,
            (Category::Parking, _) => 300.0,
        }
    }

    /// French label.
    pub fn label(self) -> &'static str {
        match self {
            Category::Secours => "secours",
            Category::Vente => "point de vente de la redevance",
            Category::OfficeTourisme => "office de tourisme",
            Category::EcoleSki => "école de ski",
            Category::Parking => "parking",
        }
    }
}

/// A point of interest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Poi {
    /// `node`, `way` or `relation`.
    pub osm_type: String,
    /// OSM id.
    pub osm_id: i64,
    /// Position (node, or centre).
    #[serde(skip)]
    pub pos: LonLat,
    /// Category.
    pub category: Category,
    /// Kind within the category: `mountain_rescue`, `ambulance_station`, `fire_station`
    /// (« centre de secours »), `first_aid`,
    /// `patrol_post`, `defibrillator`, `emergency_phone`, `access_point`; `ticket`,
    /// `foyer`, `vending`; `tourist_office`; `ski_school`; `parking`.
    pub kind: String,
    /// Name.
    pub name: Option<String>,
    /// Phones (`phone`, `contact:phone`, `emergency:phone`): establishments only (the
    /// categories above are all establishments or public equipment).
    pub phones: Vec<Phone>,
    /// Web site (`website`, `contact:website`, `url`).
    pub website: Option<String>,
    /// Opening hours (`opening_hours`).
    pub opening_hours: Option<String>,
    /// Operator.
    pub operator: Option<String>,
    /// Car park: capacity.
    pub capacity: Option<String>,
    /// Car park: `fee`.
    pub fee: Option<String>,
    /// Car park: `access`.
    pub access: Option<String>,
    /// Element page on openstreetmap.org.
    pub url: String,
    /// Domains it belongs to, with the distance to their footprint (m).
    pub domains: Vec<(String, f64)>,
}

fn tag<'a>(t: &'a BTreeMap<String, String>, k: &str) -> Option<&'a str> {
    t.get(k)
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn has_any(s: &str, words: &[&str]) -> bool {
    let l = s.to_lowercase();
    words.iter().any(|w| l.contains(w))
}

/// Category and kind of an element from its tags, or `None`.
pub fn classify(t: &BTreeMap<String, String>) -> Option<(Category, &'static str)> {
    let name = tag(t, "name").unwrap_or_default();
    let e = tag(t, "emergency");
    let a = tag(t, "amenity");
    match (e, a) {
        (Some("mountain_rescue"), _) | (_, Some("mountain_rescue")) => {
            return Some((Category::Secours, "mountain_rescue"));
        }
        (Some("ambulance_station"), _) => return Some((Category::Secours, "ambulance_station")),
        (_, Some("fire_station")) => return Some((Category::Secours, "fire_station")),
        (Some("first_aid" | "rescue_station"), _) | (_, Some("first_aid")) => {
            return Some((Category::Secours, "first_aid"));
        }
        (Some("defibrillator"), _) => return Some((Category::Secours, "defibrillator")),
        (Some("phone"), _) => return Some((Category::Secours, "emergency_phone")),
        (Some("access_point"), _) => return Some((Category::Secours, "access_point")),
        _ => {}
    }
    if tag(t, "healthcare") == Some("first_aid") {
        return Some((Category::Secours, "first_aid"));
    }
    if has_any(
        name,
        &[
            "poste de secours",
            "pc sécurité",
            "pc securite",
            "pisteur",
            "secours",
        ],
    ) {
        return Some((Category::Secours, "patrol_post"));
    }
    if tag(t, "shop") == Some("ticket") {
        return Some((Category::Vente, "ticket"));
    }
    if tag(t, "vending").is_some_and(|v| has_any(v, &["ski"])) {
        return Some((Category::Vente, "vending"));
    }
    let lodge = has_any(
        name,
        &[
            "foyer",
            "caisse",
            "billetterie",
            "point de vente",
            "accueil nordique",
        ],
    );
    if lodge
        && has_any(
            name,
            &[
                "nordique",
                "ski",
                "fond",
                "piste",
                "neige",
                "billetterie",
                "caisse",
            ],
        )
    {
        return Some((Category::Vente, "foyer"));
    }
    if tag(t, "tourism") == Some("information") && tag(t, "information") == Some("office") {
        return Some((Category::OfficeTourisme, "tourist_office"));
    }
    if a == Some("ski_school")
        || tag(t, "office") == Some("ski_school")
        || has_any(
            name,
            &[
                "esf ",
                "école du ski",
                "ecole du ski",
                "école de ski",
                "ecole de ski",
            ],
        )
        || name.starts_with("ESF")
    {
        return Some((Category::EcoleSki, "ski_school"));
    }
    if a == Some("parking") {
        if matches!(
            tag(t, "access"),
            Some("private" | "no" | "customers" | "permit")
        ) {
            return None;
        }
        return Some((Category::Parking, "parking"));
    }
    None
}

/// Overpass QL of one box.
pub fn query(b: &BBox) -> String {
    let bb = b.overpass();
    format!(
        "[out:json][timeout:240];(\
nwr[\"emergency\"~\"^(mountain_rescue|ambulance_station|first_aid|rescue_station|defibrillator|phone|access_point)$\"]({bb});\
nwr[\"amenity\"~\"^(mountain_rescue|first_aid|ski_school|parking)$\"]({bb});\
nwr[\"healthcare\"=\"first_aid\"]({bb});\
nwr[\"shop\"=\"ticket\"]({bb});\
nwr[\"vending\"~\"ski\"]({bb});\
nwr[\"tourism\"=\"information\"][\"information\"=\"office\"]({bb});\
nwr[\"office\"=\"ski_school\"]({bb});\
nwr[\"name\"~\"foyer|caisse|billetterie|point de vente|accueil nordique|secours|pisteur|^ESF|cole d[ue] ski\",i]({bb});\
);out center tags;"
    )
}

/// Boxes covering every domain plus [`QUERY_MARGIN_DEG`], neighbouring boxes merged
/// while the merged box stays within [`CLUSTER_MAX_DEG`]. Returns the boxes and, for
/// each, the domain indices it covers.
pub fn clusters(domains: &[Domain]) -> Vec<(BBox, Vec<usize>)> {
    clusters_with(domains, CLUSTER_MAX_DEG)
}

/// [`clusters`] with another largest box (degrees of latitude, longitude).
pub fn clusters_with(domains: &[Domain], max_deg: (f64, f64)) -> Vec<(BBox, Vec<usize>)> {
    let m = QUERY_MARGIN_DEG;
    let mut cl: Vec<(BBox, Vec<usize>)> = domains
        .iter()
        .enumerate()
        .map(|(i, d)| {
            (
                BBox::new(
                    d.bbox.south - m,
                    d.bbox.west - m * 1.4,
                    d.bbox.north + m,
                    d.bbox.east + m * 1.4,
                ),
                vec![i],
            )
        })
        .collect();
    let union = |a: &BBox, b: &BBox| {
        BBox::new(
            a.south.min(b.south),
            a.west.min(b.west),
            a.north.max(b.north),
            a.east.max(b.east),
        )
    };
    let fits = |b: &BBox| b.north - b.south <= max_deg.0 && b.east - b.west <= max_deg.1;
    let overlap = |a: &BBox, b: &BBox| {
        a.south <= b.north && b.south <= a.north && a.west <= b.east && b.west <= a.east
    };
    loop {
        let mut merged = false;
        'outer: for i in 0..cl.len() {
            for j in i + 1..cl.len() {
                let u = union(&cl[i].0, &cl[j].0);
                if overlap(&cl[i].0, &cl[j].0) && fits(&u) {
                    let (_, mut dj) = cl.remove(j);
                    cl[i].0 = u;
                    cl[i].1.append(&mut dj);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    for c in &mut cl {
        c.1.sort_unstable();
    }
    cl.sort_by(|a, b| a.1.cmp(&b.1));
    cl
}

/// Parses an Overpass answer into classified points (no domain yet).
pub fn parse(v: &Value) -> Result<Vec<Poi>> {
    let elements = v
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| StationsError::format(SOURCE, "no elements"))?;
    let mut out = Vec::new();
    for e in elements {
        let osm_type = e.get("type").and_then(Value::as_str).unwrap_or_default();
        let Some(id) = e.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let (lat, lon) = match (
            e.get("lat")
                .and_then(Value::as_f64)
                .or_else(|| e.pointer("/center/lat").and_then(Value::as_f64)),
            e.get("lon")
                .and_then(Value::as_f64)
                .or_else(|| e.pointer("/center/lon").and_then(Value::as_f64)),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let t: BTreeMap<String, String> = e
            .get("tags")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let Some((category, kind)) = classify(&t) else {
            continue;
        };
        let mut phones: Vec<Phone> = Vec::new();
        for k in ["phone", "contact:phone", "emergency:phone"] {
            if let Some(v) = tag(&t, k) {
                for p in privacy::structured_phone(v, kind, "openstreetmap") {
                    if !phones.iter().any(|x| x.e164 == p.e164) {
                        phones.push(p);
                    }
                }
            }
        }
        out.push(Poi {
            osm_type: osm_type.to_owned(),
            osm_id: id,
            pos: LonLat::new(lon, lat),
            category,
            kind: kind.to_owned(),
            name: tag(&t, "name").map(str::to_owned),
            phones,
            website: ["website", "contact:website", "url"]
                .iter()
                .find_map(|k| tag(&t, k))
                .map(str::to_owned),
            opening_hours: tag(&t, "opening_hours").map(str::to_owned),
            operator: tag(&t, "operator").map(str::to_owned),
            capacity: tag(&t, "capacity").map(str::to_owned),
            fee: tag(&t, "fee").map(str::to_owned),
            access: tag(&t, "access").map(str::to_owned),
            url: format!("https://www.openstreetmap.org/{osm_type}/{id}"),
            domains: Vec::new(),
        });
    }
    Ok(out)
}

/// Attaches each point to the domains within reach of its category; drops the points of
/// no domain. Tourist offices and ski schools: only the 2 nearest of each domain.
pub fn assign(pois: &mut Vec<Poi>, domains: &[Domain]) {
    for p in pois.iter_mut() {
        let reach = p.category.reach_m(&p.kind);
        p.domains = domains
            .iter()
            .filter(|d| d.near_bbox(p.pos, reach))
            .filter_map(|d| {
                let m = d.distance_m(p.pos);
                (m <= reach).then(|| (d.id.clone(), (m * 10.0).round() / 10.0))
            })
            .collect();
    }
    // Nearest 2 tourist offices / ski schools per domain.
    for cat in [Category::OfficeTourisme, Category::EcoleSki] {
        let mut by_domain: BTreeMap<String, Vec<(f64, usize)>> = BTreeMap::new();
        for (i, p) in pois.iter().enumerate().filter(|(_, p)| p.category == cat) {
            for (d, m) in &p.domains {
                by_domain.entry(d.clone()).or_default().push((*m, i));
            }
        }
        let mut keep: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (d, mut v) in by_domain {
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, i) in v.into_iter().take(2) {
                keep.entry(i).or_default().push(d.clone());
            }
        }
        for (i, p) in pois
            .iter_mut()
            .enumerate()
            .filter(|(_, p)| p.category == cat)
        {
            let k = keep.remove(&i).unwrap_or_default();
            p.domains.retain(|(d, _)| k.contains(d));
        }
    }
    pois.retain(|p| !p.domains.is_empty());
}

/// What [`fetch`] downloaded.
#[derive(Debug, Clone)]
pub struct Fetch {
    /// Points attached to at least one domain, sorted by OSM element.
    pub pois: Vec<Poi>,
    /// Overpass answers, one per cluster.
    pub answers: Vec<Fetched>,
    /// Number of clusters.
    pub clusters: usize,
}

/// Queries the points of every cluster and attaches them to the domains.
pub fn fetch(dl: &Downloader, domains: &[Domain]) -> Result<Fetch> {
    let cl = clusters(domains);
    let mut all: BTreeMap<(String, i64), Poi> = BTreeMap::new();
    let mut answers = Vec::with_capacity(cl.len());
    for (i, (b, ds)) in cl.iter().enumerate() {
        tracing::info!(
            cluster = i + 1,
            of = cl.len(),
            domains = ds.len(),
            "overpass"
        );
        let f = dl.fetch(&Request::post_form(
            SOURCE,
            ENDPOINT,
            &[("data", &query(b))],
        ))?;
        for p in parse(&f.json()?)? {
            all.entry((p.osm_type.clone(), p.osm_id)).or_insert(p);
        }
        answers.push(f);
    }
    let mut pois: Vec<Poi> = all.into_values().collect();
    assign(&mut pois, domains);
    Ok(Fetch {
        pois,
        answers,
        clusters: cl.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn classification() {
        assert_eq!(
            classify(&t(&[("emergency", "defibrillator")])),
            Some((Category::Secours, "defibrillator"))
        );
        assert_eq!(
            classify(&t(&[("amenity", "first_aid")])),
            Some((Category::Secours, "first_aid"))
        );
        assert_eq!(
            classify(&t(&[("name", "Poste de secours des pistes")]))
                .unwrap()
                .1,
            "patrol_post"
        );
        assert_eq!(
            classify(&t(&[("name", "Foyer de ski de fond")])),
            Some((Category::Vente, "foyer"))
        );
        // A village « foyer rural » is not a pass sale point.
        assert_eq!(
            classify(&t(&[
                ("name", "Foyer rural"),
                ("amenity", "community_centre")
            ])),
            None
        );
        assert_eq!(
            classify(&t(&[("tourism", "information"), ("information", "office")])),
            Some((Category::OfficeTourisme, "tourist_office"))
        );
        assert_eq!(
            classify(&t(&[("name", "ESF Chapelle-des-Bois")]))
                .unwrap()
                .0,
            Category::EcoleSki
        );
        assert_eq!(
            classify(&t(&[("amenity", "parking")])).unwrap().0,
            Category::Parking
        );
        assert_eq!(
            classify(&t(&[("amenity", "parking"), ("access", "private")])),
            None
        );
        assert_eq!(
            classify(&t(&[("tourism", "information"), ("information", "board")])),
            None
        );
    }

    #[test]
    fn query_text() {
        let q = query(&BBox::new(46.5, 6.0, 46.7, 6.2));
        assert!(
            q.contains("(46.5,6,46.7,6.2)") || q.contains("(46.5,6.0,46.7,6.2)"),
            "{q}"
        );
        assert!(q.ends_with("out center tags;"));
    }
}
