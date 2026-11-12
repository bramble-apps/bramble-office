//! EVOC 20 PolySynth: a synthetic side chain (band-limited bursts) drives the vocoder; the output
//! follows its spectrum and envelope, and each control does what the guide says.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::evoc_ps::{self as e, EvocPs};

fn evoc() -> EvocPs {
    let mut s = EvocPs::new(RATE);
    s.set(e::ENSEMBLE, 0.0);
    s
}

/// Noise through a resonator around `hz` (a band-limited burst), amplitude `amp`.
fn band_noise(hz: f32, secs: f32, amp: f32, seed: u32) -> Vec<f32> {
    let mut rng = lg_synth::dsp::Rng::new(seed);
    let n = (secs * RATE) as usize;
    let w = std::f32::consts::TAU * hz / RATE;
    let r = 0.995f32;
    let (mut y1, mut y2) = (0.0f32, 0.0f32);
    let mut out: Vec<f32> = (0..n)
        .map(|_| {
            let y = 2.0 * r * w.cos() * y1 - r * r * y2 + rng.bipolar();
            y2 = y1;
            y1 = y;
            y
        })
        .collect();
    let pk = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    for v in &mut out {
        *v *= amp / pk;
    }
    out
}

fn white(secs: f32, amp: f32, seed: u32) -> Vec<f32> {
    let mut rng = lg_synth::dsp::Rng::new(seed);
    (0..(secs * RATE) as usize).map(|_| rng.bipolar() * amp).collect()
}

/// Renders `total` seconds with `sc` as the side chain (zero after it ends), block by block.
fn run(inst: &mut EvocPs, sc: &[f32], total: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (total * RATE) as usize;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    let mut pos = 0;
    while pos < n {
        let m = (n - pos).min(256);
        let blk: Vec<f32> = (0..m).map(|i| sc.get(pos + i).copied().unwrap_or(0.0)).collect();
        inst.side_chain(&blk, &blk);
        inst.render(&mut l[pos..pos + m], &mut r[pos..pos + m]);
        pos += m;
    }
    (l, r)
}

fn seg(x: &[f32], a: f32, b: f32) -> &[f32] {
    &x[(a * RATE) as usize..(b * RATE) as usize]
}

#[test]
fn stays_finite_everywhere() {
    sweep_all(&|| {
        let mut s = EvocPs::new(RATE);
        // feed something every block through a wrapper-free path: the sweep calls render only,
        // so prime the side chain once here
        let sc = band_noise(1500.0, 0.05, 0.5, 1);
        s.side_chain(&sc[..256], &sc[..256]);
        Box::new(s)
    });
}

#[test]
fn output_follows_the_side_chain_spectrum_and_envelope() {
    let mut s = evoc();
    s.note_on(48, 100);
    let sc = band_noise(1500.0, 0.4, 0.4, 3);
    let (l, _) = run(&mut s, &sc, 1.0);
    // while the burst plays the output holds energy around 1.5 kHz, little far from it
    let burst = seg(&l, 0.15, 0.38);
    let near = tone_band(burst, 1500.0);
    let far = tone_band(burst, 5200.0).max(tone_band(burst, 300.0));
    assert!(near > 4.0 * far, "near {near} far {far}");
    assert!(rms(burst) > 0.003, "audible: {}", rms(burst));
    // after the side chain stops (plus the release) the vocoder falls silent although the note holds
    let after = seg(&l, 0.8, 1.0);
    assert!(rms(after) < 0.05 * rms(burst), "silent after: {} vs {}", rms(after), rms(burst));
    // no side chain: no sound at all while the note plays
    let mut s2 = evoc();
    s2.note_on(48, 100);
    let silent = render(&mut s2, 0.5);
    assert!(rms(&silent) < 1e-6);
}

/// Energy around `hz` (several nearby bins, to catch noise).
fn tone_band(x: &[f32], hz: f32) -> f32 {
    [0.9, 0.95, 1.0, 1.05, 1.1].iter().map(|k| tone(x, hz * k)).sum::<f32>()
}

