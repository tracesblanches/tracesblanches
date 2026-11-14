//! Against the real pockettts.cpp and French model (workstation only):
//! `cargo test -p skimap-voice --test pocket -- --ignored`. Paths: see `VoiceConfig::from_env`.

use skimap_voice::VoiceConfig;
use skimap_voice::audio::shape;
use skimap_voice::manifest::{PackNames, manifest};
use skimap_voice::names::{Colour, PisteName};
use skimap_voice::tokenizer::PocketText;
use skimap_voice::tts::SAMPLE_RATE;

#[test]
#[ignore = "needs the French Pocket TTS model (docs/voix.md)"]
fn token_counts_of_the_french_model() {
    let cfg = VoiceConfig::from_env().unwrap();
    let t = PocketText::from_gguf(&cfg.tts.model).unwrap();
    // The French pack: no padding, `;` -> `,`, quotes, `’`, `:` and brackets deleted.
    assert!(!t.rules.pad_with_spaces && t.rules.remove_semicolons);
    assert_eq!(t.rules.replace.len(), 13);
    // Logged by pocket-tts ("[TTS] chunk 1/1: N tokens") on 2026-11-14.
    assert_eq!(t.count("Dans deux cents mètres, tournez à gauche."), 13);
    assert_eq!(t.count("Greffier, piste bleue."), 10);
    let names = PackNames {
        site: "Chapelle-des-Bois".into(),
        pistes: vec![PisteName::new(
            "Transjurassienne (itinéraire course uniquement)",
            Some(Colour::Red),
        )],
        places: vec!["Massif du Mont-d'Or, du Noirmont et du Risol".into()],
        dates: vec![],
        status_pistes: 120,
    };
    let max = manifest(&names)
        .clips
        .values()
        .map(|(_, text)| t.count(text))
        .max()
        .unwrap();
    assert!(max <= 50, "{max}");
}

#[test]
#[ignore = "needs pockettts.cpp, the French model and ffmpeg (docs/voix.md)"]
fn synthesis_trim_and_opus_round_trip() {
    let cfg = VoiceConfig::from_env().unwrap();
    let t = PocketText::from_gguf(&cfg.tts.model).unwrap();
    let lines: Vec<String> = [
        "Vous avez quitté l'itinéraire.",
        "Greffier, piste noire.",
        "Il reste quatre kilomètres.",
    ]
    .map(String::from)
    .to_vec();
    let (utts, stats) = cfg.tts.synthesize(&lines).unwrap();
    assert_eq!(utts.len(), 3);
    assert!(stats.audio_s > 2.0, "{stats:?}");
    let dir = tempfile::tempdir().unwrap();
    for (l, u) in lines.iter().zip(&utts) {
        assert_eq!(u.chunk_tokens, [t.count(l)], "{l}");
        assert!(!u.missing_eos, "{l}");
        let (s, sh) = shape(&u.samples, SAMPLE_RATE, &cfg.shape).unwrap();
        assert!(s.len() < u.samples.len() && sh.tail_ms > 0.0, "{l}: {sh:?}");
        let p = dir.path().join("c.opus");
        cfg.ffmpeg.encode_opus(&s, SAMPLE_RATE, &p).unwrap();
        let back = cfg.ffmpeg.decode(&p, SAMPLE_RATE).unwrap();
        // Ogg/Opus keeps the exact length (pre-skip and end trimming in the granule).
        assert!(
            back.len().abs_diff(s.len()) <= 1,
            "{} vs {}",
            back.len(),
            s.len()
        );
    }
}
