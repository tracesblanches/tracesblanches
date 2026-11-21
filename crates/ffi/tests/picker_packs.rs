//! Area picker on the real catalogues (#175): the census areas of the app resource
//! (`android/app/src/main/assets/catalog.json`) against the sites published by the
//! production server (`tests/data/server-catalog-20261121.json`: `/v1/catalog.json` of
//! 2026-11-21, 268 sites, the `record` and `start_ranking` of each site left out). Each
//! area points to its own pack, never to the pack of a neighbour whose box contains it.

use std::collections::BTreeMap;

use skimap_ffi::sync::{ServerSite, parse_server_catalog};
use skimap_ffi::{Bounds, Catalog, Domain, PackEntry, PackStatus};

fn catalogs() -> (Catalog, Vec<ServerSite>) {
    let app = include_str!("../../../android/app/src/main/assets/catalog.json");
    let server = include_str!("data/server-catalog-20261121.json");
    (
        Catalog::parse(app).expect("app catalogue"),
        parse_server_catalog(server.to_owned()).expect("server catalogue"),
    )
}

/// The pack of `site` installed on the phone, as `list_packs` lists it.
fn installed(site: &ServerSite) -> PackEntry {
    PackEntry {
        dir: format!("/packs/{}", site.site_id),
        site_id: site.site_id.clone(),
        site_name: site.name.clone(),
        pack_version: site.latest_version.clone(),
        error: None,
        bounds: Some(Bounds {
            west: site.bbox[0],
            south: site.bbox[1],
            east: site.bbox[2],
            north: site.bbox[3],
        }),
    }
}

#[test]
fn each_area_points_to_its_own_pack() {
    let (catalog, server) = catalogs();
    assert_eq!(server.len(), 268);
    let by_id: BTreeMap<&str, &ServerSite> =
        server.iter().map(|s| (s.site_id.as_str(), s)).collect();

    // Nothing installed: every area of the census is published, by its own site (same
    // name), and no two areas share a pack.
    let all = catalog.domains(Vec::new(), server.clone());
    assert_eq!(all.len(), 219);
    let mut owners: BTreeMap<String, Vec<&Domain>> = BTreeMap::new();
    let mut wrong = Vec::new();
    for d in &all {
        let Some(site) = d.server_site.as_deref() else {
            wrong.push(format!("{} ({}): no site", d.name, d.id));
            continue;
        };
        let s = by_id[site];
        if s.name != d.name || d.status != PackStatus::Available {
            wrong.push(format!("{} ({}) -> {} ({})", d.name, d.id, s.name, site));
        }
        assert_eq!(d.download_bytes, s.first_download_bytes, "{}", d.id);
        owners.entry(site.to_owned()).or_default().push(d);
    }
    let shared: Vec<_> = owners
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(s, v)| format!("{s}: {}", v.len()))
        .collect();
    println!(
        "areas on another pack: {}, packs of several areas: {}",
        wrong.len(),
        shared.len()
    );
    assert!(wrong.is_empty(), "{} areas: {wrong:#?}", wrong.len());
    assert!(shared.is_empty(), "{shared:#?}");

    // Each of the 268 packs installed alone: installed is only the area of that pack;
    // the actions of no other area reach it.
    let mut leaks = Vec::new();
    let mut covered = 0usize;
    for s in &server {
        let v = catalog.domains(vec![installed(s)], server.clone());
        for d in &v {
            let own = d.server_site.as_deref() == Some(s.site_id.as_str());
            let says_installed = d.status == PackStatus::Installed || d.pack_dir.is_some();
            if own != says_installed {
                leaks.push(format!("{} installed: {} ({})", s.site_id, d.name, d.id));
            }
            // A neighbour inside the box of that pack says it apart, nothing more.
            let [w, so, e, n] = s.bbox[..] else {
                panic!("bbox")
            };
            let inside = d.lon >= w && d.lon <= e && d.lat >= so && d.lat <= n;
            let expected = (!own && inside).then(|| s.name.clone());
            if d.covered_by != expected {
                leaks.push(format!(
                    "{}: {} covered_by {:?}",
                    s.site_id, d.id, d.covered_by
                ));
            }
            covered += usize::from(expected.is_some());
        }
    }
    println!("areas said inside a neighbour's pack (one pack at a time): {covered}");
    assert!(leaks.is_empty(), "{} leaks: {leaks:#?}", leaks.len());
    assert!(covered > 0);

    // Every pack installed: each area opens its own.
    let packs: Vec<PackEntry> = server.iter().map(installed).collect();
    for d in catalog.domains(packs, server.clone()) {
        let site = d.server_site.clone().expect("site");
        assert_eq!(d.status, PackStatus::Installed, "{}", d.id);
        assert_eq!(d.pack_dir, Some(format!("/packs/{site}")), "{}", d.id);
        assert_eq!(d.covered_by, None, "{}", d.id);
    }
}
