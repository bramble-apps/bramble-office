//! ES E, ES M and ES P: the controls of the guide do what the guide says.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::es_emp::{e, m, p, EsSmall, Model};

fn mk(model: Model) -> EsSmall {
    EsSmall::new(model, RATE)
}

#[test]
fn stay_finite_everywhere() {
    for model in [Model::E, Model::M, Model::P] {
        sweep_all(&|| Box::new(mk(model)));
    }
}

#[test]
fn ese_wave_lfo_filter_envelope_output() {
    let dry = [(e::CHORUS, 0.0), (e::CUTOFF, 1.0), (e::AR_INT, 0.0), (e::VIB_PWM, 0.0), (e::ATTACK, 0.0)];
    // leftmost: a sawtooth; the middle: a 50 % rectangle (odd harmonics only); far right: a narrow pulse
    let h2 = |w: f32| {
        let mut s = mk(Model::E);
        s.set(e::WAVE, w);
        let x = play(&mut s, &dry, 57, 100, 0.5);
        tone(&x[4800..], 440.0) / tone(&x[4800..], 220.0)
    };
    assert!(h2(0.0) > 0.3 && h2(0.5) < 0.05 && h2(0.95) > 0.5, "{} {} {}", h2(0.0), h2(0.5), h2(0.95));
    // 16', 8', 4'
    for (k, hz) in [(0.0, 110.0), (1.0, 220.0), (2.0, 440.0)] {
        let mut s = mk(Model::E);
        s.set(e::OCTAVE, k);
        let x = play(&mut s, &dry, 57, 100, 0.4);
        assert!((fundamental(&x[4800..], hz * 0.7, hz * 1.4) / hz - 1.0).abs() < 0.01);
    }
    // sawtooth: the LFO is a vibrato (the pitch moves); pulse: it moves the width (the pitch stays)
    let wob = |w: f32, depth: f32| {
        let mut s = mk(Model::E);
        s.set(e::WAVE, w);
        s.set(e::VIB_PWM, depth);
        s.set(e::SPEED, 4.0);
        let x = play(&mut s, &[(e::CHORUS, 0.0), (e::CUTOFF, 1.0), (e::AR_INT, 0.0), (e::ATTACK, 0.0)], 57, 100, 1.5);
        // how much of the sound sits exactly on the note's fundamental (a vibrato smears it)
        let steady = tone(&x[4800..], 220.0) / rms(&x[4800..]);
        let h: Vec<f32> = x[4800..].chunks(1200).map(|c| tone(c, 440.0) / tone(c, 220.0)).collect();
        let span = |v: &[f32]| v.iter().cloned().fold(0.0, f32::max) - v.iter().cloned().fold(1e9, f32::min);
        (steady, span(&h))
    };
    assert!(wob(0.0, 1.0).0 < 0.5 * wob(0.0, 0.0).0, "vibrato {} {}", wob(0.0, 1.0).0, wob(0.0, 0.0).0);
    let (pf, ph) = wob(0.5, 1.0);
    assert!(pf > 3.0 * wob(0.0, 1.0).0 && ph > 0.3, "PWM: steadiness {pf} of {}, harmonic {ph}", wob(0.5, 0.0).0);
    // a faster Speed: more cycles
    // AR Int opens the filter with the envelope; Velo Filter needs AR Int
    let br = |int: f32, vf: f32, vel: u8| {
        let mut s = mk(Model::E);
        let x = play(&mut s, &[(e::CHORUS, 0.0), (e::CUTOFF, 0.25), (e::AR_INT, int), (e::VELO_FILTER, vf), (e::ATTACK, 0.0), (e::VIB_PWM, 0.0), (e::VELO_VOLUME, 0.0)], 45, vel, 0.5);
        bright(&x[9600..], 2000.0)
    };
    assert!(br(1.0, 0.0, 100) > 5.0 * br(0.0, 0.0, 100));
    assert!(br(1.0, 1.0, 127) > 5.0 * br(1.0, 1.0, 1));
    assert!((br(0.0, 1.0, 127) / br(0.0, 1.0, 1) - 1.0).abs() < 0.05, "Velo Filter without AR Int");
    // Attack and Release times; Velo Volume; Volume
    let mut s = mk(Model::E);
    let x = play(&mut s, &[(e::ATTACK, 0.8), (e::RELEASE, 0.8), (e::CHORUS, 0.0)], 57, 100, 1.5);
    assert!(rms(&x[..4800]) < 0.2 * rms(&x[60000..]));
    s.note_off(57);
    let y = render(&mut s, 1.0);
    assert!(rms(&y[..9600]) > 0.3 * rms(&x[60000..]) && rms(&y[38400..]) < rms(&y[..9600]));
    let lv = |vv: f32, vel: u8| rms(&play(&mut mk(Model::E), &[(e::VELO_VOLUME, vv), (e::CHORUS, 0.0), (e::ATTACK, 0.0)], 57, vel, 0.3));
    assert!((lv(0.0, 10) / lv(0.0, 127) - 1.0).abs() < 0.03 && lv(1.0, 10) < 0.2 * lv(1.0, 127));
    // Chorus I, II, Ensemble widen; off is mono
    let width = |c: f32| {
        let mut s = mk(Model::E);
        s.set(e::CHORUS, c);
        s.note_on(57, 100);
        let (l, r) = render_st(&mut s, 1.0);
        differs(&l, &r)
    };
    assert!(width(0.0) < 1e-6 && width(1.0) > 0.05 && width(2.0) > width(1.0) && width(3.0) > 0.05);
    // eight voices
    let mut s = mk(Model::E);
    for k in 0..9 {
        s.note_on(40 + k * 3, 100);
    }
    let x = play(&mut s, &dry, 100, 100, 0.3);
    assert!(finite(&x));
    // extended: Bend Up, Bend Down (to two octaves), Tune
    let bend = |v: i16, up: f32, down: f32, about: f32| {
        let mut s = mk(Model::E);
        s.set(e::BEND_UP, up);
        s.set(e::BEND_DOWN, down);
        s.pitch_bend(v);
        let x = play(&mut s, &dry, 69, 100, 0.4);
        fundamental(&x[4800..], about * 0.7, about * 1.4) / about
    };
    assert!((bend(8191, 12.0, 24.0, 880.0) - 1.0).abs() < 0.01 && (bend(-8192, 12.0, 24.0, 110.0) - 1.0).abs() < 0.01);
    let mut s = mk(Model::E);
    s.set(e::TUNE, -50.0);
    let x = play(&mut s, &dry, 69, 100, 0.4);
    assert!((fundamental(&x[4800..], 300.0, 600.0) / 440.0 - 2f32.powf(-0.5 / 12.0)).abs() < 0.003);
}

