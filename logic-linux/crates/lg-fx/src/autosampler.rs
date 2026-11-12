//! Auto Sampler (Utility ▸ Auto Sampler): the settings for sampling the instrument it sits after
//! (key range, every how many semitones, round robins, sustain, velocity layers and their
//! response curve, auto loop, one shot, input gain) and the level it hears, with clip detection.
//! The sampling itself (playing the notes, recording, trimming, looping, writing the instrument)
//! is done by the app, which drives the track's instrument; this effect passes audio through.
//!
//! Also the loop searches the app uses: `find_loop`, `crossfade_loop`, `reverse_xfade_loop`,
//! `penrose_loop` and `bidirectional_loop` work on one channel of samples.

use crate::util::{Values, plugin_impl};
use lg_dsp::{Effect, Param, Unit};

pub const RANGE_START: u32 = 0;
pub const RANGE_END: u32 = 1;
pub const EVERY: u32 = 2;
pub const ROUND_ROBIN: u32 = 3;
pub const SUSTAIN: u32 = 4;
pub const LAYERS: u32 = 5;
pub const RESPONSE: u32 = 6;
pub const AUTO_LOOP: u32 = 7;
pub const LOOP_START: u32 = 8;
pub const LOOP_END: u32 = 9;
pub const ONE_SHOT: u32 = 10;
pub const INPUT_GAIN: u32 = 11;
/// Custom layer velocities (Velocity editor), one per layer.
pub const CUSTOM: u32 = 20;
pub const MAX_LAYERS: usize = 16;

pub const RESPONSES: &[&str] = &["Linear", "Exp1", "Exp2", "Exp3", "Log1", "Log2", "Log3", "Custom"];
pub const LOOPS: &[&str] = &["None", "Search", "Search with XFade", "Search with Rev XFade", "Penrose Machine", "Bidirectional"];

fn params() -> &'static [Param] {
    static P: std::sync::OnceLock<Vec<Param>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let mut v = vec![
            Param::new(RANGE_START, "Range Start", 0.0, 127.0, 36.0, Unit::None),
            Param::new(RANGE_END, "Range End", 0.0, 127.0, 96.0, Unit::None),
            Param::new(EVERY, "Sample Every", 1.0, 12.0, 3.0, Unit::Semitones),
            Param::menu(ROUND_ROBIN, "Round Robin", &["No", "2", "3", "4"], 0.0),
            Param::new(SUSTAIN, "Sustain", 0.1, 30.0, 3.0, Unit::Seconds),
            Param::new(LAYERS, "Velocity Layers", 1.0, MAX_LAYERS as f32, 1.0, Unit::None),
            Param::menu(RESPONSE, "Velocity Response", RESPONSES, 0.0),
            Param::menu(AUTO_LOOP, "Auto Loop", LOOPS, 0.0),
            Param::new(LOOP_START, "Auto Loop Start", 0.0, 100.0, 40.0, Unit::Percent),
            Param::new(LOOP_END, "Auto Loop End", 0.0, 100.0, 90.0, Unit::Percent),
            Param::new(ONE_SHOT, "One Shot", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(INPUT_GAIN, "Input Gain", -24.0, 24.0, 0.0, Unit::Db),
        ];
        for k in 0..MAX_LAYERS {
            v.push(Param::new(CUSTOM + k as u32, Box::leak(format!("Layer {} Velocity", k + 1).into_boxed_str()), 1.0, 127.0, 127.0, Unit::None).hide());
        }
        v
    })
}

/// The top velocity of each of `n` layers for a response curve (Exp: more layers near the top,
/// Log: more near the bottom).
pub fn layer_velocities(n: usize, response: usize, custom: &[f32]) -> Vec<u8> {
    let n = n.clamp(1, MAX_LAYERS);
    (1..=n)
        .map(|k| {
            if response == 7 {
                return custom.get(k - 1).copied().unwrap_or(127.0).clamp(1.0, 127.0).round() as u8;
            }
            let x = k as f32 / n as f32;
            let y = match response {
                1..=3 => 1.0 - (1.0 - x).powf(1.0 + 0.6 * response as f32),
                4..=6 => x.powf(1.0 + 0.6 * (response - 3) as f32),
                _ => x,
            };
            (y * 127.0).round().clamp(1.0, 127.0) as u8
        })
        .collect()
}

