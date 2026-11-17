//! VO6: the names of a site said by the English voice, for the transcription check of
//! the French proper names (`docs/voix/anglais.md`).
//!
//! ```sh
//! SKIMAP_DATA_DIR=~/projects/skimap/data cargo run --release -p skimap-voice \
//!     --example en_names -- <out dir> <voice> <site>...
//! ```
//!
//! Writes `<out>/<voice>/<site>/<n>.wav` and `<out>/<voice>/<site>/texts.tsv`
//! (`file<TAB>clip id<TAB>text`): every public name clip of the site in English
//! (pistes with their colour, places), with the English model, seed 42.

use std::io::Write;

use skimap_common::config::Config;
use skimap_common::site;
use skimap_voice::manifest::{ClipKind, manifest_in};
use skimap_voice::pack::VoiceConfig;
use skimap_voice::tts::{PocketTts, SAMPLE_RATE};
use skimap_voice::{Lang, site_names};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> Res<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [out, voice, sites @ ..] = args.as_slice() else {
        return Err("usage: en_names <out dir> <voice> <site>...".into());
    };
    let layout = Config::from_env()?.layout();
    let cfg = VoiceConfig::from_env_lang(Lang::En)?;
    let tts = PocketTts {
        voice: voice.clone(),
        ..cfg.tts.clone()
    };
    for s in sites {
        let site = site::site(s)?;
        let names = site_names(&layout, site)?;
        let m = manifest_in(&names.public, Lang::En);
        let clips: Vec<(&String, &String)> = m
            .clips
            .iter()
            .filter(|(_, (k, _))| *k == ClipKind::Name)
            .map(|(id, (_, t))| (id, t))
            .collect();
        let lines: Vec<String> = clips.iter().map(|(_, t)| (*t).clone()).collect();
        let (utts, stats) = tts.synthesize(&lines)?;
        let dir = std::path::Path::new(out).join(voice).join(s);
        std::fs::create_dir_all(&dir)?;
        let mut tsv = std::fs::File::create(dir.join("texts.tsv"))?;
        for (i, ((id, text), u)) in clips.iter().zip(utts).enumerate() {
            let f = format!("{i:04}.wav");
            skimap_voice::audio::write_wav(&dir.join(&f), &u.samples, SAMPLE_RATE)?;
            writeln!(tsv, "{f}\t{id}\t{text}")?;
        }
        println!(
            "{s}: {} name clips, voice {voice}, TTS {:.1} s",
            clips.len(),
            stats.wall_s
        );
    }
    Ok(())
}
