//! ES1: every control of the guide changes the sound the way the guide says.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::es1::{self as e, Es1};

fn es1() -> Es1 {
    let mut s = Es1::new(RATE);
    s.set(e::ANALOG, 0.0);
    s
}

#[test]
fn stays_finite_everywhere() {
    sweep_all(&|| Box::new(Es1::new(RATE)));
}

#[test]
fn waves_and_octaves() {
    // sawtooth has every harmonic, square only the odd ones, triangle is softer than both
    let saw = play(&mut es1(), &[(e::WAVE, 1.0), (e::CUTOFF, 1.0), (e::RESONANCE, 0.0)], 57, 100, 0.5);
    let sq = play(&mut es1(), &[(e::WAVE, 2.0), (e::CUTOFF, 1.0), (e::RESONANCE, 0.0)], 57, 100, 0.5);
    let tri = play(&mut es1(), &[(e::WAVE, 0.0), (e::CUTOFF, 1.0), (e::RESONANCE, 0.0)], 57, 100, 0.5);
    let (s, q) = (&saw[4800..], &sq[4800..]);
    assert!(tone(s, 440.0) > 0.3 * tone(s, 220.0));
    assert!(tone(q, 440.0) < 0.05 * tone(q, 220.0));
    assert!(bright(&tri[4800..], 2000.0) < 0.5 * bright(s, 2000.0));
    // a narrow pulse gets its even harmonics back
    let pu = play(&mut es1(), &[(e::WAVE, 3.0), (e::CUTOFF, 1.0), (e::RESONANCE, 0.0)], 57, 100, 0.5);
    assert!(tone(&pu[4800..], 440.0) > 0.3 * tone(&pu[4800..], 220.0));
    // 32' to 2': four octaves
    for (k, hz) in [(0.0, 55.0), (2.0, 220.0), (4.0, 880.0)] {
        let x = play(&mut es1(), &[(e::OCTAVE, k), (e::WAVE, 0.0), (e::CUTOFF, 1.0)], 57, 100, 0.5);
        let f = fundamental(&x[4800..], hz * 0.7, hz * 1.4);
        assert!((f / hz - 1.0).abs() < 0.01, "octave {k}: {f} Hz");
    }
}

#[test]
fn sub_oscillator_and_mix() {
    let base = [(e::WAVE, 0.0), (e::CUTOFF, 1.0), (e::RESONANCE, 0.0), (e::MIX, 1.0)];
    // all sub: one octave (110 Hz) or two (55 Hz) below the A at 220 Hz
    let mut s = es1();
    s.set(e::SUB, 1.0);
    let x = play(&mut s, &base, 57, 100, 0.5);
    assert!(tone(&x[4800..], 110.0) > 10.0 * tone(&x[4800..], 220.0));
    let mut s = es1();
    s.set(e::SUB, 2.0);
    let x = play(&mut s, &base, 57, 100, 0.5);
    assert!(tone(&x[4800..], 55.0) > 0.1);
    // noise has no pitch
    let mut s = es1();
    s.set(e::SUB, 6.0);
    let x = play(&mut s, &base, 57, 100, 0.5);
    assert!(rms(&x) > 0.02 && tone(&x[4800..], 220.0) < 0.3 * rms(&x));
    // Off: nothing of the sub left, and with Mix all the way over only silence (no resonance)
    let mut s = es1();
    s.set(e::SUB, 0.0);
    let x = play(&mut s, &base, 57, 100, 0.5);
    assert!(rms(&x) < 1e-3);
    // EXT: the side chain goes through filter and amplifier
    let mut s = es1();
    s.set(e::SUB, 7.0);
    for &(id, v) in &base {
        s.set(id, v);
    }
    s.note_on(57, 100);
    let mut out = Vec::new();
    for b in 0..80 {
        let sc: Vec<f32> = (0..256).map(|i| (std::f32::consts::TAU * 1000.0 * (b * 256 + i) as f32 / RATE).sin() * 0.5).collect();
        s.side_chain(&sc, &sc);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        s.render(&mut l, &mut r);
        out.extend(l);
    }
    assert!(tone(&out[4800..], 1000.0) > 0.05, "side chain not heard: {}", tone(&out[4800..], 1000.0));
}

