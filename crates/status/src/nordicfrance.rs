//! Nordic France white-label station widget (`/widget/marqueblanche/station/<id>`), the
//! bulletin the ski areas fill in with the « Nordic App ». Cervières - Izoard is station
//! 3167 (found in nordicalpesdusud.com, report 02).
//!
//! **Personal use only**: no API, no licence; reading the server-rendered HTML without an
//! agreement is only acceptable for personal, low-frequency use (report 03). Polled at
//! most every [`MIN_INTERVAL`].

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::html::{after, attr_after, before, chunks, element_text, text};

/// Downloader source.
pub const SOURCE: &str = "nordicfrance";
/// Shortest interval between two requests (the cache serves the page in between).
pub const MIN_INTERVAL: Duration = Duration::from_secs(3 * 3600);

/// Station identifier of a site.
pub fn station_id(site_id: &str) -> Option<u32> {
    match site_id {
        "cervieres" => Some(3167),
        _ => None,
    }
}

/// Widget URL of a station.
pub fn station_url(id: u32) -> String {
    format!("https://www.nordicfrance.fr/widget/marqueblanche/station/{id}")
}

/// One piste or itinerary of the widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NfPiste {
    /// Name.
    pub name: String,
    /// Activity line as displayed (« 16km - Raquettes , Marche/trail »).
    pub activity: String,
    /// Length, km, when the activity line starts with it.
    pub length_km: Option<f64>,
    /// Colour flag (`Vert`, `Bleu`, `Rouge`, `Noir`) or `raquettes`.
    pub flag: Option<String>,
    /// State modifier of the widget (`closed`, `open`, ...).
    pub state: Option<String>,
    /// State label as displayed (« Fermée », « Ouverte »).
    pub state_label: Option<String>,
    /// Comment of the piste.
    pub comment: Option<String>,
}

/// One sector of the widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NfSector {
    /// Name (« IZOARD »).
    pub name: String,
    /// State icon of the header (`cancel` = closed, ...).
    pub state: Option<String>,
    /// Last grooming date (`dd/mm`).
    pub grooming: Option<String>,
    /// Comment of the sector.
    pub comment: Option<String>,
    /// Pistes.
    pub pistes: Vec<NfPiste>,
}

/// Normalised content of the widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NfStation {
    /// Station name.
    pub name: Option<String>,
    /// « Hauteur de neige ».
    pub snow_depth: Option<String>,
    /// « Dernier damage » (`dd/mm`).
    pub last_grooming: Option<String>,
    /// « Qualité de neige ».
    pub snow_quality: Option<String>,
    /// « Mise à jour » (`dd/mm`).
    pub updated: Option<String>,
    /// Open length summary (« 0 / 95km »).
    pub open_summary: Option<String>,
    /// Sectors.
    pub sectors: Vec<NfSector>,
}

fn snow_field(html: &str, title: &str) -> Option<String> {
    let s = after(html, &format!("StationWidget-snowTitle\">{title}</span>"))?;
    let v = text(before(s, "</div>"));
    let v = v.trim_start_matches(':').trim();
    (!v.is_empty()).then(|| v.to_owned())
}

fn icon_modifier(chunk: &str, prefix: &str) -> Option<String> {
    let s = after(chunk, prefix)?;
    let m: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!m.is_empty()).then_some(m)
}

