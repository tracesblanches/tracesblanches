//! Loudness before and after a change of the loudness target (#179), on the clips of
//! built voice layers, and what the phone's `LoudnessEnhancer` makes of them.
//!
//! ```sh
//! cargo run --release -p skimap-voice --example relevel -- \
//!     --out <scratch dir> [--enhancer-mb 600,800] <voice layer dir>...
//! ```
//!
//! Every clip of a layer is decoded at the synthesis rate (24 kHz), levelled again with
//! `Target::default()` (or `--lufs`, `--ratio`, `--threshold`) exactly as the pipeline levels a new clip
//! (`pack::encode_levelled`: compression, gain, limiter, Ogg/Opus, check of the decoded
//! file) into `--out` (never into the layer), and both files are measured decoded at
//! 48 kHz as the phone decodes them. `--enhancer-mb`: each decoded clip also goes through
//! a port of the AOSP `LoudnessEnhancer` (frameworks/av, media/libeffects/loudness,
//! `AdaptiveDynamicRangeCompression`: gain at the input, 7:1 compressor with a knee at
//! -8 to -10 dBFS, 1 ms attack, 15 ms release, hard clamp at full scale), and the
//! clamped samples (the clipping that can be heard) are counted.

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, writes only to the scratch directory given"
)]
use std::path::PathBuf;

use rayon::prelude::*;
use skimap_voice::loudness::{Loudness, Normalized, measure};
use skimap_voice::pack::{MEASURE_RATE, VoiceConfig, VoiceManifest, encode_levelled};
use skimap_voice::tts::SAMPLE_RATE;

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn quantiles(mut v: Vec<f64>) -> [f64; 5] {
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return [f64::NAN; 5];
    }
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    [q(0.0), q(0.1), q(0.5), q(0.9), q(1.0)]
}

fn print_q(name: &str, v: Vec<f64>) {
    let q = quantiles(v);
    println!(
        "  {name:<28} min {:7.2}  p10 {:7.2}  médiane {:7.2}  p90 {:7.2}  max {:7.2}",
        q[0], q[1], q[2], q[3], q[4]
    );
}

/// `math::fast_log` of the AOSP effect (log2 by the float bits, times ln 2).
fn fast_log(v: f32) -> f32 {
    let mut x = v.to_bits() as i32;
    let log_2 = ((x >> 23) & 255) - 128;
    x &= !(255 << 23);
    x += 127 << 23;
    let val = f32::from_bits(x as u32);
    let val = ((-1.0f32 / 3.0) * val + 2.0) * val - 2.0 / 3.0;
    (val + log_2 as f32) * std::f32::consts::LN_2
}

/// What the AOSP `LoudnessEnhancer` of `gain_mb` does to `x` (16-bit scale, mono played
/// on both channels): output samples, and the samples clamped at full scale.
fn enhancer(x: &[i16], rate: u32, gain_mb: i32) -> (Vec<i16>, usize, f32) {
    let amp = 10f32.powf(gain_mb as f32 / 2000.0);
    // kTargetGain {1..5} -> kKneeThreshold {-8, -8, -8.5, -9, -10} dB, linear.
    let gains = [1.0f32, 2.0, 3.0, 4.0, 5.0];
    let knees = [-8.0f32, -8.0, -8.5, -9.0, -10.0];
    let knee_db = if amp <= gains[0] {
        knees[0]
    } else if amp >= gains[4] {
        knees[4]
    } else {
        let i = gains.iter().rposition(|&g| g <= amp).unwrap_or(0);
        knees[i] + (knees[i + 1] - knees[i]) * (amp - gains[i]) / (gains[i + 1] - gains[i])
    };
    let knee = 0.115_129_25_f32 * knee_db + 10.397_177;
    let alpha_attack = (-1.0f32 / (0.001 * rate as f32)).exp();
    let alpha_release = (-1.0f32 / (0.015 * rate as f32)).exp();
    let slope = 1.0f32 / 7.0 - 1.0;
    let min_log_abs = 0.032_767_f32;
    let mut state = 0.0f32;
    let mut gain = 1.0f32;
    let mut clamped = 0;
    let mut worst = 0.0f32;
    let mut out = Vec::with_capacity(x.len());
    for &s in x {
        let v = amp * f32::from(s);
        let overshoot = fast_log(v.abs().max(min_log_abs)) - knee;
        let cv = overshoot.max(0.0) * slope;
        let prev = state;
        if cv <= state {
            state = alpha_attack * state + (1.0 - alpha_attack) * cv;
        } else {
            state = alpha_release * state + (1.0 - alpha_release) * cv;
        }
        gain *= (state - prev).exp();
        let y = v * gain;
        if y.abs() > 32767.0 {
            clamped += 1;
            worst = worst.max(20.0 * (y.abs() / 32767.0).log10());
        }
        out.push(y.clamp(-32767.0, 32767.0) as i16);
    }
    (out, clamped, worst)
}

struct Clip {
    before: Loudness,
    after: Loudness,
    level: Normalized,
    /// Per enhancer gain: loudness out, clamped samples, worst overshoot dB, samples.
    enh_before: Vec<(f64, usize, f32, usize)>,
    enh_after: Vec<(f64, usize, f32, usize)>,
}

fn enh(pcm: &[i16], gains: &[i32]) -> Res<Vec<(f64, usize, f32, usize)>> {
    gains
        .iter()
        .map(|&g| {
            let (y, c, w) = enhancer(pcm, MEASURE_RATE, g);
            Ok((measure(&y, MEASURE_RATE)?.lufs, c, w, y.len()))
        })
        .collect()
}

