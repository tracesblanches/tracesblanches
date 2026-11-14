//! Checks of the built zone layers of the two real sites against research reports 04,
//! 05 and 06. They read `data/out/<site>/zones/` and are ignored by default:
//!
//! ```sh
//! cargo run --release -p skimap-zones --features pipeline --example build_zones
//! cargo test -p skimap-zones --features pipeline --test real_sites -- --ignored
//! ```

use std::path::PathBuf;

use skimap_common::geom::BBox;
use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS, Site};
use skimap_zones::geom::{bbox_polygon, geodesic_area_m2};
use skimap_zones::{Topic, ZoneKind, ZoneLayer};

fn data_dir() -> PathBuf {
    std::env::var("SKIMAP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data")))
}

fn layer(site: &Site) -> ZoneLayer {
    let p = data_dir()
        .join("out")
        .join(site.id)
        .join("zones")
        .join("zones.geojson");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    ZoneLayer::from_geojson_str(&s).unwrap()
}

fn area(l: &ZoneLayer, id: &str) -> f64 {
    l.get(id)
        .unwrap_or_else(|| panic!("zone {id} missing"))
        .props
        .area_ha
}

fn assert_close(what: &str, got: f64, want: f64, rel: f64) {
    println!("{what}: {got:.1} (report {want})");
    assert!((got / want - 1.0).abs() <= rel, "{what}: {got} vs {want}");
}

fn bbox_ha(b: BBox) -> f64 {
    geodesic_area_m2(&geo::MultiPolygon::new(vec![bbox_polygon(b, 50)])) / 1e4
}

#[test]
#[ignore = "needs data/out/chapelle-des-bois/zones"]
fn chapelle_des_bois_layer_matches_reports() {
    let l = layer(&CHAPELLE_DES_BOIS);
    // Report 05: APPB areas inside the box.
    assert_close(
        "APPB Haut-Doubs FR3801178 (ha)",
        area(&l, "inpn:FR3801178"),
        1123.0,
        0.01,
    );
    assert_close(
        "APPB Haut-Jura FR3800988 (ha)",
        area(&l, "inpn:FR3800988"),
        938.0,
        0.01,
    );
    assert_close(
        "APPB Forêt du Paradis FR3800137 (ha)",
        area(&l, "inpn:FR3800137"),
        637.0,
        0.01,
    );
    assert_eq!(
        l.get("inpn:FR3801178").unwrap().props.designated.as_deref(),
        Some("2025-12-31")
    );
    // Report 05: site inscrit of Chapelle-des-Bois, 3 971 ha, 02/05/1974 (DREAL list).
    let si = l
        .zones
        .iter()
        .find(|z| z.props.name == "Site de Chapelle-des-Bois")
        .unwrap();
    assert_eq!(si.kind(), ZoneKind::SiteInscrit);
    assert!(!si.props.clipped);
    assert_eq!(si.props.designated.as_deref(), Some("1974-05-02"));
    assert_close(
        "site inscrit Chapelle-des-Bois (ha)",
        si.props.area_ha,
        3971.0,
        0.005,
    );
    // Site classé Haute vallée de la Saine (report 05: 2.3 % of the box).
    let share = area(&l, "inpn:FR3201483") / bbox_ha(CHAPELLE_DES_BOIS.bbox) * 100.0;
    assert_close("Haute vallée de la Saine (% of the box)", share, 2.3, 0.05);
    // Report 05: PNR du Haut-Jura covers 80 % of the box.
    let share = area(&l, "inpn:FR8000015") / bbox_ha(CHAPELLE_DES_BOIS.bbox) * 100.0;
    assert_close("PNR Haut-Jura (% of the box)", share, 80.0, 0.01);
    // Report 05: 6 Biodiv'sports areas; report 04: "Grand tétras - hiver", Dec-Jun.
    let bs: Vec<_> = l
        .zones
        .iter()
        .filter(|z| z.kind() == ZoneKind::BiodivSports)
        .collect();
    assert_eq!(bs.len(), 6);
    let grouse = l.get("biodiv:1183").unwrap();
    assert_eq!(grouse.props.name, "Grand tétras - hiver");
    assert_eq!(
        grouse.props.months.unwrap().numbers(),
        vec![1, 2, 3, 4, 5, 6, 12]
    );
    assert!(grouse.props.practices.iter().any(|p| p == "terrestre"));
    // Report 06: French part of the box 172.7 km² (Lambert-93 planar, ~0.2 % below the
    // geodesic area at this latitude).
    assert_close(
        "French part (km²)",
        area(&l, "country:fr") / 100.0,
        172.7,
        0.005,
    );
    assert!(l.get("dept:25").is_some() && l.get("dept:39").is_some());
    // Swiss side: Vaud, Le Chenit, Le Noirmont federal hunting reserve (n° 30).
    assert!(l.get("canton:vd").is_some() && l.get("commune:ch-5872").is_some());
    let jbg: Vec<_> = l
        .zones
        .iter()
        .filter(|z| z.kind() == ZoneKind::DistrictFranc)
        .collect();
    assert!(
        !jbg.is_empty()
            && jbg
                .iter()
                .all(|z| z.props.name.contains("n° 30 Le Noirmont"))
    );
    // No national park, no nature reserve (report 05).
    for k in [
        ZoneKind::ParcNational,
        ZoneKind::ReserveNaturelleNationale,
        ZoneKind::ReserveNaturelleRegionale,
    ] {
        assert!(l.zones.iter().all(|z| z.kind() != k), "{k:?}");
    }
    println!("{} zones", l.zones.len());
}

