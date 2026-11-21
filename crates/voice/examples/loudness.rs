//! Loudness of the clips of voice layers (#179): integrated loudness (BS.1770-4, LUFS) and
//! true peak (dBTP) of every clip decoded as the phone decodes it (Ogg/Opus, 48 kHz).
//!
//! ```sh
//! cargo run --release -p skimap-voice --example loudness -- <voice layer dir>...
//! ```
//!
//! Prints, per layer, the distribution (min, 10 %, median, 90 %, max) of the loudness,
//! of the true peak and of the peak-to-loudness ratio, and the clips outside the target
//! (`skimap_voice::loudness::Target::default`). `--json <file>`: every clip.

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::path::PathBuf;

use skimap_voice::audio::Ffmpeg;
use skimap_voice::loudness::Target;
use skimap_voice::pack::measure_layer;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn quantiles(mut v: Vec<f64>) -> [f64; 5] {
    v.sort_by(f64::total_cmp);
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    [q(0.0), q(0.1), q(0.5), q(0.9), q(1.0)]
}

fn main() -> Res<()> {
    let mut args = std::env::args().skip(1);
    let mut dirs = Vec::new();
    let mut json: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        if a == "--json" {
            json = Some(args.next().ok_or("--json <file>")?.into());
        } else {
            dirs.push(PathBuf::from(a));
        }
    }
    let ffmpeg = Ffmpeg {
        binary: std::env::var_os("SKIMAP_FFMPEG").map_or_else(|| "ffmpeg".into(), PathBuf::from),
        kbps: 24,
    };
    let t = Target::default();
    let mut all = serde_json::Map::new();
    for d in &dirs {
        let m = measure_layer(d, &ffmpeg)?;
        let lufs: Vec<f64> = m.iter().map(|(_, l)| l.lufs).collect();
        let tp: Vec<f64> = m.iter().map(|(_, l)| l.true_peak_dbtp).collect();
        let plr: Vec<f64> = m.iter().map(|(_, l)| l.true_peak_dbtp - l.lufs).collect();
        let out: Vec<&(String, _)> = m
            .iter()
            .filter(|(_, l)| !t.accepts(l, t.true_peak_dbtp))
            .collect();
        println!("{} : {} clips", d.display(), m.len());
        for (name, v) in [
            ("LUFS intégré", lufs),
            ("crête vraie dBTP", tp),
            ("crête − sonie dB", plr),
        ] {
            let q = quantiles(v);
            println!(
                "  {name:<18} min {:7.2}  p10 {:7.2}  médiane {:7.2}  p90 {:7.2}  max {:7.2}",
                q[0], q[1], q[2], q[3], q[4]
            );
        }
        println!(
            "  hors cible ({} LUFS ± {} LU, crête ≤ {} dBTP) : {}",
            t.lufs,
            t.tolerance_lu,
            t.true_peak_dbtp,
            out.len()
        );
        for (id, l) in out.iter().take(5) {
            println!(
                "    {id} : {:.2} LUFS, {:.2} dBTP",
                l.lufs, l.true_peak_dbtp
            );
        }
        all.insert(
            d.display().to_string(),
            m.iter()
                .map(|(id, l)| {
                    (
                        id.clone(),
                        serde_json::json!({"lufs": l.lufs, "true_peak_dbtp": l.true_peak_dbtp}),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
                .into(),
        );
    }
    if let Some(p) = json {
        std::fs::write(p, serde_json::to_vec_pretty(&all)?)?;
    }
    Ok(())
}
