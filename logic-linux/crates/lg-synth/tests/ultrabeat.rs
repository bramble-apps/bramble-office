//! Ultrabeat: every area of the guide changes the sound (or the pattern) the way it says.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin, Transport};
use lg_synth::dsp::Rng;
use lg_synth::ultrabeat::{self as ub, vid, Cell, Init, Ultrabeat};

/// A plain sound on voice `v`: sine, no filter, long decay.
fn plain(v: usize) -> Ultrabeat {
    let mut u = Ultrabeat::new(RATE);
    for (id, x) in u.init_pairs(v, Init::Standard) {
        u.set(id, x);
    }
    u.set(vid(v, ub::ENV0 + 24 + 2), 0.78);
    u
}

fn setv(u: &mut Ultrabeat, v: usize, s: &[(u32, f32)]) {
    for &(k, x) in s {
        u.set(vid(v, k), x);
    }
}

fn hit(u: &mut Ultrabeat, v: usize, s: &[(u32, f32)], secs: f32) -> Vec<f32> {
    setv(u, v, s);
    let note = if v < 24 { 36 + v as u8 } else { 60 };
    u.note_on(note, 110);
    render(u, secs)
}

fn f0() -> f32 {
    261.626
}

#[test]
fn stays_finite_everywhere() {
    let ids: Vec<(u32, f32, f32)> = Ultrabeat::new(RATE).params().iter().filter(|p| !p.hidden || p.id < 100).filter(|p| p.id < 100 || (p.id >= 1000 && (p.id - 1000) / 256 == 0)).map(|p| (p.id, p.min, p.max)).collect();
    assert!(ids.len() > 150);
    for (id, min, max) in ids {
        for v in [min, max] {
            let mut u = Ultrabeat::new(RATE);
            u.set(id, v);
            u.note_on(36, 110);
            u.note_on(60, 60);
            let a = render(&mut u, 0.12);
            u.note_off(36);
            let b = render(&mut u, 0.08);
            assert!(finite(&a) && finite(&b), "parameter {id} = {v} blows up");
        }
    }
}

#[test]
fn twenty_five_voices_and_the_keys() {
    // notes C1 … B2 play sounds 1–24, C3 and up the 25th, which plays chromatically
    let u = Ultrabeat::new(RATE);
    assert_eq!(u.params().iter().filter(|p| p.name.starts_with("Voice ") && p.name.ends_with("Osc 1 Pitch")).count(), 25);
    let mut a = plain(5);
    a.set(vid(5, ub::SOLO), 1.0);
    let x = play(&mut a, &[], 41, 100, 0.2);
    assert!(rms(&x) > 0.01, "note 41 plays sound 6");
    let mut a = plain(5);
    a.set(vid(5, ub::SOLO), 1.0);
    let x = play(&mut a, &[], 36, 100, 0.2);
    assert!(rms(&x) < 1e-6, "solo on sound 6 mutes sound 1");
    // the 25th sound follows the key
    let mut a = plain(24);
    a.set(vid(24, ub::SOLO), 1.0);
    let lo = play(&mut a, &[], 60, 100, 0.4);
    a.reset();
    let hi = play(&mut a, &[], 72, 100, 0.4);
    let (fl, fh) = (fundamental(&lo[4800..], 200.0, 330.0), fundamental(&hi[4800..], 400.0, 660.0));
    assert!((fh / fl - 2.0).abs() < 0.03, "{fl} {fh}");
    // notes below C1 do nothing
    let mut a = plain(0);
    assert!(rms(&play(&mut a, &[], 30, 100, 0.2)) < 1e-6);
    assert_eq!(ub::vspecs().len() as u32 > 180, true);
}

#[test]
fn phase_oscillator_shapes() {
    let sine = hit(&mut plain(0), 0, &[], 0.5);
    let s = &sine[4800..];
    assert!(tone(s, f0()) > 0.1);
    assert!(tone(s, 2.0 * f0()) < 0.02 * tone(s, f0()), "all zero is a sine");
    // saturation: clipped, more odd overtones (square)
    let sq = hit(&mut plain(0), 0, &[(ub::O1_SAT, 1.0), (ub::O1_SLOPE, 1.0)], 0.5);
    let q = &sq[4800..];
    assert!(tone(q, 3.0 * f0()) > 0.2 * tone(q, f0()), "odd harmonics");
    assert!(tone(q, 2.0 * f0()) < 0.05 * tone(q, f0()), "no even harmonics");
    // asym up and slope -0.2: sawtooth, with its even harmonics
    let saw = hit(&mut plain(0), 0, &[(ub::O1_ASYM, 1.0), (ub::O1_SLOPE, -0.2)], 0.5);
    let w = &saw[4800..];
    assert!(tone(w, 2.0 * f0()) > 0.35 * tone(w, f0()), "sawtooth has the second harmonic");
    // slope alone steepens: more nasal
    let steep = hit(&mut plain(0), 0, &[(ub::O1_SLOPE, 1.0)], 0.5);
    assert!(bright(&steep[4800..], 600.0) > 3.0 * bright(s, 600.0));
    // oscillator 2 has the same shaper; its volume at -60 dB is off
    let two = hit(&mut plain(0), 0, &[(ub::O1_ON, 0.0), (ub::O2_ON, 1.0), (ub::O2_ASYM, 1.0), (ub::O2_SLOPE, -0.2)], 0.5);
    assert!(tone(&two[4800..], 2.0 * f0()) > 0.35 * tone(&two[4800..], f0()));
    let off = hit(&mut plain(0), 0, &[(ub::O1_VOL, -60.0)], 0.3);
    assert!(rms(&off) < 1e-6, "-60 dB is silent");
    // pitch in semitones and shift-cents
    let up = hit(&mut plain(0), 0, &[(ub::O1_PITCH, 12.0)], 0.5);
    assert!((fundamental(&up[4800..], 400.0, 660.0) / (2.0 * f0()) - 1.0).abs() < 0.02);
}