#[test]
fn attack_and_release_of_the_analysis() {
    let sc = band_noise(1500.0, 0.2, 0.4, 5);
    let rise = |att: f32| {
        let mut s = evoc();
        s.set(e::ATTACK, att);
        s.note_on(48, 100);
        let (l, _) = run(&mut s, &sc, 0.5);
        rms(seg(&l, 0.0, 0.03))
    };
    assert!(rise(0.2) > 1.5 * rise(80.0), "short attack reacts faster");
    let tail = |rel: f32| {
        let mut s = evoc();
        s.set(e::RELEASE, rel);
        s.note_on(48, 100);
        let (l, _) = run(&mut s, &sc, 0.8);
        rms(seg(&l, 0.25, 0.4))
    };
    assert!(tail(600.0) > 3.0 * tail(5.0), "long release sustains");
}

#[test]
fn freeze_holds_the_spectrum() {
    let mut s = evoc();
    s.note_on(48, 100);
    let sc = band_noise(1500.0, 0.3, 0.4, 7);
    run(&mut s, &sc, 0.3);
    s.set(e::FREEZE, 1.0);
    // silence in, yet the held spectrum keeps sounding; Release changes nothing now
    s.set(e::RELEASE, 1.0);
    let (l, _) = run(&mut s, &[], 0.5);
    assert!(rms(seg(&l, 0.2, 0.5)) > 0.003, "frozen: {}", rms(seg(&l, 0.2, 0.5)));
    // input is ignored while frozen
    let (l2, _) = run(&mut s, &white(0.3, 0.8, 1), 0.3);
    assert!(near_same(&l[(0.2 * RATE) as usize..], &l2[..]), "no change from new input");
}

fn near_same(a: &[f32], b: &[f32]) -> bool {
    let (ra, rb) = (rms(a), rms(&b[..a.len().min(b.len())]));
    (ra / rb - 1.0).abs() < 0.5
}

#[test]
fn band_count_and_formant_range() {
    let sc = band_noise(1500.0, 0.4, 0.4, 9);
    let level = |set: &[(u32, f32)]| {
        let mut s = evoc();
        for &(i, v) in set {
            s.set(i, v);
        }
        s.note_on(48, 100);
        let (l, _) = run(&mut s, &sc, 0.45);
        seg(&l, 0.15, 0.4).to_vec()
    };
    // a range that excludes 1.5 kHz removes it
    let wide = level(&[]);
    let narrow = level(&[(e::HIGH_FREQ, 1000.0), (e::LOW_FREQ, 100.0)]);
    assert!(tone_band(&narrow, 1500.0) < 0.3 * tone_band(&wide, 1500.0));
    // a few bands still pass the burst (fewer bands: less precise)
    let few = level(&[(e::BANDS, 5.0)]);
    assert!(rms(&few) > 0.001);
    let d = EvocPs::new(RATE);
    assert_eq!(d.display().n, 20);
}

#[test]
fn formant_shift_stretch_and_resonance() {
    let sc = band_noise(1500.0, 0.4, 0.4, 11);
    let out = |set: &[(u32, f32)]| {
        let mut s = evoc();
        for &(i, v) in set {
            s.set(i, v);
        }
        s.note_on(48, 100);
        let (l, _) = run(&mut s, &sc, 0.45);
        seg(&l, 0.15, 0.4).to_vec()
    };
    let flat = out(&[]);
    // shifting the synthesis bands up an octave moves the carrier's partner band away from 1.5 kHz
    let up = out(&[(e::SHIFT, 1.0)]);
    assert!(tone_band(&up, 1500.0) < 0.6 * tone_band(&flat, 1500.0));
    assert!(tone_band(&up, 3000.0) > tone_band(&flat, 3000.0));
    // stretch at 0 aligns the banks; a stretch changes the output
    let st = out(&[(e::STRETCH, 0.8)]);
    assert!(differs(&flat, &st) > 0.3);
    // resonance changes the character
    let lo = out(&[(e::RESONANCE, 0.0)]);
    let hi = out(&[(e::RESONANCE, 1.0)]);
    assert!(differs(&lo, &hi) > 0.2);
    // the display mirrors the settings at once
    let mut s = evoc();
    s.set(e::SHIFT, 1.0);
    let d = s.display();
    assert!((d.synthesis[5] / d.analysis[5] - 2.0).abs() < 0.05);
}

