//! Alchemy's filters: every type runs, the families shape the spectrum as they should, the
//! special types (comb, ring modulator, distortions, compressor, FM) do their own thing, and the
//! source filters / main filters route as Logic's (series / parallel, FX amount).

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::{Alchemy, SRC_FILT};
use lg_synth::alchemy_filter::*;

const RATE: f32 = 48000.0;

fn noise(n: usize) -> Vec<f32> {
    let mut s = 1u32;
    (0..n).map(|_| {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 8) as f32 / 8388608.0 - 1.0
    }).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Energy below / above 1 kHz (one-pole split).
fn split(x: &[f32]) -> (f32, f32) {
    let k = 1.0 - (-std::f32::consts::TAU * 1000.0 / RATE).exp();
    let mut lp = 0.0;
    let (mut lo, mut hi) = (0.0, 0.0);
    for &v in x {
        lp += (v - lp) * k;
        lo += lp * lp;
        hi += (v - lp) * (v - lp);
    }
    (lo, hi)
}

fn run(ty: usize, cut: f32, b: f32, c: f32, x: &[f32]) -> Vec<f32> {
    let mut f = AFilt::default();
    let mut out = Vec::with_capacity(x.len());
    for blk in x.chunks(64) {
        f.set(ty, cutoff_hz(cut), b, c, RATE);
        out.extend(blk.iter().map(|&v| f.run(v)));
    }
    out
}

#[test]
fn every_type_runs_and_is_finite() {
    assert_eq!(FILTER_TYPES.len(), 39);
    let x = noise(8000);
    for t in 0..FILTER_TYPES.len() {
        for (cut, b, c) in [(0.2, 0.0, 0.0), (0.5, 0.9, 0.9), (0.95, 0.5, 0.2)] {
            let y = run(t, cut, b, c, &x);
            assert!(y.iter().all(|v| v.is_finite() && v.abs() < 50.0), "{} blew up", FILTER_TYPES[t]);
        }
        assert_eq!(knob_names(t).len(), 3);
    }
    assert!(!has_drive(3) && has_drive(1), "Clean filters have no Drive");
    assert_eq!(knob_names(27), ["Note", "Feedback", "Damp"]);
}

fn sine(hz: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 * (i as f32 * std::f32::consts::TAU * hz / RATE).sin()).collect()
}

/// Gain of a filter type at a frequency (cutoff knob at about 1 kHz).
fn gain(t: usize, hz: f32) -> f32 {
    let x = sine(hz, 12000);
    let y = run(t, 0.6, 0.1, 0.0, &x);
    rms(&y[6000..]) / rms(&x[6000..])
}

#[test]
fn families_shape_the_spectrum() {
    for (t, n) in FILTER_TYPES.iter().enumerate() {
        let (lo, hi) = (gain(t, 100.0), gain(t, 10000.0));
        if n.starts_with("LP") {
            assert!(hi < lo * 0.3, "{n} keeps the highs: {lo} {hi}");
        } else if n.starts_with("HP") {
            assert!(lo < hi * 0.3, "{n} keeps the lows: {lo} {hi}");
        } else if n.starts_with("BP") {
            let mid = gain(t, cutoff_hz(0.6));
            assert!(mid > lo && mid > hi, "{n} passes its band: {lo} {mid} {hi}");
        }
    }
    // steeper slopes cut harder
    let far = |t: usize| gain(t, 8000.0);
    assert!(far(5) < far(4) && far(4) < far(3) && far(3) < far(2), "60 > 24 > 12 > 6 dB");
}

#[test]
fn comb_rings_at_its_note_and_ring_mod_moves_energy() {
    // an impulse into Comb Pos rings with the note's period
    let mut x = vec![0.0f32; 4800];
    x[0] = 1.0;
    let cut = ((440.0f32 / 20.0).ln() / 1000f32.ln()).clamp(0.0, 1.0);
    let y = run(27, cut, 0.9, 0.0, &x);
    let p = (RATE / 440.0).round() as usize;
    assert!(y[p].abs() > 0.3 && y[p / 2].abs() < 0.05, "echo one period later: {} {}", y[p], y[p / 2]);
    let n = run(28, cut, 0.9, 0.0, &x);
    assert!(n[2 * p] > 0.0 && n[p] < 0.0, "Comb Neg flips each echo");
    // ring modulation of a low sine moves its energy up
    let s: Vec<f32> = (0..9600).map(|i| (i as f32 * std::f32::consts::TAU * 100.0 / RATE).sin()).collect();
    let r = run(30, 0.6, 0.0, 0.0, &s);
    let (lo, hi) = split(&r);
    assert!(hi > lo, "the sidebands sit around the carrier");
}

#[test]
fn distortion_compressor_and_fm() {
    let s: Vec<f32> = (0..9600).map(|i| 0.5 * (i as f32 * std::f32::consts::TAU * 110.0 / RATE).sin()).collect();
    for t in [31, 32, 33, 34] {
        let dry = run(t, 0.9, 0.0, 0.0, &s);
        assert!(dry.iter().zip(&s).all(|(a, b)| (a - b).abs() < 1e-4), "{} Mix 0 % is clean", FILTER_TYPES[t]);
        let wet = run(t, 0.9, 1.0, 0.5, &s);
        let (_, hi) = split(&wet);
        let (_, hi0) = split(&s);
        assert!(hi > hi0 * 2.0, "{} adds harmonics", FILTER_TYPES[t]);
    }
    let comp = run(35, 0.3, 1.0, 0.0, &s);
    assert!(rms(&comp[4800..]) < rms(&s[4800..]) * 0.6, "the compressor turns a loud signal down");
    let fm = run(36, 0.5, 0.0, 0.0, &vec![0.0; 4800]);
    assert!(rms(&fm) > 0.3, "FM makes its own sine");
}

fn render(a: &mut Alchemy, secs: f32) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for c in (0..n).step_by(256) {
        let e = (c + 256).min(n);
        a.render(&mut l[c..e], &mut r[c..e]);
    }
    l
}

#[test]
fn source_filters_series_parallel_and_fx_amount() {
    let level = |setup: &dyn Fn(&mut Alchemy)| {
        let mut a = Alchemy::new(RATE);
        a.set(5, 0.0);
        a.set(278, 0.0); // source B off
        setup(&mut a);
        a.note_on(60, 110);
        rms(&render(&mut a, 0.4)[4800..])
    };
    let open = level(&|_| {});
    // filter 1 a shut low pass, filter 2 a high pass: in series almost nothing gets through
    let shut = |a: &mut Alchemy| {
        a.set(51, 0.05);
        a.set(SRC_FILT + 1, 1.0);
        a.set(SRC_FILT + 3, 18.0);
        a.set(59, 0.6);
    };
    let series = level(&|a| shut(a));
    let par = level(&|a| {
        shut(a);
        a.set(SRC_FILT + 8, 1.0);
    });
    assert!(series < open * 0.2 && par > series * 2.0, "open {open} series {series} parallel {par}");
    // the FX amount: at 0 % the sound passes the effects rack (here a muting Bitcrusher mix)
    let fx = |amount: f32| level(&|a| {
        a.set(lg_synth::alchemy_extra::FX, 2.0); // Bitcrusher
        a.set(88, amount);
    });
    let (with, past) = (fx(1.0), fx(0.0));
    assert!((past - open).abs() < open * 0.05 && (with - open).abs() > open * 0.01, "{open} {with} {past}");
}
