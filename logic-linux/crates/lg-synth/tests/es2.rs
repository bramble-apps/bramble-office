//! ES2: oscillators, filters, envelopes, LFOs, router, Planar Pad, Vector Envelope, macros,
//! controller assignments and randomizing behave as the guide says.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::es2::{self, Es2, VecEdit, VEC_POINTS};

/// A plain patch: oscillator 1 alone (the Triangle's pointer in its corner), no router, no
/// Analog, the same start phase every time, a flat level envelope.
fn base() -> Es2 {
    let mut s = Es2::new(RATE);
    for (id, v) in [(3, 0.0), (5, 1.0), (26, 0.5), (27, 1.0), (41, 0.0), (46, 0.0), (112, 1.0), (114, 0.0), (32, 1.0), (36, 1.0), (33, 0.0), (37, 0.0), (30, 0.0), (14, 2.0)] {
        s.set(id, v);
    }
    s
}

fn with(set: &[(u32, f32)]) -> Es2 {
    let mut s = base();
    for &(id, v) in set {
        s.set(id, v);
    }
    s
}

fn note(set: &[(u32, f32)], n: u8, secs: f32) -> Vec<f32> {
    let mut s = with(set);
    s.note_on(n, 100);
    render(&mut s, secs)
}

fn t(target: &str) -> f32 {
    es2::MM_TARGETS.iter().position(|x| *x == target).unwrap() as f32
}
fn src(source: &str) -> f32 {
    es2::MM_SOURCES.iter().position(|x| *x == source).unwrap() as f32
}
/// Router slot 1: source → target at an intensity (no via).
fn route(source: &str, target: &str, amount: f32) -> Vec<(u32, f32)> {
    vec![(41, t(target)), (42, src(source)), (43, 0.0), (44, amount), (45, amount), (147, 1.0)]
}

#[test]
fn stays_finite_everywhere() {
    sweep_all(&|| Box::new(Es2::new(RATE)));
}

#[test]
fn oscillators() {
    let h = |x: &[f32], n: f32| tone(&x[4800..], 220.0 * n);
    // the basic waves
    let sine = note(&[(14, 0.0)], 57, 0.5);
    let saw = note(&[(14, 2.0)], 57, 0.5);
    let rect = note(&[(14, 3.0)], 57, 0.5);
    let tri = note(&[(14, 1.0)], 57, 0.5);
    assert!(h(&sine, 2.0) < 0.01 * h(&sine, 1.0) && h(&saw, 2.0) > 0.3 * h(&saw, 1.0));
    assert!(h(&rect, 2.0) < 0.05 * h(&rect, 1.0) && h(&rect, 3.0) > 0.2 * h(&rect, 1.0));
    assert!(h(&tri, 3.0) < 0.2 * h(&tri, 1.0) && h(&tri, 3.0) > 0.05 * h(&tri, 1.0));
    // 100 Digiwaves on the Sine position: other spectra, each its own
    let d30 = note(&[(14, 0.0), (15, 30.0)], 57, 0.5);
    let d90 = note(&[(14, 0.0), (15, 90.0)], 57, 0.5);
    assert!(differs(&d30[4800..], &sine[4800..]) > 0.2 && differs(&d30[4800..], &d90[4800..]) > 0.2);
    // semitones and cents
    let f = fundamental(&note(&[(14, 1.0), (12, 7.0), (13, 50.0)], 57, 0.5)[4800..], 250.0, 420.0);
    assert!((f / (220.0 * 2f32.powf(7.5 / 12.0)) - 1.0).abs() < 0.003, "{f}");
    // FM: oscillator 2 modulates oscillator 1's frequency; more FM = more sidebands, and the
    // sine stays clean without it. Oscillator 2 may be silent in the mix.
    let fm = |amt: f32| bright(&note(&[(14, 4.0), (190, amt), (19, 0.0), (17, 0.0), (18, 0.0)], 57, 0.5)[4800..], 900.0);
    assert!(fm(1.0) > 5.0 * fm(0.2) && fm(0.2) > 5.0 * fm(0.0), "{} {} {}", fm(0.0), fm(0.2), fm(1.0));
    // only oscillator 2: pointer on the lower left corner
    let o2 = |set: &[(u32, f32)]| {
        let mut v = vec![(26, 0.0), (27, 0.0), (18, 0.0)];
        v.extend_from_slice(set);
        note(&v, 57, 0.5)
    };
    // ring: oscillator 2's rectangle times oscillator 1 (here a sine 7 semitones up): sum and
    // difference tones, no plain fundamental of either
    let ring = o2(&[(19, 6.0), (14, 0.0), (12, 7.0)]);
    let f1 = 220.0 * 2f32.powf(7.0 / 12.0);
    assert!(tone(&ring[4800..], f1 - 220.0) > 0.02 && tone(&ring[4800..], f1 + 220.0) > 0.02 && tone(&ring[4800..], 220.0) < 0.3 * tone(&ring[4800..], f1 + 220.0));
    // sync: oscillator 2 restarts with oscillator 1, so the pitch heard is oscillator 1's
    let sync = o2(&[(19, 5.0), (17, 19.0)]);
    assert!((fundamental(&sync[4800..], 150.0, 300.0) / 220.0 - 1.0).abs() < 0.01);
    let free = o2(&[(19, 2.0), (17, 19.0)]);
    assert!((fundamental(&free[4800..], 500.0, 800.0) / (220.0 * 2f32.powf(19.0 / 12.0)) - 1.0).abs() < 0.01);
    // the base pulse width of oscillators 2 and 3: from 50 % to a thin pulse (even harmonics appear)
    let wide = o2(&[(19, 3.0), (191, 0.0)]);
    let thin = o2(&[(19, 3.0), (191, 1.0)]);
    assert!(h(&wide, 2.0) < 0.05 * h(&wide, 1.0) && h(&thin, 2.0) > 0.5 * h(&thin, 1.0));
    // noise on oscillator 3, coloured by Osc3Wave: negative = darker, positive = brighter
    let noise = |amt: f32| {
        let mut v = vec![(26, 1.0), (27, 0.0), (24, 6.0)];
        v.extend(route("Max", "Osc3Wave", amt));
        let x = note(&v, 57, 0.5);
        let x = &x[4800..];
        let hi = highpassed(x, 400.0);
        let low: Vec<f32> = x.iter().zip(&hi).map(|(a, b)| a - b).collect();
        (bright(x, 3000.0), rms(&low) / rms(x))
    };
    let (dark, white, blue) = (noise(-0.6), noise(0.0), noise(0.6));
    assert!(dark.0 < 0.2 * white.0 && blue.1 < 0.8 * white.1, "{dark:?} {white:?} {blue:?}");
    // an oscillator switched off is silent, the others carry on
    let off = note(&[(11, 0.0)], 57, 0.3);
    assert!(rms(&off) < 1e-4);
    // Osc Start: soft and hard give the same start every time (zero crossing / peak), free doesn't
    let start = |mode: f32| {
        let run = || {
            let mut s = Es2::new(RATE);
            for (id, v) in [(3, 0.0), (5, mode), (14, 0.0), (27, 1.0), (32, 1.0), (36, 1.0), (41, 0.0), (46, 0.0), (108, 0.0), (109, 0.0)] {
                s.set(id, v);
            }
            s.note_on(57, 100);
            render(&mut s, 0.05)
        };
        (run(), run())
    };
    let (a, b) = start(1.0);
    assert!(differs(&a, &b) < 1e-6);
    let (a2, b2) = start(2.0);
    assert!(differs(&a2, &b2) < 1e-6 && differs(&a, &a2) > 0.5, "soft and hard start elsewhere in the cycle");
    let mut s = with(&[(5, 0.0), (14, 0.0), (108, 0.0), (109, 0.0), (113, 0.0)]);
    let mut starts = Vec::new();
    for _ in 0..3 {
        s.note_on(57, 100);
        starts.push(render(&mut s, 0.05));
        s.note_off(57);
        render(&mut s, 0.3);
    }
    assert!(differs(&starts[0], &starts[1]) > 0.1 || differs(&starts[0], &starts[2]) > 0.1, "free: a random phase");
}

