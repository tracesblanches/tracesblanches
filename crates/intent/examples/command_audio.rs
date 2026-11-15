//! Spoken questions for the end-to-end test of the voice commands (VO5 #87, A11 #98):
//! every question of `tests/data/test.tsv` (the held-out set, commands and questions
//! outside the list) said by Pocket TTS on the workstation (`docs/voix.md`), French
//! model for the French questions and English model for the English ones, two voices
//! each, then mixed with synthetic wind at several signal-to-noise ratios and written as
//! 16 kHz mono WAV (what the phone's microphone gives the recognizer).
//!
//! ```sh
//! cargo run --release -p skimap-intent --example command_audio -- ~/.cache/skimap/voicecmd/audio
//! ```
//!
//! Output: `<dir>/index.tsv` (file, label, lang, voice, snr, text) and `<dir>/<snr>/*.wav`.
//! Clips go through the quality control of the packs (attack cut: another seed).
//! SNR is the ratio of the speech RMS (the TTS clip, its silences trimmed by the energy
//! gate below) to the wind RMS over the whole file, in dB; `clean` has no wind.
//!
//! Wind (no recording is redistributable here): brown noise (leaky integral of white
//! noise, below ~300 Hz like wind on a microphone), with gusts (slow random envelope
//! between 0.25 and 1 over 0.5 to 2 s) and buffeting (bursts of low-passed noise, 50 to
//! 300 ms, three times a second on average), seeded: the same files every run.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use skimap_intent::Lang;
use skimap_intent::measure::{class_name, parse};
use skimap_voice::audio::{MIN_ONSET_MS, Shape, speech_onset_ms, write_wav};
use skimap_voice::tts::{PocketTts, SAMPLE_RATE};

const OUT_RATE: u32 = 16_000;
const SNRS: [Option<f64>; 5] = [None, Some(10.0), Some(5.0), Some(0.0), Some(-5.0)];
const LEAD_S: f64 = 0.6;
const TAIL_S: f64 = 1.0;

/// Deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Roughly normal (sum of 4 uniforms).
    fn gauss(&mut self) -> f64 {
        (0..4).map(|_| self.next()).sum::<f64>() - 2.0
    }
}

/// Synthetic wind, `n` samples at 16 kHz, unit RMS.
fn wind(n: usize, seed: u64) -> Vec<f64> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let fs = f64::from(OUT_RATE);
    // Brown noise: leaky integrator (pole at ~ 20 Hz) then one-pole low-pass at 300 Hz.
    let leak = (-2.0 * std::f64::consts::PI * 20.0 / fs).exp();
    let lp = (-2.0 * std::f64::consts::PI * 300.0 / fs).exp();
    let mut b = 0.0;
    let mut l = 0.0;
    // Gust envelope: piecewise linear between random levels.
    let mut env_from = 0.6;
    let mut env_to = 0.6;
    let mut seg_len = 1;
    let mut seg_pos = 0;
    // Buffeting bursts.
    let mut burst_left = 0usize;
    let mut burst_gain = 0.0;
    let mut bl = 0.0;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        if seg_pos >= seg_len {
            env_from = env_to;
            env_to = 0.25 + 0.75 * r.next();
            seg_len = ((0.5 + 1.5 * r.next()) * fs) as usize;
            seg_pos = 0;
        }
        let env = env_from + (env_to - env_from) * seg_pos as f64 / seg_len as f64;
        seg_pos += 1;
        b = leak * b + r.gauss();
        l = lp * l + (1.0 - lp) * b;
        if burst_left == 0 && r.next() < 3.0 / fs {
            burst_left = ((0.05 + 0.25 * r.next()) * fs) as usize;
            burst_gain = 2.0 + 4.0 * r.next();
        }
        let mut s = l * env;
        if burst_left > 0 {
            bl = 0.9 * bl + 0.1 * r.gauss();
            s += burst_gain * bl * 3.0;
            burst_left -= 1;
        }
        out.push(s);
    }
    let rms = (out.iter().map(|x| x * x).sum::<f64>() / n as f64).sqrt();
    out.iter_mut().for_each(|x| *x /= rms);
    out
}

/// 24 kHz → 16 kHz: windowed-sinc low-pass at 7.2 kHz (Hann, 64 taps of the input).
fn resample(x: &[i16]) -> Vec<f64> {
    let ratio = f64::from(SAMPLE_RATE) / f64::from(OUT_RATE);
    let cutoff = 7200.0 / f64::from(SAMPLE_RATE);
    let half = 32isize;
    let n = (x.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 * ratio;
            let c = t.floor() as isize;
            let mut acc = 0.0;
            for k in (c - half + 1)..=(c + half) {
                if k < 0 || k as usize >= x.len() {
                    continue;
                }
                let d = t - k as f64;
                let w = 0.5 * (1.0 + (std::f64::consts::PI * d / half as f64).cos());
                let a = 2.0 * cutoff * d;
                let sinc = if a.abs() < 1e-9 {
                    1.0
                } else {
                    (std::f64::consts::PI * a).sin() / (std::f64::consts::PI * a)
                };
                acc += f64::from(x[k as usize]) / 32768.0 * 2.0 * cutoff * sinc * w;
            }
            acc
        })
        .collect()
}