#[test]
fn lowest_highest_modes() {
    // white noise in: a high-pass lowest band passes more low-frequency energy than the band-pass
    let sc = white(0.4, 0.3, 13);
    let lowband = |mode: f32| {
        let mut s = evoc();
        s.set(e::LOWEST_MODE, mode);
        s.note_on(36, 100);
        let (l, _) = run(&mut s, &sc, 0.4);
        rms(&l[(0.15 * RATE) as usize..])
    };
    assert!(lowband(0.0).is_finite() && lowband(1.0).is_finite());
    let hi = |mode: f32| {
        let mut s = evoc();
        s.set(e::HIGHEST_MODE, mode);
        s.set(e::HIGH_FREQ, 3000.0);
        s.note_on(36, 100);
        let (l, _) = run(&mut s, &sc, 0.4);
        bright(&l[(0.15 * RATE) as usize..], 6000.0)
    };
    assert!(hi(1.0) != hi(0.0));
}

#[test]
fn unvoiced_detection_modes() {
    // a hissy side chain (white noise): U/V replaces the synth by noise, which changes the sound
    let sc = white(0.5, 0.3, 21);
    let out = |mode: f32, sens: f32, lvl: f32| {
        let mut s = evoc();
        s.set(e::UV_MODE, mode);
        s.set(e::UV_SENS, sens);
        s.set(e::UV_LEVEL, lvl);
        s.note_on(48, 100);
        let (l, _) = run(&mut s, &sc, 0.5);
        l[(0.25 * RATE) as usize..].to_vec()
    };
    let off = out(0.0, 0.0, 0.0);
    let noise = out(0.0, 1.0, 0.8);
    assert!(differs(&off, &noise) > 0.1, "noise replaces the unvoiced part");
    let both = out(1.0, 1.0, 0.8);
    assert!(rms(&both) > 0.0);
    // Blend ignores sensitivity
    let b1 = out(2.0, 0.0, 0.8);
    let b2 = out(2.0, 1.0, 0.8);
    assert!(differs(&b1, &b2) < 1e-4);
    // the level scales the replacement
    assert!(rms(&out(0.0, 1.0, 1.0)) != rms(&out(0.0, 1.0, 0.1)));
    // a low voiced-sounding side chain does not trigger the detector
    let low = band_noise(300.0, 0.5, 0.3, 4);
    let mut s = evoc();
    s.set(e::UV_SENS, 0.3);
    s.set(e::UV_LEVEL, 1.0);
    s.note_on(48, 100);
    let (a, _) = run(&mut s, &low, 0.5);
    let mut s = evoc();
    s.set(e::UV_SENS, 0.3);
    s.set(e::UV_LEVEL, 0.0);
    s.note_on(48, 100);
    let (b, _) = run(&mut s, &low, 0.5);
    assert!(differs(&a[12000..], &b[12000..]) < 0.05, "low content is voiced: no replacement");
}