#[test]
fn triangle_and_sine_level() {
    // each corner: one oscillator alone; along a side the third is silent
    let lv = |x: f32, y: f32| {
        let s = note(&[(26, x), (27, y), (14, 0.0), (19, 0.0), (24, 0.0), (12, 0.0), (17, 12.0), (22, 24.0), (18, 0.0), (23, 0.0)], 45, 0.5);
        [tone(&s[4800..], 110.0), tone(&s[4800..], 220.0), tone(&s[4800..], 440.0)]
    };
    let c1 = lv(0.5, 1.0);
    assert!(c1[1] < 0.01 * c1[0] && c1[2] < 0.01 * c1[0]);
    let c2 = lv(0.0, 0.0);
    assert!(c2[0] < 0.01 * c2[1] && c2[2] < 0.01 * c2[1]);
    let c3 = lv(1.0, 0.0);
    assert!(c3[0] < 0.01 * c3[2] && c3[1] < 0.01 * c3[2]);
    let side = lv(0.5, 0.0);
    assert!(side[0] < 0.01 * side[1] && (side[1] / side[2] - 1.0).abs() < 0.1, "between 2 and 3: 1 is muted");
    let mid = lv(0.5, 1.0 / 3.0);
    assert!((mid[0] / mid[1] - 1.0).abs() < 0.1 && (mid[1] / mid[2] - 1.0).abs() < 0.1, "the centre: equal levels");
    // Sine Level: a sine at oscillator 1's pitch past the filters, even with the oscillator off
    let s = note(&[(11, 0.0), (39, 1.0), (32, 0.0), (36, 0.0)], 57, 0.5);
    assert!(tone(&s[4800..], 220.0) > 0.05 && tone(&s[4800..], 440.0) < 0.01 * tone(&s[4800..], 220.0));
}

#[test]
fn filters() {
    let br = |set: &[(u32, f32)], hz: f32| bright(&note(set, 45, 0.5)[4800..], hz);
    // series, blend +1: only Filter 2; −1: only Filter 1
    let f2_only = |cut: f32| br(&[(30, 1.0), (36, cut), (32, 0.0)], 2000.0);
    assert!(f2_only(1.0) > 20.0 * f2_only(0.3), "Filter 2 alone sets the tone at blend +1");
    let f1_only = |cut: f32| br(&[(30, -1.0), (32, cut), (36, 0.0)], 2000.0);
    assert!(f1_only(1.0) > 20.0 * f1_only(0.3), "Filter 1 alone at blend −1");
    // series at blend 0: both in a row (darker than either alone)
    let both = br(&[(30, 0.0), (32, 0.4), (36, 0.4)], 1500.0);
    let one = br(&[(30, 1.0), (32, 0.4), (36, 0.4), (34, 0.0)], 1500.0);
    assert!(both < one);
    // parallel: the two outputs are mixed, so a closed Filter 1 doesn't block an open Filter 2
    let par = br(&[(29, 1.0), (30, 0.0), (32, 0.0), (36, 1.0)], 2000.0);
    let ser = br(&[(29, 0.0), (30, 0.0), (32, 0.0), (36, 1.0)], 2000.0);
    assert!(par > 20.0 * ser);
    // Filter 1 modes at a cutoff near 630 Hz on a 110 Hz sawtooth
    let m = |mode: f32| {
        let x = note(&[(30, -1.0), (31, mode), (32, 0.5), (33, 0.3)], 45, 0.5);
        (tone(&x[4800..], 110.0), tone(&x[4800..], 660.0), tone(&x[4800..], 3520.0))
    };
    let (lo, hi, pk, brj, bp) = (m(0.0), m(1.0), m(2.0), m(3.0), m(4.0));
    assert!(lo.2 < 0.2 * hi.2 && hi.0 < 0.2 * lo.0, "Lo passes the bass, Hi the top");
    assert!(bp.0 < 0.3 * lo.0 && bp.2 < 0.3 * hi.2 && bp.1 > 0.5 * lo.1, "BP keeps the band");
    assert!(brj.1 < 0.5 * lo.1 && brj.0 > 0.7 * lo.0 && brj.2 > 0.7 * hi.2, "BR cuts the band");
    assert!(pk.1 > 1.5 * lo.1 && pk.0 > 0.7 * lo.0 && pk.2 > 0.7 * hi.2, "Peak boosts the band and keeps the rest");
    // Filter 2 slopes: 12, 18, 24 dB; Fat keeps the bass at high resonance
    let slope = |s: f32| br(&[(30, 1.0), (36, 0.45), (34, s)], 3000.0);
    assert!(slope(2.0) < slope(1.0) && slope(1.0) < slope(0.0));
    let low = |fat: f32, res: f32| tone(&note(&[(30, 1.0), (36, 0.5), (34, 2.0), (35, fat), (37, res)], 45, 0.5)[4800..], 110.0);
    assert!(low(0.0, 0.8) / low(0.0, 0.0) < 0.6 && low(1.0, 0.8) / low(1.0, 0.0) > 1.5 * low(0.0, 0.8) / low(0.0, 0.0));
    // Drive distorts (a triangle gains harmonics); at 0 it does nothing
    let drv = |d: f32| br(&[(14, 1.0), (28, d)], 1500.0);
    assert!(drv(1.0) > 5.0 * drv(0.0));
    // Filter FM: the sine at oscillator 1's frequency moves Filter 2's cutoff at audio rate
    let fm = note(&[(30, 1.0), (36, 0.5), (37, 0.5), (38, 1.0)], 45, 0.5);
    let nofm = note(&[(30, 1.0), (36, 0.5), (37, 0.5), (38, 0.0)], 45, 0.5);
    assert!(differs(&fm[4800..], &nofm[4800..]) > 0.2);
    // full resonance, oscillators muted: silence, until Filter Reset gives each note a trigger
    let so = |reset: f32| note(&[(11, 0.0), (30, 1.0), (36, 0.5), (37, 1.0), (4, reset), (34, 2.0)], 57, 0.6);
    assert!(rms(&so(0.0)[9600..]) < 1e-4);
    let x = so(1.0);
    assert!(rms(&x[9600..]) > 0.01, "self-oscillation: {}", rms(&x[9600..]));
    let fq = fundamental(&x[9600..], 200.0, 2500.0);
    assert!(fq > 400.0 && fq < 900.0, "a sine near the cutoff: {fq}");
    // the Filter button off: the oscillators pass unfiltered
    assert!(br(&[(115, 0.0), (32, 0.0), (36, 0.0)], 2000.0) > 0.5 * br(&[(115, 1.0), (32, 1.0), (36, 1.0)], 2000.0));
}