#[test]
fn filter_slopes_resonance_key_and_boost() {
    let set = |slope: f32, res: f32| [(e::WAVE, 1.0), (e::CUTOFF, 0.45), (e::RESONANCE, res), (e::SLOPE, slope), (e::KEY, 0.0), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 0.0), (e::DRIVE, 0.0)];
    let hi = |slope: f32| bright(&play(&mut es1(), &set(slope, 0.0), 45, 100, 0.5)[4800..], 3000.0);
    // 24 dB cuts the top hardest, then 18, then 12
    assert!(hi(0.0) < hi(3.0) && hi(3.0) < hi(2.0), "{} {} {}", hi(0.0), hi(3.0), hi(2.0));
    // classic loses low end with resonance, fat keeps it
    let low = |slope: f32, res: f32| tone(&play(&mut es1(), &set(slope, res), 45, 100, 0.5)[4800..], 110.0);
    let classic = low(0.0, 0.8) / low(0.0, 0.0);
    let fat = low(1.0, 0.8) / low(1.0, 0.0);
    assert!(classic < 0.6 && fat > 1.5 * classic, "classic {classic} fat {fat}");
    // full resonance, no oscillator: the filter sings by itself, and Key makes it follow the keys
    let osc = |note: u8, key: f32| {
        let x = play(&mut es1(), &[(e::SUB, 0.0), (e::MIX, 1.0), (e::RESONANCE, 1.0), (e::CUTOFF, 0.5), (e::KEY, key), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 0.0)], note, 100, 0.6);
        assert!(rms(&x[9600..]) > 0.01, "no self-oscillation");
        fundamental(&x[9600..], 150.0, 3000.0)
    };
    let (a, b) = (osc(60, 1.0), osc(72, 1.0));
    assert!((b / a - 2.0).abs() < 0.15, "key follow: {a} {b}");
    let (a, b) = (osc(60, 0.0), osc(72, 0.0));
    assert!((b / a - 1.0).abs() < 0.03, "no key follow: {a} {b}");
    // Drive overdrives the filter (more harmonics of a triangle); Filter Boost keeps it cleaner and louder
    let tri = |drive: f32, boost: f32| play(&mut es1(), &[(e::WAVE, 0.0), (e::CUTOFF, 1.0), (e::RESONANCE, 0.0), (e::DRIVE, drive), (e::BOOST, boost)], 45, 100, 0.5);
    let (clean, driven) = (tri(0.0, 0.0), tri(1.0, 0.0));
    assert!(bright(&driven[4800..], 1500.0) > 3.0 * bright(&clean[4800..], 1500.0));
    let boosted = tri(1.0, 1.0);
    assert!(bright(&boosted[4800..], 1500.0) < bright(&driven[4800..], 1500.0));
    assert!(rms(&boosted) > rms(&driven));
}

