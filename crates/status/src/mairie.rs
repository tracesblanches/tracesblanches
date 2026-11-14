//! « Info damage » page of the Cervières town hall
//! (<https://cervieres.fr/domaine-nordique-2/info-damage/>): a WordPress page with one
//! table per sector (piste, open/closed, last grooming, classic state, skating state,
//! comment), edited by hand.
//!
//! **Personal use only** (no licence statement); polled at most every [`MIN_INTERVAL`].
//! The page date (`article:modified_time`) is the published stamp used to measure the
//! update frequency.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::html::{after, attr_after, before, chunks, text};

/// Downloader source.
pub const SOURCE: &str = "cervieres-mairie";
/// Page URL.
pub const URL: &str = "https://cervieres.fr/domaine-nordique-2/info-damage/";
/// Shortest interval between two requests.
pub const MIN_INTERVAL: Duration = Duration::from_secs(3 * 3600);

/// One row of a sector table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MairieRow {
    /// Piste (with its length as written, « Le Gibera 1.5km »).
    pub piste: String,
    /// « OUVERT/FERME » cell.
    pub state: String,
    /// « Dernier DAMAGE ».
    pub last_grooming: String,
    /// « ETAT CLASSIC ».
    pub classic: String,
    /// « ETAT SKATING ».
    pub skating: String,
    /// « COMMENTAIRE ».
    pub comment: String,
}

/// One sector table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MairieSector {
    /// Header above the table (« SECTEUR LE LAUS-IZOARD / Fermé le 8 mars »).
    pub header: String,
    /// Rows (header row excluded).
    pub rows: Vec<MairieRow>,
}

/// Normalised content of the page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MairiePage {
    /// `article:modified_time` of the page.
    pub modified_time: Option<String>,
    /// Text before the first sector (general message).
    pub message: String,
    /// Sector tables.
    pub sectors: Vec<MairieSector>,
}

/// Parses the page.
pub fn parse(html: &str) -> MairiePage {
    let modified_time = attr_after(html, "property=\"article:modified_time\"", "content");
    let content = after(html, "class=\"entry-content\"")
        .and_then(|s| after(s, ">"))
        .unwrap_or(html);
    let content = before(content, "</article>");
    let mut sectors = Vec::new();
    let mut message = String::new();
    let mut prev_end = 0;
    for (i, table) in content.match_indices("<table") {
        let between = text(&content[prev_end..i]);
        let (pre, header) = match between.find("SECTEUR") {
            Some(k) => (
                between[..k].trim().to_owned(),
                between[k..].trim().to_owned(),
            ),
            None => (between.trim().to_owned(), String::new()),
        };
        if sectors.is_empty() {
            message = pre;
        }
        let body = &content[i + table.len()..];
        let end = body.find("</table>").unwrap_or(body.len());
        let rows: Vec<Vec<String>> = chunks(&body[..end], "<tr")
            .into_iter()
            .map(|tr| {
                chunks(tr, "<td")
                    .into_iter()
                    .map(|td| text(before(after(td, ">").unwrap_or(""), "</td>")))
                    .collect()
            })
            .collect();
        let rows = rows
            .into_iter()
            .filter(|r| !r.is_empty() && r[0] != "PISTE")
            .map(|r| {
                let cell = |k: usize| r.get(k).cloned().unwrap_or_default();
                MairieRow {
                    piste: cell(0),
                    state: cell(1),
                    last_grooming: cell(2),
                    classic: cell(3),
                    skating: cell(4),
                    comment: cell(5),
                }
            })
            .collect();
        sectors.push(MairieSector { header, rows });
        prev_end = i + table.len() + end;
    }
    MairiePage {
        modified_time,
        message,
        sectors,
    }
}

/// Published stamp: the page modification time.
pub fn stamps(v: &Value) -> Vec<String> {
    v["modified_time"]
        .as_str()
        .map(|s| vec![format!("page modifiée {s}")])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Excerpt of the page fetched on 14/11/2026 (styles removed, first table shortened).
    const PAGE: &str = r#"<meta property="article:modified_time" content="2026-04-13T08:58:38+02:00" />
<div class="entry-content">
<p><span><strong>INFOS DAMAGE<br />
DOMAINE CERVIERES-IZOARD</strong></span></p>
<p><strong><span><span><br />
LE DOMAINE NORDIQUE EST FERME RDV LA SAISON PROCHAINE !</span></span></strong></p>
<p><span><strong><span>SECTEUR LE LAUS-IZOARD /</span></strong></span><span><strong> Fermé le 8 mars</p>
<p></strong></span></p>
<table border="1">
<tbody>
<tr>
<td><strong>PISTE</strong></td>
<td><strong>OUVERT/FERME</strong></td>
<td><strong>Dernier DAMAGE</strong></td>
<td><strong>ETAT CLASSIC</strong></td>
<td><strong>ETAT SKATING</strong></td>
<td><strong>COMMENTAIRE</strong></td>
</tr>
<tr>
<td><span><strong>Le Gibera 1.5km</strong></span></td>
<td>Fermé</td>
<td></td>
<td></td>
<td></td>
<td></td>
</tr>
<tr>
<td><span><strong>Izoard<br />
16km</strong></span></td>
<td>Fermé</td>
<td></td>
<td></td>
<td></td>
<td></td>
</tr>
</tbody>
</table>
<p><span><strong><span><span>SECTEUR LES FONTS </span></span></strong></span><span><strong><span><span> <span>Fermé le 6 avril<br />
</span></span></span></strong></span></p>
<table border="1">
<tbody>
<tr>
<td><strong>PISTE</strong></td>
<td><strong>OUVERT/FERME</strong></td>
<td><strong>Dernier DAMAGE</strong></td>
<td><strong>ETAT CLASSIC</strong></td>
<td><strong>ETAT SKATING</strong></td>
<td><strong>COMMENTAIRE</strong></td>
</tr>
<tr>
<td><strong>L&#8217;Aigalan<br />
8km</strong></td>
<td>Fermé</td>
<td></td>
<td></td>
<td></td>
<td></td>
</tr>
</tbody>
</table>
</div></article>"#;

    #[test]
    fn page() {
        let p = parse(PAGE);
        assert_eq!(
            p.modified_time.as_deref(),
            Some("2026-04-13T08:58:38+02:00")
        );
        assert_eq!(
            p.message,
            "INFOS DAMAGE DOMAINE CERVIERES-IZOARD LE DOMAINE NORDIQUE EST FERME RDV LA SAISON PROCHAINE !"
        );
        assert_eq!(p.sectors.len(), 2);
        assert_eq!(
            p.sectors[0].header,
            "SECTEUR LE LAUS-IZOARD / Fermé le 8 mars"
        );
        assert_eq!(p.sectors[0].rows.len(), 2);
        assert_eq!(p.sectors[0].rows[1].piste, "Izoard 16km");
        assert_eq!(p.sectors[0].rows[1].state, "Fermé");
        assert_eq!(p.sectors[1].header, "SECTEUR LES FONTS Fermé le 6 avril");
        assert_eq!(p.sectors[1].rows[0].piste, "L’Aigalan 8km");
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(stamps(&v), ["page modifiée 2026-04-13T08:58:38+02:00"]);
    }
}
