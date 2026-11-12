//! Vintage Clav's extended parameters: MIDI Mono Mode (a voice per channel) and Mono Mode Bend
//! Range.

use lg_dsp::{Instrument, Plugin};
use lg_synth::clav::{VintageClav, MONO_BEND, MONO_MODE, MONO_MODES};

const RATE: f32 = 48000.0;

fn hz(c: &mut VintageClav) -> f32 {
    let n = 256 * 94;
    let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
    for k in (0..n).step_by(256) {
        c.render(&mut l[k..k + 256], &mut r[k..k + 256]);
    }
    // the fundamental by autocorrelation (40 Hz – 2 kHz)
    let x = &l[4800..];
    let corr = |lag: usize| x[..x.len() - lag].iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f32>();
    let (lo, hi) = ((RATE / 2000.0) as usize, (RATE / 40.0) as usize);
    let best = (lo..hi).map(|lag| (lag, corr(lag))).fold((lo, f32::MIN), |b, c| if c.1 > b.1 { c } else { b });
    let floor = best.1 * 0.9;
    // the first lag near the best (not a multiple of the period)
    let lag = (lo..hi).find(|&k| corr(k) >= floor && corr(k) >= corr(k - 1) && corr(k) >= corr(k + 1)).unwrap_or(best.0);
    RATE / lag as f32
}

#[test]
fn parameters() {
    let c = VintageClav::new(RATE);
    let mm = c.params().iter().find(|p| p.id == MONO_MODE).unwrap();
    assert_eq!(mm.choices, MONO_MODES);
    assert_eq!(MONO_MODES.len(), 3);
    let br = c.params().iter().find(|p| p.id == MONO_BEND).unwrap();
    assert_eq!((br.min, br.max, br.default), (0.0, 96.0, 48.0));
}

#[test]
fn member_channels_bend_their_own_voice() {
    // Mono Mode on (base channel 1): a note on channel 2 bent by +12 (a quarter of 48)
    let mut a = VintageClav::new(RATE);
    a.set(MONO_MODE, 1.0);
    a.note_on_ch(1, 45, 100);
    let base = hz(&mut a);
    let mut b = VintageClav::new(RATE);
    b.set(MONO_MODE, 1.0);
    b.note_on_ch(1, 45, 100);
    b.pitch_bend_ch(1, 8192 / 4);
    let up = hz(&mut b);
    assert!((up / base - 2.0).abs() < 0.12, "an octave up: {base} → {up}");
    // a bend on another member channel leaves it alone
    let mut c = VintageClav::new(RATE);
    c.set(MONO_MODE, 1.0);
    c.note_on_ch(1, 45, 100);
    c.pitch_bend_ch(4, 8191);
    assert!((hz(&mut c) / base - 1.0).abs() < 0.05);
    // the base channel bends every voice (by the Pitch Wheel range, 2 semitones)
    let mut d = VintageClav::new(RATE);
    d.set(MONO_MODE, 1.0);
    d.note_on_ch(1, 45, 100);
    d.pitch_bend_ch(0, 8191);
    let r = hz(&mut d) / base;
    assert!(r > 1.08 && r < 1.17, "{r}");
    // Mono Mode off: any channel's bend is the normal pitch wheel
    let mut e = VintageClav::new(RATE);
    e.note_on_ch(1, 45, 100);
    e.pitch_bend_ch(1, 8191);
    let r = hz(&mut e) / base;
    assert!(r > 1.08 && r < 1.17, "{r}");
}