/// The notes sampled: from Range Start to Range End every so many semitones (the end note
/// always included).
pub fn sample_notes(start: u8, end: u8, every: u8) -> Vec<u8> {
    let (a, b) = (start.min(end), start.max(end));
    let mut v: Vec<u8> = (a..=b).step_by(every.max(1) as usize).collect();
    if v.last() != Some(&b) {
        v.push(b);
    }
    v
}

pub struct AutoSampler {
    p: Values,
    rate: f32,
    level: [f32; 2],
    clip: f32,
}

impl AutoSampler {
    pub fn new(rate: f32) -> AutoSampler {
        AutoSampler { p: Values::new(params()), rate, level: [0.0; 2], clip: 0.0 }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.level = [0.0; 2];
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for AutoSampler {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let g = lg_dsp::db_to_gain(self.p.get(INPUT_GAIN));
        let fall = lg_dsp::db_to_gain(-20.0 / self.rate);
        for (l, r) in left.iter().zip(right.iter()) {
            for (m, x) in self.level.iter_mut().zip([*l * g, *r * g]) {
                *m = (*m * fall).max(x.abs());
                if x.abs() >= 1.0 {
                    self.clip = 1.0;
                }
            }
        }
        self.clip = (self.clip - left.len() as f32 / self.rate).max(0.0);
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Level L", lg_dsp::gain_to_db(self.level[0])), ("Level R", lg_dsp::gain_to_db(self.level[1])), ("Clip", self.clip)]
    }
}

plugin_impl!(AutoSampler);

// ---- loops --------------------------------------------------------------------------------------

/// The best loop between `lo` and `hi` (sample indexes): the end that best continues into the
/// start (matching a short window around both points), at least a tenth of the range long.
pub fn find_loop(x: &[f32], lo: usize, hi: usize) -> Option<(usize, usize)> {
    let hi = hi.min(x.len().saturating_sub(65));
    if hi <= lo + 256 {
        return None;
    }
    let w = 32usize;
    let min_len = ((hi - lo) / 10).max(128);
    let start = lo.max(w);
    // candidate starts: rising zero crossings near the search start
    let zc = |i: usize| x[i - 1] <= 0.0 && x[i] > 0.0;
    let starts: Vec<usize> = (start..(start + (hi - lo) / 4).min(hi)).filter(|i| zc(*i)).take(24).collect();
    let mut best: Option<(f32, usize, usize)> = None;
    for &s in starts.iter() {
        let mut e = hi;
        let mut tries = 0;
        while e > s + min_len && tries < 400 {
            if zc(e) {
                tries += 1;
                let err: f32 = (0..w).map(|k| (x[e - w / 2 + k] - x[s - w / 2 + k]).powi(2)).sum();
                if best.is_none_or(|b| err < b.0) {
                    best = Some((err, s, e));
                }
            }
            e -= 1;
        }
    }
    best.map(|b| (b.1, b.2))
}

/// Bakes a crossfade into the samples before the loop end so it joins the loop start smoothly.
pub fn crossfade_loop(x: &mut [f32], ls: usize, le: usize, len: usize) {
    let len = len.min(ls).min(le - ls);
    for k in 0..len {
        let t = (k + 1) as f32 / len as f32;
        let i = le - len + k;
        let j = ls - len + k;
        x[i] = x[i] * (1.0 - t).sqrt() + x[j] * t.sqrt();
    }
}

/// Search with Rev XFade: the loop mixed with its reversed copy, then crossfaded.
pub fn reverse_xfade_loop(x: &mut [f32], ls: usize, le: usize, len: usize) {
    let seg: Vec<f32> = x[ls..le].to_vec();
    let n = seg.len();
    for k in 0..n {
        x[ls + k] = 0.5 * (seg[k] + seg[n - 1 - k]);
    }
    crossfade_loop(x, ls, le, len);
}

/// Penrose Machine: an artificial loop made from a snapshot of the sound at `at` — its spectrum
/// with the phases scrambled, resynthesized as one seamless period — appended after `at` with
/// a crossfade into it. Returns the new samples and the loop (start, end).
pub fn penrose_loop(x: &[f32], at: usize, seed: u32) -> (Vec<f32>, usize, usize) {
    use crate::convolve::{C, Fft};
    let n = 1 << 15;
    let at = at.min(x.len().saturating_sub(4096));
    let snap = 4096.min(x.len() - at);
    let fft = Fft::new(n);
    let mut buf: Vec<C> = (0..n).map(|i| {
        let v = if i < snap { x[at + i] * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / snap as f32).cos()) } else { 0.0 };
        C { re: v, im: 0.0 }
    }).collect();
    fft.run(&mut buf, false);
    let mut s = seed | 1;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s as f32 / u32::MAX as f32 * std::f32::consts::TAU
    };
    for k in 1..n / 2 {
        let mag = (buf[k].re * buf[k].re + buf[k].im * buf[k].im).sqrt();
        let ph = rnd();
        buf[k] = C { re: mag * ph.cos(), im: mag * ph.sin() };
        buf[n - k] = C { re: buf[k].re, im: -buf[k].im };
    }
    buf[0] = C { re: 0.0, im: 0.0 };
    buf[n / 2] = C { re: 0.0, im: 0.0 };
    fft.run(&mut buf, true);
    let loop_sig: Vec<f32> = buf.iter().map(|c| c.re).collect();
    // match the snapshot's level
    let rms = |v: &[f32]| (v.iter().map(|a| a * a).sum::<f32>() / v.len().max(1) as f32).sqrt();
    let g = rms(&x[at..at + snap]) / rms(&loop_sig).max(1e-9);
    let fade = 2048.min(at);
    let mut out = x[..at].to_vec();
    for k in 0..fade {
        let t = k as f32 / fade as f32;
        let i = out.len() - fade + k;
        out[i] = out[i] * (1.0 - t) + loop_sig[k] * g * t;
    }
    let ls = out.len();
    out.extend(loop_sig[fade..].iter().map(|v| v * g));
    out.extend(loop_sig[..fade].iter().map(|v| v * g));
    let le = out.len();
    (out, ls, le)
}

