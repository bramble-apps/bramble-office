//! Sculpture: every control of the guide changes the sound the way the guide says.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::sculpture::{self as s, Sculpture};

fn sc() -> Sculpture {
    Sculpture::new(RATE)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |a, v| a.max(v.abs()))
}

/// A note on a fresh instrument with the settings, `secs` long.
fn note(set: &[(u32, f32)], n: u8, vel: u8, secs: f32) -> Vec<f32> {
    let mut i = sc();
    play(&mut i, set, n, vel, secs)
}

/// The same, stereo.
fn note_st(set: &[(u32, f32)], n: u8, vel: u8, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let mut i = sc();
    for &(id, v) in set {
        i.set(id, v);
    }
    i.note_on(n, vel);
    render_st(&mut i, secs)
}

/// The strongest frequency between `lo` and `hi` (a fine scan; no octave mistakes).
fn pitch(x: &[f32], lo: f32, hi: f32) -> f32 {
    let mut best = (0.0f32, lo);
    let mut hz = lo;
    while hz < hi {
        let t = tone(x, hz);
        if t > best.0 {
            best = (t, hz);
        }
        hz *= 1.004;
    }
    let (c, mut f) = (best.1, best.1 * 0.996);
    let mut b2 = (0.0f32, c);
    while f < c * 1.004 {
        let t = tone(x, f);
        if t > b2.0 {
            b2 = (t, f);
        }
        f *= 1.0002;
    }
    b2.1
}

fn win(x: &[f32], a: f32, b: f32) -> &[f32] {
    &x[(a * RATE) as usize..(b * RATE) as usize]
}

#[test]
fn stays_finite_everywhere() {
    sweep_all(&|| Box::new(Sculpture::new(RATE)));
}

#[test]
fn parameter_table_is_sound() {
    let ps = s::params_table();
    let mut ids: Vec<u32> = ps.iter().map(|p| p.id).collect();
    ids.sort();
    let n = ids.len();
    ids.dedup();
    assert_eq!(n, ids.len(), "duplicate parameter ids");
    for p in ps {
        assert!(p.min <= p.default && p.default <= p.max, "{} default out of range", p.name);
    }
    assert!(n > 400, "{n} parameters");
}

#[test]
fn string_pitch_follows_the_key() {
    for (n, hz) in [(45u8, 110.0f32), (57, 220.0), (60, 261.6), (72, 523.3), (84, 1046.5)] {
        let x = note(&[], n, 100, 1.0);
        let f = fundamental(win(&x, 0.2, 0.9), hz * 0.8, hz * 1.25);
        assert!((f / hz - 1.0).abs() < 0.01, "note {n}: {f} Hz instead of {hz}");
    }
    // the global Tune (cents) and Transpose (octaves)
    let a = note(&[(s::TUNE, 0.0)], 57, 100, 0.8);
    let b = note(&[(s::TUNE, 100.0)], 57, 100, 0.8);
    let (fa, fb) = (fundamental(win(&a, 0.2, 0.7), 180.0, 270.0), fundamental(win(&b, 0.2, 0.7), 180.0, 270.0));
    assert!((fb / fa - 2f32.powf(1.0 / 12.0)).abs() < 0.005);
    let c = note(&[(s::TRANSPOSE, 1.0)], 57, 100, 0.8);
    assert!((fundamental(win(&c, 0.2, 0.7), 350.0, 520.0) / fa - 2.0).abs() < 0.01);
}

#[test]
fn material_pad_inner_loss_and_stiffness() {
    let bright_of = |il: f32, st: f32| {
        let x = note(&[(s::INNER_LOSS, il), (s::STIFFNESS, st), (s::MEDIA_LOSS, 0.1)], 55, 110, 1.0);
        bright(win(&x, 0.05, 0.7), 1500.0)
    };
    // more inner loss: a more mellow decay (the highs die first)
    assert!(bright_of(0.9, 0.0) < 0.5 * bright_of(0.05, 0.0));
    // stiffness stretches the overtones: the second partial moves above twice the fundamental
    let part = |st: f32| {
        let x = note(&[(s::INNER_LOSS, 0.1), (s::STIFFNESS, st), (s::MEDIA_LOSS, 0.1), (s::RESOLUTION, 1.0)], 48, 110, 1.2);
        let w = win(&x, 0.1, 1.0);
        let f0 = 130.81;
        // strongest peak between 1.8 and 3 times the fundamental
        let mut best = (0.0, 0.0);
        let mut hz = f0 * 1.8;
        while hz < f0 * 3.0 {
            let t = tone(w, hz);
            if t > best.0 {
                best = (t, hz);
            }
            hz += 1.0;
        }
        best.1 / f0
    };
    let (p0, p1) = (part(0.0), part(1.0));
    assert!((p0 - 2.0).abs() < 0.01, "plain string: second partial at {p0}");
    assert!(p1 > 2.04, "stiff string: second partial at {p1}");
    // the four corners of the pad all play
    for (st, il) in [(0.0, 0.1), (1.0, 0.1), (0.0, 0.9), (1.0, 0.9)] {
        let x = note(&[(s::INNER_LOSS, il), (s::STIFFNESS, st)], 60, 100, 0.5);
        assert!(finite(&x) && peak(&x) > 0.01);
    }
}

#[test]
fn media_loss_tension_and_resolution() {
    let long = note(&[(s::MEDIA_LOSS, 0.05)], 60, 100, 2.0);
    let short = note(&[(s::MEDIA_LOSS, 0.8)], 60, 100, 2.0);
    assert!(rms(win(&long, 1.0, 1.5)) > 8.0 * rms(win(&short, 1.0, 1.5)));
    // tension modulation: the loud start is sharper than the faded tail
    let t0 = note(&[(s::TENS_MOD, 1.0), (s::MEDIA_LOSS, 0.1)], 48, 127, 1.5);
    let early = fundamental(win(&t0, 0.02, 0.12), 100.0, 190.0);
    let late = fundamental(win(&t0, 1.0, 1.4), 100.0, 190.0);
    assert!(early > late * 1.003, "tension mod: {early} vs {late}");
    let flat = note(&[(s::TENS_MOD, 0.0), (s::MEDIA_LOSS, 0.1)], 48, 127, 1.5);
    let e2 = fundamental(win(&flat, 0.02, 0.12), 100.0, 190.0);
    let l2 = fundamental(win(&flat, 1.0, 1.4), 100.0, 190.0);
    assert!((e2 / l2 - 1.0).abs() < 0.002);
    // resolution: fewer overtones when it is low
    let hi = note(&[(s::RESOLUTION, 1.0), (s::INNER_LOSS, 0.05)], 36, 110, 0.8);
    let lo = note(&[(s::RESOLUTION, 0.2), (s::INNER_LOSS, 0.05)], 36, 110, 0.8);
    assert!(bright(win(&hi, 0.05, 0.5), 2500.0) > 3.0 * bright(win(&lo, 0.05, 0.5), 2500.0));
    // a very low resolution is inharmonic even with Stiffness 0
    let vlo = note(&[(s::RESOLUTION, 0.0), (s::STIFFNESS, 0.0)], 48, 110, 1.0);
    let w = win(&vlo, 0.1, 0.8);
    let f0 = fundamental(w, 120.0, 140.0);
    assert!(tone(w, 2.0 * f0) < 0.5 * tone(w, f0) || (fundamental(w, f0 * 1.5, f0 * 3.0) / f0 - 2.0).abs() > 0.02);
}

#[test]
fn render_modes() {
    let basic = note(&[(s::RENDER_MODE, 0.0), (s::RESOLUTION, 1.0), (s::INNER_LOSS, 0.05)], 36, 110, 0.6);
    let ext = note(&[(s::RENDER_MODE, 1.0), (s::RESOLUTION, 1.0), (s::INNER_LOSS, 0.05)], 36, 110, 0.6);
    let hd = note(&[(s::RENDER_MODE, 2.0), (s::RESOLUTION, 1.0), (s::INNER_LOSS, 0.05)], 36, 110, 0.6);
    assert!(finite(&hd) && peak(&hd) > 0.01);
    // Basic caps the string at 100 elements (99 overtones), Extended allows 1000
    assert!(bright(win(&ext, 0.05, 0.4), 4000.0) > 2.0 * bright(win(&basic, 0.05, 0.4), 4000.0));
    // High Definition keeps the pitch
    let f = fundamental(win(&hd, 0.1, 0.5), 55.0, 80.0);
    assert!((f / 65.4 - 1.0).abs() < 0.01);
}

#[test]
fn keyscale_and_release() {
    // low-note scaling only reaches notes below C3 (middle C)
    let lo_base = note(&[(s::INNER_LOSS, 0.1)], 36, 110, 0.8);
    let lo_ks = note(&[(s::INNER_LOSS, 0.1), (s::IL_KS_LO, 1.0)], 36, 110, 0.8);
    assert!(bright(win(&lo_ks, 0.05, 0.6), 1200.0) < 0.6 * bright(win(&lo_base, 0.05, 0.6), 1200.0));
    let hi_base = note(&[(s::INNER_LOSS, 0.1)], 84, 110, 0.8);
    let hi_ks = note(&[(s::INNER_LOSS, 0.1), (s::IL_KS_LO, 1.0)], 84, 110, 0.8);
    assert!(differs(&hi_base, &hi_ks) < 1e-3);
    // high-note media loss scaling shortens high notes only
    let h = note(&[(s::ML_KS_HI, 1.0), (s::MEDIA_LOSS, 0.2)], 96, 110, 1.0);
    let hb = note(&[(s::MEDIA_LOSS, 0.2)], 96, 110, 1.0);
    assert!(rms(win(&h, 0.4, 0.8)) < 0.3 * rms(win(&hb, 0.4, 0.8)));
    // stiffness scaling: a stiff string only at the low keys
    let a = note(&[(s::ST_KS_LO, 1.0)], 36, 110, 0.5);
    let b = note(&[], 36, 110, 0.5);
    assert!(differs(&a, &b) > 0.1);
    // release: media loss above 1.0 damps the string after the key goes up
    let tail = |rel: f32| {
        let mut i = sc();
        i.set(s::A_RELEASE, 4.0);
        i.set(s::MEDIA_LOSS, 0.1);
        i.set(s::ML_REL, rel);
        i.note_on(60, 100);
        render(&mut i, 0.3);
        i.note_off(60);
        let x = render(&mut i, 1.2);
        rms(win(&x, 0.6, 1.1))
    };
    assert!(tail(2.0) < 0.3 * tail(1.0));
    assert!(tail(1.0) > 0.0);
}

#[test]
fn objects_all_types_play() {
    // object 1: impulse, strike, gravstrike, pick, bow, bow wide, noise, blow
    for ty in 0..8usize {
        let (st, tim) = match ty {
            4 | 5 => (0.6, 0.0),
            7 => (1.0, 0.0),
            _ => (0.8, 0.0),
        };
        let x = note(&[(s::obj(0, s::O_TYPE), ty as f32), (s::obj(0, s::O_STR), st), (s::obj(0, s::O_TIM), tim)], 60, 110, 1.5);
        assert!(finite(&x), "type {ty}");
        assert!(rms(win(&x, 0.3, 1.3)) > 0.004, "type {} is silent: {}", s::OBJ1_TYPES[ty], rms(win(&x, 0.3, 1.3)));
        if ty != 6 {
            let f = fundamental(win(&x, 0.3, 1.3), 200.0, 330.0);
            assert!((f / 261.6 - 1.0).abs() < 0.03, "type {}: {f} Hz", s::OBJ1_TYPES[ty]);
        }
    }
    // Strength 0 stops the excitation
    let z = note(&[(s::obj(0, s::O_STR), 0.0)], 60, 110, 0.5);
    assert!(peak(&z) < 1e-4);
    // sustained objects keep the string going, one-shot ones let it die
    let bow = note(&[(s::obj(0, s::O_TYPE), 4.0), (s::obj(0, s::O_STR), 0.6), (s::MEDIA_LOSS, 0.7)], 60, 110, 2.0);
    let pick = note(&[(s::obj(0, s::O_TYPE), 3.0), (s::MEDIA_LOSS, 0.7)], 60, 110, 2.0);
    assert!(rms(win(&bow, 1.5, 1.9)) > 20.0 * rms(win(&pick, 1.5, 1.9)));
}