#[test]
#[ignore = "needs data/out/cervieres/zones"]
fn cervieres_layer_matches_reports() {
    let l = layer(&CERVIERES);
    // Report 05: no APPB, no national park, no nature reserve.
    for k in [
        ZoneKind::Appb,
        ZoneKind::ParcNational,
        ZoneKind::ReserveNaturelleNationale,
        ZoneKind::ReserveNaturelleRegionale,
    ] {
        assert!(l.zones.iter().all(|z| z.kind() != k), "{k:?}");
    }
    // SPR of Cervières, about 3.3 km², contains the village.
    let spr = l.zones.iter().find(|z| z.kind() == ZoneKind::Spr).unwrap();
    assert!(spr.props.name.contains("Cervières"));
    assert_close("SPR Cervières (km²)", spr.props.area_ha / 100.0, 3.3, 0.03);
    assert!(spr.contains(skimap_common::geom::LonLat::new(6.7230, 44.8640)));
    // Sites classés Casse Déserte (0.4 % of the box) and Clarée (0.2 %); site inscrit
    // Ville vieille de Briançon.
    let b = bbox_ha(CERVIERES.bbox);
    assert_eq!(
        l.get("inpn:FR3200828").unwrap().kind(),
        ZoneKind::SiteClasse
    );
    assert_close(
        "Casse Déserte (% of the box)",
        area(&l, "inpn:FR3200828") / b * 100.0,
        0.4,
        0.15,
    );
    assert_close(
        "Clarée (% of the box)",
        area(&l, "inpn:FR3200843") / b * 100.0,
        0.2,
        0.25,
    );
    assert!(
        l.zones
            .iter()
            .any(|z| z.kind() == ZoneKind::SiteInscrit && z.props.name.contains("Briançon"))
    );
    // PNR du Queyras 28.8 %, Natura 2000 Rochebrune-Izoard 54 % of the box.
    assert_close(
        "PNR Queyras (% of the box)",
        area(&l, "inpn:FR8000002") / b * 100.0,
        28.8,
        0.01,
    );
    assert_close(
        "Natura 2000 FR9301503 (% of the box)",
        area(&l, "inpn:FR9301503") / b * 100.0,
        54.0,
        0.01,
    );
    // Réserve biologique du Bois des Ayes, 0.9 %.
    assert_close(
        "RB Bois des Ayes (% of the box)",
        area(&l, "inpn:FR2300113") / b * 100.0,
        0.9,
        0.05,
    );
    // Report 05: 10 Biodiv'sports areas, one "Tétras lyre - hiver" (terrestrial).
    assert_eq!(
        l.zones
            .iter()
            .filter(|z| z.kind() == ZoneKind::BiodivSports)
            .count(),
        10
    );
    assert!(
        l.zones
            .iter()
            .any(|z| z.props.name == "Tétras lyre - hiver")
    );
    // Report 06: French part of the box 225.6 km² (Lambert-93 planar).
    assert_close(
        "French part (km²)",
        area(&l, "country:fr") / 100.0,
        225.6,
        0.005,
    );
    assert!(l.get("commune:05027").is_some() && l.get("commune:05007").is_some());
    assert!(l.zones.iter().all(|z| z.kind() != ZoneKind::Canton));
    println!("{} zones", l.zones.len());
}

#[test]
#[ignore = "needs data/out/<site>/zones"]
fn every_rule_of_the_sites_is_sourced() {
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let dir = data_dir().join("out").join(site.id).join("zones");
        let rules = skimap_zones::RuleSet::from_json_str(
            &std::fs::read_to_string(dir.join("rules.json")).unwrap(),
        )
        .unwrap();
        let l = layer(site);
        let mut applied = 0;
        for r in &rules.rules {
            assert!(r.primary_source().url.starts_with("https://"));
            if r.matching_zones(&l).next().is_some() {
                applied += 1;
            }
        }
        let topics: std::collections::BTreeSet<Topic> = rules
            .rules
            .iter()
            .filter(|r| r.matching_zones(&l).next().is_some())
            .map(|r| r.topic)
            .collect();
        println!(
            "{}: {applied} of {} rules apply, topics {topics:?}",
            site.id,
            rules.rules.len()
        );
        assert!(topics.contains(&Topic::Bivouac) && topics.contains(&Topic::Fire));
    }
}
