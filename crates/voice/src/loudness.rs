//! Loudness of the clips (#179): measure (ITU-R BS.1770-4 / EBU R128, through the
//! `ebur128` crate) and normalisation of every clip to one target.
//!
//! Target ([`Target::default`]): integrated loudness -10 LUFS, true peak at most -1 dBTP.
//! The Pocket TTS speech normalised to -20 dBFS RMS measured about -21 LUFS with 8 to 9 dB
//! of unused peak headroom: heard « beaucoup trop faible » on a phone. Brought to -14 LUFS
//! (the level of the guidance voices of navigation apps) it was still « à peine audible »
//! at the usual media volume of a phone (about 25 %): the clips are now as loud as speech
//! can be under the peak ceiling, 4 dB above that level, which takes compression.
//!
//! [`normalize`]: a speech compressor ([`Compressor`]: 3:1 above the loud vowels, the
//! threshold relative to the clip's own loudness, so the same speech at any level is
//! compressed the same) brings the loud syllables closer to the rest, then one gain to the
//! target loudness, then a soft look-ahead limiter on the 4x oversampled peaks (no
//! clipping, no step in the gain) when the gain would push a peak above the ceiling; the
//! gain is refined until the limited clip is at the target. The limiter takes off at most
//! 6 dB ([`Target::max_limiting_db`]): a clip whose loudness sits on one loud syllable
//! keeps that bound and stays a little under the target. Pure `f64` arithmetic in a fixed
//! order: the same samples give the same output bit for bit, and a clip already at the
//! target is returned unchanged.

use ebur128::{EbuR128, Mode};

use crate::error::{Error, Result};

/// Version of the normalisation ([`normalize`], its limiter and the check of the encoded
/// file in `pack`): a change that moves a sample of a clip increments it, and every clip
/// of the cache (`cache/voice/`, key `pack::clip_key`) is made again.
/// 2: speech compression before the gain ([`Compressor`], -10 LUFS target, #179).
pub const ALGORITHM_VERSION: u32 = 2;

/// Loudness of a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Integrated loudness (gated, BS.1770-4), LUFS. A clip shorter than one 400 ms
    /// gating block is measured over its whole length (ungated).
    pub lufs: f64,
    /// True peak (4x oversampled), dBTP.
    pub true_peak_dbtp: f64,
    /// Sample peak, dBFS.
    pub sample_peak_dbfs: f64,
}

/// Loudness target of the clips.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// Integrated loudness, LUFS.
    pub lufs: f64,
    /// Highest true peak of a published (decoded) clip, dBTP.
    pub true_peak_dbtp: f64,
    /// A clip within this many LU of the target (and under the ceiling) is left as it is.
    pub tolerance_lu: f64,
    /// The limiter keeps the peaks this far under `true_peak_dbtp` before encoding: the
    /// lossy encoder moves the peaks a little.
    pub margin_db: f64,
    /// Limiter look-ahead (and hold), ms: the gain goes down over this time before a peak.
    pub lookahead_ms: f64,
    /// Limiter release time constant, ms.
    pub release_ms: f64,
    /// Largest gain reduction of the limiter, dB: beyond it the clip stays under the target.
    pub max_limiting_db: f64,
    /// Speech compression before the gain.
    pub compressor: Compressor,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            lufs: -10.0,
            true_peak_dbtp: -1.0,
            tolerance_lu: 0.5,
            margin_db: 0.5,
            lookahead_ms: 5.0,
            release_ms: 60.0,
            max_limiting_db: 6.0,
            compressor: Compressor::default(),
        }
    }
}

/// Speech compressor of [`normalize`]: the level of the clip (RMS over a short window
/// centred on each sample) above a threshold set relative to the clip's integrated
/// loudness is reduced by `ratio`, with a soft knee, the gain smoothed in dB (fast attack,
/// slower release). Offline and centred: no lag behind the syllable it compresses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compressor {
    /// Threshold, dB relative to the clip's integrated loudness (RMS dBFS against LUFS).
    pub threshold_lu: f64,
    /// Compression ratio above the threshold (1: no compression).
    pub ratio: f64,
    /// Width of the soft knee, dB (centred on the threshold).
    pub knee_db: f64,
    /// RMS window of the detection, ms.
    pub window_ms: f64,
    /// Attack time constant of the gain, ms.
    pub attack_ms: f64,
    /// Release time constant of the gain, ms.
    pub release_ms: f64,
}