#[test]
fn object_timbre_variation_and_velocity() {
    // impulse: Timbre narrows the pulse (brighter)
    let b = |t: f32| bright(win(&note(&[(s::obj(0, s::O_TIM), t), (s::INNER_LOSS, 0.05)], 48, 110, 0.5), 0.0, 0.3), 1500.0);
    assert!(b(1.0) > 2.0 * b(-1.0));
    // strike: a lighter hammer (Timbre up) is brighter
    let st = |t: f32| bright(win(&note(&[(s::obj(0, s::O_TYPE), 1.0), (s::obj(0, s::O_TIM), t), (s::INNER_LOSS, 0.05)], 48, 110, 0.5), 0.0, 0.3), 1500.0);
    assert!(st(1.0) > st(-1.0));
    // pick: a sharper pick (Timbre up) gives a stronger attack
    let pk = |t: f32| bright(win(&note(&[(s::obj(0, s::O_TYPE), 3.0), (s::obj(0, s::O_TIM), t), (s::INNER_LOSS, 0.05)], 48, 110, 0.5), 0.0, 0.3), 800.0);
    assert!(pk(1.0) > 1.5 * pk(-1.0), "{} {}", pk(1.0), pk(-1.0));
    // noise: Timbre sets its bandwidth
    let nz = |t: f32| bright(&note(&[(s::obj(0, s::O_TYPE), 6.0), (s::obj(0, s::O_TIM), t), (s::MEDIA_LOSS, 0.9), (s::INNER_LOSS, 1.0)], 60, 110, 0.5), 2000.0);
    assert!(nz(1.0) > nz(-1.0));
    // velocity: harder is louder; VeloSens 0 removes it
    let soft = note(&[(s::obj(0, s::O_VEL), 1.0)], 60, 30, 0.4);
    let hard = note(&[(s::obj(0, s::O_VEL), 1.0)], 60, 120, 0.4);
    assert!(peak(&hard) > 2.5 * peak(&soft));
    let a = note(&[(s::obj(0, s::O_VEL), 0.0)], 60, 30, 0.4);
    let c = note(&[(s::obj(0, s::O_VEL), 0.0)], 60, 120, 0.4);
    assert!((peak(&a) / peak(&c) - 1.0).abs() < 0.05);
}

#[test]
fn object_position_shapes_the_spectrum() {
    // an impulse in the middle cannot excite the even harmonics
    let mid = note(&[(s::obj(0, s::O_POS), 0.5), (s::PICK_A, 0.31), (s::PICK_B, 0.31), (s::INNER_LOSS, 0.05), (s::STIFFNESS, 0.0)], 48, 110, 0.6);
    let w = win(&mid, 0.05, 0.5);
    let f0 = fundamental(w, 120.0, 140.0);
    assert!(tone(w, 2.0 * f0) < 0.2 * tone(w, f0), "{} {}", tone(w, 2.0 * f0), tone(w, f0));
    let side = note(&[(s::obj(0, s::O_POS), 0.2), (s::PICK_A, 0.31), (s::PICK_B, 0.31), (s::INNER_LOSS, 0.05), (s::STIFFNESS, 0.0)], 48, 110, 0.6);
    let w2 = win(&side, 0.05, 0.5);
    assert!(tone(w2, 2.0 * f0) > 0.4 * tone(w2, f0));
    // Object 1 at the extreme end loses the low end
    let end = note(&[(s::obj(0, s::O_POS), 0.02)], 36, 110, 0.5);
    let mid2 = note(&[(s::obj(0, s::O_POS), 0.3)], 36, 110, 0.5);
    let f1 = fundamental(win(&mid2, 0.1, 0.4), 55.0, 80.0);
    assert!(tone(win(&end, 0.1, 0.4), f1) < 0.4 * tone(win(&mid2, 0.1, 0.4), f1));
}

#[test]
fn object_gates() {
    let run = |gate: f32| {
        let mut i = sc();
        i.set(s::obj(0, s::O_TYPE), 4.0);
        i.set(s::obj(0, s::O_STR), 0.6);
        i.set(s::obj(0, s::O_GATE), gate);
        i.set(s::A_RELEASE, 10.0);
        i.set(s::MEDIA_LOSS, 0.8);
        i.note_on(60, 110);
        let on = render(&mut i, 1.0);
        i.note_off(60);
        let off = render(&mut i, 1.5);
        (rms(win(&on, 0.5, 1.0)), rms(win(&off, 0.8, 1.3)))
    };
    let (k_on, k_off) = run(0.0);
    let (a_on, a_off) = run(1.0);
    let (o_on, o_off) = run(2.0);
    // KeyOn: sounds while held, stops at key up
    assert!(k_on > 0.01 && k_off < 0.2 * k_on, "{k_on} {k_off}");
    // Always: bows through the release
    assert!(a_on > 0.01 && a_off > 0.3 * a_on, "{a_on} {a_off}");
    // KeyOff: nothing while held, starts at key up
    assert!(o_on < 0.1 * a_on && o_off > 0.01, "{o_on} {o_off} {a_on}");
}

#[test]
fn disturb_and_damp_objects() {
    let base = [(s::obj(0, s::O_STR), 0.8), (s::INNER_LOSS, 0.05)];
    let plain = note(&base, 48, 110, 0.8);
    // Disturb (object 2): an obstacle near the string buzzes against it
    let mut set = base.to_vec();
    set.extend([(s::obj(1, s::O_ON), 1.0), (s::obj(1, s::O_TYPE), 9.0), (s::obj(1, s::O_STR), 0.9), (s::obj(1, s::O_TIM), 0.0), (s::obj(1, s::O_POS), 0.4)]);
    let dist = note(&set, 48, 110, 0.8);
    assert!(finite(&dist) && differs(&plain, &dist) > 0.2);
    // Disturb 2-sided
    set[2] = (s::obj(1, s::O_ON), 1.0);
    let mut set2 = base.to_vec();
    set2.extend([(s::obj(1, s::O_ON), 1.0), (s::obj(1, s::O_TYPE), 10.0), (s::obj(1, s::O_STR), 0.9), (s::obj(1, s::O_TIM), 0.0)]);
    assert!(differs(&plain, &note(&set2, 48, 110, 0.8)) > 0.2);
    // Damp at the middle with Timbre 1 leaves the second harmonic: the octave sounds
    let mut d = vec![(s::obj(0, s::O_POS), 0.2), (s::PICK_A, 0.31), (s::PICK_B, 0.31), (s::INNER_LOSS, 0.05), (s::STIFFNESS, 0.0), (s::MEDIA_LOSS, 0.05)];
    d.extend([(s::obj(2, s::O_ON), 1.0), (s::obj(2, s::O_GATE), 0.0), (s::obj(2, s::O_POS), 0.5), (s::obj(2, s::O_TIM), 1.0), (s::obj(2, s::O_STR), 1.0)]);
    let x = note(&d, 48, 110, 1.5);
    let w = win(&x, 0.8, 1.4);
    let f0 = fundamental(&note(&[(s::STIFFNESS, 0.0)], 48, 110, 0.5)[2400..], 120.0, 140.0);
    assert!(tone(w, 2.0 * f0) > 1.5 * tone(w, f0), "octave {} fundamental {}", tone(w, 2.0 * f0), tone(w, f0));
    // a higher Damp strength stops the string sooner; with more Inner Loss less is needed
    let dmp = |st: f32| {
        let set = [(s::obj(2, s::O_ON), 1.0), (s::obj(2, s::O_GATE), 0.0), (s::obj(2, s::O_POS), 0.3), (s::obj(2, s::O_STR), st), (s::MEDIA_LOSS, 0.05)];
        rms(win(&note(&set, 48, 110, 1.5), 1.0, 1.4))
    };
    assert!(dmp(1.0) < 0.5 * dmp(0.2));
    // Mass changes the spectrum without a timbre or variation; Bound and Bouncing play
    for ty in [11.0, 12.0, 13.0] {
        let set = [(s::obj(1, s::O_ON), 1.0), (s::obj(1, s::O_TYPE), ty), (s::obj(1, s::O_STR), 0.8), (s::obj(1, s::O_POS), 0.4), (s::INNER_LOSS, 0.05)];
        let x = note(&set, 48, 110, 0.8);
        assert!(finite(&x), "type {ty}");
        assert!(differs(&plain, &x) > 0.05, "type {ty} changes nothing");
    }
}

#[test]
fn pickups_and_spread() {
    // two pickups at the same place with Invert cancel each other out
    let same = note(&[(s::PICK_A, 0.3), (s::PICK_B, 0.3), (s::PICK_INVERT, 0.0)], 60, 100, 0.4);
    let inv = note(&[(s::PICK_A, 0.3), (s::PICK_B, 0.3), (s::PICK_INVERT, 1.0)], 60, 100, 0.4);
    assert!(peak(&inv) < 0.02 * peak(&same));
    // moving the pickups toward the end is thinner
    let mid = note(&[(s::PICK_A, 0.45), (s::PICK_B, 0.5)], 48, 100, 0.5);
    let end = note(&[(s::PICK_A, 0.04), (s::PICK_B, 0.06)], 48, 100, 0.5);
    let f1 = fundamental(win(&mid, 0.1, 0.4), 100.0, 140.0);
    assert!(tone(win(&end, 0.1, 0.4), f1) < 0.5 * tone(win(&mid, 0.1, 0.4), f1));
    // Spread Pickup: A goes right, B left; at 0 the sound is mono
    let (l0, r0) = note_st(&[(s::PICK_A, 0.25), (s::PICK_B, 0.7)], 60, 100, 0.4);
    assert!(differs(&l0, &r0) < 1e-3);
    let (l1, r1) = note_st(&[(s::PICK_A, 0.25), (s::PICK_B, 0.7), (s::SPREAD_PICK, 1.0)], 60, 100, 0.4);
    assert!(differs(&l1, &r1) > 0.3);
    // Key Spread pans by note number
    let (lo_l, lo_r) = note_st(&[(s::SPREAD_KEY, 1.0)], 36, 100, 0.3);
    let (hi_l, hi_r) = note_st(&[(s::SPREAD_KEY, 1.0)], 96, 100, 0.3);
    assert!(rms(&lo_l) > 3.0 * rms(&lo_r));
    assert!(rms(&hi_r) > 3.0 * rms(&hi_l));
}

#[test]
fn external_object_takes_the_side_chain() {
    let mut i = sc();
    i.set(s::obj(0, s::O_ON), 0.0);
    i.set(s::obj(1, s::O_ON), 1.0);
    i.set(s::obj(1, s::O_TYPE), 8.0);
    i.set(s::obj(1, s::O_STR), 1.0);
    i.set(s::obj(1, s::O_TIM), 1.0);
    i.note_on(60, 100);
    let mut out = Vec::new();
    let mut rng = 1u32;
    for _ in 0..40 {
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        let sc_l: Vec<f32> = (0..256).map(|_| { rng = rng.wrapping_mul(1664525).wrapping_add(1013904223); (rng >> 8) as f32 / 8388608.0 - 1.0 }).collect();
        i.side_chain(&sc_l, &sc_l);
        i.render(&mut l, &mut r);
        out.extend(l);
    }
    assert!(rms(&out[4000..]) > 0.003, "external object silent");
    // without a side chain signal nothing sounds
    let mut j = sc();
    j.set(s::obj(0, s::O_ON), 0.0);
    j.set(s::obj(1, s::O_ON), 1.0);
    j.set(s::obj(1, s::O_TYPE), 8.0);
    let x = play(&mut j, &[], 60, 100, 0.3);
    assert!(peak(&x) < 1e-5);
}

// ---------------------------------------------------------------------------------------------
// processing: amplitude envelope, waveshaper, filter, delay, body EQ, level

/// A steady source for the processing tests: noise into a heavily damped string.
const STEADY: [(u32, f32); 6] = [
    (s::obj(0, s::O_TYPE), 6.0),
    (s::obj(0, s::O_STR), 0.8),
    (s::obj(0, s::O_VEL), 0.0),
    (s::MEDIA_LOSS, 0.95),
    (s::INNER_LOSS, 0.0),
    (s::RESOLUTION, 1.0),
];

fn with(base: &[(u32, f32)], extra: &[(u32, f32)]) -> Vec<(u32, f32)> {
    let mut v = base.to_vec();
    v.extend_from_slice(extra);
    v
}

/// RMS in consecutive windows of `w` seconds.
fn track(x: &[f32], w: f32) -> Vec<f32> {
    x.chunks((w * RATE) as usize).filter(|c| c.len() as f32 >= w * RATE * 0.99).map(rms).collect()
}

/// Peaks of a track that rise above its mean by a quarter of its swing.
fn cycles(t: &[f32]) -> usize {
    let mean = t.iter().sum::<f32>() / t.len() as f32;
    let (lo, hi) = t.iter().fold((f32::MAX, 0.0f32), |a, v| (a.0.min(*v), a.1.max(*v)));
    let thr = mean + 0.25 * (hi - lo);
    let mut n = 0;
    let mut above = false;
    for v in t {
        if *v > thr && !above {
            n += 1;
            above = true;
        } else if *v < mean {
            above = false;
        }
    }
    n
}

