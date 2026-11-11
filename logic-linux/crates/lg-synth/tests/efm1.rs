//! EFM1: the controls of the guide do what the guide says.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::efm1::{self as f, Efm1};

fn mk() -> Efm1 {
    let mut s = Efm1::new(RATE);
    // a plain FM pair to start from
    for (id, v) in [(f::FM_DEPTH, 0.0), (f::VE_S, 1.0), (f::VELOCITY, 0.0)] {
        s.set(id, v);
    }
    s
}

#[test]
fn stays_finite_everywhere() {
    sweep_all(&|| Box::new(Efm1::new(RATE)));
}

#[test]
fn ratios_intensity_and_modulator() {
    let h = |x: &[f32], n: f32| tone(&x[4800..], 220.0 * n);
    // no FM: the carrier's sine alone; more FM: brighter
    let x0 = play(&mut mk(), &[(f::FM, 0.0)], 57, 100, 0.5);
    assert!(h(&x0, 2.0) < 0.01 * h(&x0, 1.0));
    let x1 = play(&mut mk(), &[(f::FM, 0.4)], 57, 100, 0.5);
    let x2 = play(&mut mk(), &[(f::FM, 0.9)], 57, 100, 0.5);
    assert!(bright(&x2[4800..], 900.0) > 3.0 * bright(&x1[4800..], 900.0) && bright(&x1[4800..], 900.0) > 10.0 * bright(&x0[4800..], 900.0));
    // 1:1 has every harmonic (sawtooth-like); modulator 2 : carrier 1 only the odd ones (square-like)
    assert!(h(&x1, 2.0) > 0.05 * h(&x1, 1.0));
    let sq = play(&mut mk(), &[(f::FM, 0.5), (f::MOD_HARMONIC, 2.0)], 57, 100, 0.5);
    assert!(h(&sq, 2.0) < 0.02 * h(&sq, 1.0) && h(&sq, 3.0) > 0.05 * h(&sq, 1.0));
    // the carrier's harmonic moves the pitch heard: 3 = a twelfth up
    let c3 = play(&mut mk(), &[(f::FM, 0.0), (f::CAR_HARMONIC, 3.0)], 57, 100, 0.5);
    assert!((fundamental(&c3[4800..], 500.0, 800.0) / 660.0 - 1.0).abs() < 0.005);
    let c32 = play(&mut mk(), &[(f::FM, 0.0), (f::CAR_HARMONIC, 32.0)], 33, 100, 0.5);
    assert!(tone(&c32[4800..], 55.0 * 32.0) > 0.05, "the 32nd harmonic");
    // Fine: half a harmonic either way
    let fine = play(&mut mk(), &[(f::FM, 0.0), (f::CAR_FINE, 0.5)], 57, 100, 0.5);
    assert!((fundamental(&fine[4800..], 250.0, 400.0) / 330.0 - 1.0).abs() < 0.005);
    let mfine = play(&mut mk(), &[(f::FM, 0.5), (f::MOD_FINE, 0.25)], 57, 100, 0.5);
    assert!(differs(&mfine[4800..], &x1[4800..]) > 0.3, "a detuned modulator makes inharmonic sidebands");
    // the modulator's wave: a sine at the far left, richer waves to the right
    let w = |wave: f32| bright(&play(&mut mk(), &[(f::FM, 0.3), (f::MOD_WAVE, wave)], 57, 100, 0.5)[4800..], 2500.0);
    assert!(w(1.0) > 3.0 * w(0.0) && w(0.5) > 1.5 * w(0.0), "{} {} {}", w(0.0), w(0.5), w(1.0));
    // Fixed Carrier: the carrier ignores the key and the bend, the modulator still follows
    let fx = |note: u8, bend: i16| {
        let mut s = mk();
        s.set(f::FIXED, 1.0);
        s.set(f::FM, 0.0);
        s.pitch_bend(bend);
        let x = play(&mut s, &[], note, 100, 0.4);
        fundamental(&x[4800..], 60.0, 500.0)
    };
    assert!((fx(48, 0) / fx(72, 0) - 1.0).abs() < 0.002 && (fx(60, 8191) / fx(60, 0) - 1.0).abs() < 0.002);
}

