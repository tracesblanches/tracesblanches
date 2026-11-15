//! Sites: every cross-country ski area of the census (P3) and the SRN areas, each with a
//! stable identifier, a working box, its Météo-France massifs and its department.
//!
//! - [`Site`] is the value every pipeline crate receives. Its fields are `'static` (cheap
//!   to copy, shared between threads): the sites of a [`Registry`] are loaded once per
//!   process and kept for its lifetime.
//! - [`Registry`]: every site of France, `data/out/france/sites/sites.json`, written by
//!   `skimap domaines` from the census (`skimap-cli`, `france.rs`). Without that file, the
//!   registry holds the evaluation sites only.
//! - Site-specific data live in data tables, not in code: `data/sites.json` (working box,
//!   commune and verified massifs of the two evaluation sites, starts, archived day, plan
//!   requests, proof outings, status sources) and `data/status_sources.json` (trail status
//!   source of each census domain, research report 08).
//!
//! The v1 evaluation sites, Cervières (05) and Chapelle-des-Bois (25), are two census
//! domains like the others: same identifiers as before (`cervieres`,
//! `chapelle-des-bois`), their hand-picked working box kept by the table. Their massifs
//! were verified by P6 (#19) on the snowtools polygons (`UMR-CNRM/snowtools`,
//! `massifs.shp`) against the commune outlines (geo.api.gouv.fr): Queyras (17) covers 100 %
//! of Cervières; Haut-Doubs (55) covers 93.2 % of Chapelle-des-Bois and the village,
//! Haut-Jura (56) the Risoux side (6.0 %). The Jura has no BERA.
//!
//! Bounding boxes are WGS84 and may extend beyond France: crates that need French-only
//! data clip with commune or department outlines.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::crs;
use crate::error::{Error, Result};
use crate::geom::{BBox, LonLat};
use crate::layout::DataLayout;

/// A Météo-France massif (snowpack simulation, BERA).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Massif {
    /// Météo-France massif number.
    pub code: u16,
    /// Massif name.
    pub name: &'static str,
}

/// How sure we are of the massif attachment of a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MassifStatus {
    /// Verified on the snowtools massif polygons (UMR-CNRM/snowtools `massifs.shp`): the
    /// first massif covers most of the commune (evaluation sites, against the commune
    /// outline) or of the domain envelope (census domains, points every 250 m).
    Verified,
    /// Single massif, assumed from geography, not verified against Météo-France.
    Assumed,
    /// Several candidates, in order of preference; to be confirmed.
    ToConfirm,
    /// No snowtools massif covers the site (Ardennes, Morvan): no snowpack simulation, no
    /// BERA; [`Site::massifs`] is empty.
    Outside,
}

/// What kind of place a site is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SiteKind {
    /// Groomed cross-country ski area (census P3): trail status and grooming apply.
    #[default]
    NordicStation,
    /// Nordic ski touring (SRN) playground that is not a groomed station (Vercors high
    /// plateaus…): no trail status, no grooming.
    SrnArea,
    /// Long linear traverse (GTJ, GTV…): a corridor around an itinerary, built in
    /// sections ([`split_extent`]).
    Traverse,
}

impl SiteKind {
    /// French label.
    pub fn label_fr(self) -> &'static str {
        match self {
            Self::NordicStation => "station nordique damée",
            Self::SrnArea => "domaine de jeu SRN",
            Self::Traverse => "traversée",
        }
    }

    /// Whether trail status and grooming apply (groomed stations only).
    pub fn has_groomed_trails(self) -> bool {
        self == Self::NordicStation
    }
}

/// Trail status sources a site can be polled with (`skimap-status`, personal use only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct StatusSources {
    /// ENJ Snowmap city identifier (`cities=` filter of the Geotrek API).
    pub enj_city: Option<u32>,
    /// Nordic France white-label widget station identifier.
    pub nordicfrance_station: Option<u32>,
    /// Town hall « Info damage » page (format of the Cervières page).
    pub town_hall_page: Option<&'static str>,
}

impl StatusSources {
    /// Whether no source is known.
    pub fn is_empty(&self) -> bool {
        self.enj_city.is_none() && self.nordicfrance_station.is_none()
    }
}

/// A site.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Site {
    /// Stable identifier used in paths and on the command line ([`slug`] of the census
    /// name, see [`assign_ids`]).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// French department code.
    pub department: &'static str,
    /// INSEE code of the main commune (geo.api.gouv.fr).
    pub insee: &'static str,
    /// Centre of the main commune for the evaluation sites; representative point of the
    /// domain (on its trail network, census P3) for the other sites. WGS84.
    pub commune_center: LonLat,
    /// Working area, WGS84.
    pub bbox: BBox,
    /// Météo-France massifs covering the site, the main one first (empty when
    /// [`MassifStatus::Outside`]).
    pub massifs: &'static [Massif],
    /// Confidence in [`Site::massifs`].
    pub massif_status: MassifStatus,
    /// Whether a BERA (avalanche bulletin) exists for the main massif.
    pub has_bera: bool,
    /// Kind of site.
    pub kind: SiteKind,
    /// Site this one is a section of (long traverse or large area split by
    /// [`split_extent`]).
    pub parent: Option<&'static str>,
    /// Trail status sources.
    pub status: StatusSources,
}