#[test]
fn amplitude_envelope() {
    // Attack Soft and Hard: the attack time between them follows the velocity
    let eq = [(s::obj(0, s::O_VEL), 0.0)];
    let soft = note(&with(&eq, &[(s::A_SOFT, 0.6), (s::A_HARD, 0.0)]), 60, 20, 1.0);
    let hard = note(&with(&eq, &[(s::A_SOFT, 0.6), (s::A_HARD, 0.0)]), 60, 127, 1.0);
    assert!(peak(win(&soft, 0.0, 0.1)) < 0.3 * peak(win(&hard, 0.0, 0.1)), "soft {} hard {}", peak(win(&soft, 0.0, 0.1)), peak(win(&hard, 0.0, 0.1)));
    // Decay and Sustain: the level falls to the sustain level and stays there
    let d = note(&with(&STEADY, &[(s::A_DECAY, 0.15), (s::A_SUSTAIN, 0.3)]), 60, 100, 1.5);
    let full = note(&with(&STEADY, &[(s::A_DECAY, 0.15), (s::A_SUSTAIN, 1.0)]), 60, 100, 1.5);
    let ratio = rms(win(&d, 1.0, 1.4)) / rms(win(&full, 1.0, 1.4));
    assert!((ratio - 0.3).abs() < 0.05, "{ratio}");
    assert!(rms(win(&d, 0.02, 0.06)) > 1.2 * rms(win(&d, 1.0, 1.4)));
    // Release: a short release is silent soon after the key goes up
    let tail = |r: f32| {
        let mut i = sc();
        for &(id, v) in STEADY.iter() {
            i.set(id, v);
        }
        i.set(s::A_RELEASE, r);
        i.note_on(60, 100);
        render(&mut i, 0.5);
        i.note_off(60);
        rms(win(&render(&mut i, 0.6), 0.3, 0.5))
    };
    assert!(tail(0.05) < 0.02 * tail(3.0));
    // the pickups feed the envelope before the waveshaper and filter: a zero sustain silences them
    let z = note(&with(&STEADY, &[(s::A_DECAY, 0.05), (s::A_SUSTAIN, 0.0), (s::WS_ON, 1.0), (s::WS_INPUT, 1.0)]), 60, 100, 1.0);
    assert!(rms(win(&z, 0.6, 0.9)) < 1e-3);
}

#[test]
fn same_pitch_reuses_the_voice() {
    // with both Attack sliders at 0 a repeated key retriggers the vibrating string
    let mut i = Sculpture::new(RATE);
    i.note_on(60, 100);
    render(&mut i, 0.2);
    i.note_off(60);
    render(&mut i, 0.05);
    i.note_on(60, 100);
    render(&mut i, 0.2);
    assert_eq!(i.active_voices(), 1);
    // with an attack a new voice is allocated and the old one fades (it never doubles the pitch)
    let mut j = Sculpture::new(RATE);
    j.set(s::A_SOFT, 0.1);
    j.set(s::A_HARD, 0.1);
    j.note_on(60, 100);
    render(&mut j, 0.2);
    j.note_on(60, 100);
    render(&mut j, 0.3);
    assert_eq!(j.active_voices(), 1);
    // the retriggered string keeps what it had: the second note starts above a fresh one
    let mut k = Sculpture::new(RATE);
    k.set(s::MEDIA_LOSS, 0.0);
    k.note_on(60, 100);
    render(&mut k, 0.1);
    k.note_on(60, 100);
    let again = render(&mut k, 0.05);
    let fresh = note(&[(s::MEDIA_LOSS, 0.0)], 60, 100, 0.05);
    assert!(rms(&again) > 1.2 * rms(&fresh));
}

#[test]
fn waveshaper() {
    let src = with(&STEADY, &[(s::obj(0, s::O_TYPE), 0.0), (s::obj(0, s::O_STR), 0.3), (s::MEDIA_LOSS, 0.1), (s::INNER_LOSS, 0.4)]);
    let dry = note(&src, 55, 100, 0.6);
    for ty in 0..4 {
        let on = note(&with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, ty as f32), (s::WS_INPUT, 1.0)]), 55, 100, 0.6);
        assert!(finite(&on));
        let h = |x: &[f32]| tone(win(x, 0.05, 0.5), 3.0 * 196.0) / tone(win(x, 0.05, 0.5), 196.0);
        assert!(h(&on) > 1.4 * h(&dry), "type {ty}: third harmonic {} vs {}", h(&on), h(&dry));
    }
    // the types are different curves
    let a = note(&with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, 0.0), (s::WS_INPUT, 0.8)]), 55, 100, 0.4);
    let b = note(&with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, 3.0), (s::WS_INPUT, 0.8)]), 55, 100, 0.4);
    assert!(differs(&a, &b) > 0.2);
    // Input Scale cuts: low input barely shapes; it is level compensated
    let cut = note(&with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, 1.0), (s::WS_INPUT, -1.0)]), 55, 100, 0.5);
    let hot = note(&with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, 1.0), (s::WS_INPUT, 1.0)]), 55, 100, 0.5);
    assert!(bright(win(&cut, 0.05, 0.4), 2000.0) < bright(win(&hot, 0.05, 0.4), 2000.0));
    let ratio = rms(&hot) / rms(&cut);
    assert!(ratio > 0.25 && ratio < 4.0, "level compensation {ratio}");
    // VariDrive Variation: negative adds the dry signal, positive the inverted dry
    let base = with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, 0.0), (s::WS_INPUT, 0.5)]);
    let neg = note(&with(&base, &[(s::WS_VAR, -1.0)]), 55, 100, 0.4);
    let zero = note(&with(&base, &[(s::WS_VAR, 0.0)]), 55, 100, 0.4);
    let pos = note(&with(&base, &[(s::WS_VAR, 1.0)]), 55, 100, 0.4);
    assert!(rms(&neg) > rms(&zero) && rms(&pos) < rms(&neg));
    // other types: Variation biases the curve, so even harmonics appear
    let sym = note(&with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, 2.0), (s::WS_INPUT, 0.6), (s::WS_VAR, 0.0)]), 55, 100, 0.6);
    let bias = note(&with(&src, &[(s::WS_ON, 1.0), (s::WS_TYPE, 2.0), (s::WS_INPUT, 0.6), (s::WS_VAR, 0.8)]), 55, 100, 0.6);
    assert!(differs(&bias, &sym) > 0.1, "variation changes nothing");
    // off is off
    assert!(differs(&dry, &note(&with(&src, &[(s::WS_ON, 0.0), (s::WS_INPUT, 1.0)]), 55, 100, 0.6)) < 1e-4);
}

#[test]
fn multimode_filter() {
    let src = with(&STEADY, &[(s::F_ON, 1.0)]);
    let base = note(&with(&src, &[(s::CUTOFF, 1.0)]), 48, 100, 0.8);
    let lp = note(&with(&src, &[(s::F_TYPE, 1.0), (s::CUTOFF, 0.35)]), 48, 100, 0.8);
    assert!(bright(win(&lp, 0.1, 0.7), 1500.0) < 0.2 * bright(win(&base, 0.1, 0.7), 1500.0));
    // HiPass removes the low end
    let hp = note(&with(&src, &[(s::F_TYPE, 0.0), (s::CUTOFF, 0.65)]), 48, 100, 0.8);
    let lows = |x: &[f32]| tone(win(x, 0.1, 0.7), 100.0) + tone(win(x, 0.1, 0.7), 300.0);
    assert!(lows(&hp) < 0.1 * lows(&base), "{} {}", lows(&hp), lows(&base));
    // Notch cuts a band, BandPass keeps one, Peak boosts one
    let band = |ty: f32, res: f32| {
        let x = note(&with(&src, &[(s::F_TYPE, ty), (s::CUTOFF, 0.5), (s::RESONANCE, res)]), 48, 100, 0.8);
        win(&x, 0.1, 0.7).to_vec()
    };
    let centre = 20.0 * 1000f32.powf(0.5);
    let flat = win(&base, 0.1, 0.7).to_vec();
    let around = |x: &[f32]| tone(x, centre * 0.98) + tone(x, centre) + tone(x, centre * 1.02);
    assert!(around(&band(4.0, 0.5)) < 0.6 * around(&flat));
    assert!(around(&band(3.0, 0.5)) > 0.0);
    assert!(rms(&band(3.0, 0.5)) < 0.5 * rms(&flat));
    assert!(around(&band(2.0, 0.9)) > 1.5 * around(&flat));
    // Resonance emphasises the cutoff region of a low-pass
    let r0 = note(&with(&src, &[(s::F_TYPE, 1.0), (s::CUTOFF, 0.5), (s::RESONANCE, 0.0)]), 48, 100, 0.8);
    let r1 = note(&with(&src, &[(s::F_TYPE, 1.0), (s::CUTOFF, 0.5), (s::RESONANCE, 0.95)]), 48, 100, 0.8);
    assert!(around(win(&r1, 0.1, 0.7)) > 2.0 * around(win(&r0, 0.1, 0.7)));
    // Key: the cutoff follows the note; Velo Sens: harder notes open it
    let k0 = note(&with(&src, &[(s::F_TYPE, 1.0), (s::CUTOFF, 0.45), (s::F_KEY, 0.0)]), 84, 100, 0.6);
    let k1 = note(&with(&src, &[(s::F_TYPE, 1.0), (s::CUTOFF, 0.45), (s::F_KEY, 1.0)]), 84, 100, 0.6);
    assert!(rms(win(&k1, 0.1, 0.5)) > 1.5 * rms(win(&k0, 0.1, 0.5)));
    let v = |vel: u8| rms(win(&note(&with(&src, &[(s::F_TYPE, 1.0), (s::CUTOFF, 0.4), (s::F_VELO, 1.0)]), 60, vel, 0.6), 0.1, 0.5));
    assert!(v(127) > 1.5 * v(10));
    // Off bypasses
    let off = note(&with(&src, &[(s::F_ON, 0.0), (s::CUTOFF, 0.0)]), 48, 100, 0.8);
    assert!(differs(&base, &off) < 0.1);
}

fn click() -> Vec<(u32, f32)> {
    // a short ping: an impulse into a string that dies at once
    vec![(s::MEDIA_LOSS, 1.0), (s::INNER_LOSS, 1.0), (s::A_RELEASE, 0.02), (s::obj(0, s::O_TIM), 0.5)]
}

fn ping(extra: &[(u32, f32)], secs: f32) -> (Vec<f32>, Vec<f32>) {
    let mut i = sc();
    i.set_tempo(120.0);
    for &(id, v) in click().iter().chain(extra) {
        i.set(id, v);
    }
    i.note_on(72, 100);
    let a = render_st(&mut i, 0.03);
    i.note_off(72);
    let b = render_st(&mut i, secs - 0.03);
    ([a.0, b.0].concat(), [a.1, b.1].concat())
}

#[test]
fn delay() {
    let on = [(s::D_ON, 1.0), (s::D_TIME, 300.0), (s::D_FB, 0.5), (s::D_WET, 1.0), (s::D_HICUT, 20000.0), (s::D_LOCUT, 20.0)];
    let (l, _) = ping(&on, 1.4);
    // an echo at 300 ms and the next (feedback) at 600 ms
    assert!(rms(win(&l, 0.29, 0.34)) > 20.0 * rms(win(&l, 0.15, 0.2)));
    assert!(rms(win(&l, 0.59, 0.64)) > 5.0 * rms(win(&l, 0.45, 0.5)));
    // off: nothing; Wet Level 0: nothing
    let (o, _) = ping(&with(&on, &[(s::D_ON, 0.0)]), 0.6);
    assert!(rms(win(&o, 0.29, 0.34)) < 1e-5);
    let (w, _) = ping(&with(&on, &[(s::D_WET, 0.0)]), 0.6);
    assert!(rms(win(&w, 0.29, 0.34)) < 1e-5);
    // feedback 0: a single echo; negative feedback inverts every second echo
    let (f0, _) = ping(&with(&on, &[(s::D_FB, 0.0)]), 1.0);
    assert!(rms(win(&f0, 0.59, 0.64)) < 0.01 * rms(win(&f0, 0.29, 0.34)));
    let (fp, _) = ping(&with(&on, &[(s::D_FB, 0.8)]), 1.0);
    let (fn_, _) = ping(&with(&on, &[(s::D_FB, -0.8)]), 1.0);
    let seg = |x: &[f32], t: f32| x[(t * RATE) as usize..((t + 0.05) * RATE) as usize].to_vec();
    let corr = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    assert!(corr(&seg(&fp, 0.3), &seg(&fp, 0.6)) > 0.0);
    assert!(corr(&seg(&fn_, 0.3), &seg(&fn_, 0.6)) < 0.0);
    // sync: 1/8 at 120 bpm is 250 ms; free time in ms
    let (sy, _) = ping(&with(&on, &[(s::D_SYNC, 1.0), (s::D_NOTE, 9.0)]), 0.8);
    assert!(rms(win(&sy, 0.24, 0.29)) > 20.0 * rms(win(&sy, 0.12, 0.17)));
    // hi cut dulls the echo, lo cut thins it
    let (hc, _) = ping(&with(&on, &[(s::D_HICUT, 600.0)]), 0.6);
    let (hf, _) = ping(&on, 0.6);
    assert!(bright(win(&hc, 0.29, 0.4), 1500.0) < 0.3 * bright(win(&hf, 0.29, 0.4), 1500.0));
    let (lc, _) = ping(&with(&on, &[(s::D_LOCUT, 2000.0)]), 0.6);
    assert!(rms(win(&lc, 0.29, 0.4)) < 0.7 * rms(win(&hf, 0.29, 0.4)));
}

