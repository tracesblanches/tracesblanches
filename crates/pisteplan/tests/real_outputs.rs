//! The phone-side reader on the real outputs of both sites (`--ignored`: needs
//! `data/out/<site>/plan_pistes/`, built by the `plan_pistes` example).

use std::path::PathBuf;

use skimap_pisteplan::transcript::{MatchMethod, read_landmarks, read_pistes};

fn dir(site: &str) -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    root.join("out").join(site).join("plan_pistes")
}

#[test]
#[ignore = "needs data/out/<site>/plan_pistes"]
fn transcriptions_are_read_back() {
    for site in ["cervieres", "chapelle-des-bois"] {
        let d = dir(site);
        let pistes =
            read_pistes(&std::fs::read_to_string(d.join("plan_pistes.geojson")).unwrap()).unwrap();
        let lms =
            read_landmarks(&std::fs::read_to_string(d.join("plan_landmarks.geojson")).unwrap())
                .unwrap();
        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(d.join("plan_pistes.meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            pistes.len() as u64,
            meta["stats"]["transcribed"].as_u64().unwrap()
        );
        assert_eq!(
            lms.len() as u64,
            meta["stats"]["landmarks"].as_u64().unwrap()
        );
        for p in &pistes {
            assert_eq!(p.properties.method, MatchMethod::Exact);
            assert!(p.properties.score >= 0.5, "{}", p.properties.name);
            assert!(!p.properties.osm_way_ids.is_empty());
            assert_eq!(p.geometry["type"], "MultiLineString");
        }
        println!(
            "{site}: {} pistes, {} landmarks read",
            pistes.len(),
            lms.len()
        );
    }
}