/// Parses the widget page.
pub fn parse(html: &str) -> NfStation {
    // `<div class="NordicDetail-secteur active">`, not `NordicDetail-secteurHeader`.
    const SECTOR: &str = "<div class=\"NordicDetail-secteur";
    let starts: Vec<usize> = html
        .match_indices(SECTOR)
        .map(|(i, _)| i)
        .filter(|&i| html[i + SECTOR.len()..].starts_with(['"', ' ']))
        .collect();
    let sectors = starts
        .iter()
        .enumerate()
        .map(|(k, &i)| &html[i..starts.get(k + 1).copied().unwrap_or(html.len())])
        .map(|c| {
            let (header, list) = match c.find("NordicDetail-pisteList") {
                Some(i) => (&c[..i], &c[i..]),
                None => (c, ""),
            };
            let name = element_text(header, "class=\"NordicDetail-secteurName\"", "p")
                .map(|n| {
                    n.trim_start_matches("Secteur")
                        .trim()
                        .trim_start_matches(':')
                        .trim()
                        .to_owned()
                })
                .unwrap_or_default();
            NfSector {
                name,
                state: icon_modifier(header, "NordicDetail-icon--"),
                grooming: element_text(header, "class=\"NordicDetail-damagelabel\"", "span"),
                comment: attr_after(header, "class=\"NordicDetail-iconInfo\"", "data-comment")
                    .filter(|s| !s.is_empty()),
                pistes: chunks(list, "<div class=\"NordicDetail-piste\">")
                    .into_iter()
                    .map(|p| {
                        let activity = element_text(p, "class=\"NordicDetail-pisteActivity\"", "p")
                            .unwrap_or_default();
                        let length_km = activity
                            .split_once("km")
                            .and_then(|(n, _)| n.trim().replace(',', ".").parse().ok());
                        let flag = after(p, "class=\"NordicDetail-pisteIcon\"")
                            .and_then(|s| after(s, "xlink:href=\"#"))
                            .map(|s| before(s, "\"").trim_start_matches("flag-").to_owned())
                            .filter(|f| !f.is_empty());
                        let ouverture = after(p, "NordicDetail-pisteOuverture ");
                        NfPiste {
                            name: element_text(p, "class=\"NordicDetail-pisteName\"", "p")
                                .unwrap_or_default(),
                            activity,
                            length_km,
                            flag,
                            state: ouverture
                                .and_then(|s| icon_modifier(s, "NordicDetail-pisteOuverture--")),
                            state_label: ouverture.and_then(|s| element_text(s, "<p", "p")),
                            comment: attr_after(
                                before(p, "NordicDetail-pisteOuverture"),
                                "class=\"NordicDetail-iconInfo\"",
                                "data-comment",
                            )
                            .filter(|s| !s.is_empty()),
                        }
                    })
                    .collect(),
            }
        })
        .collect();
    NfStation {
        name: element_text(html, "class=\"StationWidget-title\"", "h1"),
        snow_depth: snow_field(html, "Hauteur de neige"),
        last_grooming: snow_field(html, "Dernier damage"),
        snow_quality: snow_field(html, "Qualité de neige"),
        updated: snow_field(html, "Mise à jour"),
        open_summary: after(html, "class=\"StationWidget-ouvertureSection\"")
            .map(|s| text(before(after(s, ">").unwrap_or(s), "</div>")))
            .filter(|s| !s.is_empty()),
        sectors,
    }
}