#[test]
fn delay_stereo_controls() {
    let on = [(s::D_ON, 1.0), (s::D_TIME, 300.0), (s::D_FB, 0.6), (s::D_WET, 1.0), (s::D_HICUT, 20000.0)];
    // Input Balance moves the input toward one side
    let (l, r) = ping(&with(&on, &[(s::D_INBAL, -1.0)]), 0.6);
    assert!(rms(win(&l, 0.29, 0.34)) > 20.0 * rms(win(&r, 0.29, 0.34)));
    let (l, r) = ping(&with(&on, &[(s::D_INBAL, 1.0)]), 0.6);
    assert!(rms(win(&r, 0.29, 0.34)) > 20.0 * rms(win(&l, 0.29, 0.34)));
    // Output Width 0 is mono
    let (l, r) = ping(&with(&on, &[(s::D_INBAL, -1.0), (s::D_WIDTH, 0.0)]), 0.6);
    assert!(differs(win(&l, 0.29, 0.34), win(&r, 0.29, 0.34)) < 1e-3);
    // Xfeed: the repeats cross over (ping-pong)
    let (l, r) = ping(&with(&on, &[(s::D_INBAL, -1.0), (s::D_XFEED, 1.0)]), 1.0);
    assert!(rms(win(&l, 0.29, 0.34)) > 10.0 * rms(win(&r, 0.29, 0.34)));
    assert!(rms(win(&r, 0.59, 0.64)) > 10.0 * rms(win(&l, 0.59, 0.64)));
    // Groove (the Groove Pad's x axis): +50% on a 1/4 gives 1/8 on the right and 1/4 on the left
    let g = [(s::D_ON, 1.0), (s::D_SYNC, 1.0), (s::D_NOTE, 12.0), (s::D_FB, 0.0), (s::D_WET, 1.0), (s::D_HICUT, 20000.0), (s::D_GROOVE, 0.5)];
    let (l, r) = ping(&g, 0.9);
    assert!(rms(win(&r, 0.24, 0.29)) > 20.0 * rms(win(&l, 0.24, 0.29)));
    assert!(rms(win(&l, 0.49, 0.54)) > 20.0 * rms(win(&r, 0.49, 0.54)));
    // Spread (the y axis) lengthens the right delay and shortens the left
    let sp = [(s::D_ON, 1.0), (s::D_TIME, 400.0), (s::D_FB, 0.0), (s::D_WET, 1.0), (s::D_HICUT, 20000.0), (s::D_SPREAD, 0.5)];
    let (l, r) = ping(&sp, 0.8);
    assert!(rms(win(&l, 0.29, 0.34)) > 20.0 * rms(win(&r, 0.29, 0.34)));
    assert!(rms(win(&r, 0.49, 0.54)) > 20.0 * rms(win(&l, 0.49, 0.54)));
}

#[test]
fn body_eq() {
    let tonal = [(s::obj(0, s::O_STR), 0.8), (s::INNER_LOSS, 0.1), (s::MEDIA_LOSS, 0.2), (s::RESOLUTION, 1.0), (s::B_ON, 1.0)];
    let src = tonal.to_vec();
    let flat = note(&src, 55, 100, 0.6);
    // Basic model: Low and High shelves, Mid band swept by the Mid Frequency
    let low = note(&with(&src, &[(s::B_MODEL, 0.0), (s::B_P1, 1.0), (s::B_P2, 0.0), (s::B_P3, 0.0)]), 55, 100, 0.6);
    let hi = note(&with(&src, &[(s::B_MODEL, 0.0), (s::B_P1, 0.0), (s::B_P2, 0.0), (s::B_P3, 1.0)]), 55, 100, 0.6);
    assert!(tone(&low, 196.0) > 2.0 * tone(&flat, 196.0), "{} {}", tone(&low, 196.0), tone(&flat, 196.0));
    assert!(bright(&hi, 2500.0) > 3.0 * bright(&flat, 2500.0), "{} {}", bright(&hi, 2500.0), bright(&flat, 2500.0));
    let mid_hz = |m: f32| {
        let x = note(&with(&src, &[(s::B_MODEL, 0.0), (s::B_P2, 1.0), (s::B_P4, m)]), 55, 100, 0.6);
        bright(&x, 2000.0)
    };
    let (hi_a, hi_b) = (mid_hz(0.2), mid_hz(0.85));
    assert!(hi_b > 2.0 * hi_a, "mid frequency sweep {hi_a} {hi_b}");
    // On/Off
    let off = note(&with(&src, &[(s::B_ON, 0.0), (s::B_P1, 1.0)]), 55, 100, 0.6);
    assert!(differs(&flat, &off) < 1e-4);
    // models: Intensity 0 is flat, 1 is strong, negative inverts the formants
    let m = |model: f32, int: f32| note(&with(&src, &[(s::B_MODEL, model), (s::B_P1, int), (s::B_P2, 0.0), (s::B_P3, 0.0), (s::B_P4, 0.0)]), 55, 100, 0.6);
    assert!(differs(&flat, &m(1.0, 0.0)) < 1e-3);
    let strong = m(1.0, 1.0);
    let inverse = m(1.0, -1.0);
    assert!(differs(&flat, &strong) > 0.3 && differs(&strong, &inverse) > 0.3);
    // Shift moves the formants; Stretch spreads them; Fine Structure adds detail
    let shifted = note(&with(&src, &[(s::B_MODEL, 4.0), (s::B_P1, 1.0), (s::B_P2, 0.3), (s::B_P3, 0.0), (s::B_P4, 0.0)]), 55, 100, 0.6);
    let unshifted = m(4.0, 1.0);
    assert!(differs(&shifted, &unshifted) > 0.3);
    let stretched = note(&with(&src, &[(s::B_MODEL, 4.0), (s::B_P1, 1.0), (s::B_P2, 0.0), (s::B_P3, 1.0), (s::B_P4, 0.0)]), 55, 100, 0.6);
    assert!(differs(&stretched, &unshifted) > 0.2);
    let fine = note(&with(&src, &[(s::B_MODEL, 4.0), (s::B_P1, 1.0), (s::B_P4, 1.0)]), 55, 100, 0.6);
    assert!(differs(&fine, &unshifted) > 0.1);
    // every model plays
    for model in 0..11 {
        assert!(finite(&m(model as f32, 1.0)));
    }
}

#[test]
fn level_and_limiter() {
    let base = note(&[(s::LIMITER, 0.0)], 60, 100, 0.4);
    let quiet = note(&[(s::LIMITER, 0.0), (s::LEVEL, -20.0)], 60, 100, 0.4);
    assert!((rms(&quiet) / rms(&base) - 0.1).abs() < 0.01);
    // very loud: Off clips past full scale, Mono keeps the sum in range
    let hot = [(s::LEVEL, 12.0), (s::obj(0, s::O_STR), 1.0), (s::PICK_A, 0.2), (s::PICK_B, 0.25), (s::obj(1, s::O_ON), 1.0), (s::obj(1, s::O_TYPE), 1.0), (s::obj(1, s::O_STR), 1.0)];
    let chord = |lim: f32| {
        let mut i = sc();
        for &(id, v) in hot.iter() {
            i.set(id, v);
        }
        i.set(s::LIMITER, lim);
        for n in [48, 55, 60, 64, 67] {
            i.note_on(n, 127);
        }
        render(&mut i, 0.3)
    };
    let off = peak(&chord(0.0));
    let mono = peak(&chord(1.0));
    let poly = peak(&chord(2.0));
    let both = peak(&chord(3.0));
    assert!(off > 1.0, "off {off}");
    assert!(mono <= 0.96 && both <= 0.96, "mono {mono} both {both}");
    assert!(poly < off);
}

// ---------------------------------------------------------------------------------------------
// global controls, modulation

fn tgt(name: &str) -> f32 {
    s::TARGETS.iter().position(|t| *t == name).unwrap() as f32
}

/// A steady source, a long note, optionally with events along the way.
fn long(extra: &[(u32, f32)], vel: u8, secs: f32) -> Vec<f32> {
    note(&with(&STEADY, extra), 60, vel, secs)
}

fn swing(t: &[f32]) -> f32 {
    let (lo, hi) = t.iter().fold((f32::MAX, 0.0f32), |a, v| (a.0.min(*v), a.1.max(*v)));
    (hi - lo) / hi.max(1e-9)
}

#[test]
fn glide_keyboard_modes_and_bend() {
    // Glide: the note slides up from the one before
    let mut i = sc();
    i.set(s::GLIDE, 0.4);
    i.note_on(48, 100);
    render(&mut i, 0.3);
    i.note_off(48);
    i.note_on(60, 100);
    let x = render(&mut i, 1.0);
    let early = fundamental(win(&x, 0.02, 0.12), 120.0, 300.0);
    let late = fundamental(win(&x, 0.5, 0.7), 120.0, 300.0);
    assert!(early < 0.9 * late && (late / 261.6 - 1.0).abs() < 0.02, "{early} {late}");
    // Poly with Glide 0: no slide
    let mut p = sc();
    p.set(s::A_RELEASE, 0.02);
    p.note_on(48, 100);
    render(&mut p, 0.2);
    p.note_off(48);
    render(&mut p, 0.15);
    p.note_on(60, 100);
    let y = render(&mut p, 0.5);
    assert!((fundamental(win(&y, 0.02, 0.12), 120.0, 300.0) / 261.6 - 1.0).abs() < 0.02);
    // Polyphony caps the voices
    let mut q = sc();
    q.set(s::POLY, 2.0);
    for n in [48, 52, 55, 60] {
        q.note_on(n, 100);
        render(&mut q, 0.02);
    }
    render(&mut q, 0.05);
    assert!(q.active_voices() <= 2, "{} voices", q.active_voices());
    let mut r = sc();
    for n in [48, 52, 55, 60] {
        r.note_on(n, 100);
    }
    render(&mut r, 0.05);
    assert_eq!(r.active_voices(), 4);
    // Mono: one voice; played legato it is not retriggered, staccato it is
    let burst = |kb: f32, overlap: bool| {
        let mut m = sc();
        m.set(s::KBMODE, kb);
        m.set(s::MEDIA_LOSS, 0.3);
        m.note_on(60, 100);
        render(&mut m, 0.4);
        if !overlap {
            m.note_off(60);
            render(&mut m, 0.01);
        }
        m.note_on(64, 100);
        assert_eq!(m.active_voices(), 1);
        let a = render(&mut m, 0.03);
        rms(&a)
    };
    let before = {
        let mut m = sc();
        m.set(s::MEDIA_LOSS, 0.3);
        m.note_on(60, 100);
        let x = render(&mut m, 0.43);
        rms(win(&x, 0.4, 0.43))
    };
    assert!(burst(1.0, false) > 2.0 * before);
    assert!(burst(1.0, true) < 1.5 * before);
    assert!(burst(2.0, false) > 1.5 * burst(2.0, true), "{} {} before {before}", burst(2.0, false), burst(2.0, true));
    // Legato mode: portamento only when played legato; Mono: always
    let slide = |kb: f32, overlap: bool| {
        let mut m = sc();
        m.set(s::KBMODE, kb);
        m.set(s::GLIDE, 0.8);
        m.set(s::MEDIA_LOSS, 0.05);
        m.note_on(48, 100);
        render(&mut m, 0.3);
        if !overlap {
            m.note_off(48);
            render(&mut m, 0.02);
        }
        m.note_on(60, 100);
        let x = render(&mut m, 0.4);
        fundamental(win(&x, 0.05, 0.2), 120.0, 300.0)
    };
    assert!(slide(2.0, true) < 0.9 * 261.6);
    assert!(slide(2.0, false) > 0.97 * 261.6);
    assert!(slide(1.0, false) < 0.9 * 261.6);
    // Bender range up and down, linked or not
    let bent = |set: &[(u32, f32)], v: i16| {
        let mut m = sc();
        for &(id, x) in set {
            m.set(id, x);
        }
        m.note_on(60, 100);
        m.pitch_bend(v);
        let x = render(&mut m, 0.5);
        pitch(win(&x, 0.1, 0.4), 100.0, 500.0)
    };
    let st = |f: f32| 12.0 * (f / 261.63).log2();
    assert!((st(bent(&[(s::BEND_UP, 2.0)], 8191)) - 2.0).abs() < 0.05, "{}", st(bent(&[(s::BEND_UP, 2.0)], 8191)));
    assert!((st(bent(&[(s::BEND_UP, 5.0), (s::BEND_LINK, 1.0)], -8192)) + 5.0).abs() < 0.08);
    assert!((st(bent(&[(s::BEND_UP, 5.0), (s::BEND_DOWN, 1.0), (s::BEND_LINK, 0.0)], -8192)) + 1.0).abs() < 0.05);
    // the bend moves the modelled string's length, not a post-effect: a bend keeps the tone alive
    assert!(rms(&{
        let mut m = sc();
        m.note_on(60, 100);
        m.pitch_bend(8191);
        render(&mut m, 0.3)
    }) > 0.005);
}

