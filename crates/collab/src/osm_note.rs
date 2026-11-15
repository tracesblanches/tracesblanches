//! Corrections of OpenStreetMap from the app (V12 #75): a note of the OSM notes API
//! (`POST /api/0.6/notes`, anonymous, no account) that the app prepares and the user
//! reads, edits and sends themself. skimap never edits OSM data: the local mappers read
//! the note and fix the map. The note is public on openstreetmap.org: the app shows the
//! exact text before sending and says so.

use serde::Serialize;

/// Longest note text sent (the OSM API refuses longer comments).
pub const MAX_TEXT: usize = 2000;
/// Longest comment of the user kept in the note.
pub const MAX_COMMENT: usize = 1000;

/// What is wrong on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    /// A nordic piste is here but missing from the map.
    MissingPiste,
    /// The piste on the map no longer exists.
    PisteGone,
    /// Wrong name.
    WrongName,
    /// Wrong difficulty (colour).
    WrongDifficulty,
    /// Wrong grooming (classic, skating, backcountry).
    WrongGrooming,
    /// Wrong one-way direction.
    WrongOneway,
    /// Something else (the comment says what).
    Other,
}

impl Correction {
    /// Identifier (app, tests).
    pub fn id(self) -> &'static str {
        match self {
            Correction::MissingPiste => "missing_piste",
            Correction::PisteGone => "piste_gone",
            Correction::WrongName => "wrong_name",
            Correction::WrongDifficulty => "wrong_difficulty",
            Correction::WrongGrooming => "wrong_grooming",
            Correction::WrongOneway => "wrong_oneway",
            Correction::Other => "other",
        }
    }

    /// From its identifier.
    pub fn parse(s: &str) -> Option<Self> {
        [
            Correction::MissingPiste,
            Correction::PisteGone,
            Correction::WrongName,
            Correction::WrongDifficulty,
            Correction::WrongGrooming,
            Correction::WrongOneway,
            Correction::Other,
        ]
        .into_iter()
        .find(|c| c.id() == s)
    }

    fn sentence(self, fr: bool) -> &'static str {
        match (self, fr) {
            (Correction::MissingPiste, true) => {
                "Une piste de ski de fond passe ici mais n'est pas sur la carte."
            }
            (Correction::MissingPiste, false) => {
                "A cross-country ski piste runs here but is missing from the map."
            }
            (Correction::PisteGone, true) => {
                "Cette piste de ski de fond n'existe plus sur le terrain."
            }
            (Correction::PisteGone, false) => {
                "This cross-country ski piste no longer exists on the ground."
            }
            (Correction::WrongName, true) => "Le nom de cette piste est faux.",
            (Correction::WrongName, false) => "The name of this piste is wrong.",
            (Correction::WrongDifficulty, true) => {
                "La difficulté (couleur) de cette piste est fausse."
            }
            (Correction::WrongDifficulty, false) => {
                "The difficulty (colour) of this piste is wrong."
            }
            (Correction::WrongGrooming, true) => {
                "Le damage de cette piste (classique, skating, non damée) est faux."
            }
            (Correction::WrongGrooming, false) => {
                "The grooming of this piste (classic, skating, ungroomed) is wrong."
            }
            (Correction::WrongOneway, true) => "Le sens unique de cette piste est faux.",
            (Correction::WrongOneway, false) => "The one-way direction of this piste is wrong.",
            (Correction::Other, true) => "Erreur sur la carte des pistes de ski de fond.",
            (Correction::Other, false) => "Error on the map of the cross-country ski pistes.",
        }
    }
}

/// The piste the note is about, from the pack.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Piste {
    /// Name on the map.
    pub name: Option<String>,
    /// OSM way.
    pub osm_way_id: Option<i64>,
}

/// A note ready to be reviewed and sent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Draft {
    /// Latitude (7 decimals: OSM's precision).
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
    /// The text, as it will be published.
    pub text: String,
}

