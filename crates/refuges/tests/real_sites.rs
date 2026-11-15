//! Real builds of the refuges layer (network, data directory): ignored by default.
//!
//! ```sh
//! SKIMAP_DATA_DIR=$PWD/data cargo test -p skimap-refuges --test real_sites -- --ignored --nocapture
//! ```

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::SITES;
use skimap_refuges::{FICHES_FILE, privacy};

/// Builds both sites, then checks that no written file of the layer holds personal data
/// (phone outside the checked `phones` lists, e-mail, titled name, comment text or
/// author, custodian phone) and that every card has a date and a refuges.info link.
#[test]
#[ignore = "network and data directory"]
fn both_sites_without_personal_data() {
    let cfg = Config::from_env().unwrap();
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg).unwrap();
    for site in SITES {
        let out = skimap_refuges::build(site, &layout, &dl).unwrap();
        for name in [
            FICHES_FILE,
            "refuges_info.geojson",
            "osm.geojson",
            "camptocamp.geojson",
            "links.json",
        ] {
            let v: serde_json::Value =
                serde_json::from_slice(&std::fs::read(out.dir.join(name)).unwrap()).unwrap();
            let found = privacy::findings(&v);
            assert!(found.is_empty(), "{} {name}: {found:#?}", site.id);
        }
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.dir.join(FICHES_FILE)).unwrap()).unwrap();
        let cards = v["features"].as_array().unwrap();
        assert!(!cards.is_empty());
        for c in cards {
            let p = &c["properties"];
            assert!(p["refuges_info_url"].is_string(), "{p}");
            assert!(p["info_date"].is_string(), "{} without date", p["name"]);
        }
        println!(
            "{}: {}",
            site.id,
            serde_json::to_string_pretty(&out.stats["fiches"]).unwrap()
        );
    }
}

/// Scheduled refresh: a forced run journals the changes and dates every card; a second
/// run the same day is not due and changes nothing (idempotent).
#[test]
#[ignore = "network and data directory"]
fn refresh_is_idempotent() {
    let cfg = Config::from_env().unwrap();
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg).unwrap();
    let now = chrono::Utc::now();
    for site in SITES {
        let first = skimap_refuges::refresh(site, &layout, &dl, now, true).unwrap();
        assert!(first.ran && first.error.is_none());
        let again = skimap_refuges::refresh(site, &layout, &dl, now, false).unwrap();
        assert!(!again.ran, "not due the same day");
        let dir = layout.layer_dir(site, skimap_refuges::LAYER).unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join(FICHES_FILE)).unwrap()).unwrap();
        for c in v["features"].as_array().unwrap() {
            assert!(c["properties"]["checked_at"].is_string());
            assert!(c["properties"]["changed_at"].is_string());
        }
        let status: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("status.json")).unwrap()).unwrap();
        assert!(status["last_ok"].is_string() && status["error"].is_null());
        println!(
            "{}: {} changements au premier passage",
            site.id, first.changes
        );
    }
}