#[test]
fn envelope_modes_and_velocity_ranges() {
    let base = [(e::WAVE, 1.0), (e::ATTACK, 0.75), (e::DECAY, 0.5), (e::SUSTAIN, 0.0), (e::RELEASE, 0.3), (e::LEVEL_VEL_LO, 1.0), (e::LEVEL_VEL_HI, 1.0), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 0.0), (e::CUTOFF, 1.0)];
    let run = |mode: f32| {
        let mut s = es1();
        s.set(e::AMP_MODE, mode);
        play(&mut s, &base, 57, 100, 2.5)
    };
    let (agr, adsr, gr) = (run(0.0), run(1.0), run(2.0));
    let seg = |x: &[f32], a: f32, b: f32| rms(&x[(a * RATE) as usize..(b * RATE) as usize]);
    // GateR: full level at once; the others fade in over the attack (about half a second)
    assert!(seg(&gr, 0.0, 0.05) > 5.0 * seg(&adsr, 0.0, 0.05));
    assert!(seg(&gr, 0.0, 0.05) > 5.0 * seg(&agr, 0.0, 0.05));
    // ADSR falls to its sustain of zero; AGateR and GateR hold the level
    assert!(seg(&adsr, 2.2, 2.5) < 0.1 * seg(&agr, 2.2, 2.5));
    assert!(seg(&gr, 2.2, 2.5) > 0.8 * seg(&gr, 0.5, 0.8));
    // the filter still gets the whole ADSR in GateR mode
    let mut s = es1();
    s.set(e::AMP_MODE, 2.0);
    s.set(e::ADSR_VEL_LO, 1.0);
    s.set(e::ADSR_VEL_HI, 1.0);
    let mut b2 = base.to_vec();
    b2.retain(|x| x.0 != e::ADSR_VEL_LO && x.0 != e::ADSR_VEL_HI && x.0 != e::CUTOFF);
    b2.push((e::CUTOFF, 0.2));
    let x = play(&mut s, &b2, 57, 100, 2.5);
    assert!(bright(&x[(0.6 * RATE) as usize..(0.9 * RATE) as usize], 1500.0) > 4.0 * bright(&x[(2.2 * RATE) as usize..], 1500.0));
    // Level via Vel: arrows together = no velocity response; apart = soft notes quieter
    let lv = |lo: f32, hi: f32, vel: u8| rms(&play(&mut es1(), &[(e::LEVEL_VEL_LO, lo), (e::LEVEL_VEL_HI, hi), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 0.0)], 57, vel, 0.3));
    assert!((lv(0.8, 0.8, 20) / lv(0.8, 0.8, 127) - 1.0).abs() < 0.02);
    assert!(lv(0.1, 1.0, 1) < 0.15 * lv(0.1, 1.0, 127));
    // ADSR via Vel: the upper arrow is the cutoff envelope amount at full velocity
    let br = |vel: u8| bright(&play(&mut es1(), &[(e::CUTOFF, 0.25), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 1.0), (e::SUSTAIN, 1.0), (e::LEVEL_VEL_LO, 1.0)], 45, vel, 0.4)[4800..], 2000.0);
    assert!(br(127) > 5.0 * br(1));
}