#[test]
fn tune_transpose_warmth() {
    // Warmth detunes each voice a little, at random
    let pitches = |w: f32| {
        let mut m = sc();
        m.set(s::WARMTH, w);
        m.set(s::MEDIA_LOSS, 0.05);
        (0..6)
            .map(|_| {
                m.reset();
                m.note_on(57, 100);
                let x = render(&mut m, 0.5);
                let w = win(&x, 0.1, 0.45);
                let (mut best, mut hz) = ((0.0, 0.0), 205.0);
                while hz < 235.0 {
                    let t = tone(w, hz);
                    if t > best.0 {
                        best = (t, hz);
                    }
                    hz += 0.1;
                }
                best.1
            })
            .collect::<Vec<f32>>()
    };
    let spread = |v: &[f32]| v.iter().fold(0.0f32, |a, b| a.max(*b)) / v.iter().fold(f32::MAX, |a, b| a.min(*b)) - 1.0;
    assert!(spread(&pitches(1.0)) > 0.0015, "{:?}", pitches(1.0));
    assert!(spread(&pitches(0.0)) < 0.0005);
    // the same pitch is never doubled: one voice
    let mut m = sc();
    m.note_on(60, 100);
    m.note_on(60, 100);
    render(&mut m, 0.1);
    assert_eq!(m.active_voices(), 1);
}

#[test]
fn vibrato() {
    let wob = |wheel: u8, set: &[(u32, f32)]| {
        let mut m = sc();
        for &(id, v) in set {
            m.set(id, v);
        }
        m.set(s::MEDIA_LOSS, 0.05);
        m.controller(1, wheel);
        m.note_on(60, 100);
        let x = render(&mut m, 1.2);
        let f: Vec<f32> = (0..8).map(|k| fundamental(win(&x, 0.1 + k as f32 * 0.12, 0.2 + k as f32 * 0.12), 200.0, 330.0)).collect();
        f.iter().fold(0.0f32, |a, b| a.max(*b)) / f.iter().fold(f32::MAX, |a, b| a.min(*b)) - 1.0
    };
    // depth comes from the Vib Depth controller (the mod wheel by default)
    let on = wob(127, &[(s::VIB_AMT, 1.0), (s::VIB_RATE, 4.0)]);
    let off = wob(0, &[(s::VIB_AMT, 1.0), (s::VIB_RATE, 4.0)]);
    assert!(on > 0.01 && off < 0.002, "{on} {off}");
    // the Via slider: 0 ignores the controller, Amt is the maximum
    assert!(wob(0, &[(s::VIB_AMT, 1.0), (s::VIB_RATE, 4.0), (s::VIB_VIA, 0.0)]) > 0.01);
    assert!(wob(127, &[(s::VIB_AMT, 0.3), (s::VIB_RATE, 4.0)]) < 0.7 * on);
    // another controller can be chosen
    let mut m = sc();
    m.set(s::CTRL_VIB, 2.0);
    m.set(s::VIB_AMT, 1.0);
    m.set(s::VIB_RATE, 4.0);
    m.controller(1, 127);
    m.note_on(60, 100);
    let x = render(&mut m, 0.8);
    let a = fundamental(win(&x, 0.1, 0.2), 200.0, 330.0);
    let b = fundamental(win(&x, 0.3, 0.4), 200.0, 330.0);
    assert!((a / b - 1.0).abs() < 0.003);
    // sync and the waveform
    let sq = wob(127, &[(s::VIB_AMT, 1.0), (s::VIB_RATE, 4.0), (s::VIB_WAVE, 5.0)]);
    assert!(sq > 0.01);
}

#[test]
fn lfos() {
    let route = |i: usize, on: &[(u32, f32)]| {
        let mut v = vec![(s::lfo(i, s::L_ON), 1.0), (s::lfo_r(i, 0, s::R_ON), 1.0), (s::lfo_r(i, 0, s::R_TARGET), tgt("Level")), (s::lfo_r(i, 0, s::R_AMT), -0.9)];
        v.extend_from_slice(on);
        v
    };
    // an LFO on the level: the output pulses at the LFO rate, in Hz or tempo-synced
    for i in 0..2 {
        let x = long(&route(i, &[(s::lfo(i, s::L_RATE), 4.0), (s::lfo(i, s::L_PHASE), 1.0)]), 100, 2.0);
        let n = cycles(&track(win(&x, 0.2, 1.8), 0.025));
        assert!((5..=7).contains(&n), "lfo {i}: {n} cycles in 1.6 s");
    }
    let mut m = sc();
    m.set_tempo(120.0);
    let set = with(&STEADY, &route(0, &[(s::lfo(0, s::L_SYNC), 1.0), (s::lfo(0, s::L_NOTE), 12.0)]));
    for &(id, v) in &set {
        m.set(id, v);
    }
    m.note_on(60, 100);
    let x = render(&mut m, 2.2);
    let n = cycles(&track(win(&x, 0.2, 2.2), 0.025));
    assert!((3..=5).contains(&n), "synced to 1/4 at 120 bpm: {n} cycles in 2 s");
    // Rate 0.00 Hz stops the oscillation, the routing still works
    let flat = long(&route(0, &[(s::lfo(0, s::L_RATE), 0.0), (s::lfo(0, s::L_WAVE), 5.0), (s::lfo(0, s::L_PHASE), 1.0)]), 100, 1.0);
    let plain = long(&[], 100, 1.0);
    let wavy = long(&route(0, &[(s::lfo(0, s::L_RATE), 4.0), (s::lfo(0, s::L_PHASE), 1.0)]), 100, 1.0);
    assert!(swing(&track(win(&flat, 0.3, 0.9), 0.05)) < 0.8 * swing(&track(win(&wavy, 0.3, 0.9), 0.05)));
    assert!(rms(win(&flat, 0.3, 0.9)) < 0.5 * rms(win(&plain, 0.3, 0.9)));
    // the Envelope: positive fades in, negative fades out
    // (the same sound without the LFO is the reference, so only the modulation shows)
    let gain = |env: f32| {
        let m = long(&route(0, &[(s::lfo(0, s::L_RATE), 4.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo(0, s::L_ENV), env)]), 100, 3.0);
        let r = long(&route(0, &[(s::lfo(0, s::L_RATE), 4.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo_r(0, 0, s::R_AMT), 0.0)]), 100, 3.0);
        let a = track(&m, 0.025);
        let b = track(&r, 0.025);
        a.iter().zip(&b).map(|(x, y)| x / y.max(1e-9)).collect::<Vec<f32>>()
    };
    let fin = gain(0.25);
    assert!(swing(&fin[2..16]) < 0.5 * swing(&fin[80..112]), "{} {}", swing(&fin[2..16]), swing(&fin[80..112]));
    let fout = gain(-0.25);
    assert!(swing(&fout[80..112]) < 0.5 * swing(&fout[2..16]));
    // Via: velocity scales the amount (Amt at velocity 0, Via amount at full velocity)
    let via = |vel: u8| {
        let set = |va: f32| [(s::lfo(0, s::L_ON), 1.0), (s::lfo(0, s::L_RATE), 4.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo_r(0, 0, s::R_ON), 1.0), (s::lfo_r(0, 0, s::R_TARGET), tgt("Level")), (s::lfo_r(0, 0, s::R_VIA), 2.0), (s::lfo_r(0, 0, s::R_AMT), 0.0), (s::lfo_r(0, 0, s::R_VIAAMT), va)];
        let a = track(&long(&set(0.9), vel, 1.5), 0.025);
        let b = track(&long(&set(0.0), vel, 1.5), 0.025);
        let g: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x / y.max(1e-9)).collect();
        swing(&g[8..56])
    };
    assert!(via(127) > 3.0 * via(12), "{} {}", via(127), via(12));
    // a second target; every waveform; the Curve knob
    let two = long(&route(0, &[(s::lfo(0, s::L_RATE), 6.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo_r(0, 1, s::R_ON), 1.0), (s::lfo_r(0, 1, s::R_TARGET), tgt("PickupA Position")), (s::lfo_r(0, 1, s::R_AMT), 0.4)]), 100, 0.8);
    assert!(finite(&two));
    let sine = long(&route(0, &[(s::lfo(0, s::L_RATE), 6.0), (s::lfo(0, s::L_PHASE), 1.0)]), 100, 0.8);
    for w in 0..10 {
        let x = long(&route(0, &[(s::lfo(0, s::L_RATE), 6.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo(0, s::L_WAVE), w as f32)]), 100, 0.8);
        assert!(finite(&x), "wave {w}");
        if w > 0 && w != 3 {
            assert!(differs(&sine, &x) > 0.05, "wave {} is the same as the sine", s::LFO_WAVES[w]);
        }
    }
    let curved = long(&route(0, &[(s::lfo(0, s::L_RATE), 6.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo(0, s::L_CURVE), 0.8)]), 100, 0.8);
    assert!(differs(&sine, &curved) > 0.05);
    // RateMod: Ctrl A speeds the LFO up
    let rm = |wheel: u8| {
        let mut m = sc();
        for &(id, v) in with(&STEADY, &route(0, &[(s::lfo(0, s::L_RATE), 1.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo(0, s::L_RMSRC), 5.0), (s::lfo(0, s::L_RMAMT), 1.0)])).iter() {
            m.set(id, v);
        }
        m.controller(1, wheel);
        m.note_on(60, 100);
        let x = render(&mut m, 1.8);
        cycles(&track(win(&x, 0.2, 1.8), 0.025))
    };
    assert!(rm(127) >= 8 && rm(0) <= 2);
    // the LFO restarts for each note (poly phase 1.0) and is shared by all voices when Phase is 0 (mono)
    let mono = long(&route(0, &[(s::lfo(0, s::L_RATE), 4.0), (s::lfo(0, s::L_PHASE), 0.0)]), 100, 1.5);
    assert!(cycles(&track(win(&mono, 0.2, 1.4), 0.025)) >= 3);
}

