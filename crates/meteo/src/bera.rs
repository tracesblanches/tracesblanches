//! BERA (bulletin d'estimation du risque d'avalanche) per massif, P8 (#21).
//!
//! Sources, in order of preference:
//! 1. **API « DonneesPubliquesBRA »** of the Météo-France portal (account and key required,
//!    `METEOFRANCE_API_KEY`): `GET {API_BASE}/massif/BRA?id-massif=<id>&format=xml` with the
//!    key in the `apikey` header. Without a key the portal answers 401 (verified on
//!    14/11/2026 for `/massif/BRA` and `/liste-massifs`; an unknown path answers 404).
//! 2. **Public archive** `donneespubliques.meteofrance.fr/donnees_libres/Pdf/BRA/`: a daily
//!    index `bra.YYYYMMDD.json` lists, per massif, the publication stamps; the bulletin is
//!    `BRA.<MASSIF>.<stamp>.pdf` and, until April 2023, also `.xml`. Since the 2023-24
//!    season the XML of the archive redirects to an "unavailable" page; the PDF stays. The
//!    current season was not in the archive on 14/11/2026 (index redirects).
//!
//! The XML format is described in « Descriptif technique des informations du bulletin
//! avalanche », version of 26/10/2023
//! (<https://donneespubliques.meteofrance.fr/client/document/descriptif_bulletinrisqueavalanche_formatxml_20231026_309.pdf>).
//! The parser also reads the older archive format (root `<Bulletins>`, fresh snow by sector
//! `SS241`/`SS242`, images embedded in base64, `TENDANCES`, `BSH/AVALANCHES`).
//!
//! The Jura has no BERA: for such sites the product is a notice with links to the
//! Météo-France mountain page and to the SLF bulletin, which covers the Swiss Jura (regions
//! CH-8111 Saint-Cergue, CH-8112 Vallée de Joux, CH-8211..8213, seen in the SLF CAAML API
//! for 15/02/2026).
//!
//! Output (`data/out/<site>/bera/`): `bera.json` ([`BeraFile`]), `bera.xml` (raw bulletin
//! when one was obtained) and `bera.meta.json`.

use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use skimap_common::config::Secret;
use skimap_common::download::{Downloader, Request};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::error::{Error, Result};
use crate::xml::{self, Element};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef, now_rfc3339};
use skimap_common::output::{write_bytes, write_json};

/// Base URL of the DonneesPubliquesBRA API.
pub const API_BASE: &str = "https://public-api.meteofrance.fr/public/DPBRA/v1";
/// Base URL of the public archive.
pub const ARCHIVE_BASE: &str = "https://donneespubliques.meteofrance.fr/donnees_libres/Pdf/BRA";
/// Downloader source of the API.
pub const SOURCE_API: &str = "meteofrance";
/// Downloader source of the public archive.
pub const SOURCE_ARCHIVE: &str = "meteofrance-archive";
/// Layer name.
pub const LAYER: &str = "bera";

/// Météo-France page of the avalanche bulletins of the southern Alps.
pub const MF_ALPES_DU_SUD_URL: &str =
    "https://meteofrance.com/meteo-montagne/alpes-du-sud/risques-avalanche";
/// Météo-France mountain page of the Jura (no BERA, weather and warnings only).
pub const MF_JURA_URL: &str = "https://meteofrance.com/meteo-montagne/jura";
/// Météo-France weather warnings of the Doubs.
pub const MF_VIGILANCE_DOUBS_URL: &str = "https://vigilance.meteofrance.fr/fr/doubs";
/// SLF avalanche bulletin (covers the Swiss Jura).
pub const SLF_BULLETIN_URL: &str = "https://www.slf.ch/fr/bulletin-davalanches/";
/// SLF bulletin in CAAML JSON (machine-readable, French).
pub const SLF_CAAML_URL: &str = "https://aws.slf.ch/api/bulletin/caaml/fr/json";

/// A BERA massif: identifier, name (annex 1 of the specification) and the upper-case name
/// used in the public archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BeraMassif {
    /// Météo-France massif identifier.
    pub id: u16,
    /// Name in annex 1 of the specification.
    pub name: &'static str,
    /// Name in the archive file names (`BRA.<archive_name>.<stamp>.xml`).
    pub archive_name: &'static str,
}

const fn m(id: u16, name: &'static str, archive_name: &'static str) -> BeraMassif {
    BeraMassif {
        id,
        name,
        archive_name,
    }
}

/// The 36 BERA massifs (annex 1 of the specification; archive names from the archive
/// index of 15/01/2025).
pub const MASSIFS: &[BeraMassif] = &[
    m(1, "Chablais", "CHABLAIS"),
    m(2, "Aravis", "ARAVIS"),
    m(3, "Mont-Blanc", "MONT-BLANC"),
    m(4, "Bauges", "BAUGES"),
    m(5, "Beaufortain", "BEAUFORTAIN"),
    m(6, "Haute-Tarentaise", "HAUTE-TARENTAISE"),
    m(7, "Chartreuse", "CHARTREUSE"),
    m(8, "Belledonne", "BELLEDONNE"),
    m(9, "Maurienne", "MAURIENNE"),
    m(10, "Vanoise", "VANOISE"),
    m(11, "Haute-Maurienne", "HAUTE-MAURIENNE"),
    m(12, "Grandes-Rousses", "GRANDES-ROUSSES"),
    m(13, "Thabor", "THABOR"),
    m(14, "Vercors", "VERCORS"),
    m(15, "Oisans", "OISANS"),
    m(16, "Pelvoux", "PELVOUX"),
    m(17, "Queyras", "QUEYRAS"),
    m(18, "Devoluy", "DEVOLUY"),
    m(19, "Champsaur", "CHAMPSAUR"),
    m(20, "Embrunais Parpaillon", "EMBRUNAIS-PARPAILLON"),
    m(21, "Ubaye", "UBAYE"),
    m(22, "Haut-Var Haut-Verdon", "HAUT-VAR-HAUT-VERDON"),
    m(23, "Mercantour", "MERCANTOUR"),
    m(40, "Cinto Rotondo", "CINTO-ROTONDO"),
    m(41, "Renoso Incudine", "RENOSO-INCUDINE"),
    m(64, "Pays-Basque", "PAYS-BASQUE"),
    m(65, "Aspe Ossau", "ASPE-OSSAU"),
    m(66, "Haute-Bigorre", "HAUTE-BIGORRE"),
    m(67, "Aure Louron", "AURE-LOURON"),
    m(68, "Luchonnais", "LUCHONNAIS"),
    m(69, "Couserans", "COUSERANS"),
    m(70, "Haute-Ariege", "HAUTE-ARIEGE"),
    m(71, "Andorre", "ANDORRE"),
    m(72, "Orlu St-Barthelemy", "ORLU-ST-BARTHELEMY"),
    m(73, "Capcir Puymorens", "CAPCIR-PUYMORENS"),
    m(74, "Cerdagne Canigou", "CERDAGNE-CANIGOU"),
];