#[test]
fn lfo_router_and_modulation_envelope() {
    // rate 0: a constant, so the wheel sets the pitch offset by hand
    let mut s = es1();
    for (id, v) in [(e::WAVE, 0.0), (e::LFO_RATE, 0.0), (e::LFO_TARGET, 0.0), (e::LFO_INT_LO, 0.0), (e::LFO_INT_HI, 1.0), (e::CUTOFF, 1.0)] {
        s.set(id, v);
    }
    let a = fundamental(&play(&mut s, &[], 57, 100, 0.4)[4800..], 100.0, 2000.0);
    s.controller(1, 127);
    let b = fundamental(&render(&mut s, 0.4)[4800..], 100.0, 2000.0);
    assert!((a - 220.0).abs() < 2.0 && b > 1.5 * a, "{a} {b}");
    // a free LFO on Volume: the level goes up and down at its rate; synced, the tempo sets it
    let trem = |rate: f32, bpm: f64| {
        let mut s = es1();
        s.set_tempo(bpm);
        let x = play(&mut s, &[(e::LFO_RATE, rate), (e::LFO_TARGET, 5.0), (e::LFO_INT_LO, 1.0), (e::LFO_INT_HI, 1.0), (e::LFO_WAVE, 3.0), (e::WAVE, 2.0)], 69, 100, 4.0);
        // count level dips
        let env: Vec<f32> = x.chunks(480).map(rms).collect();
        let mid = (env.iter().cloned().fold(0.0, f32::max)) * 0.5;
        env.windows(2).filter(|w| w[0] < mid && w[1] >= mid).count()
    };
    assert!(trem(0.6, 120.0) > trem(0.3, 120.0));
    // −0.5 is one of the synced lengths: doubling the tempo doubles the rate
    let (slow, fast) = (trem(-0.3, 60.0), trem(-0.3, 120.0));
    assert!(fast as f32 > 1.6 * slow as f32 && slow >= 1, "{slow} {fast}");
    // Pulse Width target, Form full, Int via Vel apart: the velocity sets the pulse width
    let pw = |vel: u8| {
        let x = play(&mut es1(), &[(e::WAVE, 2.0), (e::ENV_TARGET, 1.0), (e::FORM, 0.0), (e::ENV_INT_LO, 0.0), (e::ENV_INT_HI, 1.0), (e::CUTOFF, 1.0), (e::LFO_INT_HI, 0.0), (e::LEVEL_VEL_LO, 1.0)], 57, vel, 0.4);
        tone(&x[4800..], 440.0) / tone(&x[4800..], 220.0)
    };
    assert!(pw(1) < 0.05 && pw(127) > 0.3, "{} {}", pw(1), pw(127));
    // Cutoff target: positive Form fades the modulation in, negative fades it out
    let cut = |form: f32| {
        let x = play(&mut es1(), &[(e::CUTOFF, 0.2), (e::ENV_TARGET, 3.0), (e::FORM, form), (e::ENV_INT_LO, 1.0), (e::ENV_INT_HI, 1.0), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 0.0), (e::LFO_INT_HI, 0.0)], 45, 100, 3.0);
        (bright(&x[2400..9600], 1500.0), bright(&x[(2.6 * RATE) as usize..], 1500.0))
    };
    let (a0, a1) = cut(0.5);
    let (d0, d1) = cut(-0.5);
    assert!(a1 > 5.0 * a0 && d0 > 5.0 * d1, "in {a0} {a1}, out {d0} {d1}");
    // LFO Amp: the envelope fades the LFO's vibrato in
    let vib = |x: &[f32]| {
        let f: Vec<f32> = x.chunks(2400).map(|c| fundamental(c, 150.0, 330.0)).collect();
        f.iter().cloned().fold(0.0, f32::max) - f.iter().cloned().fold(1e9, f32::min)
    };
    let x = play(&mut es1(), &[(e::WAVE, 0.0), (e::CUTOFF, 1.0), (e::LFO_TARGET, 0.0), (e::LFO_INT_LO, 0.0), (e::LFO_INT_HI, 0.0), (e::LFO_RATE, 0.7), (e::ENV_TARGET, 7.0), (e::FORM, 0.6), (e::ENV_INT_LO, 0.5), (e::ENV_INT_HI, 0.5)], 57, 100, 4.0);
    assert!(vib(&x[..(0.5 * RATE) as usize]) < 0.3 * vib(&x[(3.2 * RATE) as usize..]), "{} {}", vib(&x[..(0.5 * RATE) as usize]), vib(&x[(3.2 * RATE) as usize..]));
    // Filter FM adds sidebands a static cutoff offset doesn't
    let fm = play(&mut es1(), &[(e::WAVE, 0.0), (e::CUTOFF, 0.5), (e::RESONANCE, 0.7), (e::ENV_TARGET, 6.0), (e::ENV_INT_LO, 1.0), (e::ENV_INT_HI, 1.0), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 0.0)], 57, 100, 0.5);
    let no = play(&mut es1(), &[(e::WAVE, 0.0), (e::CUTOFF, 0.5), (e::RESONANCE, 0.7), (e::ENV_TARGET, 6.0), (e::ENV_INT_LO, 0.0), (e::ENV_INT_HI, 0.0), (e::ADSR_VEL_LO, 0.0), (e::ADSR_VEL_HI, 0.0)], 57, 100, 0.5);
    assert!(differs(&fm[4800..], &no[4800..]) > 0.2);
}