#[test]
fn oscillators_noise_tuning_and_envelope() {
    // Syn output: the synth alone, no side chain needed
    let syn = |set: &[(u32, f32)], note: u8| {
        let mut s = evoc();
        s.set(e::SIGNAL, 1.0);
        for &(i, v) in set {
            s.set(i, v);
        }
        s.note_on(note, 100);
        render(&mut s, 0.5)
    };
    let a = syn(&[(e::WAVE1, 1.0), (e::BALANCE, 0.0), (e::DETUNE, 0.0)], 57);
    let f = fundamental(&a[4800..], 150.0, 300.0);
    assert!((f / 220.0 - 1.0).abs() < 0.02, "8' at {f}");
    // 16' is an octave down, 4' an octave up
    let lo = syn(&[(e::OCTAVE, 0.0), (e::BALANCE, 0.0), (e::DETUNE, 0.0)], 57);
    assert!((fundamental(&lo[4800..], 80.0, 150.0) / 110.0 - 1.0).abs() < 0.03);
    let hi = syn(&[(e::OCTAVE, 2.0), (e::BALANCE, 0.0), (e::DETUNE, 0.0)], 57);
    assert!((fundamental(&hi[4800..], 300.0, 600.0) / 440.0 - 1.0).abs() < 0.03);
    // waveforms differ; semi tunes osc 2 (balance fully on it)
    let w2 = syn(&[(e::WAVE1, 40.0), (e::BALANCE, 0.0), (e::DETUNE, 0.0)], 57);
    assert!(differs(&a[4800..], &w2[4800..]) > 0.3);
    let s12 = syn(&[(e::BALANCE, 1.0), (e::SEMI, 12.0), (e::DETUNE, 0.0), (e::WAVE2, 1.0)], 57);
    assert!((fundamental(&s12[4800..], 300.0, 600.0) / 440.0 - 1.0).abs() < 0.03);
    // FM mode: wave 1 has no effect; Int adds overtones
    let fm = |w1: f32, int: f32| syn(&[(e::OSC_MODE, 1.0), (e::WAVE1, w1), (e::FM_INT, int), (e::FM_COARSE, 12.0), (e::WAVE2, 1.0)], 57);
    assert!(differs(&fm(0.0, 0.5)[4800..], &fm(40.0, 0.5)[4800..]) < 1e-4);
    assert!(bright(&fm(0.0, 1.0)[4800..], 1500.0) > 2.0 * bright(&fm(0.0, 0.0)[4800..], 1500.0));
    // noise: level adds, colour moves it up
    let quiet = syn(&[(e::BALANCE, 0.0), (e::NOISE_LEVEL, 0.0), (e::WAVE1, 0.0)], 57);
    let noisy = syn(&[(e::BALANCE, 0.0), (e::NOISE_LEVEL, 0.6), (e::WAVE1, 0.0)], 57);
    assert!(rms(&noisy[4800..]) > 1.3 * rms(&quiet[4800..]));
    let white_n = syn(&[(e::OCTAVE, 0.0), (e::BALANCE, 0.0), (e::WAVE1, 0.0), (e::NOISE_LEVEL, 1.0), (e::NOISE_COLOR, 0.0), (e::SEMI, 0.0)], 40);
    let blue_n = syn(&[(e::OCTAVE, 0.0), (e::BALANCE, 0.0), (e::WAVE1, 0.0), (e::NOISE_LEVEL, 1.0), (e::NOISE_COLOR, 1.0)], 40);
    let ratio = |x: &[f32]| bright(&x[4800..], 6000.0) / rms(&x[4800..]);
    assert!(ratio(&blue_n) > ratio(&white_n));
    // tune in cents, bend range
    let t = syn(&[(e::TUNE, 100.0), (e::BALANCE, 0.0), (e::DETUNE, 0.0)], 57);
    assert!((fundamental(&t[4800..], 150.0, 300.0) / 233.1 - 1.0).abs() < 0.02);
    // filter cutoff darkens before the formant banks
    let open = syn(&[(e::CUTOFF, 1.0)], 57);
    let dark = syn(&[(e::CUTOFF, 0.2)], 57);
    assert!(bright(&dark[4800..], 2000.0) < 0.3 * bright(&open[4800..], 2000.0));
    // envelope attack and release
    let ar = |att: f32| {
        let mut s = evoc();
        s.set(e::SIGNAL, 1.0);
        s.set(e::ENV_ATTACK, att);
        s.note_on(57, 100);
        render(&mut s, 0.2)
    };
    assert!(rms(&ar(0.0)[..2400]) > 3.0 * rms(&ar(1.0)[..2400]));
    let mut s = evoc();
    s.set(e::SIGNAL, 1.0);
    s.set(e::ENV_RELEASE, 1.0);
    s.note_on(57, 100);
    render(&mut s, 0.2);
    s.note_off(57);
    let rel = render(&mut s, 0.3);
    assert!(rms(&rel[..4800]) > 0.1 * rms(&a[4800..9600]));
}

