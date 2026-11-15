//! Nordic stations of France: practical cards (H1, #132), access titles and their
//! validity zones (H2, #133).
//!
//! Input: the national census of the domains (P3, `data/out/france/domaines/`).
//! Output, in `data/out/france/stations/` (WGS84):
//! - `stations.geojson`: one card per domain (rescue numbers, 112 first, and posts; pass
//!   sale points; station, tourist office and ski school contacts; car parks), from
//!   OpenStreetMap and OpenSkiMap (ODbL) and SkiRadar (attribution « skiradar.fr »);
//! - `pois.geojson`: the OSM points behind the cards, with their domains;
//! - with `personal_sources` only (the issuers display no reuse licence): `titres.json`
//!   (titles, dated and sourced prices, members, check of every source), `titres.geojson`
//!   (footprint of each title), `couverture.json` (titles valid on each domain, ways of
//!   each title);
//! - `stations.meta.json`.
//!
//! Pipeline side: uses `skimap_common::download`.

pub mod catalogue;
pub mod contacts;
pub mod domains;
pub mod error;
pub mod fiche;
pub mod pois;
pub mod titres;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Value, json};
use skimap_common::download::Downloader;
use skimap_common::layout::{DataLayout, FRANCE};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{feature, point, round7, write_geojson, write_json};

pub use error::{Result, StationsError};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
/// Layer name.
pub const LAYER: &str = "stations";
/// Census layer read.
pub const CENSUS_LAYER: &str = "domaines";
/// Files written only with `personal_sources`.
pub const PERSONAL_FILES: &[&str] = &["titres.json", "titres.geojson", "couverture.json"];

/// Result of [`build`].
#[derive(Debug, Clone)]
pub struct StationsOutput {
    /// Layer directory.
    pub dir: PathBuf,
    /// Key figures (also in the meta file).
    pub stats: Value,
}

fn count<'a>(it: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut m = BTreeMap::new();
    for k in it {
        *m.entry(k).or_default() += 1;
    }
    m
}

