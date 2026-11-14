//! Voice layer of a pack (VO3): every clip of the pack generated with Pocket TTS on the
//! workstation, trimmed, normalised, encoded in Ogg/Opus, with its manifest.
//!
//! ```text
//! data/out/<site>/voice/
//!   clips/<clip id>.opus     one clip per manifest entry
//!   manifest.json            clip id -> text, kind, file, duration, size, tokens
//!   voice.meta.json          sources, licences, generation date
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::audio::{Ffmpeg, MIN_ONSET_MS, Shape, shape, speech_onset_ms, write_wav};
use crate::bank::Announcement;
use crate::error::{Error, Result};
use crate::manifest::{ClipKind, PackNames, manifest};
use crate::tokenizer::PocketText;
use crate::tts::{PocketTts, SAMPLE_RATE, Utterance};

/// Layer name.
pub const LAYER: &str = "voice";
/// Token budget of one clip (pockettts.cpp chunk budget, speak measurement).
pub const MAX_TOKENS: usize = 50;
/// Seeds tried for a clip: the configured one, then the next ones.
pub const SEED_ATTEMPTS: u32 = 8;

/// Generation settings.
#[derive(Debug, Clone)]
pub struct VoiceConfig {
    /// Text-to-speech.
    pub tts: PocketTts,
    /// Encoder.
    pub ffmpeg: Ffmpeg,
    /// Trimming and level.
    pub shape: Shape,
}

fn env_path(var: &str, default: impl FnOnce() -> PathBuf) -> PathBuf {
    std::env::var_os(var).map_or_else(default, PathBuf::from)
}

impl VoiceConfig {
    /// From the environment, with the defaults of this workstation:
    ///
    /// | Variable | Default |
    /// |---|---|
    /// | `SKIMAP_POCKETTTS_BIN` | `~/.cache/skimap/pockettts/build-cuda/pocket-tts` |
    /// | `SKIMAP_POCKETTTS_MODEL` | `~/projects/speak/app/models/tts/pocket-tts-french-Q8_0.gguf` |
    /// | `SKIMAP_POCKETTTS_VOICES` | `~/projects/speak/app/models/tts/pocket-tts-french-voices.gguf` |
    /// | `SKIMAP_VOICE` | `estelle` |
    /// | `SKIMAP_TTS_DEVICE` | best device (CUDA0 here) |
    /// | `SKIMAP_TTS_SEED` | `42` |
    /// | `SKIMAP_FFMPEG` | `ffmpeg` |
    /// | `SKIMAP_OPUS_KBPS` | `24` |
    pub fn from_env() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Config("HOME is not set".into()))?;
        let tts_dir = home.join("projects/speak/app/models/tts");
        let num = |var: &str, default: u32| -> Result<u32> {
            match std::env::var(var) {
                Ok(v) => v
                    .parse()
                    .map_err(|_| Error::Config(format!("{var}={v} is not a number"))),
                Err(_) => Ok(default),
            }
        };
        Ok(Self {
            tts: PocketTts {
                binary: env_path("SKIMAP_POCKETTTS_BIN", || {
                    home.join(".cache/skimap/pockettts/build-cuda/pocket-tts")
                }),
                model: env_path("SKIMAP_POCKETTTS_MODEL", || {
                    tts_dir.join("pocket-tts-french-Q8_0.gguf")
                }),
                voices: env_path("SKIMAP_POCKETTTS_VOICES", || {
                    tts_dir.join("pocket-tts-french-voices.gguf")
                }),
                voice: std::env::var("SKIMAP_VOICE").unwrap_or_else(|_| "estelle".into()),
                device: std::env::var("SKIMAP_TTS_DEVICE").ok(),
                seed: num("SKIMAP_TTS_SEED", 42)?,
            },
            ffmpeg: Ffmpeg {
                binary: env_path("SKIMAP_FFMPEG", || PathBuf::from("ffmpeg")),
                kbps: num("SKIMAP_OPUS_KBPS", 24)?,
            },
            shape: Shape::default(),
        })
    }
}

