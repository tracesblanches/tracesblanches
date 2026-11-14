//! Clip audio: silence trimming, level normalisation, Ogg/Opus through ffmpeg, WAV.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Analysis window: 10 ms at 24 kHz.
const WIN: usize = 240;

/// Silence trimming and level parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    /// A 10 ms window is speech when its RMS is within this many dB of the loudest window.
    pub below_peak_db: f64,
    /// ... and above this absolute level, dBFS.
    pub floor_dbfs: f64,
    /// Audio kept before the first speech window, ms (consonant onsets are quiet).
    pub pre_ms: u32,
    /// Audio kept after the last speech window, ms.
    pub post_ms: u32,
    /// Fade in and out, ms.
    pub fade_ms: u32,
    /// Target RMS of the speech windows, dBFS.
    pub target_dbfs: f64,
    /// Peak ceiling after gain, dBFS.
    pub ceiling_dbfs: f64,
    /// A sound touching the end of the buffer at most this long, ms...
    pub end_burst_max_ms: u32,
    /// ... after at least this much silence, ms, is a cut-off artefact, not speech.
    pub end_gap_min_ms: u32,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            below_peak_db: 40.0,
            floor_dbfs: -60.0,
            pre_ms: 40,
            post_ms: 80,
            fade_ms: 5,
            target_dbfs: -20.0,
            ceiling_dbfs: -1.0,
            end_burst_max_ms: 150,
            end_gap_min_ms: 150,
        }
    }
}

/// What [`shape`] did to a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaped {
    /// Leading audio removed, ms.
    pub lead_ms: f64,
    /// Trailing audio removed, ms.
    pub tail_ms: f64,
    /// Gain applied, dB.
    pub gain_db: f64,
    /// Whether a cut-off sound at the end of the buffer was removed.
    pub end_burst: bool,
}

fn dbfs(rms: f64) -> f64 {
    if rms <= 0.0 {
        -200.0
    } else {
        20.0 * (rms / 32768.0).log10()
    }
}

fn window_db(s: &[i16]) -> Vec<f64> {
    s.chunks(WIN)
        .map(|w| {
            let e: f64 = w.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
            dbfs((e / w.len() as f64).sqrt())
        })
        .collect()
}

/// Start of the first speech window, ms (`None`: no speech).
///
/// Pocket TTS drops the first 80 ms frame of every utterance (the Mimi cold-start click);
/// when the model starts speaking at once, the first phoneme is cut ("Greffier" heard as
/// "véhier"), or the click's tail opens the clip. Both show as an early onset.
pub fn speech_onset_ms(samples: &[i16], sample_rate: u32, p: &Shape) -> Option<f64> {
    let db = window_db(samples);
    let peak = db.iter().copied().fold(f64::MIN, f64::max);
    let thr = (peak - p.below_peak_db).max(p.floor_dbfs);
    let first = db.iter().position(|&d| d >= thr)?;
    Some((first * WIN) as f64 * 1000.0 / f64::from(sample_rate))
}

/// Onsets earlier than this (ms) are cut or open on the click tail: the clip is generated
/// again with another seed. Measured on "Chiens tenus en laisse." over six seeds: onsets of
/// 20 to 50 ms are heard "bien tenue" (cut "ch") three times out of four, onsets of 120 and
/// 130 ms "chien tenu".
pub const MIN_ONSET_MS: f64 = 60.0;