#[test]
fn modulation_envelope_lfo_and_controllers() {
    // FM Depth: the envelope raises the FM (bright start, darker later); turned left it is inverted
    let run = |depth: f32| play(&mut mk(), &[(f::FM, 0.3), (f::FM_DEPTH, depth), (f::ME_A, 0.0), (f::ME_D, 0.75), (f::ME_S, 0.0)], 57, 100, 2.0);
    let b = |x: &[f32], a: usize, z: usize| bright(&x[a..z], 900.0);
    let (pos, neg, zero) = (run(0.8), run(-0.8), run(0.0));
    assert!(b(&pos, 2400, 7200) > 5.0 * b(&pos, 80000, 96000));
    assert!(b(&neg, 2400, 7200) < 0.3 * b(&neg, 80000, 96000));
    assert!((b(&zero, 2400, 7200) / b(&zero, 80000, 96000) - 1.0).abs() < 0.3, "centred: no effect");
    // attack, sustain and release of the modulation envelope
    let slow = play(&mut mk(), &[(f::FM, 0.0), (f::FM_DEPTH, 0.8), (f::ME_A, 0.75), (f::ME_S, 1.0)], 57, 100, 2.0);
    assert!(b(&slow, 0, 4800) < 0.2 * b(&slow, 80000, 96000), "attack");
    let sus = |s: f32| b(&play(&mut mk(), &[(f::FM, 0.0), (f::FM_DEPTH, 0.8), (f::ME_D, 0.3), (f::ME_S, s)], 57, 100, 1.0), 38400, 48000);
    assert!(sus(1.0) > 5.0 * sus(0.2), "sustain");
    let rel = |r: f32| {
        let mut s = mk();
        let _ = play(&mut s, &[(f::FM, 0.0), (f::FM_DEPTH, 0.8), (f::ME_S, 1.0), (f::ME_R, r), (f::VE_R, 0.8)], 57, 100, 0.5);
        s.note_off(57);
        let y = render(&mut s, 0.6);
        b(&y, 9600, 19200)
    };
    assert!(rel(0.8) > 3.0 * rel(0.2), "release");
    // Modulator Pitch: the envelope bends the modulator's pitch (the tone changes as it falls)
    let mp = play(&mut mk(), &[(f::FM, 0.4), (f::MOD_PITCH, 0.6), (f::ME_D, 0.5), (f::ME_S, 0.0)], 57, 100, 2.0);
    let plain = play(&mut mk(), &[(f::FM, 0.4), (f::MOD_PITCH, 0.0), (f::ME_D, 0.5), (f::ME_S, 0.0)], 57, 100, 2.0);
    assert!(differs(&mp[..9600], &plain[..9600]) > 0.3 && (tone(&mp[86400..], 440.0) / tone(&plain[86400..], 440.0) - 1.0).abs() < 0.1);
    // the LFO knob: right = FM intensity wobbles (pitch steady), left = vibrato; Rate sets its speed
    let wob = |lfo: f32, rate: f32| {
        let x = play(&mut mk(), &[(f::FM, 0.3), (f::LFO, lfo), (f::RATE, rate)], 57, 100, 2.0);
        let steady = tone(&x[4800..], 220.0) / rms(&x[4800..]);
        let br: Vec<f32> = x[4800..].chunks(600).map(|c| bright(c, 900.0)).collect();
        let mean = br.iter().sum::<f32>() / br.len() as f32;
        let ups = br.windows(2).filter(|w| w[0] < mean && w[1] >= mean).count();
        (steady, ups)
    };
    let (vib, fm, off) = (wob(-1.0, 5.0), wob(1.0, 5.0), wob(0.0, 5.0));
    assert!(vib.0 < 0.5 * off.0 && fm.0 > 2.0 * vib.0, "{vib:?} {fm:?} {off:?}");
    assert!(fm.1 >= 8 && fm.1 <= 11 && wob(1.0, 2.0).1 <= 5, "rate: {} {}", fm.1, wob(1.0, 2.0).1);
    // a controller assigned to FM raises the intensity; one assigned to Vibrato adds vibrato
    let mut s = mk();
    s.set(f::CTRL_FM, 4.0);
    s.set(f::CTRL_FM_AMT, 1.0);
    let a = play(&mut s, &[(f::FM, 0.1)], 57, 100, 0.5);
    s.controller(4, 127);
    let c = render(&mut s, 0.5);
    assert!(bright(&c[4800..], 900.0) > 5.0 * bright(&a[4800..], 900.0));
    s.controller(1, 127);
    let d = render(&mut s, 0.5);
    assert!(differs(&d[4800..], &c[4800..]) > 0.0 && tone(&d[4800..], 220.0) / rms(&d[4800..]) < 0.9 * tone(&c[4800..], 220.0) / rms(&c[4800..]), "the mod wheel (Ctrl Vibrato) adds vibrato");
    let mut s = mk();
    s.set(f::CTRL_VIB, 0.0);
    let a = play(&mut s, &[(f::FM, 0.1)], 57, 100, 0.5);
    s.controller(1, 127);
    let c = render(&mut s, 0.5);
    assert!((tone(&c[4800..], 220.0) / tone(&a[4800..], 220.0) - 1.0).abs() < 0.02, "unassigned: no vibrato");
}