#[test]
fn envelopes() {
    let seg = |x: &[f32], a: f32, b: f32| rms(&x[(a * RATE) as usize..(b * RATE) as usize]);
    // ENV 3 is the level: attack split by velocity (one value at soft, one at hard)
    let att = |vel: u8| {
        let mut s = with(&[(108, 0.8), (109, 0.0)]);
        s.note_on(57, vel);
        let x = render(&mut s, 1.0);
        seg(&x, 0.0, 0.05) / seg(&x, 0.9, 1.0)
    };
    assert!(att(127) > 0.8 && att(1) < 0.2, "{} {}", att(127), att(1));
    // decay to the sustain level, release
    let x = note(&[(110, 0.5), (112, 0.25)], 57, 1.5);
    let full = note(&[(110, 0.5), (112, 1.0)], 57, 1.5);
    assert!((seg(&x, 1.2, 1.5) / seg(&full, 1.2, 1.5) - 0.25).abs() < 0.03);
    assert!(seg(&x, 0.005, 0.02) > 2.0 * seg(&x, 1.2, 1.5), "it starts at the top and decays");
    // Sustain Time: centre holds, lower half falls (lower = faster), upper half rises
    let st = |v: f32| {
        let x = note(&[(110, 0.3), (112, 0.5), (111, v)], 57, 3.0);
        let hold = note(&[(110, 0.3), (112, 0.5), (111, 0.0)], 57, 3.0);
        seg(&x, 2.8, 3.0) / seg(&hold, 2.8, 3.0)
    };
    let held = note(&[(110, 0.3), (112, 0.5), (111, 0.0)], 57, 3.0);
    assert!((seg(&held, 2.8, 3.0) / seg(&held, 0.5, 0.7) - 1.0).abs() < 0.05, "hold");
    assert!(st(-0.5) < 0.2 && st(-0.2) > st(-0.5), "fall: {} {}", st(-0.5), st(-0.2));
    assert!(st(0.5) > 1.6 && st(0.5) > st(0.2), "rise: {} {}", st(0.5), st(0.2));
    // Velocity Sensitivity
    let lv = |sens: f32, vel: u8| {
        let mut s = with(&[(114, sens)]);
        s.note_on(57, vel);
        rms(&render(&mut s, 0.3))
    };
    assert!((lv(0.0, 20) / lv(0.0, 127) - 1.0).abs() < 0.02 && lv(1.0, 20) < 0.3 * lv(1.0, 127));
    let half = lv(1.0, 64) / lv(1.0, 127);
    assert!((half - 0.5).abs() < 0.05, "full sensitivity: half velocity, half level ({half})");
    // ENV 1 to the cutoff: D decays while the key is held, R holds until release
    let e1 = |mode: f32| {
        let mut v = vec![(30, 1.0), (36, 0.2), (96, mode), (100, 0.72)];
        v.extend(route("ENV1", "Cutoff 2", 0.6));
        let x = note(&v, 45, 1.5);
        (hi_rms(&x[480..2880], 800.0), hi_rms(&x[62400..], 800.0))
    };
    let (d, r) = (e1(0.0), e1(1.0));
    assert!(d.0 > 5.0 * d.1 && r.1 > 0.5 * r.0, "{d:?} {r:?}");
    // ENV 1 trigger modes: Poly restarts per voice; Mono only after all keys are up; Retrig with every key
    let trig = |mode: f32| {
        let mut v = vec![(30, 1.0), (36, 0.2), (96, 0.0), (100, 0.6), (97, mode)];
        v.extend(route("ENV1", "Cutoff 2", 0.6));
        let mut s = with(&v);
        s.note_on(45, 100);
        render(&mut s, 1.5);
        s.note_on(52, 100);
        let x = render(&mut s, 0.05);
        hi_rms(&x, 1000.0)
    };
    let (poly, mono, retrig) = (trig(0.0), trig(1.0), trig(2.0));
    assert!(mono < 0.2 * poly && retrig > 0.5 * poly, "{poly} {mono} {retrig}");
    // ENV 2 and ENV 3 are both router sources; their times are (scaled) targets
    let mut v = vec![(110, 0.45), (112, 0.0)];
    v.extend(route("Velo", "Env3Dec", 1.0));
    let dec = |vel: u8| {
        let mut s = with(&v);
        s.note_on(57, vel);
        let x = render(&mut s, 1.0);
        seg(&x, 0.3, 0.4) / seg(&x, 0.0, 0.02)
    };
    assert!(dec(127) > 5.0 * dec(1), "velocity lengthens the decay: {} {}", dec(127), dec(1));
    assert!((es2::scaled(1.0) - 10.0).abs() < 1e-4 && (es2::scaled(-1.0) - 0.04).abs() < 1e-4 && es2::scaled(0.0) == 1.0);
}

#[test]
fn lfos() {
    let vib = |x: &[f32]| tone(x, 220.0) / rms(x);
    // LFO 1 on the pitch; its envelope fades it in (positive) or out (negative)
    let run = |eg: f32| {
        let mut v = vec![(14, 1.0), (92, 6.0), (93, eg)];
        v.extend(route("LFO1", "Pitch123", 0.3));
        note(&v, 57, 3.0)
    };
    let (plain, fin, fout) = (run(0.0), run(0.5), run(-0.5));
    assert!(vib(&plain[..24000]) < 0.6 * vib(&note(&[(14, 1.0)], 57, 0.5)));
    assert!(vib(&fin[2400..19200]) > 1.5 * vib(&fin[120000..]), "fades in: steady at first");
    assert!(vib(&fout[2400..19200]) < 0.7 * vib(&fout[120000..]), "fades out: steady in the end");
    // LFO 1 is per voice and key-synced: a second note later has its own phase (the two
    // voices' vibratos are not in step), while LFO 2 moves all voices alike
    let two = |source: &str| {
        let mut v = vec![(14, 0.0), (92, 2.0), (95, 0.46), (91, 2.0), (94, 2.0)];
        v.extend(route(source, "Amp", 1.0));
        let mut s = with(&v);
        s.note_on(57, 100);
        render(&mut s, 0.13);
        s.note_on(74, 100);
        let x = render(&mut s, 2.0);
        // how often both notes are loud together
        let (mut both, mut any) = (0, 0);
        for c in x.chunks(960) {
            let (a, b) = (tone(c, 220.0) > 0.05, tone(c, 587.33) > 0.05);
            if a || b {
                any += 1;
            }
            if a && b {
                both += 1;
            }
        }
        both as f32 / any.max(1) as f32
    };
    assert!(two("LFO2") > 0.9 && two("LFO1") < 0.8, "{} {}", two("LFO2"), two("LFO1"));
    // LFO 2 Rate: free in the upper half, synced to the tempo in the lower (twice the tempo, twice as fast)
    let count = |rate: f32, bpm: f64| {
        let mut v = vec![(14, 1.0), (95, rate), (94, 2.0)];
        v.extend(route("LFO2", "Amp", 1.0));
        let mut s = with(&v);
        s.set_tempo(bpm);
        s.note_on(57, 100);
        let x = render(&mut s, 4.0);
        let env: Vec<f32> = x.chunks(480).map(rms).collect();
        let mid = env.iter().cloned().fold(0.0, f32::max) * 0.5;
        env.windows(2).filter(|w| w[0] < mid && w[1] >= mid).count()
    };
    assert!(count(0.6, 120.0) > count(0.4, 120.0) && count(0.6, 60.0) == count(0.6, 120.0));
    let (slow, fast) = (count(-0.3, 60.0), count(-0.3, 120.0));
    assert!(slow >= 1 && (fast as f32 / slow as f32 - 2.0).abs() < 0.6, "{slow} {fast}");
    // the unipolar rectangle switches between a positive value and zero: the pitch only goes up
    let pitch = |wave: f32| {
        let mut v = vec![(14, 1.0), (92, 2.0), (91, wave)];
        v.extend(route("LFO1", "Pitch123", 0.28));
        let x = note(&v, 57, 2.0);
        let f: Vec<f32> = x[4800..].chunks(2400).map(|c| fundamental(c, 180.0, 270.0)).collect();
        (f.iter().cloned().fold(1e9, f32::min), f.iter().cloned().fold(0.0, f32::max))
    };
    let (lo, hi) = pitch(3.0);
    assert!(lo > 218.0 && hi > 230.0, "unipolar: {lo} {hi}");
    let (lo, hi) = pitch(2.0);
    assert!(lo < 210.0 && hi > 230.0, "bipolar: {lo} {hi}");
}