impl Site {
    /// Preferred massif.
    ///
    /// # Panics
    /// When the site is outside every massif ([`MassifStatus::Outside`]); use
    /// [`Site::main_massif`] when that can happen.
    pub fn primary_massif(&self) -> Massif {
        self.massifs[0]
    }

    /// Preferred massif, if any.
    pub fn main_massif(&self) -> Option<Massif> {
        self.massifs.first().copied()
    }
}

// --- Owned records (JSON) -------------------------------------------------------------

/// A massif in the JSON tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MassifRecord {
    /// Météo-France massif number.
    pub code: u16,
    /// Massif name.
    pub name: String,
    /// Share of the site's sample points in this massif (census domains).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<f64>,
}

/// Trail status sources in the JSON tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StatusRecord {
    /// ENJ Snowmap city identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enj_city: Option<u32>,
    /// Nordic France widget station identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nordicfrance_station: Option<u32>,
    /// Town hall « Info damage » page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub town_hall_page: Option<String>,
}

/// A site as stored in `sites.json` (registry) and in the evaluation table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteRecord {
    /// Identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Kind.
    #[serde(default)]
    pub kind: SiteKind,
    /// Parent site of a section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Census identifier (`site-<relation>` or `net-<way>`, P3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub census_id: Option<String>,
    /// Department code.
    pub department: String,
    /// INSEE code of the main commune.
    pub insee: String,
    /// Name of the main commune.
    #[serde(default)]
    pub commune: String,
    /// See [`Site::commune_center`].
    pub commune_center: LonLat,
    /// Working box.
    pub bbox: BBox,
    /// Box of the domain envelope itself (census hull), without the working margin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_bbox: Option<BBox>,
    /// Massifs, main first.
    pub massifs: Vec<MassifRecord>,
    /// Massif confidence.
    pub massif_status: MassifStatus,
    /// How the massifs were obtained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub massif_note: Option<String>,
    /// BERA available for the main massif.
    pub has_bera: bool,
    /// Trail status sources usable by `skimap-status`.
    #[serde(default)]
    pub status: StatusRecord,
    /// Trail status source of research report 08 (any kind, usable or not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_source: Option<StatusSourceInfo>,
    /// Nordic km of the census domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub km_nordic: Option<f64>,
    /// Snowshoe / hiking-only km of the census domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub km_hike_only: Option<f64>,
    /// Sections of a site too large to build in one piece (built instead of it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<String>,
    /// Local data (evaluation table only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalData>,
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

impl SiteRecord {
    /// The [`Site`] of this record. Strings are leaked: call once per site and process
    /// ([`Registry`] does).
    pub fn to_site(&self) -> Site {
        let massifs: Vec<Massif> = self
            .massifs
            .iter()
            .map(|m| Massif {
                code: m.code,
                name: leak(&m.name),
            })
            .collect();
        Site {
            id: leak(&self.id),
            name: leak(&self.name),
            department: leak(&self.department),
            insee: leak(&self.insee),
            commune_center: self.commune_center,
            bbox: self.bbox,
            massifs: Box::leak(massifs.into_boxed_slice()),
            massif_status: self.massif_status,
            has_bera: self.has_bera,
            kind: self.kind,
            parent: self.parent.as_deref().map(leak),
            status: StatusSources {
                enj_city: self.status.enj_city,
                nordicfrance_station: self.status.nordicfrance_station,
                town_hall_page: self.status.town_hall_page.as_deref().map(leak),
            },
        }
    }

    /// Whether the record is built (a site split in sections is built through them).
    pub fn buildable(&self) -> bool {
        self.sections.is_empty()
    }

    /// Checks the invariants of a record.
    pub fn check(&self) -> Result<()> {
        let bad = |m: String| Err(Error::Config(format!("site `{}`: {m}", self.id)));
        if slug(&self.id) != self.id {
            return bad("identifier is not a slug".into());
        }
        let b = self.bbox;
        if !(b.south < b.north && b.west < b.east) {
            return bad(format!("empty box {}", b.overpass()));
        }
        if self.massifs.is_empty() != (self.massif_status == MassifStatus::Outside) {
            return bad("massifs empty if and only if outside".into());
        }
        Ok(())
    }
}

/// Trail status source of a census domain (research report 08, `08-sources-etat-pistes.csv`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSourceInfo {
    /// Kind: `nordicfrance_widget`, `enj`, `nordicfrance_page`,
    /// `nordicfrance_page_uncertain`, `pyreneige`, `none`.
    pub kind: String,
    /// Domain name in the report.
    #[serde(default)]
    pub name: String,
    /// URL of the source, or of the station site when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Nordic France widget station identifier, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nordicfrance_station: Option<u32>,
    /// Whether an authorisation is required (`oui`, `inconnu`).
    #[serde(default)]
    pub authorisation_required: String,
    /// How the attachment was established.
    #[serde(default)]
    pub evidence: String,
}