#[test]
fn fm_ring_modulator_and_side_chain() {
    // FM needs oscillator 1 in FM mode and oscillator 2 on; the amount adds partials
    let fm = |amt: f32, o2: f32| {
        let x = hit(&mut plain(0), 0, &[(ub::O1_MODE, 1.0), (ub::O2_ON, o2), (ub::O2_PITCH, 7.0), (ub::O2_VOL, -60.0), (ub::O1_FM, amt)], 0.5);
        bright(&x[4800..], 1500.0)
    };
    assert!(fm(0.7, 1.0) > 5.0 * fm(0.0, 1.0) + 1e-4, "FM amount adds sidebands");
    assert!(fm(0.7, 0.0) < 1e-3, "without oscillator 2 it stays a sine");
    // oscillator 2's pitch sets the ratio: the spectrum changes
    let a = hit(&mut plain(0), 0, &[(ub::O1_MODE, 1.0), (ub::O2_ON, 1.0), (ub::O2_PITCH, 12.0), (ub::O1_FM, 0.5), (ub::O2_VOL, -60.0)], 0.4);
    let b = hit(&mut plain(0), 0, &[(ub::O1_MODE, 1.0), (ub::O2_ON, 1.0), (ub::O2_PITCH, 7.0), (ub::O1_FM, 0.5), (ub::O2_VOL, -60.0)], 0.4);
    assert!(differs(&a, &b) > 0.3);
    // ring modulator: sum and difference tones of the two oscillators, whatever their levels
    let rm = hit(&mut plain(0), 0, &[(ub::O2_ON, 1.0), (ub::O2_PITCH, 7.0), (ub::O1_VOL, -60.0), (ub::O2_VOL, -60.0), (ub::RM_ON, 1.0)], 0.5);
    let r = &rm[4800..];
    let (fa, fb) = (f0(), f0() * 2f32.powf(7.0 / 12.0));
    assert!(tone(r, fb - fa) > 3.0 * tone(r, fa) && tone(r, fb + fa) > 3.0 * tone(r, fb), "sum and difference only");
    // mutes if either oscillator is off, and has its own level
    let off = hit(&mut plain(0), 0, &[(ub::O2_ON, 0.0), (ub::O1_VOL, -60.0), (ub::RM_ON, 1.0)], 0.3);
    assert!(rms(&off) < 1e-6);
    let quiet = hit(&mut plain(0), 0, &[(ub::O2_ON, 1.0), (ub::O2_PITCH, 7.0), (ub::O1_VOL, -60.0), (ub::O2_VOL, -60.0), (ub::RM_ON, 1.0), (ub::RM_VOL, -12.0)], 0.5);
    assert!(rms(&quiet[4800..]) < 0.35 * rms(r));
    // side chain: heard only when a note triggers the sound
    let side = |note: bool| {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::O1_MODE, 2.0)]);
        if note {
            u.note_on(36, 100);
        }
        let mut out = Vec::new();
        for b in 0..40 {
            let sig: Vec<f32> = (0..256).map(|i| 0.5 * (std::f32::consts::TAU * 700.0 * (b * 256 + i) as f32 / RATE).sin()).collect();
            u.side_chain(&sig, &sig);
            let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
            u.render(&mut l, &mut r);
            out.extend(l);
        }
        out
    };
    let heard = side(true);
    assert!(tone(&heard[2000..], 700.0) > 0.05, "the side chain plays through the sound");
    assert!(rms(&side(false)) < 1e-6, "side chain alone triggers nothing");
}

#[test]
fn sample_and_model() {
    // sample mode plays the loaded audio; Pitch transposes it
    let sine: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / RATE).sin()).collect();
    let mut u = plain(0);
    u.load_sample(0, sine.clone(), RATE, "sine");
    assert_eq!(u.sample_name(0), Some("sine"));
    let x = hit(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::O2_ON, 1.0), (ub::O2_MODE, 1.0)], 0.4);
    assert!(tone(&x[2000..], 440.0) > 0.1);
    let mut u = plain(0);
    u.load_sample(0, sine.clone(), RATE, "sine");
    let y = hit(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::O2_ON, 1.0), (ub::O2_MODE, 1.0), (ub::O2_PITCH, 12.0)], 0.4);
    assert!(tone(&y[2000..], 880.0) > 0.1);
    // start point by velocity: a ramp sample starts later at high velocity when Max > Min
    let ramp: Vec<f32> = (0..48000).map(|i| i as f32 / 48000.0).collect();
    let first = |vel: u8| {
        let mut u = plain(0);
        u.load_sample(0, ramp.clone(), RATE, "ramp");
        setv(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::O2_ON, 1.0), (ub::O2_MODE, 1.0), (ub::S_MIN, 0.0), (ub::S_MAX, 0.8), (ub::VOICE_VOL, 6.0)]);
        u.note_on(36, vel);
        let x = render(&mut u, 0.02);
        x[300] / (0.35 + 0.65 * vel as f32 / 127.0)
    };
    assert!(first(127) > 3.0 * first(10) + 0.01, "{} {}", first(127), first(10));
    // reverse plays backward: the ramp falls
    let mut u = plain(0);
    u.load_sample(0, ramp.clone(), RATE, "ramp");
    setv(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::O2_ON, 1.0), (ub::O2_MODE, 1.0), (ub::S_REV, 1.0)]);
    u.note_on(36, 100);
    let x = render(&mut u, 0.05);
    assert!(x[200] > x[2000], "backward");
    // model: a string at C3 rings at its pitch; stiffness makes it inharmonic
    let model = |stiff: f32, loss: f32, res: f32| {
        let mut u = plain(0);
        hit(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::O2_ON, 1.0), (ub::O2_MODE, 2.0), (ub::M_STIFF, stiff), (ub::M_LOSS, loss), (ub::M_RES, res)], 0.8)
    };
    let m = model(0.0, 0.0, 0.6);
    assert!(tone(&m[2000..20000], f0()) > 0.02, "rings at C3");
    let ratio = |x: &[f32]| tone(x, 2.0 * f0()) / tone(x, 2.0 * f0() * 1.1).max(1e-6);
    assert!(ratio(&m[2000..20000]) > 3.0, "harmonic string has a clean second partial");
    let st = model(1.0, 0.0, 0.6);
    assert!(ratio(&st[2000..20000]) < 0.7 * ratio(&m[2000..20000]), "stiff strings are inharmonic");
    // inner loss mellows and shortens; resolution adds partials
    let lossy = model(0.0, 1.0, 0.6);
    assert!(hi_rms(&lossy[0..24000], 1500.0) < hi_rms(&m[0..24000], 1500.0));
    assert!(rms(&lossy[30000..38000]) < 0.5 * rms(&m[30000..38000]));
    assert!(hi_rms(&model(0.0, 0.0, 1.0)[0..24000], 1500.0) > 1.5 * hi_rms(&model(0.0, 0.0, 0.0)[0..24000], 1500.0));
    // the two exciters sound different
    let a = hit(&mut plain(0), 0, &[(ub::O1_ON, 0.0), (ub::O2_ON, 1.0), (ub::O2_MODE, 2.0), (ub::M_EXC, 1.0)], 0.5);
    assert!(differs(&a, &model(0.3, 0.3, 0.6)[..a.len()]) > 0.1, "{}", differs(&a, &model(0.3, 0.3, 0.6)[..a.len()]));
}