#[test]
fn global_voices_glide_analog_bend_chorus_remote() {
    // Legato: one voice, no new attack on tied notes, the pitch glides only then
    let mut s = es1();
    for (id, v) in [(e::VOICES_P, 0.0), (e::GLIDE, 0.6), (e::WAVE, 0.0), (e::CUTOFF, 1.0), (e::ATTACK, 0.6), (e::SUSTAIN, 1.0), (e::LEVEL_VEL_LO, 1.0)] {
        s.set(id, v);
    }
    s.note_on(57, 100);
    let a = render(&mut s, 1.0);
    s.note_on(69, 100);
    let b = render(&mut s, 0.1);
    assert!(rms(&b) > 0.8 * rms(&a[(0.8 * RATE) as usize..]), "legato retriggered the envelope");
    let f = fundamental(&b, 150.0, 600.0);
    assert!(f > 225.0 && f < 430.0, "no glide on the tied note: {f}");
    s.note_off(69);
    let c = render(&mut s, 0.3);
    assert!((fundamental(&c[4800..], 150.0, 600.0) - 220.0).abs() < 25.0, "falls back to the held note");
    // a detached note in Legato starts on pitch
    s.note_off(57);
    render(&mut s, 1.5);
    s.note_on(81, 100);
    let d = render(&mut s, 0.3);
    assert!((fundamental(&d[9600..], 600.0, 1200.0) / 880.0 - 1.0).abs() < 0.02);
    // Voices 1: always glides; Voices 4: four notes sound, a fifth takes one away
    let mut s = es1();
    for (id, v) in [(e::VOICES_P, 4.0), (e::WAVE, 0.0), (e::CUTOFF, 1.0), (e::SUSTAIN, 1.0)] {
        s.set(id, v);
    }
    for n in [48, 52, 55, 59, 62] {
        s.note_on(n, 100);
    }
    let x = render(&mut s, 0.5);
    let x = &x[9600..];
    assert!(tone(x, lg_dsp::midi_hz(62.0)) > 0.02 && tone(x, lg_dsp::midi_hz(48.0)) < 0.2 * tone(x, lg_dsp::midi_hz(62.0)));
    // Analog 0: voices start in step (two renders are the same); above 0 they drift
    let one = |an: f32| {
        let mut s = Es1::new(RATE);
        s.set(e::ANALOG, an);
        s.note_on(57, 100);
        s.note_on(57 + 12, 100);
        render(&mut s, 0.3)
    };
    assert!(differs(&one(0.0), &one(0.0)) < 1e-6);
    let (p, q) = (fundamental(&play(&mut Es1::new(RATE), &[(e::ANALOG, 1.0), (e::WAVE, 0.0)], 57, 100, 0.5)[4800..], 150.0, 300.0), 220.0);
    assert!((p - q).abs() > 0.02 && (p / q - 1.0).abs() < 0.02, "analog detune {p}");
    // Tune in cents, Bender Range up and its own range down
    let f = fundamental(&play(&mut es1(), &[(e::TUNE, 50.0), (e::WAVE, 0.0)], 57, 100, 0.5)[4800..], 150.0, 300.0);
    assert!((f / 220.0 - 2f32.powf(0.5 / 12.0)).abs() < 0.003);
    let bend = |down: f32, v: i16| {
        let mut s = es1();
        s.set(e::BEND, 12.0);
        s.set(e::BEND_DOWN, down);
        s.set(e::WAVE, 0.0);
        s.pitch_bend(v);
        fundamental(&play(&mut s, &[], 57, 100, 0.5)[4800..], 60.0, 600.0)
    };
    assert!((bend(0.0, 8191) / 440.0 - 1.0).abs() < 0.01);
    assert!((bend(0.0, -8192) / 110.0 - 1.0).abs() < 0.01, "linked");
    assert!((bend(1.0, -8192) / 220.0 - 1.0).abs() < 0.01, "0 = no bend down");
    assert!((bend(3.0, -8192) / 196.0 - 1.0).abs() < 0.01, "2 semitones down");
    // Chorus widens (left and right differ), C2 more than C1; Out Level is the volume
    let width = |c: f32| {
        let mut s = es1();
        s.set(e::CHORUS_P, c);
        s.note_on(57, 100);
        let (l, r) = render_st(&mut s, 1.0);
        differs(&l, &r)
    };
    assert!(width(0.0) < 1e-6 && width(1.0) > 0.05 && width(2.0) > width(1.0) && width(3.0) > 0.05);
    let lv = |db: f32| rms(&play(&mut es1(), &[(e::OUT, db)], 57, 100, 0.3));
    assert!((lv(-6.0) / lv(-12.0) - 2.0).abs() < 0.05);
    // MIDI Mode: Full Remote lets CC 17 move the cutoff; Off leaves it alone
    let mut s = es1();
    s.controller(17, 0);
    assert!(s.get(e::CUTOFF) > 0.5);
    s.set(e::MIDI_MODE, 1.0);
    s.controller(17, 0);
    assert_eq!(s.get(e::CUTOFF), 0.0);
    s.controller(19, 127);
    assert_eq!(s.get(e::SLOPE), 3.0);
    s.controller(115, 0);
    assert_eq!(s.get(e::VOICES_P), 0.0);
}
