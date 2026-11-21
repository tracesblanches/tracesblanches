//! Builds the `zones` layer of a site (P9) and installs the rule file next to it (P10).
//!
//! Output, in `data/out/<site>/zones/`:
//! - `zones.geojson`: every zone clipped to the site box, WGS84, properties documented in
//!   [`crate::layer`];
//! - `rules.json`: the hand-entered rules ([`crate::rules`]), verbatim;
//! - `zones.meta.json`: sources, licences, dates, counts, and for each rule the zones of
//!   this site it applies to.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use geo::{BooleanOps, MultiPolygon};
use serde::Serialize;
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::error::{Error, Result};
use crate::geom::{area_ha, clip_to_bbox, geodesic_area_m2, union_all};
use crate::layer::{Zone, ZoneKind, ZoneLayer};
use crate::local::{SiteLocalRules, site_local_rules};
use crate::rules::{RULES_JSON, RuleSet};
use crate::sources::gpu::{Ac2Class, GpuZone};
use crate::sources::{RawZone, biodiv, gpu, inpn, jurisdiction, swiss};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{write_json, write_text};

/// Layer (directory) name.
pub const LAYER: &str = "zones";

/// Zones of this site a rule applies to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleResolution {
    /// Rule identifier.
    pub rule: String,
    /// Matching zone identifiers (empty: the rule does not apply anywhere in the site).
    pub zones: Vec<String>,
}

/// Summary of a build.
#[derive(Debug, Clone, Serialize)]
pub struct ZonesReport {
    /// Site identifier.
    pub site: String,
    /// Output directory.
    pub dir: PathBuf,
    /// Number of zones.
    pub zones: usize,
    /// Zones per kind.
    pub by_kind: BTreeMap<String, usize>,
    /// Rules and the zones they apply to.
    pub rules: Vec<RuleResolution>,
    /// National rules of the site and status of its local rules (G4).
    pub local_rules: SiteLocalRules,
    /// Wall-clock time of the build.
    pub elapsed: Duration,
}

fn clip(raw: RawZone, site: &Site) -> Option<Zone> {
    let c = clip_to_bbox(&raw.geometry, site.bbox)?;
    if geodesic_area_m2(&c.geometry) <= 0.0 {
        return None;
    }
    let mut props = raw.props;
    props.area_ha = area_ha(&c.geometry);
    props.clipped |= c.clipped;
    Some(Zone::new(props, c.geometry))
}

/// Merges zones sharing an id (several features of one protected area).
fn merge_duplicates(zones: Vec<Zone>) -> Vec<Zone> {
    let mut by_id: BTreeMap<String, Vec<Zone>> = BTreeMap::new();
    for z in zones {
        by_id.entry(z.id().to_owned()).or_default().push(z);
    }
    by_id
        .into_values()
        .map(|mut group| {
            if group.len() == 1 {
                return group.pop().expect("one element");
            }
            let n = group.len();
            let geometry = union_all(group.iter().map(|z| &z.geometry));
            let mut props = group.swap_remove(0).props;
            props.area_ha = area_ha(&geometry);
            props.note = Some(format!(
                "{}{n} entités de la source fusionnées",
                props.note.map(|n| format!("{n} ; ")).unwrap_or_default()
            ));
            Zone::new(props, geometry)
        })
        .collect()
}

/// Share of `a` covered by `b`.
fn covered_share(a: &MultiPolygon<f64>, b: &MultiPolygon<f64>) -> f64 {
    let area = geodesic_area_m2(a);
    if area <= 0.0 {
        return 0.0;
    }
    geodesic_area_m2(&a.intersection(b)) / area
}

