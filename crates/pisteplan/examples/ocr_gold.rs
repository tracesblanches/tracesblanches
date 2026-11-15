//! OCR read rate on the Cervières plan, a picture without text layer: the map labels
//! transcribed by hand from the plan (gold list below, 15/11/2026) against the OCR of the
//! pipeline (text-enhanced image, 800 px tiles). A label is read when an OCR line on the
//! map has a folded similarity of at least 0.8 with it (exact: 1.0).
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example ocr_gold`

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_pisteplan::build::{OCR_OVERLAP, OCR_TILE, OCR_TOPHAT_GAIN, OCR_TOPHAT_R};
use skimap_pisteplan::labels::{fold, similarity, split_elevation};
use skimap_pisteplan::ocr;

/// Map labels of the Cervières plan, read by hand (names only, elevations dropped).
const GOLD: &[&str] = &[
    "Cime de Saurel",
    "Refuge Mautino",
    "Lago Nero",
    "Le Chenaillet",
    "Lac Gignoux",
    "Gondran",
    "Lac des Sarailles",
    "Le Lac Noir",
    "Bergerie de Peyre Moutte",
    "Les Fraches",
    "Col de Bousson",
    "Lac des Chiens",
    "Cabane des Douaniers",
    "Col de Chabaud",
    "La Chau",
    "Les Huguets",
    "Rif Tord",
    "Le Bourget",
    "Mur des Aittes",
    "Fort des Aittes",
    "Bois de Rebanc",
    "L'Alp du Pied",
    "Terre Rouge",
    "Cervières",
    "Prafauchier",
    "Bois du Bourget",
    "Lasseron",
    "Turges de Peyron",
    "Le Laus",
    "Col du Lasseron",
    "Cime de la Charvie",
    "La Lauzette",
    "Col de Prafauchier",
    "Les Chalps",
    "Le Bourgea",
    "Le Cloitet",
    "Les Fonts",
    "Refuge des Fonts",
    "Lac de l'Étoile",
    "Lac de la Madeleine",
    "Lac des Cordes",
    "Pic du Villon Crouzet",
    "Col des Marsailles",
    "Col de Chaude Maison",
    "Turges de la Suffie",
    "Bois des Talias",
    "Bois des Loubatières",
    "Petit Peygu",
    "Col des Peygus",
    "Grand Peygu",
    "Chalets d'Izoard",
    "L'Arpelin",
    "Refuge Napoléon",
    "Le Col Perdu",
    "Cascade des Oules",
    "Col d'Izoard",
    "Col des Ourdes",
    "Casse des Oules",
    "Grand Pic de Rochebrune",
    "Col des Portes",
    "Col de Péas",
    "Italie",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    // The site's plan as the pipeline reads it (catalogue, automatic sheet).
    let planet = skimap_osm::extract::load_planet(&dl)?;
    let e = ocr::engine(&dl)?;
    let site = skimap_common::site::site("cervieres")?;
    let (reg, _) = skimap_pisteplan::build::register_site(site, &cfg.layout(), &dl, &planet, &e)?;
    let (spec, inp) = (&reg.spec, &reg.input);
    let enhanced = ocr::enhance_text(&inp.image, OCR_TOPHAT_R, OCR_TOPHAT_GAIN);
    let lines: Vec<String> =
        ocr::read_lines_tiled(&e, &enhanced, OCR_TILE, OCR_OVERLAP, inp.pt_per_px)?
            .into_iter()
            .filter(|l| {
                let c = l.center();
                spec.on_map(c[0], c[1])
            })
            .map(|l| fold(&split_elevation(&l.text).0))
            .collect();
    let best = |g: &str| {
        let f = fold(g);
        lines
            .iter()
            .map(|l| similarity(l, &f).max(if l.contains(&f) { 1.0 } else { 0.0 }))
            .fold(0.0, f64::max)
    };
    let (mut read, mut exact) = (0, 0);
    for g in GOLD {
        let s = best(g);
        read += usize::from(s >= 0.8);
        exact += usize::from(s >= 1.0);
        println!("{s:4.2} {g}");
    }
    println!(
        "\n{} gold labels, {} OCR lines on the map: read (similarity >= 0.8) {} = {:.1} %, exact {} = {:.1} %",
        GOLD.len(),
        lines.len(),
        read,
        100.0 * read as f64 / GOLD.len() as f64,
        exact,
        100.0 * exact as f64 / GOLD.len() as f64
    );
    Ok(())
}