impl Default for Compressor {
    fn default() -> Self {
        Self {
            threshold_lu: -3.0,
            ratio: 3.0,
            knee_db: 6.0,
            window_ms: 10.0,
            attack_ms: 5.0,
            release_ms: 80.0,
        }
    }
}

impl Compressor {
    /// Gain of a level `over` dB above the threshold, dB (≤ 0).
    fn reduction_db(&self, over: f64) -> f64 {
        let slope = 1.0 / self.ratio - 1.0;
        let half = self.knee_db / 2.0;
        if over <= -half {
            0.0
        } else if over >= half {
            slope * over
        } else {
            slope * (over + half).powi(2) / (2.0 * self.knee_db)
        }
    }

    /// `x` compressed for a clip of integrated loudness `lufs`, and the largest gain
    /// reduction, dB (≥ 0).
    pub fn apply(&self, x: &[f64], sample_rate: u32, lufs: f64) -> (Vec<f64>, f64) {
        let n = x.len();
        if n == 0 || self.ratio <= 1.0 {
            return (x.to_vec(), 0.0);
        }
        let fs = f64::from(sample_rate);
        let half = ((self.window_ms * fs / 2000.0).round() as usize).max(1);
        let mut prefix = vec![0.0; n + 1];
        for i in 0..n {
            prefix[i + 1] = prefix[i] + x[i] * x[i];
        }
        let threshold = lufs + self.threshold_lu;
        let attack = (-1000.0 / (self.attack_ms * fs)).exp();
        let release = (-1000.0 / (self.release_ms * fs)).exp();
        let mut state = 0.0f64;
        let mut deepest = 0.0f64;
        let mut y = Vec::with_capacity(n);
        for (i, &v) in x.iter().enumerate() {
            let lo = i.saturating_sub(half);
            let hi = (i + half).min(n - 1);
            let ms = (prefix[hi + 1] - prefix[lo]) / (hi + 1 - lo) as f64;
            let want = if ms > 0.0 {
                self.reduction_db(10.0 * ms.log10() - threshold)
            } else {
                0.0
            };
            let a = if want < state { attack } else { release };
            state = a * state + (1.0 - a) * want;
            deepest = deepest.min(state);
            y.push(v * 10f64.powf(state / 20.0));
        }
        (y, -deepest)
    }
}

impl Target {
    /// Limiter ceiling before encoding, dBTP.
    pub fn ceiling_dbtp(&self) -> f64 {
        self.true_peak_dbtp - self.margin_db
    }

    /// Whether a measured clip is at the target.
    pub fn accepts(&self, l: &Loudness, ceiling_dbtp: f64) -> bool {
        (l.lufs - self.lufs).abs() <= self.tolerance_lu && l.true_peak_dbtp <= ceiling_dbtp
    }

    /// Whether a normalised clip is right: at the target, or under it when the limiter
    /// reached its bound (`capped`).
    pub fn accepts_level(&self, l: &Loudness, ceiling_dbtp: f64, capped: bool) -> bool {
        self.accepts(l, ceiling_dbtp)
            || (capped && l.lufs < self.lufs && l.true_peak_dbtp <= ceiling_dbtp)
    }
}