#[test]
fn router() {
    // the pitch scale: 8 → 10 cents, 20 → 50, 28 → a semitone, 36 → two, 76 → an octave, 100 → three octaves
    for (i, c) in [(0.08, 10.0), (0.20, 50.0), (0.28, 100.0), (0.36, 200.0), (0.76, 1200.0), (1.0, 3600.0)] {
        assert!((es2::pitch_cents(i) - c).abs() < 0.01, "{i}");
    }
    assert!((es2::pitch_cents(0.04) - 5.0).abs() < 0.01 && (es2::pitch_cents(-0.28) + 100.0).abs() < 0.01);
    let f0 = |set: Vec<(u32, f32)>, lo: f32, hi: f32| fundamental(&note(&set, 57, 0.5)[4800..], lo, hi);
    let mut v = vec![(14, 1.0)];
    v.extend(route("Max", "Pitch123", 0.76));
    assert!((f0(v, 300.0, 600.0) / 440.0 - 1.0).abs() < 0.005, "Max is a constant +1");
    // via: the lower value at the via source's minimum, the upper at its maximum
    let mut s = with(&[(14, 1.0), (41, t("Pitch123")), (42, src("Max")), (43, src("ModWhl")), (44, 0.0), (45, 0.28), (147, 1.0)]);
    s.note_on(57, 100);
    let a = fundamental(&render(&mut s, 0.4)[4800..], 150.0, 300.0);
    s.controller(1, 127);
    let b = fundamental(&render(&mut s, 0.4)[4800..], 150.0, 300.0);
    assert!((a / 220.0 - 1.0).abs() < 0.003 && (b / 233.08 - 1.0).abs() < 0.003, "{a} {b}");
    // via Whl+To: the wheel or aftertouch
    let mut s = with(&[(14, 1.0), (41, t("Pitch123")), (42, src("Max")), (43, src("Whl+To")), (44, 0.0), (45, 0.28), (147, 1.0)]);
    s.controller(1, 127);
    s.note_on(57, 100);
    assert!((fundamental(&render(&mut s, 0.4)[4800..], 150.0, 300.0) / 233.08 - 1.0).abs() < 0.003);
    // the bypass button keeps the routing but turns it off
    let mut v = vec![(14, 1.0)];
    v.extend(route("Max", "Pitch123", 0.76));
    v.push((147, 0.0));
    assert!((f0(v, 150.0, 300.0) / 220.0 - 1.0).abs() < 0.003);
    // Kybd on Cut 1+2 at 0.5: the cutoff follows the keys (a self-oscillating filter plays in tune-ish)
    let so = |n: u8| {
        let mut v = vec![(11, 0.0), (30, 1.0), (36, 0.5), (37, 1.0), (4, 1.0), (34, 2.0)];
        v.extend(route("Kybd", "Cut 1+2", 0.5));
        fundamental(&note(&v, n, 0.6)[9600..], 100.0, 4000.0)
    };
    let ratio = so(72) / so(60);
    assert!((ratio - 2.0).abs() < 0.2, "an octave up, the cutoff an octave up: {ratio}");
    // Bender works as a source even with a bend range of 0
    let mut v = vec![(14, 2.0), (6, 0.0), (167, 0.0)];
    v.extend(route("Bender", "Cutoff 2", 0.5));
    v.extend([(30, 1.0), (36, 0.3)]);
    let mut s = with(&v);
    s.note_on(45, 100);
    let a = render(&mut s, 0.3);
    s.pitch_bend(8191);
    let b = render(&mut s, 0.3);
    let fb = fundamental(&b[4800..], 80.0, 160.0);
    assert!((fb / 110.0 - 1.0).abs() < 0.005 && hi_rms(&b[4800..], 1500.0) > 5.0 * hi_rms(&a[4800..], 1500.0), "{fb} {} {}", hi_rms(&b[4800..], 1500.0), hi_rms(&a[4800..], 1500.0));
    // Touch, RndNO1 (a new value per note), SideCh (the side chain's level)
    let mut v = vec![(14, 1.0)];
    v.extend(route("Touch", "Amp", -1.0));
    let mut s = with(&v);
    s.note_on(57, 100);
    assert!(rms(&render(&mut s, 0.2)) > 0.01);
    let mut v = vec![(14, 1.0)];
    v.extend(route("RndNO1", "Pitch123", 0.36));
    let mut s = with(&v);
    let mut fs = Vec::new();
    for _ in 0..4 {
        s.note_on(57, 100);
        fs.push(fundamental(&render(&mut s, 0.3)[4800..], 170.0, 280.0));
        s.note_off(57);
        render(&mut s, 0.5);
    }
    assert!(fs.iter().any(|f| (f - fs[0]).abs() > 1.0), "{fs:?}");
    let mut v = vec![(14, 2.0), (30, 1.0), (36, 0.3)];
    v.extend(route("SideCh", "Cutoff 2", 0.6));
    let mut s = with(&v);
    s.note_on(45, 100);
    let quiet = render(&mut s, 0.3);
    let mut loud = Vec::new();
    for _ in 0..60 {
        let sc = vec![0.5f32; 256];
        s.side_chain(&sc, &sc);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        s.render(&mut l, &mut r);
        loud.extend(l);
    }
    assert!(hi_rms(&loud[4800..], 1500.0) > 5.0 * hi_rms(&quiet[4800..], 1500.0));
    // OscLScle: −1 mutes the oscillators; Osc1Levl; FltBlend; Pan
    let mut v = vec![(14, 1.0)];
    v.extend(route("Max", "OscLScle", -0.34));
    assert!(rms(&note(&v, 57, 0.3)) < 1e-3);
    let mut v = vec![(14, 1.0)];
    v.extend(route("Max", "Pan", 1.0));
    let mut s = with(&v);
    s.note_on(57, 100);
    let (l, r) = render_st(&mut s, 0.3);
    assert!(rms(&l) < 0.01 * rms(&r));
    // the Mod macro scales every routing's depth
    let mut v = vec![(14, 1.0), (177, -1.0)];
    v.extend(route("Max", "Pitch123", 0.76));
    assert!((f0(v, 150.0, 300.0) / 220.0 - 1.0).abs() < 0.003);
}