#[test]
fn voice_modes_glide_and_unison() {
    let mut s = evoc();
    s.set(e::SIGNAL, 1.0);
    s.set(e::BALANCE, 0.0);
    s.set(e::DETUNE, 0.0);
    // poly: two keys, two pitches
    s.note_on(48, 100);
    s.note_on(60, 100);
    let x = render(&mut s, 0.4);
    assert!(tone(&x[4800..], 130.8) > 0.02 && tone(&x[4800..], 261.6) > 0.02);
    // mono: only the last key sounds, glide always active
    let mut m = evoc();
    m.set(e::SIGNAL, 1.0);
    m.set(e::BALANCE, 0.0);
    m.set(e::DETUNE, 0.0);
    m.set(e::VOICE_MODE, 1.0);
    m.set(e::GLIDE, 0.5);
    m.note_on(48, 100);
    render(&mut m, 0.3);
    m.note_on(60, 100);
    let y = render(&mut m, 0.1);
    let f_early = fundamental(&y[..2400], 100.0, 300.0);
    assert!(f_early < 255.0, "gliding: {f_early}");
    // legato: tied notes do not retrigger (the envelope keeps its level)
    let mut g = evoc();
    g.set(e::SIGNAL, 1.0);
    g.set(e::VOICE_MODE, 2.0);
    g.set(e::ENV_ATTACK, 0.0);
    g.note_on(48, 100);
    render(&mut g, 0.3);
    g.note_on(55, 100);
    let z = render(&mut g, 0.05);
    assert!(rms(&z[..480]) > 0.5 * rms(&z[2000..]));
    // voices: 8 poly voices by default; Unison halves them to at most 8 and louder stack
    let mut u = evoc();
    u.set(e::VOICES, 16.0);
    assert_eq!(u.voices_shown(), 16);
    u.set(e::UNISON, 1.0);
    assert_eq!(u.voices_shown(), 8);
    u.set(e::VOICE_MODE, 1.0);
    assert_eq!(u.voices_shown(), 16);
    let level = |uni: f32| {
        let mut s = evoc();
        s.set(e::SIGNAL, 1.0);
        s.set(e::VOICE_MODE, 1.0);
        s.set(e::UNISON, uni);
        s.set(e::ANALOG, 0.5);
        s.note_on(48, 100);
        rms(&render(&mut s, 0.4)[4800..])
    };
    assert!(level(1.0) > level(0.0), "stacking raises the volume");
}

#[test]
fn lfos_and_tempo_sync() {
    // Pitch LFO: vibrato widens the pitch (dual osc, syn out)
    let mk = |int: f32| {
        let mut s = evoc();
        s.set(e::SIGNAL, 1.0);
        s.set(e::BALANCE, 0.0);
        s.set(e::DETUNE, 0.0);
        s.set(e::PITCH_INT_MIN, int);
        s.set(e::PITCH_INT_MAX, int);
        s.set(e::PITCH_RATE, 0.8);
        s.note_on(57, 100);
        render(&mut s, 1.0)
    };
    let steady = mk(0.0);
    let vib = mk(1.0);
    assert!(differs(&steady[4800..], &vib[4800..]) > 0.3);
    // the wheel sets the intensity between min and max
    let mut s = evoc();
    s.set(e::SIGNAL, 1.0);
    s.set(e::BALANCE, 0.0);
    s.set(e::PITCH_INT_MIN, 0.0);
    s.set(e::PITCH_INT_MAX, 1.0);
    s.set(e::PITCH_RATE, 0.8);
    s.note_on(57, 100);
    let a = render(&mut s, 0.5);
    s.controller(1, 127);
    let b = render(&mut s, 0.5);
    let wob = |x: &[f32]| (0..4).map(|k| fundamental(&x[2400 + k * 4000..6400 + k * 4000], 150.0, 300.0)).fold((1e9f32, 0.0f32), |(lo, hi), f| (lo.min(f), hi.max(f)));
    let (wa, wb) = (wob(&a), wob(&b));
    assert!(wb.1 - wb.0 > wa.1 - wa.0);
    // rate: left of centre is tempo-synced, right is Hz
    assert_eq!(e::sync_division(0.2), Some(4));
    assert!(e::sync_division(0.7).is_none() && e::free_hz(0.7) > 0.05);
    assert!(e::rate_text(0.1).contains("Bar") && e::rate_text(0.9).contains("Hz"));
    // Shift LFO sweeps the formant shift: the vocoder output changes over time
    let sc = band_noise(1500.0, 1.0, 0.4, 15);
    let mut v = evoc();
    v.set(e::SHIFT_INT, 1.0);
    v.set(e::SHIFT_RATE, 0.85);
    v.note_on(48, 100);
    let (l, _) = run(&mut v, &sc, 1.0);
    let mut w = evoc();
    w.note_on(48, 100);
    let (l0, _) = run(&mut w, &sc, 1.0);
    assert!(differs(&l0[9600..], &l[9600..]) > 0.2);
}