#[test]
fn noise_generator_and_flow() {
    let n = |s: &[(u32, f32)]| {
        let mut all = vec![(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0)];
        all.extend_from_slice(s);
        hit(&mut plain(0), 0, &all, 0.4)
    };
    let lp = n(&[(ub::NZ_TYPE, 0.0), (ub::NZ_CUT, 0.4)]);
    let hp = n(&[(ub::NZ_TYPE, 1.0), (ub::NZ_CUT, 0.7)]);
    assert!(bright(&lp, 3000.0) < 0.01 && bright(&hp, 3000.0) > 0.1, "{} {}", bright(&lp, 3000.0), bright(&hp, 3000.0));
    let byp = n(&[(ub::NZ_TYPE, 3.0)]);
    assert!(bright(&byp, 3000.0) > 0.1 && rms(&byp) > rms(&lp));
    // band-pass at a mid cutoff keeps neither the lows nor the highs
    let bp = n(&[(ub::NZ_TYPE, 2.0), (ub::NZ_CUT, 0.55), (ub::NZ_RES, 0.6)]);
    assert!(bright(&bp, 6000.0) < 0.1 && tone(&bp, 100.0) < tone(&bp, kit_cut(0.55)));
    // resonance up to self oscillation rings at the cutoff
    let ring = n(&[(ub::NZ_TYPE, 0.0), (ub::NZ_CUT, 0.5), (ub::NZ_RES, 1.0)]);
    assert!(rms(&ring) > 2.0 * rms(&n(&[(ub::NZ_TYPE, 0.0), (ub::NZ_CUT, 0.5)])));
    // dirt grainier: less high content than clean white noise at the same level, changes the sound
    let dirty = n(&[(ub::NZ_TYPE, 3.0), (ub::NZ_DIRT, 1.0)]);
    assert!(differs(&dirty, &byp) > 0.3 && bright(&dirty, 6000.0) < bright(&byp, 6000.0));
    // volume
    let quiet = n(&[(ub::NZ_TYPE, 3.0), (ub::NZ_VOL, -12.0)]);
    assert!((rms(&quiet) / rms(&byp) - 0.25).abs() < 0.05);
    // signal flow: through the filter (a low cutoff darkens it) or straight to the EQ
    let through = n(&[(ub::NZ_TYPE, 3.0), (ub::F_ON, 1.0), (ub::F_CUT, 0.3), (ub::NZ_FLOW, 1.0)]);
    let direct = n(&[(ub::NZ_TYPE, 3.0), (ub::F_ON, 1.0), (ub::F_CUT, 0.3), (ub::NZ_FLOW, 0.0)]);
    assert!(bright(&through, 3000.0) < 0.01 && bright(&direct, 3000.0) > 0.1, "{} {}", bright(&through, 3000.0), bright(&direct, 3000.0));
}

fn kit_cut(c: f32) -> f32 {
    lg_synth::synthkit::cutoff_hz(c)
}

#[test]
fn filter_and_distortion_circuit() {
    let src = |s: &[(u32, f32)]| {
        let mut all = vec![(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0), (ub::NZ_TYPE, 3.0), (ub::F_ON, 1.0)];
        all.extend_from_slice(s);
        hit(&mut plain(0), 0, &all, 0.4)
    };
    let off = {
        let mut all = vec![(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0), (ub::NZ_TYPE, 3.0), (ub::F_ON, 0.0), (ub::F_CUT, 0.2)];
        all.push((ub::F_TYPE, 0.0));
        hit(&mut plain(0), 0, &all, 0.4)
    };
    let lp = src(&[(ub::F_TYPE, 0.0), (ub::F_CUT, 0.4)]);
    let hp = src(&[(ub::F_TYPE, 1.0), (ub::F_CUT, 0.7)]);
    assert!(bright(&off, 3000.0) > 0.1, "the Filter button off passes everything {}", bright(&off, 3000.0));
    assert!(bright(&lp, 3000.0) < 0.01 && bright(&hp, 3000.0) > 0.08);
    // 24 dB is steeper than 12 dB
    let s12 = src(&[(ub::F_TYPE, 0.0), (ub::F_CUT, 0.45), (ub::F_SLOPE, 0.0)]);
    let s24 = src(&[(ub::F_TYPE, 0.0), (ub::F_CUT, 0.45), (ub::F_SLOPE, 1.0)]);
    assert!(hi_rms(&s24, 6000.0) < 0.5 * hi_rms(&s12, 6000.0));
    // band-pass and band-reject
    let bp = src(&[(ub::F_TYPE, 2.0), (ub::F_CUT, 0.5), (ub::F_RES, 0.7)]);
    let br = src(&[(ub::F_TYPE, 3.0), (ub::F_CUT, 0.5), (ub::F_RES, 0.7)]);
    assert!(tone(&bp, kit_cut(0.5)) > 2.0 * tone(&br, kit_cut(0.5)), "BR rejects the band BP passes");
    // resonance: a peak at the cutoff
    let flat = src(&[(ub::F_TYPE, 0.0), (ub::F_CUT, 0.5), (ub::F_RES, 0.0)]);
    let res = src(&[(ub::F_TYPE, 0.0), (ub::F_CUT, 0.5), (ub::F_RES, 0.9)]);
    assert!(tone(&res, kit_cut(0.5)) > 2.0 * tone(&flat, kit_cut(0.5)));
    // distortion: distort adds harmonics to a sine; the drive sets the amount
    let sine = |s: &[(u32, f32)]| {
        let mut all = vec![(ub::F_ON, 1.0), (ub::F_CUT, 1.0), (ub::F_TYPE, 0.0), (ub::O1_VOL, -12.0)];
        all.extend_from_slice(s);
        let x = hit(&mut plain(0), 0, &all, 0.5);
        x[4800..].to_vec()
    };
    let clean = sine(&[]);
    let d1 = sine(&[(ub::D_MODE, 2.0), (ub::D_DRIVE, 0.3), (ub::D_COLOR, 1.0)]);
    let d2 = sine(&[(ub::D_MODE, 2.0), (ub::D_DRIVE, 1.0), (ub::D_COLOR, 1.0)]);
    assert!(tone(&clean, 3.0 * f0()) < 0.005);
    assert!(tone(&d1, 3.0 * f0()) > 0.02 && tone(&d2, 3.0 * f0()) > tone(&d1, 3.0 * f0()));
    // colour: higher is brighter
    let dark = sine(&[(ub::D_MODE, 2.0), (ub::D_DRIVE, 1.0), (ub::D_COLOR, 0.0)]);
    assert!(bright(&dark, 1500.0) < bright(&d2, 1500.0));
    // crush: aliasing noise at high drive; neither mode active leaves the signal alone
    let crush = sine(&[(ub::D_MODE, 1.0), (ub::D_DRIVE, 1.0), (ub::D_LEVEL, 0.0)]);
    assert!(bright(&crush, 3000.0) > 5.0 * bright(&clean, 3000.0) + 1e-5);
    assert!(differs(&clean, &sine(&[(ub::D_MODE, 0.0), (ub::D_DRIVE, 1.0)])) < 1e-4);
    // Level: input threshold in Crush mode
    let thr = sine(&[(ub::D_MODE, 1.0), (ub::D_DRIVE, 0.2), (ub::D_LEVEL, 1.0)]);
    assert!(rms(&thr) < 0.3 * rms(&clean));
    // the arrow: distortion before the filter is filtered after, so it is less bright
    let before = sine(&[(ub::D_MODE, 2.0), (ub::D_DRIVE, 1.0), (ub::D_COLOR, 1.0), (ub::F_CUT, 0.45), (ub::D_POS, 1.0)]);
    let after = sine(&[(ub::D_MODE, 2.0), (ub::D_DRIVE, 1.0), (ub::D_COLOR, 1.0), (ub::F_CUT, 0.45), (ub::D_POS, 0.0)]);
    assert!(bright(&before, 1200.0) < bright(&after, 1200.0));
}

