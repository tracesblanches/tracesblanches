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
                + meta["stats"]["estimated"].as_u64().unwrap_or(0)
        );
        assert_eq!(
            lms.len() as u64,
            meta["stats"]["landmarks"].as_u64().unwrap()
        );
        let threshold = meta["stats"]["model"]["threshold"].as_f64();
        for p in &pistes {
            match p.properties.method {
                // Exact: drawn share of at least half the geometry.
                MatchMethod::Exact => assert!(p.properties.score >= 0.5, "{}", p.properties.name),
                // Learnt: probability at the model's threshold (0.9 at least).
                MatchMethod::Ml => {
                    let t = threshold.expect("an ml piste needs the model in the metadata");
                    assert!(t >= skimap_pisteplan::evaluate::MIN_THRESHOLD);
                    assert!(p.properties.score >= t, "{}", p.properties.name);
                }
                // Estimated trace (G1): always uncertain, never on OSM ways.
                MatchMethod::EstimatedPaths | MatchMethod::EstimatedDrawing => {
                    assert_eq!(
                        p.properties.certainty(),
                        skimap_pisteplan::transcript::Certainty::Uncertain
                    );
                    // Written only at the smallest score of the estimation (0.3).
                    assert!(p.properties.score >= 0.3, "{}", p.properties.name);
                    continue;
                }
            }
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