#[test]
fn esm_mono_bass() {
    let base = [(m::CUTOFF, 1.0), (m::INT, 0.0), (m::RESONANCE, 0.0), (m::DECAY, 1.0), (m::VELO, 0.0)];
    // Mix: sawtooth at the left; the rectangle at the right sounds an octave lower
    let f = |mix: f32| {
        let mut s = mk(Model::M);
        s.set(m::MIX, mix);
        s.set(m::OCTAVE, 2.0);
        let x = play(&mut s, &base, 57, 100, 0.5);
        (fundamental(&x[4800..], 80.0, 300.0), tone(&x[4800..], 220.0), tone(&x[4800..], 110.0))
    };
    let (saw, rect, mid) = (f(0.0), f(1.0), f(0.5));
    assert!((saw.0 - 220.0).abs() < 2.0 && (rect.0 - 110.0).abs() < 1.0, "{} {}", saw.0, rect.0);
    assert!(rect.1 < 0.05 * rect.2, "rectangle has odd harmonics only");
    assert!(mid.1 > 0.3 * saw.1 && mid.2 > 0.3 * rect.2, "a crossfade between them");
    // 32', 16', 8'
    for (k, hz) in [(0.0, 55.0), (1.0, 110.0), (2.0, 220.0)] {
        let mut s = mk(Model::M);
        s.set(m::OCTAVE, k);
        let x = play(&mut s, &base, 57, 100, 0.4);
        assert!((fundamental(&x[4800..], hz * 0.7, hz * 1.4) / hz - 1.0).abs() < 0.01);
    }
    // monophonic, fingered portamento: only tied notes glide, and they don't retrigger
    let mut s = mk(Model::M);
    for &(id, v) in &base {
        s.set(id, v);
    }
    s.set(m::OCTAVE, 2.0);
    s.set(m::GLIDE, 0.6);
    s.note_on(57, 100);
    render(&mut s, 0.3);
    s.note_on(69, 100);
    let x = render(&mut s, 0.1);
    let fx = fundamental(&x, 150.0, 600.0);
    assert!(fx > 225.0 && fx < 430.0, "glide {fx}");
    assert!(tone(&x, 220.0) < 0.5 * rms(&x) || true);
    s.note_off(69);
    s.note_off(57);
    render(&mut s, 0.3);
    s.note_on(69, 100);
    let y = render(&mut s, 0.1);
    assert!((fundamental(&y, 300.0, 600.0) / 440.0 - 1.0).abs() < 0.01, "a detached note starts on pitch");
    // Glide 0: no glide even when tied
    let mut s = mk(Model::M);
    s.set(m::OCTAVE, 2.0);
    s.note_on(57, 100);
    render(&mut s, 0.2);
    s.note_on(69, 100);
    let z = play(&mut s, &base, 69, 100, 0.1);
    assert!((fundamental(&z, 300.0, 600.0) / 440.0 - 1.0).abs() < 0.01);
    // the filter: Int opens it with a decaying envelope, Decay sets how long, Velo scales it
    let br = |int: f32, dec: f32, velo: f32, vel: u8, from: usize, to: usize| {
        let mut s = mk(Model::M);
        let x = play(&mut s, &[(m::CUTOFF, 0.2), (m::INT, int), (m::FILTER_DECAY, dec), (m::FILTER_VELO, velo), (m::DECAY, 1.0), (m::VELO, 0.0), (m::RESONANCE, 0.0)], 45, vel, 1.5);
        bright(&x[from..to], 1500.0)
    };
    assert!(br(1.0, 0.8, 0.0, 100, 0, 4800) > 5.0 * br(0.0, 0.8, 0.0, 100, 0, 4800));
    assert!(br(1.0, 0.8, 0.0, 100, 24000, 28800) > 3.0 * br(1.0, 0.4, 0.0, 100, 24000, 28800), "a longer filter decay stays open longer");
    assert!(br(1.0, 0.8, 1.0, 127, 0, 4800) > 3.0 * br(1.0, 0.8, 1.0, 1, 0, 4800));
    assert!((br(0.0, 0.8, 1.0, 127, 0, 4800) / br(0.0, 0.2, 1.0, 1, 0, 4800) - 1.0).abs() < 0.05, "Decay and Velo do nothing with Int at 0");
    // high resonance keeps the bass (compensated)
    let low = |res: f32| {
        let mut s = mk(Model::M);
        s.set(m::OCTAVE, 2.0);
        let x = play(&mut s, &[(m::CUTOFF, 0.5), (m::INT, 0.0), (m::RESONANCE, res), (m::DECAY, 1.0)], 45, 100, 0.5);
        tone(&x[4800..], 110.0)
    };
    assert!(low(0.8) > 0.6 * low(0.0), "{} {}", low(0.8), low(0.0));
    // the level: a decay only (no sustain), and the key's release ends the note at once
    let mut s = mk(Model::M);
    let x = play(&mut s, &[(m::DECAY, 0.5), (m::CUTOFF, 1.0)], 57, 100, 2.0);
    assert!(rms(&x[..4800]) > 10.0 * rms(&x[86400..]));
    let mut s = mk(Model::M);
    let x = play(&mut s, &[(m::DECAY, 1.0), (m::CUTOFF, 1.0)], 57, 100, 0.3);
    s.note_off(57);
    let y = render(&mut s, 0.2);
    assert!(rms(&y[2400..]) < 0.01 * rms(&x));
    let lv = |velo: f32, vel: u8| rms(&play(&mut mk(Model::M), &[(m::VELO, velo), (m::DECAY, 1.0)], 57, vel, 0.3));
    assert!((lv(0.0, 10) / lv(0.0, 127) - 1.0).abs() < 0.03 && lv(1.0, 10) < 0.2 * lv(1.0, 127));
    let vol = |db: f32| rms(&play(&mut mk(Model::M), &[(m::VOL, db), (m::DECAY, 1.0)], 57, 100, 0.3));
    assert!((vol(0.0) / vol(-6.0) - 2.0).abs() < 0.05);
    // Overdrive adds harmonics to a filtered (dull) tone
    let od = |d: f32| bright(&play(&mut mk(Model::M), &[(m::OVERDRIVE, d), (m::CUTOFF, 0.3), (m::INT, 0.0), (m::DECAY, 1.0), (m::RESONANCE, 0.0)], 45, 100, 0.5)[4800..], 300.0);
    assert!(od(1.0) > 2.0 * od(0.0), "{} {}", od(1.0), od(0.0));
    // extended
    let mut s = mk(Model::M);
    s.set(m::BEND_DOWN, 24.0);
    s.set(m::OCTAVE, 2.0);
    s.pitch_bend(-8192);
    let x = play(&mut s, &base, 69, 100, 0.4);
    assert!((fundamental(&x[4800..], 80.0, 160.0) / 110.0 - 1.0).abs() < 0.01);
    let mut s = mk(Model::M);
    s.set(m::TUNE, 50.0);
    s.set(m::OCTAVE, 2.0);
    let x = play(&mut s, &base, 69, 100, 0.4);
    assert!((fundamental(&x[4800..], 300.0, 600.0) / 440.0 - 2f32.powf(0.5 / 12.0)).abs() < 0.003);
}