#[test]
fn eq_pan_spread_and_mixer() {
    let n = |s: &[(u32, f32)]| {
        let mut all = vec![(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0), (ub::NZ_TYPE, 3.0)];
        all.extend_from_slice(s);
        hit(&mut plain(0), 0, &all, 0.3)
    };
    let flat = n(&[]);
    let hi = n(&[(ub::EQ2_ON, 1.0), (ub::EQ2_GAIN, 12.0)]);
    let lo = n(&[(ub::EQ1_ON, 1.0), (ub::EQ1_GAIN, 12.0)]);
    assert!(hi_rms(&hi, 4000.0) > 2.0 * hi_rms(&flat, 4000.0), "band 2 is a high shelf");
    assert!(rms(&lowpass(&lo)) > 2.0 * rms(&lowpass(&flat)), "band 1 is a low shelf");
    assert!(differs(&flat, &n(&[(ub::EQ1_ON, 1.0), (ub::EQ1_GAIN, 0.0), (ub::EQ2_ON, 1.0)])) < 1e-3, "gain 0 has no effect");
    assert!(differs(&flat, &n(&[(ub::EQ1_ON, 0.0), (ub::EQ1_GAIN, 12.0)])) < 1e-3, "band off: unaffected");
    // peak mode with Q: narrow vs broad
    let pk = n(&[(ub::EQ2_ON, 1.0), (ub::EQ2_TYPE, 1.0), (ub::EQ2_FREQ, 3000.0), (ub::EQ2_GAIN, 15.0), (ub::EQ2_Q, 8.0)]);
    assert!(tone(&pk[4800..], 3000.0) > 2.0 * tone(&flat[4800..], 3000.0) && hi_rms(&pk, 8000.0) < 1.5 * hi_rms(&flat, 8000.0));
    // mixer pan
    let (l, r) = {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::MIX_PAN, -1.0)]);
        u.note_on(36, 100);
        render_st(&mut u, 0.2)
    };
    assert!(rms(&l) > 20.0 * rms(&r));
    // volume, mute, solo, master
    let base = rms(&hit(&mut plain(0), 0, &[], 0.2));
    assert!((rms(&hit(&mut plain(0), 0, &[(ub::MIX_VOL, -6.0)], 0.2)) / base - 0.5).abs() < 0.03);
    assert!(rms(&hit(&mut plain(0), 0, &[(ub::MUTE, 1.0)], 0.2)) < 1e-9);
    let mut u = plain(0);
    u.set(ub::G_MASTER, -6.0);
    assert!((rms(&hit(&mut u, 0, &[], 0.2)) / base - 0.5).abs() < 0.03);
    // voice volume sits before the mixer: -6 dB too
    assert!((rms(&hit(&mut plain(0), 0, &[(ub::VOICE_VOL, -6.0)], 0.2)) / base - 0.5).abs() < 0.03);
    // pan modulation moves the sound relative to the mixer pan; spread widens it
    let (l, r) = {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::PAN_MODE, 1.0), (ub::MIX_PAN, 0.0), (ub::mod_k(ub::T_PAN, 0), 7.0), (ub::mod_k(ub::T_PAN, 1), 1.0)]);
        u.note_on(36, 100);
        render_st(&mut u, 0.2)
    };
    assert!(rms(&r) > 20.0 * rms(&l), "pan modulation at Max");
    let side = |mode: f32| {
        let (l, r) = {
            let mut u = plain(0);
            setv(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0), (ub::NZ_TYPE, 3.0), (ub::PAN_MODE, mode), (ub::SPREAD_LO, 1.0), (ub::SPREAD_HI, 1.0)]);
            u.note_on(36, 100);
            render_st(&mut u, 0.3)
        };
        let d: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
        rms(&d) / rms(&l)
    };
    assert!(side(0.0) < 0.01 && side(2.0) > 0.3, "spread decorrelates");
}

fn lowpass(x: &[f32]) -> Vec<f32> {
    let mut z = 0.0;
    x.iter().map(|v| { z += (v - z) * 0.05; z }).collect()
}