fn db(linear: f64) -> f64 {
    if linear <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

fn ebu_err(e: ebur128::Error) -> Error {
    Error::Audio(format!("loudness measure: {e}"))
}

/// Shortest clip measured by the gated integrated loudness: one 400 ms block.
const BLOCK_MS: u64 = 400;

fn measure_with(
    sample_rate: u32,
    len: usize,
    add: impl FnOnce(&mut EbuR128) -> std::result::Result<(), ebur128::Error>,
) -> Result<Loudness> {
    let mut m = EbuR128::new(
        1,
        sample_rate,
        Mode::I | Mode::TRUE_PEAK | Mode::SAMPLE_PEAK,
    )
    .map_err(ebu_err)?;
    let ms = len as u64 * 1000 / u64::from(sample_rate);
    add(&mut m).map_err(ebu_err)?;
    let lufs = if ms >= BLOCK_MS {
        m.loudness_global().map_err(ebu_err)?
    } else {
        // Shorter than one gating block: the mean square of the whole clip, K-weighted.
        m.loudness_window(ms.max(1) as u32).map_err(ebu_err)?
    };
    Ok(Loudness {
        lufs,
        true_peak_dbtp: db(m.true_peak(0).map_err(ebu_err)?),
        sample_peak_dbfs: db(m.sample_peak(0).map_err(ebu_err)?),
    })
}

/// Loudness of mono 16-bit samples.
pub fn measure(samples: &[i16], sample_rate: u32) -> Result<Loudness> {
    measure_with(sample_rate, samples.len(), |m| m.add_frames_i16(samples))
}

/// Loudness of mono samples in [-1, 1].
pub fn measure_f64(samples: &[f64], sample_rate: u32) -> Result<Loudness> {
    measure_with(sample_rate, samples.len(), |m| m.add_frames_f64(samples))
}

/// What [`normalize`] did to a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Normalized {
    /// Loudness before.
    pub before: Loudness,
    /// Loudness after (16-bit output).
    pub after: Loudness,
    /// Largest gain reduction of the speech compressor, dB (0: not compressed).
    pub compressor_db: f64,
    /// Gain applied before the limiter, dB (0 when the clip was left as it is).
    pub gain_db: f64,
    /// Largest gain reduction of the limiter, dB (0: not limited).
    pub limiter_db: f64,
    /// The limiter reached [`Target::max_limiting_db`]: the clip is under the target.
    pub capped: bool,
}

/// Half length of the interpolation kernel, input samples.
const TAPS: usize = 16;
/// Oversampling of the peak detection.
const OVERSAMPLE: usize = 4;

/// Hann-windowed sinc kernels of the fractional positions 1/4, 2/4, 3/4: `k[p][j]` weighs
/// `x[n + j - TAPS + 1]` for the point `n + (p + 1) / 4`.
fn kernels() -> [[f64; 2 * TAPS]; OVERSAMPLE - 1] {
    let mut k = [[0.0; 2 * TAPS]; OVERSAMPLE - 1];
    for (p, kp) in k.iter_mut().enumerate() {
        let f = (p + 1) as f64 / OVERSAMPLE as f64;
        for (j, w) in kp.iter_mut().enumerate() {
            // Distance from the interpolated point to the input sample.
            let t = (j as f64 - (TAPS as f64 - 1.0)) - f;
            let x = std::f64::consts::PI * t;
            let sinc = if t == 0.0 { 1.0 } else { x.sin() / x };
            let hann = 0.5 * (1.0 + (std::f64::consts::PI * t / TAPS as f64).cos());
            *w = sinc * hann;
        }
    }
    k
}

/// Peak of every sample and of the 3 interpolated points after it (absolute values).
fn oversampled_peaks(x: &[f64]) -> Vec<f64> {
    let k = kernels();
    let n = x.len();
    let at = |i: isize| -> f64 {
        if i < 0 || i as usize >= n {
            0.0
        } else {
            x[i as usize]
        }
    };
    (0..n)
        .map(|i| {
            let mut p = x[i].abs();
            for kp in &k {
                let mut s = 0.0;
                for (j, w) in kp.iter().enumerate() {
                    s += w * at(i as isize + j as isize - (TAPS as isize - 1));
                }
                p = p.max(s.abs());
            }
            p
        })
        .collect()
}

/// Running minimum over `[i - r, i + r]` (monotonic queue).
fn running_min(v: &[f64], r: usize) -> Vec<f64> {
    let n = v.len();
    let mut out = vec![0.0; n];
    let mut q: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut next = 0;
    for (i, o) in out.iter_mut().enumerate() {
        let hi = (i + r).min(n - 1);
        while next <= hi {
            while q.back().is_some_and(|&b| v[b] >= v[next]) {
                q.pop_back();
            }
            q.push_back(next);
            next += 1;
        }
        while q.front().is_some_and(|&f| f + r < i) {
            q.pop_front();
        }
        *o = v[*q.front().expect("window not empty")];
    }
    out
}