#[test]
fn planar_pad_and_vector_envelope() {
    // the pad's axes: X to Cutoff 2 (bipolar), with intensity and polarity
    let br = |x: f32, int: f32| hi_rms(&note(&[(30, 1.0), (36, 0.4), (124, t("Cutoff 2")), (125, int), (122, x)], 45, 0.4)[4800..], 800.0);
    assert!(br(1.0, 0.5) > 5.0 * br(0.0, 0.5) && br(-1.0, 0.5) < 0.3 * br(0.0, 0.5) && br(1.0, -0.5) < 0.3 * br(0.0, 0.5));
    // ---- the Vector Envelope: three points, pad X going −1 → +1 → −1 over half a second each
    let pts = |v: &mut Vec<(u32, f32)>, list: &[(f32, f32)]| {
        v.push((135, (list.len() - 1) as f32));
        for (k, (time, x)) in list.iter().enumerate() {
            v.push((VEC_POINTS + k as u32 * 5, *time));
            v.push((VEC_POINTS + k as u32 * 5 + 3, *x));
        }
    };
    let mk = |list: &[(f32, f32)], extra: &[(u32, f32)]| {
        let mut v = vec![(142, 2.0), (134, 0.0), (136, 1.0), (140, 0.0), (138, 0.0), (168, 10.0)];
        pts(&mut v, list);
        v.extend_from_slice(extra);
        with(&v)
    };
    let three = [(0.0, -1.0), (0.5, 1.0), (0.5, -1.0)];
    let x_at = |s: &mut Es2, secs: f32| {
        render(s, secs);
        s.vector_pos().map(|p| p[2]).unwrap_or(f32::NAN)
    };
    // travels to the sustain point, holds there while the key is down, then goes on to the end
    let mut s = mk(&three, &[]);
    s.note_on(57, 100);
    assert!((x_at(&mut s, 0.25) - 0.0).abs() < 0.05, "half way");
    assert!((x_at(&mut s, 0.5) - 1.0).abs() < 0.02 && (x_at(&mut s, 1.0) - 1.0).abs() < 0.02, "held at the sustain point");
    s.set(113, 0.9);
    s.note_off(57);
    assert!((x_at(&mut s, 0.25) - 0.0).abs() < 0.06 && (x_at(&mut s, 0.5) + 1.0).abs() < 0.02, "the release runs to the end point");
    // released early: the release starts from where the pointer is
    let mut s = mk(&three, &[(113, 0.9)]);
    s.note_on(57, 100);
    render(&mut s, 0.25);
    s.note_off(57);
    let x = x_at(&mut s, 0.25);
    assert!(x < -0.3, "from 0 toward −1 in the release: {x}");
    // Finish: plays every point to the end whatever the key does, the sustain point ignored
    let mut s = mk(&three, &[(137, 1.0)]);
    s.note_on(57, 100);
    assert!((x_at(&mut s, 0.75) - 0.0).abs() < 0.06 && (x_at(&mut s, 0.5) + 1.0).abs() < 0.02);
    // Time Scaling: 50 % is twice as fast, 200 % half as fast
    let mut s = mk(&three, &[(128, 0.5)]);
    s.note_on(57, 100);
    assert!((x_at(&mut s, 0.125) - 0.0).abs() < 0.06);
    let mut s = mk(&three, &[(128, 2.0)]);
    s.note_on(57, 100);
    assert!((x_at(&mut s, 0.5) - 0.0).abs() < 0.06);
    // curves: hold+step jumps at the end of the transition, step+hold at its start
    let mut s = mk(&three, &[(168, 0.0)]);
    s.note_on(57, 100);
    assert!((x_at(&mut s, 0.4) + 1.0).abs() < 0.02 && (x_at(&mut s, 0.15) - 1.0).abs() < 0.02);
    let mut s = mk(&three, &[(168, 20.0)]);
    s.note_on(57, 100);
    assert!((x_at(&mut s, 0.05) - 1.0).abs() < 0.02);
    assert!(es2::curve(15, 0.5) > 0.6 && es2::curve(5, 0.5) < 0.4 && (es2::curve(10, 0.5) - 0.5).abs() < 1e-6);
    // ---- loops: four points, the loop point 1, the sustain point 3 (L before S: loops while held)
    let four = [(0.0, -1.0), (0.2, 0.0), (0.2, 0.5), (0.2, 1.0), (0.2, -1.0)];
    let lp = |mode: f32, extra: &[(u32, f32)]| {
        let mut e = vec![(136, 3.0), (140, 1.0), (138, mode)];
        e.extend_from_slice(extra);
        let mut s = mk(&four, &e);
        s.note_on(57, 100);
        s
    };
    // Forward: 0 → 0.5 → 1, back to 0 at once, again
    let mut s = lp(1.0, &[]);
    assert!((x_at(&mut s, 0.6) - 1.0).abs() < 0.05, "reaches the sustain point");
    assert!((x_at(&mut s, 0.2) - 0.5).abs() < 0.05 && (x_at(&mut s, 0.19) - 1.0).abs() < 0.08 && (x_at(&mut s, 0.21) - 0.5).abs() < 0.08, "forward loop");
    // Backward: from the sustain point down to the loop point, then again from the top
    let mut s = lp(2.0, &[]);
    render(&mut s, 0.6);
    assert!((x_at(&mut s, 0.1) - 0.75).abs() < 0.05 && (x_at(&mut s, 0.2) - 0.25).abs() < 0.05 && (x_at(&mut s, 0.2) - 0.75).abs() < 0.08, "backward loop");
    // Alternate: back and forth
    let mut s = lp(3.0, &[]);
    render(&mut s, 0.6);
    assert!((x_at(&mut s, 0.4) - 0.0).abs() < 0.05 && (x_at(&mut s, 0.2) - 0.5).abs() < 0.05 && (x_at(&mut s, 0.2) - 1.0).abs() < 0.05, "alternate loop");
    // the key's release leaves the loop for the end point
    s.set(113, 0.9);
    s.note_off(57);
    assert!((x_at(&mut s, 0.3) + 1.0).abs() < 0.02);
    // Loop Count 2: two turns, then it stays at the sustain point
    let mut s = lp(1.0, &[(139, 2.0)]);
    render(&mut s, 0.6 + 0.8);
    assert!((x_at(&mut s, 0.3) - 1.0).abs() < 0.02 && (x_at(&mut s, 0.5) - 1.0).abs() < 0.02, "after two loops");
    // Loop Rate: "as set" follows the point times; a free rate sets the cycle length itself
    let mut s = lp(1.0, &[(132, 0.75)]);
    render(&mut s, 0.6);
    let hz = lg_synth::synthkit::bipolar_rate(0.75, 30.0, 120.0).0;
    let cyc = 1.0 / hz;
    assert!(cyc < 0.3, "{cyc}");
    let a = x_at(&mut s, cyc * 0.5);
    let b = x_at(&mut s, cyc);
    assert!((a - 0.5).abs() < 0.12 && (b - 0.5).abs() < 0.12, "one cycle later, the same place: {a} {b}");
    // a synced rate follows the tempo
    let mut s = lp(1.0, &[(132, -0.5)]);
    s.set_tempo(120.0);
    assert!(s.value_text(132, -0.5).is_some_and(|t| !t.contains("Hz")) && s.value_text(132, 0.0).as_deref() == Some("as set"));
    // Loop Smooth: the jump back becomes a glide (some time after the loop's end the pointer is between)
    let mut s = lp(1.0, &[(131, 1.0)]);
    render(&mut s, 0.6);
    let mid = x_at(&mut s, 0.1);
    assert!(mid > 0.1 && mid < 0.9, "smoothed: {mid}");
    // L after S: holds at the sustain point while the key is down, loops after the release
    let mut s = mk(&four, &[(136, 1.0), (140, 3.0), (138, 1.0), (113, 1.0), (112, 1.0)]);
    s.note_on(57, 100);
    assert!((x_at(&mut s, 0.6) - 0.0).abs() < 0.02, "held at S");
    s.note_off(57);
    assert!((x_at(&mut s, 0.4) - 1.0).abs() < 0.05 && (x_at(&mut s, 0.2) - 0.5).abs() < 0.08 && (x_at(&mut s, 0.2) - 1.0).abs() < 0.08, "looping in the release");
    // ---- Vector Mode and Solo Point
    // Mix: the envelope moves the Triangle (oscillator 1 fades to oscillator 2); the pad stays manual
    let mut v = vec![(142, 1.0), (134, 0.0), (136, 2.0), (135, 2.0), (14, 0.0), (19, 0.0), (17, 12.0), (18, 0.0), (168, 10.0)];
    for (k, (time, mx, my)) in [(0.0, 0.0, 1.0), (0.5, 0.0, 0.0), (0.5, 0.0, 0.0)].into_iter().enumerate() {
        v.extend([(VEC_POINTS + k as u32 * 5, time), (VEC_POINTS + k as u32 * 5 + 1, mx), (VEC_POINTS + k as u32 * 5 + 2, my)]);
    }
    let mut s = with(&v);
    s.note_on(57, 100);
    let a = render(&mut s, 0.1);
    render(&mut s, 0.5);
    let b = render(&mut s, 0.2);
    assert!(tone(&a, 220.0) > 5.0 * tone(&a, 440.0) && tone(&b, 440.0) > 5.0 * tone(&b, 220.0), "the Triangle moved from oscillator 1 to 2");
    // off: manual positions only
    let mut s = with(&v);
    s.set(142, 0.0);
    s.note_on(57, 100);
    render(&mut s, 0.7);
    let c = render(&mut s, 0.2);
    assert!(tone(&c, 220.0) > 5.0 * tone(&c, 440.0));
    // Solo Point: no movement, the selected point's positions are in effect
    let mut s = with(&v);
    s.set(134, 1.0);
    s.set(133, 1.0);
    s.note_on(57, 100);
    let d = render(&mut s, 0.2);
    assert!(tone(&d[2400..], 440.0) > 5.0 * tone(&d[2400..], 220.0));
    // every voice has its own envelope, restarted with each note
    let mut s = mk(&three, &[]);
    s.note_on(57, 100);
    render(&mut s, 0.5);
    s.note_on(64, 100);
    assert!((x_at(&mut s, 0.25) - 0.0).abs() < 0.06, "the new voice is half way while the first holds");
}