#[derive(Deserialize)]
struct StatusTable {
    domains: BTreeMap<String, StatusSourceInfo>,
}

static STATUS_TABLE: LazyLock<BTreeMap<String, StatusSourceInfo>> = LazyLock::new(|| {
    serde_json::from_str::<StatusTable>(include_str!("../data/status_sources.json"))
        .expect("data/status_sources.json")
        .domains
});

/// Trail status source of a census domain (research report 08), by census identifier.
pub fn status_source(census_id: &str) -> Option<&'static StatusSourceInfo> {
    STATUS_TABLE.get(census_id)
}

/// Every census domain of research report 08 (census identifier → source).
pub fn status_sources() -> &'static BTreeMap<String, StatusSourceInfo> {
    &STATUS_TABLE
}

// --- Local data -----------------------------------------------------------------------

/// A named start point of a site (forecast points, plan and suggestion starts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Start {
    /// Identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Name as said in a sentence (« Le Laus », « le départ des pistes de … »).
    pub phrase: String,
    /// Longitude.
    pub lon: f64,
    /// Latitude.
    pub lat: f64,
    /// Where the coordinates come from.
    pub provenance: String,
}

impl Start {
    /// Position.
    pub fn position(&self) -> LonLat {
        LonLat::new(self.lon, self.lat)
    }
}

/// Technique of a preset request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetStyle {
    /// Classic.
    Classic,
    /// Skating.
    Skating,
}

/// A preset day-plan request of a site (`skimap plan`, J1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanPreset {
    /// Label.
    pub label: String,
    /// Index in [`LocalData::starts`].
    pub start: usize,
    /// Fixed start time `HH:MM` (otherwise the best time from 8 h).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    /// Time budget, minutes.
    pub budget_min: f64,
    /// Technique.
    pub style: PresetStyle,
    /// Back before `HH:MM`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_before: Option<String>,
    /// Green and blue trails only.
    #[serde(default)]
    pub easy_only: bool,
}

/// A proof outing of the assistant (A6): a route file of the `routes` layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofOuting {
    /// Label.
    pub label: String,
    /// File in `routes/`.
    pub file: String,
    /// Start time `HH:MM`.
    pub start_time: String,
    /// Off-piste route.
    pub offpiste: bool,
}

/// Local data of a site (evaluation table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LocalData {
    /// Start points, main first.
    #[serde(default)]
    pub starts: Vec<Start>,
    /// Day archived in `assist/history/` for the day plans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_day: Option<NaiveDate>,
    /// Preset day-plan requests.
    #[serde(default)]
    pub plan_requests: Vec<PlanPreset>,
    /// Proof outings of the assistant.
    #[serde(default)]
    pub proof_outings: Vec<ProofOuting>,
}

/// Parses `HH:MM`.
pub fn parse_hhmm(s: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .map_err(|e| Error::Config(format!("time `{s}` (HH:MM): {e}")))
}

// --- Evaluation table -----------------------------------------------------------------

#[derive(Deserialize)]
struct SiteTable {
    sites: Vec<SiteRecord>,
}

static EVALUATION_RECORDS: LazyLock<Vec<SiteRecord>> = LazyLock::new(|| {
    let t: SiteTable =
        serde_json::from_str(include_str!("../data/sites.json")).expect("data/sites.json");
    for r in &t.sites {
        r.check().expect("data/sites.json");
    }
    t.sites
});

/// Evaluation sites of the v1 proof of concept, from `data/sites.json`: the sites the
/// tests and examples compare with the research reports.
pub static SITES: LazyLock<Vec<Site>> =
    LazyLock::new(|| EVALUATION_RECORDS.iter().map(SiteRecord::to_site).collect());

/// Cervières (Hautes-Alpes), from `data/sites.json`.
pub static CERVIERES: LazyLock<Site> = LazyLock::new(|| evaluation("cervieres"));

/// Chapelle-des-Bois (Doubs), from `data/sites.json`.
pub static CHAPELLE_DES_BOIS: LazyLock<Site> = LazyLock::new(|| evaluation("chapelle-des-bois"));

fn evaluation(id: &str) -> Site {
    *SITES
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("data/sites.json has no site `{id}`"))
}

/// Records of the evaluation table (overrides of the census values).
pub fn evaluation_records() -> &'static [SiteRecord] {
    &EVALUATION_RECORDS
}

/// Evaluation site by identifier (the registry has every site: [`Registry::get`]).
pub fn site(id: &str) -> Result<&'static Site> {
    SITES
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| Error::UnknownSite(id.to_owned()))
}

/// Local data of a site (starts, archived day, plan requests, proof outings), if the
/// table has some.
pub fn local(site_id: &str) -> Option<&'static LocalData> {
    EVALUATION_RECORDS
        .iter()
        .find(|r| r.id == site_id)
        .and_then(|r| r.local.as_ref())
}