#[test]
fn envelopes_and_voice_volume() {
    // Env 4 is wired to the voice volume: its decay time sets the length
    let len = |d: f32| {
        let x = hit(&mut plain(0), 0, &[(ub::ENV0 + 26, d)], 3.0);
        let thr = 0.05 * x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        x.iter().rposition(|v| v.abs() > thr).unwrap_or(0) as f32 / RATE
    };
    assert!(len(0.7) > 2.0 * len(0.5));
    // attack: a slow attack delays the peak
    let peak = |a: f32| {
        let x = hit(&mut plain(0), 0, &[(ub::ENV0 + 24, a)], 1.0);
        let m = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        x.iter().position(|v| v.abs() > 0.9 * m).unwrap_or(0) as f32 / RATE
    };
    assert!(peak(0.0) < 0.01 && peak(0.6) > 0.1);
    // one-shot ignores the note length; sustain follows it
    let held = |sus: f32, off_at: f32| {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::ENV0 + 28, sus), (ub::ENV0 + 29, 0.3), (ub::ENV0 + 26, 0.75)]);
        u.note_on(36, 100);
        let mut x = render(&mut u, off_at);
        u.note_off(36);
        x.extend(render(&mut u, 0.6));
        x
    };
    let one = held(0.0, 0.1);
    let one_long = held(0.0, 0.5);
    let n = (0.1 * RATE) as usize;
    assert!((rms(&one[n + 4800..n + 9600]) / rms(&one_long[n + 4800..n + 9600]) - 1.0).abs() < 0.1, "one-shot: the same either way");
    let s = held(1.0, 0.4);
    let a = rms(&s[(0.3 * RATE) as usize..(0.38 * RATE) as usize]);
    let b = rms(&s[(0.8 * RATE) as usize..(0.95 * RATE) as usize]);
        assert!(a > 0.0 && b < a, "sustain holds then lets go at the note release: {a} {b}");
    // velocity on the decay time: high velocity shortens the segment
    let dur = |vel: u8| {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::ENV0 + 30, 2.0), (ub::ENV0 + 31, 1.0), (ub::ENV0 + 26, 0.6)]);
        u.note_on(36, vel);
        let x = render(&mut u, 2.0);
        let m = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        x.iter().rposition(|v| v.abs() > 0.05 * m).unwrap_or(0) as f32 / RATE
    };
    assert!(dur(127) < 0.6 * dur(10));
    // an envelope on a parameter: Env 1 sweeps the pitch down
    let sweep = hit(&mut plain(0), 0, &[(ub::mod_k(ub::T_O1_PITCH, 0), 3.0), (ub::mod_k(ub::T_O1_PITCH, 1), 0.25), (ub::ENV0 + 2, 0.55)], 0.6);
    let (a, b) = (fundamental(&sweep[200..2600], 150.0, 1500.0), fundamental(&sweep[24000..28000], 150.0, 1500.0));
    assert!(a > 1.5 * b, "{a} {b}");
    // negative amounts go below the base value
    let neg = hit(&mut plain(0), 0, &[(ub::mod_k(ub::T_NZ_CUT, 0), 3.0), (ub::NZ_ON, 1.0), (ub::O1_ON, 0.0), (ub::NZ_TYPE, 0.0), (ub::NZ_CUT, 0.8), (ub::mod_k(ub::T_NZ_CUT, 1), -0.6), (ub::ENV0 + 2, 0.6)], 0.5);
    assert!(bright(&neg[0..2400], 2000.0) < 0.5 * bright(&neg[14000..18000], 2000.0), "a negative amount drives the cutoff below its base value at first");
}

#[test]
fn lfo_mod_and_via() {
    use lg_synth::ultrabeat_dsp::{lfo_hz, lfo_shape};
    assert!((lfo_shape(0.25, 0.58, 0.0) - 1.0).abs() < 1e-4 && (lfo_shape(0.75, 0.58, 0.0) + 1.0).abs() < 1e-4, "0.58 is a square");
    assert!((lfo_shape(0.3, 1.0, 0.42) - 0.42).abs() < 1e-5, "far right: sample and hold");
    assert!((lfo_hz(false, 1.0, 120.0) - 100.0).abs() < 0.01, "up to 100 Hz");
    let list = ub::sync_list();
    assert_eq!(list.first().unwrap().0, "32 bars");
    assert!(list.iter().any(|(n, _)| n == "1/64") && list.iter().any(|(n, _)| n == "1/8 t") && list.iter().any(|(n, _)| n == "1/16 d"));
    let q = list.iter().position(|(n, _)| n == "1/4").unwrap();
    assert!((lfo_hz(true, q as f32 / (list.len() - 1) as f32, 120.0) - 2.0).abs() < 1e-3);
    // an LFO on the pitch: a square at 4 Hz jumps between two pitches
    let sq = |cycles: f32| {
        hit(&mut plain(0), 0, &[(ub::LFO0 + 2, 0.65), (ub::LFO0 + 3, 0.58), (ub::LFO0 + 4, cycles), (ub::mod_k(ub::T_O1_PITCH, 0), 1.0), (ub::mod_k(ub::T_O1_PITCH, 1), 0.125)], 0.8)
    };
    let x = sq(100.0);
    let (a, b) = (fundamental(&x[2400..9600], 150.0, 1200.0), fundamental(&x[(0.15 * RATE) as usize..(0.23 * RATE) as usize], 80.0, 1200.0));
    assert!((a / b - 4.0).abs() < 0.3 || (b / a - 4.0).abs() < 0.3, "{a} {b}: +12 then -12 semitones");
    // cycles below the maximum: it stops after that many cycles
    let y = sq(1.0);
    let c = fundamental(&y[(0.5 * RATE) as usize..(0.7 * RATE) as usize], 150.0, 1200.0);
    assert!((c / f0() - 1.0).abs() < 0.03, "after one cycle the base pitch: {c}");
    // ramp: fade-in of the modulation
    // Mod Off: no modulation at all
    let off = hit(&mut plain(0), 0, &[(ub::mod_k(ub::T_O1_PITCH, 0), 0.0), (ub::mod_k(ub::T_O1_PITCH, 1), 1.0)], 0.3);
    assert!((fundamental(&off[2400..], 150.0, 600.0) / f0() - 1.0).abs() < 0.02);
    // via: Max with a controller as via: depth runs from the mod amount to the via amount
    let filt = |cc: u8| {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0), (ub::NZ_TYPE, 3.0), (ub::F_ON, 1.0), (ub::F_CUT, 0.3), (ub::mod_k(ub::T_F_CUT, 0), 7.0), (ub::mod_k(ub::T_F_CUT, 1), 0.0), (ub::mod_k(ub::T_F_CUT, 2), 2.0), (ub::mod_k(ub::T_F_CUT, 3), 0.6)]);
        u.controller(1, cc);
        u.note_on(36, 100);
        bright(&render(&mut u, 0.3), 3000.0)
    };
    assert!(filt(127) > 3.0 * filt(0) + 1e-3, "{} {}", filt(127), filt(0));
    // via velocity
    let vel = |v: u8| {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0), (ub::NZ_TYPE, 3.0), (ub::F_ON, 1.0), (ub::F_CUT, 0.3), (ub::mod_k(ub::T_F_CUT, 0), 7.0), (ub::mod_k(ub::T_F_CUT, 1), 0.0), (ub::mod_k(ub::T_F_CUT, 2), 1.0), (ub::mod_k(ub::T_F_CUT, 3), 0.6)]);
        u.note_on(36, v);
        bright(&render(&mut u, 0.3), 3000.0)
    };
    assert!(vel(127) > 2.0 * vel(5));
    // Ctrl learn: the next controller within the window is taken
    let mut u = Ultrabeat::new(RATE);
    u.start_learn(1);
    u.controller(80, 64);
    assert_eq!(u.get(ub::G_CTRL + 1), 80.0);
    assert!(u.learning().is_none());
    u.controller(80, 127);
    assert!((u.get(ub::G_CV + 1) - 1.0).abs() < 1e-6);
    u.start_learn(2);
    u.cancel_learn();
    u.controller(81, 10);
    assert_ne!(u.get(ub::G_CTRL + 2), 81.0);
}