/// One clip in `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipEntry {
    /// Clip id.
    pub id: String,
    /// Kind.
    pub kind: ClipKind,
    /// Text spoken.
    pub text: String,
    /// File, relative to the layer directory.
    pub file: String,
    /// Duration after trimming, ms.
    pub duration_ms: u32,
    /// File size, bytes.
    pub bytes: u64,
    /// Pocket TTS tokens (checked against the pockettts.cpp log).
    pub tokens: usize,
    /// Leading silence removed, ms.
    pub lead_trim_ms: u32,
    /// Trailing silence removed, ms.
    pub tail_trim_ms: u32,
    /// Gain applied, dB.
    pub gain_db: f64,
    /// Whether a cut-off sound at the end of the generated audio was removed.
    pub end_burst: bool,
    /// Noise seed of the kept generation.
    pub seed: u32,
    /// Speech still starts before `MIN_ONSET_MS` after every seed tried (onset maybe cut).
    pub onset_clipped: bool,
}

/// `manifest.json` of the voice layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceManifest {
    /// Format version.
    pub format: u32,
    /// Voice name.
    pub voice: String,
    /// Model file name.
    pub model: String,
    /// SHA-256 of the model file.
    pub model_sha256: String,
    /// First noise seed (clips that needed another one record theirs).
    pub seed: u32,
    /// Sample rate of the clips before encoding, Hz.
    pub sample_rate: u32,
    /// Opus bitrate, kbit/s.
    pub opus_kbps: u32,
    /// Pause durations, ms: short (inside a message), long (between messages).
    pub pauses_ms: (u32, u32),
    /// Clips, sorted by id.
    pub clips: Vec<ClipEntry>,
}

impl VoiceManifest {
    /// Reads `manifest.json` from a voice layer directory.
    pub fn read(dir: &Path) -> Result<Self> {
        let p = dir.join("manifest.json");
        let b = std::fs::read(&p).map_err(|e| Error::io(&p, e))?;
        serde_json::from_slice(&b).map_err(|source| Error::Json { path: p, source })
    }

    /// Entry of a clip id.
    pub fn get(&self, id: &str) -> Option<&ClipEntry> {
        self.clips
            .binary_search_by(|c| c.id.as_str().cmp(id))
            .ok()
            .map(|i| &self.clips[i])
    }
}

/// A data source of the layer, for `voice.meta.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// What it is.
    pub name: String,
    /// Where it comes from.
    pub url: String,
    /// Licence.
    pub licence: String,
    /// Attribution to display.
    pub attribution: String,
}

#[derive(Serialize)]
struct Meta<'a> {
    layer: &'static str,
    site: &'a str,
    generated_at: String,
    valid_until: &'static str,
    sources: Vec<Source>,
    notes: &'static [&'static str],
}

/// Sources of the voice itself (model, voice, engine).
pub fn voice_sources(voice: &str) -> Vec<Source> {
    let mut s = vec![
        Source {
            name: "Pocket TTS, modèle français 6 couches (pack d'octobre), converti en GGUF Q8_0".into(),
            url: "https://huggingface.co/kyutai/pocket-tts-without-voice-cloning/tree/08ffc58775e62aad9409511b9591c9af626270f3/languages/french".into(),
            licence: "CC-BY-4.0, avec les usages interdits de Kyutai (pas de clonage sans consentement, pas de contenu trompeur)".into(),
            attribution: "Voix de synthèse : Pocket TTS, Kyutai".into(),
        },
        Source {
            name: "pockettts.cpp fab79722 + patchs speak 0001-0008".into(),
            url: "https://github.com/ServeurpersoCom/pockettts.cpp".into(),
            licence: "MIT".into(),
            attribution: "pockettts.cpp".into(),
        },
    ];
    if voice == "estelle" {
        s.push(Source {
            name: "Voix estelle (unmute-prod-website/developpeuse-3.wav)".into(),
            url: "https://huggingface.co/kyutai/tts-voices".into(),
            licence: "CC0 (enregistrement de Kyutai)".into(),
            attribution: "Voix estelle, Kyutai".into(),
        });
    }
    s
}

