//! Helpers of the synthesizer tests: rendering, levels, pitch and band energy.
#![allow(dead_code)]

use lg_dsp::Instrument;

pub const RATE: f32 = 48000.0;

pub fn render(inst: &mut dyn Instrument, secs: f32) -> Vec<f32> {
    render_st(inst, secs).0
}

pub fn render_st(inst: &mut dyn Instrument, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * RATE) as usize;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        inst.render(cl, cr);
    }
    (l, r)
}

/// A note held for `secs`, with the given parameters set first.
pub fn play(inst: &mut dyn Instrument, set: &[(u32, f32)], note: u8, vel: u8, secs: f32) -> Vec<f32> {
    for &(id, v) in set {
        inst.set(id, v);
    }
    inst.note_on(note, vel);
    render(inst, secs)
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

pub fn finite(x: &[f32]) -> bool {
    x.iter().all(|v| v.is_finite() && v.abs() < 20.0)
}

/// The level of one frequency (Goertzel), as an amplitude.
pub fn tone(x: &[f32], hz: f32) -> f32 {
    let w = std::f32::consts::TAU * hz / RATE;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = v as f64 + c as f64 * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let p = s1 * s1 + s2 * s2 - c as f64 * s1 * s2;
    (p.max(0.0).sqrt() * 2.0 / x.len() as f64) as f32
}

/// Energy above `hz` over the whole energy (0…1): how bright the sound is.
pub fn bright(x: &[f32], hz: f32) -> f32 {
    // one-pole high-pass, run twice
    let a = (-std::f32::consts::TAU * hz / RATE).exp();
    let mut lp1 = 0.0;
    let mut lp2 = 0.0;
    let mut hi = 0.0f64;
    let mut all = 0.0f64;
    for &v in x {
        lp1 = v + (lp1 - v) * a;
        let h1 = v - lp1;
        lp2 = h1 + (lp2 - h1) * a;
        let h = h1 - lp2;
        hi += (h * h) as f64;
        all += (v * v) as f64;
    }
    (hi / all.max(1e-20)) as f32
}

/// Fundamental by normalised autocorrelation between `lo` and `hi` Hz.
pub fn fundamental(x: &[f32], lo: f32, hi: f32) -> f32 {
    let min_lag = (RATE / hi) as usize;
    let max_lag = (RATE / lo) as usize;
    let n = x.len() - max_lag - 2;
    let mut corr = vec![0.0f32; max_lag + 2];
    for lag in min_lag.max(1) - 1..=max_lag + 1 {
        let (mut s, mut e1, mut e2) = (0.0, 0.0, 0.0);
        for i in 0..n {
            s += x[i] * x[i + lag];
            e1 += x[i] * x[i];
            e2 += x[i + lag] * x[i + lag];
        }
        corr[lag] = s / (e1 * e2).sqrt().max(1e-12);
    }
    let mut best = (min_lag, f32::MIN);
    for lag in min_lag..=max_lag {
        if corr[lag] > best.1 {
            best = (lag, corr[lag]);
        }
    }
    let k = best.0;
    let (a, b, c) = (corr[k - 1], corr[k], corr[k + 1]);
    let d = 0.5 * (a - c) / (a - 2.0 * b + c);
    RATE / (k as f32 + if d.is_finite() { d } else { 0.0 })
}

/// How far apart two sounds are (RMS of the difference over the RMS of the first).
pub fn differs(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let d: Vec<f32> = (0..n).map(|i| a[i] - b[i]).collect();
    rms(&d) / rms(&a[..n]).max(1e-9)
}

/// Every parameter at its minimum, maximum and default, with notes: nothing may blow up.
pub fn sweep_all(make: &dyn Fn() -> Box<dyn Instrument>) {
    let ids: Vec<(u32, f32, f32)> = make().params().iter().map(|p| (p.id, p.min, p.max)).collect();
    for (id, min, max) in ids {
        for v in [min, max] {
            let mut i = make();
            i.set(id, v);
            i.note_on(48, 110);
            i.note_on(67, 60);
            let a = render(i.as_mut(), 0.15);
            i.note_off(48);
            let b = render(i.as_mut(), 0.1);
            assert!(finite(&a) && finite(&b), "parameter {id} = {v} blows up");
        }
    }
}