#[test]
fn trigger_modes_groups_and_gate() {
    let noise = |s: &[(u32, f32)]| {
        let mut v = vec![(ub::O1_ON, 0.0), (ub::NZ_ON, 1.0), (ub::NZ_TYPE, 3.0), (ub::ENV0 + 26, 0.75)];
        v.extend_from_slice(s);
        v
    };
    // Multi: the earlier hit keeps decaying; Single: a new hit cuts it
    let run = |trig: f32| {
        let mut u = plain(0);
        setv(&mut u, 0, &noise(&[(ub::TRIG, trig)]));
        u.note_on(36, 100);
        let mut x = render(&mut u, 0.1);
        u.note_on(36, 100);
        x.extend(render(&mut u, 0.4));
        rms(&x[(0.12 * RATE) as usize..(0.4 * RATE) as usize])
    };
    assert!(run(0.0) > 1.2 * run(1.0));
    // Group: sounds in the same group cut each other
    let g = |same: bool| {
        let mut u = plain(0);
        setv(&mut u, 0, &noise(&[(ub::GROUP, 1.0)]));
        setv(&mut u, 1, &[(ub::O1_ON, 0.0), (ub::GROUP, if same { 1.0 } else { 2.0 }), (ub::MUTE, 1.0)]);
        u.note_on(36, 100);
        let mut x = render(&mut u, 0.05);
        u.note_on(37, 100);
        x.extend(render(&mut u, 0.3));
        rms(&x[(0.1 * RATE) as usize..(0.3 * RATE) as usize])
    };
    assert!(g(false) > 10.0 * g(true) + 1e-6, "{} {}", g(false), g(true));
    // Gate: the sound is cut on release whatever the envelopes
    let gate = |on: f32| {
        let mut u = plain(0);
        setv(&mut u, 0, &[(ub::GATE, on)]);
        u.note_on(36, 100);
        let mut x = render(&mut u, 0.1);
        u.note_off(36);
        x.extend(render(&mut u, 0.3));
        rms(&x[(0.2 * RATE) as usize..(0.4 * RATE) as usize])
    };
    assert!(gate(0.0) > 0.01 && gate(1.0) < 1e-4);
}

fn beats_run(u: &mut Ultrabeat, v: usize, bpm: f64, beats: f64, start: f64) -> Vec<f64> {
    // the times (in beats) at which sound `v` is hit while the host plays
    let blk = 64usize;
    let spb = RATE as f64 * 60.0 / bpm;
    let n = (beats * spb / blk as f64) as usize;
    let mut prev = 0.0f32;
    let mut out = Vec::new();
    for b in 0..n {
        let pos = start + (b * blk) as f64 / spb;
        u.set_transport(Transport { beats: pos, bpm, playing: true });
        let (mut l, mut r) = (vec![0.0; blk], vec![0.0; blk]);
        u.render(&mut l, &mut r);
        let h = u.hit_level(v);
        if h > prev + 0.5 {
            out.push(pos);
        }
        prev = h;
    }
    out
}

fn short(u: &mut Ultrabeat) {
    u.set(vid(0, ub::ENV0 + 24 + 2), 0.6);
}

fn setcell(u: &mut Ultrabeat, p: usize, v: usize, f: impl FnOnce(&mut Cell)) {
    let mut c = u.cell(p, v).clone();
    f(&mut c);
    for (id, x) in u.cell_pairs(p, v, &c) {
        u.set(id, x);
    }
}

#[test]
fn step_sequencer_follows_the_host() {
    let mut u = plain(0);
    setcell(&mut u, 0, 0, |c| {
        c.set_trig(0, true);
        c.set_trig(4, true);
        c.set_trig(8, true);
    });
    // at 1/16 a step is a quarter beat: steps 0, 4, 8 are beats 0, 1, 2; 16 steps = 4 beats
    let h = beats_run(&mut u, 0, 120.0, 8.0, 0.0);
    let want = [0.0, 1.0, 2.0, 4.0, 5.0, 6.0];
    assert_eq!(h.len(), want.len(), "{h:?}");
    for (a, b) in h.iter().zip(want) {
        assert!((a - b).abs() < 0.03, "{h:?}");
    }
    // when the host is stopped nothing plays; with the sequencer off neither
    let mut u2 = plain(0);
    setcell(&mut u2, 0, 0, |c| c.set_trig(0, true));
    u2.set_transport(Transport { beats: 0.0, bpm: 120.0, playing: false });
    assert!(rms(&render(&mut u2, 0.3)) < 1e-9);
    u2.set(ub::G_SEQ_ON, 0.0);
    assert!(beats_run(&mut u2, 0, 120.0, 2.0, 0.0).is_empty());
    // length: 6 steps loop every 1.5 beats
    let mut u3 = plain(0);
    setcell(&mut u3, 0, 0, |c| c.set_trig(0, true));
    u3.set(ub::pat_len_id(0), 6.0);
    let h = beats_run(&mut u3, 0, 120.0, 4.0, 0.0);
    assert_eq!(h.len(), 3, "{h:?}");
    assert!((h[1] - 1.5).abs() < 0.03);
    // resolution: 1/8 steps are twice as long
    let mut u4 = plain(0);
    setcell(&mut u4, 0, 0, |c| c.set_trig(1, true));
    u4.set(ub::pat_res_id(0), 1.0);
    let h = beats_run(&mut u4, 0, 120.0, 2.0, 0.0);
    assert!((h[0] - 0.5).abs() < 0.03, "{h:?}");
    // another pattern holds another sequence
    let mut u5 = plain(0);
    setcell(&mut u5, 1, 0, |c| c.set_trig(2, true));
    assert!(beats_run(&mut u5, 0, 120.0, 2.0, 0.0).is_empty());
    u5.set(ub::G_PAT, 1.0);
    let h = beats_run(&mut u5, 0, 120.0, 2.0, 0.0);
    assert!((h[0] - 0.5).abs() < 0.03);
    // swing moves only the even steps (and only for sounds with Swing on, at 1/8 or 1/16)
    let mut u6 = plain(0);
    setcell(&mut u6, 0, 0, |c| {
        c.set_trig(0, true);
        c.set_trig(1, true);
    });
    u6.set(ub::G_SWING, 1.0);
    let h0 = beats_run(&mut u6, 0, 120.0, 1.0, 0.0);
    assert!((h0[1] - 0.25).abs() < 0.03, "swing enable is off for the sound");
    let mut u7 = plain(0);
    setcell(&mut u7, 0, 0, |c| {
        c.set_trig(0, true);
        c.set_trig(1, true);
    });
    u7.set(ub::G_SWING, 1.0);
    u7.set(vid(0, ub::SWING_EN), 1.0);
    let h1 = beats_run(&mut u7, 0, 120.0, 1.0, 0.0);
    assert!((h1[0]).abs() < 0.03 && h1[1] > 0.3, "{h1:?}");
    u7.set(ub::pat_res_id(0), 0.0);
    let h2 = beats_run(&mut u7, 0, 120.0, 2.0, 1.0);
    let _ = h2;
    // velocity, gate and accent of a step
    let mut u8 = plain(0);
    short(&mut u8);
    setcell(&mut u8, 0, 0, |c| {
        c.set_trig(0, true);
        c.set_vel(0, 1.0);
        c.set_trig(8, true);
        c.set_vel(8, 0.2);
    });
    let peak = |u: &mut Ultrabeat, at: f64| {
        let spb = RATE as f64 * 60.0 / 120.0;
        let mut m = 0.0f32;
        for b in 0..((0.4 * spb) / 64.0) as usize {
            u.set_transport(Transport { beats: at + (b * 64) as f64 / spb, bpm: 120.0, playing: true });
            let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
            u.render(&mut l, &mut r);
            m = m.max(rms(&l));
        }
        m
    };
    let (a, b) = (peak(&mut u8, 0.0), peak(&mut u8, 2.0));
    assert!(a > 1.5 * b, "{a} {b}");
    let mut u9 = plain(0);
    short(&mut u9);
    setcell(&mut u9, 0, 0, |c| {
        c.set_trig(0, true);
        c.set_accent(0, true);
        c.set_trig(8, true);
    });
    u9.set(ub::G_ACCENT, 1.0);
    let (a, b) = (peak(&mut u9, 0.0), peak(&mut u9, 2.0));
    assert!(a > 1.4 * b, "accent: louder {a} {b}");
    u9.set(vid(0, ub::ACC_EN), 0.0);
    let (a, b) = (peak(&mut u9, 0.0), peak(&mut u9, 2.0));
    assert!((a / b - 1.0).abs() < 0.05, "accent off for the sound");
}

