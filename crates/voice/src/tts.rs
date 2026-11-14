//! Pocket TTS on the workstation through the `pocket-tts` command of pockettts.cpp.
//!
//! One process for a whole pack: the model loads once, then `--stream-by-line` synthesises
//! each stdin line into its own WAV on stdout. A subprocess rather than FFI: the workspace
//! forbids `unsafe`, and linking the C++/CUDA library would tie the cargo build (and the
//! phone build of this crate) to CUDA. Built by `crates/voice/scripts/build-pockettts.sh`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::error::{Error, Result};

/// Audio frame of the Mimi codec: 80 ms at 24 kHz.
pub const FRAME_SAMPLES: usize = 1920;
/// Output sample rate of Pocket TTS.
pub const SAMPLE_RATE: u32 = 24_000;
const HEADER_LEN: usize = 44;

/// How to run pockettts.cpp.
#[derive(Debug, Clone)]
pub struct PocketTts {
    /// `pocket-tts` binary.
    pub binary: PathBuf,
    /// Pack model GGUF (`pocket-tts-french-Q8_0.gguf`).
    pub model: PathBuf,
    /// Pack voices GGUF (`pocket-tts-french-voices.gguf`).
    pub voices: PathBuf,
    /// Voice name (`estelle`).
    pub voice: String,
    /// GGML device (`CUDA0`), `None` for the best one.
    pub device: Option<String>,
    /// Noise seed: the same text always gives the same audio.
    pub seed: u32,
}

/// One synthesised line.
#[derive(Debug, Clone)]
pub struct Utterance {
    /// 16-bit mono samples at [`SAMPLE_RATE`].
    pub samples: Vec<i16>,
    /// Tokens of each chunk, as logged.
    pub chunk_tokens: Vec<usize>,
    /// Frames of each chunk, as logged.
    pub chunk_frames: Vec<usize>,
    /// Whether a chunk ended without the end-of-speech signal (truncated speech).
    pub missing_eos: bool,
}

/// Timings of one run.
#[derive(Debug, Clone, Copy)]
pub struct RunStats {
    /// Wall time of the whole process, seconds.
    pub wall_s: f64,
    /// Model load time reported by the tool, milliseconds.
    pub load_ms: f64,
    /// Audio produced, seconds.
    pub audio_s: f64,
}

impl PocketTts {
    /// Synthesises every line (one utterance each). Lines must be non-empty, single-line.
    pub fn synthesize(&self, lines: &[String]) -> Result<(Vec<Utterance>, RunStats)> {
        for l in lines {
            if l.trim().is_empty() || l.contains('\n') || l.contains('\r') {
                return Err(Error::Tts(format!(
                    "invalid line for --stream-by-line: {l:?}"
                )));
            }
        }
        let mut cmd = Command::new(&self.binary);
        cmd.arg("--model")
            .arg(&self.model)
            .arg("--voices")
            .arg(&self.voices)
            .arg("--voice")
            .arg(&self.voice)
            .arg("--seed")
            .arg(self.seed.to_string())
            .args(["--stream-by-line", "-o", "-"]);
        if let Some(d) = &self.device {
            cmd.arg("--device").arg(d);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let t0 = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Tts(format!("cannot start {}: {e}", self.binary.display())))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let (out, log) = std::thread::scope(|s| {
            let w = s.spawn(move || stdin.write_all(input.as_bytes()));
            let r = s.spawn(move || {
                let mut b = Vec::new();
                stdout.read_to_end(&mut b).map(|_| b)
            });
            let log: Vec<String> = BufReader::new(stderr)
                .lines()
                .map_while(std::io::Result::ok)
                .collect();
            let wrote = w.join().expect("writer thread");
            let out = r.join().expect("reader thread");
            (wrote.and(out), log)
        });
        let status = child
            .wait()
            .map_err(|e| Error::Tts(format!("waiting for pocket-tts: {e}")))?;
        let wall_s = t0.elapsed().as_secs_f64();
        let out = out.map_err(|e| Error::Tts(format!("pocket-tts pipes: {e}")))?;
        if !status.success() {
            let tail: Vec<&String> = log.iter().rev().take(5).collect();
            return Err(Error::Tts(format!(
                "pocket-tts failed ({status}): {tail:?}"
            )));
        }
        let parsed = parse_log(&log)?;
        if parsed.utterances.len() != lines.len() {
            return Err(Error::Tts(format!(
                "{} lines sent, {} utterances logged",
                lines.len(),
                parsed.utterances.len()
            )));
        }
        let utterances = split_stream(&out, parsed.utterances)?;
        let audio_s = utterances
            .iter()
            .map(|u| u.samples.len() as f64 / f64::from(SAMPLE_RATE))
            .sum();
        Ok((
            utterances,
            RunStats {
                wall_s,
                load_ms: parsed.load_ms,
                audio_s,
            },
        ))
    }
}

struct ParsedLog {
    load_ms: f64,
    utterances: Vec<Utterance>,
}