/// Start points of a site: those of the table, or else one at the site centre (the
/// representative point of a census domain, on its network).
pub fn starts(site: &Site) -> Vec<Start> {
    match local(site.id) {
        Some(l) if !l.starts.is_empty() => l.starts.clone(),
        _ => vec![Start {
            id: "centre".into(),
            name: format!("Centre du domaine {}", site.name),
            phrase: format!("le centre du domaine {}", site.name),
            lon: site.commune_center.lon,
            lat: site.commune_center.lat,
            provenance: "point représentatif du domaine (recensement P3 : point du réseau le plus proche du barycentre)".into(),
        }],
    }
}

// --- Registry -------------------------------------------------------------------------

/// France-wide layer of the registry, `data/out/france/sites/`.
pub const REGISTRY_LAYER: &str = "sites";
/// Registry file name.
pub const REGISTRY_FILE: &str = "sites.json";

/// The registry file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryFile {
    /// Generation instant (RFC 3339).
    pub generated_at: String,
    /// How the records were made.
    pub notes: Vec<String>,
    /// Every site, buildable or split in sections.
    pub sites: Vec<SiteRecord>,
}

/// Every site of France.
#[derive(Debug)]
pub struct Registry {
    records: Vec<SiteRecord>,
    sites: Vec<&'static Site>,
    path: Option<PathBuf>,
}

impl Registry {
    /// Path of the registry file.
    pub fn path(layout: &DataLayout) -> Result<PathBuf> {
        Ok(layout.france_layer_dir(REGISTRY_LAYER)?.join(REGISTRY_FILE))
    }

    /// Registry of records (checked; identifiers unique).
    pub fn from_records(records: Vec<SiteRecord>) -> Result<Self> {
        let mut seen = BTreeSet::new();
        for r in &records {
            r.check()?;
            if !seen.insert(r.id.as_str()) {
                return Err(Error::Config(format!("duplicate site `{}`", r.id)));
            }
        }
        let sites = records
            .iter()
            .map(|r| &*Box::leak(Box::new(r.to_site())))
            .collect();
        Ok(Self {
            records,
            sites,
            path: None,
        })
    }

    /// Registry of the evaluation sites only.
    pub fn evaluation() -> Self {
        Self {
            records: EVALUATION_RECORDS.clone(),
            sites: SITES.iter().collect(),
            path: None,
        }
    }

    /// Reads `data/out/france/sites/sites.json`.
    pub fn read(path: &Path) -> Result<Self> {
        let body = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        let file: RegistryFile = serde_json::from_slice(&body)
            .map_err(|e| Error::json(path.display().to_string(), e))?;
        let mut r = Self::from_records(file.sites)?;
        r.path = Some(path.to_owned());
        Ok(r)
    }

    /// The registry of the data directory, or the evaluation sites when the census has not
    /// been run (`skimap domaines`).
    pub fn load(layout: &DataLayout) -> Result<Self> {
        let path = Self::path(layout)?;
        if path.is_file() {
            Self::read(&path)
        } else {
            Ok(Self::evaluation())
        }
    }

    /// File the registry was read from (`None`: evaluation sites).
    pub fn source(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Site by identifier.
    pub fn get(&self, id: &str) -> Result<&'static Site> {
        self.sites
            .iter()
            .find(|s| s.id == id)
            .copied()
            .ok_or_else(|| Error::UnknownSite(id.to_owned()))
    }

    /// Record by identifier.
    pub fn record(&self, id: &str) -> Option<&SiteRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    /// Every record, in registry order.
    pub fn records(&self) -> &[SiteRecord] {
        &self.records
    }

    /// Every site (including the parents of sections).
    pub fn all(&self) -> &[&'static Site] {
        &self.sites
    }

    /// Sites that are built: every site but the parents of sections.
    pub fn buildable(&self) -> Vec<&'static Site> {
        self.records
            .iter()
            .zip(&self.sites)
            .filter(|(r, _)| r.buildable())
            .map(|(_, s)| *s)
            .collect()
    }

    /// Writes the registry file.
    pub fn write(path: &Path, file: &RegistryFile) -> Result<()> {
        crate::output::write_json(path, file)
    }
}

// --- Identifiers ----------------------------------------------------------------------

/// ASCII slug: lower case, accents removed, every other run of characters a single `-`
/// (« Chapelle-des-Bois » → `chapelle-des-bois`, « Alpe d'Huez » → `alpe-d-huez`).
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let base = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => {
                "a"
            }
            'æ' | 'Æ' => "ae",
            'ç' | 'Ç' => "c",
            'è' | 'é' | 'ê' | 'ë' | 'ė' | 'È' | 'É' | 'Ê' | 'Ë' | 'Ė' => "e",
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => "i",
            'ñ' | 'Ñ' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => "o",
            'œ' | 'Œ' => "oe",
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => "u",
            'ý' | 'ÿ' | 'Ý' => "y",
            c if c.is_ascii_alphanumeric() => {
                out.push(c.to_ascii_lowercase());
                continue;
            }
            _ => "-",
        };
        if base == "-" {
            if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push_str(base);
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// A census domain to name: census identifier, display name, nordic km.
#[derive(Debug, Clone, PartialEq)]
pub struct IdInput<'a> {
    /// Census identifier (`site-<relation>` or `net-<way>`).
    pub census_id: &'a str,
    /// Display name.
    pub name: &'a str,
    /// Nordic km.
    pub km: f64,
}

