//! Alchemy's effects racks: every effect runs; reverbs ring on, the delay repeats, Band Reject
//! restores what Bandpass took, Amp / Panner / compressors / distortion do their job; sources
//! and main filters reach racks A–D.

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::Alchemy;
use lg_synth::alchemy_fx::*;

const RATE: f32 = 48000.0;

fn unit(ty: usize) -> (Unit3, [f32; KNOBS]) {
    let mut u = Unit3::new(RATE);
    u.configure(ty);
    let mut v = [0.5; KNOBS];
    for (i, s) in knobs(ty).iter().enumerate() {
        v[i] = unscale(s, s.def);
    }
    if ty == 3 {
        u.load_ir(&v, &[]);
    }
    (u, v)
}

fn run(u: &mut Unit3, v: &[f32; KNOBS], x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    let mut rej = ([0.0; 64], [0.0; 64], false);
    for c in (0..x.len()).step_by(64) {
        let e = (c + 64).min(x.len());
        u.process(v, 120.0, &mut l[c..e], &mut r[c..e], &mut rej);
    }
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn impulse(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn sine(hz: f32, n: usize, a: f32) -> Vec<f32> {
    (0..n).map(|i| a * (i as f32 * std::f32::consts::TAU * hz / RATE).sin()).collect()
}

#[test]
fn every_effect_runs() {
    let x = sine(220.0, 9600, 0.5);
    for ty in 1..TYPES.len() {
        let (mut u, v) = unit(ty);
        let (l, r) = run(&mut u, &v, &x);
        assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 20.0), "{}", TYPES[ty]);
        for (i, s) in knobs(ty).iter().enumerate() {
            assert!(!text(s, v[i]).is_empty(), "{} {}", TYPES[ty], s.name);
        }
        assert!(!presets(ty).is_empty());
    }
}

#[test]
fn reverbs_ring_and_delay_repeats() {
    for ty in [1, 2, 3] {
        let (mut u, mut v) = unit(ty);
        if ty != 3 {
            let mix = knobs(ty).iter().position(|s| s.name == "Mix").unwrap();
            v[mix] = 1.0;
        }
        let (l, _) = run(&mut u, &v, &impulse(48000));
        assert!(rms(&l[24000..36000]) > 1e-5, "{} has a tail", TYPES[ty]);
    }
    let (mut u, mut v) = unit(4);
    v[0] = 0.0; // free time
    v[1] = unscale(&knobs(4)[1], 100.0);
    v[18] = 1.0;
    let (l, _) = run(&mut u, &v, &impulse(24000));
    let at = (0.1 * RATE) as usize;
    assert!(l[at - 2..at + 3].iter().any(|x| x.abs() > 0.3), "an echo at 100 ms");
}

#[test]
fn utility_and_dynamics() {
    let x = sine(220.0, 9600, 0.5);
    let (mut u, mut v) = unit(7);
    v[0] = 0.0;
    assert!(rms(&run(&mut u, &v, &x).0) < 1e-6, "Amp 0 % silences");
    let (mut u, mut v) = unit(8);
    v[0] = 0.0;
    let (l, r) = run(&mut u, &v, &x);
    assert!(rms(&r) < rms(&l) * 0.1, "Panner left");
    for ty in [9, 10] {
        let (mut u, v) = unit(ty);
        let loud = sine(220.0, 24000, 0.9);
        let (l, _) = run(&mut u, &v, &loud);
        let quiet = sine(220.0, 24000, 0.05);
        let (q, _) = run(&mut unit(ty).0, &v, &quiet);
        assert!(rms(&l[12000..]) / rms(&loud[12000..]) < rms(&q[12000..]) / rms(&quiet[12000..]), "{} compresses the loud one more", TYPES[ty]);
    }
}

#[test]
fn bandpass_then_reject_restores() {
    // a rack: Bandpass then Band Reject at 100 % gives back the input
    let mut a = Alchemy::new(RATE);
    for (f, v) in [(0, 11.0), (2, unscale(&knobs(11)[0], 500.0)), (3, unscale(&knobs(11)[1], 1000.0))] {
        a.set(id(0, 0, f), v);
    }
    a.set(id(0, 1, 0), 12.0);
    a.set(id(0, 1, 2), 1.0);
    assert!(a.racks.used[0]);
    let x = sine(4000.0, 4800, 0.5);
    let (mut l, mut r) = (x.clone(), x.clone());
    a.racks.process(0, 120.0, &mut l[..64], &mut r[..64]);
    assert!(l[..64].iter().zip(&x).all(|(a, b)| (a - b).abs() < 1e-4));
    // without the reject, the band pass takes the 4 kHz tone out
    a.set(id(0, 1, 0), 0.0);
    let mut l2 = sine(4000.0, 4800, 0.5);
    let mut r2 = l2.clone();
    for c in (0..4800).step_by(64) {
        a.racks.process(0, 120.0, &mut l2[c..c + 64], &mut r2[c..c + 64]);
    }
    assert!(rms(&l2[2400..]) < 0.2);
}

#[test]
fn distortion_and_waveshaper() {
    let x = sine(110.0, 9600, 0.5);
    let (mut u, mut v) = unit(16);
    v[1] = 1.0;
    let (l, _) = run(&mut u, &v, &x);
    assert!(l.iter().zip(&x).any(|(a, b)| (a - b).abs() > 0.1), "Mech bends the wave");
    let (mut u, mut v) = unit(17);
    // a flat curve at the middle: everything maps to 0
    for (k, val) in [(6, 0.5), (8, 0.5), (9, 0.5)] {
        v[k] = val;
    }
    let (l, _) = run(&mut u, &v, &x);
    assert!(rms(&l) < 1e-3, "the curve decides the output");
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
fn sources_and_filters_reach_racks() {
    let level = |setup: &dyn Fn(&mut Alchemy)| {
        let mut a = Alchemy::new(RATE);
        a.set(5, 0.0);
        a.set(278, 0.0);
        setup(&mut a);
        a.note_on(60, 110);
        rms(&render(&mut a, 0.4)[4800..])
    };
    let open = level(&|_| {});
    // source A sent fully to rack A (F1/FX A, knob right), rack A an Amp at 0 %
    let muted = level(&|a| {
        a.set(SEND, 1.0);
        a.set(lg_synth::alchemy::ROUTE_F2, 1.0);
        a.set(id(1, 0, 0), 7.0);
        a.set(id(1, 0, 2), 0.0);
    });
    assert!(muted < open * 0.01, "rack A's Amp mutes source A: {open} {muted}");
    // filter 1's FX share to rack B with an Amp at 0 %
    let f = level(&|a| {
        a.set(SEND + 4, 2.0);
        a.set(id(2, 0, 0), 7.0);
        a.set(id(2, 0, 2), 0.0);
    });
    assert!(f < open * 0.01, "{f}");
    // the Main rack's Amp at half
    let h = level(&|a| {
        a.set(id(0, 0, 0), 7.0);
        a.set(id(0, 0, 2), 0.5);
    });
    assert!((h / open - 0.5).abs() < 0.1, "{h} {open}");
}