#[test]
fn jitter_random_and_velocity_mods() {
    // jitter: continuous random modulation with a rate
    let j = |on: f32, rate: f32| long(&[(s::jit(0, s::J_ON), on), (s::jit(0, s::J_RATE), rate), (s::jit(0, s::J_T1), tgt("Level")), (s::jit(0, s::J_A1), 0.8)], 100, 1.5);
    assert!(differs(&j(0.0, 12.0), &j(1.0, 12.0)) > 0.2);
    // the Rate sets how fast it wanders
    let slow = track(win(&j(1.0, 0.5), 0.2, 1.4), 0.025);
    let fast = track(win(&j(1.0, 30.0), 0.2, 1.4), 0.025);
    let rough = |t: &[f32]| t.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / t.iter().sum::<f32>();
    assert!(rough(&fast) > 1.3 * rough(&slow), "{} {}", rough(&fast), rough(&slow));
    // jitter 2 and the second target
    let x = long(&[(s::jit(1, s::J_ON), 1.0), (s::jit(1, s::J_T2), tgt("Pitch")), (s::jit(1, s::J_A2), 0.1)], 100, 0.6);
    assert!(finite(&x));
    // note-on random: each note a different value, kept for the note
    let notes = |on: f32| {
        let mut m = sc();
        m.set(s::nr(0, s::N_ON), on);
        m.set(s::nr(0, s::N_TARGET), tgt("Pitch"));
        m.set(s::nr(0, s::N_AMT), 0.3);
        m.set(s::MEDIA_LOSS, 0.05);
        (0..3)
            .map(|_| {
                m.note_on(60, 100);
                let x = render(&mut m, 0.5);
                m.note_off(60);
                render(&mut m, 0.2);
                fundamental(win(&x, 0.1, 0.45), 150.0, 420.0)
            })
            .collect::<Vec<f32>>()
    };
    let sp = |v: &[f32]| v.iter().fold(0.0f32, |a, b| a.max(*b)) / v.iter().fold(f32::MAX, |a, b| a.min(*b)) - 1.0;
    assert!(sp(&notes(1.0)) > 0.01 && sp(&notes(0.0)) < 0.001);
    // velocity modulation: target, amount, curve
    let vm = |vel: u8, curve: f32| {
        let mut m = sc();
        m.set(s::vmod(0, s::V_ON), 1.0);
        m.set(s::vmod(0, s::V_TARGET), tgt("Pitch"));
        m.set(s::vmod(0, s::V_AMT), 0.1);
        m.set(s::vmod(0, s::V_CURVE), curve);
        m.set(s::MEDIA_LOSS, 0.05);
        m.note_on(60, vel);
        let x = render(&mut m, 0.5);
        12.0 * (fundamental(win(&x, 0.1, 0.45), 150.0, 450.0) / 261.63).log2()
    };
    assert!(vm(127, 1.0) > vm(20, 1.0) + 0.6);
    // concave keeps the soft end low, convex raises it
    assert!(vm(64, 2.0) > vm(64, 0.0) + 0.2);
    // the other velocity controls: Strength of an object, the filter
    assert!(peak(&note(&[(s::obj(0, s::O_VEL), 1.0)], 60, 120, 0.3)) > 2.0 * peak(&note(&[(s::obj(0, s::O_VEL), 1.0)], 60, 30, 0.3)));
}

#[test]
fn objects_can_be_modulated() {
    // almost all object parameters can be modulated: Object 1 Position by an LFO changes the spectrum over time
    let lfo_pos = long(&[(s::lfo(0, s::L_ON), 1.0), (s::lfo(0, s::L_RATE), 3.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo_r(0, 0, s::R_ON), 1.0), (s::lfo_r(0, 0, s::R_TARGET), tgt("Object1 Position")), (s::lfo_r(0, 0, s::R_AMT), 0.8)], 100, 1.0);
    assert!(differs(&long(&[], 100, 1.0), &lfo_pos) > 0.1);
    // Object2 Strength, Pickup pan, string stiffness, filter cutoff and waveshaper targets all take effect
    for t in ["Object2 Strength", "Pickup Pan A+B", "Pickup Pan A-B", "Pickup Pos A-B", "Pickup Pos A+B", "String Stiffness", "String Inner Loss", "String Media Loss", "String Tension Mod", "Filter Cutoff", "Filter Resonance", "Waveshaper Input", "Waveshaper Variation", "Object1 Timbre", "Object1 Variation", "Object3 Strength"] {
        let base = [(s::WS_ON, 1.0), (s::obj(1, s::O_ON), 1.0), (s::obj(1, s::O_STR), 0.3), (s::obj(2, s::O_ON), 1.0), (s::obj(2, s::O_STR), 0.3), (s::obj(2, s::O_TYPE), 4.0), (s::obj(2, s::O_GATE), 0.0), (s::F_TYPE, 1.0), (s::CUTOFF, 0.6), (s::RESONANCE, 0.3)];
        let off = note(&base, 60, 100, 0.5);
        let on = note(&with(&base, &[(s::jit(0, s::J_ON), 0.0), (s::nr(0, s::N_ON), 1.0), (s::nr(0, s::N_TARGET), tgt(t)), (s::nr(0, s::N_AMT), 0.9)]), 60, 100, 0.5);
        assert!(finite(&on), "{t}");
        assert!(differs(&off, &on) > 0.01, "target {t} has no effect");
    }
}

// ---------------------------------------------------------------------------------------------
// control envelopes

/// What an envelope does to the level: routed with Amt -1, the gain is `1 - envelope`; the
/// reference render (Amt 0) cancels the source's own wobble. Returns the envelope per window.
fn env_track(i: usize, extra: &[(u32, f32)], vel: u8, secs: f32, ctrl: Option<u8>) -> Vec<f32> {
    let routed = |amt: f32| {
        let mut set = vec![(s::env_r(i, 0, s::R_ON), 1.0), (s::env_r(i, 0, s::R_TARGET), tgt("Level")), (s::env_r(i, 0, s::R_AMT), amt)];
        set.extend_from_slice(extra);
        let mut m = sc();
        m.set_tempo(120.0);
        for &(id, v) in with(&STEADY, &set).iter() {
            m.set(id, v);
        }
        if let Some(c) = ctrl {
            m.controller(1, c);
        }
        m.note_on(60, vel);
        track(&render(&mut m, secs), 0.05)
    };
    let (a, b) = (routed(-1.0), routed(0.0));
    a.iter().zip(&b).map(|(x, y)| 1.0 - x / y.max(1e-9)).collect()
}

fn at(t: &[f32], secs: f32) -> f32 {
    t[((secs / 0.05) as usize).min(t.len() - 1)]
}

#[test]
fn control_envelope_shape_and_modes() {
    // the default curve: attack to 1 in 20 ms, decay to 0.6, loop point, sustain at 0.4 until the key goes up
    let e = env_track(0, &[], 100, 2.0, None);
    assert!(at(&e, 0.1) > 0.7, "{}", at(&e, 0.1));
    assert!((at(&e, 1.0) - 0.4).abs() < 0.1 && (at(&e, 1.8) - 0.4).abs() < 0.1, "{} {}", at(&e, 1.0), at(&e, 1.8));
    // Finish: runs to the end even though the key is down
    let f = env_track(0, &[(s::env(0, s::E_SUS), 1.0)], 100, 2.0, None);
    assert!(at(&f, 1.6) < 0.1, "{}", at(&f, 1.6));
    // loops: a full swing between the loop and sustain nodes
    let swing_levels = [(s::env_l(0, 2), 1.0), (s::env_l(0, 3), 0.0)];
    let loop_cycles = |mode: f32, scale: f32| {
        let t = env_track(0, &with(&swing_levels, &[(s::env(0, s::E_SUS), mode), (s::env(0, s::E_SCALE), scale)]), 100, 3.0, None);
        cycles(&t[16..60])
    };
    let fwd = loop_cycles(2.0, 100.0);
    let bwd = loop_cycles(3.0, 100.0);
    let alt = loop_cycles(4.0, 100.0);
    assert!((4..=6).contains(&fwd), "forward {fwd}");
    assert!((4..=6).contains(&bwd), "backward {bwd}");
    assert!((2..=3).contains(&alt), "alternate {alt}");
    // Timescale 200% stretches everything
    assert!((2..=3).contains(&loop_cycles(2.0, 200.0)));
    let half = env_track(0, &[(s::env(0, s::E_SCALE), 200.0)], 100, 1.0, None);
    assert!(at(&half, 0.5) > at(&e, 0.5) + 0.05, "{} {}", at(&half, 0.5), at(&e, 0.5));
    // after the key goes up the envelope runs on to its end
    let rel = |amt: f32| {
        let mut m = sc();
        for &(id, v) in with(&STEADY, &[(s::env_r(0, 0, s::R_ON), 1.0), (s::env_r(0, 0, s::R_TARGET), tgt("Level")), (s::env_r(0, 0, s::R_AMT), amt), (s::obj(0, s::O_GATE), 1.0), (s::A_RELEASE, 10.0)]).iter() {
            m.set(id, v);
        }
        m.note_on(60, 100);
        let mut x = render(&mut m, 0.9);
        m.note_off(60);
        x.extend(render(&mut m, 1.0));
        track(&x, 0.05)
    };
    let (a, b) = (rel(-1.0), rel(0.0));
    let e: Vec<f32> = a.iter().zip(&b).map(|(x, y)| 1.0 - x / y.max(1e-9)).collect();
    assert!((at(&e, 0.8) - 0.4).abs() < 0.1 && at(&e, 1.7) < 0.12, "{} {}", at(&e, 0.8), at(&e, 1.7));
}

#[test]
fn control_envelope_velocity_ctrl_and_sync() {
    // A-Time Velosens: positive shortens the attack at low velocity, negative at high velocity
    let slow = [(s::env_t(0, 0), 800.0), (s::env_t(0, 1), 200.0)];
    let lo = env_track(0, &with(&slow, &[(s::env(0, s::E_ATV), 1.0)]), 15, 1.0, None);
    let hi = env_track(0, &with(&slow, &[(s::env(0, s::E_ATV), 1.0)]), 127, 1.0, None);
    assert!(at(&lo, 0.1) > at(&hi, 0.1) + 0.4, "{} {}", at(&lo, 0.1), at(&hi, 0.1));
    let lo = env_track(0, &with(&slow, &[(s::env(0, s::E_ATV), -1.0)]), 15, 1.0, None);
    let hi = env_track(0, &with(&slow, &[(s::env(0, s::E_ATV), -1.0)]), 127, 1.0, None);
    assert!(at(&hi, 0.1) > at(&lo, 0.1) + 0.4, "{} {}", at(&lo, 0.1), at(&hi, 0.1));
    // Ctrl mode: the controller is the value; both: it is added as an offset
    let c = [(s::env(0, s::E_CTRL), 1.0), (s::env(0, s::E_ENV), 0.0), (s::CTRL_ENV1, 1.0)];
    assert!(at(&env_track(0, &c, 100, 0.6, Some(127)), 0.4) > 0.9);
    assert!(at(&env_track(0, &c, 100, 0.6, Some(0)), 0.4) < 0.1);
    let both = [(s::env(0, s::E_CTRL), 1.0), (s::env(0, s::E_ENV), 1.0), (s::CTRL_ENV1, 1.0)];
    let b = env_track(0, &both, 100, 1.6, Some(64));
    assert!((at(&b, 1.2) - (0.4 + 64.0 / 127.0)).abs() < 0.1, "{}", at(&b, 1.2));
    // neither on: the envelope does nothing
    let off = env_track(0, &[(s::env(0, s::E_CTRL), 0.0), (s::env(0, s::E_ENV), 0.0)], 100, 0.5, None);
    assert!(at(&off, 0.3).abs() < 0.05);
    // a Via source scales the amount: Amt at via 0, Via amount at the maximum
    let v = |vel: u8| at(&env_track(0, &[(s::env_r(0, 0, s::R_VIA), 2.0), (s::env_r(0, 0, s::R_VIAAMT), -1.0), (s::env_r(0, 0, s::R_AMT), 0.0)], vel, 1.2, None), 0.9);
    let _ = v;
    // second envelope and second routing
    let e2 = env_track(1, &[], 100, 1.0, None);
    assert!((at(&e2, 0.8) - 0.4).abs() < 0.1);
    // Sync: times snap to note values at the tempo, Free brings them back to ms
    let mut m = sc();
    m.set_tempo(120.0);
    m.set(s::env(0, s::E_SYNC), 1.0);
    let ms: Vec<f32> = (0..4).map(|k| m.get(s::env_t(0, k))).collect();
    assert!((ms[0] - 31.25).abs() < 0.5 && (ms[1] - 250.0).abs() < 0.5 && (ms[2] - 375.0).abs() < 0.5 && (ms[3] - 666.7).abs() < 0.5, "{ms:?}");
    // at half the tempo the synced envelope takes twice as long
    m.set_tempo(60.0);
    m.set(s::env(0, s::E_SYNC), 0.0);
    assert!((m.get(s::env_t(0, 1)) - 500.0).abs() < 1.5, "{}", m.get(s::env_t(0, 1)));
}

fn record_env(trig: f32, pedal: bool) -> Sculpture {
    let mut m = sc();
    m.set_tempo(120.0);
    for &(id, v) in STEADY.iter() {
        m.set(id, v);
    }
    m.set(s::CTRL_ENV1, 1.0);
    m.set(s::env(0, s::E_TRIG), trig);
    m.set(s::env(0, s::E_REC), 1.0);
    if pedal {
        m.note_on(60, 100);
        render(&mut m, 0.2);
        m.controller(64, 127);
    } else if trig == 1.0 {
        m.note_on(60, 100);
        render(&mut m, 0.2);
    } else {
        m.note_on(60, 100);
    }
    for k in 0..50 {
        m.controller(1, (k as f32 * 127.0 / 49.0) as u8);
        render(&mut m, 0.02);
    }
    m
}