#[test]
fn vector_envelope_edits() {
    let s = with(&[(135, 3.0), (136, 2.0), (140, 1.0), (200 + 5, 0.4), (200 + 10, 0.6), (200 + 15, 1.0), (200 + 8, -0.5), (200 + 13, 0.5)]);
    let get = |id: u32| s.get(id);
    let apply = |edit: VecEdit| {
        let mut s2 = with(&[]);
        for id in 0..es2::NP as u32 {
            s2.set(id, s.get(id));
        }
        for (id, v) in es2::vec_edit(&get, &edit) {
            s2.set(id, v);
        }
        s2
    };
    let times = |s: &Es2| es2::vec_points(&|id| s.get(id)).iter().map(|p| p[0]).collect::<Vec<f32>>();
    assert_eq!(times(&s), vec![0.0, 0.4, 0.6, 1.0]);
    // Insert to the right of point 2 (index 1): the segment splits in halves, later points keep their times
    let a = apply(VecEdit::InsertAfter(1));
    assert_eq!(times(&a), vec![0.0, 0.4, 0.3, 0.3, 1.0]);
    assert_eq!((a.get(136), a.get(140), a.get(133)), (3.0, 1.0, 2.0), "the sustain point moved along, the new point is selected");
    let pa = es2::vec_points(&|id| a.get(id));
    assert!((pa[2][3] - 0.0).abs() < 1e-6, "between its neighbours' pad positions (−0.5 and 0.5)");
    // Shift-click at 0.7 s: a point there, the rest unmoved
    let b = apply(VecEdit::InsertAt(0.7));
    assert!(times(&b).iter().zip([0.0, 0.4, 0.3, 0.3, 1.0]).all(|(x, y)| (x - y).abs() < 1e-5));
    // Delete: never fewer than three points; the following point takes over the time
    let c = apply(VecEdit::Delete(1));
    assert_eq!(times(&c), vec![0.0, 1.0, 1.0]);
    assert_eq!((c.get(136), c.get(140)), (1.0, 1.0));
    let d = es2::vec_edit(&|id| c.get(id), &VecEdit::Delete(1));
    assert!(d.is_empty(), "start, sustain and end points stay");
    // Paste / Paste to All Points
    let clip = [9.0, 0.1, 0.2, 0.3, 0.4];
    let e = apply(VecEdit::Paste(2, clip));
    let pe = es2::vec_points(&|id| e.get(id));
    assert_eq!(&pe[2][1..], &clip[1..]);
    assert_eq!(pe[2][0], 0.6, "the time stays");
    let f = apply(VecEdit::PasteAll(clip));
    assert!(es2::vec_points(&|id| f.get(id)).iter().all(|p| p[1..] == clip[1..]));
    // Set Segments to Average Time
    let g = apply(VecEdit::AverageTime);
    assert!(times(&g)[1..].iter().all(|t| (t - 2.0 / 3.0).abs() < 1e-5));
    // Init to 8/8 and 16/16 Loop, Clear Envelope
    assert_eq!(times(&apply(VecEdit::InitLoop(8))).len(), 8);
    let h = apply(VecEdit::InitLoop(16));
    assert_eq!(times(&h).len(), 16);
    assert!(times(&h)[1..].windows(2).all(|w| w[0] == w[1]) && h.get(138) == 1.0);
    let i = apply(VecEdit::Clear);
    assert_eq!((times(&i).len(), i.get(136)), (3, 1.0));
    // a time change shifts the later points; Control-drag makes the next point compensate
    assert_eq!(times(&apply(VecEdit::SetTime(1, 0.7, false))), vec![0.0, 0.7, 0.6, 1.0]);
    let j = times(&apply(VecEdit::SetTime(1, 0.7, true)));
    assert!((j[1] - 0.7).abs() < 1e-6 && (j[2] - 0.3).abs() < 1e-6 && j[3] == 1.0);
    // Fix Timing: the scaling goes into the times, nothing changes to the ear
    let mut k = with(&[]);
    for id in 0..es2::NP as u32 {
        k.set(id, s.get(id));
    }
    k.set(128, 2.0);
    k.set(132, -0.5);
    let fixed = es2::vec_edit(&|id| k.get(id), &VecEdit::FixTiming);
    for (id, v) in fixed {
        k.set(id, v);
    }
    assert_eq!(times(&k), vec![0.0, 0.8, 1.2, 2.0]);
    assert_eq!((k.get(128), k.get(132)), (1.0, 0.0), "100 %, and a synced loop rate becomes as set");
}