/// Bidirectional: the loop followed by its reversed copy (a loop that plays forward and back),
/// crossfaded back into the loop start. Returns the new samples and the loop.
pub fn bidirectional_loop(x: &[f32], ls: usize, le: usize, len: usize) -> (Vec<f32>, usize, usize) {
    let mut out = x[..le].to_vec();
    let rev: Vec<f32> = x[ls..le].iter().rev().copied().collect();
    out.extend(rev);
    let le2 = out.len();
    crossfade_loop(&mut out, ls, le2, len);
    (out, ls, le2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_and_curves() {
        assert_eq!(layer_velocities(1, 0, &[]), vec![127]);
        assert_eq!(layer_velocities(4, 0, &[]), vec![32, 64, 95, 127]);
        let exp = layer_velocities(4, 2, &[]);
        let log = layer_velocities(4, 5, &[]);
        // exp: layers bunch up near the top; log: near the bottom
        assert!(exp[0] > 32 && log[0] < 32);
        assert_eq!(layer_velocities(2, 7, &[50.0, 110.0]), vec![50, 110]);
    }

    #[test]
    fn note_list() {
        assert_eq!(sample_notes(36, 48, 3), vec![36, 39, 42, 45, 48]);
        assert_eq!(sample_notes(36, 47, 5), vec![36, 41, 46, 47]);
    }

    #[test]
    fn loop_search_finds_whole_periods() {
        // a 100 Hz sine at 48 kHz: the loop should be whole periods (480 samples each)
        let x: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * 100.0 * i as f32 / 48000.0).sin()).collect();
        let (s, e) = find_loop(&x, 10000, 40000).unwrap();
        let len = (e - s) as f32 / 480.0;
        assert!((len - len.round()).abs() < 0.02, "{len}");
        let (y, ls, le) = penrose_loop(&x, 20000, 7);
        assert!(le > ls && ls <= 20000 && y.len() == le);
        let (z, a, b) = bidirectional_loop(&x, s, e, 64);
        assert_eq!(b - a, 2 * (e - s));
        assert_eq!(z.len(), b);
    }
}
