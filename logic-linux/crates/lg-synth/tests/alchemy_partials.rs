//! Alchemy's additive / spectral partial engine: PVar, the three additive effect units, the
//! synthesized formant filter, the spectral element's cuts and effects, Complex shapes in a voice.

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::Alchemy;
use lg_synth::alchemy_partials::*;

fn saw() -> ([f32; NP], [f32; NP]) {
    let mut a = [0.0; NP];
    let mut r = [0.0; NP];
    for k in 0..NP {
        a[k] = 1.0 / (k + 1) as f32;
        r[k] = (k + 1) as f32 * 1.01;
    }
    (a, r)
}

fn run(cfg: &PartCfg, spectral: bool, f0: f32) -> ([f32; NP], [f32; NP], [f32; NP]) {
    let (a, r) = saw();
    let mut st = PartState::default();
    let (mut pa, mut pf, mut pp) = ([0.0; NP], [0.0; NP], [0.0; NP]);
    for _ in 0..20 {
        process(cfg, spectral, &a, &r, f0, 0.01, 24000.0, &mut st, &mut pa, &mut pf, &mut pp);
    }
    (pa, pf, pp)
}

fn base() -> PartCfg {
    PartCfg { vol: 1.0, num: 64, pvar: 1.0, sym: 0.5, sp_vol: 1.0, lo: 0.0, hi: 1.0, fm_size: 1.0, fm_center: 0.5, ..Default::default() }
}

fn unit(c: &mut PartCfg, u: usize, ty: u8, k: [f32; 4]) {
    c.u[u] = Unit3 { on: true, ty, k };
}

#[test]
fn pvar_harmonic_and_num_partials() {
    let mut c = base();
    c.pvar = 0.0;
    c.num = 10;
    let (a, f, _) = run(&c, false, 100.0);
    assert!((f[4] - 500.0).abs() < 1e-3, "PVar 0 tunes to the harmonic series");
    assert!(a[10] == 0.0 && a[9] > 0.0, "only Num Partials sound");
    c.pvar = 1.0;
    assert!((run(&c, false, 100.0).1[4] - 505.0).abs() < 1e-2, "PVar 100 % keeps the content's tuning");
}

#[test]
fn amplitude_unit() {
    let mut c = base();
    unit(&mut c, 0, 1, [0.0, 0.5, 0.5, 0.0]);
    assert_eq!(run(&c, false, 100.0).0[0], 0.0, "Fundamental 0 removes it");
    unit(&mut c, 0, 1, [1.0, 0.5, 0.5, 0.0]);
    let a = run(&c, false, 100.0).0;
    assert!(a[0] > 0.0 && a[2] == 0.0, "100 % isolates it");
    unit(&mut c, 0, 2, [0.0, 0.0, 0.5, 0.0]);
    let a = run(&c, false, 100.0).0;
    assert!(a[1] == 0.0 && a[2] > 0.0, "Pulse mutes even harmonics");
    unit(&mut c, 0, 0, [0.0; 4]);
    let a = run(&c, false, 100.0).0;
    assert!((a[0] - a[30]).abs() < 1e-6, "None: all partials equal");
}

#[test]
fn pitch_unit() {
    let mut c = base();
    c.pvar = 0.0;
    unit(&mut c, 1, 3, [0.5, 0.55, 0.0, 0.0]);
    let f = run(&c, false, 220.0).1;
    assert!((f[0] - 320.0).abs() < 0.5 && (f[1] - 540.0).abs() < 0.5, "a 100 Hz shift: {} {}", f[0], f[1]);
    unit(&mut c, 1, 4, [1.0, 0.25, 0.0, 0.0]);
    let f = run(&c, false, 100.0).1;
    assert!((f[0] - f[10]).abs() < 1e-3, "Magnet 100 % puts every partial on the target");
    unit(&mut c, 1, 2, [1.0, 0.0, 0.0, 0.0]);
    let f = run(&c, false, 100.0).1;
    assert!(f[0] == 100.0 && f[9] > 1000.0 * 1.05, "Stretch keeps the fundamental, sharpens the top");
    unit(&mut c, 1, 1, [0.5, 0.0, 0.0, 0.0]);
    let f = run(&c, false, 100.0).1;
    assert!(f[1] != 200.0 && f[2] == 300.0, "Beating partial 2 detunes 2, 4, 6");
}