#[test]
fn global_modes_unison_tuning() {
    // Mono: staccato retriggers, legato doesn't; glide in every mode. Legato: glide only when tied
    let mut s = with(&[(8, 1.0), (14, 1.0), (108, 0.6), (109, 0.6), (7, 0.6)]);
    s.note_on(57, 100);
    let a = render(&mut s, 1.0);
    s.note_on(69, 100);
    let b = render(&mut s, 0.1);
    assert!(rms(&b) > 0.8 * rms(&a[38400..]), "tied: no new attack");
    let fb = fundamental(&b, 150.0, 600.0);
    assert!(fb > 225.0 && fb < 430.0, "{fb}");
    s.note_off(69);
    s.note_off(57);
    render(&mut s, 1.0);
    s.note_on(69, 100);
    let c = render(&mut s, 0.05);
    assert!(rms(&c) < 0.3 * rms(&a[38400..]) && fundamental(&c, 150.0, 600.0) < 430.0, "detached: a new attack, still gliding");
    let mut s = with(&[(8, 2.0), (14, 1.0), (7, 0.6)]);
    s.note_on(57, 100);
    render(&mut s, 0.3);
    s.note_off(57);
    render(&mut s, 1.0);
    s.note_on(69, 100);
    assert!((fundamental(&render(&mut s, 0.1), 300.0, 600.0) / 440.0 - 1.0).abs() < 0.01, "Legato: a detached note starts on pitch");
    // Voices
    let mut s = with(&[(1, 2.0), (14, 1.0)]);
    for n in [48, 55, 64] {
        s.note_on(n, 100);
    }
    let x = render(&mut s, 0.5);
    assert!(tone(&x[9600..], lg_dsp::midi_hz(48.0)) < 0.1 * tone(&x[9600..], lg_dsp::midi_hz(64.0)));
    // Poly + Unison: two voices per note, spread left and right; halves the polyphony
    let beat = |x: &[f32]| {
        let e: Vec<f32> = x[4800..].chunks(2400).map(rms).collect();
        e.iter().cloned().fold(0.0, f32::max) / e.iter().cloned().fold(1e9, f32::min)
    };
    let mut s = with(&[(9, 1.0), (14, 1.0), (3, 0.3)]);
    s.note_on(57, 100);
    let (l, r) = render_st(&mut s, 3.0);
    assert!(differs(&l, &r) > 0.2, "spread over the panorama");
    let mono_sum: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a + b).collect();
    assert!(beat(&mono_sum) > 1.3, "two detuned voices: {}", beat(&mono_sum));
    let mut s = with(&[(9, 1.0), (14, 1.0), (1, 4.0)]);
    for n in [48, 55, 64] {
        s.note_on(n, 100);
    }
    let x = render(&mut s, 0.5);
    assert!(tone(&x[9600..], lg_dsp::midi_hz(48.0)) < 0.2 * tone(&x[9600..], lg_dsp::midi_hz(64.0)), "four voices = two notes");
    // Mono + Unison: Voices stacked on one note, Analog sets their detune; without Unison Analog does nothing in mono
    let stack = |uni: f32, analog: f32, voices: f32| {
        let mut s = with(&[(8, 1.0), (9, uni), (3, analog), (1, voices), (14, 1.0)]);
        s.note_on(57, 100);
        let (l, r) = render_st(&mut s, 3.0);
        let sum: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a + b).collect();
        (beat(&sum), rms(&sum))
    };
    assert!(stack(1.0, 0.5, 6.0).0 > 1.3 && stack(1.0, 0.0, 6.0).0 < 1.05 && stack(0.0, 1.0, 6.0).0 < 1.05);
    assert!(stack(1.0, 0.0, 6.0).1 > 2.0 * stack(1.0, 0.0, 2.0).1, "more voices, a fatter sound");
    // Analog in poly: each note a little off, at random
    let mut s = with(&[(3, 1.0), (14, 1.0)]);
    let mut fs = Vec::new();
    for _ in 0..4 {
        s.note_on(57, 100);
        fs.push(fundamental(&render(&mut s, 0.4)[4800..], 190.0, 250.0));
        s.note_off(57);
        render(&mut s, 0.6);
    }
    assert!(fs.iter().any(|f| (f - fs[0]).abs() > 0.05) && fs.iter().all(|f| (f / 220.0 - 1.0).abs() < 0.02), "{fs:?}");
    // Constant Beat Detuning: the beats between two detuned oscillators keep their speed up the keyboard
    let beats = |cbd: f32, n: u8| {
        let mut s = with(&[(10, cbd), (26, 0.0), (27, 0.5), (14, 0.0), (19, 0.0), (17, 0.0), (18, 12.0), (13, 0.0)]);
        s.note_on(n, 100);
        let x = render(&mut s, 6.0);
        let e: Vec<f32> = x.chunks(480).map(rms).collect();
        let mid = (e.iter().cloned().fold(0.0, f32::max) + e.iter().cloned().fold(1e9, f32::min)) * 0.5;
        e.windows(2).filter(|w| w[0] < mid && w[1] >= mid).count() as f32
    };
    let (lo, hi) = (beats(0.0, 60), beats(0.0, 84));
    assert!(hi > 3.0 * lo, "without CBD the beats quadruple two octaves up: {lo} {hi}");
    let (lo4, hi4) = (beats(4.0, 60), beats(4.0, 84));
    assert!((hi4 - lo4).abs() <= 2.0 && (lo4 - lo).abs() <= 1.0, "100 %: constant, C3 unchanged: {lo4} {hi4}");
    // Tune, bend ranges: the upper field can be linked to the lower one
    let bend = |up: f32, down: f32, v: i16, about: f32| {
        let mut s = with(&[(6, up), (167, down), (14, 1.0)]);
        s.pitch_bend(v);
        s.note_on(57, 100);
        fundamental(&render(&mut s, 0.4)[4800..], about * 0.8, about * 1.25) / about
    };
    assert!((bend(12.0, 2.0, 8191, 440.0) - 1.0).abs() < 0.01 && (bend(12.0, 2.0, -8192, 196.0) - 1.0).abs() < 0.01);
    assert!((bend(37.0, 5.0, 8191, 293.66) - 1.0).abs() < 0.01, "Link: up as far as down");
    let f = fundamental(&note(&[(2, 50.0), (14, 1.0)], 57, 0.4)[4800..], 150.0, 300.0);
    assert!((f / 220.0 - 2f32.powf(0.5 / 12.0)).abs() < 0.003);
}