/// Massif by identifier.
pub fn massif(id: u16) -> Option<&'static BeraMassif> {
    MASSIFS.iter().find(|m| m.id == id)
}

/// Label of a `TEMPSSENSIBLE` code (table of section 6 of the specification). Codes found
/// in real bulletins but absent from the table (e.g. 5, 10, 11) give `None`.
pub fn sky_label(code: i32) -> Option<&'static str> {
    Some(match code {
        0 => "clair",
        1 => "peu nuageux",
        2 => "nuageux",
        3 => "très nuageux",
        4 => "couvert",
        6 => "voilé",
        7 => "mer de nuages",
        32 => "brouillard",
        38 => "brouillard givrant",
        51 => "pluie faible",
        53 => "pluie forte",
        58 => "pluie et neige mêlées",
        59 => "pluie verglaçante",
        61 => "neige faible",
        63 => "neige forte",
        70 => "averses",
        71 => "rares averses",
        77 => "averses de pluie et neige mêlées",
        80 => "averses de neige",
        85 => "averses de grêle",
        90 => "orages",
        99 => "orages violents",
        _ => return None,
    })
}

/// Label of a risk level (European avalanche danger scale, French wording of the BERA).
pub fn risk_label(level: u8) -> Option<&'static str> {
    Some(match level {
        1 => "faible",
        2 => "limité",
        3 => "marqué",
        4 => "fort",
        5 => "très fort",
        _ => return None,
    })
}

/// Risk block (`CARTOUCHERISQUE/RISQUE`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Risk {
    /// Risk 1 to 5, lower band when the bulletin splits by altitude or area.
    pub risk1: Option<u8>,
    /// Time evolution of risk 1 (target level), if any.
    pub evolution1: Option<u8>,
    /// Localisation of risk 1 (`<2100`, `E`, `S`, empty).
    pub loc1: Option<String>,
    /// Altitude separating the two bands, metres.
    pub altitude: Option<i32>,
    /// Risk of the upper band or second area.
    pub risk2: Option<u8>,
    /// Time evolution of risk 2.
    pub evolution2: Option<u8>,
    /// Localisation of risk 2 (`>2100`, `O`, `N`, empty).
    pub loc2: Option<String>,
    /// Maximum risk of the day.
    pub max: Option<u8>,
    /// Text of the risk.
    pub comment: Option<String>,
    /// Maximum risk at J+2 (specification 2023).
    pub max_j2: Option<u8>,
    /// Date of the J+2 risk.
    pub date_j2: Option<String>,
}

/// Rose of the most dangerous aspects (`PENTE`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Aspects {
    /// North.
    pub n: bool,
    /// North-east.
    pub ne: bool,
    /// East.
    pub e: bool,
    /// South-east.
    pub se: bool,
    /// South.
    pub s: bool,
    /// South-west.
    pub sw: bool,
    /// West.
    pub w: bool,
    /// North-west.
    pub nw: bool,
    /// Text under the rose.
    pub comment: Option<String>,
}

/// Snow depth at one altitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnowLevel {
    /// Altitude, metres.
    pub altitude: i32,
    /// Depth on north slopes, cm.
    pub north_cm: Option<i32>,
    /// Depth on south slopes, cm.
    pub south_cm: Option<i32>,
}

/// Off-piste snow cover (`ENNEIGEMENT`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnowCover {
    /// Date.
    pub date: Option<String>,
    /// Altitude of continuous snow cover, south aspects, metres.
    pub limit_south_m: Option<i32>,
    /// Altitude of continuous snow cover, north aspects, metres.
    pub limit_north_m: Option<i32>,
    /// Depth per altitude.
    pub levels: Vec<SnowLevel>,
}

/// Fresh snow over 24 h (06 UTC to 06 UTC), one day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreshSnowDay {
    /// Date.
    pub date: Option<String>,
    /// Lower bound, cm (specification 2023: `SS24Min`).
    pub min_cm: Option<i32>,
    /// Upper bound, cm (specification 2023: `SS24Max`).
    pub max_cm: Option<i32>,
    /// Value of sector 1, cm (older format: `SS241`; negative values are codes of the
    /// producer, kept as is).
    pub sector1_cm: Option<i32>,
    /// Value of sector 2, cm (older format: `SS242`).
    pub sector2_cm: Option<i32>,
}

/// Fresh snow block (`NEIGEFRAICHE`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreshSnow {
    /// Altitude of the measure, metres.
    pub altitude_m: Option<i32>,
    /// Name of sector 1 (older format).
    pub sector1: Option<String>,
    /// Name of sector 2 (older format).
    pub sector2: Option<String>,
    /// Days.
    pub days: Vec<FreshSnowDay>,
}

/// One forecast step of the weather overview (`METEO/ECHEANCE`).
///
/// Values are kept as published; `-1` means "not applicable": no sea of clouds
/// (`sea_of_clouds_m`), dry weather (`rain_snow_limit_m`), missing wind or value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherStep {
    /// Date and time of the step.
    pub date: Option<String>,
    /// Wind speed at the low altitude, km/h.
    pub wind1_kmh: Option<i32>,
    /// Wind direction at the low altitude (8-point rose, `Vrb` or `_` variable).
    pub wind1_dir: Option<String>,
    /// Wind speed at the high altitude, km/h.
    pub wind2_kmh: Option<i32>,
    /// Wind direction at the high altitude.
    pub wind2_dir: Option<String>,
    /// Altitude of the 0 °C isotherm, metres.
    pub iso0_m: Option<i32>,
    /// Altitude of the −10 °C isotherm, metres (older format).
    pub iso_minus10_m: Option<i32>,
    /// Rain-snow limit, metres.
    pub rain_snow_limit_m: Option<i32>,
    /// Sky code (`TEMPSSENSIBLE`), see [`sky_label`].
    pub sky_code: Option<i32>,
    /// Label of the sky code when the specification lists it.
    pub sky_label: Option<String>,
    /// Top of the sea of clouds, metres.
    pub sea_of_clouds_m: Option<i32>,
}