/// Stable site identifiers of census domains: the [`slug`] of the name; when several
/// domains share a slug, the one with the most nordic km keeps it and the others get
/// `-<digits of the census identifier>` (`les-rousses`, `les-rousses-478721195`). The
/// identifier changes only if the census name or identifier changes. `fixed` pins
/// identifiers by census identifier (evaluation table).
pub fn assign_ids(domains: &[IdInput<'_>], fixed: &BTreeMap<&str, &str>) -> Vec<String> {
    let mut by_slug: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, d) in domains.iter().enumerate() {
        let s = fixed
            .get(d.census_id)
            .map_or_else(|| slug(d.name), |f| (*f).to_owned());
        by_slug.entry(s).or_default().push(i);
    }
    let mut out = vec![String::new(); domains.len()];
    for (s, mut idx) in by_slug {
        let s = if s.is_empty() {
            "domaine".to_owned()
        } else {
            s
        };
        // A pinned identifier keeps the plain slug; otherwise the largest domain.
        idx.sort_by(|&a, &b| {
            let pa = fixed.contains_key(domains[a].census_id);
            let pb = fixed.contains_key(domains[b].census_id);
            pb.cmp(&pa)
                .then(domains[b].km.total_cmp(&domains[a].km))
                .then(domains[a].census_id.cmp(domains[b].census_id))
        });
        for (k, i) in idx.into_iter().enumerate() {
            out[i] = if k == 0 {
                s.clone()
            } else {
                let digits: String = domains[i]
                    .census_id
                    .chars()
                    .filter(char::is_ascii_digit)
                    .collect();
                format!("{s}-{digits}")
            };
        }
    }
    out
}

// --- Working boxes and sections -------------------------------------------------------

/// Margin around a census domain envelope, metres. A rare-scenario avalanche (Flow-Py
/// α = 18°, the « exposé » class) released 2 km away from the envelope must drop at least
/// 2000 × tan 18° ≈ 650 m to reach it; the SRN off-piste around the trails and the huts
/// near the domain fit in it too.
pub const DOMAIN_MARGIN_M: f64 = 2000.0;

/// Largest side of a site built in one piece, metres. The evaluation sites (16 × 17 km)
/// are built in one piece; every census domain with its margin is below 21 km. A larger
/// site (long traverse, large SRN area) is split into sections ([`split_extent`]).
pub const MAX_SITE_SIDE_M: f64 = 25_000.0;

/// Lambert-93 rectangle `(min_x, min_y, max_x, max_y)`.
pub type Rect93 = (f64, f64, f64, f64);

/// WGS84 box covering a Lambert-93 rectangle (edges densified).
pub fn bbox_of_lambert93(r: Rect93) -> Result<BBox> {
    let (x0, y0, x1, y1) = r;
    let n = 32;
    let mut b = BBox::new(
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let (x, y) = (x0 + t * (x1 - x0), y0 + t * (y1 - y0));
        for (px, py) in [(x, y0), (x, y1), (x0, y), (x1, y)] {
            let p = crs::from_lambert93(px, py)?;
            b.south = b.south.min(p.lat);
            b.north = b.north.max(p.lat);
            b.west = b.west.min(p.lon);
            b.east = b.east.max(p.lon);
        }
    }
    Ok(round_bbox(b))
}

/// Rounds a box outwards to 1e-5 degree (about a metre).
fn round_bbox(b: BBox) -> BBox {
    let k = 1e5;
    BBox::new(
        (b.south * k).floor() / k,
        (b.west * k).floor() / k,
        (b.north * k).ceil() / k,
        (b.east * k).ceil() / k,
    )
}

/// Lambert-93 rectangle of polygon rings given in WGS84 (`(lon, lat)`).
pub fn lambert93_extent(rings: &[Vec<(f64, f64)>]) -> Result<Rect93> {
    let mut r = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for ring in rings {
        for &(lon, lat) in ring {
            let (x, y) = crs::to_lambert93(LonLat::new(lon, lat))?;
            r = (r.0.min(x), r.1.min(y), r.2.max(x), r.3.max(y));
        }
    }
    if !r.0.is_finite() {
        return Err(Error::Config("empty envelope".into()));
    }
    Ok(r)
}

/// Grows a rectangle by `m` metres on every side.
pub fn grow(r: Rect93, m: f64) -> Rect93 {
    (r.0 - m, r.1 - m, r.2 + m, r.3 + m)
}