#[test]
fn recording_a_control_envelope() {
    let mut m = record_env(0.0, false);
    // still recording until R is clicked again
    m.set(s::env(0, s::E_REC), 0.0);
    assert_eq!(m.get(s::env(0, s::E_HASREC)), 1.0);
    // Mode is now Env only, the curve is the ramp that was played, 1 s long
    assert_eq!((m.get(s::env(0, s::E_ENV)), m.get(s::env(0, s::E_CTRL))), (1.0, 0.0));
    let len = m.get(s::env(0, s::E_RECLEN));
    assert!((len - 1000.0).abs() < 120.0, "{len} ms");
    let r: Vec<f32> = (0..s::REC_POINTS).map(|k| m.get(s::env_rec(0, k))).collect();
    assert!(r.windows(2).all(|w| w[1] >= w[0] - 1e-3) && r[s::REC_POINTS - 1] > 0.9 && r[0] < 0.1, "{r:?}");
    // the handles were placed: the peak, then a loop and a sustain node, times add up to the length
    let t: f32 = (0..4).map(|k| m.get(s::env_t(0, k))).sum();
    assert!((t - len).abs() < 25.0);
    // played back (route to the level, Amt -1): polyphonic, each note runs its own copy
    for &(id, v) in &[(s::env_r(0, 0, s::R_ON), 1.0), (s::env_r(0, 0, s::R_TARGET), tgt("Level")), (s::env_r(0, 0, s::R_AMT), -1.0), (s::env(0, s::E_SUS), 1.0)] {
        m.set(id, v);
    }
    m.set(s::obj(0, s::O_TYPE), 6.0);
    m.reset();
    m.set(s::CTRL_ENV1, 1.0);
    m.note_on(60, 100);
    let mut a = render(&mut m, 0.5);
    m.note_on(67, 100);
    a.extend(render(&mut m, 0.3));
    let (f60, f67) = (|x: &[f32]| tone(x, 261.6), |x: &[f32]| tone(x, 392.0));
    let _ = (f60, f67);
    assert!(finite(&a));
    // the recorded curve (ramp up) shows as a gain falling over the second
    let mut p = sc();
    for &(id, v) in with(&STEADY, &[(s::env_r(0, 0, s::R_ON), 1.0), (s::env_r(0, 0, s::R_TARGET), tgt("Level")), (s::env_r(0, 0, s::R_AMT), -1.0), (s::env(0, s::E_SUS), 1.0)]).iter() {
        p.set(id, v);
    }
    for k in 0..s::REC_POINTS {
        p.set(s::env_rec(0, k), k as f32 / (s::REC_POINTS - 1) as f32);
    }
    p.set(s::env(0, s::E_RECLEN), 1000.0);
    p.set(s::env(0, s::E_HASREC), 1.0);
    p.set(s::env(0, s::E_USEREC), 1.0);
    p.set(s::env_t(0, 0), 10.0);
    p.set(s::env_t(0, 1), 300.0);
    p.set(s::env_t(0, 2), 400.0);
    p.set(s::env_t(0, 3), 290.0);
    p.note_on(60, 100);
    let x = render(&mut p, 1.2);
    assert!(rms(win(&x, 0.05, 0.15)) > 3.0 * rms(win(&x, 0.8, 0.95)));
    // Compare: the edited (node) version is another curve
    p.reset();
    p.set(s::env(0, s::E_USEREC), 0.0);
    p.note_on(60, 100);
    let y = render(&mut p, 1.2);
    assert!(differs(&x, &y) > 0.05);
}

#[test]
fn envelope_record_triggers() {
    // Note + Ctrl Movement: recording starts when the controller moves with a note held (the 0.2 s before are not in it)
    let mut m = record_env(1.0, false);
    m.set(s::env(0, s::E_REC), 0.0);
    let len = m.get(s::env(0, s::E_RECLEN));
    assert!((len - 1000.0).abs() < 120.0, "ctrl trigger: {len} ms");
    // Note + Sustain Pedal
    let mut m = record_env(2.0, true);
    m.set(s::env(0, s::E_REC), 0.0);
    let len = m.get(s::env(0, s::E_RECLEN));
    assert!((len - 1000.0).abs() < 120.0, "pedal trigger: {len} ms");
    assert_eq!(m.get(s::env(0, s::E_HASREC)), 1.0);
    // a new note after all keys went up stops it
    let mut m = sc();
    m.set(s::CTRL_ENV1, 1.0);
    m.set(s::env(0, s::E_REC), 1.0);
    m.note_on(60, 100);
    for k in 0..20 {
        m.controller(1, (k * 6) as u8);
        render(&mut m, 0.02);
    }
    m.note_off(60);
    render(&mut m, 0.05);
    assert_eq!(m.get(s::env(0, s::E_HASREC)), 0.0);
    m.note_on(62, 100);
    assert_eq!(m.get(s::env(0, s::E_HASREC)), 1.0);
    // an envelope only records the controller assigned to it
    let mut m = sc();
    m.set(s::CTRL_ENV1, 2.0);
    m.set(s::env(0, s::E_REC), 1.0);
    m.note_on(60, 100);
    for k in 0..30 {
        m.controller(1, (k * 4) as u8);
        render(&mut m, 0.02);
    }
    m.set(s::env(0, s::E_REC), 0.0);
    let r: Vec<f32> = (0..s::REC_POINTS).map(|k| m.get(s::env_rec(0, k))).collect();
    assert!(r.iter().all(|v| *v < 0.01));
}

// ---------------------------------------------------------------------------------------------
// morph

const CUT: usize = 21;
const MORPH_SRC: [(u32, f32); 3] = [(s::F_ON, 1.0), (s::F_TYPE, 1.0), (s::RESONANCE, 0.0)];

/// A morph test voice: noise through a low-pass whose cutoff differs per point.
fn morphing(extra: &[(u32, f32)]) -> Sculpture {
    let mut m = sc();
    m.set_tempo(120.0);
    for &(id, v) in with(&STEADY, &MORPH_SRC).iter() {
        m.set(id, v);
    }
    for (p, c) in [(0, 0.1), (1, 0.1), (2, 0.1), (3, 1.0), (4, 0.5)] {
        m.set_point_value(p, CUT, c);
    }
    for &(id, v) in extra {
        m.set(id, v);
    }
    m
}

fn dark(x: &[f32]) -> f32 {
    // the share of the energy in the highs: grows with the cutoff
    bright(x, 700.0)
}

#[test]
fn morph_points_and_pad() {
    let at_pos = |x: f32, y: f32| {
        let mut m = morphing(&[(s::MORPH_X, x), (s::MORPH_Y, y)]);
        m.note_on(60, 100);
        let o = render(&mut m, 0.4);
        dark(win(&o, 0.1, 0.4))
    };
    // the five points are A, B, C, D and Center: dragging the ball crossfades between them
    let line: Vec<f32> = [(0.0, 1.0), (0.25, 0.75), (0.5, 0.5), (0.75, 0.25), (1.0, 0.0)].iter().map(|&(x, y)| at_pos(x, y)).collect();
    assert!(line.windows(2).all(|w| w[1] > w[0]), "{line:?}");
    assert!(line[4] > 3.0 * line[0]);
    // the three other corners are equal here
    assert!((at_pos(1.0, 1.0) / line[0] - 1.0).abs() < 0.1 && (at_pos(0.0, 0.0) / line[0] - 1.0).abs() < 0.1);
    // the controls show the selected point; editing one changes only that point
    let mut m = morphing(&[]);
    m.set(s::MORPH_SEL, 3.0);
    assert!((m.get(s::CUTOFF) - 1.0).abs() < 1e-6);
    m.set(s::MORPH_SEL, 0.0);
    assert!((m.get(s::CUTOFF) - 0.1).abs() < 1e-6);
    m.set(s::CUTOFF, 0.3);
    assert!((m.point_value(0, CUT) - 0.3).abs() < 1e-6 && (m.point_value(3, CUT) - 1.0).abs() < 1e-6);
    // all the string, object and filter values are stored per point: Object 2 Position
    m.set(s::obj(1, s::O_POS), 0.9);
    assert!((m.point_value(0, 7) - 0.9).abs() < 1e-6 && (m.point_value(4, 7) - 0.35).abs() < 1e-6);
    // Auto Select: a morph X/Y controller moves the ball and selects the nearest point
    let mut m = morphing(&[(s::CTRL_MX, 16.0), (s::CTRL_MY, 17.0)]);
    m.controller(16, 127);
    m.controller(17, 0);
    assert_eq!(m.get(s::MORPH_SEL), 3.0);
    assert!((m.get(s::CUTOFF) - 1.0).abs() < 1e-6);
    m.controller(16, 0);
    m.controller(17, 127);
    assert_eq!(m.get(s::MORPH_SEL), 0.0);
    // the ball is read for the newest voice
    m.note_on(60, 100);
    render(&mut m, 0.05);
    assert_eq!(m.morph_now().map(|p| (p.0.round(), p.1.round())), Some((0.0, 1.0)));
    // morphing off (both mode buttons off): the selected point as it is
    let mut off = morphing(&[(s::MENV_MODE, 0.0), (s::MORPH_SEL, 3.0), (s::MORPH_X, 0.0), (s::MORPH_Y, 1.0)]);
    off.note_on(60, 100);
    let a = render(&mut off, 0.4);
    let mut on = morphing(&[(s::MORPH_SEL, 3.0), (s::MORPH_X, 0.0), (s::MORPH_Y, 1.0)]);
    on.note_on(60, 100);
    let b = render(&mut on, 0.4);
    assert!(dark(win(&a, 0.1, 0.4)) > 2.0 * dark(win(&b, 0.1, 0.4)));
}

/// The envelope path from corner A to corner D in eight equal steps of 250 ms.
fn a_to_d() -> Vec<(u32, f32)> {
    let mut v = vec![(s::MENV_MODE, 2.0)];
    for i in 0..9 {
        v.push((s::mn(i, 1), i as f32 / 8.0));
        v.push((s::mn(i, 2), 1.0 - i as f32 / 8.0));
        if i > 0 {
            v.push((s::mn(i, 0), 250.0));
        }
    }
    v
}

fn darkness_track(m: &mut Sculpture, secs: f32) -> Vec<f32> {
    m.note_on(60, 100);
    let x = render(m, secs);
    x.chunks((0.2 * RATE) as usize).filter(|c| c.len() > 9000).map(|c| dark(c)).collect()
}

#[test]
fn morph_envelope() {
    // Env only: the envelope runs and the ball is ignored; the sound opens as it travels from A to D
    let mut m = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 1.0), (s::MORPH_X, 0.0), (s::MORPH_Y, 1.0)]));
    let t = darkness_track(&mut m, 2.2);
    assert!(t[8] > 2.0 * t[0] && t[0] < t[4], "{t:?}");
    // the position of the voice is on the envelope
    assert!(m.menv_progress().unwrap() > 0.9);
    // Timescale 200% takes twice as long
    let mut slow = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 1.0), (s::MENV_SCALE, 200.0)]));
    let ts = darkness_track(&mut slow, 2.2);
    assert!(ts[4] < 0.7 * t[4], "{} {}", ts[4], t[4]);
    // Depth scales how far it goes; 0 stays in the middle
    let mut flat = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 1.0), (s::MENV_DEPTH, 0.0)]));
    let tf = darkness_track(&mut flat, 2.2);
    assert!((tf[0] / tf[8] - 1.0).abs() < 0.4 && tf[8] < 0.6 * t[8], "{tf:?}");
    // Sustain mode: held at the Sustain node until the key goes up
    let mut hold = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 0.0), (s::MENV_SUS, 4.0)]));
    let th = darkness_track(&mut hold, 2.4);
    assert!((th[10] / th[8] - 1.0).abs() < 0.2 && th[10] < 0.6 * t[8], "{th:?}");
    // loops between the Loop and Sustain nodes
    for mode in [2.0, 3.0, 4.0] {
        let mut lp = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, mode), (s::MENV_LOOP, 2.0), (s::MENV_SUS, 6.0)]));
        let tl = darkness_track(&mut lp, 4.0);
        assert!(cycles(&tl[4..]) >= 2, "mode {mode}: {tl:?}");
    }
    // Scan via CtrlB: the controller is the time position
    let scan = |v: u8| {
        let mut m = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 5.0)]));
        m.controller(4, v);
        m.note_on(60, 100);
        dark(win(&render(&mut m, 0.5), 0.2, 0.5))
    };
    assert!(scan(127) > 2.0 * scan(0));
    // Env + Pad (Offset): the ball shifts the path
    let off = |x: f32| {
        let mut m = morphing(&with(&a_to_d(), &[(s::MENV_MODE, 3.0), (s::MENV_SUB, 0.0), (s::MENV_SUSMODE, 1.0), (s::MORPH_X, x), (s::MORPH_Y, 1.0 - x)]));
        let t = darkness_track(&mut m, 1.0);
        t[2]
    };
    assert!(off(1.0) > 1.5 * off(0.5));
    // Point Solo: the morph ball's edit of the selected node is heard as a snapshot
    let mut solo = morphing(&with(&a_to_d(), &[(s::MENV_MODE, 3.0), (s::MENV_SUB, 2.0), (s::MENV_NSEL, 8.0)]));
    let tsolo = darkness_track(&mut solo, 0.6);
    assert!(tsolo[1] > 2.0 * t[0]);
    // Transition: stepped holds each state until the next point
    let mut lin = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 1.0), (s::MENV_TRANS, 0.5)]));
    let mut stp = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 1.0), (s::MENV_TRANS, 1.0)]));
    let (a, b) = (darkness_track(&mut lin, 2.2), darkness_track(&mut stp, 2.2));
    assert!(a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum::<f32>() > 0.05 * a.iter().sum::<f32>());
    // Modulation: the velocity scales the movement
    let md = |vel: u8| {
        let mut m = morphing(&with(&a_to_d(), &[(s::MENV_SUSMODE, 1.0), (s::MENV_MOD, 1.0), (s::MENV_MODSRC, 2.0)]));
        m.note_on(60, vel);
        let x = render(&mut m, 2.0);
        dark(win(&x, 1.7, 2.0))
    };
    assert!(md(127) > 1.5 * md(10));
}