/// Weather overview (`METEO`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    /// Altitude of the low wind, metres.
    pub wind1_altitude_m: Option<i32>,
    /// Altitude of the high wind, metres (9999 in the older format when absent).
    pub wind2_altitude_m: Option<i32>,
    /// Comment.
    pub comment: Option<String>,
    /// Steps.
    pub steps: Vec<WeatherStep>,
}

/// Trend of the following days (`TENDANCES/TENDANCE`, older format).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trend {
    /// Date.
    pub date: Option<String>,
    /// Value as published (0 stable; sign gives the direction).
    pub value: Option<i32>,
}

/// Maximum risk of a past day (`BSH/RISQUES/RISQUE`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PastRisk {
    /// Date.
    pub date: Option<String>,
    /// Maximum risk.
    pub max: Option<i32>,
}

/// Observed avalanche activity of a past day (`BSH/AVALANCHES/AVALANCHE`, older format).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PastAvalanches {
    /// Date.
    pub date: Option<String>,
    /// Spontaneous activity code as published (`-1` unknown).
    pub spontaneous: Option<i32>,
    /// Triggered activity code as published (`-1` unknown).
    pub triggered: Option<i32>,
}

/// Conditions of the past seven days (`BSH`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PastDays {
    /// Weather steps (4 or 3 a day).
    pub weather: Vec<WeatherStep>,
    /// Snow cover per day.
    pub snow_cover: Vec<SnowCover>,
    /// Fresh snow per day.
    pub fresh_snow: Option<FreshSnow>,
    /// Maximum risk per day.
    pub risks: Vec<PastRisk>,
    /// Avalanche activity per day.
    pub avalanches: Vec<PastAvalanches>,
}

/// One parsed bulletin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bulletin {
    /// Massif identifier (`ID`).
    pub massif_id: u16,
    /// Massif name as written in the bulletin (`MASSIF`).
    pub massif: String,
    /// Producing centre (`PRODUCTEUR`, older format).
    pub producer: Option<String>,
    /// Writing time (`DATEBULLETIN`), local time as published.
    pub date_bulletin: Option<String>,
    /// Expiry (`DATEECHEANCE`).
    pub date_expiry: Option<String>,
    /// Validity (`DATEVALIDITE`, or `<DateValidite>`).
    pub date_validity: Option<String>,
    /// Diffusion time (`DATEDIFFUSION`).
    pub date_diffusion: Option<String>,
    /// Amended bulletin.
    pub amendment: bool,
    /// Estimated risk.
    pub risk: Risk,
    /// Most dangerous aspects.
    pub aspects: Aspects,
    /// Triggering by skiers (`ACCIDENTEL`).
    pub accidental: Option<String>,
    /// Spontaneous avalanches (`NATUREL`).
    pub natural: Option<String>,
    /// Summary (`RESUME`).
    pub summary: Option<String>,
    /// Label of the J+2 maximum risk (`RisqueJ2`).
    pub risk_j2_label: Option<String>,
    /// J+2 trend text (`CommentaireRisqueJ2`).
    pub risk_j2_comment: Option<String>,
    /// Special notice (`AVIS`: empty, « fort risque d'avalanche »...).
    pub notice: Option<String>,
    /// Weather warning text (`VIGILANCE`).
    pub vigilance: Option<String>,
    /// Typical avalanche situations (`SitAvalTyp` SAT1, SAT2), 1 to 6.
    pub typical_situations: Vec<u8>,
    /// Snowpack stability text.
    pub stability: Option<String>,
    /// Snow quality text.
    pub snow_quality: Option<String>,
    /// Off-piste snow cover.
    pub snow_cover: Option<SnowCover>,
    /// Fresh snow.
    pub fresh_snow: Option<FreshSnow>,
    /// Weather overview.
    pub weather: Option<Weather>,
    /// Trends of the following days.
    pub trends: Vec<Trend>,
    /// Past seven days.
    pub past: Option<PastDays>,
    /// Names of the illustration images referenced by the bulletin.
    pub images: Vec<String>,
}