/// Trims the leading and trailing silence and normalises the speech level.
/// Returns `None` when the clip has no speech at all.
pub fn shape(samples: &[i16], sample_rate: u32, p: &Shape) -> Option<(Vec<i16>, Shaped)> {
    let db = window_db(samples);
    let peak = db.iter().copied().fold(f64::MIN, f64::max);
    let thr = (peak - p.below_peak_db).max(p.floor_dbfs);
    let first = db.iter().position(|&d| d >= thr)?;
    let mut last = db.iter().rposition(|&d| d >= thr)?;
    // Pocket TTS keeps a few frames after the end of speech; the last one often starts a
    // new sound that the end of the buffer cuts (measured: -45 to -23 dBFS over the last
    // 20-50 ms, after 250 ms of silence). Drop such a short burst touching the end.
    let mut end_burst = false;
    if last + 2 >= db.len() {
        let island = (0..=last).rev().take_while(|&i| db[i] >= thr).count();
        let island_start = last + 1 - island;
        let prev = db[..island_start].iter().rposition(|&d| d >= thr);
        if let Some(prev) = prev
            && island * 10 <= p.end_burst_max_ms as usize
            && (island_start - prev - 1) * 10 >= p.end_gap_min_ms as usize
        {
            last = prev;
            end_burst = true;
        }
    }
    let per_ms = f64::from(sample_rate) / 1000.0;
    let start = (first * WIN).saturating_sub((f64::from(p.pre_ms) * per_ms) as usize);
    let end = ((last + 1) * WIN + (f64::from(p.post_ms) * per_ms) as usize).min(samples.len());
    let cut = &samples[start..end];
    // Level: RMS of the speech windows only.
    let (mut e, mut n) = (0.0f64, 0usize);
    for (w, &d) in cut.chunks(WIN).zip(&window_db(cut)) {
        if d >= thr {
            e += w.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>();
            n += w.len();
        }
    }
    let rms_db = dbfs((e / n.max(1) as f64).sqrt());
    let peak_abs = cut.iter().map(|&x| i32::from(x).abs()).max().unwrap_or(0);
    let peak_db = dbfs(f64::from(peak_abs));
    let gain_db = (p.target_dbfs - rms_db).min(p.ceiling_dbfs - peak_db);
    let g = 10f64.powf(gain_db / 20.0);
    let fade = ((f64::from(p.fade_ms) * per_ms) as usize)
        .min(cut.len() / 2)
        .max(1);
    let len = cut.len();
    let out: Vec<i16> = cut
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let f = if i < fade {
                i as f64 / fade as f64
            } else if i >= len - fade {
                (len - 1 - i) as f64 / fade as f64
            } else {
                1.0
            };
            (f64::from(x) * g * f).round().clamp(-32768.0, 32767.0) as i16
        })
        .collect();
    Some((
        out,
        Shaped {
            lead_ms: start as f64 / per_ms,
            tail_ms: (samples.len() - end) as f64 / per_ms,
            gain_db,
            end_burst,
        },
    ))
}

/// ffmpeg with libopus.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    /// ffmpeg binary.
    pub binary: PathBuf,
    /// Opus bitrate, kbit/s.
    pub kbps: u32,
}