/// RMS of the frames (20 ms) louder than 40 dB under the loudest: the speech, not its gaps.
fn speech_rms(x: &[f64]) -> f64 {
    let f = (OUT_RATE / 50) as usize;
    let rms: Vec<f64> = x
        .chunks(f)
        .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt())
        .collect();
    let max = rms.iter().copied().fold(0.0, f64::max);
    let loud: Vec<f64> = rms.into_iter().filter(|r| *r > max * 0.01).collect();
    (loud.iter().map(|r| r * r).sum::<f64>() / loud.len().max(1) as f64).sqrt()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: command_audio <out dir>")?,
    );
    let home = PathBuf::from(std::env::var("HOME")?);
    let models = home.join("projects/speak/app/models/tts");
    let bin = home.join(".cache/skimap/pockettts/build-cuda/pocket-tts");
    let cases = parse(include_str!("../tests/data/test.tsv"))?;
    // Voices: CC0 or CC-BY (docs/voix.md), two per language.
    let runs = [
        (Lang::Fr, "french", "estelle"),
        (Lang::Fr, "french", "marius"),
        (Lang::En, "english", "alba"),
        (Lang::En, "english", "peter_yearsley"),
    ];
    for snr in SNRS {
        std::fs::create_dir_all(out.join(snr_dir(snr)))?;
    }
    let mut index = std::fs::File::create(out.join("index.tsv"))?;
    writeln!(index, "file\tlabel\tlang\tvoice\tsnr\ttext")?;
    let t0 = std::time::Instant::now();
    let mut audio_s = 0.0;
    for (lang, model, voice) in runs {
        let lines: Vec<(usize, String)> = cases
            .iter()
            .enumerate()
            .filter(|(_, c)| c.lang == lang)
            .map(|(i, c)| (i, c.text.clone()))
            .collect();
        let mut tts = PocketTts {
            binary: bin.clone(),
            model: models.join(format!("pocket-tts-{model}-Q8_0.gguf")),
            voices: models.join(format!("pocket-tts-{model}-voices.gguf")),
            voice: voice.into(),
            device: None,
            seed: 42,
        };
        let texts: Vec<String> = lines.iter().map(|(_, t)| t.clone()).collect();
        let (mut utts, stats) = tts.synthesize(&texts)?;
        // The quality control of the packs (VO3, docs/voix.md): an attack cut by the
        // dropped first frame (onset before MIN_ONSET_MS) or a missing end of speech is
        // synthesised again with the next seed, 8 seeds at most.
        let bad = |u: &skimap_voice::tts::Utterance| {
            u.missing_eos
                || speech_onset_ms(&u.samples, SAMPLE_RATE, &Shape::default())
                    .is_none_or(|o| o < MIN_ONSET_MS)
        };
        let mut redone = 0;
        for seed in 43..50 {
            let todo: Vec<usize> = (0..utts.len()).filter(|&k| bad(&utts[k])).collect();
            if todo.is_empty() {
                break;
            }
            tts.seed = seed;
            let again: Vec<String> = todo.iter().map(|&k| texts[k].clone()).collect();
            let (u2, _) = tts.synthesize(&again)?;
            for (k, u) in todo.into_iter().zip(u2) {
                if !bad(&u) || seed == 49 {
                    utts[k] = u;
                    redone += 1;
                }
            }
        }
        let still = utts.iter().filter(|u| bad(u)).count();
        println!(
            "{voice}: {} lines, {:.1} s of audio in {:.1} s; {redone} redone with another seed, {still} still early",
            utts.len(),
            stats.audio_s,
            stats.wall_s
        );
        for ((i, _), u) in lines.iter().zip(&utts) {
            let speech = resample(&u.samples);
            audio_s += speech.len() as f64 / f64::from(OUT_RATE);
            let lead = (LEAD_S * f64::from(OUT_RATE)) as usize;
            let tail = (TAIL_S * f64::from(OUT_RATE)) as usize;
            let mut clean = vec![0.0; lead];
            clean.extend_from_slice(&speech);
            clean.extend(std::iter::repeat_n(0.0, tail));
            let s_rms = speech_rms(&speech);
            let noise = wind(clean.len(), (*i as u64) << 8 | voice.len() as u64);
            let c = &cases[*i];
            for snr in SNRS {
                let g = snr.map_or(0.0, |db| s_rms / 10f64.powf(db / 20.0));
                let mix: Vec<f64> = clean.iter().zip(&noise).map(|(s, n)| s + g * n).collect();
                // Never clipped: scaled down when the gusts would.
                let peak = mix.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                let k = if peak > 0.99 { 0.99 / peak } else { 1.0 };
                let pcm: Vec<i16> = mix.iter().map(|v| (v * k * 32767.0) as i16).collect();
                let name = format!("{}/{i:03}-{voice}.wav", snr_dir(snr));
                write_wav(&out.join(&name), &pcm, OUT_RATE)?;
                writeln!(
                    index,
                    "{name}\t{}\t{}\t{voice}\t{}\t{}",
                    class_name(c.expected),
                    match c.lang {
                        Lang::Fr => "fr",
                        Lang::En => "en",
                    },
                    snr_dir(snr),
                    c.text
                )?;
            }
        }
    }
    println!(
        "{} s of speech, {} files in {}, {:.1} s",
        audio_s.round(),
        cases.len() * 2 * SNRS.len(),
        out.display(),
        t0.elapsed().as_secs_f64()
    );
    check_dir(&out)
}

fn snr_dir(snr: Option<f64>) -> String {
    snr.map_or("clean".into(), |d| format!("snr{d:+.0}"))
}

fn check_dir(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let n = std::fs::read_to_string(out.join("index.tsv"))?
        .lines()
        .count()
        - 1;
    println!("index: {n} files");
    Ok(())
}
