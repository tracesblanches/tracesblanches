//! Clip shaping with exact figures (Q4, #104: survivors of `cargo mutants -p
//! skimap-voice` in `audio.rs`): trimming margins, level, fades, onset, WAV header.

use skimap_voice::audio::{Shape, shape, speech_onset_ms, write_wav};

const RATE: u32 = 24_000;
/// Analysis window of the module, samples (10 ms at 24 kHz).
const WIN: usize = 240;

/// `lead` windows of silence, `speech` windows of a square wave of amplitude `amp`,
/// `tail` windows of silence.
fn clip(lead: usize, speech: usize, tail: usize, amp: i16) -> Vec<i16> {
    let mut v = vec![0i16; lead * WIN];
    v.extend((0..speech * WIN).map(|i| if (i / 12) % 2 == 0 { amp } else { -amp }));
    v.extend(vec![0i16; tail * WIN]);
    v
}

#[test]
fn margins_level_and_fades() {
    let p = Shape::default();
    let s = clip(50, 30, 60, 1000);
    // Onset: first speech window, 50 × 10 ms.
    assert_eq!(speech_onset_ms(&s, RATE, &p), Some(500.0));
    let (out, info) = shape(&s, RATE, &p).unwrap();
    // 40 ms kept before, 80 ms after the speech.
    assert!(
        (info.lead_ms - (500.0 - f64::from(p.pre_ms))).abs() < 1e-9,
        "{info:?}"
    );
    assert!(
        (info.tail_ms - (600.0 - f64::from(p.post_ms))).abs() < 1e-9,
        "{info:?}"
    );
    assert_eq!(out.len(), (30 * 10 + 40 + 80) * 24);
    assert!(!info.end_burst);
    // Square wave of amplitude 1000: RMS 1000 → gain to −20 dBFS, below the ceiling.
    let rms_db = 20.0 * (1000.0f64 / 32768.0).log10();
    assert!(
        (info.gain_db - (p.target_dbfs - rms_db)).abs() < 1e-9,
        "{info:?}"
    );
    let g = 10f64.powf(info.gain_db / 20.0);
    // Fades of 5 ms (120 samples) at both ends; silence before the speech stays zero.
    let fade = (f64::from(p.fade_ms) * 24.0) as usize;
    assert_eq!(out[0], 0);
    let first = 40 * 24; // first speech sample
    assert_eq!(out[first], (1000.0 * g).round() as i16);
    assert_eq!(out[first + 12], -(1000.0 * g).round() as i16);
    assert!(fade < first);
    let last = out.len() - 1;
    assert_eq!(out[last], 0);
    // The level of the speech is the target.
    let speech = &out[first..first + 30 * WIN];
    let rms =
        (speech.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / speech.len() as f64).sqrt();
    assert!((20.0 * (rms / 32768.0).log10() - p.target_dbfs).abs() < 0.01);
}

#[test]
fn fades_ramp_linearly() {
    let p = Shape {
        pre_ms: 0,
        post_ms: 0,
        ..Shape::default()
    };
    let s = clip(0, 20, 0, 3000);
    let (out, info) = shape(&s, RATE, &p).unwrap();
    assert_eq!((info.lead_ms, info.tail_ms), (0.0, 0.0));
    assert_eq!(out.len(), s.len());
    let g = 10f64.powf(info.gain_db / 20.0);
    let fade = 120usize;
    for i in [0usize, 1, 60, 119] {
        let want = (f64::from(s[i]) * g * i as f64 / fade as f64).round() as i16;
        assert_eq!(out[i], want, "sample {i}");
    }
    assert_eq!(out[120], (f64::from(s[120]) * g).round() as i16);
    let n = out.len();
    for k in [0usize, 1, 119] {
        let i = n - 1 - k;
        let want = (f64::from(s[i]) * g * k as f64 / fade as f64).round() as i16;
        assert_eq!(out[i], want, "sample {i}");
    }
}

#[test]
fn silence_has_no_onset_and_quiet_speech_counts() {
    let p = Shape::default();
    assert_eq!(speech_onset_ms(&vec![0i16; 10 * WIN], RATE, &p), None);
    assert!(shape(&vec![0i16; 10 * WIN], RATE, &p).is_none());
    // A window 39 dB below the peak is speech, 41 dB below is not.
    let mut s = clip(5, 1, 0, 340);
    s.extend(clip(5, 10, 5, 30000));
    let on = speech_onset_ms(&s, RATE, &p).unwrap();
    assert_eq!(on, 50.0, "{on}");
    let mut s = clip(5, 1, 0, 260);
    s.extend(clip(5, 10, 5, 30000));
    assert_eq!(speech_onset_ms(&s, RATE, &p).unwrap(), 110.0);
}

#[test]
fn wav_header_fields() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.wav");
    write_wav(&p, &[1, -1, 2, 7], RATE).unwrap();
    let b = std::fs::read(&p).unwrap();
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(u32_at(4), 36 + 8);
    assert_eq!(&b[8..16], b"WAVEfmt ");
    assert_eq!((u32_at(16), u16_at(20), u16_at(22)), (16, 1, 1));
    assert_eq!((u32_at(24), u32_at(28)), (RATE, 2 * RATE));
    assert_eq!((u16_at(32), u16_at(34)), (2, 16));
    assert_eq!(&b[36..40], b"data");
    assert_eq!(u32_at(40), 8);
    assert_eq!(i16::from_le_bytes([b[46], b[47]]), -1);
}
