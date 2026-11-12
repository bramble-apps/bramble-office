//! Alchemy's master voice section (Trigger Mode, Num, Voice Priority, Glide Rate / Time) and
//! the VA element's Phase, wide unison mode and noise oscillator.

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::{unison_spread, Alchemy, NOISE, VOICE};

const RATE: f32 = 48000.0;

fn render(a: &mut Alchemy, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * RATE) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for c in (0..n).step_by(256) {
        let e = (c + 256).min(n);
        a.render(&mut l[c..e], &mut r[c..e]);
    }
    (l, r)
}

/// Num 1, Glide 0.4 s, the given Trigger Mode (0 Always, 1 Retrigger, 2 Legato).
fn mono(trig: f32) -> Alchemy {
    let mut a = Alchemy::new(RATE);
    a.set(376, 1.0);
    a.set(371, trig);
    a.set(98, 0.4);
    a
}

fn one(a: &Alchemy) -> (u8, f32, f32) {
    let v = a.voice_states();
    assert_eq!(v.len(), 1, "one voice: {v:?}");
    v[0]
}

#[test]
fn trigger_legato() {
    let mut a = mono(2.0);
    a.note_on(60, 100);
    render(&mut a, 0.2);
    assert_eq!(one(&a).1, 60.0, "the first note of a group doesn't glide");
    a.note_on(64, 100);
    render(&mut a, 0.05);
    let (n, p, age) = one(&a);
    assert_eq!(n, 64);
    assert!(p > 60.0 && p < 64.0, "an overlapping note glides: {p}");
    assert!(age > 0.2, "and isn't retriggered: {age}");
    a.note_off(60);
    a.note_off(64);
    render(&mut a, 0.05);
    a.note_on(67, 100);
    render(&mut a, 0.02);
    let v = a.voice_states();
    let (_, p, age) = *v.iter().find(|v| v.0 == 67).unwrap();
    assert_eq!(p, 67.0, "a new legato group starts on its note");
    assert!(age < 0.05);
}

#[test]
fn trigger_always_and_retrigger() {
    // Always: overlapping notes don't retrigger, but every note glides (a new group too)
    let mut a = mono(0.0);
    a.note_on(60, 100);
    render(&mut a, 0.2);
    a.note_on(64, 100);
    render(&mut a, 0.05);
    let (_, p, age) = one(&a);
    assert!(p < 64.0 && age > 0.2, "{p} {age}");
    a.note_off(60);
    a.note_off(64);
    render(&mut a, 0.6);
    a.note_on(72, 100);
    render(&mut a, 0.02);
    let v = a.voice_states();
    let (_, p, age) = *v.iter().find(|v| v.0 == 72).unwrap();
    assert!(p < 72.0 && age < 0.05, "a new group glides from the last note: {p} {age}");
    // Retrigger: every note triggers and glides
    let mut a = mono(1.0);
    a.note_on(60, 100);
    render(&mut a, 0.2);
    a.note_on(64, 100);
    render(&mut a, 0.02);
    let (n, p, age) = one(&a);
    assert!(n == 64 && p < 64.0 && age < 0.05, "{n} {p} {age}");
    // releasing goes back to the key still held
    a.note_off(64);
    render(&mut a, 0.6);
    assert_eq!(one(&a).0, 60);
    assert_eq!(one(&a).1, 60.0);
}

#[test]
fn glide_rate_and_time() {
    for (time, wide_end) in [(false, 54.0), (true, 60.0)] {
        for (to, want) in [(60u8, 54.0f32), (72, wide_end)] {
            let mut a = mono(0.0);
            a.set(98, 1.0);
            a.set(VOICE + 1, if time { 1.0 } else { 0.0 });
            a.note_on(48, 100);
            render(&mut a, 0.1);
            a.note_on(to, 100);
            render(&mut a, 0.5);
            let p = one(&a).1;
            assert!((p - want).abs() < 0.3, "time {time} to {to}: {p} (want {want})");
        }
    }
}

#[test]
fn thirty_two_voices() {
    let mut a = Alchemy::new(RATE);
    a.set(376, 32.0);
    for k in 0..32 {
        a.note_on(40 + k, 100);
    }
    render(&mut a, 0.05);
    assert_eq!(a.voice_states().len(), 32);
}

fn notes(a: &Alchemy) -> Vec<u8> {
    let mut v: Vec<u8> = a.voice_states().iter().map(|v| v.0).collect();
    v.sort();
    v
}

