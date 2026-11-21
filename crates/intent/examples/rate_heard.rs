//! Rate of good command on what the speech recognizer heard (VO5 #87): the transcriptions
//! written by the end-to-end test of `android/voicecmd` (`VoiceCommandAudioTest`, one row
//! per spoken question: `file, label, lang, voice, snr, heard[, decode_ms]`) are classified
//! here with the core's classifier, in the language of the question as the phone's
//! default, and counted per SNR, language and voice.
//!
//! ```sh
//! cargo run --release -p skimap-intent --example rate_heard -- android/voicecmd/build/voicecmd-heard.tsv
//! ```
//!
//! With `--errors`, every wrong French command is listed with what was heard.

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::collections::BTreeMap;

use skimap_intent::measure::class_name;
use skimap_intent::{Lang, classify};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let errors = args.iter().any(|a| a == "--errors");
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: rate_heard <heard.tsv> [--errors]");
        std::process::exit(2);
    };
    let tsv = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut rates: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for line in tsv.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let [file, label, lang, voice, snr, heard, ..] = f[..] else {
            panic!("bad row: {line}");
        };
        let l = if lang == "fr" { Lang::Fr } else { Lang::En };
        let got = class_name(classify(heard, l).intent);
        let ok = got == label;
        let kind = if label == "unknown" {
            "outside"
        } else {
            "commands"
        };
        for k in [
            snr.to_string(),
            format!("{snr}/{lang}"),
            format!("{snr}/{lang}/{kind}"),
            format!("{snr}/{lang}/{voice}"),
        ] {
            let e = rates.entry(k).or_default();
            e.0 += usize::from(ok);
            e.1 += 1;
        }
        if errors && !ok && lang == "fr" {
            println!("{file}\t{label} -> {got}\t« {heard} »");
        }
    }
    for (k, (ok, n)) in &rates {
        println!(
            "{k:<28} {ok:>4} / {n:>4} = {:>5.1} %",
            100.0 * *ok as f64 / *n as f64
        );
    }
}
