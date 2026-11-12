//! ES2 and EFM1 extended parameters: MIDI Mono Mode gives each voice its channel; a member
//! channel's bend moves only its voice (by Mono Mode Bend Range), the base channel all.

use lg_dsp::{Instrument, Plugin};
use lg_synth::efm1::Efm1;
use lg_synth::es2::Es2;

const RATE: f32 = 48000.0;

fn hz(x: &mut dyn Instrument) -> f32 {
    let n = 24000;
    let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
    for k in (0..n).step_by(256) {
        x.render(&mut l[k..k + 256], &mut r[k..k + 256]);
    }
    let s = &l[4800..];
    let z = s.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    z as f32 / (s.len() as f32 / RATE)
}

/// (bent on its own channel, another member channel bent, base channel bent) / unbent.
fn ratios(make: &dyn Fn() -> Box<dyn Instrument>, bend: &dyn Fn(&mut Box<dyn Instrument>, u8, i16)) -> (f32, f32, f32) {
    let mut a = make();
    a.note_on_ch(1, 57, 100);
    let base = hz(a.as_mut());
    let mut run = |ch: u8, v: i16| {
        let mut b = make();
        b.note_on_ch(1, 57, 100);
        bend(&mut b, ch, v);
        hz(b.as_mut()) / base
    };
    (run(1, 8192 / 4), run(5, 8191), run(0, 8191))
}

#[test]
fn es2_mono_mode() {
    let make = || -> Box<dyn Instrument> {
        let mut e = Es2::new(RATE);
        e.set(184, 1.0);
        // one plain sine (oscillator 1), filters open
        e.set(16, 0.0);
        e.set(21, 0.0);
        e.set(14, 0.0);
        e.set(27, 1.0);
        e.set(32, 1.0);
        e.set(36, 1.0);
        Box::new(e)
    };
    let bend = |b: &mut Box<dyn Instrument>, ch: u8, v: i16| {
        b.as_any_mut().unwrap().downcast_mut::<Es2>().unwrap().pitch_bend_ch(ch, v);
    };
    let (own, other, base) = ratios(&make, &bend);
    assert!((own - 2.0).abs() < 0.1, "a quarter of 48 semitones: {own}");
    assert!((other - 1.0).abs() < 0.03, "{other}");
    assert!(base > 1.05, "the base channel bends all: {base}");
}

#[test]
fn efm1_mono_mode() {
    let make = || -> Box<dyn Instrument> {
        let mut e = Efm1::new(RATE);
        e.set(lg_synth::efm1::MONO_MODE, 2.0);
        Box::new(e)
    };
    let bend = |b: &mut Box<dyn Instrument>, ch: u8, v: i16| {
        b.as_any_mut().unwrap().downcast_mut::<Efm1>().unwrap().pitch_bend_ch(if ch == 0 { 15 } else { ch }, v);
    };
    let (own, other, base) = ratios(&make, &bend);
    assert!((own - 2.0).abs() < 0.15, "{own}");
    assert!((other - 1.0).abs() < 0.03, "{other}");
    assert!(base > 1.05, "base channel 16 bends all: {base}");
}

#[test]
fn alchemy_mono_mode() {
    use lg_synth::alchemy::{Alchemy, MONO_BEND, MONO_MODE};
    let make = || -> Box<dyn Instrument> {
        let mut a = Alchemy::new(RATE);
        a.set(MONO_MODE, 1.0);
        a.set(278, 0.0);
        a.set(364, 0.0);
        a.set(220, 1.0);
        a.set(8, 0.0);
        a.set(9, 0.0);
        Box::new(a)
    };
    let bend = |b: &mut Box<dyn Instrument>, ch: u8, v: i16| {
        b.as_any_mut().unwrap().downcast_mut::<Alchemy>().unwrap().pitch_bend_ch(ch, v);
    };
    let (own, other, base) = ratios(&make, &bend);
    assert!((own - 2.0).abs() < 0.15, "{own}");
    assert!((other - 1.0).abs() < 0.03, "{other}");
    assert!(base > 1.05, "{base}");
    let a = Alchemy::new(RATE);
    let p = a.params().iter().find(|p| p.id == MONO_BEND).unwrap();
    assert_eq!((p.min, p.max, p.default), (0.0, 96.0, 48.0));
}