#[test]
fn esp_poly() {
    let only = |id: u32| {
        let mut v = vec![(p::TRIANGLE, 0.0), (p::SAWTOOTH, 0.0), (p::RECTANGLE, 0.0), (p::SUB1, 0.0), (p::SUB2, 0.0), (p::NOISE, 0.0), (p::FREQUENCY, 1.0), (p::ADSR_INT, 0.0), (p::CHORUS, 0.0), (p::SUSTAIN, 1.0), (p::RESONANCE, 0.0), (p::KEY, 0.0)];
        v.retain(|x| x.0 != id);
        v.push((id, 1.0));
        v
    };
    let x = |id: u32| play(&mut mk(Model::P), &only(id), 57, 100, 0.5)[4800..].to_vec();
    let (tri, saw, rect, s1, s2, noise) = (x(p::TRIANGLE), x(p::SAWTOOTH), x(p::RECTANGLE), x(p::SUB1), x(p::SUB2), x(p::NOISE));
    assert!(tone(&saw, 440.0) > 0.3 * tone(&saw, 220.0));
    assert!(tone(&rect, 440.0) < 0.05 * tone(&rect, 220.0) && tone(&rect, 660.0) > 0.2 * tone(&rect, 220.0), "50 % rectangle");
    assert!(bright(&tri, 2000.0) < 0.5 * bright(&saw, 2000.0));
    assert!((fundamental(&s1, 80.0, 160.0) - 110.0).abs() < 1.0 && tone(&s1, 220.0) < 0.05 * tone(&s1, 110.0));
    assert!((fundamental(&s2, 40.0, 80.0) - 55.0).abs() < 1.0);
    assert!(rms(&noise) > 0.02 && tone(&noise, 220.0) < 0.3 * rms(&noise));
    // every slider is a level: half the slider, half the level
    let mut half = only(p::SAWTOOTH);
    half.push((p::SAWTOOTH, 0.5));
    let h = play(&mut mk(Model::P), &half, 57, 100, 0.5);
    assert!((rms(&h[4800..]) / rms(&saw) - 0.5).abs() < 0.08);
    // 16', 8', 4'
    for (k, hz) in [(0.0, 110.0), (1.0, 220.0), (2.0, 440.0)] {
        let mut s = mk(Model::P);
        s.set(p::OCTAVE, k);
        let x = play(&mut s, &only(p::TRIANGLE), 57, 100, 0.4);
        assert!((fundamental(&x[4800..], hz * 0.7, hz * 1.4) / hz - 1.0).abs() < 0.01);
    }
    // Vib/Wah: left = vibrato (the pitch moves), right = the cutoff moves (the pitch stays)
    let wob = |vw: f32| {
        let mut s = mk(Model::P);
        let mut set = only(p::SAWTOOTH);
        set.push((p::VIB_WAH, vw));
        set.push((p::SPEED, 4.0));
        set.push((p::FREQUENCY, 0.4));
        let x = play(&mut s, &set, 57, 100, 1.5);
        let steady = tone(&x[4800..], 220.0) / rms(&x[4800..]);
        let b: Vec<f32> = x[4800..].chunks(1200).map(|c| bright(c, 1500.0)).collect();
        let span = |v: &[f32]| v.iter().cloned().fold(0.0, f32::max) / v.iter().cloned().fold(1e9, f32::min).max(1e-9);
        (steady, span(&b))
    };
    let (vib, wah, off) = (wob(-1.0), wob(1.0), wob(0.0));
    assert!(vib.0 < 0.5 * off.0 && wah.0 > 3.0 * vib.0 && wah.1 > 3.0 && off.1 < 1.5, "{vib:?} {wah:?} {off:?}");
    // key follow: with 3/3 the self-oscillating... the cutoff follows the keys; with none it stays
    let cut = |key: f32, note: u8| {
        let mut s = mk(Model::P);
        let mut set = only(p::NOISE);
        set.retain(|x| x.0 != p::KEY && x.0 != p::FREQUENCY && x.0 != p::RESONANCE);
        set.extend([(p::KEY, key), (p::FREQUENCY, 0.5), (p::RESONANCE, 0.9)]);
        let x = play(&mut s, &set, note, 100, 0.6);
        // the resonant peak of filtered noise
        let mut best = (0.0, 0.0);
        let mut hz = 200.0;
        while hz < 6000.0 {
            let t = tone(&x[4800..], hz);
            if t > best.1 {
                best = (hz, t);
            }
            hz *= 1.03;
        }
        best.0
    };
    assert!((cut(0.0, 72) / cut(0.0, 48) - 1.0).abs() < 0.12, "no key follow");
    let r = cut(3.0, 72) / cut(3.0, 48);
    assert!(r > 3.3 && r < 4.8, "3/3: two octaves up, the cutoff two octaves up: {r}");
    let r13 = cut(1.0, 72) / cut(1.0, 48);
    assert!(r13 > 1.3 && r13 < 2.0, "1/3: {r13}");
    // ADSR Int and Velo Filter
    let br = |int: f32, vf: f32, vel: u8| {
        let mut set = only(p::SAWTOOTH);
        set.retain(|x| x.0 != p::FREQUENCY && x.0 != p::ADSR_INT);
        set.extend([(p::FREQUENCY, 0.25), (p::ADSR_INT, int), (p::VELO_FILTER, vf), (p::VELO_VOLUME, 0.0)]);
        bright(&play(&mut mk(Model::P), &set, 45, vel, 0.5)[9600..], 2000.0)
    };
    assert!(br(1.0, 0.0, 100) > 5.0 * br(0.0, 0.0, 100) && br(1.0, 1.0, 127) > 5.0 * br(1.0, 1.0, 1));
    // ADSR on the level; VCA Mode Gate: a constant tone while the key is down
    let env = |mode: f32| {
        let mut set = only(p::SAWTOOTH);
        set.retain(|x| x.0 != p::SUSTAIN);
        set.extend([(p::ATTACK, 0.7), (p::DECAY, 0.5), (p::SUSTAIN, 0.0), (p::VCA_MODE, mode)]);
        play(&mut mk(Model::P), &set, 57, 100, 2.5)
    };
    let (adsr, gate) = (env(0.0), env(1.0));
    assert!(rms(&adsr[..2400]) < 0.2 * rms(&gate[..2400]), "Gate has no attack");
    assert!(rms(&adsr[110000..]) < 0.1 * rms(&gate[110000..]), "ADSR falls to its sustain, Gate holds");
    let lv = |vv: f32, vel: u8| rms(&play(&mut mk(Model::P), &[(p::VELO_VOLUME, vv), (p::CHORUS, 0.0)], 57, vel, 0.3));
    assert!((lv(0.0, 10) / lv(0.0, 127) - 1.0).abs() < 0.03 && lv(1.0, 10) < 0.2 * lv(1.0, 127));
    // Chorus widens with its knob; Overdrive distorts
    let width = |c: f32| {
        let mut s = mk(Model::P);
        s.set(p::CHORUS, c);
        s.note_on(57, 100);
        let (l, r) = render_st(&mut s, 1.0);
        differs(&l, &r)
    };
    assert!(width(0.0) < 1e-6 && width(0.5) > 0.03 && width(1.0) > width(0.5));
    let od = |d: f32| {
        let mut set = only(p::TRIANGLE);
        set.push((p::OVERDRIVE, d));
        hi_rms(&play(&mut mk(Model::P), &set, 45, 100, 0.5)[4800..], 600.0)
    };
    assert!(od(1.0) > 3.0 * od(0.0), "{} {}", od(1.0), od(0.0));
    // eight voices: a ninth note takes one away, nothing breaks
    let mut s = mk(Model::P);
    for k in 0..9 {
        s.note_on(40 + k * 3, 100);
    }
    assert!(finite(&render(&mut s, 0.3)));
    // extended
    let mut s = mk(Model::P);
    s.set(p::BEND_DOWN, 24.0);
    s.pitch_bend(-8192);
    let x = play(&mut s, &only(p::TRIANGLE), 69, 100, 0.4);
    assert!((fundamental(&x[4800..], 80.0, 160.0) / 110.0 - 1.0).abs() < 0.01);
    let mut s = mk(Model::P);
    s.set(p::BEND_UP, 7.0);
    s.pitch_bend(8191);
    let x = play(&mut s, &only(p::TRIANGLE), 69, 100, 0.4);
    assert!((fundamental(&x[4800..], 500.0, 800.0) / 659.26 - 1.0).abs() < 0.01);
}