fn main() -> Res<()> {
    let mut args = std::env::args().skip(1);
    let mut dirs = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut gains: Vec<i32> = Vec::new();
    let mut cfg = VoiceConfig::from_env()?;
    let num = |args: &mut dyn Iterator<Item = String>, flag: &str| -> Res<f64> {
        Ok(args.next().ok_or(format!("{flag} <value>"))?.parse()?)
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = Some(args.next().ok_or("--out <dir>")?.into()),
            "--enhancer-mb" => {
                gains = args
                    .next()
                    .ok_or("--enhancer-mb <mB,...>")?
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
            }
            // Another target, to compare (`--ratio 1 --lufs -14`: the target before).
            "--lufs" => cfg.loudness.lufs = num(&mut args, "--lufs")?,
            "--ratio" => cfg.loudness.compressor.ratio = num(&mut args, "--ratio")?,
            "--threshold" => cfg.loudness.compressor.threshold_lu = num(&mut args, "--threshold")?,
            _ => dirs.push(PathBuf::from(a)),
        }
    }
    let out = out.ok_or("--out <scratch dir>")?;
    let t = cfg.loudness;
    println!(
        "cible : {} LUFS, crête ≤ {} dBTP, compresseur {:?}",
        t.lufs, t.true_peak_dbtp, t.compressor
    );
    for d in &dirs {
        let vm = VoiceManifest::read(d)?;
        let name = d
            .parent()
            .and_then(|p| p.file_name())
            .map_or_else(|| "layer".into(), |n| n.to_string_lossy().into_owned());
        let to = out.join(&name);
        std::fs::create_dir_all(to.join("clips"))?;
        let clips: Vec<Clip> = vm
            .clips
            .par_iter()
            .map(|c| -> Res<Clip> {
                let src = d.join(&c.file);
                let pcm48 = cfg.ffmpeg.decode(&src, MEASURE_RATE)?;
                let before = measure(&pcm48, MEASURE_RATE)?;
                let pcm24 = cfg.ffmpeg.decode(&src, SAMPLE_RATE)?;
                let dst = to.join(&c.file);
                let (level, after) = encode_levelled(&c.id, &pcm24, &dst, &cfg)?;
                let new48 = cfg.ffmpeg.decode(&dst, MEASURE_RATE)?;
                Ok(Clip {
                    before,
                    after,
                    level,
                    enh_before: enh(&pcm48, &gains)?,
                    enh_after: enh(&new48, &gains)?,
                })
            })
            .collect::<Res<_>>()?;
        println!("{} : {} clips", d.display(), clips.len());
        print_q("avant LUFS", clips.iter().map(|c| c.before.lufs).collect());
        print_q("après LUFS", clips.iter().map(|c| c.after.lufs).collect());
        print_q(
            "avant crête vraie dBTP",
            clips.iter().map(|c| c.before.true_peak_dbtp).collect(),
        );
        print_q(
            "après crête vraie dBTP",
            clips.iter().map(|c| c.after.true_peak_dbtp).collect(),
        );
        print_q(
            "avant crête − sonie dB",
            clips
                .iter()
                .map(|c| c.before.true_peak_dbtp - c.before.lufs)
                .collect(),
        );
        print_q(
            "après crête − sonie dB",
            clips
                .iter()
                .map(|c| c.after.true_peak_dbtp - c.after.lufs)
                .collect(),
        );
        print_q(
            "compression max dB",
            clips.iter().map(|c| c.level.compressor_db).collect(),
        );
        print_q(
            "limiteur max dB",
            clips.iter().map(|c| c.level.limiter_db).collect(),
        );
        let gained: Vec<f64> = clips.iter().map(|c| c.after.lufs - c.before.lufs).collect();
        print_q("gain de sonie dB", gained);
        println!(
            "  bornés (limiteur à 6 dB) : {} ; hors tolérance (> {} LU) : {} ; crête > {} dBTP : {}",
            clips.iter().filter(|c| c.level.capped).count(),
            t.tolerance_lu,
            clips
                .iter()
                .filter(|c| !c.level.capped && (c.after.lufs - t.lufs).abs() > t.tolerance_lu)
                .count(),
            t.true_peak_dbtp,
            clips
                .iter()
                .filter(|c| c.after.true_peak_dbtp > t.true_peak_dbtp)
                .count()
        );
        for (k, g) in gains.iter().enumerate() {
            for (label, pick) in [("avant", 0), ("après", 1)] {
                let e = |c: &Clip| {
                    if pick == 0 {
                        c.enh_before[k]
                    } else {
                        c.enh_after[k]
                    }
                };
                let samples: usize = clips.iter().map(|c| e(c).3).sum();
                let clamped: usize = clips.iter().map(|c| e(c).1).sum();
                let with: usize = clips.iter().filter(|c| e(c).1 > 0).count();
                let worst = clips.iter().map(|c| e(c).2).fold(0.0f32, f32::max);
                println!(
                    "  LoudnessEnhancer {g} mB sur les clips {label} : écrêtés {clamped} échantillons sur {samples} ({:.5} %), {with} clips touchés, dépassement max {worst:.2} dB",
                    100.0 * clamped as f64 / samples as f64
                );
                print_q(
                    &format!("  sonie sortie {label} LUFS"),
                    clips.iter().map(|c| e(c).0).collect(),
                );
                print_q(
                    &format!("  gain effectif {label} dB"),
                    clips
                        .iter()
                        .map(|c| {
                            e(c).0
                                - if pick == 0 {
                                    c.before.lufs
                                } else {
                                    c.after.lufs
                                }
                        })
                        .collect(),
                );
            }
        }
    }
    Ok(())
}
