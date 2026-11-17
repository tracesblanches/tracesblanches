//! Golden fingerprints of the assistant on the reference data set (Q2, #102): the
//! generated sentences of the suggestions (A5) and of the equipment list (A6), and the
//! figures behind them, for both sites, without weather (the weather sources are personal
//! use only and never versioned).
//!
//! References: `fixtures/golden/assist/<site>.json`; update with
//! `SKIMAP_BLESS=1 cargo test -p skimap-assist --test golden` and review the diff.
//!
//! Tolerances: fixed layers, fixed requests, deterministic search: sentences and counts
//! exact, figures `Tol::FLOAT`. The figures cross-check report 06 (5.1 km without any
//! network on Le Laus → Les Fonts) and the C1 graph limits (138 of 189 nodes out of reach
//! from Les Fonts).

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use fixtures::golden::{Golden, Tol};
use skimap_assist::equipment;
use skimap_assist::inputs::{Paths, SiteData};
use skimap_assist::suggest::{self, Request};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use skimap_core::eta::EtaModel;
use skimap_core::routing::{Router, Style};

fn data(site: &Site) -> SiteData {
    SiteData::load(&DataLayout::new(fixtures::data()), site, &Paths::default()).unwrap()
}

fn request(
    start: LonLat,
    name: &str,
    at: &str,
    min: f64,
    style: Style,
    back: Option<u32>,
) -> Request {
    Request {
        label: format!("{min} min"),
        start,
        start_name: name.into(),
        start_time: chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M").unwrap(),
        budget_min: min,
        style,
        return_before: back.map(|h| chrono::NaiveTime::from_hms_opt(h, 0, 0).unwrap()),
        max_difficulty: None,
        max_results: 3,
    }
}

fn suggestions(d: &SiteData, r: &Request, key: &str, k: &mut Golden) {
    let s = suggest::suggest(d, r);
    k.lines(&format!("{key}.text"), &s.text.join("\n"))
        .count(&format!("{key}.suggestions"), s.suggestions.len());
    for (i, x) in s.suggestions.iter().enumerate() {
        k.num(&format!("{key}.{i}.length_km"), x.length_km, Tol::FLOAT)
            .num(&format!("{key}.{i}.eta_min"), x.eta_min, Tol::FLOAT);
    }
}

/// Layers the fixtures do not have (said by the assistant), without the local paths.
fn missing(d: &SiteData, k: &mut Golden) {
    let names: Vec<&str> = d
        .missing
        .iter()
        .map(|m| m.split(" : ").next().unwrap_or(m))
        .collect();
    k.lines("missing_layers", &names.join("\n"));
}

#[test]
fn chapelle_des_bois() {
    let d = data(&CHAPELLE_DES_BOIS);
    let mut k = Golden::new("assist/chapelle-des-bois");
    missing(&d, &mut k);
    let village = LonLat::new(6.1125, 46.6032);
    suggestions(
        &d,
        &request(
            village,
            "village",
            "2026-02-22T10:00",
            120.0,
            Style::Skating,
            Some(16),
        ),
        "suggest.village_120min_skating",
        &mut k,
    );
    suggestions(
        &d,
        &request(
            village,
            "village",
            "2027-01-15T13:30",
            60.0,
            Style::Classic,
            None,
        ),
        "suggest.village_60min_classic",
        &mut k,
    );
    k.check();
}

#[test]
fn cervieres() {
    let d = data(&CERVIERES);
    let mut k = Golden::new("assist/cervieres");
    missing(&d, &mut k);
    suggestions(
        &d,
        &request(
            LonLat::new(6.8161, 44.8448),
            "Les Fonts",
            "2026-01-09T14:30",
            90.0,
            Style::Classic,
            None,
        ),
        "suggest.les_fonts_90min_classic",
        &mut k,
    );
    let g = d.graph.as_ref().unwrap();
    let route = Router::new(g, EtaModel::new(Style::Classic))
        .route(
            LonLat::new(6.725_006, 44.858_940),
            LonLat::new(6.8157, 44.84465),
            500.0,
        )
        .unwrap()
        .polyline(g);
    let o = equipment::Outing {
        label: "Le Laus → Les Fonts".into(),
        start: chrono::NaiveDate::from_ymd_opt(2026, 1, 5)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap(),
        duration_min: 135.0,
        route,
        offpiste: false,
        kick_unreliable: false,
    };
    let e = equipment::equipment(&d, &o);
    k.lines("equipment.laus_fonts.text", &e.text)
        .num(
            "equipment.laus_fonts.no_network_km",
            e.facts.no_network_km,
            Tol::FLOAT,
        )
        .count("equipment.laus_fonts.items", e.items.len());
    k.check();
}
