//! Sensible responses of the smaller effects: each does what its controls say.

mod common;
use common::*;

fn sine(f: f32, a: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| a * (std::f32::consts::TAU * f * i as f32 / RATE).sin()).collect()
}

fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
}

fn through(name: &str, set: &[(u32, f32)], x: &[f32]) -> Vec<f32> {
    let mut fx = lg_fx::by_logic_name(name, RATE).unwrap();
    fx.set_tempo(120.0);
    for &(id, v) in set {
        fx.set(id, v);
    }
    run(fx.as_mut(), x).0
}

#[test]
fn autofilter_cuts_above_its_cutoff() {
    // cutoff 50 % = about 630 Hz, no LFO: 5 kHz is cut hard, 100 Hz passes
    let set = [(2, 50.0), (12, 0.0), (3, 0.0), (5, 3.0)];
    let hi = through("AutoFilter", &set, &sine(5000.0, 0.3, 24000));
    let lo = through("AutoFilter", &set, &sine(100.0, 0.3, 24000));
    assert!(rms(&hi[12000..]) < 0.3 / 2f32.sqrt() * 0.01, "{}", rms(&hi[12000..]));
    assert!(rms(&lo[12000..]) > 0.3 / 2f32.sqrt() * 0.8);
}

#[test]
fn distortion_adds_harmonics_and_keeps_level() {
    let x = sine(220.0, 0.3, 48000);
    for name in ["Distortion", "Overdrive", "Amp Designer"] {
        let y = through(name, &[(1, 20000.0)], &x);
        let g = lg_dsp::gain_to_db(rms(&y[24000..]) / rms(&x[24000..]));
        assert!(g.abs() < 8.0, "{name} level {g}");
        // residual after removing the fundamental's best fit
        let (mut c, mut s) = (0.0f32, 0.0f32);
        for (i, v) in y[24000..].iter().enumerate() {
            let ph = std::f32::consts::TAU * 220.0 * (i + 24000) as f32 / RATE;
            c += v * ph.cos();
            s += v * ph.sin();
        }
        let n = 24000.0;
        let fund = ((c * 2.0 / n).powi(2) + (s * 2.0 / n).powi(2)).sqrt() / 2f32.sqrt();
        let thd = (rms(&y[24000..]).powi(2) - fund.powi(2)).max(0.0).sqrt() / fund;
        assert!(thd > 0.03, "{name} THD {thd}");
    }
}

#[test]
fn bitcrusher_quantises() {
    let y = through("Bitcrusher", &[(3, 2.0), (0, 0.0)], &sine(100.0, 0.9, 4800));
    let mut levels: Vec<i32> = y.iter().map(|v| (v * 1000.0).round() as i32).collect();
    levels.sort();
    levels.dedup();
    assert!(levels.len() <= 5, "{levels:?}");
}

#[test]
fn chorus_and_flanger_change_the_sound_but_not_its_level_much() {
    let mut s: u32 = 3;
    let x: Vec<f32> = (0..48000)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f32 / u32::MAX as f32 - 0.5) * 0.5
        })
        .collect();
    for name in ["Chorus", "Ensemble", "Flanger", "Phaser", "Microphaser"] {
        let y = through(name, &[], &x);
        let diff = rms(&y.iter().zip(x.iter()).map(|(a, b)| a - b).collect::<Vec<_>>());
        let g = lg_dsp::gain_to_db(rms(&y[4800..]) / rms(&x[4800..]));
        assert!(diff > 0.01, "{name} does nothing");
        assert!(g.abs() < 6.0, "{name} level {g}");
    }
}

#[test]
fn exciter_adds_only_highs() {
    let low = sine(200.0, 0.5, 24000);
    let y = through("Exciter", &[(2, 100.0), (3, 0.0)], &low);
    assert!(rms(&y[12000..]) < 0.01, "a 200 Hz tone has nothing above 2 kHz to excite");
    let high = sine(3000.0, 0.5, 24000);
    let y = through("Exciter", &[(2, 100.0), (3, 0.0)], &high);
    assert!(rms(&y[12000..]) > 0.01);
}

#[test]
fn enveloper_boosts_attacks() {
    // a repeated burst: with Attack Gain +100 % its onsets come out louder
    let mut x = vec![0.0f32; 48000];
    for b in 0..4 {
        for i in 0..4000 {
            x[b * 12000 + i] = 0.3 * (i as f32 * 0.2).sin();
        }
    }
    let y = through("Enveloper", &[(1, 100.0), (5, 0.0)], &x);
    let onset = rms(&y[12000..12300]);
    let body = rms(&y[14000..15000]);
    assert!(onset > body * 1.5, "{onset} vs {body}");
}

#[test]
fn noise_gate_ducks_to_reduction() {
    let y = through("Noise Gate", &[(0, -20.0), (2, -20.0)], &sine(440.0, 0.01, 24000));
    let g = lg_dsp::gain_to_db(rms(&y[12000..]) / (0.01 / 2f32.sqrt()));
    assert!((g + 20.0).abs() < 0.5, "{g}");
}

/// At default settings every effect keeps a -20 dBFS guitar-range tone within a sensible level
/// window (amps and saturators colour it; nothing should jump or vanish).
#[test]
fn default_levels_are_sensible() {
    let x = sine(220.0, 0.1, 96000);
    for (logic, ours) in lg_fx::list() {
        let y = through(logic, &[], &x);
        let g = lg_dsp::gain_to_db(rms(&y[48000..]) / rms(&x[48000..]));
        eprintln!("{ours:>20}: {g:+6.1} dB");
        assert!(g > -16.0 && g < 14.0, "{ours} {g}");
    }
}
