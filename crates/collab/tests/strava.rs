//! Import of a Strava export archive built like Strava's (V15 #78): `activities.csv` with
//! its many columns, originals in `activities/` as GPX, gzipped TCX, gzipped FIT; other
//! sports skipped; a manual activity without file; a second import adds nothing.

#![cfg(feature = "phone")]

use std::io::Write;

use chrono::{DateTime, Duration, Utc};
use skimap_collab::strava_export::{ImportError, import};
use skimap_collab::track::{OBSERVATION_FILE, OUTING_FILE, Outing, Point};
use skimap_core::calibration::OutingObs;
use skimap_core::routing::Style;

fn points(start: &str, n: i64, lat0: f64) -> Vec<Point> {
    let t0: DateTime<Utc> = start.parse().unwrap();
    (0..n)
        .map(|i| Point {
            lon: 6.1,
            lat: lat0 + i as f64 * 3.0 / 111_195.0,
            ele: Some(1100.0 + i as f64 * 0.03),
            t: Some(t0 + Duration::seconds(i)),
        })
        .collect()
}

fn gpx(p: &[Point]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx creator=\"StravaGPX\" version=\"1.1\" xmlns=\"http://www.topografix.com/GPX/1/1\"><trk><name>Matin</name><type>nordic_skiing</type><trkseg>",
    );
    for q in p {
        s.push_str(&format!(
            "<trkpt lat=\"{}\" lon=\"{}\"><ele>{:.1}</ele><time>{}</time></trkpt>",
            q.lat,
            q.lon,
            q.ele.unwrap(),
            q.t.unwrap().format("%Y-%m-%dT%H:%M:%SZ")
        ));
    }
    s.push_str("</trkseg></trk></gpx>");
    s
}

fn tcx(p: &[Point]) -> String {
    let mut s = String::from(
        "   <?xml version=\"1.0\" encoding=\"UTF-8\"?><TrainingCenterDatabase xmlns=\"http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2\"><Activities><Activity Sport=\"Other\"><Lap><Track>",
    );
    for q in p {
        s.push_str(&format!(
            "<Trackpoint><Time>{}</Time><Position><LatitudeDegrees>{}</LatitudeDegrees><LongitudeDegrees>{}</LongitudeDegrees></Position><AltitudeMeters>{}</AltitudeMeters></Trackpoint>",
            q.t.unwrap().to_rfc3339(),
            q.lat,
            q.lon,
            q.ele.unwrap()
        ));
    }
    s.push_str("</Track></Lap></Activity></Activities></TrainingCenterDatabase>");
    s
}