/// Summary of a build.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceBuild {
    /// Layer directory.
    pub dir: PathBuf,
    /// Clips per kind: (count, bytes, audio ms).
    pub per_kind: Vec<(ClipKind, usize, u64, u64)>,
    /// All clips: bytes.
    pub total_bytes: u64,
    /// All clips: audio, s.
    pub total_audio_s: f64,
    /// Largest token count of a clip.
    pub max_tokens: usize,
    /// Leading silence removed: min, median, max, ms.
    pub lead_trim_ms: (u32, u32, u32),
    /// Clips whose cut-off final sound was removed.
    pub end_bursts: usize,
    /// Clips synthesised in each pass (pass k uses seed + k).
    pub passes: Vec<usize>,
    /// Clips whose onset is still at the start after every seed.
    pub onset_clipped: usize,
    /// pockettts.cpp wall time, s (model load included).
    pub tts_wall_s: f64,
    /// Model load, ms.
    pub tts_load_ms: f64,
    /// Encoding wall time, s.
    pub encode_wall_s: f64,
    /// Whole build wall time, s.
    pub total_wall_s: f64,
}

fn sha256_file(p: &Path) -> Result<String> {
    let mut f = std::fs::File::open(p).map_err(|e| Error::io(p, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf).map_err(|e| Error::io(p, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Generates the voice layer of `site` for the pack names `names`. `name_sources` are the
/// sources of the names (OSM, refuges.info...), written to `voice.meta.json`.
pub fn build(
    site: &Site,
    layout: &DataLayout,
    names: &PackNames,
    name_sources: &[Source],
    cfg: &VoiceConfig,
) -> Result<VoiceBuild> {
    let t0 = Instant::now();
    let dir = layout.layer_dir(site, LAYER)?;
    let clips_dir = dir.join("clips");
    if clips_dir.exists() {
        std::fs::remove_dir_all(&clips_dir).map_err(|e| Error::io(&clips_dir, e))?;
    }
    DataLayout::ensure(&clips_dir)?;

    let m = manifest(names);
    let text = PocketText::from_gguf(&cfg.tts.model)?;
    let clips: Vec<(String, ClipKind, String, usize)> = m
        .clips
        .iter()
        .map(|(id, (kind, t))| (id.clone(), *kind, t.clone(), text.count(t)))
        .collect();
    for (id, _, t, n) in &clips {
        if *n > MAX_TOKENS || *n == 0 {
            return Err(Error::Clip {
                id: id.clone(),
                message: format!("{n} tokens (budget {MAX_TOKENS}): {t}"),
            });
        }
    }
    // Generation with quality control: a clip whose speech is cut at the start (or opens
    // on the cold-start click) or never reaches the end of speech is generated again with
    // the next seed, up to SEED_ATTEMPTS seeds; failing that, the latest onset is kept and
    // flagged.
    let mut best: Vec<Option<(u32, Utterance, f64)>> = vec![None; clips.len()];
    let mut done = vec![false; clips.len()];
    let mut pending: Vec<usize> = (0..clips.len()).collect();
    let (mut tts_wall_s, mut tts_load_ms, mut passes) = (0.0, f64::NAN, Vec::new());
    for attempt in 0..SEED_ATTEMPTS {
        if pending.is_empty() {
            break;
        }
        let tts = PocketTts {
            seed: cfg.tts.seed.wrapping_add(attempt),
            ..cfg.tts.clone()
        };
        let lines: Vec<String> = pending.iter().map(|&i| clips[i].2.clone()).collect();
        let (utts, stats) = tts.synthesize(&lines)?;
        tts_wall_s += stats.wall_s;
        if attempt == 0 {
            tts_load_ms = stats.load_ms;
        }
        passes.push(pending.len());
        for (&i, u) in pending.iter().zip(utts) {
            let (id, _, t, n) = &clips[i];
            if u.chunk_tokens != [*n] {
                return Err(Error::Clip {
                    id: id.clone(),
                    message: format!(
                        "pockettts.cpp read {:?} tokens, the Rust port counts {n}: {t}",
                        u.chunk_tokens
                    ),
                });
            }
            let onset = speech_onset_ms(&u.samples, SAMPLE_RATE, &cfg.shape).unwrap_or(-1.0);
            if !u.missing_eos && onset >= MIN_ONSET_MS {
                done[i] = true;
            }
            let better = best[i].as_ref().is_none_or(|(_, b, o)| {
                (b.missing_eos && !u.missing_eos) || (b.missing_eos == u.missing_eos && onset > *o)
            });
            if better {
                best[i] = Some((tts.seed, u, onset));
            }
        }
        pending.retain(|&i| !done[i]);
    }

    let mut shaped = Vec::with_capacity(clips.len());
    for (((id, kind, t, n), b), clean) in clips.into_iter().zip(best).zip(done) {
        let (seed, u, _) = b.expect("every clip was synthesised");
        if u.missing_eos {
            return Err(Error::Clip {
                id,
                message: format!("no end of speech with {SEED_ATTEMPTS} seeds: {t}"),
            });
        }
        let Some((samples, s)) = shape(&u.samples, SAMPLE_RATE, &cfg.shape) else {
            return Err(Error::Clip {
                id,
                message: format!("silent clip: {t}"),
            });
        };
        shaped.push((id, kind, t, n, samples, s, seed, !clean));
    }

    let t_enc = Instant::now();
    let entries: Vec<ClipEntry> = shaped
        .par_iter()
        .map(|(id, kind, t, n, samples, s, seed, clipped)| {
            let file = format!("clips/{id}.opus");
            let path = dir.join(&file);
            cfg.ffmpeg.encode_opus(samples, SAMPLE_RATE, &path)?;
            let bytes = std::fs::metadata(&path)
                .map_err(|e| Error::io(&path, e))?
                .len();
            Ok(ClipEntry {
                id: id.clone(),
                kind: *kind,
                text: t.clone(),
                file,
                duration_ms: (samples.len() as u64 * 1000 / u64::from(SAMPLE_RATE)) as u32,
                bytes,
                tokens: *n,
                lead_trim_ms: s.lead_ms.round() as u32,
                tail_trim_ms: s.tail_ms.round() as u32,
                gain_db: (s.gain_db * 10.0).round() / 10.0,
                end_burst: s.end_burst,
                seed: *seed,
                onset_clipped: *clipped,
            })
        })
        .collect::<Result<_>>()?;
    let encode_wall_s = t_enc.elapsed().as_secs_f64();

    let vm = VoiceManifest {
        format: 1,
        voice: cfg.tts.voice.clone(),
        model: cfg
            .tts
            .model
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        model_sha256: sha256_file(&cfg.tts.model)?,
        seed: cfg.tts.seed,
        sample_rate: SAMPLE_RATE,
        opus_kbps: cfg.ffmpeg.kbps,
        pauses_ms: (
            crate::bank::Pause::Short.ms(),
            crate::bank::Pause::Long.ms(),
        ),
        clips: entries,
    };
    write_json(&dir.join("manifest.json"), &vm)?;
    let mut sources = voice_sources(&cfg.tts.voice);
    sources.extend_from_slice(name_sources);
    write_json(
        &dir.join(format!("{LAYER}.meta.json")),
        &Meta {
            layer: LAYER,
            site: site.id,
            generated_at: chrono::Utc::now().to_rfc3339(),
            valid_until: "jusqu'au prochain changement des noms du pack (pistes, refuges, lieux, zones)",
            sources,
            notes: &[
                "Clips Ogg/Opus mono, silence de début et de fin coupé, niveau normalisé.",
                "Tout texte absent du manifeste est lu par la voix Android (repli).",
            ],
        },
    )?;

    let mut per_kind = Vec::new();
    for k in [ClipKind::Fixed, ClipKind::Value, ClipKind::Name] {
        let it = vm.clips.iter().filter(|c| c.kind == k);
        per_kind.push((
            k,
            it.clone().count(),
            it.clone().map(|c| c.bytes).sum(),
            it.map(|c| u64::from(c.duration_ms)).sum(),
        ));
    }
    let mut leads: Vec<u32> = vm.clips.iter().map(|c| c.lead_trim_ms).collect();
    leads.sort_unstable();
    Ok(VoiceBuild {
        dir,
        per_kind,
        total_bytes: vm.clips.iter().map(|c| c.bytes).sum(),
        total_audio_s: vm
            .clips
            .iter()
            .map(|c| f64::from(c.duration_ms))
            .sum::<f64>()
            / 1000.0,
        max_tokens: vm.clips.iter().map(|c| c.tokens).max().unwrap_or(0),
        lead_trim_ms: (
            leads.first().copied().unwrap_or(0),
            leads.get(leads.len() / 2).copied().unwrap_or(0),
            leads.last().copied().unwrap_or(0),
        ),
        end_bursts: vm.clips.iter().filter(|c| c.end_burst).count(),
        passes,
        onset_clipped: vm.clips.iter().filter(|c| c.onset_clipped).count(),
        tts_wall_s,
        tts_load_ms,
        encode_wall_s,
        total_wall_s: t0.elapsed().as_secs_f64(),
    })
}

fn write_json(path: &Path, v: &impl Serialize) -> Result<()> {
    let b = serde_json::to_vec_pretty(v).map_err(|source| Error::Json {
        path: path.to_owned(),
        source,
    })?;
    std::fs::write(path, b).map_err(|e| Error::io(path, e))
}

/// An assembled announcement.
#[derive(Debug, Clone, Serialize)]
pub struct Assembled {
    /// Text of the announcement.
    pub text: String,
    /// Clip ids played.
    pub clips: Vec<String>,
    /// Duration, s.
    pub duration_s: f64,
    /// Files written (WAV, Ogg/Opus).
    pub files: Vec<PathBuf>,
}

/// Assembles `a` as the phone does: decodes each Ogg/Opus clip of the layer in `dir`,
/// concatenates them with the pauses of the announcement, and writes `<stem>.wav` and
/// `<stem>.opus`. Fails if a clip is missing (the phone would use the fallback voice).
pub fn assemble(
    dir: &Path,
    vm: &VoiceManifest,
    a: &Announcement,
    ffmpeg: &Ffmpeg,
    stem: &Path,
) -> Result<Assembled> {
    let mut out: Vec<i16> = Vec::new();
    let sr = vm.sample_rate;
    for (i, s) in a.segments.iter().enumerate() {
        let e = vm.get(&s.clip.id).ok_or_else(|| Error::Clip {
            id: s.clip.id.clone(),
            message: "not in the pack (fallback voice)".into(),
        })?;
        if e.text != s.clip.text {
            return Err(Error::Clip {
                id: s.clip.id.clone(),
                message: format!("pack text {:?} differs from {:?}", e.text, s.clip.text),
            });
        }
        out.extend(ffmpeg.decode(&dir.join(&e.file), sr)?);
        if i + 1 < a.segments.len() {
            out.extend(std::iter::repeat_n(
                0i16,
                (s.pause.ms() * sr / 1000) as usize,
            ));
        }
    }
    let wav = stem.with_extension("wav");
    let opus = stem.with_extension("opus");
    if let Some(p) = stem.parent() {
        DataLayout::ensure(p)?;
    }
    write_wav(&wav, &out, sr)?;
    ffmpeg.encode_opus(&out, sr, &opus)?;
    Ok(Assembled {
        text: a.text(),
        clips: a.clip_ids().into_iter().map(str::to_owned).collect(),
        duration_s: out.len() as f64 / f64::from(sr),
        files: vec![wav, opus],
    })
}