#[test]
fn output_signal_level_ensemble_and_width() {
    let sc = band_noise(1500.0, 0.5, 0.4, 17);
    let go = |set: &[(u32, f32)]| {
        let mut s = evoc();
        for &(i, v) in set {
            s.set(i, v);
        }
        s.note_on(48, 100);
        run(&mut s, &sc, 0.5)
    };
    // Ana outputs the analysis input itself, Syn only the synth, Voc the vocoder
    let (ana, _) = go(&[(e::SIGNAL, 2.0)]);
    assert!(differs(&ana[..24000], &sc[..24000]) < 0.01);
    let (syn, _) = go(&[(e::SIGNAL, 1.0)]);
    let (voc, _) = go(&[(e::SIGNAL, 0.0)]);
    assert!(differs(&syn[12000..], &voc[12000..]) > 0.3);
    // level: -12 dB is a quarter of the amplitude
    let (a, _) = go(&[]);
    let (b, _) = go(&[(e::LEVEL, -12.0)]);
    assert!((rms(&b[9600..]) / rms(&a[9600..]) - 0.251).abs() < 0.02);
    // ensemble changes the sound (I and II differ)
    let (o, _) = go(&[(e::ENSEMBLE, 0.0)]);
    let (i1, _) = go(&[(e::ENSEMBLE, 1.0)]);
    let (i2, _) = go(&[(e::ENSEMBLE, 2.0)]);
    assert!(differs(&o[9600..], &i1[9600..]) > 0.05 && differs(&i1[9600..], &i2[9600..]) > 0.05);
    // stereo width: 0 is centred (L = R), middle spreads, full alternates
    let (l0, r0) = go(&[(e::WIDTH, 0.0)]);
    assert!(differs(&l0[9600..], &r0[9600..]) < 1e-3, "centred");
    let (lm, rm) = go(&[(e::WIDTH, 0.5)]);
    assert!(differs(&lm[9600..], &rm[9600..]) > 0.1);
    let (lf, rf) = go(&[(e::WIDTH, 1.0)]);
    assert!(differs(&lf[9600..], &rf[9600..]) > 0.1);
    // a wide-band burst with width 0.5: ascending bands put low frequencies left, high right
    let wide = white(0.5, 0.3, 23);
    let mut s = evoc();
    s.set(e::WIDTH, 0.5);
    s.note_on(48, 100);
    let (l, r) = run(&mut s, &wide, 0.5);
    let lo = (tone_band(&l[9600..], 200.0) / tone_band(&r[9600..], 200.0).max(1e-9), 0);
    let hi = (tone_band(&l[9600..], 6000.0) / tone_band(&r[9600..], 6000.0).max(1e-9), 0);
    assert!(lo.0 > hi.0, "low bands left, high bands right: {} vs {}", lo.0, hi.0);
}

#[test]
fn synth_overload_and_gaps() {
    // loud side chain plus loud level stays finite
    let sc = white(0.3, 1.0, 29);
    let mut s = evoc();
    s.set(e::LEVEL, 12.0);
    s.set(e::UV_LEVEL, 1.0);
    s.set(e::UV_SENS, 1.0);
    for n in [36u8, 48, 60, 72] {
        s.note_on(n, 127);
    }
    let (l, r) = run(&mut s, &sc, 0.3);
    assert!(finite(&l) && finite(&r));
    // a gap in the synth signal stops the vocoder output; a longer synth Release bridges it
    let sc = band_noise(1500.0, 0.6, 0.4, 31);
    let gap = |rel: f32| {
        let mut s = evoc();
        s.set(e::ENV_RELEASE, rel);
        s.note_on(48, 100);
        let mut l = run(&mut s, &sc[..14400], 0.3).0;
        s.note_off(48);
        s.note_on(48, 100);
        l.extend(run(&mut s, &sc[14400..], 0.3).0);
        rms(&l[(0.3 * RATE) as usize..(0.35 * RATE) as usize])
    };
    assert!(gap(2.0) > 0.0);
}