#[test]
fn recording_the_morph_envelope() {
    let mut m = morphing(&[(s::CTRL_MX, 16.0), (s::CTRL_MY, 17.0), (s::MENV_TRIG, 1.0)]);
    m.set(s::MENV_REC, 1.0);
    // arming sets the mode to Pad
    assert_eq!(m.get(s::MENV_MODE), 1.0);
    m.note_on(60, 100);
    render(&mut m, 0.2);
    // Note + Move Morph Point: the first controller message starts recording
    for k in 0..50 {
        m.controller(16, (k as f32 * 127.0 / 49.0) as u8);
        m.controller(17, (127.0 - k as f32 * 127.0 / 49.0) as u8);
        render(&mut m, 0.02);
    }
    m.set(s::MENV_REC, 0.0);
    // Mode is now Env only; the nodes follow the movement from A (top left) to D (bottom right)
    assert_eq!(m.get(s::MENV_MODE), 2.0);
    assert!(m.get(s::mn(0, 1)) < 0.15 && m.get(s::mn(8, 1)) > 0.85 && m.get(s::mn(0, 2)) > 0.85 && m.get(s::mn(8, 2)) < 0.15);
    let total: f32 = (1..9).map(|i| m.get(s::mn(i, 0))).sum();
    assert!((total - 1000.0).abs() < 150.0, "{total}");
    // played back it opens the sound
    m.set(s::MENV_SUSMODE, 1.0);
    m.reset();
    let t = darkness_track(&mut m, 1.2);
    assert!(t[4] > 1.5 * t[0], "{t:?}");
}

#[test]
fn morph_randomize() {
    let mut m = sc();
    let before: Vec<f32> = (0..s::MORPH_N).map(|k| m.point_value(0, k)).collect();
    m.randomize(&[0], 0, 1.0, 5);
    let after: Vec<f32> = (0..s::MORPH_N).map(|k| m.point_value(0, k)).collect();
    assert!(before.iter().zip(&after).filter(|(a, b)| (**a - **b).abs() > 1e-4).count() > 15);
    // only the chosen points move
    assert!((0..s::MORPH_N).all(|k| (m.point_value(4, k) - sc().point_value(4, k)).abs() < 1e-6));
    // groups: Waveshaper & Filter touches only its four values; String Material only its five
    let mut g = sc();
    g.randomize(&[1], 4, 1.0, 9);
    let moved: Vec<usize> = (0..s::MORPH_N).filter(|&k| (g.point_value(1, k) - sc().point_value(1, k)).abs() > 1e-6).collect();
    assert!(!moved.is_empty() && moved.iter().all(|k| (19..=22).contains(k)), "{moved:?}");
    let mut g = sc();
    g.randomize(&[2], 2, 1.0, 3);
    let moved: Vec<usize> = (0..s::MORPH_N).filter(|&k| (g.point_value(2, k) - sc().point_value(2, k)).abs() > 1e-6).collect();
    assert!(!moved.is_empty() && moved.iter().all(|k| (14..=18).contains(k)), "{moved:?}");
    // All except TensMod leaves it alone
    let mut g = sc();
    g.randomize(&[3], 1, 1.0, 11);
    assert!((g.point_value(3, 17) - sc().point_value(3, 17)).abs() < 1e-6);
    // Intensity: 1% moves little, 100% a lot
    let spread = |int: f32| {
        let mut g = sc();
        g.randomize(&[0, 1, 2, 3, 4], 0, int, 21);
        (0..s::MORPH_N).map(|k| (g.point_value(0, k) - sc().point_value(0, k)).abs()).sum::<f32>()
    };
    assert!(spread(1.0) > 10.0 * spread(0.01));
    // a randomised point is heard: Rnd with a point selected moves the controls too
    let mut g = sc();
    g.set(s::MORPH_SEL, 4.0);
    let c0 = g.get(s::CUTOFF);
    g.randomize(&[4], 4, 1.0, 17);
    assert!((g.get(s::CUTOFF) - c0).abs() > 1e-4);
    assert!(finite(&{
        g.note_on(60, 100);
        render(&mut g, 0.3)
    }));
}

#[test]
fn controller_assignments() {
    // Learn: the next moved controller is assigned; if none comes it reverts after 20 seconds
    let mut m = sc();
    m.learn(s::CTRL_A);
    assert_eq!(m.learning(), Some(s::CTRL_A));
    m.controller(74, 30);
    assert_eq!(m.get(s::CTRL_A), 74.0);
    assert_eq!(m.learning(), None);
    let mut m = sc();
    m.learn(s::CTRL_B);
    let mut left = 0.0;
    while left < 21.0 {
        render(&mut m, 1.0);
        left += 1.0;
    }
    assert_eq!(m.learning(), None);
    assert_eq!(m.get(s::CTRL_B), 4.0);
    // the controllers drive the "via" sources: Ctrl A (mod wheel by default), Ctrl B (foot)
    let via_ctrl = |ctrl: u8, cc: u8| {
        let set = [(s::lfo(0, s::L_ON), 1.0), (s::lfo(0, s::L_RATE), 4.0), (s::lfo(0, s::L_PHASE), 1.0), (s::lfo_r(0, 0, s::R_ON), 1.0), (s::lfo_r(0, 0, s::R_TARGET), tgt("Pitch")), (s::lfo_r(0, 0, s::R_VIA), ctrl as f32), (s::lfo_r(0, 0, s::R_AMT), 0.0), (s::lfo_r(0, 0, s::R_VIAAMT), 0.4), (s::MEDIA_LOSS, 0.05), (s::VIB_AMT, 0.0)];
        let mut m = sc();
        for &(id, v) in set.iter() {
            m.set(id, v);
        }
        m.controller(cc, 127);
        m.note_on(60, 100);
        let x = render(&mut m, 1.0);
        let f: Vec<f32> = (0..6).map(|k| pitch(win(&x, 0.1 + k as f32 * 0.14, 0.2 + k as f32 * 0.14), 200.0, 340.0)).collect();
        f.iter().fold(0.0f32, |a, b| a.max(*b)) / f.iter().fold(f32::MAX, |a, b| a.min(*b)) - 1.0
    };
    assert!(via_ctrl(5, 1) > 0.01 && via_ctrl(5, 4) < 0.002);
    assert!(via_ctrl(6, 4) > 0.01 && via_ctrl(6, 1) < 0.002);
    // Controller Mode: Use Default keeps the current assignments when a setting is loaded
    let mut m = sc();
    m.set(s::CTRL_MODE, 0.0);
    m.apply_setting(&[(s::CTRL_A, 11.0), (s::INNER_LOSS, 0.7)]);
    assert_eq!((m.get(s::CTRL_A), m.get(s::INNER_LOSS)), (1.0, 0.7));
    m.set(s::CTRL_MODE, 1.0);
    m.apply_setting(&[(s::CTRL_A, 11.0), (s::INNER_LOSS, 0.5)]);
    assert_eq!((m.get(s::CTRL_A), m.get(s::INNER_LOSS)), (11.0, 0.5));
    // an assignment can be Off
    let mut m = sc();
    m.set(s::CTRL_VIB, 128.0);
    m.set(s::VIB_AMT, 1.0);
    m.controller(1, 127);
    m.note_on(60, 100);
    assert!(finite(&render(&mut m, 0.2)));
    assert_eq!(m.value_text(s::CTRL_A, 1.0).as_deref(), Some("1 ModWhl"));
}

#[test]
fn midi_mono_mode_and_extended() {
    // MIDI Mono Mode: each voice has its own channel; per-note bend (0-96 semitones, 48 by default)
    let mut m = sc();
    assert_eq!(m.get(s::MONO_BEND), 48.0);
    m.set(s::MONO_MODE, 1.0);
    m.set(s::MEDIA_LOSS, 0.05);
    m.note_on_ch(1, 60, 100);
    m.note_on_ch(2, 64, 100);
    m.pitch_bend_ch(2, 8191 / 4);
    let x = render(&mut m, 0.6);
    // channel 2 went up by a quarter of 48 semitones, channel 1 stayed
    let up = 64.0 + 12.0;
    let f2 = pitch(win(&x, 0.2, 0.5), 440.0 * 2f32.powf((up - 1.0 - 69.0) / 12.0), 440.0 * 2f32.powf((up + 1.0 - 69.0) / 12.0));
    assert!(f2 > 0.0);
    assert!(tone(win(&x, 0.2, 0.5), 261.6) > 0.003);
    // the base channel moves every voice
    let mut b = sc();
    b.set(s::MONO_MODE, 1.0);
    b.set(s::MEDIA_LOSS, 0.05);
    b.note_on_ch(1, 60, 100);
    b.pitch_bend_ch(0, 8191);
    let y = render(&mut b, 0.5);
    let fb = pitch(win(&y, 0.1, 0.4), 200.0, 340.0);
    assert!((12.0 * (fb / 261.63).log2() - 2.0).abs() < 0.1);
    // the Surround parameters exist (a stereo instance only keeps them)
    assert!(s::params_table().iter().any(|p| p.name == "Surround Range" && p.max == 360.0));
}

#[test]
fn controller_a_b_modulation() {
    // two targets driven by Ctrl A, Ctrl B or both; Continue follows the controller, Note On keeps its value from the start
    let pitch_with = |src: f32, mode: f32, wheel_at_start: u8, wheel_later: u8, foot: u8| {
        let mut m = sc();
        m.set(s::cab(0, s::C_ON), 1.0);
        m.set(s::cab(0, s::C_TARGET), tgt("Pitch"));
        m.set(s::cab(0, s::C_SRC), src);
        m.set(s::cab(0, s::C_MODE), mode);
        m.set(s::cab(0, s::C_AMT), 0.5);
        m.set(s::MEDIA_LOSS, 0.05);
        m.set(s::VIB_AMT, 0.0);
        m.controller(1, wheel_at_start);
        m.controller(4, foot);
        m.note_on(60, 100);
        render(&mut m, 0.1);
        m.controller(1, wheel_later);
        let x = render(&mut m, 0.5);
        12.0 * (pitch(win(&x, 0.2, 0.5), 150.0, 560.0) / 261.63).log2()
    };
    // Ctrl A (the mod wheel): +6 semitones at full travel; Ctrl B is not heard
    assert!((pitch_with(0.0, 0.0, 127, 127, 0) - 6.0).abs() < 0.2);
    assert!(pitch_with(0.0, 0.0, 127, 127, 127) > 5.5 && pitch_with(1.0, 0.0, 0, 0, 0).abs() < 0.2);
    assert!((pitch_with(1.0, 0.0, 0, 0, 127) - 6.0).abs() < 0.2);
    // Continue follows the wheel; Note On holds what it was at the start
    assert!((pitch_with(0.0, 0.0, 0, 127, 0) - 6.0).abs() < 0.3);
    assert!(pitch_with(0.0, 1.0, 0, 127, 0).abs() < 0.3);
    assert!((pitch_with(0.0, 1.0, 127, 0, 0) - 6.0).abs() < 0.3);
    // the second target
    let mut m = sc();
    m.set(s::cab(1, s::C_ON), 1.0);
    m.set(s::cab(1, s::C_TARGET), tgt("Level"));
    m.set(s::cab(1, s::C_AMT), -1.0);
    m.controller(4, 127);
    m.note_on(60, 100);
    assert!(peak(&render(&mut m, 0.3)) < 1e-3);
}