/// Gain envelope of the limiter for the peaks `peaks` after a gain `g`: at most
/// `ceiling / (g * peak)` at every sample, ramps of `2r + 1` samples, exponential release.
fn envelope(peaks: &[f64], g: f64, ceiling: f64, r: usize, release: f64) -> Vec<f64> {
    let req: Vec<f64> = peaks
        .iter()
        .map(|&p| {
            if g * p > ceiling {
                ceiling / (g * p)
            } else {
                1.0
            }
        })
        .collect();
    if req.iter().all(|&x| x >= 1.0) {
        return req;
    }
    // Hold: every sample within r of a peak gets at most that peak's gain.
    let mut h = running_min(&req, r);
    // Release towards 1, never above the hold.
    for i in 1..h.len() {
        let up = h[i - 1] + (1.0 - h[i - 1]) * release;
        h[i] = h[i].min(up);
    }
    // Smoothing: moving average over [i - r, i + r]. Every value averaged is at most the
    // gain asked at i (each one holds the minimum of a window that contains i).
    let n = h.len();
    let mut prefix = vec![0.0; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + h[i];
    }
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(r);
            let hi = (i + r).min(n - 1);
            // Outside the clip the gain is the edge value.
            let inside = prefix[hi + 1] - prefix[lo];
            let before = (r - (i - lo)) as f64 * h[0];
            let after = (r - (hi - i)) as f64 * h[n - 1];
            (inside + before + after) / (2 * r + 1) as f64
        })
        .collect()
}

