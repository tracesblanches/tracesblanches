//! Phone path on the real rasters of both sites (`SKR1` of `data/out/<site>/offpiste`,
//! coverage and zones), on the off-piste routes computed by C3. Needs `data/`:
//! `cargo test -p skimap-srndiff --test real_sites -- --ignored`.

use std::path::PathBuf;

use jiff::civil::date;
use skimap_common::geom::LonLat;
use skimap_common::site::Site;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use skimap_core::offpiste::{self, OffpisteGrid};
use skimap_srndiff::field::{Areas, Classes, ElevationDm, SiteZones, SlopeHalfDeg};
use skimap_srndiff::{Layers, Level, Params, assess, profile};
use skimap_zones::layer::ZoneLayer;
use skimap_zones::rules::RuleSet;

fn data() -> PathBuf {
    std::env::var("SKIMAP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"))
}

fn route(site: &Site, name: &str) -> Vec<LonLat> {
    let p = data().join("out").join(site.id).join("routes").join(name);
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).expect("C3 route")).unwrap();
    let f = v["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["properties"]["kind"] == "route")
        .unwrap();
    f["geometry"]["coordinates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| LonLat::new(c[0].as_f64().unwrap(), c[1].as_f64().unwrap()))
        .collect()
}

fn assess_on_site(site: &Site, file: &str) -> skimap_srndiff::Report {
    let out = data().join("out").join(site.id);
    let g = OffpisteGrid::load(&out.join("offpiste")).expect("offpiste rasters");
    let exp: skimap_common::grid::Grid<u8> =
        offpiste::read_raster(&out.join("offpiste").join(offpiste::FILE_EXPOSURE)).unwrap();
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap();
    let net = Areas::new(
        Areas::polygons_from_geojson(&read(out.join("coverage/no_network.geojson"))).unwrap(),
        Some(
            Areas::polygons_from_geojson(&read(out.join("coverage/analysed_area.geojson")))
                .unwrap(),
        ),
    );
    let layer = ZoneLayer::from_geojson_str(&read(out.join("zones/zones.geojson"))).unwrap();
    let rules = RuleSet::from_json_str(&read(out.join("zones/rules.json"))).unwrap();
    let zones = SiteZones {
        layer: &layer,
        rules: &rules,
        at: skimap_common::dates::utc(date(2026, 2, 15).at(9, 0, 0, 0)).unwrap(),
    };
    let elev = ElevationDm(&g.elevation);
    let slope = SlopeHalfDeg(&g.slope);
    let ex = Classes {
        grid: &exp,
        nodata: offpiste::EXPOSURE_NODATA,
    };
    let pistes = Classes {
        grid: &g.pistes,
        nodata: u8::MAX,
    };
    let t = std::time::Instant::now();
    let prof = profile::build(
        &route(site, file),
        &Layers {
            elevation: Some(&elev),
            slope: Some(&slope),
            exposure: Some(&ex),
            pistes: Some(&pistes),
            no_network: Some(&net),
            zones: Some(&zones),
            ..Layers::default()
        },
        None,
    )
    .unwrap();
    let r = assess(&prof, &Params::default(), None);
    eprintln!(
        "{} {file}: {} samples, {:.1} ms\n{}",
        site.id,
        prof.samples.len(),
        t.elapsed().as_secs_f64() * 1000.0,
        r.text_fr(file)
    );
    r
}

#[test]
#[ignore = "needs data/out of both sites"]
fn cervieres_laus_izoard_is_never_easy() {
    let r = assess_on_site(&CERVIERES, "offpiste_laus_izoard.geojson");
    let m = &r.forward.metrics;
    // C3 proof (docs/horspiste.md): 5.75 km, D+ 620 m, 655 cells « exposé », no « très
    // exposé », slope below 30° on every cell.
    assert!((m.length_km - 5.75).abs() < 0.3, "{}", m.length_km);
    assert!((m.ascent_m - 620.0).abs() < 60.0, "{}", m.ascent_m);
    assert!(m.exposed_km > 2.0, "{}", m.exposed_km);
    assert_eq!(m.high_exposed_km, 0.0);
    assert!(m.max_terrain_deg < 30.5, "{}", m.max_terrain_deg);
    assert!(r.forward.rating.level >= Level::Moyen);
    assert!(r.reverse.rating.level >= Level::Moyen);
    assert!(m.zones_unknown_km < m.length_km);
}

#[test]
#[ignore = "needs data/out of both sites"]
fn chapelle_risoux_reports_the_appb() {
    let r = assess_on_site(&CHAPELLE_DES_BOIS, "offpiste_risoux_fevrier.geojson");
    let m = &r.forward.metrics;
    // C3 proof: 12.1 km, D+ 277 m; in February the route stays on marked itineraries
    // inside the APPB, where leaving them is forbidden.
    assert!((m.length_km - 12.1).abs() < 0.6, "{}", m.length_km);
    assert!(m.leave_routes_forbidden_km > 0.0);
    assert!(
        r.forward
            .advice
            .iter()
            .any(|a| a.text.contains("Sortie des itinéraires balisés interdite"))
    );
    assert!(m.no_network_km.is_some());
}
