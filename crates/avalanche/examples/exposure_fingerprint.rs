//! Fingerprints every layer of the exposure model (X25, #159): runs the whole model on the
//! 5 m inputs of each site (`inputs_5m`, as the layer build) and prints the SHA-256 of the
//! raw bytes of every intermediate and final raster, plus the stage timings, so that two
//! builds of the crate can be compared bit for bit on the real case:
//!
//! ```sh
//! cargo run --release -p skimap-avalanche --example exposure_fingerprint -- <site>...
//! ```

use std::time::Instant;

use sha2::{Digest, Sha256};
use skimap_avalanche::exposure::{self, raster::Raster};
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

trait Bytes {
    fn bytes(&self, out: &mut Sha256);
}

impl Bytes for f32 {
    fn bytes(&self, out: &mut Sha256) {
        out.update(self.to_bits().to_le_bytes());
    }
}

impl Bytes for u32 {
    fn bytes(&self, out: &mut Sha256) {
        out.update(self.to_le_bytes());
    }
}

impl Bytes for u8 {
    fn bytes(&self, out: &mut Sha256) {
        out.update([*self]);
    }
}

impl Bytes for bool {
    fn bytes(&self, out: &mut Sha256) {
        out.update([u8::from(*self)]);
    }
}

fn digest<T: Bytes>(data: &[T]) -> String {
    let mut h = Sha256::new();
    for v in data {
        v.bytes(&mut h);
    }
    hex::encode(h.finalize())
}

fn line<T: Bytes>(name: &str, r: &Raster<T>) {
    println!("{name:<28} {}", digest(&r.data));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    for id in std::env::args().skip(1) {
        let s = site::site(&id)?;
        let inputs = exposure::inputs_5m(s, &layout, &dl)?;
        let t = Instant::now();
        let m = exposure::model(&inputs);
        let secs = t.elapsed().as_secs_f64();
        println!("== {id}: {} x {} cells", inputs.dem.w, inputs.dem.h);
        line("dem", &inputs.dem);
        line("canopy", &inputs.canopy);
        line("slope", &m.slope);
        line("windshelter", &m.windshelter);
        for (name, p) in [("frequent", &m.pra_frequent), ("extreme", &m.pra_extreme)] {
            line(&format!("pra_{name}_continuous"), &p.continuous);
            line(&format!("pra_{name}_binary"), &p.binary);
            line(&format!("pra_{name}_release"), &p.release);
        }
        for (name, r) in [
            ("frequent", &m.runout_frequent),
            ("extreme", &m.runout_extreme),
        ] {
            line(&format!("runout_{name}_zdelta"), &r.zdelta);
            line(&format!("runout_{name}_flux"), &r.rout_flux_sum);
            line(&format!("runout_{name}_travel"), &r.fp_travel_angle);
            line(&format!("runout_{name}_counts"), &r.cell_counts);
            println!("runout_{name}_paths_visits    {} {}", r.paths, r.visits);
        }
        println!("{:<28} {}", "ates_valid", digest(&m.ates.valid));
        line("ates_raw", &m.ates.raw);
        line("ates_classes", &m.ates.classes);
        line("ates_coarse_slope", &m.ates.coarse_slope);
        line("ates_class0_score", &m.ates.class0_score);
        line("exposure", &m.exposure);
        eprintln!(
            "{id}: model {secs:.1} s, {} threads, {:?}",
            rayon::current_num_threads(),
            m.timings
        );
    }
    Ok(())
}