#[test]
fn voice_priority() {
    // poly: Num 2
    let play = |prio: f32, keys: &[u8]| {
        let mut a = Alchemy::new(RATE);
        a.set(376, 2.0);
        a.set(VOICE, prio);
        for &k in keys {
            a.note_on(k, 100);
            render(&mut a, 0.01);
        }
        notes(&a)
    };
    assert_eq!(play(0.0, &[60, 62, 64]), vec![62, 64], "Newest");
    assert_eq!(play(1.0, &[60, 62, 64]), vec![60, 62], "Oldest");
    assert_eq!(play(2.0, &[60, 62, 64]), vec![60, 62], "Lowest drops a higher note");
    assert_eq!(play(2.0, &[60, 62, 55]), vec![55, 60], "Lowest replaces the highest");
    assert_eq!(play(3.0, &[60, 62, 55]), vec![60, 62], "Highest");
    assert_eq!(play(3.0, &[60, 62, 64]), vec![62, 64]);
    // Num 1, Lowest: the lowest held key sounds
    let mut a = mono(2.0);
    a.set(98, 0.0);
    a.set(VOICE, 2.0);
    a.note_on(60, 100);
    a.note_on(64, 100);
    render(&mut a, 0.01);
    assert_eq!(one(&a).0, 60);
    a.note_on(55, 100);
    render(&mut a, 0.01);
    assert_eq!(one(&a).0, 55);
    a.note_off(55);
    render(&mut a, 0.01);
    assert_eq!(one(&a).0, 60);
}

/// Source A only, Master filter off, the oscillator's start point fixed.
fn plain() -> Alchemy {
    let mut a = Alchemy::new(RATE);
    a.set(278, 0.0);
    a.set(364, 0.0);
    a.set(212, 0.0);
    a
}

#[test]
fn va_phase() {
    let first = |a: &mut Alchemy| {
        a.note_on(60, 100);
        let (l, _) = render(a, 0.02);
        a.note_off(60);
        render(a, 3.0);
        l
    };
    let mut a = plain();
    a.set(212, 0.3);
    let (x, y) = (first(&mut a), first(&mut a));
    assert_eq!(x, y, "a fixed start point: every note starts alike");
    a.set(212, 1.0);
    let (x, y) = (first(&mut a), first(&mut a));
    assert_ne!(x, y, "100 %: a random start each trigger");
}

#[test]
fn wide_unison() {
    assert_eq!(unison_spread(true, 0, 1, 1.0), (0.0, 0.0));
    assert_eq!([0, 1].map(|u| unison_spread(true, u, 2, 1.0).1), [-1.0, 1.0], "2: hard left and right");
    assert_eq!([0, 1, 2].map(|u| unison_spread(true, u, 3, 1.0).1), [0.0, -1.0, 1.0]);
    let p4 = [0, 1, 2, 3].map(|u| unison_spread(true, u, 4, 1.0).1);
    assert_eq!(p4.iter().filter(|p| **p == -1.0).count(), 2);
    assert_eq!(p4.iter().filter(|p| **p == 1.0).count(), 2);
    assert_eq!(unison_spread(true, 1, 2, 1.0).0, 2.0, "100 %: ±2 semitones");
    assert_eq!(unison_spread(true, 1, 2, 0.5).0, 1.0, "50 %: ±1 semitone");
    assert!(unison_spread(false, 1, 2, 1.0).0 < 0.5, "the older mode detunes less");
    // two oscillators in wide mode: two hard-panned (differently tuned) channels
    let mut a = plain();
    a.set(VOICE + 2, 1.0);
    a.set(220, 2.0);
    a.set(216, 1.0);
    a.note_on(60, 100);
    let (l, r) = render(&mut a, 0.3);
    let zc = |x: &[f32]| x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    let (zl, zr) = (zc(&l[4800..]), zc(&r[4800..]));
    assert!(zr > zl + zl / 6, "right is the upper oscillator: {zl} {zr}");
}

#[test]
fn noise_oscillator() {
    // the noise alone: the same note with and without noise (the oscillator is deterministic)
    let noise = |cfg: &dyn Fn(&mut Alchemy)| {
        let mut a = plain();
        a.note_on(48, 100);
        let (dry, _) = render(&mut a, 0.3);
        let mut a = plain();
        a.set(305, 1.0);
        cfg(&mut a);
        a.note_on(48, 100);
        let (wet, _) = render(&mut a, 0.3);
        let d: Vec<f32> = wet.iter().zip(&dry).map(|(w, d)| w - d).collect();
        let zc = d.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let rms = (d.iter().map(|x| x * x).sum::<f32>() / d.len() as f32).sqrt();
        (zc, rms)
    };
    let (white, wr) = noise(&|_| {});
    assert!(wr > 1e-3);
    let (dark, _) = noise(&|a| a.set(NOISE + 12, 0.3));
    assert!(dark * 4 < white, "High Cut: {dark} vs {white}");
    let (bright, _) = noise(&|a| a.set(NOISE + 8, 0.85));
    assert!(bright > white, "Low Cut: {bright} vs {white}");
    let (band, _) = noise(&|a| {
        a.set(NOISE + 8, 0.5);
        a.set(NOISE + 12, 0.55);
    });
    assert!(band < white && band > dark, "band-pass: {band}");
    let (brown, _) = noise(&|a| a.set(NOISE, 2.0));
    let (pink, _) = noise(&|a| a.set(NOISE, 1.0));
    assert!(brown < pink && pink < white, "{brown} {pink} {white}");
    let (_, half) = noise(&|a| a.set(NOISE + 4, 0.5));
    assert!((half / wr - 0.5).abs() < 0.1, "Noise Vol: {half} vs {wr}");
}