#[test]
fn macros_controllers_and_effects() {
    // macros: Cutoff, Resonance, Attack, Release, Detune, Wave move groups of parameters
    let br = |m: f32| hi_rms(&note(&[(30, 1.0), (36, 0.4), (175, m)], 45, 0.4)[4800..], 800.0);
    assert!(br(1.0) > 5.0 * br(0.0) && br(-1.0) < 0.3 * br(0.0));
    let att = |m: f32| {
        let x = note(&[(108, 0.5), (109, 0.5), (180, m)], 57, 0.6);
        rms(&x[..2400]) / rms(&x[24000..])
    };
    assert!(att(1.0) < 0.5 * att(0.0) && att(-1.0) > 1.5 * att(0.0), "{} {} {}", att(-1.0), att(0.0), att(1.0));
    let det = note(&[(26, 0.0), (27, 0.5), (18, 0.0), (17, 0.0), (178, 1.0)], 57, 2.0);
    let nodet = note(&[(26, 0.0), (27, 0.5), (18, 0.0), (17, 0.0), (178, 0.0)], 57, 2.0);
    let wob = |x: &[f32]| {
        let e: Vec<f32> = x[4800..].chunks(2400).map(rms).collect();
        e.iter().cloned().fold(0.0, f32::max) / e.iter().cloned().fold(1e9, f32::min)
    };
    assert!(wob(&det) > 1.3 && wob(&nodet) < 1.05, "the Detune macro spreads the oscillators");
    // Ctrl A–F: the menus list the assignable controllers only
    assert!(es2::ctrl_index(0).is_none() && es2::ctrl_index(32).is_none() && es2::ctrl_index(40).is_none() && es2::ctrl_index(64).is_none() && es2::ctrl_index(69).is_none() && es2::ctrl_index(120).is_none());
    for cc in (1..32).chain(70..120) {
        assert_eq!(es2::ctrl_cc(es2::ctrl_index(cc).unwrap()), Some(cc));
    }
    assert_eq!(es2::CTRL_MENU[es2::CTRL_LEARN], "Learn");
    // a controller as a router source
    let mut v = vec![(14, 1.0), (169, es2::ctrl_index(20).unwrap() as f32)];
    v.extend(route("Ctrl A", "Pitch123", 0.76));
    let mut s = with(&v);
    s.note_on(57, 100);
    render(&mut s, 0.1);
    s.controller(20, 127);
    assert!((fundamental(&render(&mut s, 0.4)[4800..], 300.0, 600.0) / 440.0 - 1.0).abs() < 0.005);
    // 14 bits: the fine half is 32 above the coarse one; the coarse one alone works at 7 bits
    s.controller(20, 64);
    let coarse = fundamental(&render(&mut s, 0.4)[4800..], 250.0, 400.0);
    s.controller(52, 127);
    let fine = fundamental(&render(&mut s, 0.4)[4800..], 250.0, 400.0);
    assert!(fine > coarse * 1.001 && fine < coarse * 1.02, "{coarse} {fine}");
    // Learn: the next controller moved is assigned; nothing within 20 seconds: the old one again
    let mut s = with(&[(169, 2.0)]);
    s.set(169, es2::CTRL_LEARN as f32);
    assert_eq!(s.learning(), Some(0));
    s.controller(64, 127);
    assert_eq!(s.learning(), Some(0), "a pedal is not assignable");
    s.controller(74, 10);
    assert_eq!((s.learning(), s.get(169)), (None, es2::ctrl_index(74).unwrap() as f32));
    s.set(170, es2::CTRL_LEARN as f32);
    render(&mut s, 19.0);
    assert_eq!(s.learning(), Some(1));
    render(&mut s, 1.5);
    assert_eq!((s.learning(), s.get(170)), (None, 4.0));
    // effects: distortion (soft / hard, tone), chorus / flanger / phaser widen or move the sound
    let dist = |d: f32, hard: f32, tone_: f32| bright(&note(&[(14, 1.0), (117, d), (116, hard), (118, tone_)], 45, 0.4)[4800..], 1500.0);
    assert!(dist(1.0, 0.0, 1.0) > 5.0 * dist(0.0, 0.0, 1.0) && dist(1.0, 1.0, 1.0) > dist(1.0, 0.0, 1.0) && dist(1.0, 1.0, 0.0) < 0.5 * dist(1.0, 1.0, 1.0));
    for ty in 0..3 {
        let mut s = with(&[(119, ty as f32), (121, 0.8), (120, 1.0)]);
        s.note_on(57, 100);
        let wet = render(&mut s, 1.0);
        let dry = note(&[(121, 0.0)], 57, 1.0);
        assert!(differs(&wet[4800..], &dry[4800..]) > 0.1, "effect {ty}");
    }
    // Volume
    let lv = |db: f32| rms(&note(&[(40, db)], 57, 0.3));
    assert!((lv(0.0) / lv(-6.0) - 2.0).abs() < 0.05);
}

#[test]
fn randomizing() {
    let s = Es2::new(RATE);
    let get = |id: u32| s.get(id);
    let ids = |dest: usize| -> Vec<u32> { es2::randomize(&get, 1.0, dest, 3).iter().map(|x| x.0).collect() };
    // never: master level, the filter button, the oscillators' on / off
    for dest in 0..es2::RND_DEST.len() {
        let v = ids(dest);
        assert!(v.iter().all(|id| ![40, 115, 11, 16, 21].contains(id)), "destination {dest}");
        assert!(!v.is_empty(), "destination {dest} changes something");
    }
    let has = |v: &[u32], id: u32| v.contains(&id);
    let all = ids(0);
    assert!(has(&all, 12) && has(&all, 44) && has(&all, 32) && has(&all, 205));
    let no_router = ids(1);
    assert!(!no_router.iter().any(|id| (41..=90).contains(id)) && !has(&no_router, 12) && !has(&no_router, 17) && has(&no_router, 13), "router and semitones stay, fine tuning moves");
    assert!(!ids(2).iter().any(|id| *id >= 200 || (124..=140).contains(id)), "all except the Vector Envelope");
    assert!(ids(3).iter().all(|id| [14, 19, 24, 15, 20, 25, 190, 191, 192].contains(id)), "Waves");
    assert!(ids(4).iter().all(|id| [15, 20, 25].contains(id)) && ids(4).len() == 3, "Digiwaves: a new wave on every oscillator");
    assert!(ids(5).iter().all(|id| (29..=38).contains(id)), "Filters");
    assert!(ids(6).iter().all(|id| (96..=114).contains(id)), "Envs");
    assert!(ids(7).iter().all(|id| (91..=95).contains(id)), "LFOs");
    assert!(ids(8).iter().all(|id| (41..=90).contains(id)), "Router");
    assert!(ids(9).iter().all(|id| (116..=121).contains(id)), "FX");
    assert!(ids(10).iter().all(|id| *id >= 200 || (124..=140).contains(id) || *id == 168 || *id == 134), "Vector Envelope with the pad's routing");
    assert!(has(&ids(10), 134), "randomizing the Vector Envelope turns Solo Point off");
    let off = |v: &[u32], offs: &[u32]| v.iter().filter(|id| **id >= 200).all(|id| offs.contains(&((id - 200) % 5)));
    assert!(off(&ids(11), &[1, 2]) && off(&ids(12), &[3, 4]) && off(&ids(13), &[3]) && off(&ids(14), &[4]) && off(&ids(15), &[0]));
    assert!(ids(16).iter().any(|id| [135, 136, 138, 139, 140].contains(id)) && off(&ids(16), &[0]), "Structure: times, points, loop");
    // the intensity: little changes little; repeated clicks drift on from the current values
    let far = |amt: f32| es2::randomize(&get, amt, 5, 9).iter().filter(|x| ![29, 31, 34, 35].contains(&x.0)).map(|&(id, v)| ((v - s.get(id)) / (es2::PARAMS.iter().find(|p| p.id == id).unwrap().max - es2::PARAMS.iter().find(|p| p.id == id).unwrap().min)).abs()).fold(0.0, f32::max);
    assert!(far(0.05) <= 0.06);
    assert!(es2::randomize(&get, 0.5, 0, 1) != es2::randomize(&get, 0.5, 0, 2));
    // a randomized sound still plays
    let mut r = Es2::new(RATE);
    for seed in 1..6 {
        let set = es2::randomize(&|id| r.get(id), 1.0, 0, seed);
        for (id, v) in set {
            r.set(id, v);
        }
        r.note_on(57, 100);
        assert!(finite(&render(&mut r, 0.3)));
        r.note_off(57);
    }
}