fn multipolygon(ds: &[&domains::Domain]) -> Value {
    let polys: Vec<Value> = ds
        .iter()
        .flat_map(|d| d.polygons.iter())
        .map(|rings| {
            Value::Array(
                rings
                    .iter()
                    .map(|r| {
                        Value::Array(
                            r.iter()
                                .map(|p| json!([round7(p.lon), round7(p.lat)]))
                                .collect(),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    json!({"type": "MultiPolygon", "coordinates": polys})
}

/// Builds the stations layer of France. `personal_sources`: also the titles, prices and
/// validity zones (sources without a reuse licence); without it those files are removed.
pub fn build(
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<StationsOutput> {
    let census_dir = layout.france_layer_dir(CENSUS_LAYER)?;
    let (domains, ways) = domains::load(&census_dir)?;
    let dir = layout.france_layer_dir(LAYER)?;

    let pf = pois::fetch(dl, &domains)?;
    let cf = contacts::fetch(dl, &domains, &census_dir)?;
    let cards: Vec<fiche::Fiche> = domains
        .iter()
        .map(|d| fiche::fiche(d, &pf.pois, cf.by_domain.get(&d.id)))
        .collect();
    let card_features: Vec<Value> = domains
        .iter()
        .zip(&cards)
        .map(|(d, c)| {
            feature(
                point(d.point),
                serde_json::to_value(c).unwrap_or(Value::Null),
            )
        })
        .collect();
    write_geojson(&dir.join("stations.geojson"), &card_features)?;
    let poi_features: Vec<Value> = pf
        .pois
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
            v["domains"] = Value::Array(
                p.domains
                    .iter()
                    .map(|(id, m)| json!({"id": id, "distance_m": m}))
                    .collect(),
            );
            feature(point(p.pos), v)
        })
        .collect();
    write_geojson(&dir.join("pois.geojson"), &poi_features)?;

    let n = cards.len();
    let with = |k: &str| {
        cards
            .iter()
            .filter(|c| c.has.iter().any(|h| h == k))
            .count()
    };
    let pct = |x: usize| (1000.0 * x as f64 / n.max(1) as f64).round() / 10.0;
    let mut stats = json!({
        "domains": n,
        "with_secours_post": with("secours"), "with_secours_post_pct": pct(with("secours")),
        "with_vente": with("vente"), "with_vente_pct": pct(with("vente")),
        "with_phone": with("telephone"), "with_phone_pct": pct(with("telephone")),
        "with_site": with("site"), "with_site_pct": pct(with("site")),
        "with_parking": with("parking"), "with_parking_pct": pct(with("parking")),
        "with_secours_vente": cards.iter().filter(|c| c.has.iter().any(|h| h == "secours") && c.has.iter().any(|h| h == "vente")).count(),
        "pois_by_category": count(pf.pois.iter().map(|p| p.category.label())),
        "secours_by_kind": count(pf.pois.iter().filter(|p| p.category == pois::Category::Secours).map(|p| p.kind.as_str())),
        "vente_by_kind": count(pf.pois.iter().filter(|p| p.category == pois::Category::Vente).map(|p| p.kind.as_str())),
        "links_by_source": count(cards.iter().flat_map(|c| c.links.iter().map(|l| l.source.as_str()))),
        "overpass_clusters": pf.clusters,
    });

    let mut sources: Vec<SourceRef> = pf
        .answers
        .iter()
        .enumerate()
        .map(|(i, f)| {
            SourceRef::from_fetched(
                format!(
                    "OpenStreetMap (Overpass), points des stations, groupe {}",
                    i + 1
                ),
                pois::LICENCE,
                pois::ATTRIBUTION,
                f,
            )
        })
        .collect();
    for (name, lic, attr, f) in &cf.sources {
        sources.push(SourceRef::from_fetched(
            name.clone(),
            lic.clone(),
            attr.clone(),
            f,
        ));
    }
    sources.push(SourceRef::new(
        "Recensement des domaines (P3)",
        census_dir.join("domaines.geojson").display().to_string(),
        "ODbL 1.0 (OSM, OpenSkiMap) ; communes : Licence Ouverte 2.0",
        "© les contributeurs d'OpenStreetMap ; OpenSkiMap",
    ));

    let mut files = vec![
        FileMeta::new("stations.geojson", "fiche pratique de chaque domaine du recensement (point représentatif) : secours (112 en premier), points de vente de la redevance, contacts, parkings ; information seulement, pas d'achat")
            .licence("ODbL 1.0 (OSM, OpenSkiMap) ; SkiRadar : attribution « skiradar.fr » et lien vers la fiche")
            .count(card_features.len())
            .properties_json(&json!({
                "id": "identifiant du domaine (recensement P3)", "name": "nom", "departement": "département", "commune": "commune du point représentatif", "km_nordic": "km nordiques",
                "secours": "numbers : numéros à composer, 112 toujours en premier, puis 15 et 114 ; posts : postes et équipements OSM (patrol_post, first_aid, mountain_rescue, ambulance_station, fire_station, defibrillator, emergency_phone, access_point), postes tenus d'abord puis par distance",
                "vente": "points de vente de la redevance OSM (ticket : shop=ticket, foyer : foyer, caisse ou billetterie nordique, vending : distributeur) à moins de 1 km",
                "links": "liens : url, role (station, ecole_ski, skiradar), label, source (openstreetmap, openskimap, skiradar)",
                "phones": "téléphones de la station (relation OSM site=piste) : e164, display, label, source",
                "operator": "exploitant (relation OSM)",
                "offices_tourisme": "offices de tourisme OSM, les 2 plus proches à moins de 6 km", "ecoles_ski": "écoles de ski OSM, les 2 plus proches à moins de 6 km",
                "parkings": "parkings OSM à moins de 300 m (sauf privés), du plus proche au plus lointain",
                "has": "ce que la fiche contient : secours, vente, telephone, site, parking",
                "place": "kind, name, phones, website, opening_hours, distance_m (à l'emprise, 0 dedans), lon_lat, url (OSM)",
            })),
        FileMeta::new("pois.geojson", "points OSM des fiches (secours, vente, office de tourisme, école de ski, parking) et domaines auxquels ils sont rattachés")
            .licence(pois::LICENCE)
            .count(poi_features.len())
            .properties_json(&json!({
                "category": "secours, vente, office_tourisme, ecole_ski, parking", "kind": "sous-type (voir stations.geojson)",
                "name": "nom", "phones": "téléphones (phone, contact:phone, emergency:phone)", "website": "site", "opening_hours": "horaires",
                "operator": "exploitant", "capacity": "parking : places", "fee": "parking : payant", "access": "parking : accès",
                "url": "page OSM", "osm_type": "node, way, relation", "osm_id": "identifiant OSM",
                "domains": "domaines : id, distance_m à l'emprise",
            })),
    ];

    if personal_sources {
        let (checks, fetched) = titres::check_sources(dl);
        let checks_by: BTreeMap<String, titres::DocCheck> =
            checks.iter().map(|c| (c.id.clone(), c.clone())).collect();
        for (d, f) in &fetched {
            sources.push(
                SourceRef::from_fetched(
                    format!("{} : {}", d.publisher, d.title),
                    "aucune licence de réutilisation affichée : usage personnel",
                    d.publisher,
                    f,
                )
                .personal_use_only(),
            );
        }
        let covs = titres::coverages(&domains);
        let by_id: BTreeMap<&str, &domains::Domain> =
            domains.iter().map(|d| (d.id.as_str(), d)).collect();
        let mut domain_titres: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut titre_json = Vec::new();
        let mut titre_features = Vec::new();
        let mut ways_by_titre: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
        for (t, cov) in catalogue::TITRES.iter().zip(&covs) {
            let ds: Vec<&domains::Domain> = cov
                .domains
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect();
            let mut w: BTreeSet<i64> = BTreeSet::new();
            for d in &ds {
                domain_titres.entry(d.id.as_str()).or_default().push(t.id);
                w.extend(ways.get(&d.id).into_iter().flatten().copied());
            }
            ways_by_titre.insert(t.id, w.iter().copied().collect());
            titre_json.push(titres::titre_json(t, cov, w.len(), &checks_by));
            let full = t
                .prices
                .iter()
                .filter(|p| {
                    p.category == "adulte" && (p.tier == "plein_tarif" || p.tier == "tarif_unique")
                })
                .map(|p| p.eur)
                .next();
            titre_features.push(feature(
                multipolygon(&ds),
                json!({"id": t.id, "name": t.name, "scope": t.scope, "issuer": t.issuer,
                    "domains": ds.len(), "ways": w.len(), "adult_full_price_eur": full,
                    "season": t.prices.first().map(|p| p.season), "personal_use_only": true}),
            ));
        }
        let site_rule = |dep: &str| {
            catalogue::SITE_RULES
                .iter()
                .find(|r| r.depts.contains(&dep))
        };
        let couverture: BTreeMap<&str, Value> = domains
            .iter()
            .map(|d| {
                let ts = domain_titres.get(d.id.as_str()).cloned().unwrap_or_default();
                let sp: Vec<Value> = catalogue::SITE_PRICES
                    .iter()
                    .filter(|s| s.domains.contains(&d.id.as_str()))
                    .flat_map(|s| s.prices.iter().map(|p| titres::price_json(p, &checks_by)))
                    .collect();
                let rule = site_rule(&d.departement);
                (d.id.as_str(), json!({
                    "name": d.name, "departement": d.departement,
                    "titres": ts,
                    "site_title": {
                        "rule": rule.map(|r| r.rule),
                        "rule_source": rule.and_then(|r| catalogue::DOCS.iter().find(|x| x.id == r.doc)).map(|x| x.url),
                        "prices": sp,
                    },
                    "not_member_jura": catalogue::JURA_NOT_MEMBERS.contains(&d.id.as_str()),
                }))
            })
            .collect();
        write_json(
            &dir.join("titres.json"),
            &json!({
                "read_on": catalogue::READ_ON,
                "personal_use_only": true,
                "reuse": "aucun émetteur n'affiche de licence de réutilisation ; tarifs : faits, extraction limitée ; listes d'adhérents : base de données possiblement protégée (CPI L341-1, L342-1) ; usage personnel en attendant l'accord des émetteurs (courriers L1, L4)",
                "documents": catalogue::DOCS.iter().map(|d| json!({"id": d.id, "publisher": d.publisher, "title": d.title, "url": d.url, "format": d.format, "read_on": d.read_on, "check": checks_by.get(d.id)})).collect::<Vec<_>>(),
                "titres": titre_json,
                "site_rules": catalogue::SITE_RULES.iter().map(|r| json!({"departements": r.depts, "rule": r.rule, "source": catalogue::DOCS.iter().find(|x| x.id == r.doc).map(|x| x.url)})).collect::<Vec<_>>(),
            }),
        )?;
        write_geojson(&dir.join("titres.geojson"), &titre_features)?;
        write_json(
            &dir.join("couverture.json"),
            &json!({
                "personal_use_only": true,
                "domains": couverture,
                "ways": ways_by_titre,
            }),
        )?;
        // Share of the domains with a dated price (a title with prices, or site prices).
        let priced = |d: &domains::Domain| {
            domain_titres
                .get(d.id.as_str())
                .is_some_and(|ts| !ts.is_empty())
                || catalogue::SITE_PRICES
                    .iter()
                    .any(|s| s.domains.contains(&d.id.as_str()))
        };
        let n_priced = domains.iter().filter(|d| priced(d)).count();
        let n_day = domains
            .iter()
            .filter(|d| {
                catalogue::SITE_PRICES
                    .iter()
                    .any(|s| s.domains.contains(&d.id.as_str()))
            })
            .count();
        let all3 = domains
            .iter()
            .zip(&cards)
            .filter(|(d, c)| {
                priced(d)
                    && c.has.iter().any(|h| h == "secours")
                    && c.has.iter().any(|h| h == "vente")
            })
            .count();
        stats["personal"] = json!({
            "with_dated_price": n_priced, "with_dated_price_pct": pct(n_priced),
            "with_day_price": n_day,
            "with_secours_vente_and_price": all3, "with_secours_vente_and_price_pct": pct(all3),
            "domains_by_titre": catalogue::TITRES.iter().zip(&covs).map(|(t, c)| (t.id, c.domains.len())).collect::<BTreeMap<_, _>>(),
            "members_without_domain": catalogue::TITRES.iter().zip(&covs).map(|(t, c)| (t.id, c.members.iter().filter(|m| m.domains.is_empty()).map(|m| m.label.clone()).collect::<Vec<_>>())).collect::<BTreeMap<_, _>>(),
            "documents": checks.iter().map(|c| (c.id.as_str(), c.status.as_str())).collect::<BTreeMap<_, _>>(),
        });
        files.push(
            FileMeta::new("titres.json", "titres d'accès (redevances, Nordic Pass) : validité citée, prix datés et sourcés, sites membres et domaines du recensement, contrôle de chaque source")
                .licence("usage personnel (aucune licence de réutilisation affichée par les émetteurs)")
                .count(catalogue::TITRES.len())
                .properties_json(&json!({
                    "documents": "sources : id, publisher, title, url, format, read_on, check (status ok, quotes_missing, changed, unreachable ; missing ; sha256 ; fetched_at)",
                    "titres": "id, name, scope (national, massif, departemental, regroupement), issuer, products, validity (citation), validity_source, notes, abroad (validité hors recensement), prices, members (label, confidence verifie / probable / auto, domains), members_without_domain, domains, ways (nombre), personal_use_only",
                    "prices": "product (saison, hebdo, semaine, journee, 3h, saison_site), category (adulte, jeune, senior), ages, eur, tier (prevente, promo_flash, promo, plein_tarif, tarif_unique), from, until (jours inclus), season, source, publisher, read_on, quote, confirmed_now (citation retrouvée dans la source relue)",
                    "site_rules": "règles des titres de site (journée, séance) par département, citées",
                })),
        );
        files.push(
            FileMeta::new("titres.geojson", "emprise de chaque titre : réunion des emprises des domaines couverts (MultiPolygon)")
                .licence("usage personnel ; géométrie : ODbL 1.0 (recensement)")
                .count(catalogue::TITRES.len())
                .properties_json(&json!({"id": "titre", "name": "nom", "scope": "portée", "issuer": "émetteur", "domains": "domaines couverts", "ways": "ways OSM couverts", "adult_full_price_eur": "plein tarif adulte", "season": "saison", "personal_use_only": "true"})),
        );
        files.push(
            FileMeta::new("couverture.json", "titres valables sur chaque domaine et ways OSM de chaque titre, pour colorer « valable avec ton titre » / « autre redevance »")
                .licence("usage personnel")
                .count(domains.len())
                .properties_json(&json!({
                    "domains": "par domaine : name, departement, titres (identifiants de titres.json), site_title (rule : règle du titre de site citée, rule_source, prices : prix journée relevés), not_member_jura (domaine du Jura hors Espace Nordique Jurassien)",
                    "ways": "par titre : identifiants des ways OSM couverts (domaines_ways.json du recensement)",
                })),
        );
    } else {
        for f in PERSONAL_FILES {
            let p = dir.join(f);
            if p.exists() {
                std::fs::remove_file(&p).map_err(|e| skimap_common::Error::io(&p, e))?;
            }
        }
    }

    let meta = LayerMeta {
        tickets: vec!["H1 (#132)".to_owned(), "H2 (#133)".to_owned()],
        valid_from: Some(skimap_common::meta::now_rfc3339()),
        validity: Some("points OSM : état de l'extrait Overpass du jour ; tarifs : saison indiquée sur chaque prix".to_owned()),
        crs: Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned()),
        sources,
        files,
        notes: vec![
            "Le 112 est en tête de toute fiche de secours (puis 15 et 114).".to_owned(),
            "Information seulement : l'app n'achète rien ; un prix n'est publié qu'avec sa saison, sa source et sa date de lecture.".to_owned(),
            "Points de vente : seuls ceux que OSM décrit (shop=ticket, foyer, caisse ou billetterie nordique) ; les offices de tourisme et commerces qui vendent les pass saison ne sont pas tous connus.".to_owned(),
            "Titres, tarifs et zones de validité : seulement avec personal_sources (sources sans licence de réutilisation).".to_owned(),
        ],
        parameters: json!({
            "reach_m": {"secours (poste, premiers secours)": 1000, "secours de montagne, ambulance, centre de secours": 3000, "défibrillateur": 500, "téléphone d'urgence": 200, "point de rencontre des secours": 100, "vente": 1000, "office de tourisme, école de ski (2 plus proches)": 6000, "parking": 300},
            "overpass_margin_deg": pois::QUERY_MARGIN_DEG,
            "cluster_max_deg": [pois::CLUSTER_MAX_DEG.0, pois::CLUSTER_MAX_DEG.1],
            "personal_sources": personal_sources,
        }),
        stats: stats.clone(),
        ..LayerMeta::new(LAYER, FRANCE)
    };
    write_json(&dir.join(format!("{LAYER}.meta.json")), &meta)?;
    Ok(StationsOutput { dir, stats })
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