/// Published stamps: « Mise à jour » and the grooming date of each sector.
pub fn stamps(v: &Value) -> Vec<String> {
    let mut s = Vec::new();
    if let Some(u) = v["updated"].as_str() {
        s.push(format!("mise à jour {u}"));
    }
    for sec in v["sectors"].as_array().into_iter().flatten() {
        if let (Some(n), Some(g)) = (sec["name"].as_str(), sec["grooming"].as_str()) {
            s.push(format!("damage {n} {g}"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Excerpt of the widget of station 3167 fetched on 14/11/2026 (one sector, two
    /// pistes), unchanged apart from removed blank lines.
    const PAGE: &str = r##"<div class="StationWidget">
    <h1 class="StationWidget-title">Cervières - Izoard</h1>
        <div class="StationWidget-neigeContainer">
            <div>
                <span class="StationWidget-snowTitle">Hauteur de neige</span>
                : 0cm/0cm
            </div>
            <div>
                <span class="StationWidget-snowTitle">Dernier damage</span>
                : 05/04
            </div>
            <div>
                <span class="StationWidget-snowTitle">Qualité de neige</span>
                : Non renseigné
            </div>
            <div>
                <span class="StationWidget-snowTitle">Mise à jour</span>
                : 06/04
            </div>
        </div>
        <div class="StationWidget-ouvertureSection">
                                                                                    <div class="NordicDetail-indicatorRow">
                <svg role="img" class="NordicDetail-indicatorIcon" width="28" height="28">
                    <use xlink:href="#sign"/>
                </svg>
                0 / 95km
            </div>
        </div>
    <div class="NordicDetail-bloc" id="NordicDetail-piste">
                    <div class="NordicDetail-secteur active">
                                                                                                            <div class="NordicDetail-secteurHeader">
                    <svg role="img" class="NordicDetail-icon--cancel" width="20" height="20">
                        <title>closed</title>
                        <use xlink:href="#cancel"/>
                    </svg>
                    <p class="NordicDetail-secteurName">Secteur
                        : IZOARD</p>
                                            <div class="NordicDetail-iconInfo" data-comment="La mythique route du col de l’Izoard, que vous pouvez pratiquer en ski de fond classique ou skating">
                        </div>
                                        <div class="NordicDetail-damageContainer">
                        <span class="NordicDetail-damagelabel">04/03</span>
                    </div>
                </div>
                <div class="NordicDetail-pisteList">
                                                                        <div class="NordicDetail-piste">
                                <div class="NordicDetail-pisteLeft">
                                    <svg role="img" class="NordicDetail-pisteIcon" width="35" height="35">
                                        <use xlink:href="#flag-Rouge"/>
                                    </svg>
                                    <div class="NordicDetail-pisteLeftInner">
                                        <p class="NordicDetail-pisteName">L&#039;Izoard</p>
                                        <p class="NordicDetail-pisteActivity">
                                                                                            16km -
                                                                                        Ski de fond
                                                                                    </p>
                                    </div>
                                                                            <div class="NordicDetail-iconInfo" data-comment="Piste linéaire">
                                        </div>
                                                                    </div>
                                <span class="NordicDetail-pisteOuverture NordicDetail-pisteOuverture--closed">
                                    <svg role="img" class="NordicDetail-icon--cancel" width="17" height="17">
                                        <title>closed</title><use xlink:href="#cancel"/>
                                    </svg>
                                    <p>Fermée</p>
                                    </span>
                            </div>
                                                                                                <div class="NordicDetail-piste">
                                <div class="NordicDetail-pisteLeft">
                                    <svg role="img" class="NordicDetail-pisteIcon" width="35" height="35">
                                        <use xlink:href="#raquettes"/>
                                    </svg>
                                    <div class="NordicDetail-pisteLeftInner">
                                        <p class="NordicDetail-pisteName">Piéton multiactivités Izoard</p>
                                        <p class="NordicDetail-pisteActivity">
                                                                                            16km -
                                                                                        Raquettes
                                                                                            , Marche/trail
                                                                                            , Fatbike
                                                                                    </p>
                                    </div>
                                                                    </div>
                                <span class="NordicDetail-pisteOuverture NordicDetail-pisteOuverture--closed">
                                    <p>Fermée</p>
                                    </span>
                            </div>
                                                            </div>
            </div>"##;

    #[test]
    fn widget() {
        let s = parse(PAGE);
        assert_eq!(s.name.as_deref(), Some("Cervières - Izoard"));
        assert_eq!(s.snow_depth.as_deref(), Some("0cm/0cm"));
        assert_eq!(s.last_grooming.as_deref(), Some("05/04"));
        assert_eq!(s.snow_quality.as_deref(), Some("Non renseigné"));
        assert_eq!(s.updated.as_deref(), Some("06/04"));
        assert_eq!(s.open_summary.as_deref(), Some("0 / 95km"));
        assert_eq!(s.sectors.len(), 1);
        let z = &s.sectors[0];
        assert_eq!(z.name, "IZOARD");
        assert_eq!(z.state.as_deref(), Some("cancel"));
        assert_eq!(z.grooming.as_deref(), Some("04/03"));
        assert!(z.comment.as_deref().unwrap().starts_with("La mythique"));
        assert_eq!(z.pistes.len(), 2);
        let p = &z.pistes[0];
        assert_eq!(p.name, "L'Izoard");
        assert_eq!(p.activity, "16km - Ski de fond");
        assert_eq!(p.length_km, Some(16.0));
        assert_eq!(p.flag.as_deref(), Some("Rouge"));
        assert_eq!(p.state.as_deref(), Some("closed"));
        assert_eq!(p.state_label.as_deref(), Some("Fermée"));
        assert_eq!(p.comment.as_deref(), Some("Piste linéaire"));
        assert_eq!(z.pistes[1].flag.as_deref(), Some("raquettes"));
        assert_eq!(z.pistes[1].comment, None);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(stamps(&v), ["mise à jour 06/04", "damage IZOARD 04/03"]);
        assert_eq!(station_id("cervieres"), Some(3167));
    }
}