/// Clips a ring to a rectangle (Sutherland–Hodgman). For a concave ring the result may
/// have degenerate edges along the rectangle, but its vertices are those of the
/// intersection, so its bounding box is the intersection's.
fn clip_ring(ring: &[(f64, f64)], r: Rect93) -> Vec<(f64, f64)> {
    let (x0, y0, x1, y1) = r;
    let mut pts: Vec<(f64, f64)> = ring.to_vec();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    type Inside = fn((f64, f64), f64) -> bool;
    type Cut = fn((f64, f64), (f64, f64), f64) -> (f64, f64);
    let cut_x: Cut = |a, b, x| (x, a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0));
    let cut_y: Cut = |a, b, y| (a.0 + (b.0 - a.0) * (y - a.1) / (b.1 - a.1), y);
    let edges: [(Inside, Cut, f64); 4] = [
        (|p, v| p.0 >= v, cut_x, x0),
        (|p, v| p.0 <= v, cut_x, x1),
        (|p, v| p.1 >= v, cut_y, y0),
        (|p, v| p.1 <= v, cut_y, y1),
    ];
    for (inside, cut, v) in edges {
        if pts.is_empty() {
            break;
        }
        let input = std::mem::take(&mut pts);
        let n = input.len();
        for i in 0..n {
            let cur = input[i];
            let prev = input[(i + n - 1) % n];
            match (inside(cur, v), inside(prev, v)) {
                (true, true) => pts.push(cur),
                (true, false) => {
                    pts.push(cut(prev, cur, v));
                    pts.push(cur);
                }
                (false, true) => pts.push(cut(prev, cur, v)),
                (false, false) => {}
            }
        }
    }
    pts
}

/// Splits an envelope (polygon rings in Lambert-93, a corridor around a long itinerary or
/// a large area) into sections of at most `max_side` metres: a grid of square cells over
/// the envelope's rectangle, each cell kept if the envelope crosses it and shrunk to the
/// envelope's part inside, then grown by `overlap` metres so that neighbouring sections
/// share a strip (a route or an avalanche path cut at a cell edge is seen by both). An
/// envelope that fits in one cell gives one section, its own rectangle grown by `overlap`.
pub fn split_extent(rings: &[Vec<(f64, f64)>], max_side: f64, overlap: f64) -> Vec<Rect93> {
    let mut r = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in rings.iter().flatten() {
        r = (r.0.min(p.0), r.1.min(p.1), r.2.max(p.0), r.3.max(p.1));
    }
    if !r.0.is_finite() {
        return Vec::new();
    }
    let side = (max_side - 2.0 * overlap).max(max_side / 2.0);
    let nx = ((r.2 - r.0) / side).ceil().max(1.0) as usize;
    let ny = ((r.3 - r.1) / side).ceil().max(1.0) as usize;
    let mut out = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let cell = (
                r.0 + i as f64 * side,
                r.1 + j as f64 * side,
                (r.0 + (i + 1) as f64 * side).min(r.2),
                (r.1 + (j + 1) as f64 * side).min(r.3),
            );
            let mut c = (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            );
            for ring in rings {
                for p in clip_ring(ring, cell) {
                    c = (c.0.min(p.0), c.1.min(p.1), c.2.max(p.0), c.3.max(p.1));
                }
            }
            if c.0.is_finite() && (c.2 - c.0) * (c.3 - c.1) > 0.0 {
                out.push(grow(c, overlap));
            }
        }
    }
    out
}