/// Rounds to 16 bits.
fn to_i16(x: &[f64]) -> Vec<i16> {
    x.iter()
        .map(|&v| (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

/// Brings mono 16-bit samples to the target loudness under the peak ceiling
/// `ceiling_dbtp` (usually [`Target::ceiling_dbtp`]): compressed ([`Target::compressor`]),
/// raised, limited. A clip already within the tolerance and under the ceiling is returned
/// unchanged.
///
/// The limiter takes off at most [`Target::max_limiting_db`]: a clip whose loudness sits
/// on a few loud syllables (« Sept heures trente. », the first word 15 dB above the rest)
/// would otherwise be squashed by 15 dB to reach the target. Such a clip keeps the highest
/// gain under that bound ([`Normalized::capped`]), a little under the target.
pub fn normalize(
    samples: &[i16],
    sample_rate: u32,
    t: &Target,
    ceiling_dbtp: f64,
) -> Result<(Vec<i16>, Normalized)> {
    let before = measure(samples, sample_rate)?;
    if !before.lufs.is_finite() {
        return Err(Error::Audio("silent clip: no loudness to normalise".into()));
    }
    if t.accepts(&before, ceiling_dbtp) {
        return Ok((
            samples.to_vec(),
            Normalized {
                before,
                after: before,
                compressor_db: 0.0,
                gain_db: 0.0,
                limiter_db: 0.0,
                capped: false,
            },
        ));
    }
    let raw: Vec<f64> = samples.iter().map(|&s| f64::from(s) / 32768.0).collect();
    let (x, compressor_db) = t.compressor.apply(&raw, sample_rate, before.lufs);
    let compressed = measure_f64(&x, sample_rate)?;
    let peaks = oversampled_peaks(&x);
    let peak_db = db(peaks.iter().copied().fold(0.0, f64::max));
    let r = ((t.lookahead_ms * f64::from(sample_rate) / 1000.0).round() as usize).max(1);
    let release = 1.0 - (-1000.0 / (t.release_ms * f64::from(sample_rate))).exp();
    let mut ceiling_db = ceiling_dbtp;
    let mut gain_db = t.lufs - compressed.lufs;
    let mut best: Option<(Vec<i16>, Normalized)> = None;
    // The gain reached the bound of the limiter: the target is out of reach.
    let mut reached_cap = false;
    for _ in 0..16 {
        // Highest gain whose limiting stays within the bound.
        let cap_db = ceiling_db + t.max_limiting_db - peak_db;
        let capped = gain_db >= cap_db;
        reached_cap |= capped;
        gain_db = gain_db.min(cap_db);
        let g = 10f64.powf(gain_db / 20.0);
        let env = envelope(&peaks, g, 10f64.powf(ceiling_db / 20.0), r, release);
        let y: Vec<f64> = x.iter().zip(&env).map(|(&v, &e)| v * g * e).collect();
        let out = to_i16(&y);
        let after = measure(&out, sample_rate)?;
        let n = Normalized {
            before,
            after,
            compressor_db,
            gain_db,
            limiter_db: -db(env.iter().copied().fold(1.0, f64::min)),
            capped: capped && after.lufs < t.lufs,
        };
        let under = after.true_peak_dbtp <= ceiling_dbtp;
        let miss = |l: &Normalized| (l.after.lufs - t.lufs).abs();
        if under && best.as_ref().is_none_or(|(_, b)| miss(&n) < miss(b)) {
            best = Some((out, n));
        }
        if !under {
            // The interpolated peaks of the 16-bit output are above the detection's: lower
            // the ceiling of the limiter by the excess.
            ceiling_db -= after.true_peak_dbtp - ceiling_dbtp + 0.005;
            continue;
        }
        if miss(&n) <= 0.05 || n.capped {
            break;
        }
        gain_db += t.lufs - after.lufs;
    }
    let (out, mut n) = best.ok_or_else(|| {
        Error::Audio(format!(
            "loudness: peaks above {ceiling_dbtp} dBTP after limiting (clip at {:.1} LUFS)",
            before.lufs
        ))
    })?;
    n.capped = reached_cap && n.after.lufs < t.lufs;
    if !t.accepts_level(&n.after, ceiling_dbtp, n.capped) {
        return Err(Error::Audio(format!(
            "loudness: {:.2} LUFS, {:.2} dBTP after normalisation (target {} LUFS, ceiling {ceiling_dbtp} dBTP)",
            n.after.lufs, n.after.true_peak_dbtp, t.lufs
        )));
    }
    Ok((out, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 24_000;

    /// A 1 kHz sine of `dbfs` peak, `ms` long.
    fn sine(dbfs: f64, hz: f64, ms: u32) -> Vec<i16> {
        let a = 32768.0 * 10f64.powf(dbfs / 20.0);
        (0..(SR * ms / 1000) as usize)
            .map(|i| {
                (a * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(SR)).sin()).round()
                    as i16
            })
            .collect()
    }

    /// Speech-like test signal: a 300 Hz carrier in 4 Hz syllables at a fifth of the peak,
    /// and a 1 ms burst of `burst` (relative to the peak) every 250 ms.
    fn speechy(dbfs: f64, burst: f64) -> Vec<i16> {
        let a = 32768.0 * 10f64.powf(dbfs / 20.0);
        (0..SR as usize * 2)
            .map(|i| {
                let t = i as f64 / f64::from(SR);
                let syl = (2.0 * std::f64::consts::PI * 4.0 * t).sin().abs().powi(2);
                let voice = 0.2 * syl * (2.0 * std::f64::consts::PI * 300.0 * t).sin();
                let b = if i % 6000 < 24 {
                    burst * (2.0 * std::f64::consts::PI * 3000.0 * t).sin()
                } else {
                    0.0
                };
                (a * (voice + b).clamp(-1.0, 1.0)).round() as i16
            })
            .collect()
    }

    /// The limiter alone, at the target before the compression (-14 LUFS).
    fn limiter_only() -> Target {
        Target {
            lufs: -14.0,
            compressor: Compressor {
                ratio: 1.0,
                ..Compressor::default()
            },
            ..Target::default()
        }
    }

    /// Syllables of 125 ms alternately at `loud` and `loud - 12 dB`, a 300 Hz carrier.
    fn two_levels(loud: f64) -> Vec<f64> {
        let a = 10f64.powf(loud / 20.0);
        (0..SR as usize * 2)
            .map(|i| {
                let t = i as f64 / f64::from(SR);
                let k = (t * 8.0) as usize;
                let amp = if k % 2 == 0 { a } else { a / 4.0 };
                amp * (2.0 * std::f64::consts::PI * 300.0 * t).sin()
            })
            .collect()
    }

    fn rms_db(x: &[f64]) -> f64 {
        10.0 * (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).log10()
    }

    #[test]
    fn default_target_is_loud_and_compressed() {
        let t = Target::default();
        assert_eq!(t.lufs, -10.0);
        assert_eq!(t.true_peak_dbtp, -1.0);
        assert!(t.compressor.ratio > 1.0);
        assert_eq!(ALGORITHM_VERSION, 2);
    }

    #[test]
    fn compression_brings_loud_syllables_closer_whatever_the_level() {
        let c = Compressor::default();
        let syl = SR as usize / 8;
        // The middle of a loud and of a quiet syllable, away from the transitions.
        let spread = |y: &[f64]| {
            rms_db(&y[4 * syl + syl / 4..5 * syl - syl / 4])
                - rms_db(&y[5 * syl + syl / 4..6 * syl - syl / 4])
        };
        let x = two_levels(-6.0);
        let l = measure_f64(&x, SR).unwrap().lufs;
        let (y, db) = c.apply(&x, SR, l);
        assert!((spread(&x) - 12.0).abs() < 0.1, "{}", spread(&x));
        assert!(
            spread(&y) < 10.5,
            "spread {} dB, compression {db}",
            spread(&y)
        );
        assert!(db > 3.0 && db < 12.0, "{db}");
        // The same speech 20 dB lower is compressed the same (threshold from its loudness).
        let q: Vec<f64> = x.iter().map(|v| v / 10.0).collect();
        let (z, dbq) = c.apply(&q, SR, measure_f64(&q, SR).unwrap().lufs);
        assert!((db - dbq).abs() < 1e-6, "{db} {dbq}");
        assert!((spread(&z) - spread(&y)).abs() < 1e-6);
        // Ratio 1: unchanged.
        let off = Compressor {
            ratio: 1.0,
            ..Compressor::default()
        };
        assert_eq!(off.apply(&x, SR, l), (x.clone(), 0.0));
    }

    #[test]
    fn speech_reaches_minus_10_under_the_ceiling_reproducibly() {
        let t = Target::default();
        let x: Vec<i16> = two_levels(-20.0)
            .iter()
            .map(|v| (v * 32768.0).round() as i16)
            .collect();
        let (y, n) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        assert!((n.after.lufs - t.lufs).abs() <= 0.05, "{n:?}");
        assert!(n.after.true_peak_dbtp <= t.ceiling_dbtp(), "{n:?}");
        assert!(n.compressor_db > 3.0, "{n:?}");
        let (y2, n2) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        assert_eq!(y, y2);
        assert_eq!(n, n2);
        // At the target: left as it is.
        let (z, nz) = normalize(&y, SR, &t, t.ceiling_dbtp()).unwrap();
        assert_eq!(z, y);
        assert_eq!(nz.compressor_db, 0.0);
    }

    #[test]
    fn sine_reference_levels() {
        // BS.1770: the K-weighting is +0.69 dB at 1 kHz and the offset -0.691 dB, so a
        // 1 kHz sine of peak -20 dBFS (RMS -23.01 dBFS) reads -23.0 LUFS in one channel.
        let l = measure(&sine(-20.0, 1000.0, 3000), SR).unwrap();
        assert!((l.lufs + 23.0).abs() < 0.1, "{l:?}");
        assert!((l.sample_peak_dbfs + 20.0).abs() < 0.01, "{l:?}");
        assert!((l.true_peak_dbtp + 20.0).abs() < 0.1, "{l:?}");
        // Shorter than one gating block: measured over the clip.
        let s = measure(&sine(-20.0, 1000.0, 300), SR).unwrap();
        assert!((s.lufs + 23.0).abs() < 0.2, "{s:?}");
        // Inter-sample peak: a sine at fs/4 sampled at 45 degrees reads 3 dB under its peak.
        let a = 32768.0 * 10f64.powf(-6.0 / 20.0);
        let isp: Vec<i16> = (0..SR as usize)
            .map(|i| {
                (a * (std::f64::consts::FRAC_PI_2 * i as f64 + std::f64::consts::FRAC_PI_4).sin())
                    .round() as i16
            })
            .collect();
        let l = measure(&isp, SR).unwrap();
        assert!((l.sample_peak_dbfs + 9.01).abs() < 0.05, "{l:?}");
        assert!((l.true_peak_dbtp + 6.0).abs() < 0.3, "{l:?}");
    }

    #[test]
    fn quiet_clip_reaches_the_target_under_the_ceiling() {
        let t = limiter_only();
        let x = speechy(-12.0, 0.45);
        let (y, n) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        assert!(n.before.lufs < -25.0, "{n:?}");
        assert!((n.after.lufs - t.lufs).abs() <= 0.05, "{n:?}");
        assert!(n.after.true_peak_dbtp <= t.ceiling_dbtp(), "{n:?}");
        assert!(
            n.limiter_db > 1.0 && n.limiter_db < t.max_limiting_db && !n.capped,
            "the bursts are limited: {n:?}"
        );
        assert_eq!(y.len(), x.len());
        let again = measure(&y, SR).unwrap();
        assert_eq!(again, n.after);
    }

    #[test]
    fn limiting_is_bounded() {
        // Bursts 14 dB above the syllables: reaching -14 LUFS would take them down by more
        // than the bound; the clip keeps the bound, under the target.
        let t = limiter_only();
        let x = speechy(-12.0, 1.0);
        let (_, n) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        assert!(n.capped, "{n:?}");
        assert!(n.after.lufs < t.lufs - t.tolerance_lu, "{n:?}");
        assert!(n.after.lufs > n.before.lufs + 10.0, "{n:?}");
        assert!(
            (n.limiter_db - t.max_limiting_db).abs() < 0.5,
            "limited up to the bound: {n:?}"
        );
        assert!(n.after.true_peak_dbtp <= t.ceiling_dbtp(), "{n:?}");
        assert!(t.accepts_level(&n.after, t.ceiling_dbtp(), true));
        assert!(!t.accepts_level(&n.after, t.ceiling_dbtp(), false));
    }

    #[test]
    fn low_crest_clip_is_only_amplified() {
        let t = limiter_only();
        let x = sine(-30.0, 440.0, 1500);
        let (y, n) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        assert_eq!(n.limiter_db, 0.0, "{n:?}");
        assert!((n.after.lufs - t.lufs).abs() <= 0.05, "{n:?}");
        // Pure gain: the ratio of two samples is kept.
        let g = f64::from(y[100]) / f64::from(x[100]);
        assert!((20.0 * g.log10() - n.gain_db).abs() < 0.05, "{g} {n:?}");
    }

    #[test]
    fn loud_clip_is_brought_down() {
        let t = limiter_only();
        let x = sine(-1.0, 440.0, 1500);
        let (_, n) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        assert!(n.gain_db < -9.0, "{n:?}");
        assert!((n.after.lufs - t.lufs).abs() <= 0.05, "{n:?}");
    }

    #[test]
    fn reproducible_and_idempotent() {
        let t = limiter_only();
        let x = speechy(-20.0, 0.45);
        let (a, na) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        let (b, nb) = normalize(&x, SR, &t, t.ceiling_dbtp()).unwrap();
        assert_eq!(a, b, "same input, same bytes");
        assert_eq!(na, nb);
        // A clip at the target is left as it is, bit for bit.
        let (c, nc) = normalize(&a, SR, &t, t.ceiling_dbtp()).unwrap();
        assert_eq!(c, a);
        assert_eq!(nc.gain_db, 0.0);
    }

    #[test]
    fn silence_is_an_error() {
        let t = limiter_only();
        assert!(normalize(&[0; 24_000], SR, &t, t.ceiling_dbtp()).is_err());
    }

    #[test]
    fn running_min_matches_the_naive_one() {
        let v: Vec<f64> = (0..500).map(|i| ((i * 7919) % 101) as f64).collect();
        for r in [1, 3, 17, 600] {
            let fast = running_min(&v, r);
            for (i, &f) in fast.iter().enumerate() {
                let lo = i.saturating_sub(r);
                let hi = (i + r).min(v.len() - 1);
                let slow = v[lo..=hi].iter().copied().fold(f64::INFINITY, f64::min);
                assert_eq!(f, slow, "r {r} i {i}");
            }
        }
    }

    #[test]
    fn envelope_never_exceeds_the_gain_asked() {
        let peaks: Vec<f64> = (0..5000)
            .map(|i| {
                if i % 997 == 0 {
                    0.9
                } else {
                    0.1 + 0.05 * ((i % 13) as f64 / 13.0)
                }
            })
            .collect();
        let g = 4.0;
        let c = 0.84;
        let env = envelope(&peaks, g, c, 120, 0.001);
        for (i, (&p, &e)) in peaks.iter().zip(&env).enumerate() {
            assert!(g * p * e <= c + 1e-12, "{i}: {}", g * p * e);
            assert!(e <= 1.0 && e > 0.0);
        }
        // Soft: no step larger than one ramp increment.
        let max_step = env
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max);
        assert!(max_step < 0.01, "{max_step}");
    }
}