#[test]
fn global_and_output() {
    // Transpose in semitones and octaves, Tune in cents
    let f0 = |set: &[(u32, f32)], about: f32| {
        let mut s = mk();
        s.set(f::FM, 0.0);
        let x = play(&mut s, set, 57, 100, 0.4);
        fundamental(&x[4800..], about * 0.8, about * 1.25) / about
    };
    assert!((f0(&[(f::TRANSPOSE_P, 0.0)], 55.0) - 1.0).abs() < 0.005);
    assert!((f0(&[(f::TRANSPOSE_P, 26.0)], 880.0) - 1.0).abs() < 0.005);
    assert!((f0(&[(f::TRANSPOSE_P, 20.0)], 329.63) - 1.0).abs() < 0.005, "+7");
    assert!((f0(&[(f::TUNE, 50.0)], 220.0 * 2f32.powf(0.5 / 12.0)) - 1.0).abs() < 0.003);
    // Mono: staccato retriggers, legato doesn't; Glide works in every mode
    let mut s = mk();
    for (id, v) in [(f::VOICES_P, 0.0), (f::FM, 0.0), (f::VE_A, 0.6), (f::GLIDE, 0.6)] {
        s.set(id, v);
    }
    s.note_on(57, 100);
    let a = render(&mut s, 1.0);
    s.note_on(69, 100);
    let b = render(&mut s, 0.1);
    assert!(rms(&b) > 0.8 * rms(&a[38400..]), "a legato note keeps the envelope going");
    let fb = fundamental(&b, 150.0, 600.0);
    assert!(fb > 225.0 && fb < 430.0, "glide {fb}");
    s.note_off(69);
    s.note_off(57);
    render(&mut s, 1.0);
    s.note_on(69, 100);
    let c = render(&mut s, 0.05);
    assert!(rms(&c) < 0.3 * rms(&a[38400..]), "a staccato note starts the envelope again");
    assert!(fundamental(&c, 150.0, 600.0) < 430.0, "Mono glides between detached notes too");
    // Legato: detached notes start on pitch
    let mut s = mk();
    for (id, v) in [(f::VOICES_P, 1.0), (f::FM, 0.0), (f::GLIDE, 0.6)] {
        s.set(id, v);
    }
    s.note_on(57, 100);
    render(&mut s, 0.3);
    s.note_off(57);
    render(&mut s, 1.0);
    s.note_on(69, 100);
    let c = render(&mut s, 0.1);
    assert!((fundamental(&c, 300.0, 600.0) / 440.0 - 1.0).abs() < 0.01);
    // polyphonic glide: a new note slides from the one before
    let mut s = mk();
    for (id, v) in [(f::VOICES_P, 16.0), (f::FM, 0.0), (f::GLIDE, 0.7)] {
        s.set(id, v);
    }
    s.note_on(45, 100);
    render(&mut s, 0.5);
    s.note_off(45);
    render(&mut s, 1.0);
    s.note_on(69, 100);
    let c = render(&mut s, 0.1);
    assert!(fundamental(&c, 100.0, 600.0) < 400.0);
    // Voices: with 2, a third note takes the oldest away; Unison layers two voices per note
    let mut s = mk();
    s.set(f::VOICES_P, 2.0);
    s.set(f::FM, 0.0);
    for n in [48, 55, 64] {
        s.note_on(n, 100);
    }
    let x = render(&mut s, 0.5);
    assert!(tone(&x[9600..], lg_dsp::midi_hz(48.0)) < 0.1 * tone(&x[9600..], lg_dsp::midi_hz(64.0)));
    let uni = play(&mut mk(), &[(f::UNISON, 1.0), (f::FM, 0.0)], 57, 100, 3.0);
    let one = play(&mut mk(), &[(f::UNISON, 0.0), (f::FM, 0.0)], 57, 100, 3.0);
    let env = |x: &[f32]| {
        let e: Vec<f32> = x[4800..].chunks(2400).map(rms).collect();
        e.iter().cloned().fold(0.0, f32::max) / e.iter().cloned().fold(1e9, f32::min)
    };
    assert!(env(&uni) > 1.5 && env(&one) < 1.05, "two detuned voices beat: {} {}", env(&uni), env(&one));
    let mut s = mk();
    s.set(f::UNISON, 1.0);
    for n in 40..56 {
        s.note_on(n, 100);
    }
    assert!(finite(&render(&mut s, 0.2)));
    // the sub-oscillator: a sine an octave below
    let x = play(&mut mk(), &[(f::SUB, 1.0), (f::FM, 0.0)], 57, 100, 0.5);
    assert!(tone(&x[4800..], 110.0) > 0.5 * tone(&x[4800..], 220.0));
    let x = play(&mut mk(), &[(f::SUB, 0.0), (f::FM, 0.0)], 57, 100, 0.5);
    assert!(tone(&x[4800..], 110.0) < 0.01 * tone(&x[4800..], 220.0));
    // Stereo Detune: left and right differ (and a mono sum beats)
    let st = |d: f32| {
        let mut s = mk();
        s.set(f::STEREO, d);
        s.set(f::FM, 0.3);
        s.note_on(57, 100);
        let (l, r) = render_st(&mut s, 1.0);
        differs(&l, &r)
    };
    assert!(st(0.0) < 1e-6 && st(0.3) > 0.1 && st(1.0) > 0.5);
    // the volume envelope
    let x = play(&mut mk(), &[(f::VE_A, 0.7), (f::VE_D, 0.5), (f::VE_S, 0.0), (f::FM, 0.0)], 57, 100, 3.0);
    assert!(rms(&x[..2400]) < 0.2 * rms(&x[14400..19200]) && rms(&x[134400..]) < 0.1 * rms(&x[14400..19200]));
    let mut s = mk();
    let x = play(&mut s, &[(f::VE_R, 0.75), (f::FM, 0.0)], 57, 100, 0.3);
    s.note_off(57);
    let y = render(&mut s, 0.5);
    assert!(rms(&y) > 0.1 * rms(&x));
    // Main Level; Velocity: harder = louder and brighter, fully left = no response
    let lv = |db: f32| rms(&play(&mut mk(), &[(f::LEVEL, db)], 57, 100, 0.3));
    assert!((lv(0.0) / lv(-6.0) - 2.0).abs() < 0.05);
    let v = |sens: f32, vel: u8| {
        let x = play(&mut mk(), &[(f::VELOCITY, sens), (f::FM, 0.5)], 57, vel, 0.5);
        (rms(&x[4800..]), bright(&x[4800..], 900.0))
    };
    let (soft, hard) = (v(1.0, 20), v(1.0, 127));
    assert!(hard.0 > 2.0 * soft.0 && hard.1 > 2.0 * soft.1, "{soft:?} {hard:?}");
    let (soft, hard) = (v(0.0, 20), v(0.0, 127));
    assert!((hard.0 / soft.0 - 1.0).abs() < 0.02 && (hard.1 / soft.1 - 1.0).abs() < 0.05);
    // Pitch Bend Range
    let mut s = mk();
    s.set(f::BEND, 24.0);
    s.set(f::FM, 0.0);
    s.pitch_bend(8191);
    let x = play(&mut s, &[], 45, 100, 0.4);
    assert!((fundamental(&x[4800..], 300.0, 600.0) / 440.0 - 1.0).abs() < 0.01);
}

#[test]
fn randomize() {
    let s = mk();
    let get = |id: u32| s.get(id);
    // a small amount stays near the sound, a large one goes far; each seed is a new variation
    let dist = |amount: f32, seed: u32| f::randomized(&get, amount, seed).iter().map(|&(id, v)| {
        let p = &f::PARAMS[id as usize];
        ((v - s.get(id)) / (p.max - p.min)).abs()
    }).fold(0.0, f32::max);
    assert!(dist(0.05, 1) <= 0.05 + 1e-3 + 0.5 / 31.0 && dist(1.0, 1) > 0.3);
    assert!(f::randomized(&get, 0.5, 1) != f::randomized(&get, 0.5, 2));
    for (id, v) in f::randomized(&get, 1.0, 7) {
        let p = &f::PARAMS[id as usize];
        assert!(v >= p.min && v <= p.max);
        if id == f::MOD_HARMONIC || id == f::CAR_HARMONIC {
            assert_eq!(v, v.round());
        }
    }
    // level, tuning and voices are left alone
    assert!(f::RANDOMIZED.iter().all(|id| ![f::LEVEL, f::TUNE, f::TRANSPOSE_P, f::VOICES_P].contains(id)));
}