#[test]
fn step_automation_offsets() {
    let mut u = plain(0);
    short(&mut u);
    setcell(&mut u, 0, 0, |c| {
        c.set_trig(0, true);
        c.set_trig(8, true);
        let l = c.lane_for(ub::O1_PITCH).unwrap();
        c.offs[l][8] = 4; // about half the pitch range: +24 semitones... scaled 15/31 of 96
    });
    let f = |at: f64, u: &mut Ultrabeat, lo: f32, hi: f32| {
        let spb = RATE as f64 * 60.0 / 120.0;
        let mut x = Vec::new();
        for b in 0..((0.5 * spb) / 64.0) as usize {
            u.set_transport(Transport { beats: at + (b * 64) as f64 / spb, bpm: 120.0, playing: true });
            let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
            u.render(&mut l, &mut r);
            x.extend(l);
        }
        fundamental(&x[1500..9000], lo, hi)
    };
    let (a, b) = (f(0.0, &mut u, 200.0, 330.0), f(2.0, &mut u, 400.0, 700.0));
    let semis = 12.0 * (b / a).log2();
    assert!((semis - 4.0 / 31.0 * 96.0).abs() < 1.5, "offset applied: {semis} semitones {a} {b}");
    // lane muted: no offset
    setcell(&mut u, 0, 0, |c| c.flags = 1);
    let (a, b) = (f(0.0, &mut u, 200.0, 330.0), f(2.0, &mut u, 200.0, 330.0));
    assert!((b / a - 1.0).abs() < 0.02);
    // only four lanes, and menus / buttons / pan can't be automated
    assert!(ub::automatable(ub::O1_PITCH) && ub::automatable(ub::F_CUT));
    assert!(!ub::automatable(ub::O1_MODE) && !ub::automatable(ub::O1_ON) && !ub::automatable(ub::MIX_PAN) && !ub::automatable(ub::SPREAD_LO));
    let mut c = Cell::default();
    for k in [ub::O1_PITCH, ub::O2_PITCH, ub::F_CUT, ub::F_RES] {
        assert!(c.lane_for(k).is_some());
    }
    assert!(c.lane_for(ub::NZ_CUT).is_none());
    c.set_trig(3, true);
    let mut rng = Rng::new(3);
    c.alter_lane(0, 16, &mut rng);
    c.randomize_lane(1, 16, &mut rng);
    assert!(c.offs[1].iter().any(|&o| o != 0));
    c.zero_lane(1);
    assert!(c.offs[1].iter().all(|&o| o == 0));
    c.delete_lane(1);
    assert_eq!(c.lane[1], 0);
}

#[test]
fn trigger_shortcut_menu_and_cells() {
    let mut c = Cell::default();
    c.add_every(32, 4, 0);
    assert_eq!(c.count(32), 8, "downbeats 1, 5, 9…");
    assert!(c.trig(0) && c.trig(4) && !c.trig(2));
    c.add_every(32, 4, 2);
    assert_eq!(c.count(32), 16, "upbeats added, existing kept");
    c.clear_triggers();
    c.set_trig(1, true);
    c.set_trig(2, true);
    c.shift(16, 1);
    assert!(c.trig(2) && c.trig(3) && !c.trig(1));
    c.shift(16, -2);
    assert!(c.trig(0) && c.trig(1));
    c.reverse(4);
    assert!(c.trig(2) && c.trig(3));
    let mut rng = Rng::new(9);
    c.alter_randomly(16, &mut rng);
    assert_eq!(c.count(16), 2, "alter keeps the number of triggers");
    c.create_randomly(32, 8, &mut rng);
    assert_eq!(c.count(32), 8);
    // velocity, gate: defaults are 75 percent with all four sections
    let d = Cell::default();
    assert!((d.vel(0) - 0.766).abs() < 0.02 && d.gate(0) == 0xf && d.gate_len(0) == 1.0);
    let mut e = Cell::default();
    e.set_vel(3, 0.5);
    e.set_gate(3, 0b0011);
    e.set_accent(3, true);
    assert!((e.vel(3) - 0.5).abs() < 0.02 && e.gate_len(3) == 0.5 && e.accent(3));
    e.randomize_vel(32, &mut rng);
    e.randomize_gate(32, &mut rng);
    e.alter_vel(32, &mut rng);
    e.alter_gate(32, &mut rng);
    e.reset_vel_gate();
    assert!((e.vel(3) - 0.766).abs() < 0.02 && e.gate(3) == 0xf && e.accent(3), "Reset keeps accents");
    // the words round-trip, an empty cell is all zeros
    assert!(Cell::default().words().iter().all(|w| *w == 0.0));
    let mut f = Cell::default();
    f.set_trig(5, true);
    f.set_vel(5, 0.9);
    f.set_gate(31, 0b0101);
    let l = f.lane_for(ub::F_CUT).unwrap();
    f.offs[l][7] = -20;
    f.offs[l][30] = 31;
    f.flags = 0x12;
    assert_eq!(Cell::from_words(&f.words()), f);
}