impl Ffmpeg {
    fn run(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
        let mut child = Command::new(&self.binary)
            .args(["-hide_banner", "-loglevel", "error"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Audio(format!("cannot start {}: {e}", self.binary.display())))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let (out, err) = std::thread::scope(|s| {
            let w = s.spawn(move || stdin.write_all(input));
            let e = s.spawn(move || {
                let mut b = String::new();
                stderr.read_to_string(&mut b).map(|_| b)
            });
            let mut b = Vec::new();
            let r = stdout.read_to_end(&mut b);
            let wrote = w.join().expect("writer thread");
            let err = e.join().expect("stderr thread").unwrap_or_default();
            (wrote.and(r).map(|_| b), err)
        });
        let status = child
            .wait()
            .map_err(|e| Error::Audio(format!("waiting for ffmpeg: {e}")))?;
        let out = out.map_err(|e| Error::Audio(format!("ffmpeg pipes: {e}")))?;
        if !status.success() {
            return Err(Error::Audio(format!("ffmpeg {args:?}: {status}: {err}")));
        }
        Ok(out)
    }

    /// Encodes mono 16-bit samples to an Ogg/Opus file (no metadata, bit-exact output).
    pub fn encode_opus(&self, samples: &[i16], sample_rate: u32, out: &Path) -> Result<()> {
        let pcm: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let sr = sample_rate.to_string();
        let br = format!("{}k", self.kbps);
        let path = out
            .to_str()
            .ok_or_else(|| Error::Audio(format!("non UTF-8 path {}", out.display())))?;
        self.run(
            &[
                "-f",
                "s16le",
                "-ar",
                &sr,
                "-ac",
                "1",
                "-i",
                "pipe:0",
                "-c:a",
                "libopus",
                "-b:a",
                &br,
                "-vbr",
                "on",
                "-application",
                "audio",
                "-frame_duration",
                "20",
                "-map_metadata",
                "-1",
                "-fflags",
                "+bitexact",
                "-flags:a",
                "+bitexact",
                "-y",
                path,
            ],
            &pcm,
        )?;
        Ok(())
    }

    /// Decodes an audio file to mono 16-bit samples at `sample_rate`.
    pub fn decode(&self, path: &Path, sample_rate: u32) -> Result<Vec<i16>> {
        let p = path
            .to_str()
            .ok_or_else(|| Error::Audio(format!("non UTF-8 path {}", path.display())))?;
        let sr = sample_rate.to_string();
        let out = self.run(
            &["-i", p, "-f", "s16le", "-ar", &sr, "-ac", "1", "pipe:1"],
            &[],
        )?;
        Ok(out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect())
    }
}

/// Writes a 16-bit mono WAV file.
pub fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) -> Result<()> {
    let data = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(sample_rate.to_le_bytes());
    b.extend((sample_rate * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for s in samples {
        b.extend(s.to_le_bytes());
    }
    std::fs::write(path, b).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 500 ms of faint noise, 500 ms of a 220 Hz tone at -30 dBFS RMS, 300 ms of noise.
    fn clip() -> Vec<i16> {
        let sr = 24_000.0;
        let amp = 32768.0 * 10f64.powf(-30.0 / 20.0) * 2f64.sqrt();
        let noise = |i: usize| ((i * 7919 % 13) as f64 - 6.0) as i16; // about -75 dBFS
        let mut s: Vec<i16> = (0..12_000).map(noise).collect();
        s.extend(
            (0..12_000)
                .map(|i| (amp * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / sr).sin()) as i16),
        );
        s.extend((0..7_200).map(noise));
        s
    }

    #[test]
    fn trims_silence_and_normalises() {
        let p = Shape::default();
        let (out, s) = shape(&clip(), 24_000, &p).unwrap();
        // Speech starts at 500 ms: 40 ms of pre-roll kept.
        assert!((s.lead_ms - 460.0).abs() <= 10.0, "{s:?}");
        // Speech ends at 1000 ms: 80 ms kept, 220 ms removed.
        assert!((s.tail_ms - 220.0).abs() <= 10.0, "{s:?}");
        assert!((s.gain_db - 10.0).abs() < 0.2, "{s:?}");
        let mid = &out[2000..10_000];
        let rms =
            (mid.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / mid.len() as f64).sqrt();
        assert!((dbfs(rms) + 20.0).abs() < 0.3, "{}", dbfs(rms));
        assert_eq!(out[0], 0, "fade in");
        assert!(
            shape(&[0i16; 4800], 24_000, &p).is_none(),
            "digital silence"
        );
    }

    #[test]
    fn drops_a_cut_off_burst_at_the_end() {
        // Speech, 300 ms of silence, then 40 ms of sound cut by the end of the buffer.
        let mut s = clip();
        s.truncate(24_000 + 7_200);
        s.extend((0..960).map(|i| ((i as f64 * 0.3).sin() * 2000.0) as i16));
        let (out, sh) = shape(&s, 24_000, &Shape::default()).unwrap();
        assert!(sh.end_burst, "{sh:?}");
        assert_eq!(out.len(), (1000 + 80 - 460) * 24, "{sh:?}");
        // A long final sound is speech: kept.
        s.extend((0..4800).map(|i| ((i as f64 * 0.3).sin() * 2000.0) as i16));
        let (_, sh) = shape(&s, 24_000, &Shape::default()).unwrap();
        assert!(!sh.end_burst && sh.tail_ms < 10.0, "{sh:?}");
    }

    #[test]
    fn peak_ceiling_limits_gain() {
        // A loud click in silence: the gain is bounded by the -1 dBFS ceiling.
        let mut s = vec![0i16; 4800];
        s[2400] = 30_000;
        s[2401] = 20_000;
        let (out, _) = shape(&s, 24_000, &Shape::default()).unwrap();
        let peak = out.iter().map(|&x| i32::from(x).abs()).max().unwrap();
        assert!(peak <= 29_205, "{peak}"); // -1 dBFS = 29 204.5
    }

    #[test]
    fn wav_header() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_wav(&p, &[1, -1, 2], 24_000).unwrap();
        let b = std::fs::read(&p).unwrap();
        assert_eq!(b.len(), 44 + 6);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([b[40], b[41], b[42], b[43]]), 6);
    }
}