/// Why a draft is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DraftError {
    /// Bad position.
    #[error("position outside the area served")]
    Position,
    /// `Other` needs a comment.
    #[error("say what is wrong")]
    EmptyComment,
}

fn clean_comment(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    let s = s.trim();
    s.chars().take(MAX_COMMENT).collect()
}

/// The note for a correction at a position. `fr`: in French (else English, the language
/// of the phone).
pub fn draft(
    correction: Correction,
    piste: &Piste,
    comment: &str,
    lon: f64,
    lat: f64,
    fr: bool,
) -> Result<Draft, DraftError> {
    if !crate::rules::in_area(lon, lat) {
        return Err(DraftError::Position);
    }
    let comment = clean_comment(comment);
    if correction == Correction::Other && comment.is_empty() {
        return Err(DraftError::EmptyComment);
    }
    let mut text = String::new();
    text.push_str(correction.sentence(fr));
    if let Some(name) = piste
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        text.push_str(if fr { "\nPiste : « " } else { "\nPiste: \"" });
        text.push_str(name);
        text.push_str(if fr { " »" } else { "\"" });
    }
    if let Some(id) = piste.osm_way_id {
        text.push_str(&format!("\nhttps://www.openstreetmap.org/way/{id}"));
    }
    if !comment.is_empty() {
        text.push_str("\n\n");
        text.push_str(&comment);
    }
    text.push_str(if fr {
        "\n\n(Signalé sur le terrain avec l'application skimap) #skimap"
    } else {
        "\n\n(Reported on the ground with the skimap app) #skimap"
    });
    let text: String = text.chars().take(MAX_TEXT).collect();
    let round = |v: f64| (v * 1e7).round() / 1e7;
    Ok(Draft {
        lat: round(lat),
        lon: round(lon),
        text,
    })
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The form body of `POST /api/0.6/notes` (`application/x-www-form-urlencoded`).
pub fn form_body(d: &Draft) -> String {
    format!(
        "lat={}&lon={}&text={}",
        d.lat,
        d.lon,
        percent_encode(&d.text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_in_french_with_piste_and_comment() {
        let p = Piste {
            name: Some("La Chaux Mourant".into()),
            osm_way_id: Some(8207721),
        };
        let d = draft(
            Correction::WrongName,
            &p,
            "  Sur le panneau : « Chaux Mourant »\u{7}",
            6.1234567891,
            46.5,
            true,
        )
        .unwrap();
        assert_eq!(d.lon, 6.1234568);
        assert_eq!(
            d.text,
            "Le nom de cette piste est faux.\nPiste : « La Chaux Mourant »\n\
             https://www.openstreetmap.org/way/8207721\n\nSur le panneau : « Chaux Mourant »\n\n\
             (Signalé sur le terrain avec l'application skimap) #skimap"
        );
        let body = form_body(&d);
        assert!(body.starts_with("lat=46.5&lon=6.1234568&text=Le%20nom%20de"));
        assert!(!body.contains(' ') && !body.contains('\n'));
    }

    #[test]
    fn english_other_needs_a_comment_and_is_bounded() {
        assert_eq!(
            draft(Correction::Other, &Piste::default(), " ", 6.0, 45.0, false),
            Err(DraftError::EmptyComment)
        );
        assert_eq!(
            draft(
                Correction::PisteGone,
                &Piste::default(),
                "",
                0.0,
                0.0,
                false
            ),
            Err(DraftError::Position)
        );
        let long = "x".repeat(5000);
        let d = draft(
            Correction::Other,
            &Piste::default(),
            &long,
            6.0,
            45.0,
            false,
        )
        .unwrap();
        assert!(d.text.starts_with("Error on the map"));
        assert!(d.text.chars().count() <= MAX_TEXT);
        assert!(d.text.contains(&"x".repeat(MAX_COMMENT)));
        assert!(!d.text.contains(&"x".repeat(MAX_COMMENT + 1)));
        assert_eq!(
            Correction::parse("wrong_oneway"),
            Some(Correction::WrongOneway)
        );
    }
}