#[test]
fn export_and_save_roundtrip() {
    let mut u = plain(0);
    setcell(&mut u, 2, 0, |c| {
        c.set_trig(0, true);
        c.set_trig(5, true);
        c.set_vel(5, 0.5);
        c.set_gate(5, 0b0011);
        c.set_accent(5, true);
    });
    setcell(&mut u, 2, 4, |c| c.set_trig(7, true));
    u.set(ub::pat_len_id(2), 8.0);
    let notes = u.pattern_notes(2);
    assert_eq!(notes.len(), 3);
    assert_eq!((notes[0].1, notes[0].0), (36, 0.0));
    assert!((notes[1].0 - 1.25).abs() < 1e-9 && notes[1].1 == 36 && (notes[1].3 - 0.125).abs() < 1e-9, "{notes:?}");
    assert!(notes[1].2 > 64, "an accent raises the velocity");
    assert_eq!(notes[2].1, 40);
    assert!(u.pattern_notes(0).is_empty());
    // every declared parameter of the cells is saved and loads into a new instance
    let saved: Vec<(u32, f32)> = u.params().iter().map(|p| (p.id, u.get(p.id))).collect();
    assert!(saved.iter().any(|(id, _)| *id >= ub::SEQ_BASE && *id < ub::PAT_BASE));
    let mut v = Ultrabeat::new(RATE);
    for (id, x) in &saved {
        v.set(*id, *x);
    }
    assert_eq!(v.pattern_notes(2), notes);
    // names
    for (id, x) in Ultrabeat::name_pairs(3, "Fat Kick 9") {
        u.set(id, x);
    }
    assert_eq!(u.voice_name(3), "Fat Kick 9");
    let mut w = Ultrabeat::new(RATE);
    for p in u.params().to_vec() {
        w.set(p.id, u.get(p.id));
    }
    assert_eq!(w.voice_name(3), "Fat Kick 9");
    // copy a sound into another: paste voice keeps the mixer
    let mut a = Ultrabeat::new(RATE);
    a.set(vid(1, ub::MIX_VOL), -9.0);
    let src = a.voice_params(0);
    for (id, x) in Ultrabeat::assign_pairs(1, &src, true) {
        a.set(id, x);
    }
    assert_eq!(a.vparam(1, ub::O1_PITCH), a.vparam(0, ub::O1_PITCH));
    assert_eq!(a.vparam(1, ub::MIX_VOL), -9.0);
    // Init
    let pairs = a.init_pairs(1, Init::Standard);
    for (id, x) in pairs {
        a.set(id, x);
    }
    assert_eq!(a.vparam(1, ub::O1_PITCH), 0.0);
}

#[test]
fn midi_controls_the_patterns() {
    let hits = |u: &mut Ultrabeat, from: f64, beats: f64| beats_run(u, 0, 120.0, beats, from);
    let mk = || {
        let mut u = plain(0);
        // pattern 3 (note 2): a hit on step 0 of a 4-step loop (one beat)
        setcell(&mut u, 2, 0, |c| c.set_trig(0, true));
        u.set(ub::pat_len_id(2), 4.0);
        u.set(ub::G_PMODE, 1.0);
        u
    };
    // pattern mode: nothing plays until a note picks a pattern (C-1 … B0 = patterns 1…24)
    let mut u = mk();
    assert!(hits(&mut u, 0.0, 2.0).is_empty());
    // One-Shot Trig: plays once, note-offs ignored
    let mut u = mk();
    u.set(ub::G_PBMODE, 0.0);
    u.set_transport(Transport { beats: 0.0, bpm: 120.0, playing: true });
    u.note_on(2, 100);
    u.note_off(2);
    let h = hits(&mut u, 0.0, 4.0);
    assert_eq!(h.len(), 1, "{h:?}");
    // Sustain: loops until the note is released
    let mut u = mk();
    u.set(ub::G_PBMODE, 1.0);
    u.set_transport(Transport { beats: 0.0, bpm: 120.0, playing: true });
    u.note_on(2, 100);
    let h = hits(&mut u, 0.0, 2.9);
    assert_eq!(h.len(), 3, "{h:?}");
    u.note_off(2);
    assert!(hits(&mut u, 3.0, 3.0).is_empty());
    // Toggle: the same note stops it
    let mut u = mk();
    u.set(ub::G_PBMODE, 2.0);
    u.set_transport(Transport { beats: 0.0, bpm: 120.0, playing: true });
    u.note_on(2, 100);
    u.note_off(2);
    assert_eq!(hits(&mut u, 0.0, 2.9).len(), 3);
    u.note_on(2, 100);
    assert!(hits(&mut u, 3.0, 3.0).is_empty());
    // Voice Mute Mode: notes C1 and up toggle the mute of the sound
    let mut u = plain(0);
    u.set(ub::G_MUTEMODE, 1.0);
    u.note_on(36, 100);
    assert_eq!(u.vparam(0, ub::MUTE), 1.0);
    assert!(rms(&render(&mut u, 0.1)) < 1e-9);
    u.note_on(36, 100);
    assert_eq!(u.vparam(0, ub::MUTE), 0.0);
    // Voice Auto Select
    let mut u = plain(0);
    u.set(ub::G_AUTOSEL, 1.0);
    u.note_on(40, 100);
    assert_eq!(u.selected(), 4);
}

#[test]
fn light_on_incoming_notes() {
    let mut u = plain(0);
    assert!(u.hit_level(3) < 0.01);
    u.note_on(39, 100);
    let _ = render(&mut u, 0.01);
    assert!(u.hit_level(3) > 0.5 && u.hit_level(4) < 0.01);
    for _ in 0..30 {
        let _ = render(&mut u, 0.01);
    }
    assert!(u.hit_level(3) < 0.5, "the light fades");
}