#[test]
fn output_unit_and_formant() {
    let mut c = base();
    unit(&mut c, 2, 5, [1.0, 0.0, 0.0, 0.5]);
    let (_, _, p) = run(&c, false, 100.0);
    assert!(p.iter().any(|x| x.abs() > 0.1), "Spread pans");
    unit(&mut c, 2, 3, [0.0, 0.5, 0.5, 0.5]);
    let a = run(&c, false, 50.0).0;
    let flat = run(&base(), false, 50.0).0;
    assert!(a[0] < flat[0] * 0.3 && (a[40] - flat[40]).abs() < flat[40] * 0.3, "EQ Low cut");
    let mut c = base();
    c.fm_on = true;
    c.fm_types = [20, 21, 22, 23];
    assert!(formant_unit(20, 800.0, 0.0, 1.0, 1000.0) > formant_unit(20, 2000.0, 0.0, 1.0, 1000.0), "vowel A's first formant");
    let a = run(&c, false, 100.0).0;
    assert!(a[7] / a[19] > (1.0 / 8.0) / (1.0 / 20.0), "the formant shapes the partials");
    assert_eq!(formant_unit(0, 500.0, 0.0, 1.0, 1000.0), 1.0, "Off passes");
}

#[test]
fn spectral_cuts_and_effects() {
    let mut c = base();
    c.lo = 0.4; // about 316 Hz
    c.hi = 0.7; // about 2.5 kHz
    let a = run(&c, true, 100.0).0;
    assert!(a[1] == 0.0 && a[5] > 0.0 && a[40] == 0.0, "low and high cut make a band pass");
    let mut c = base();
    c.sfx[0] = SpecFx { on: true, ty: 6, mix: 1.0, k: [0.6, 0.0, 0.0] };
    let a = run(&c, true, 100.0).0;
    assert!(a[0] > 0.0 && a[30] < 1e-3, "Gate drops quiet partials");
    c.sfx[0] = SpecFx { on: true, ty: 11, mix: 1.0, k: [0.5, 0.5, 1.0] };
    let a = run(&c, true, 100.0).0;
    assert!((a[0] - 1.0).abs() > 0.1, "Smear averages neighbours");
    c.sfx[0] = SpecFx { on: true, ty: 4, mix: 0.0, k: [0.9, 0.0, 1.0] };
    assert_eq!(run(&c, true, 100.0).1[0], 101.0, "Mix 0 % leaves it dry");
    for ty in 1..SPEC_FX.len() as u8 {
        c.sfx[1] = SpecFx { on: true, ty, mix: 0.7, k: [0.4, 0.6, 0.3] };
        let (a, f, _) = run(&c, true, 110.0);
        assert!(a.iter().chain(&f).all(|x| x.is_finite()), "{}", SPEC_FX[ty as usize]);
    }
}

#[test]
fn complex_shapes_in_a_voice() {
    let render = |complex: f32| {
        let mut a = Alchemy::new(48000.0);
        a.set(5, 0.0);
        a.set(278, 0.0);
        a.set(293, 1.0); // source A additive
        a.set(id(0, A_COMPLEX), complex);
        a.set(id(0, A_SHAPE), 2.0);
        a.note_on(48, 100);
        let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
        a.render(&mut l, &mut r);
        l
    };
    let (s, q) = (render(0.0), render(1.0));
    assert!(q.iter().all(|x| x.is_finite()) && s.iter().zip(&q).any(|(a, b)| (a - b).abs() > 1e-3));
}