/// Reads the per-chunk and per-utterance lines of the pockettts.cpp log.
fn parse_log(log: &[String]) -> Result<ParsedLog> {
    let mut load_ms = f64::NAN;
    let mut utterances = Vec::new();
    let mut cur = Utterance {
        samples: Vec::new(),
        chunk_tokens: Vec::new(),
        chunk_frames: Vec::new(),
        missing_eos: false,
    };
    for line in log {
        if let Some(rest) = line.strip_prefix("[Load] Ready in ") {
            load_ms = rest.trim_end_matches(" ms").parse().unwrap_or(f64::NAN);
        } else if let Some(rest) = line.strip_prefix("[TTS] chunk ") {
            if rest.contains("without EOS") {
                cur.missing_eos = true;
                continue;
            }
            // "1/1: 13 tokens, prefill 27.2 ms, 26 frames, EOS at 23"
            let field = |suffix: &str| -> Option<usize> {
                rest.split(", ")
                    .flat_map(|p| p.split(": "))
                    .find_map(|p| p.strip_suffix(suffix).and_then(|n| n.trim().parse().ok()))
            };
            match (field(" tokens"), field(" frames")) {
                (Some(t), Some(f)) => {
                    cur.chunk_tokens.push(t);
                    cur.chunk_frames.push(f);
                }
                _ => return Err(Error::Tts(format!("unexpected log line: {line}"))),
            }
        } else if line.starts_with("[TTS] ") && line.contains(" s of audio in ") {
            utterances.push(std::mem::replace(
                &mut cur,
                Utterance {
                    samples: Vec::new(),
                    chunk_tokens: Vec::new(),
                    chunk_frames: Vec::new(),
                    missing_eos: false,
                },
            ));
        } else if line.contains("ERROR") {
            return Err(Error::Tts(line.clone()));
        }
    }
    Ok(ParsedLog {
        load_ms,
        utterances,
    })
}

/// Cuts the stdout stream into utterances: each one is a 44-byte live WAV header followed
/// by (frames − 1) × 1920 samples per chunk (the first frame of every chunk is dropped).
fn split_stream(out: &[u8], mut utterances: Vec<Utterance>) -> Result<Vec<Utterance>> {
    let header = out
        .get(..HEADER_LEN)
        .ok_or_else(|| Error::Tts("empty WAV stream".into()))?
        .to_vec();
    check_header(&header)?;
    let mut pos = 0;
    for (k, u) in utterances.iter_mut().enumerate() {
        if out.get(pos..pos + HEADER_LEN) != Some(&header[..]) {
            return Err(Error::Tts(format!(
                "WAV header of utterance {k} not at {pos}"
            )));
        }
        pos += HEADER_LEN;
        let n: usize = u
            .chunk_frames
            .iter()
            .map(|&f| f.saturating_sub(1) * FRAME_SAMPLES)
            .sum();
        let data = out
            .get(pos..pos + 2 * n)
            .ok_or_else(|| Error::Tts(format!("utterance {k}: stream too short")))?;
        u.samples = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        pos += 2 * n;
    }
    if pos != out.len() {
        return Err(Error::Tts(format!(
            "{} bytes left after the last utterance",
            out.len() - pos
        )));
    }
    Ok(utterances)
}

fn check_header(h: &[u8]) -> Result<()> {
    let ok = &h[0..4] == b"RIFF"
        && &h[8..16] == b"WAVEfmt "
        && u16::from_le_bytes([h[20], h[21]]) == 1
        && u16::from_le_bytes([h[22], h[23]]) == 1
        && u32::from_le_bytes([h[24], h[25], h[26], h[27]]) == SAMPLE_RATE
        && u16::from_le_bytes([h[34], h[35]]) == 16
        && &h[36..40] == b"data";
    if ok {
        Ok(())
    } else {
        Err(Error::Tts(
            "unexpected WAV header (want PCM s16 mono 24 kHz)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Vec<u8> {
        let mut h = Vec::new();
        h.extend(b"RIFF");
        h.extend(0x7fff_ffffu32.to_le_bytes());
        h.extend(b"WAVEfmt ");
        h.extend(16u32.to_le_bytes());
        h.extend(1u16.to_le_bytes());
        h.extend(1u16.to_le_bytes());
        h.extend(SAMPLE_RATE.to_le_bytes());
        h.extend((SAMPLE_RATE * 2).to_le_bytes());
        h.extend(2u16.to_le_bytes());
        h.extend(16u16.to_le_bytes());
        h.extend(b"data");
        h.extend(0x7fff_ffffu32.to_le_bytes());
        h
    }

    #[test]
    fn log_and_stream_are_split_per_line() {
        let log: Vec<String> = [
            "[Load] Ready in 223 ms",
            "[TTS] chunk 1/1: 13 tokens, prefill 27.2 ms, 3 frames, EOS at 1",
            "[TTS] 0.24 s of audio in 221 ms (9.4x real time), step 1.37 ms/frame",
            "[TTS] chunk 1/2: 7 tokens, prefill 0.6 ms, 2 frames, EOS at 1",
            "[TTS] chunk 1 reached 9 frames without EOS",
            "[TTS] chunk 2/2: 5 tokens, prefill 0.6 ms, 2 frames, EOS at 1",
            "[TTS] 0.32 s of audio in 24 ms (68.8x real time), step 0.62 ms/frame",
        ]
        .map(String::from)
        .to_vec();
        let p = parse_log(&log).unwrap();
        assert_eq!(p.load_ms, 223.0);
        assert_eq!(p.utterances.len(), 2);
        assert_eq!(p.utterances[1].chunk_tokens, [7, 5]);
        assert!(p.utterances[1].missing_eos && !p.utterances[0].missing_eos);
        let mut out = header();
        out.extend(std::iter::repeat_n(1u8, 2 * 2 * FRAME_SAMPLES));
        out.extend(header());
        out.extend(std::iter::repeat_n(2u8, 2 * 2 * FRAME_SAMPLES));
        let u = split_stream(&out, p.utterances).unwrap();
        assert_eq!(u[0].samples.len(), 2 * FRAME_SAMPLES);
        assert_eq!(u[1].samples.len(), 2 * FRAME_SAMPLES);
        assert_eq!(u[1].samples[0], 0x0202);
        out.push(0);
        let p = parse_log(&log).unwrap();
        assert!(split_stream(&out, p.utterances).is_err());
    }
}
