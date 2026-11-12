//! Alchemy's morph section: crossfade weights (XY, linear, groups), XFade levels and a morph
//! that glides the pitch between differently tuned sources as one sound.

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::{morph_weights, Alchemy, MORPH};

const RATE: f32 = 48000.0;

#[test]
fn weights() {
    assert_eq!(morph_weights(1, 0, 0.0, 0.0), [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(morph_weights(1, 0, 1.0, 0.0), [0.0, 1.0, 0.0, 0.0], "X: A, C against B, D");
    assert_eq!(morph_weights(1, 0, 0.0, 1.0), [0.0, 0.0, 1.0, 0.0], "Y: A, B against C, D");
    assert_eq!(morph_weights(2, 0, 2.0 / 3.0, 0.0), [0.0, 0.0, 1.0, 0.0], "linear runs A → B → C → D");
    assert_eq!(morph_weights(4, 1, 0.25, 0.0), [0.0, 0.0, 0.75, 0.25], "group C-D");
}

fn pitch_of(x: &[f32]) -> f32 {
    // zero crossings upward per second
    let n = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    n as f32 / (x.len() as f32 / RATE)
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

fn sines() -> Alchemy {
    let mut a = Alchemy::new(RATE);
    a.set(5, 0.0);
    // A and B: plain sines (one oscillator each), B an octave up
    for s in 0..2u32 {
        a.set(301 + s, 0.0);
        a.set(220 + s, 1.0);
    }
    a.set(8, 0.0); // the Perform Square/Saw knob would change A's shape
    a.set(9, 0.0);
    a.set(10, 0.0);
    a.set(4, 0.5); // Octave Osc neutral
    a.set(28, 12.0);
    a.set(85, 1.0);
    a
}

#[test]
fn xfade_and_morph() {
    // XFade XY at X 0: only A sounds
    let mut a = sines();
    a.set(MORPH, 1.0);
    a.set(75, 0.0);
    a.note_on(57, 100);
    let p = pitch_of(&render(&mut a, 0.5)[4800..]);
    assert!((p - 220.0).abs() < 15.0, "A alone: {p}");
    // Morph XY halfway: one sound between the tunings (a fifth or so up, not two pitches)
    let mut a = sines();
    a.set(MORPH, 3.0);
    a.set(75, 0.5);
    a.note_on(57, 100);
    let p = pitch_of(&render(&mut a, 0.5)[4800..]);
    assert!(p > 260.0 && p < 380.0, "morphed pitch: {p}");
    // Fixed Pitch keeps the root
    a.set(MORPH + 4, 1.0);
    let p = pitch_of(&render(&mut a, 0.5)[4800..]);
    assert!((p - 220.0).abs() < 15.0, "fixed: {p}");
}

#[test]
fn keyscale_and_wait() {
    use lg_synth::alchemy::{KEYSCALE, WAIT};
    // Keyscale Off: every key plays C3
    let mut a = sines();
    a.set(278, 0.0);
    a.set(KEYSCALE, 2.0);
    a.note_on(72, 100);
    let p = pitch_of(&render(&mut a, 0.5)[4800..]);
    assert!((p - 261.6).abs() < 15.0, "{p}");
    // Wait: silent for the first 0.2 s
    let mut a = sines();
    a.set(278, 0.0);
    a.set(WAIT, 0.2);
    a.note_on(60, 100);
    let l = render(&mut a, 0.4);
    let early = l[..4800].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let late = l[12000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(early < 1e-4 && late > 1e-3, "{early} {late}");
}