/// Buffer of a polyline (Lambert-93) as a polygon ring: the corridor of a long traverse,
/// `half_width` metres on each side (square caps; joins by offset vertices).
pub fn corridor(line: &[(f64, f64)], half_width: f64) -> Vec<(f64, f64)> {
    let n = line.len();
    if n < 2 {
        return line
            .first()
            .map(|&(x, y)| {
                let h = half_width;
                vec![
                    (x - h, y - h),
                    (x + h, y - h),
                    (x + h, y + h),
                    (x - h, y + h),
                ]
            })
            .unwrap_or_default();
    }
    let normal = |a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy).max(1e-9);
        (-dy / l, dx / l)
    };
    let dir = |a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy).max(1e-9);
        (dx / l, dy / l)
    };
    let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for k in 0..n {
        let (nx, ny) = if k == 0 {
            normal(line[0], line[1])
        } else if k == n - 1 {
            normal(line[n - 2], line[n - 1])
        } else {
            let a = normal(line[k - 1], line[k]);
            let b = normal(line[k], line[k + 1]);
            let (sx, sy) = (a.0 + b.0, a.1 + b.1);
            let l = sx.hypot(sy).max(1e-9);
            (sx / l, sy / l)
        };
        let mut p = line[k];
        if k == 0 {
            let d = dir(line[0], line[1]);
            p = (p.0 - d.0 * half_width, p.1 - d.1 * half_width);
        } else if k == n - 1 {
            let d = dir(line[n - 2], line[n - 1]);
            p = (p.0 + d.0 * half_width, p.1 + d.1 * half_width);
        }
        left.push((p.0 + nx * half_width, p.1 + ny * half_width));
        right.push((p.0 - nx * half_width, p.1 - ny * half_width));
    }
    right.reverse();
    left.extend(right);
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup() {
        assert_eq!(site("cervieres").unwrap().insee, "05027");
        assert_eq!(site("chapelle-des-bois").unwrap().primary_massif().code, 55);
        assert!(matches!(site("mouthe"), Err(Error::UnknownSite(_))));
    }

    #[test]
    fn evaluation_sites_from_the_table() {
        for s in SITES.iter() {
            assert!(
                s.bbox.south < s.bbox.north && s.bbox.west < s.bbox.east,
                "{}",
                s.id
            );
            assert!(s.bbox.contains(s.commune_center), "{}", s.id);
            assert!(!s.massifs.is_empty());
            assert_eq!(s.massif_status, MassifStatus::Verified, "{}", s.id);
            assert_eq!(s.kind, SiteKind::NordicStation);
        }
        let codes: Vec<_> = CHAPELLE_DES_BOIS.massifs.iter().map(|m| m.code).collect();
        assert_eq!(codes, [55, 56], "Haut-Doubs first (93.2 % of the commune)");
        let ids: Vec<_> = SITES.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["cervieres", "chapelle-des-bois"]);
        // Same values as the former constants.
        assert_eq!(CERVIERES.bbox, BBox::new(44.77, 6.66, 44.92, 6.86));
        assert_eq!(CERVIERES.commune_center, LonLat::new(6.7746, 44.8586));
        assert!(CERVIERES.has_bera && !CHAPELLE_DES_BOIS.has_bera);
        assert_eq!(CHAPELLE_DES_BOIS.bbox, BBox::new(46.54, 6.02, 46.68, 6.20));
        assert_eq!(CERVIERES.status.nordicfrance_station, Some(3167));
        assert_eq!(CHAPELLE_DES_BOIS.status.enj_city, Some(14));
        assert!(CERVIERES.status.town_hall_page.is_some());
        assert!(CHAPELLE_DES_BOIS.status.town_hall_page.is_none());
    }

    #[test]
    fn local_data() {
        let c = local("cervieres").unwrap();
        assert_eq!(c.starts[0].phrase, "Le Laus");
        assert_eq!(c.archived_day, NaiveDate::from_ymd_opt(2026, 1, 9));
        assert_eq!(c.plan_requests.len(), 2);
        assert!(c.plan_requests[1].easy_only);
        assert_eq!(c.proof_outings.len(), 2);
        let j = local("chapelle-des-bois").unwrap();
        assert_eq!(j.plan_requests[0].return_before.as_deref(), Some("16:00"));
        assert_eq!(
            parse_hhmm("14:30").unwrap(),
            NaiveTime::from_hms_opt(14, 30, 0).unwrap()
        );
        assert!(local("mouthe").is_none());
        // A site without local data starts at its centre.
        let mut s = *CERVIERES;
        s.id = "autre";
        s.name = "Autre";
        let st = starts(&s);
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].position(), s.commune_center);
        assert_eq!(st[0].phrase, "le centre du domaine Autre");
    }

    #[test]
    fn status_table_from_report_08() {
        assert_eq!(status_sources().len(), 219);
        let c = status_source("net-152532950").unwrap();
        assert_eq!(c.kind, "nordicfrance_widget");
        assert_eq!(c.nordicfrance_station, Some(3167));
        assert_eq!(status_source("site-13681086").unwrap().kind, "enj");
        let widgets = status_sources()
            .values()
            .filter(|s| s.nordicfrance_station.is_some())
            .count();
        assert_eq!(widgets, 72, "report 08: 72 domains with a widget id");
        let none = status_sources()
            .values()
            .filter(|s| s.kind == "none")
            .count();
        assert_eq!(none, 68, "report 08: 68 domains without a source");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Cervières"), "cervieres");
        assert_eq!(slug("Chapelle-des-Bois"), "chapelle-des-bois");
        assert_eq!(
            slug("Alpe d'Huez Grand Domaine"),
            "alpe-d-huez-grand-domaine"
        );
        assert_eq!(slug("Foncine / Chaux-Neuve"), "foncine-chaux-neuve");
        assert_eq!(slug("  Ratėry 1700 "), "ratery-1700");
        assert_eq!(slug("Œuf & Cœur"), "oeuf-coeur");
        for s in ["cervieres", "a-b", "x1"] {
            assert_eq!(slug(s), s);
            crate::layout::checked_component(s).unwrap();
        }
    }

    #[test]
    fn ids_are_unique_and_stable() {
        let d = [
            IdInput {
                census_id: "net-478721195",
                name: "Les Rousses",
                km: 4.63,
            },
            IdInput {
                census_id: "site-3161169",
                name: "Les Rousses",
                km: 32.16,
            },
            IdInput {
                census_id: "net-152532950",
                name: "Cervières",
                km: 34.7,
            },
            IdInput {
                census_id: "net-1",
                name: "Cervieres",
                km: 50.0,
            },
        ];
        let fixed = BTreeMap::from([("net-152532950", "cervieres")]);
        let ids = assign_ids(&d, &fixed);
        assert_eq!(
            ids,
            [
                "les-rousses-478721195",
                "les-rousses",
                "cervieres",
                "cervieres-1"
            ]
        );
        // Order of the input does not matter.
        let mut r = d.to_vec();
        r.reverse();
        let mut ids2 = assign_ids(&r, &fixed);
        ids2.reverse();
        assert_eq!(ids, ids2);
    }

    #[test]
    fn registry_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path());
        // Without the file: the evaluation sites.
        let r = Registry::load(&layout).unwrap();
        assert!(r.source().is_none());
        assert_eq!(r.get("cervieres").unwrap().insee, "05027");
        let mut recs = evaluation_records().to_vec();
        let mut other = recs[0].clone();
        other.id = "gtj".into();
        other.kind = SiteKind::Traverse;
        other.sections = vec!["gtj-1".into()];
        let mut sec = other.clone();
        sec.id = "gtj-1".into();
        sec.parent = Some("gtj".into());
        sec.sections.clear();
        recs.extend([other, sec]);
        let path = Registry::path(&layout).unwrap();
        DataLayout::ensure(path.parent().unwrap()).unwrap();
        Registry::write(
            &path,
            &RegistryFile {
                generated_at: "2026-11-15T00:00:00Z".into(),
                notes: vec![],
                sites: recs,
            },
        )
        .unwrap();
        let r = Registry::load(&layout).unwrap();
        assert_eq!(r.source(), Some(path.as_path()));
        assert_eq!(r.all().len(), 4);
        let built: Vec<_> = r.buildable().iter().map(|s| s.id).collect();
        assert_eq!(built, ["cervieres", "chapelle-des-bois", "gtj-1"]);
        assert_eq!(r.get("gtj-1").unwrap().parent, Some("gtj"));
        assert_eq!(*r.get("cervieres").unwrap(), *CERVIERES);
        // Duplicates and bad identifiers are refused.
        let mut dup = evaluation_records().to_vec();
        dup.push(dup[0].clone());
        assert!(Registry::from_records(dup).is_err());
        let mut bad = evaluation_records()[0].clone();
        bad.id = "Cervières".into();
        assert!(Registry::from_records(vec![bad]).is_err());
    }

    #[test]
    fn working_box_of_an_envelope() {
        // A 1 km square around Cervières grown by the 2 km margin: 5 km a side, a bit more
        // in WGS84 because the Lambert-93 grid is rotated by the meridian convergence
        // (about 2.7° at 6.8° E: 5000 × (cos + sin) ≈ 5.2 km).
        let (x, y) = crs::to_lambert93(LonLat::new(6.77, 44.86)).unwrap();
        let b = bbox_of_lambert93(grow(
            (x - 500.0, y - 500.0, x + 500.0, y + 500.0),
            DOMAIN_MARGIN_M,
        ))
        .unwrap();
        let (w, h) = b.size_m();
        assert!((4_950.0..5_300.0).contains(&w), "{w}");
        assert!((4_950.0..5_300.0).contains(&h), "{h}");
        assert!(b.contains(LonLat::new(6.77, 44.86)));
    }

    #[test]
    fn small_envelope_is_one_section() {
        let ring = vec![
            (0.0, 0.0),
            (10_000.0, 0.0),
            (10_000.0, 8_000.0),
            (0.0, 8_000.0),
        ];
        let s = split_extent(&[ring], MAX_SITE_SIDE_M, 1_000.0);
        assert_eq!(s, vec![(-1_000.0, -1_000.0, 11_000.0, 9_000.0)]);
    }

    #[test]
    fn long_diagonal_corridor_is_split_along_the_line() {
        // A 150 km diagonal itinerary (like the GTJ, south-west to north-east), 3 km
        // corridor: its box is 106 × 106 km, the sections stay along the line.
        let line: Vec<(f64, f64)> = (0..=150)
            .map(|k| {
                let t = f64::from(k) * 1000.0 / std::f64::consts::SQRT_2;
                (t, t)
            })
            .collect();
        let ring = corridor(&line, 1_500.0);
        let s = split_extent(std::slice::from_ref(&ring), MAX_SITE_SIDE_M, 1_000.0);
        let area: f64 = s.iter().map(|r| (r.2 - r.0) * (r.3 - r.1)).sum();
        let side_max = s
            .iter()
            .map(|r| (r.2 - r.0).max(r.3 - r.1))
            .fold(0.0, f64::max);
        assert!(side_max <= MAX_SITE_SIDE_M + 1e-6, "{side_max}");
        // The whole box would be 1.1e10 m²; the sections cover about a quarter of it (a
        // diagonal crosses its cells corner to corner).
        assert!(area < 0.3 * 106_000.0 * 106_000.0, "{area}");
        // Every point of the line is inside a section.
        for p in &line {
            assert!(
                s.iter()
                    .any(|r| p.0 >= r.0 && p.0 <= r.2 && p.1 >= r.1 && p.1 <= r.3),
                "{p:?}"
            );
        }
        assert!(s.len() >= 7, "{}", s.len());
    }

    #[test]
    fn corridor_contains_its_line() {
        let ring = corridor(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0)], 100.0);
        assert_eq!(ring.len(), 6);
        let xs: Vec<f64> = ring.iter().map(|p| p.0).collect();
        assert!(xs.iter().cloned().fold(f64::INFINITY, f64::min) <= -100.0 + 1e-9);
    }
}