/// Decides sites inscrits / classés for GPU AC2 easements against the INPN sites classés,
/// which list every site classé: an AC2 easement covering (or covered by) an INPN site
/// classé for more than half of its area is that site classé and is dropped (the INPN
/// zone records it in its note); the others are sites inscrits unless their name says
/// "Site Classé".
fn resolve_ac2(inpn: &mut [Zone], gpu: Vec<(Zone, Option<Ac2Class>)>) -> Vec<Zone> {
    let mut out = Vec::new();
    for (mut z, class) in gpu {
        let Some(class) = class else {
            out.push(z);
            continue;
        };
        let matched = inpn.iter_mut().find(|s| {
            s.kind() == ZoneKind::SiteClasse
                && (covered_share(&z.geometry, &s.geometry) > 0.5
                    || covered_share(&s.geometry, &z.geometry) > 0.5)
        });
        match (matched, class) {
            (Some(s), Ac2Class::Classe | Ac2Class::Unknown) => {
                let link = z.props.url.clone().unwrap_or_default();
                s.props.note = Some(format!(
                    "{}aussi servitude GPU AC2 {} {link}",
                    s.props
                        .note
                        .take()
                        .map(|n| format!("{n} ; "))
                        .unwrap_or_default(),
                    z.id()
                ));
                continue;
            }
            (Some(s), Ac2Class::Inscrit) => {
                tracing::warn!(
                    gpu = z.id(),
                    inpn = s.id(),
                    "site inscrit overlapping a site classé"
                );
                z.props.kind = ZoneKind::SiteInscrit;
            }
            (None, Ac2Class::Classe) => {
                z.props.kind = ZoneKind::SiteClasse;
                z.props.note = Some(
                    "servitude AC2 « Site Classé », absente de la couche INPN des sites classés"
                        .into(),
                );
            }
            (None, Ac2Class::Inscrit) => z.props.kind = ZoneKind::SiteInscrit,
            (None, Ac2Class::Unknown) => {
                z.props.kind = ZoneKind::SiteInscrit;
                z.props.note = Some(
                    "servitude AC2 sans mention inscrit/classé et absente de la couche INPN des sites classés : site inscrit"
                        .into(),
                );
            }
        }
        out.push(z);
    }
    out
}

/// Zone ids named by the rules.
fn referenced_ids(rules: &RuleSet) -> BTreeSet<String> {
    rules
        .rules
        .iter()
        .flat_map(|r| r.applies_to.iter().filter_map(|s| s.zone.clone()))
        .collect()
}

/// Fetches every source, assembles the layer of `site` and writes it; Biodiv'sports
/// only with `personal_sources` of the environment configuration
/// (`SKIMAP_PERSONAL_SOURCES`, as the command line). See [`build_with`].
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<ZonesReport> {
    let personal = skimap_common::config::Config::from_env()?.personal_sources;
    build_with(site, layout, dl, personal)
}