fn text_of(e: Option<&Element>) -> Option<String> {
    let t = e?.full_text();
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

fn attr_str(e: &Element, k: &str) -> Option<String> {
    e.attr(k)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn attr_i32(e: &Element, k: &str) -> Option<i32> {
    e.attr(k).and_then(|s| s.trim().parse().ok())
}

fn attr_u8(e: &Element, k: &str) -> Option<u8> {
    e.attr(k).and_then(|s| s.trim().parse().ok())
}

fn attr_bool(e: &Element, k: &str) -> bool {
    e.attr(k)
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("true"))
}

fn parse_snow_cover(e: &Element) -> SnowCover {
    SnowCover {
        date: attr_str(e, "DATE"),
        limit_south_m: attr_i32(e, "LimiteSud"),
        limit_north_m: attr_i32(e, "LimiteNord"),
        levels: e
            .children_named("NIVEAU")
            .filter_map(|n| {
                Some(SnowLevel {
                    altitude: attr_i32(n, "ALTI")?,
                    north_cm: attr_i32(n, "N"),
                    south_cm: attr_i32(n, "S"),
                })
            })
            .collect(),
    }
}

fn parse_fresh_snow(e: &Element) -> FreshSnow {
    FreshSnow {
        altitude_m: attr_i32(e, "ALTITUDESS"),
        sector1: attr_str(e, "SECTEURSS1"),
        sector2: attr_str(e, "SECTEURSS2"),
        days: e
            .children_named("NEIGE24H")
            .map(|d| FreshSnowDay {
                date: attr_str(d, "DATE"),
                min_cm: attr_i32(d, "SS24Min"),
                max_cm: attr_i32(d, "SS24Max"),
                sector1_cm: attr_i32(d, "SS241"),
                sector2_cm: attr_i32(d, "SS242"),
            })
            .collect(),
    }
}

fn parse_steps(e: &Element) -> Vec<WeatherStep> {
    e.children_named("ECHEANCE")
        .map(|s| {
            let sky = attr_i32(s, "TEMPSSENSIBLE");
            WeatherStep {
                date: attr_str(s, "DATE"),
                wind1_kmh: attr_i32(s, "FF1"),
                wind1_dir: attr_str(s, "DD1"),
                wind2_kmh: attr_i32(s, "FF2"),
                wind2_dir: attr_str(s, "DD2"),
                iso0_m: attr_i32(s, "ISO0"),
                iso_minus10_m: attr_i32(s, "ISO-10"),
                rain_snow_limit_m: attr_i32(s, "PLUIENEIGE"),
                sky_code: sky,
                sky_label: sky.and_then(sky_label).map(str::to_owned),
                sea_of_clouds_m: attr_i32(s, "MERNUAGES"),
            }
        })
        .collect()
}

fn parse_weather(e: &Element) -> Weather {
    Weather {
        wind1_altitude_m: attr_i32(e, "ALTITUDEVENT1"),
        wind2_altitude_m: attr_i32(e, "ALTITUDEVENT2"),
        comment: text_of(e.child("COMMENTAIRE")),
        steps: parse_steps(e),
    }
}

fn collect_image_names(e: &Element, out: &mut Vec<String>) {
    if e.name.starts_with("Image") {
        let t = e.text.trim();
        if !t.is_empty() {
            // Specification 2023: the element holds the file name.
            out.push(t.to_owned());
        } else if e.child("Content").is_some() {
            // Older format: the image is embedded; keep the element name.
            out.push(format!("{} (embedded)", e.name));
        }
    }
    for c in &e.children {
        collect_image_names(c, out);
    }
}

fn parse_bulletin(b: &Element) -> Result<Bulletin> {
    let ctx = || "BERA XML".to_owned();
    let massif_id: u16 = b
        .attr("ID")
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| Error::format(ctx(), "missing or invalid ID attribute"))?;
    let massif =
        attr_str(b, "MASSIF").ok_or_else(|| Error::format(ctx(), "missing MASSIF attribute"))?;
    if b.attr("TYPEBULLETIN").is_some_and(|t| t != "BRA") {
        return Err(Error::format(ctx(), "TYPEBULLETIN is not BRA"));
    }
    let cart = b.child("CARTOUCHERISQUE");
    let risk = cart
        .and_then(|c| c.child("RISQUE"))
        .map(|r| Risk {
            risk1: attr_u8(r, "RISQUE1"),
            evolution1: attr_u8(r, "EVOLURISQUE1"),
            loc1: attr_str(r, "LOC1"),
            altitude: attr_i32(r, "ALTITUDE"),
            risk2: attr_u8(r, "RISQUE2"),
            evolution2: attr_u8(r, "EVOLURISQUE2"),
            loc2: attr_str(r, "LOC2"),
            max: attr_u8(r, "RISQUEMAXI"),
            comment: attr_str(r, "COMMENTAIRE"),
            max_j2: attr_u8(r, "RISQUEMAXIJ2"),
            date_j2: attr_str(r, "DATE_RISQUE_J2"),
        })
        .unwrap_or_default();
    let aspects = cart
        .and_then(|c| c.child("PENTE"))
        .map(|p| Aspects {
            n: attr_bool(p, "N"),
            ne: attr_bool(p, "NE"),
            e: attr_bool(p, "E"),
            se: attr_bool(p, "SE"),
            s: attr_bool(p, "S"),
            sw: attr_bool(p, "SW"),
            w: attr_bool(p, "W"),
            nw: attr_bool(p, "NW"),
            comment: attr_str(p, "COMMENTAIRE"),
        })
        .unwrap_or_default();
    let cart_text = |name: &str| text_of(cart.and_then(|c| c.child(name)));
    let stab = b.child("STABILITE");
    let typical_situations = stab
        .and_then(|s| s.child("SitAvalTyp"))
        .map(|s| {
            ["SAT1", "SAT2"]
                .iter()
                .filter_map(|k| attr_u8(s, k))
                .collect()
        })
        .unwrap_or_default();
    let past = b.child("BSH").map(|h| PastDays {
        weather: h.child("METEO").map(parse_steps).unwrap_or_default(),
        snow_cover: h
            .child("ENNEIGEMENTS")
            .map(|e| {
                e.children_named("ENNEIGEMENT")
                    .map(parse_snow_cover)
                    .collect()
            })
            .unwrap_or_default(),
        fresh_snow: h.child("NEIGEFRAICHE").map(parse_fresh_snow),
        risks: h
            .child("RISQUES")
            .map(|r| {
                r.children_named("RISQUE")
                    .map(|x| PastRisk {
                        date: attr_str(x, "DATE"),
                        max: attr_i32(x, "RISQUEMAXI"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        avalanches: h
            .child("AVALANCHES")
            .map(|r| {
                r.children_named("AVALANCHE")
                    .map(|x| PastAvalanches {
                        date: attr_str(x, "DATE"),
                        spontaneous: attr_i32(x, "SPONTANE"),
                        triggered: attr_i32(x, "PROVOQUER"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    });
    let mut images = Vec::new();
    collect_image_names(b, &mut images);
    Ok(Bulletin {
        massif_id,
        massif,
        producer: attr_str(b, "PRODUCTEUR"),
        date_bulletin: attr_str(b, "DATEBULLETIN"),
        date_expiry: attr_str(b, "DATEECHEANCE"),
        date_validity: attr_str(b, "DATEVALIDITE").or_else(|| text_of(b.child("DateValidite"))),
        date_diffusion: attr_str(b, "DATEDIFFUSION"),
        amendment: attr_bool(b, "AMENDEMENT"),
        risk,
        aspects,
        accidental: cart_text("ACCIDENTEL"),
        natural: cart_text("NATUREL"),
        summary: cart_text("RESUME"),
        risk_j2_label: cart_text("RisqueJ2"),
        risk_j2_comment: cart_text("CommentaireRisqueJ2"),
        notice: cart_text("AVIS"),
        vigilance: cart_text("VIGILANCE"),
        typical_situations,
        stability: text_of(stab.and_then(|s| s.child("TEXTE"))),
        snow_quality: text_of(b.child("QUALITE").and_then(|q| q.child("TEXTE"))),
        snow_cover: b.child("ENNEIGEMENT").map(parse_snow_cover),
        fresh_snow: b.child("NEIGEFRAICHE").map(parse_fresh_snow),
        weather: b.child("METEO").map(parse_weather),
        trends: b
            .child("TENDANCES")
            .map(|t| {
                t.children_named("TENDANCE")
                    .map(|x| Trend {
                        date: attr_str(x, "DATE"),
                        value: attr_i32(x, "VALEUR"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        past,
        images,
    })
}

/// Whether a document looks like a BERA XML (root `BULLETINS_NEIGE_AVALANCHE` or
/// `Bulletins`). Used to tell a bulletin from the HTML "unavailable" page of the archive.
pub fn looks_like_bera(body: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&body[..body.len().min(2048)]);
    head.contains("<BULLETINS_NEIGE_AVALANCHE") || head.contains("<Bulletins")
}

/// Parses a BERA XML document; returns every bulletin it contains (one per massif).
pub fn parse_xml(xml_text: &str) -> Result<Vec<Bulletin>> {
    let root = xml::parse(xml_text).map_err(|message| Error::Xml {
        context: "BERA".to_owned(),
        message,
    })?;
    let bulletins: Vec<&Element> = match root.name.as_str() {
        "BULLETINS_NEIGE_AVALANCHE" => vec![&root],
        "Bulletins" => root.children_named("BULLETINS_NEIGE_AVALANCHE").collect(),
        other => {
            return Err(Error::format(
                "BERA XML",
                format!("unexpected root element <{other}>"),
            ));
        }
    };
    if bulletins.is_empty() {
        return Err(Error::format(
            "BERA XML",
            "no BULLETINS_NEIGE_AVALANCHE element",
        ));
    }
    bulletins.into_iter().map(parse_bulletin).collect()
}

/// Parses a document and returns the bulletin of `massif_id`.
pub fn parse_massif(xml_text: &str, massif_id: u16) -> Result<Bulletin> {
    parse_xml(xml_text)?
        .into_iter()
        .find(|b| b.massif_id == massif_id)
        .ok_or_else(|| Error::format("BERA XML", format!("no bulletin for massif {massif_id}")))
}

/// Request of today's bulletin of a massif on the DonneesPubliquesBRA API.
pub fn api_request(massif_id: u16, key: &Secret) -> Request {
    Request::get(
        SOURCE_API,
        &format!("{API_BASE}/massif/BRA?id-massif={massif_id}&format=xml"),
    )
    .secret_header("apikey", key)
}

/// URL of the archive index of a day.
pub fn archive_index_url(date: Date) -> String {
    format!("{ARCHIVE_BASE}/bra.{}.json", date.strftime("%Y%m%d"))
}

/// URL of an archived bulletin (`ext` = `xml` or `pdf`).
pub fn archive_bulletin_url(massif: &BeraMassif, stamp: &str, ext: &str) -> String {
    format!("{ARCHIVE_BASE}/BRA.{}.{stamp}.{ext}", massif.archive_name)
}

#[derive(Deserialize)]
struct IndexEntry {
    massif: String,
    heures: Vec<String>,
}

/// Publication stamps (`YYYYMMDDhhmmss`) of a massif in an archive index document.
pub fn stamps_in_index(index_json: &[u8], massif: &BeraMassif) -> Result<Vec<String>> {
    let entries: Vec<IndexEntry> = serde_json::from_slice(index_json)
        .map_err(|e| Error::format(SOURCE_ARCHIVE, format!("archive index: {e}")))?;
    Ok(entries
        .into_iter()
        .filter(|e| e.massif == massif.archive_name)
        .flat_map(|e| e.heures)
        .collect())
}

/// What the product contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeraStatus {
    /// A parsed bulletin is included.
    Bulletin,
    /// The massif has a BERA but no bulletin could be read (no key, no archive XML); links
    /// and the archived PDF, if any, are given.
    Unavailable,
    /// The site has no BERA (Jura): notice and links only.
    NoBera,
}

/// A link shown to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// Label.
    pub label: String,
    /// URL.
    pub url: String,
}

/// Content of `bera.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeraFile {
    /// Site identifier.
    pub site: String,
    /// Requested day (the bulletin of the evening before is valid for it).
    pub date: String,
    /// Massif identifier, if the site has a BERA.
    pub massif_id: Option<u16>,
    /// Massif name.
    pub massif: Option<String>,
    /// What the file contains.
    pub status: BeraStatus,
    /// Where the bulletin comes from (`api`, `archive`), if any.
    pub origin: Option<String>,
    /// Public URL of the bulletin XML, if one was read.
    pub xml_url: Option<String>,
    /// Public URL of the archived PDF of the day, if listed in the archive.
    pub pdf_url: Option<String>,
    /// Parsed bulletin.
    pub bulletin: Option<Bulletin>,
    /// Message for the user (French).
    pub notice: String,
    /// Links for the user.
    pub links: Vec<Link>,
    /// Generation time, RFC 3339 UTC.
    pub generated_at: String,
}

fn link(label: &str, url: &str) -> Link {
    Link {
        label: label.to_owned(),
        url: url.to_owned(),
    }
}

/// Fixed wording shown with every bulletin: never a substitute for the official bulletin.
pub const DISCLAIMER: &str = "Résumé du BERA de Météo-France. Consultez toujours le bulletin \
officiel complet avant de partir.";

/// Notice for sites without BERA.
pub const JURA_NOTICE: &str = "Le massif du Jura n'a pas de bulletin d'estimation du risque \
d'avalanche (BERA) de Météo-France. Consultez la météo montagne et la vigilance de \
Météo-France, et le bulletin d'avalanches du SLF, qui couvre le Jura suisse voisin (régions \
Vallée de Joux, Saint-Cergue). Des avalanches restent possibles sur les pentes raides.";

/// Bulletin of a day found in the public archive.
struct Archived {
    /// Publication stamp `YYYYMMDDhhmmss`.
    stamp: String,
    /// XML body, if the archive still serves it.
    xml: Option<Vec<u8>>,
    /// URL of the PDF.
    pdf_url: String,
}

/// Looks the bulletin of `date` up in the public archive.
fn from_archive(dl: &Downloader, massif: &BeraMassif, date: Date) -> Result<Option<Archived>> {
    let idx = dl.fetch(&Request::get(SOURCE_ARCHIVE, &archive_index_url(date)))?;
    let body = idx.bytes()?;
    let stamps = match stamps_in_index(&body, massif) {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("no archive index for {date}: {e}");
            return Ok(None);
        }
    };
    let Some(stamp) = stamps.last().cloned() else {
        return Ok(None);
    };
    let pdf = archive_bulletin_url(massif, &stamp, "pdf");
    let xml = dl.fetch(&Request::get(
        SOURCE_ARCHIVE,
        &archive_bulletin_url(massif, &stamp, "xml"),
    ))?;
    let xml_body = xml.bytes()?;
    let xml_body = looks_like_bera(&xml_body).then_some(xml_body);
    Ok(Some(Archived {
        stamp,
        xml: xml_body,
        pdf_url: pdf,
    }))
}

/// Builds the BERA product of a site for `date` and writes it to
/// `data/out/<site>/bera/`.
///
/// - Site without BERA (Jura): notice and links.
/// - With `key`: today's bulletin from the API (the API only serves the current bulletin,
///   `date` is then informative).
/// - Without key: the public archive of `date` (PDF link; parsed XML for dates up to April
///   2023), otherwise links only.
pub fn build(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    key: Option<&Secret>,
    date: Date,
) -> Result<BeraFile> {
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let generated_at = now_rfc3339();
    let mf_source = SourceRef::new(
        "Météo-France, bulletin d'estimation du risque d'avalanche",
        API_BASE,
        "Licence Ouverte Etalab 2.0 (données publiques Météo-France)",
        "Météo-France",
    );
    let mut notes = Vec::new();

    let mut file = BeraFile {
        site: site.id.to_owned(),
        date: date.to_string(),
        massif_id: None,
        massif: None,
        status: BeraStatus::NoBera,
        origin: None,
        xml_url: None,
        pdf_url: None,
        bulletin: None,
        notice: String::new(),
        links: Vec::new(),
        generated_at: generated_at.clone(),
    };
    let mut raw_xml: Option<Vec<u8>> = None;

    let sources = if !site.has_bera {
        file.notice = JURA_NOTICE.to_owned();
        file.links = vec![
            link("Météo montagne Jura (Météo-France)", MF_JURA_URL),
            link(
                "Vigilance météo du Doubs (Météo-France)",
                MF_VIGILANCE_DOUBS_URL,
            ),
            link(
                "Bulletin d'avalanches SLF (Suisse, Jura compris)",
                SLF_BULLETIN_URL,
            ),
            link("Bulletin SLF en CAAML JSON", SLF_CAAML_URL),
        ];
        notes.push("Pas de BERA dans le Jura (absent de l'annexe 1 de la spécification).".into());
        vec![SourceRef::new(
            "Liens Météo-France et SLF",
            SLF_BULLETIN_URL,
            "liens seulement, aucune donnée reprise",
            "Météo-France ; WSL Institut pour l'étude de la neige et des avalanches SLF",
        )]
    } else {
        let mf = massif(site.primary_massif().code).ok_or_else(|| {
            Error::format(
                "site",
                format!("massif {} is not a BERA massif", site.primary_massif().code),
            )
        })?;
        file.massif_id = Some(mf.id);
        file.massif = Some(mf.name.to_owned());
        file.links = vec![link(
            "BERA Alpes du Sud (Météo-France)",
            MF_ALPES_DU_SUD_URL,
        )];
        if let Some(key) = key {
            let req = api_request(mf.id, key);
            let f = dl.fetch(&req)?;
            let body = f.bytes()?;
            let text = String::from_utf8_lossy(&body);
            let b = parse_massif(&text, mf.id)?;
            file.status = BeraStatus::Bulletin;
            file.origin = Some("api".to_owned());
            file.xml_url = Some(req.url().to_owned());
            file.bulletin = Some(b);
            raw_xml = Some(body);
        } else {
            notes.push(
                "METEOFRANCE_API_KEY absente : l'API DonneesPubliquesBRA (401 sans clé) n'a \
                 pas été appelée ; recours à l'archive publique."
                    .to_owned(),
            );
            match from_archive(dl, mf, date)? {
                Some(Archived {
                    stamp,
                    xml: Some(xml_body),
                    pdf_url: pdf,
                }) => {
                    let text = String::from_utf8_lossy(&xml_body);
                    file.bulletin = Some(parse_massif(&text, mf.id)?);
                    file.status = BeraStatus::Bulletin;
                    file.origin = Some("archive".to_owned());
                    file.xml_url = Some(archive_bulletin_url(mf, &stamp, "xml"));
                    file.pdf_url = Some(pdf);
                    raw_xml = Some(xml_body);
                }
                Some(Archived {
                    xml: None,
                    pdf_url: pdf,
                    ..
                }) => {
                    file.status = BeraStatus::Unavailable;
                    file.origin = Some("archive".to_owned());
                    file.pdf_url = Some(pdf);
                    notes.push(
                        "L'archive publique ne sert plus le XML (redirection), seulement le \
                         PDF."
                            .to_owned(),
                    );
                }
                None => {
                    file.status = BeraStatus::Unavailable;
                    notes.push(format!(
                        "Aucun bulletin dans l'archive publique pour le {date}."
                    ));
                }
            }
        }
        if let Some(pdf) = &file.pdf_url {
            file.links
                .push(link("Bulletin du jour en PDF (archive Météo-France)", pdf));
        }
        file.notice = match &file.bulletin {
            Some(b) => {
                let level = b
                    .risk
                    .max
                    .map(|r| format!("{r} ({})", risk_label(r).unwrap_or("?")))
                    .unwrap_or_else(|| "non renseigné".to_owned());
                format!(
                    "BERA {} : risque maximal {level}, valable jusqu'au {}. {DISCLAIMER}",
                    b.massif,
                    b.date_validity.as_deref().unwrap_or("?")
                )
            }
            None => format!(
                "BERA {} non disponible hors ligne pour le {date} : consultez le bulletin \
                 officiel (lien).",
                mf.name
            ),
        };
        let mut s = mf_source.clone();
        if file.origin.as_deref() == Some("archive") {
            s.url = ARCHIVE_BASE.to_owned();
        }
        vec![s]
    };

    let mut files = vec![FileMeta::new("bera.json", "résumé structuré (BeraFile)")];
    if let Some(body) = &raw_xml {
        write_bytes(&dir.join("bera.xml"), body)?;
        files.push(FileMeta::new(
            "bera.xml",
            "bulletin XML brut de Météo-France",
        ));
    } else {
        let stale = dir.join("bera.xml");
        if stale.exists() {
            std::fs::remove_file(&stale).map_err(|e| skimap_common::Error::io(&stale, e))?;
        }
    }
    write_json(&dir.join("bera.json"), &file)?;
    let b = file.bulletin.as_ref();
    LayerMeta {
        tickets: vec!["P8 (#21)".to_owned()],
        generated_at,
        valid_from: b.and_then(|b| b.date_bulletin.clone()),
        valid_until: b.and_then(|b| b.date_validity.clone()),
        sources,
        files,
        notes,
        ..LayerMeta::new(LAYER, site.id)
    }
    .write(&dir)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    /// Belledonne example of the specification (26/10/2023), pages 9 to 12, transcribed
    /// element by element (the `[…]` ellipses of the specification are not part of it).
    const SPEC_SAMPLE: &str = include_str!("../tests/fixtures/bera_spec_belledonne_20231026.xml");
    /// Real Queyras bulletin of 15/04/2023 from the public archive, with the base64 image
    /// contents removed (everything else unchanged).
    const REAL_QUEYRAS: &str =
        include_str!("../tests/fixtures/BRA.QUEYRAS.20230415135925.noimg.xml");

    #[test]
    fn massif_table() {
        assert_eq!(MASSIFS.len(), 36);
        assert_eq!(massif(17).unwrap().archive_name, "QUEYRAS");
        assert!(massif(55).is_none(), "no BERA in the Jura");
        let mut ids: Vec<_> = MASSIFS.iter().map(|m| m.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), 36);
    }

    #[test]
    fn archive_index_names_match_the_table() {
        // Archive index of 15/01/2025 (names only), fetched on 14/11/2026.
        let idx = r#"[{"massif": "HAUTE-TARENTAISE", "heures": ["20250115143531"]}, {"massif": "HAUTE-MAURIENNE", "heures": ["20250115143532"]}, {"massif": "MAURIENNE", "heures": ["20250115143548"]}, {"massif": "VANOISE", "heures": ["20250115143556"]}, {"massif": "BEAUFORTAIN", "heures": ["20250115143819"]}, {"massif": "CINTO-ROTONDO", "heures": ["20250115144000"]}, {"massif": "RENOSO-INCUDINE", "heures": ["20250115144007"]}, {"massif": "CHARTREUSE", "heures": ["20250115145052"]}, {"massif": "BELLEDONNE", "heures": ["20250115145105"]}, {"massif": "GRANDES-ROUSSES", "heures": ["20250115145108"]}, {"massif": "OISANS", "heures": ["20250115145120"]}, {"massif": "VERCORS", "heures": ["20250115145130"]}, {"massif": "ARAVIS", "heures": ["20250115145744"]}, {"massif": "BAUGES", "heures": ["20250115145756"]}, {"massif": "CHABLAIS", "heures": ["20250115145758"]}, {"massif": "HAUTE-BIGORRE", "heures": ["20250115150258"]}, {"massif": "ASPE-OSSAU", "heures": ["20250115150316"]}, {"massif": "AURE-LOURON", "heures": ["20250115150319"]}, {"massif": "LUCHONNAIS", "heures": ["20250115150328"]}, {"massif": "PAYS-BASQUE", "heures": ["20250115150332"]}, {"massif": "HAUT-VAR-HAUT-VERDON", "heures": ["20250115150352"]}, {"massif": "UBAYE", "heures": ["20250115150458"]}, {"massif": "MERCANTOUR", "heures": ["20250115150725"]}, {"massif": "MONT-BLANC", "heures": ["20250115150737"]}, {"massif": "CERDAGNE-CANIGOU", "heures": ["20250115152859"]}, {"massif": "ORLU-ST-BARTHELEMY", "heures": ["20250115152900"]}, {"massif": "HAUTE-ARIEGE", "heures": ["20250115152909"]}, {"massif": "COUSERANS", "heures": ["20250115152913"]}, {"massif": "CAPCIR-PUYMORENS", "heures": ["20250115152917"]}, {"massif": "ANDORRE", "heures": ["20250115152922"]}, {"massif": "EMBRUNAIS-PARPAILLON", "heures": ["20250115153209"]}, {"massif": "DEVOLUY", "heures": ["20250115153220"]}, {"massif": "THABOR", "heures": ["20250115153223"]}, {"massif": "QUEYRAS", "heures": ["20250115153228"]}, {"massif": "CHAMPSAUR", "heures": ["20250115153306"]}, {"massif": "PELVOUX", "heures": ["20250115153315"]}]"#;
        for m in MASSIFS {
            let s = stamps_in_index(idx.as_bytes(), m).unwrap();
            assert_eq!(s.len(), 1, "{}", m.archive_name);
        }
        let q = massif(17).unwrap();
        assert_eq!(
            stamps_in_index(idx.as_bytes(), q).unwrap(),
            ["20250115153228"]
        );
        assert_eq!(
            archive_bulletin_url(q, "20250115153228", "pdf"),
            "https://donneespubliques.meteofrance.fr/donnees_libres/Pdf/BRA/BRA.QUEYRAS.20250115153228.pdf"
        );
        assert_eq!(
            archive_index_url(date(2025, 1, 15)),
            "https://donneespubliques.meteofrance.fr/donnees_libres/Pdf/BRA/bra.20250115.json"
        );
        assert!(stamps_in_index(b"<html>302</html>", q).is_err());
    }

    #[test]
    fn spec_sample_belledonne() {
        let all = parse_xml(SPEC_SAMPLE).unwrap();
        assert_eq!(all.len(), 1);
        let b = &all[0];
        assert_eq!(b.massif_id, 8);
        assert_eq!(b.massif, "Belledonne");
        assert_eq!(b.date_bulletin.as_deref(), Some("2023-10-26T10:00:00"));
        assert_eq!(b.date_validity.as_deref(), Some("2023-10-27T18:00:00"));
        assert!(!b.amendment);
        let r = &b.risk;
        assert_eq!(
            (r.risk1, r.risk2, r.max, r.max_j2),
            (Some(2), Some(3), Some(3), Some(2))
        );
        assert_eq!(r.loc1.as_deref(), Some("<2100"));
        assert_eq!(r.loc2.as_deref(), Some(">2100"));
        assert_eq!(r.altitude, Some(2100));
        assert_eq!(r.evolution1, None);
        assert_eq!(r.evolution2, Some(2));
        assert_eq!(r.date_j2.as_deref(), Some("2023-10-28T00:00:00"));
        let a = &b.aspects;
        assert!(a.ne && a.s && a.nw && !a.n && !a.e && !a.se && !a.sw && !a.w);
        assert_eq!(b.risk_j2_label.as_deref(), Some("risque limité"));
        assert_eq!(b.notice, None);
        assert_eq!(b.typical_situations, [1]);
        assert!(
            b.stability
                .as_deref()
                .unwrap()
                .starts_with("Bloc de texte stabilité")
        );
        let sc = b.snow_cover.as_ref().unwrap();
        assert_eq!(
            (sc.limit_south_m, sc.limit_north_m),
            (Some(1800), Some(1600))
        );
        assert_eq!(sc.levels.len(), 3);
        assert_eq!(sc.levels[2].north_cm, Some(40));
        let fs = b.fresh_snow.as_ref().unwrap();
        assert_eq!(fs.altitude_m, Some(1800));
        assert_eq!(fs.days.len(), 6);
        assert_eq!((fs.days[2].min_cm, fs.days[2].max_cm), (Some(20), Some(30)));
        let w = b.weather.as_ref().unwrap();
        assert_eq!(w.steps.len(), 4);
        assert_eq!(w.steps[0].wind2_kmh, Some(20));
        assert_eq!(w.steps[0].iso0_m, Some(2700));
        assert_eq!(w.steps[0].sky_label.as_deref(), Some("rares averses"));
        assert_eq!(w.steps[2].wind1_dir.as_deref(), Some("Vrb"));
        let p = b.past.as_ref().unwrap();
        assert_eq!(p.weather.len(), 28);
        assert_eq!(p.snow_cover.len(), 7);
        assert_eq!(p.fresh_snow.as_ref().unwrap().days.len(), 7);
        assert_eq!(p.risks.len(), 7);
        assert!(b.images.contains(&"montagne_risques_8.png".to_owned()));
        assert!(b.images.contains(&"sept_derniers_jours_8.png".to_owned()));
    }

    #[test]
    fn real_queyras_archive_20230415() {
        let b = parse_massif(REAL_QUEYRAS, 17).unwrap();
        assert_eq!(b.massif, "QUEYRAS");
        assert_eq!(b.producer.as_deref(), Some("STA-Briancon"));
        assert_eq!(b.date_bulletin.as_deref(), Some("2023-04-15T16:00:00"));
        assert_eq!(b.date_validity.as_deref(), Some("2023-04-16T18:00:00"));
        assert_eq!(
            (b.risk.risk1, b.risk.risk2, b.risk.max),
            (Some(1), Some(2), Some(2))
        );
        assert_eq!(b.risk.altitude, Some(2400));
        assert_eq!(
            b.risk.comment.as_deref(),
            Some("Au-dessus de 2400 m : Risque limité. En-dessous : Risque faible.")
        );
        let a = &b.aspects;
        assert!(a.n && a.ne && a.e && a.nw && !a.s && !a.se && !a.sw && !a.w);
        assert_eq!(
            b.accidental.as_deref(),
            Some("quelques plaques en altitude.")
        );
        assert!(
            b.stability
                .as_deref()
                .unwrap()
                .starts_with("QUELQUES PLAQUES EN ALTITUDE")
        );
        let sc = b.snow_cover.as_ref().unwrap();
        assert_eq!(sc.levels[2].north_cm, Some(105));
        assert_eq!(sc.levels[2].south_cm, Some(25));
        let fs = b.fresh_snow.as_ref().unwrap();
        assert_eq!(fs.sector1.as_deref(), Some("Ouest"));
        assert_eq!(fs.days.len(), 6);
        assert_eq!(fs.days[5].sector2_cm, Some(2));
        let w = b.weather.as_ref().unwrap();
        assert_eq!(w.steps.len(), 3);
        assert_eq!(w.steps[0].wind1_kmh, Some(50));
        assert_eq!(w.steps[0].iso_minus10_m, Some(3800));
        assert_eq!(b.trends.len(), 2);
        let p = b.past.as_ref().unwrap();
        assert_eq!(p.weather.len(), 21);
        assert_eq!(p.snow_cover.len(), 7);
        assert_eq!(p.avalanches.len(), 7);
        assert!(
            b.images
                .iter()
                .any(|i| i.starts_with("ImageCartoucheRisque"))
        );
    }

    #[test]
    fn rejects_non_bera() {
        assert!(!looks_like_bera(
            b"<!DOCTYPE HTML PUBLIC><html>302 Found</html>"
        ));
        assert!(looks_like_bera(REAL_QUEYRAS.as_bytes()));
        assert!(parse_xml("<html><body/></html>").is_err());
        assert!(parse_massif(SPEC_SAMPLE, 17).is_err());
    }

    #[test]
    fn labels() {
        assert_eq!(risk_label(3), Some("marqué"));
        assert_eq!(risk_label(0), None);
        assert_eq!(sky_label(32), Some("brouillard"));
        assert_eq!(sky_label(11), None);
    }

    #[test]
    fn api_request_hides_the_key() {
        let key = Secret::new("SECRET-KEY-123");
        let r = api_request(17, &key);
        assert_eq!(
            r.url(),
            "https://public-api.meteofrance.fr/public/DPBRA/v1/massif/BRA?id-massif=17&format=xml"
        );
        assert!(!format!("{r:?}").contains("SECRET"));
    }
}