fn gz(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn archive(path: &std::path::Path) {
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let o = zip::write::SimpleFileOptions::default();
    let csv = "Activity ID,Activity Date,Activity Name,Activity Type,Activity Description,Elapsed Time,Distance,Max Heart Rate,Relative Effort,Commute,Activity Private Note,Activity Gear,Filename,Athlete Weight,Bike Weight,Elapsed Time,Moving Time,Distance\n\
101,\"Jan 5, 2026, 9:00:00 AM\",Risoux,Nordic Ski,,1500,4.5,,,false,,,activities/101.gpx,,,1500,1500,4500\n\
102,\"Jan 6, 2026, 9:00:00 AM\",\"Tour, du lac\",Backcountry Ski,\"Belle trace, neige poudreuse\",1500,4.5,,,false,,,activities/102.tcx.gz,,,1500,1500,4500\n\
103,\"Jan 7, 2026, 9:00:00 AM\",Montre,Nordic Ski,,1500,4.5,,,false,,,activities/103.fit.gz,,,1500,1500,4500\n\
104,\"Jan 8, 2026, 9:00:00 AM\",Course,Run,,1500,4.5,,,false,,,activities/104.gpx,,,1500,1500,4500\n\
105,\"Jan 9, 2026, 9:00:00 AM\",Piste noire,Alpine Ski,,1500,4.5,,,false,,,activities/105.gpx,,,1500,1500,4500\n\
106,\"Jan 10, 2026, 9:00:00 AM\",Saisie à la main,Nordic Ski,,3600,10,,,false,,,,,,3600,3600,10000\n\
107,\"Jan 11, 2026, 9:00:00 AM\",Fichier abîmé,Nordic Ski,,60,1,,,false,,,activities/107.fit.gz,,,60,60,1000\n";
    z.start_file("export_12345/activities.csv", o).unwrap();
    z.write_all(csv.as_bytes()).unwrap();
    z.start_file("export_12345/profile.csv", o).unwrap();
    z.write_all(b"Athlete ID,Email Address\n1,someone@example.org\n")
        .unwrap();
    let files: Vec<(&str, Vec<u8>)> = vec![
        (
            "101.gpx",
            gpx(&points("2026-01-05T09:00:00Z", 1500, 46.60)).into_bytes(),
        ),
        (
            "102.tcx.gz",
            gz(tcx(&points("2026-01-06T09:00:00Z", 1500, 46.61)).as_bytes()),
        ),
        (
            "103.fit.gz",
            gz(&skimap_collab::fit::write_for_tests(&points(
                "2026-01-07T09:00:00Z",
                1500,
                46.62,
            ))),
        ),
        (
            "104.gpx",
            gpx(&points("2026-01-08T09:00:00Z", 100, 46.63)).into_bytes(),
        ),
        (
            "105.gpx",
            gpx(&points("2026-01-09T09:00:00Z", 100, 46.64)).into_bytes(),
        ),
        ("107.fit.gz", gz(b"\x0e\x10garbage")),
    ];
    for (name, data) in files {
        z.start_file(format!("export_12345/activities/{name}"), o)
            .unwrap();
        z.write_all(&data).unwrap();
    }
    z.finish().unwrap();
}

#[test]
fn archive_imported_on_the_phone_and_learned() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("export_12345.zip");
    archive(&zip);
    let out = dir.path().join("outings");
    let r = import(&zip, &out, Style::Classic).unwrap();
    assert_eq!(r.imported, vec!["strava-101", "strava-102", "strava-103"]);
    assert_eq!((r.already, r.without_track, r.observations), (0, 1, 3));
    assert_eq!(r.other_types.get("Run"), Some(&1));
    assert_eq!(r.other_types.get("Alpine Ski"), Some(&1));
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert!(r.errors[0].contains("107.fit.gz"));
    for id in ["strava-101", "strava-102", "strava-103"] {
        let o: Outing =
            serde_json::from_str(&std::fs::read_to_string(out.join(id).join(OUTING_FILE)).unwrap())
                .unwrap();
        assert!(
            (o.distance_m - 4497.0).abs() < 5.0,
            "{id}: {}",
            o.distance_m
        );
        assert!((o.moving_s - 1499.0).abs() < 1.0, "{id}");
        assert_eq!(o.source, "strava");
        let obs: OutingObs = serde_json::from_str(
            &std::fs::read_to_string(out.join(id).join(OBSERVATION_FILE)).unwrap(),
        )
        .unwrap();
        assert!((obs.moving_total_s() - 1499.0).abs() < 1.0);
    }
    let o: Outing = serde_json::from_str(
        &std::fs::read_to_string(out.join("strava-102").join(OUTING_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(o.name, "Tour, du lac");
    assert_eq!(o.activity, "Backcountry Ski");
    // Nothing of the profile (e-mail) or the descriptions is copied.
    let all: String = walk(&out);
    assert!(!all.contains("someone@example.org") && !all.contains("poudreuse"));
    // Again: nothing new.
    let r = import(&zip, &out, Style::Classic).unwrap();
    assert_eq!((r.imported.len(), r.already), (0, 3));
}

fn walk(d: &std::path::Path) -> String {
    let mut s = String::new();
    for e in std::fs::read_dir(d).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            s.push_str(&walk(&p));
        } else {
            s.push_str(&std::fs::read_to_string(&p).unwrap());
        }
    }
    s
}

#[test]
fn not_a_strava_archive() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.zip");
    std::fs::write(&p, b"not a zip").unwrap();
    assert!(matches!(
        import(&p, dir.path(), Style::Classic),
        Err(ImportError::Zip(_))
    ));
    let f = std::fs::File::create(&p).unwrap();
    let mut z = zip::ZipWriter::new(f);
    z.start_file("photos/a.jpg", zip::write::SimpleFileOptions::default())
        .unwrap();
    z.finish().unwrap();
    assert!(matches!(
        import(&p, dir.path(), Style::Classic),
        Err(ImportError::NotStrava)
    ));
}