/// [`build`] with an explicit `personal_sources`. Biodiv'sports shows no licence
/// (docs/licences-donnees.md: « Usage personnel seulement, tant qu'il n'y a pas d'accord
/// écrit : […] Biodiv'sports (aucune licence trouvée) »): its zones are fetched only with
/// the option, and then its source is marked `personal_use_only`.
pub fn build_with(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<ZonesReport> {
    let t0 = Instant::now();
    let b = site.bbox;
    let rules = RuleSet::embedded()?;
    let referenced = referenced_ids(&rules);

    let clip_all = |raw: Vec<RawZone>| -> Vec<Zone> {
        raw.into_iter().filter_map(|r| clip(r, site)).collect()
    };

    let mut inpn_zones = merge_duplicates(clip_all(inpn::fetch(dl, b)?));
    let gpu_zones: Vec<(Zone, Option<Ac2Class>)> = gpu::fetch(dl, b)?
        .into_iter()
        .filter_map(|GpuZone { zone, ac2 }| clip(zone, site).map(|z| (z, ac2)))
        .collect();
    let gpu_zones = resolve_ac2(&mut inpn_zones, gpu_zones);
    let biodiv_zones = if personal_sources {
        merge_duplicates(clip_all(biodiv::fetch(dl, b)?))
    } else {
        Vec::new()
    };
    let district_zones = clip_all(swiss::districts(dl, b)?);
    let canton_zones = clip_all(swiss::cantons(dl, b)?);

    let fr = jurisdiction::french(dl, b)?;
    let mut juris = clip_all(vec![fr.country]);
    juris.extend(clip_all(fr.departments));
    juris.extend(clip_all(
        fr.communes
            .into_iter()
            .filter(|c| referenced.contains(&c.props.id))
            .collect(),
    ));
    for id in &referenced {
        if let Some(bfs) = id.strip_prefix("commune:ch-") {
            let bfs: u32 = bfs
                .parse()
                .map_err(|_| Error::Rules(format!("bad Swiss commune id {id}")))?;
            if let Some(c) = swiss::commune(dl, bfs)? {
                juris.extend(clip(c, site));
            }
        }
    }

    let mut zones: Vec<Zone> = Vec::new();
    zones.extend(inpn_zones);
    zones.extend(merge_duplicates(gpu_zones));
    zones.extend(biodiv_zones);
    zones.extend(district_zones);
    zones.extend(canton_zones);
    zones.extend(juris);
    zones.sort_by(|a, b| (a.kind(), a.id()).cmp(&(b.kind(), b.id())));
    let layer = ZoneLayer { zones };
    layer.check_unique_ids()?;

    let resolution: Vec<RuleResolution> = rules
        .rules
        .iter()
        .map(|r| RuleResolution {
            rule: r.id.clone(),
            zones: r
                .matching_zones(&layer)
                .map(|z| z.id().to_owned())
                .collect(),
        })
        .collect();
    let mut by_kind = BTreeMap::new();
    for z in &layer.zones {
        *by_kind
            .entry(z.kind().as_str().to_owned())
            .or_insert(0usize) += 1;
    }

    let local_rules = site_local_rules(&layer, &rules);

    let dir = layout.layer_dir(site, LAYER)?;
    write_text(&dir.join("zones.geojson"), &layer.to_geojson_string()?)?;
    write_text(&dir.join("rules.json"), RULES_JSON)?;
    let meta = layer_meta(
        site,
        &layer,
        &by_kind,
        &resolution,
        &local_rules,
        personal_sources,
    );
    meta.write(&dir)?;

    let report = ZonesReport {
        site: site.id.to_owned(),
        dir,
        zones: layer.zones.len(),
        by_kind,
        rules: resolution,
        local_rules,
        elapsed: t0.elapsed(),
    };
    write_json(&report.dir.join("build_report.json"), &report)?;
    Ok(report)
}

fn layer_meta(
    site: &Site,
    layer: &ZoneLayer,
    by_kind: &BTreeMap<String, usize>,
    resolution: &[RuleResolution],
    local_rules: &SiteLocalRules,
    personal_sources: bool,
) -> LayerMeta {
    let b = site.bbox;
    let today = skimap_common::dates::utc_date(jiff::Timestamp::now());
    let props = [
        ("id", "identifiant stable <espace>:<identifiant source>"),
        (
            "kind",
            "type de zone (appb, site_inscrit, spr, biodiv_sports, district_franc, country, commune, ...)",
        ),
        ("name", "nom officiel"),
        ("source", "inpn, gpu, biodiv-sports, geo-admin, geo-api"),
        ("url", "fiche, acte ou page d'information"),
        (
            "months",
            "mois sensibles (1 à 12) des zones Biodiv'sports ; null = pas de saison dans la source",
        ),
        (
            "practices",
            "pratiques Biodiv'sports concernées (terrestre, vertical, aerien, ...)",
        ),
        ("designated", "date de création connue (ISO)"),
        (
            "area_ha",
            "surface géodésique dans l'emprise du site, hectares",
        ),
        ("clipped", "géométrie coupée par l'emprise du site"),
        ("note", "remarque du pipeline"),
    ];
    let mut meta = LayerMeta {
        tickets: vec!["P9 (#22)".into(), "P10 (#23)".into(), "G4 (#120)".into()],
        valid_until: Some((today + jiff::SignedDuration::from_hours(24 * 30)).to_string()),
        validity: Some("Régénérer au moins chaque mois : Biodiv'sports évolue sans calendrier, INPN est mis à jour chaque semestre, le GPU en continu. Les règles (rules.json) sont relues à la main (date `reviewed`).".into()),
        crs: Some("EPSG:4326 (longitude, latitude), coordonnées arrondies à 1e-6°".into()),
        files: vec![
            FileMeta::new("zones.geojson", "toutes les zones, découpées à l'emprise du site").count(layer.zones.len()).properties(props),
            FileMeta::new("rules.json", "règles saisies à la main depuis les textes (schéma skimap_zones::rules)"),
            FileMeta::new("build_report.json", "résumé de la construction"),
        ],
        sources: vec![
            SourceRef::new("INPN espaces protégés (PatriNat)", format!("{} couches {}", crate::sources::GEOPF_WFS, inpn::LAYERS.iter().map(|l| l.0).collect::<Vec<_>>().join(", ")), "Licence Ouverte 2.0 (data.gouv.fr : « INPN - Données du programme Natura 2000 » fr-lo, MNHN ; « Espaces naturels protégés » lov2, Système d'information sur la biodiversité ; « INPN - Données du programme Espaces Protégés » other-open, MNHN)", "PatriNat (OFB, MNHN, CNRS, IRD), Inventaire national du patrimoine naturel")
                .note("WFS Géoplateforme, sortie demandée en CRS:84 (la valeur par défaut est EPSG:3857)"),
            SourceRef::new("Géoportail de l'urbanisme, servitudes d'utilité publique", format!("{} couche {}", crate::sources::GEOPF_WFS, gpu::LAYER), "Licence Ouverte (CGU du Géoportail de l'urbanisme : « L'utilisation des données d'urbanisme est soumise aux conditions définies dans la licence ouverte »)", "Géoportail de l'urbanisme")
                .note("seule source des sites inscrits (R111-33) ; AC1, AC2, AC4, AS1, A7 conservées"),
            SourceRef::new("geo.admin.ch (OFEV districts francs, swissBOUNDARIES3D)", swiss::identify_url(swiss::LAYER_DISTRICTS, b), "opendata.swiss « terms_by » (attribution obligatoire)", "© OFEV, © swisstopo")
                .note("districts francs : parties à protection intégrale ou partielle seulement"),
            SourceRef::new("geo.api.gouv.fr, contours des communes", format!("{}/departements/<code>/communes?format=geojson&geometry=contour", jurisdiction::API), "ODbL 1.0 (jeu data.gouv.fr « Contours administratifs » servi par l'API, d'après Admin Express de l'IGN et OpenStreetMap)", "Contours administratifs (data.gouv.fr, ODbL) via geo.api.gouv.fr : IGN Admin Express, © contributeurs OpenStreetMap")
                .note("France, départements et communes citées par les règles"),
        ],
        notes: vec![
            if personal_sources {
                "Biodiv'sports : aucune licence trouvée, usage personnel (option personal_sources) ; couche marquée usage personnel.".into()
            } else {
                "Biodiv'sports écarté : aucune licence trouvée, usage personnel seulement tant que la LPO n'a pas répondu (option personal_sources désactivée).".into()
            },
            "Les zones sont découpées à l'emprise du site : hors emprise, aucune règle n'est évaluée.".into(),
            "Les sites inscrits viennent des servitudes AC2 du GPU qui ne recoupent aucun site classé INPN.".into(),
            "Contours des zones cœur de l'APPB Haut-Doubs : zone Biodiv'sports 686 (hors des deux emprises actuelles).".into(),
            "Italie : aucune donnée de zone ni règle.".into(),
            format!(
                "Règles nationales appliquées partout (stats.local_rules.national) : {}. Règles locales (stats.local_rules) : {} zone(s) dont l'acte propre a été lu, {} lue(s) en partie, {} non vérifiée(s) ; arrêtés municipaux et préfectoraux non recensés hors des règles citées.",
                local_rules.national.join(", "),
                local_rules.count(crate::local::LocalStatus::Read),
                local_rules.count(crate::local::LocalStatus::Partial),
                local_rules.count(crate::local::LocalStatus::Unverified),
            ),
            format!("{} zones ; aucune règle n'est inventée : chaque règle de rules.json cite ses sources et dit ce qui a été lu.", layer.zones.len()),
        ],
        stats: serde_json::json!({
            "zones_by_kind": by_kind,
            "rules": resolution,
            "local_rules": local_rules,
            "areas_ha": layer.zones.iter().map(|z| (z.id().to_owned(), z.props.area_ha)).collect::<BTreeMap<_, _>>(),
        }),
        ..LayerMeta::new(LAYER, site.id)
    };
    if personal_sources {
        meta.sources.insert(
            2,
            SourceRef::new("Biodiv'sports", biodiv::areas_url(b), "aucune licence trouvée : usage personnel (docs/licences-donnees.md)", "Biodiv'sports (LPO AuRA et partenaires)")
                .note("period=ignore : toutes les zones, quel que soit le mois ; mois sensibles dans `months`")
                .personal_use_only(),
        );
    }
    meta
}
