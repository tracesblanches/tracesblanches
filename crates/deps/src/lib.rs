//! Compile-checks every external dependency of the workspace (F1, #1).
//!
//! The manifest lists each entry of `[workspace.dependencies]`, so `Cargo.lock` pins all of
//! them before any crate uses them. The test below touches one item per crate family to
//! prove the chosen features exist and link.

#[cfg(test)]
mod tests {
    #[test]
    fn dependencies_link() {
        let p = geo::Point::new(6.77, 44.86);
        // geo types index directly in rstar: the two crates agree on the rstar version.
        let tree = rstar::RTree::bulk_load(vec![p]);
        assert_eq!(tree.size(), 1);
        let _ = geojson::GeoJson::from(geojson::FeatureCollection {
            bbox: None,
            features: vec![],
            foreign_members: None,
        });
        let mut g = petgraph::Graph::<(), f64>::new();
        let a = g.add_node(());
        let b = g.add_node(());
        g.add_edge(a, b, 1.0);
        assert_eq!(g.edge_count(), 1);
        assert!(rayon::current_num_threads() > 0);
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let v: i64 = conn.query_row("select 40 + 2", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 42);
        let _ = chrono::NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
        let _ = httpdate::fmt_http_date(std::time::SystemTime::UNIX_EPOCH);
        let _ = quick_xml::Reader::from_str("<a/>");
        let _ = csv::ReaderBuilder::new();
        let _ = flate2::Compression::default();
        let _ = zip::CompressionMethod::Deflated;
        let _ = proj4rs::Proj::from_proj_string("+proj=longlat +datum=WGS84").unwrap();
        let _ = pmtiles::TileType::Mvt;
        let _ = tiff::tags::Tag::ImageWidth;
        let _ = sevenz_rust2::Password::empty();
        let _ = webpki_roots::TLS_SERVER_ROOTS.len();
        let _ = rustls::crypto::ring::default_provider();
    }
}
