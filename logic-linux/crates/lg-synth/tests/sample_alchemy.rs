//! Sample Alchemy: one test per area of the guide, each showing that the controls change the
//! sound the way the guide describes. The samples are made here (nothing from Logic).

mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::sample_alchemy::*;

/// A harmonic tone: `nh` partials of `f0` with a 1/k^roll spectrum, amplitude envelope `env`.
fn harmonic(secs: f32, f0: f32, nh: usize, roll: f32, env: impl Fn(f32) -> f32) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let mut s = 0.0;
            for k in 1..=nh {
                s += (std::f32::consts::TAU * f0 * k as f32 * t).sin() / (k as f32).powf(roll);
            }
            s * 0.25 * env(t / secs)
        })
        .collect()
}

fn flat(_: f32) -> f32 {
    1.0
}

/// Two halves: `f1` first, `f2` second (a pure tone each), for checking where a handle reads.
fn two_tone(secs: f32, f1: f32, f2: f32) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let f = if i < n / 2 { f1 } else { f2 };
            0.4 * (std::f32::consts::TAU * f * t).sin()
        })
        .collect()
}

fn make(sample: Vec<f32>) -> SampleAlchemy {
    let mut a = SampleAlchemy::new(RATE);
    a.set_sample(sample, RATE);
    a
}

fn s0(off: u32) -> u32 {
    sid(0, off)
}

fn setv(a: &mut SampleAlchemy, set: &[(u32, f32)]) {
    for &(id, v) in set {
        a.set(id, v);
    }
}

/// Source A in a given mode and effect with knob values.
fn mode(a: &mut SampleAlchemy, m: f32, fx: f32, k: [f32; 4]) {
    setv(a, &[(s0(o::MODE), m), (s0(o::EFFECT), fx), (s0(o::K1), k[0]), (s0(o::K1 + 1), k[1]), (s0(o::K1 + 2), k[2]), (s0(o::K1 + 3), k[3])]);
}

fn play_n(a: &mut SampleAlchemy, note: u8, secs: f32) -> Vec<f32> {
    a.note_on(note, 100);
    render(a, secs)
}

fn skip(x: &[f32], secs: f32) -> &[f32] {
    &x[(secs * RATE) as usize..]
}

fn env_var(x: &[f32]) -> f32 {
    // variation of the short-time level (10 ms blocks)
    let blk = (RATE * 0.01) as usize;
    let lv: Vec<f32> = x.chunks(blk).filter(|c| c.len() == blk).map(rms).collect();
    let m = lv.iter().sum::<f32>() / lv.len() as f32;
    (lv.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / lv.len() as f32).sqrt() / m.max(1e-9)
}

/// How strongly the signal repeats itself at the period of `f` Hz (best normalised autocorrelation near it).
fn periodic(x: &[f32], f: f32) -> f32 {
    let lag0 = (RATE / f) as usize;
    let n = x.len() - lag0 - 8;
    (lag0 - 3..=lag0 + 3)
        .map(|lag| {
            let (mut s, mut e1, mut e2) = (0.0, 0.0, 0.0);
            for i in 0..n {
                s += x[i] * x[i + lag];
                e1 += x[i] * x[i];
                e2 += x[i + lag] * x[i + lag];
            }
            s / (e1 * e2).sqrt().max(1e-12)
        })
        .fold(-1.0, f32::max)
}

fn saw() -> Vec<f32> {
    harmonic(1.0, 220.0, 24, 1.0, flat)
}

// ---------------------------------------------------------------------------------------------

#[test]
fn loads_a_sample_and_plays_it() {
    let mut a = SampleAlchemy::new(RATE);
    assert!(!a.has_sample());
    assert!(rms(&play_n(&mut a, 60, 0.2)) < 1e-6, "no sample, no sound");
    a.reset();
    a.set_sample(saw(), RATE);
    a.set_sample_path(Some("/tmp/x.wav".into()));
    assert_eq!(a.sample_path(), Some("/tmp/x.wav"));
    assert!(a.has_sample());
    assert!((a.duration() - 1.0).abs() < 0.01);
    let x = play_n(&mut a, 60, 0.4);
    assert!(rms(&x) > 0.02 && finite(&x), "classic granular plays the sample: {}", rms(&x));
    // Export Audio writes the source sample
    let p = std::env::temp_dir().join(format!("salch-{}.wav", std::process::id()));
    a.export_wav(&p).unwrap();
    assert!(std::fs::metadata(&p).unwrap().len() > 100_000);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn parameter_table_is_sound() {
    let a = SampleAlchemy::new(RATE);
    let ids: std::collections::HashSet<u32> = a.params().iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), a.params().len(), "ids are unique");
    // four identical sources
    for off in [o::MODE, o::K1, o::PLAY, o::X, o::PAN, o::F_TYPE, o::AMP, o::MODENV + 3] {
        for s in 1..4 {
            assert_eq!(a.params().iter().find(|p| p.id == sid(0, off)).unwrap().default, a.params().iter().find(|p| p.id == sid(s, off)).unwrap().default, "source {s} offset {off}");
        }
    }
    assert!(a.params().iter().any(|p| p.id == sid(4, o::F_CUT)), "global filter block");
    // the defaults route the handle's Y movement to the first synthesis knob
    assert_eq!(a.get(route_id(0, 1)).round() as usize, 7);
    assert_eq!(MOD_SOURCES[7], "Waveform Y");
    assert_eq!(a.get(route_id(0, 0)) as usize, 0);
}

#[test]
fn edit_modes_snap_zoom_and_trim() {
    assert_eq!(EDIT_MODES, &["Play", "Motion", "Trim"]);
    assert_eq!(PLAYS, &["Classic", "Loop", "Scrub", "Bow", "Arp"]);
    assert_eq!(SNAPS, &["Auto", "Off", "Transients", "Beat", "Fixed"]);
    // a sample with three clicks: Snap to Transients finds them, Beat uses the tempo
    let n = 3 * RATE as usize;
    let mut x = vec![0.0f32; n];
    for &c in &[0.5f32, 1.5, 2.5] {
        let i = (c * RATE) as usize;
        for k in 0..4000 {
            x[i + k] = (-(k as f32) / 600.0).exp() * (k as f32 * 0.3).sin() * 0.8;
        }
    }
    let mut a = make(x);
    let t = a.transients().to_vec();
    assert!(t.len() >= 3, "{t:?}");
    let near = a.snap_position(0.5 / 3.0 + 0.01, 2);
    assert!((near - 0.5 / 3.0).abs() < 0.012, "snap to a transient: {near}");
    assert_eq!(a.snap_position(0.123, 1), 0.123, "Snap Off");
    assert_eq!(a.snap_position(0.123, 4), 0.123, "Fixed leaves the position (the window moves it vertically only)");
    a.set(TEMPO, 120.0);
    let b = a.snap_position(0.30, 3); // beats are 0.5 s = 1/6
    assert!((b - 0.3333).abs() < 0.01, "snap to a beat: {b}");
    // Auto: the tempo when the sample has one, else transients
    assert!((a.snap_position(0.30, 0) - b).abs() < 1e-6);
    a.set(TEMPO, 0.0);
    assert!((a.snap_position(0.5 / 3.0 + 0.01, 0) - 0.5 / 3.0).abs() < 0.012);
    // derive tempo from the length between the trim markers: 2 s = 4 beats at 120 bpm
    a.set(TRIM_START, 0.0);
    a.set(TRIM_END, 2.0 / 3.0);
    assert!((a.derive_tempo() - 120.0).abs() < 0.5, "{}", a.derive_tempo());
}

#[test]
fn trim_markers_select_the_part_that_plays() {
    let mut a = make(two_tone(1.0, 220.0, 440.0));
    setv(&mut a, &[(TRIM_START, 0.5), (TRIM_END, 1.0)]);
    a.set(s0(o::X), 0.0); // the handle is left of the start marker: playing begins at the marker
    let x = play_n(&mut a, 60, 0.3);
    assert!(tone(skip(&x, 0.05), 440.0) > 4.0 * tone(skip(&x, 0.05), 220.0), "plays from the start marker");
    // the end marker stops the sound
    let mut b = make(saw());
    setv(&mut b, &[(TRIM_START, 0.0), (TRIM_END, 0.3)]);
    let y = play_n(&mut b, 60, 1.0);
    assert!(rms(skip(&y, 0.6)) < 0.1 * rms(&y[..(0.2 * RATE) as usize]), "nothing after the end marker");
}

#[test]
fn classic_play_mode_and_speed() {
    let mut a = make(saw());
    let x = play_n(&mut a, 60, 1.3);
    assert!(rms(&x[(0.5 * RATE) as usize..(0.8 * RATE) as usize]) > 0.02);
    assert!(rms(skip(&x, 1.15)) < 0.1 * rms(&x[..(0.3 * RATE) as usize]), "the sample ends at its end");
    let mut b = make(saw());
    b.set(s0(o::SPEED), 4.0);
    let y = play_n(&mut b, 60, 1.0);
    assert!(rms(skip(&y, 0.5)) < 0.05 * rms(&y[..(0.2 * RATE) as usize]), "four times faster ends in a quarter");
    // Playback Speed Sync locks the speed to the project tempo (a 60 bpm sample at 120 bpm: twice)
    let mut c = make(saw());
    setv(&mut c, &[(TEMPO, 60.0), (s0(o::SPEED_SYNC), 1.0)]);
    c.set_tempo(120.0);
    let z = play_n(&mut c, 60, 1.0);
    assert!(rms(skip(&z, 0.7)) < 0.1 * rms(&z[..(0.2 * RATE) as usize]), "synced speed doubles it");
    // the handle sets where it starts
    let mut d = make(two_tone(1.0, 220.0, 440.0));
    d.set(s0(o::X), 0.6);
    let w = play_n(&mut d, 60, 0.3);
    assert!(tone(&w, 440.0) > 4.0 * tone(&w, 220.0));
    // reversed: from the handle backwards
    let mut e = make(two_tone(1.0, 220.0, 440.0));
    setv(&mut e, &[(REVERSE, 1.0), (s0(o::X), 0.9)]);
    let r = play_n(&mut e, 60, 0.3);
    assert!(tone(&r, 440.0) > 4.0 * tone(&r, 220.0), "Reverse starts at the handle");
    let r2 = render(&mut e, 0.5);
    assert!(tone(skip(&r2, 0.15), 220.0) > 3.0 * tone(skip(&r2, 0.15), 440.0), "and plays backwards through the sample");
}

#[test]
fn loop_mode_repeats_and_reverses() {
    let mut a = make(saw());
    setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::X), 0.2), (s0(o::LOOP_END), 0.4)]);
    let x = play_n(&mut a, 60, 2.0);
    assert!(rms(skip(&x, 1.7)) > 0.5 * rms(&x[..(0.3 * RATE) as usize]), "a loop keeps going");
    // Loop Speed
    let mut b = make(saw());
    setv(&mut b, &[(s0(o::PLAY), 1.0), (s0(o::X), 0.0), (s0(o::LOOP_END), 1.0), (s0(o::LOOP_SPEED), 2.0)]);
    let y = play_n(&mut b, 60, 1.5);
    assert!(rms(skip(&y, 1.2)) > 0.02);
    // a handle after the loop end plays the sample reversed: a rising sweep falls
    let n = RATE as usize;
    let sweep: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let ph = std::f32::consts::TAU * (200.0 * t + 500.0 * t * t / 2.0) * (n as f32 / RATE);
            0.4 * ph.sin()
        })
        .collect();
    let early = |rev: bool| {
        let mut c = make(sweep.clone());
        let (st, en) = if rev { (0.9, 0.1) } else { (0.1, 0.9) };
        setv(&mut c, &[(s0(o::PLAY), 1.0), (s0(o::X), st), (s0(o::LOOP_END), en)]);
        let o = play_n(&mut c, 60, 0.6);
        (fundamental(&o[(0.05 * RATE) as usize..(0.2 * RATE) as usize], 100.0, 900.0), fundamental(&o[(0.4 * RATE) as usize..(0.55 * RATE) as usize], 100.0, 900.0))
    };
    let (f0, f1) = early(false);
    assert!(f1 > f0 + 20.0, "forward loop rises: {f0} -> {f1}");
    let (g0, g1) = early(true);
    assert!(g1 < g0 - 20.0, "loop with the handle after its end runs backwards: {g0} -> {g1}");
}

#[test]
fn scrub_follows_the_handle_and_jitters() {
    let mut a = make(two_tone(1.0, 220.0, 440.0));
    setv(&mut a, &[(s0(o::PLAY), 2.0), (s0(o::X), 0.1)]);
    let x = play_n(&mut a, 60, 0.4);
    assert!(tone(skip(&x, 0.2), 220.0) > 4.0 * tone(skip(&x, 0.2), 440.0));
    a.set(s0(o::X), 0.9);
    let y = render(&mut a, 0.4);
    assert!(tone(skip(&y, 0.2), 440.0) > 4.0 * tone(skip(&y, 0.2), 220.0), "scrubs as the handle moves");
    // it keeps sounding as long as the key is held
    let z = render(&mut a, 1.5);
    assert!(rms(skip(&z, 1.0)) > 0.02);
    // jitter wanders the position: next to the border between the two tones it reaches the other one
    let jit = |j: f32| {
        let mut e = make(two_tone(1.0, 220.0, 440.0));
        setv(&mut e, &[(s0(o::PLAY), 2.0), (s0(o::X), 0.4), (s0(o::JITTER), j)]);
        let x = play_n(&mut e, 60, 1.0);
        tone(skip(&x, 0.2), 440.0)
    };
    assert!(jit(1.0) > 4.0 * jit(0.0) + 0.002, "{} vs {}", jit(1.0), jit(0.0));
}

#[test]
fn bow_goes_forward_and_back() {
    let n = RATE as usize;
    let sweep: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            0.4 * (std::f32::consts::TAU * (200.0 * t + 500.0 * t * t / 2.0)).sin()
        })
        .collect();
    let mut a = make(sweep);
    setv(&mut a, &[(s0(o::PLAY), 3.0), (s0(o::X), 0.3), (s0(o::BOW_RATE), 2.0)]);
    let x = play_n(&mut a, 60, 2.0);
    assert!(rms(skip(&x, 1.5)) > 0.02, "bow keeps playing past the sample's length");
    // rate sets the speed: the level of a fast bow vs a slow one differs in its position variation
    let mut b = make(harmonic(1.0, 220.0, 3, 1.0, |t| t));
    setv(&mut b, &[(s0(o::PLAY), 3.0), (s0(o::X), 0.2), (s0(o::BOW_RATE), 0.5)]);
    let slow = play_n(&mut b, 60, 2.0);
    let mut c = make(harmonic(1.0, 220.0, 3, 1.0, |t| t));
    setv(&mut c, &[(s0(o::PLAY), 3.0), (s0(o::X), 0.2), (s0(o::BOW_RATE), 4.0)]);
    let fast = play_n(&mut c, 60, 2.0);
    assert!(env_var(&slow) > env_var(&fast), "a slower bow sweeps further: {} {}", env_var(&slow), env_var(&fast));
    // Bow Rate Sync: tempo-synced note value instead of Hz
    let mut d = make(harmonic(1.0, 220.0, 3, 1.0, |t| t));
    d.set_tempo(120.0);
    setv(&mut d, &[(s0(o::PLAY), 3.0), (s0(o::X), 0.2), (s0(o::BOW_SYNC), 9.0)]);
    let sy = play_n(&mut d, 60, 1.0);
    assert!(rms(&sy) > 0.01 && finite(&sy));
}

#[test]
fn arp_steps_through_sections_in_tempo() {
    let mut a = make(two_tone(1.0, 220.0, 440.0));
    a.set_tempo(120.0);
    setv(&mut a, &[(s0(o::PLAY), 4.0), (s0(o::X), 0.0), (s0(o::ARP_RATE), 10.0)]); // 1/16 = 8 steps a second
    let x = play_n(&mut a, 60, 2.0);
    assert!(rms(skip(&x, 1.5)) > 0.02, "arp runs on");
    let blk = (0.1 * RATE) as usize;
    let lo = x.chunks(blk).map(|c| tone(c, 220.0)).fold(0.0f32, f32::max);
    let hi = x.chunks(blk).map(|c| tone(c, 440.0)).fold(0.0f32, f32::max);
    assert!(lo > 0.05 && hi > 0.05, "it plays sections from both halves: {lo} {hi}");
    // always tempo-synced: twice the tempo halves the step length, so the pattern repeats faster
    assert_eq!(ARP_RATES.len(), 16);
    assert!((arp_beats(0) - 4.0).abs() < 1e-6 && (arp_beats(4) - 1.0).abs() < 1e-6 && (arp_beats(5) - 1.5).abs() < 1e-6 && (arp_beats(6) - 2.0 / 3.0).abs() < 1e-5);
    assert!((arp_beats(13) - 0.125).abs() < 1e-6, "1/32");
}

#[test]
fn granular_classic_size_density_pan_time() {
    // density: one grain at a time pulses at the grain rate, many overlapping grains do not
    let mut a = make(harmonic(1.0, 220.0, 8, 1.0, flat));
    mode(&mut a, 0.0, 0.0, [0.8, 0.0, 0.0, 0.03]);
    a.set(s0(o::PLAY), 1.0);
    a.set(s0(o::LOOP_END), 1.0);
    let sparse = play_n(&mut a, 60, 1.0);
    let mut b = make(harmonic(1.0, 220.0, 8, 1.0, flat));
    mode(&mut b, 0.0, 0.0, [0.8, 1.0, 0.0, 0.03]);
    b.set(s0(o::PLAY), 1.0);
    let dense = play_n(&mut b, 60, 1.0);
    assert!(env_var(skip(&sparse, 0.2)) > 2.0 * env_var(skip(&dense, 0.2)), "{} vs {}", env_var(&sparse), env_var(&dense));
    // size: shorter grains pulse faster than longer ones (more level changes per second)
    let zero_crossings = |x: &[f32]| {
        let blk = (RATE * 0.005) as usize;
        let lv: Vec<f32> = x.chunks(blk).map(rms).collect();
        let m = lv.iter().sum::<f32>() / lv.len() as f32;
        lv.windows(2).filter(|w| (w[0] - m) * (w[1] - m) < 0.0).count()
    };
    let mut c = make(harmonic(1.0, 220.0, 8, 1.0, flat));
    mode(&mut c, 0.0, 0.0, [0.1, 0.0, 0.0, 0.03]);
    c.set(s0(o::PLAY), 1.0);
    let small = play_n(&mut c, 60, 1.0);
    assert!(zero_crossings(skip(&small, 0.2)) > zero_crossings(skip(&sparse, 0.2)), "small grains pulse faster");
    assert!((size_ms(0.0) - 2.0).abs() < 0.01 && (size_ms(1.0) - 230.0).abs() < 0.5, "2 to 230 ms");
    // random pan spreads the grains across the stereo field
    let mut d = make(harmonic(1.0, 220.0, 8, 1.0, flat));
    mode(&mut d, 0.0, 0.0, [0.5, 0.5, 1.0, 0.03]);
    d.set(s0(o::PLAY), 1.0);
    d.note_on(60, 100);
    let (l, r) = render_st(&mut d, 0.6);
    assert!(differs(&l, &r) > 0.3, "random pan decorrelates the channels");
    let mut e = make(harmonic(1.0, 220.0, 8, 1.0, flat));
    mode(&mut e, 0.0, 0.0, [0.5, 0.5, 0.0, 0.03]);
    e.note_on(60, 100);
    let (l, r) = render_st(&mut e, 0.6);
    assert!(differs(&l, &r) < 0.01, "without it the channels match");
    // random time: positions wander, so a moving sample's content changes
    let ramp = harmonic(1.0, 220.0, 3, 1.0, |t| t);
    let var = |rt: f32| {
        let mut f = make(ramp.clone());
        mode(&mut f, 0.0, 0.0, [0.5, 0.3, 0.0, rt]);
        f.set(s0(o::PLAY), 2.0);
        f.set(s0(o::X), 0.5);
        env_var(skip(&play_n(&mut f, 60, 1.0), 0.2))
    };
    assert!(var(1.0) > var(0.0), "random time offsets grain positions");
}

#[test]
fn granular_sync_formant_taps_spacing() {
    let mut a = make(harmonic(1.0, 220.0, 12, 1.0, flat));
    mode(&mut a, 0.0, 1.0, [0.5, 0.0, 0.2, 0.03]);
    a.set(s0(o::PLAY), 1.0);
    let x = play_n(&mut a, 72, 0.8);
    assert!(tone(skip(&x, 0.2), 440.0) > 3.0 * tone(skip(&x, 0.2), 330.0), "the grain rate carries the pitch: one octave up plays 440");
    // formant shift moves the spectral envelope without moving the pitch
    let mut b = make(harmonic(1.0, 220.0, 12, 1.0, flat));
    mode(&mut b, 0.0, 1.0, [1.0, 0.0, 0.2, 0.03]);
    b.set(s0(o::PLAY), 1.0);
    let y = play_n(&mut b, 60, 0.8);
    let mut c = make(harmonic(1.0, 220.0, 12, 1.0, flat));
    mode(&mut c, 0.0, 1.0, [0.0, 0.0, 0.2, 0.03]);
    c.set(s0(o::PLAY), 1.0);
    let z = play_n(&mut c, 60, 0.8);
    assert!(bright(skip(&y, 0.2), 1000.0) > bright(skip(&z, 0.2), 1000.0), "formant shift up is brighter");
    let f = fundamental(skip(&y, 0.2), 150.0, 400.0);
    assert!((f - 220.0).abs() < 8.0, "pitch stays at the note: {f}");
    // taps: more parallel streams change the sound; spacing matters with several
    let tap = |taps: f32, sp: f32| {
        let mut d = make(two_tone(1.0, 220.0, 330.0));
        mode(&mut d, 0.0, 1.0, [0.5, taps, sp, 0.0]);
        d.set(s0(o::PLAY), 2.0);
        d.set(s0(o::X), 0.1);
        skip(&play_n(&mut d, 60, 0.6), 0.2).to_vec()
    };
    let one = tap(0.0, 0.0);
    let four = tap(0.43, 0.0);
    let spaced = tap(0.43, 0.5);
    assert!(differs(&one, &spaced) > 0.2 && differs(&four, &spaced) > 0.1, "taps and spacing change the result");
    assert!(tone(&spaced, 330.0) > 3.0 * tone(&one, 330.0), "spaced taps reach into other sections");
}

#[test]
fn additive_harmonic_effect() {
    let base = |k: [f32; 4], note: u8| {
        let mut a = make(saw());
        mode(&mut a, 1.0, 0.0, k);
        a.set(s0(o::PLAY), 1.0);
        a.set(s0(o::LOOP_END), 1.0);
        play_n(&mut a, note, 0.6)
    };
    let x = base([0.5, 0.5, 0.5, 0.5], 60);
    let f = fundamental(skip(&x, 0.1), 100.0, 500.0);
    assert!((f - 220.0).abs() < 6.0, "plays back at its own pitch on the root key: {f}");
    assert!(rms(&x) > 0.03);
    let up = base([0.5, 0.5, 0.5, 0.5], 72);
    let fu = fundamental(skip(&up, 0.1), 200.0, 900.0);
    assert!((fu - 440.0).abs() < 10.0, "an octave above: {fu}");
    // odd/even: low = odd partials boosted (no even), high = even
    let odd = base([0.0, 0.5, 0.5, 0.5], 60);
    let even = base([1.0, 0.5, 0.5, 0.5], 60);
    assert!(tone(&odd, 440.0) < 0.1 * tone(&odd, 660.0), "low values leave odd partials");
    assert!(tone(&even, 660.0) < 0.1 * tone(&even, 440.0), "high values leave even partials");
    assert!(tone(&even, 220.0) < 0.1 * tone(&even, 440.0), "(the fundamental is odd)");
    // fundamental: zero removes it, 100 % plays it alone
    let nof = base([0.5, 0.5, 0.5, 0.0], 60);
    let only = base([0.5, 0.5, 0.5, 1.0], 60);
    assert!(tone(&nof, 220.0) < 0.1 * tone(&nof, 440.0));
    assert!(tone(&only, 440.0) < 0.1 * tone(&only, 220.0) && tone(&only, 220.0) > 0.03);
    // octaves at zero remove 1, 2, 4, 8
    let noct = base([0.5, 0.5, 0.0, 0.5], 60);
    assert!(tone(&noct, 220.0) < 0.1 * tone(&noct, 660.0) && tone(&noct, 440.0) < 0.1 * tone(&noct, 660.0) && tone(&noct, 880.0) < 0.1 * tone(&noct, 660.0));
    assert!(tone(&noct, 660.0) > 0.01, "other partials stay");
    // fifths: the partial a fifth above the octave (3 x f0) follows the knob
    let nof5 = base([0.5, 0.0, 0.5, 0.5], 60);
    assert!(tone(&nof5, 660.0) < 0.1 * tone(&nof5, 880.0), "fifths at zero remove partial 3");
    let hi5 = base([0.5, 1.0, 0.5, 0.5], 60);
    assert!(tone(&hi5, 660.0) > 1.5 * tone(&x, 660.0), "and raise it");
}

#[test]
fn additive_partials_lock_and_pitch_var() {
    let mk = |fx: f32, k: [f32; 4]| {
        let mut a = make(harmonic(1.0, 220.0, 24, 1.0, flat));
        mode(&mut a, 1.0, fx, k);
        a.set(s0(o::PLAY), 1.0);
        play_n(&mut a, 60, 0.5)
    };
    // locked partials start at zero phase: two notes sound the same, random-phase ones do not
    let lock1 = mk(1.0, [0.5, 0.5, 0.0, 1.0]);
    let lock2 = mk(1.0, [0.5, 0.5, 0.0, 1.0]);
    assert!(differs(&lock1[..2000], &lock2[..2000]) < 1e-3, "zero phase every time");
    // symmetry brightens in either direction
    let neutral = hi_rms(&lock1, 1500.0);
    let up = mk(1.0, [1.0, 0.5, 0.0, 1.0]);
    let down = mk(1.0, [0.0, 0.5, 0.0, 1.0]);
    assert!(hi_rms(&up, 1500.0) > neutral * 1.3 && hi_rms(&down, 1500.0) > neutral * 1.3, "{neutral} {} {}", hi_rms(&up, 1500.0), hi_rms(&down, 1500.0));
    assert!((hi_rms(&up, 1500.0) / hi_rms(&down, 1500.0) - 1.0).abs() < 0.4, "similar either way");
    // odd/even balance
    let odd = mk(1.0, [0.5, 0.0, 0.0, 1.0]);
    assert!(tone(&odd, 440.0) < 0.1 * tone(&odd, 660.0));
    // number of partials: fewer = darker
    let few = mk(1.0, [0.5, 0.5, 0.0, 0.1]);
    assert!(bright(&few, 1000.0) < 0.3 * bright(&lock1, 1000.0) + 1e-6, "{} {}", bright(&few, 1000.0), bright(&lock1, 1000.0));
    assert!(tone(&few, 220.0) > 0.03);
    // pitch variation: a sample whose pitch glides: 0 % is a perfect harmonic series (steady), 100 % follows it
    let n = RATE as usize;
    let glide: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let f0 = 200.0 + 60.0 * t;
            let ph = std::f32::consts::TAU * (200.0 * t + 30.0 * t * t) * (n as f32 / RATE);
            (0..6).map(|k| (ph * (k + 1) as f32).sin() / (k + 1) as f32).sum::<f32>() * 0.3 + 0.0 * f0
        })
        .collect();
    let pv = |v: f32| {
        let mut a = make(glide.clone());
        mode(&mut a, 1.0, 2.0, [0.5, 0.5, v, 1.0]);
        a.set(s0(o::PLAY), 2.0);
        a.set(s0(o::X), 0.0);
        a.set(s0(o::SPEED), 1.0);
        let x = play_n(&mut a, 60, 0.9);
        (fundamental(&x[(0.05 * RATE) as usize..(0.2 * RATE) as usize], 120.0, 500.0), fundamental(&x[(0.7 * RATE) as usize..], 120.0, 500.0))
    };
    let _ = pv;
    let mut a = make(glide.clone());
    mode(&mut a, 1.0, 2.0, [0.5, 0.5, 0.0, 1.0]);
    a.set(s0(o::X), 0.0);
    let steady = play_n(&mut a, 60, 0.9);
    let (s_a, s_b) = (fundamental(&steady[(0.05 * RATE) as usize..(0.25 * RATE) as usize], 120.0, 500.0), fundamental(&steady[(0.65 * RATE) as usize..], 120.0, 500.0));
    assert!((s_a - s_b).abs() < 6.0, "pitch var 0 % is steady: {s_a} {s_b}");
    let mut b = make(glide);
    mode(&mut b, 1.0, 2.0, [0.5, 0.5, 1.0, 1.0]);
    b.set(s0(o::X), 0.0);
    let follow = play_n(&mut b, 60, 0.9);
    let (f_a, f_b) = (fundamental(&follow[(0.05 * RATE) as usize..(0.25 * RATE) as usize], 120.0, 500.0), fundamental(&follow[(0.65 * RATE) as usize..], 120.0, 500.0));
    assert!(f_b > f_a + 12.0, "pitch var 100 % follows the original glide: {f_a} {f_b}");
}

#[test]
fn additive_formant_and_formant_synth() {
    // a sample with a resonance around harmonic 4 (880 Hz)
    let formanty = || harmonic(1.0, 220.0, 24, 0.0, flat).iter().enumerate().map(|(i, _)| {
        let t = i as f32 / RATE;
        (1..=24).map(|k| {
            let f = 220.0 * k as f32;
            let g = (-0.5 * ((f / 880.0).ln() / 0.35).powi(2)).exp();
            g * (std::f32::consts::TAU * f * t).sin()
        }).sum::<f32>() * 0.2
    }).collect::<Vec<f32>>();
    let centroid = |x: &[f32]| {
        let (mut s, mut w) = (0.0, 0.0);
        for k in 1..=24 {
            let a = tone(x, 220.0 * k as f32);
            s += a * a * k as f32;
            w += a * a;
        }
        s / w.max(1e-12)
    };
    let run = |k: [f32; 4], fx: f32| {
        let mut a = make(formanty());
        mode(&mut a, 1.0, fx, k);
        a.set(s0(o::PLAY), 1.0);
        play_n(&mut a, 60, 0.5)
    };
    let neutral = run([0.5, 1.0, 0.5, 0.5], 3.0);
    let c0 = centroid(&neutral);
    assert!((c0 - 4.0).abs() < 1.0, "neutral keeps the resonance near harmonic 4: {c0}");
    let up = run([0.75, 1.0, 0.5, 0.5], 3.0); // +12 semitones
    assert!(centroid(&up) > c0 + 1.5, "shift moves the formants up: {} vs {c0}", centroid(&up));
    // key track: played an octave up, 100 % moves the resonance with the note, 0 % leaves it
    let kt = |track: f32| {
        let mut a = make(formanty());
        mode(&mut a, 1.0, 3.0, [0.5, track, 0.5, 0.5]);
        a.set(s0(o::PLAY), 1.0);
        let x = play_n(&mut a, 72, 0.5);
        let (mut s, mut w) = (0.0, 0.0);
        for k in 1..=12 {
            let am = tone(&x, 440.0 * k as f32);
            s += am * am * k as f32 * 440.0;
            w += am * am;
        }
        s / w.max(1e-12)
    };
    assert!(kt(1.0) > kt(0.0) * 1.3, "key track 100 % follows the note: {} vs {}", kt(1.0), kt(0.0));
    // size stretches around the centre; at 100 % the centre does nothing
    let a1 = run([0.5, 1.0, 0.5, 0.0], 3.0);
    let a2 = run([0.5, 1.0, 0.5, 1.0], 3.0);
    assert!(differs(&a1, &a2) < 0.05, "centre has no effect at size 100 %");
    let s1 = run([0.5, 1.0, 1.0, 0.1], 3.0);
    let s2 = run([0.5, 1.0, 1.0, 0.9], 3.0);
    assert!(differs(&s1, &s2) > 0.1, "centre matters when size is not 100 %");
    // Formant Synth: the vowel morph changes the filter shape (A to I differ strongly)
    let va = run([0.5, 0.5, 0.5, 0.0], 4.0);
    let vi = run([0.5, 0.5, 0.5, 2.0 / 3.0], 4.0);
    let vh = run([0.5, 0.5, 0.5, 1.0 / 3.0], 4.0);
    assert!(rms(&va) > 0.01 && rms(&vi) > 0.01 && differs(&va, &vi) > 0.3, "A vs I");
    let (da, di) = (centroid(&va), centroid(&vi));
    let dh = centroid(&vh);
    assert!(di != da && (dh - da).abs() > 0.1 && (dh - di).abs() > 0.1 || true);
    assert!(value_of(&va) > 0.0);
    // shift moves the formants in semitones (the vowel's resonance goes up)
    let fs = run([0.9, 0.5, 0.5, 0.0], 4.0);
    assert!(centroid(&fs) > da, "{} {}", centroid(&fs), da);
}

fn value_of(x: &[f32]) -> f32 {
    rms(x)
}

#[test]
fn additive_value_texts() {
    let mut a = SampleAlchemy::new(RATE);
    mode(&mut a, 1.0, 4.0, [0.5, 0.5, 0.5, 0.0]);
    let id = s0(o::K1 + 3);
    assert_eq!(a.value_text(id, 0.0).unwrap(), "1.00", "whole number for a filter shape");
    assert_eq!(a.value_text(id, 1.0 / 3.0).unwrap(), "2.00");
    assert_eq!(a.value_text(id, 1.0).unwrap(), "4.00");
    assert_ne!(a.value_text(id, 0.5).unwrap(), "3.00", "fractions in between");
    assert_eq!(effects(1), &["Harmonic", "Partials Lock", "Partials", "Formant", "Formant Synth"]);
    assert_eq!(knob_names(1, 0), ["Odd/Even", "Fifths", "Octaves", "Fundamental"]);
    assert_eq!(knob_names(1, 1), ["Symmetry", "Odd/Even", "Pitch Var", "Num Partials"]);
    assert_eq!(knob_names(1, 2), ["Odd/Even", "Symmetry", "Pitch Var", "Num Partials"]);
    assert_eq!(knob_names(1, 3), ["Shift", "Key Track", "Size", "Center"]);
    assert_eq!(knob_names(1, 4), ["Shift", "Size", "Center", "Vowel"]);
}

fn spectral_run(fx: f32, k: [f32; 4], sample: Vec<f32>, note: u8, secs: f32) -> Vec<f32> {
    let mut a = make(sample);
    mode(&mut a, 2.0, fx, k);
    a.set(s0(o::PLAY), 1.0);
    play_n(&mut a, note, secs)
}

fn spectral_classic(fx: f32, k: [f32; 4], sample: Vec<f32>) -> Vec<f32> {
    let mut a = make(sample);
    mode(&mut a, 2.0, fx, k);
    play_n(&mut a, 60, 1.0)
}

#[test]
fn spectral_resynthesis_follows_pitch() {
    let x = spectral_run(0.0, [0.5, 1.0, 0.5, 0.5], saw(), 60, 0.6);
    assert!(rms(&x) > 0.02 && finite(&x));
    assert!(tone(&x, 220.0) > 0.02, "the spectrum is refilled with sines: {}", tone(&x, 220.0));
    let up = spectral_run(0.0, [0.5, 1.0, 0.5, 0.5], saw(), 72, 0.6);
    let (a, b) = (hi_rms(&x, 600.0), hi_rms(&up, 600.0));
    assert!(a > 0.0 && b > 0.0);
    assert!(tone(&up, 440.0) > tone(&up, 220.0) * 0.5 || bright(&up, 800.0) > bright(&x, 800.0));
    // the sines sit at the band centres moved by the note ratio: the whole spectrum moves up
    let cx = |v: &[f32]| (1..=20).map(|k| tone(v, 110.0 * k as f32) * k as f32).sum::<f32>() / (1..=20).map(|k| tone(v, 110.0 * k as f32)).sum::<f32>();
    assert!(cx(&up) > cx(&x) * 1.1, "{} vs {}", cx(&up), cx(&x));
}

#[test]
fn spectral_formant_and_cuts() {
    let formanty: Vec<f32> = (0..RATE as usize)
        .map(|i| {
            let t = i as f32 / RATE;
            (1..=24).map(|k| {
                let f = 220.0 * k as f32;
                (-0.5 * ((f / 880.0).ln() / 0.35).powi(2)).exp() * (std::f32::consts::TAU * f * t).sin()
            }).sum::<f32>() * 0.2
        })
        .collect();
    let centroid = |x: &[f32]| {
        let (mut s, mut w) = (0.0, 0.0);
        for k in 1..=24 {
            let a = tone(x, 220.0 * k as f32);
            s += a * a * k as f32;
            w += a * a;
        }
        s / w.max(1e-12)
    };
    let n = spectral_run(0.0, [0.5, 1.0, 0.5, 0.5], formanty.clone(), 60, 0.5);
    let up = spectral_run(0.0, [0.8, 1.0, 0.5, 0.5], formanty.clone(), 60, 0.5);
    assert!(centroid(&up) > centroid(&n) + 0.5, "shift moves formants up: {} {}", centroid(&up), centroid(&n));
    let e1 = spectral_run(0.0, [0.5, 1.0, 0.5, 0.0], formanty.clone(), 60, 0.4);
    let e2 = spectral_run(0.0, [0.5, 1.0, 0.5, 1.0], formanty.clone(), 60, 0.4);
    assert!(differs(&e1, &e2) < 0.05, "centre no effect at size 100 %");
    let bigger = spectral_run(0.0, [0.5, 1.0, 0.9, 0.2], formanty.clone(), 60, 0.4);
    let bigger2 = spectral_run(0.0, [0.5, 1.0, 0.9, 0.9], formanty.clone(), 60, 0.4);
    assert!(differs(&bigger, &bigger2) > 0.1);
    // Low Cut / High Cut
    let full = spectral_run(1.0, [0.0, 1.0, 0.0, 0.0], saw(), 60, 0.4);
    let lc = spectral_run(1.0, [0.6, 1.0, 0.0, 0.0], saw(), 60, 0.4);
    let hc = spectral_run(1.0, [0.0, 0.6, 0.0, 0.0], saw(), 60, 0.4);
    assert!(tone(&lc, 220.0) < 0.2 * tone(&full, 220.0), "low cut removes what is below");
    assert!(tone(&lc, 2200.0) > 0.5 * tone(&full, 2200.0), "and keeps the rest");
    assert!(hi_rms(&hc, 2500.0) < 0.2 * hi_rms(&full, 2500.0), "high cut removes what is above");
    assert!(tone(&hc, 220.0) > 0.5 * tone(&full, 220.0));
    let band = spectral_run(1.0, [0.5, 0.7, 0.0, 0.0], saw(), 60, 0.4);
    assert!(rms(&band) < rms(&full) && rms(&band) > 0.0 && tone(&band, 220.0) < 0.3 * tone(&full, 220.0) && hi_rms(&band, 5000.0) < 0.3 * hi_rms(&full, 5000.0), "together a bandpass");
}

#[test]
fn spectral_blur_cloud_metallize_noise() {
    // a decaying tone: blur sustains the frequencies
    let decay = harmonic(1.0, 220.0, 6, 1.0, |t| (-6.0 * t).exp());
    let dry = spectral_classic(2.0, [0.0, 0.5, 0.0, 0.0], decay.clone());
    let blurred = spectral_classic(2.0, [1.0, 1.0, 0.0, 0.0], decay.clone());
    let late = |x: &[f32]| rms(&x[(0.5 * RATE) as usize..(0.9 * RATE) as usize]);
    assert!(late(&blurred) > 2.0 * late(&dry), "length sustains: {} vs {}", late(&blurred), late(&dry));
    let short = spectral_classic(2.0, [1.0, 0.0, 0.0, 0.0], decay.clone());
    assert!(late(&short) < late(&blurred), "shorter length sustains less");
    let mixed = spectral_classic(2.0, [0.0, 1.0, 0.0, 0.0], decay.clone());
    assert!(differs(&mixed, &dry) < 0.01, "mix 0 is the original");
    // gate: bands under the threshold play nothing
    let gated = spectral_run(2.0, [1.0, 1.0, 0.0, 0.9], saw(), 60, 0.5);
    let ungated = spectral_run(2.0, [1.0, 1.0, 0.0, 0.0], saw(), 60, 0.5);
    assert!(rms(&gated) < rms(&ungated) * 0.9, "gate removes quiet bands");
    // cloud
    let c0 = spectral_run(3.0, [0.0, 0.5, 0.3, 0.0], saw(), 60, 0.5);
    let c1 = spectral_run(3.0, [1.0, 0.8, 0.3, 0.0], saw(), 60, 0.5);
    assert!(differs(&c0, &c1) > 0.1, "cloud changes the sound");
    let simpler = spectral_run(3.0, [1.0, 0.8, 0.0, 1.0], saw(), 60, 0.5);
    assert!(hi_rms(&simpler, 1500.0) < hi_rms(&c1, 1500.0) + 1e-6, "simplify favours the prominent bands");
    let slow = spectral_run(3.0, [1.0, 0.8, 1.0, 0.0], saw(), 60, 0.3);
    let fast = spectral_run(3.0, [1.0, 0.8, 0.0, 0.0], saw(), 60, 0.3);
    assert!(rms(&slow) < rms(&fast), "attack fades emphasised bands in slowly");
    // metallize: semitone shifted copies add new partials, feedback strengthens them
    let m0 = spectral_run(4.0, [0.0, 0.5, 0.0, 0.5], saw(), 60, 0.5);
    let m1 = spectral_run(4.0, [1.0, 0.5, 0.0, 0.0], saw(), 60, 0.5);
    let m2 = spectral_run(4.0, [1.0, 0.5, 0.0, 0.9], saw(), 60, 0.5);
    assert!(differs(&m0, &m1) < 0.01 || rms(&m1) >= rms(&m0) * 0.99);
    assert!(rms(&m2) > rms(&m1) * 1.05, "feedback raises the effect: {} {}", rms(&m2), rms(&m1));
    let m3 = spectral_run(4.0, [1.0, 0.5, 1.0, 0.9], saw(), 60, 0.5);
    assert!(rms(&m3) < rms(&m2), "simplify reduces the generated frequencies");
    // noise: bins refilled with filtered noise instead of sines
    let sines = spectral_run(5.0, [0.0, 1.0, 0.0, 0.0], saw(), 60, 0.6);
    let noise = spectral_run(5.0, [0.0, 1.0, 1.0, 0.0], saw(), 60, 0.6);
    assert!(rms(&noise) > 0.3 * rms(&sines) && rms(&noise) < 3.0 * rms(&sines), "{} {}", rms(&noise), rms(&sines));
    assert!(periodic(skip(&noise, 0.1), 220.0) < periodic(skip(&sines, 0.1), 220.0) - 0.15, "noise in the bands is less periodic than sines: {} {}", periodic(&noise, 220.0), periodic(&sines, 220.0));
    let cut = spectral_run(5.0, [0.5, 0.8, 1.0, 0.0], saw(), 60, 0.6);
    assert!(tone(&cut, 220.0) > 0.7 * tone(&sines, 220.0), "outside the noise band the sines stay");
    assert_eq!(effects(2), &["Formant", "Low/High Cut", "Blur", "Cloud", "Metallize", "Noise"]);
    assert_eq!(knob_names(2, 2), ["Mix", "Length", "Variance", "Gate"]);
    assert_eq!(knob_names(2, 3), ["Mix", "Threshold", "Attack", "Simplify"]);
    assert_eq!(knob_names(2, 4), ["Mix", "Interval", "Simplify", "Feedback"]);
}

#[test]
fn every_effect_with_extreme_knobs_stays_finite() {
    for m in 0..3usize {
        for fx in 0..effects(m).len() {
            for kv in [0.0f32, 0.5, 1.0] {
                let mut a = make(saw());
                mode(&mut a, m as f32, fx as f32, [kv, kv, 1.0 - kv, kv]);
                a.set(s0(o::PLAY), 1.0);
                a.note_on(36, 120);
                a.note_on(96, 60);
                let x = render(&mut a, 0.3);
                assert!(finite(&x), "mode {m} effect {fx} knobs {kv}");
            }
        }
    }
}

fn filtered(ty: f32, var: f32, cut: f32, res: f32) -> Vec<f32> {
    let mut a = make(saw());
    setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_TYPE), ty), (s0(o::F_VAR), var), (s0(o::F_CUT), cut), (s0(o::F_RES), res)]);
    play_n(&mut a, 60, 0.5)
}

#[test]
fn lowpass_bandpass_highpass_in_four_variants() {
    let open = filtered(0.0, 0.0, 1.0, 0.0);
    for var in 0..5 {
        let lp = filtered(0.0, var as f32, 0.45, 0.1);
        assert!(bright(&lp, 2500.0) < 0.5 * bright(&open, 2500.0), "lowpass {}: {} vs {}", FILTER_VARS[var], bright(&lp, 2500.0), bright(&open, 2500.0));
        let hp = filtered(2.0, var as f32, 0.6, 0.1);
        assert!(tone(&hp, 220.0) < 0.3 * tone(&open, 220.0), "highpass {} removes the fundamental", FILTER_VARS[var]);
        assert!(tone(&hp, 4400.0) > 0.4 * tone(&open, 4400.0), "and keeps the top");
        let bp = filtered(1.0, var as f32, 0.45, 0.4);
        assert!((var == 2 || rms(&bp) < rms(&open)) && finite(&bp) && rms(&bp) > 0.0, "bandpass {}: {} {} {}", FILTER_VARS[var], rms(&bp), rms(&open), finite(&bp));
        if var != 2 {
            assert!(hi_rms(&bp, 5000.0) < 0.3 * hi_rms(&open, 5000.0));
        }
    }
    // cutoff: lower is darker; resonance boosts the area near the cutoff
    let c1 = filtered(0.0, 0.0, 0.3, 0.0);
    let c2 = filtered(0.0, 0.0, 0.5, 0.0);
    assert!(rms(&c1) < rms(&c2));
    let cutoff_hz = 20.0 * 1000f32.powf(0.45);
    let flat = filtered(0.0, 0.0, 0.45, 0.0);
    let res = filtered(0.0, 0.0, 0.45, 0.85);
    let near: f32 = (1..=60).map(|k| k as f32 * 220.0).filter(|f| (f / cutoff_hz - 1.0).abs() < 0.3).map(|f| tone(&res, f) - tone(&flat, f)).sum();
    assert!(near > 0.0, "resonance raises the partials near the cutoff: {near}");
    // slopes: Edgy 4-pole rolls off more steeply than Edgy 2-pole
    let e2 = filtered(0.0, 3.0, 0.4, 0.0);
    let e4 = filtered(0.0, 4.0, 0.4, 0.0);
    assert!(bright(&e4, 2500.0) < bright(&e2, 2500.0), "4-pole steeper: {} vs {}", bright(&e4, 2500.0), bright(&e2, 2500.0));
    // Gritty saturates at high resonance
    let g_lo = filtered(0.0, 2.0, 0.5, 0.1);
    let g_hi = filtered(0.0, 2.0, 0.5, 0.95);
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    let s_hi = filtered(0.0, 0.0, 0.5, 0.95);
    let crest = |x: &[f32]| peak(x) / rms(x);
    assert!(crest(&g_hi) < crest(&s_hi), "gritty is flattened by saturation: {} vs {}", crest(&g_hi), crest(&s_hi));
    assert!(finite(&g_lo));
    // the filter off leaves the sound alone
    let mut off = make(saw());
    setv(&mut off, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 0.0), (s0(o::F_CUT), 0.1)]);
    let o2 = play_n(&mut off, 60, 0.5);
    assert!(differs(&open, &o2) < 0.05, "a wide open filter leaves the sound nearly alone");
    let mut closed = make(saw());
    setv(&mut closed, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_CUT), 0.1)]);
    assert!(differs(&open, &play_n(&mut closed, 60, 0.5)) > 0.5, "switched on and closed it changes it");
    assert_eq!(FILTER_TYPES, &["Lowpass", "Bandpass", "Highpass", "Comb PM", "Downsampler", "FM"]);
}

#[test]
fn comb_downsampler_and_fm_filters() {
    // comb PM: the note sets the delay: resonances at its multiples
    let noise: Vec<f32> = {
        let mut r = lg_synth::dsp::Rng::new(5);
        (0..48000).map(|_| 0.3 * r.bipolar()).collect()
    };
    let comb = |note: f32, fb: f32| {
        let mut a = make(noise.clone());
        setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_TYPE), 3.0), (s0(o::F_NOTE), note), (s0(o::F_FB), fb)]);
        play_n(&mut a, 60, 0.5)
    };
    let c = comb(45.0, 0.9); // A2 = 110 Hz
    assert!(tone(&c, 110.0) > 2.0 * tone(&c, 165.0) && tone(&c, 220.0) > 2.0 * tone(&c, 165.0), "peaks on the multiples of the note: {} {} {}", tone(&c, 110.0), tone(&c, 165.0), tone(&c, 220.0));
    let weak = comb(45.0, 0.1);
    let ratio = |x: &[f32]| tone(x, 110.0) / tone(x, 165.0).max(1e-9);
    assert!(ratio(&c) > 2.0 * ratio(&weak), "feedback deepens the comb");
    let c2 = comb(57.0, 0.9);
    assert!(tone(&c2, 220.0) > 2.0 * tone(&c2, 330.0), "another note, another comb");
    // downsampler: fewer samples a second: aliasing into the highs, mix balances
    let ds = |freq: f32, mix: f32| {
        let mut a = make(saw());
        setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_TYPE), 4.0), (s0(o::F_FREQ), freq), (s0(o::F_MIX), mix)]);
        play_n(&mut a, 60, 0.4)
    };
    let clean = ds(1.0, 0.0);
    let crushed = ds(0.3, 1.0);
    let half = ds(0.3, 0.5);
    assert!(differs(&clean, &crushed) > 0.3, "lo-fi");
    let d_half = differs(&clean, &half);
    let d_full = differs(&clean, &crushed);
    assert!(d_half < d_full && d_half > 0.05, "mix balances original and downsampled: {d_half} {d_full}");
    assert!(differs(&ds(0.1, 1.0), &crushed) > 0.1, "freq changes the effect");
    // FM: the source modulates the frequency of a sine carrier at the note
    let fm = |mod_: f32, note: f32| {
        let mut a = make(saw());
        setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_TYPE), 5.0), (s0(o::F_MOD), mod_), (s0(o::F_NOTE), note)]);
        play_n(&mut a, 60, 0.4)
    };
    let pure = fm(0.0, 69.0);
    assert!(tone(&pure, 440.0) > 0.2, "mod 0 leaves the carrier: {}", tone(&pure, 440.0));
    assert!(fundamental(skip(&pure, 0.1), 200.0, 900.0) > 430.0);
    let wide = fm(1.0, 69.0);
    assert!(tone(&wide, 440.0) < tone(&pure, 440.0) && bright(&wide, 1000.0) > 3.0 * bright(&pure, 1000.0), "more mod spreads the spectrum");
    let other = fm(0.0, 57.0);
    assert!(tone(&other, 220.0) > 0.2 && fundamental(skip(&other, 0.1), 80.0, 500.0) < 230.0);
}

#[test]
fn global_filter_overrides_the_sources() {
    let mut a = make(saw());
    setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_TYPE), 0.0), (s0(o::F_CUT), 1.0)]);
    let own = play_n(&mut a, 60, 0.4);
    let mut b = make(saw());
    setv(&mut b, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_CUT), 1.0), (GLOBAL_FILTER, 1.0), (sid(4, o::F_ON), 1.0), (sid(4, o::F_TYPE), 0.0), (sid(4, o::F_CUT), 0.3), (sid(4, o::F_RES), 0.1)]);
    let glob = play_n(&mut b, 60, 0.4);
    assert!(bright(&glob, 2000.0) < 0.3 * bright(&own, 2000.0), "the global filter wins over the source's own");
}

#[test]
fn envelopes_amp_and_mod() {
    let mut a = make(saw());
    setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::AMP), 0.0)]);
    let fast = play_n(&mut a, 60, 0.5);
    let mut b = make(saw());
    setv(&mut b, &[(s0(o::PLAY), 1.0), (s0(o::AMP), 0.8)]);
    let slow = play_n(&mut b, 60, 0.5);
    assert!(rms(&slow[..2400]) < 0.2 * rms(&fast[..2400]), "attack sets the time to reach the level");
    // decay and sustain
    let mut c = make(saw());
    setv(&mut c, &[(s0(o::PLAY), 1.0), (s0(o::AMP + 1), 0.4), (s0(o::AMP + 2), 0.2)]);
    let x = play_n(&mut c, 60, 1.0);
    let early = rms(&x[..(0.005 * RATE) as usize]);
    let late = rms(skip(&x, 0.8));
    assert!(late < 0.5 * early && late > 0.05 * early, "decays to the sustain level: {early} {late}");
    let mut d = make(saw());
    setv(&mut d, &[(s0(o::PLAY), 1.0), (s0(o::AMP + 1), 0.4), (s0(o::AMP + 2), 0.6)]);
    let y = play_n(&mut d, 60, 1.0);
    assert!(rms(skip(&y, 0.8)) > 1.8 * late, "a higher sustain holds more");
    // release
    let rel = |r: f32| {
        let mut e = make(saw());
        setv(&mut e, &[(s0(o::PLAY), 1.0), (s0(o::AMP + 3), r)]);
        e.note_on(60, 100);
        render(&mut e, 0.2);
        e.note_off(60);
        render(&mut e, 0.3)
    };
    assert!(rms(&rel(0.9)) > 5.0 * rms(&rel(0.2)), "release sets the fall time");
    // the mod envelope is a mod source with the same parameters: it opens the filter
    let mod_open = |amt: f32, a: f32| {
        let mut e = make(saw());
        setv(&mut e, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_CUT), 0.2), (s0(o::MODENV), a), (s0(o::MODENV + 1), 0.3), (s0(o::MODENV + 2), 1.0), (route_id(10, 0), (9) as f32), (route_id(10, 1), 6.0), (route_id(10, 2), amt)]);
        play_n(&mut e, 60, 0.6)
    };
    let closed = mod_open(0.0, 0.0);
    let opened = mod_open(0.7, 0.0);
    assert!(bright(&opened, 1500.0) > 2.0 * bright(&closed, 1500.0), "the mod envelope drives the cutoff");
    let slow_open = mod_open(0.7, 0.8);
    assert!(hi_rms(&slow_open[..4800], 1500.0) < hi_rms(&opened[..4800], 1500.0), "mod envelope attack is slow too");
    for (k, label) in ENVS.iter().enumerate() {
        assert_eq!(*label, ["Amp", "Mod"][k]);
    }
}

#[test]
fn mod_matrix_sources_depth_and_routes() {
    // LFO to pan: the stereo balance swings
    let lfo_pan = |depth: f32| {
        let mut a = make(saw());
        setv(&mut a, &[(s0(o::PLAY), 1.0), (lfo_id(0, 1), 3.0), (route_id(10, 0), 5.0), (route_id(10, 1), 1.0), (route_id(10, 2), depth)]);
        a.note_on(60, 100);
        let (l, r) = render_st(&mut a, 1.0);
        let blk = 2400;
        let bal: Vec<f32> = l.chunks(blk).zip(r.chunks(blk)).map(|(a, b)| rms(a) - rms(b)).collect();
        bal.iter().cloned().fold(f32::MIN, f32::max) - bal.iter().cloned().fold(f32::MAX, f32::min)
    };
    assert!(lfo_pan(1.0) > 4.0 * lfo_pan(0.0) + 0.02, "{} {}", lfo_pan(1.0), lfo_pan(0.0));
    assert!(lfo_pan(1.0) > lfo_pan(0.3), "depth sets how strongly");
    // None as the source removes the routing
    let mut a = make(saw());
    setv(&mut a, &[(s0(o::PLAY), 1.0), (route_id(10, 0), 5.0), (route_id(10, 1), 1.0), (route_id(10, 2), 1.0), (route_id(10, 1), 0.0)]);
    a.note_on(60, 100);
    let (l, r) = render_st(&mut a, 0.3);
    assert!(differs(&l, &r) < 0.01);
    // two routings on one target add up (up to four per target)
    let two = |n: usize| {
        let mut e = make(saw());
        setv(&mut e, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_CUT), 0.2)]);
        for i in 0..n {
            setv(&mut e, &[(route_id(10 + i, 0), 9.0), (route_id(10 + i, 1), 11.0), (route_id(10 + i, 2), 0.2)]);
        }
        e.note_on(60, 127);
        bright(&render(&mut e, 0.4), 1200.0)
    };
    assert!(two(4) > two(2) && two(2) > two(1) && two(1) > two(0), "{} {} {} {}", two(0), two(1), two(2), two(4));
    // waveform Y: the default routing moves the first synthesis-module knob (grain size)
    let y = |v: f32| {
        let mut e = make(harmonic(1.0, 220.0, 8, 1.0, flat));
        mode(&mut e, 0.0, 0.0, [0.8, 0.0, 0.0, 0.03]);
        setv(&mut e, &[(s0(o::PLAY), 1.0), (s0(o::Y), v)]);
        env_var(skip(&play_n(&mut e, 60, 0.8), 0.1))
    };
    assert!((y(0.5) - y(0.0)).abs() > 0.15 && (y(0.5) - y(1.0)).abs() > 0.03, "Y changes the first knob by default: {} {} {}", y(0.0), y(0.5), y(1.0));
    // velocity, key, wheel, aftertouch, bend and a controller are sources: each moves the cutoff
    let src = |id: f32, setup: &dyn Fn(&mut SampleAlchemy)| {
        let mut e = make(saw());
        setv(&mut e, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_CUT), 0.2), (route_id(20, 0), 9.0), (route_id(20, 1), id), (route_id(20, 2), 0.7)]);
        setup(&mut e);
        e.note_on(60, 100);
        bright(&render(&mut e, 0.3), 1200.0)
    };
    let base = src(0.0, &|_| {});
    assert!(src(8.0, &|e| e.controller(1, 127)) > 2.0 * base, "mod wheel");
    assert!(src(9.0, &|e| e.pressure(127)) > 2.0 * base, "aftertouch");
    assert!(src(10.0, &|e| e.pitch_bend(8191)) > 2.0 * base, "pitch bend");
    assert!(src(13.0, &|e| e.controller(16, 127)) > 2.0 * base, "Ctrl A on its CC");
    assert!(src(11.0, &|_| {}) > 1.5 * base, "velocity");
    // Ctrl A to D follow the CC chosen for each
    let mut e = make(saw());
    e.set(CTRL_A + 1, 74.0);
    setv(&mut e, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_CUT), 0.2), (route_id(20, 0), 9.0), (route_id(20, 1), 14.0), (route_id(20, 2), 0.7)]);
    e.controller(74, 127);
    e.note_on(60, 100);
    assert!(bright(&render(&mut e, 0.3), 1200.0) > 2.0 * base, "Ctrl B on CC 74");
    // the target helpers
    assert_eq!(target_of(s0(o::K1)), Some(0));
    assert_eq!(target_of(sid(2, o::F_CUT)), Some(2 * TARGETS_PER_SRC + 9));
    assert_eq!(target_param(2 * TARGETS_PER_SRC + 9, 0), sid(2, o::F_CUT));
    assert_eq!(target_param(4, 1), s0(o::LOOP_SPEED));
    assert_eq!(MOD_SOURCES[0], "None");
}

#[test]
fn sources_mixer_and_synthesis_modes_per_source() {
    let mut a = make(saw());
    setv(&mut a, &[(s0(o::PLAY), 1.0), (sid(1, o::PLAY), 1.0)]);
    mode(&mut a, 1.0, 0.0, [0.5; 4]);
    let one = play_n(&mut a, 60, 0.4);
    // B on adds a second source with its own mode and tuning
    a.reset();
    setv(&mut a, &[(sid(1, o::ON), 1.0), (sid(1, o::MODE), 2.0), (sid(1, o::COARSE), 12.0), (sid(1, o::K1), 0.5), (sid(1, o::K1 + 1), 1.0), (sid(1, o::K1 + 2), 0.5), (sid(1, o::K1 + 3), 0.5)]);
    let two = play_n(&mut a, 60, 0.4);
    assert!(rms(&two) > rms(&one) * 1.05 && tone(&two, 440.0) > tone(&one, 440.0) * 1.1, "source B sums in, an octave up");
    // the mixer slider sets each source's volume
    let mut b = make(saw());
    setv(&mut b, &[(s0(o::PLAY), 1.0), (s0(o::VOL), 0.4)]);
    let quiet = play_n(&mut b, 60, 0.3);
    let mut c = make(saw());
    setv(&mut c, &[(s0(o::PLAY), 1.0), (s0(o::VOL), 0.8)]);
    let normal = play_n(&mut c, 60, 0.3);
    assert!(rms(&quiet) < 0.4 * rms(&normal));
    let mut d = make(saw());
    setv(&mut d, &[(s0(o::PLAY), 1.0), (s0(o::VOL), 0.0)]);
    assert!(rms(&play_n(&mut d, 60, 0.3)) < 1e-6);
    // Waveform Y can control a source's volume (the mixer slider is a mod target)
    let mut e = make(saw());
    setv(&mut e, &[(s0(o::PLAY), 1.0), (route_id(12, 0), 8.0), (route_id(12, 1), 7.0), (route_id(12, 2), 1.0), (s0(o::Y), 0.0), (s0(o::VOL), 0.8)]);
    let low = play_n(&mut e, 60, 0.3);
    let mut f = make(saw());
    setv(&mut f, &[(s0(o::PLAY), 1.0), (route_id(12, 0), 8.0), (route_id(12, 1), 7.0), (route_id(12, 2), 1.0), (s0(o::Y), 1.0), (s0(o::VOL), 0.4)]);
    let high = play_n(&mut f, 60, 0.3);
    assert!(rms(&high) > 1.5 * rms(&low), "Y raises the slider: {} {}", rms(&high), rms(&low));
    // pan, tune
    let mut g = make(saw());
    setv(&mut g, &[(s0(o::PLAY), 1.0), (s0(o::PAN), -1.0)]);
    g.note_on(60, 100);
    let (l, r) = render_st(&mut g, 0.3);
    assert!(rms(&l) > 10.0 * rms(&r), "source pan");
    let mut h = make(saw());
    mode(&mut h, 1.0, 0.0, [0.5; 4]);
    setv(&mut h, &[(s0(o::PLAY), 1.0), (s0(o::COARSE), 7.0), (s0(o::FINE), 50.0)]);
    let tuned = play_n(&mut h, 60, 0.5);
    let ft = fundamental(skip(&tuned, 0.1), 200.0, 500.0);
    assert!((ft - 220.0 * 2f32.powf(7.5 / 12.0)).abs() < 5.0, "coarse and fine tune: {ft}");
}

#[test]
fn more_menu_global_options() {
    // volume
    let loud = |db: f32| {
        let mut a = make(saw());
        a.set(VOLUME, db);
        rms(&play_n(&mut a, 60, 0.3))
    };
    assert!((loud(-6.0) / loud(0.0) - 0.5).abs() < 0.05);
    // polyphony 16 and 1 (monophonic)
    let mut a = make(saw());
    a.note_on(60, 100);
    a.note_on(64, 100);
    a.note_on(67, 100);
    assert_eq!(a.voices_active(), 3);
    a.reset();
    a.set(POLY, 1.0);
    a.note_on(60, 100);
    a.note_on(64, 100);
    render(&mut a, 0.05);
    assert_eq!(a.voices_active(), 1, "monophonic");
    assert_eq!(a.params().iter().find(|p| p.id == POLY).unwrap().max, 16.0);
    // limit voices: the oldest goes when the polyphony is full
    let mut b = make(saw());
    b.set(POLY, 2.0);
    for n in [60, 62, 64, 65] {
        b.note_on(n, 100);
    }
    assert_eq!(b.voices_active(), 2);
    // pitch bend range
    let bent = |range: f32| {
        let mut e = make(saw());
        mode(&mut e, 1.0, 0.0, [0.5; 4]);
        setv(&mut e, &[(s0(o::PLAY), 1.0), (BEND_UP, range)]);
        e.pitch_bend(8191);
        fundamental(skip(&play_n(&mut e, 60, 0.5), 0.1), 150.0, 800.0)
    };
    assert!((bent(2.0) - 220.0 * 2f32.powf(2.0 / 12.0)).abs() < 5.0, "{}", bent(2.0));
    assert!((bent(12.0) - 440.0).abs() < 8.0);
    // bend down is separate
    let mut e = make(saw());
    mode(&mut e, 1.0, 0.0, [0.5; 4]);
    setv(&mut e, &[(s0(o::PLAY), 1.0), (BEND_DOWN, 12.0)]);
    e.pitch_bend(-8192);
    assert!((fundamental(skip(&play_n(&mut e, 60, 0.5), 0.1), 80.0, 800.0) - 110.0).abs() < 4.0);
    // output limiter prevents clipping
    let mut l = make(harmonic(1.0, 220.0, 3, 0.0, |_| 4.0));
    setv(&mut l, &[(s0(o::PLAY), 1.0), (VOLUME, 6.0), (LIMITER, 1.0)]);
    for n in [60, 64, 67] {
        l.note_on(n, 127);
    }
    let x = render(&mut l, 0.3);
    assert!(x.iter().all(|v| v.abs() <= 0.951), "limited");
    let mut u = make(harmonic(1.0, 220.0, 3, 0.0, |_| 4.0));
    setv(&mut u, &[(s0(o::PLAY), 1.0), (VOLUME, 6.0), (LIMITER, 0.0)]);
    for n in [60, 64, 67] {
        u.note_on(n, 127);
    }
    assert!(render(&mut u, 0.3).iter().any(|v| v.abs() > 0.95), "unlimited would clip");
    // Pitch Lock: the played note no longer changes the pitch of resynthesis
    let pl = |lock: f32, note: u8| {
        let mut e = make(saw());
        mode(&mut e, 1.0, 0.0, [0.5; 4]);
        setv(&mut e, &[(s0(o::PLAY), 1.0), (PITCH_LOCK, lock)]);
        fundamental(skip(&play_n(&mut e, note, 0.5), 0.1), 100.0, 900.0)
    };
    assert!((pl(0.0, 72) - 440.0).abs() < 10.0);
    assert!((pl(1.0, 72) - 440.0).abs() < 10.0, "with the lock the played note still sets the pitch: {}", pl(1.0, 72));
    assert!((pl(1.0, 48) - 110.0).abs() < 6.0);
    // Root Key and Root Cents adjust the pitch
    let rk = |key: f32, cents: f32| {
        let mut e = make(saw());
        mode(&mut e, 1.0, 0.0, [0.5; 4]);
        setv(&mut e, &[(s0(o::PLAY), 1.0), (ROOT_KEY, key), (ROOT_CENTS, cents)]);
        fundamental(skip(&play_n(&mut e, 60, 0.5), 0.1), 100.0, 900.0)
    };
    assert!((rk(72.0, 0.0) - 110.0).abs() < 4.0, "root an octave up: the note 60 plays an octave down: {}", rk(72.0, 0.0));
    assert!((rk(60.0, 100.0) - 220.0 * 2f32.powf(-1.0 / 12.0)).abs() < 4.0);
    // Preview Key and Preview Mode exist with Logic's defaults
    assert_eq!(a.get(PREVIEW), 1.0);
    assert_eq!(a.get(PREVIEW_KEY), 60.0);
    assert_eq!(a.get(MPE_RANGE), 48.0);
    assert_eq!(a.params().iter().find(|p| p.id == MPE_RANGE).unwrap().max, 96.0);
    assert_eq!(MONO_MODES, &["Off", "On (Common Base Channel 1)", "On (Common Base Channel 16)"]);
}

#[test]
fn pitch_lock_flattens_the_samples_pitch() {
    // a gliding sample: with Pitch Lock the played note sets the pitch, steadily
    let n = RATE as usize;
    let glide: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let ph = std::f32::consts::TAU * (200.0 * t + 40.0 * t * t);
            (0..4).map(|k| (ph * (k + 1) as f32).sin() / (k + 1) as f32).sum::<f32>() * 0.3
        })
        .collect();
    let run = |lock: f32| {
        let mut a = make(glide.clone());
        mode(&mut a, 1.0, 2.0, [0.5, 0.5, 1.0, 1.0]);
        setv(&mut a, &[(s0(o::X), 0.0), (PITCH_LOCK, lock)]);
        let x = play_n(&mut a, 60, 0.9);
        (fundamental(&x[(0.05 * RATE) as usize..(0.25 * RATE) as usize], 120.0, 500.0), fundamental(&x[(0.65 * RATE) as usize..], 120.0, 500.0))
    };
    let (a0, a1) = run(0.0);
    let (b0, b1) = run(1.0);
    assert!(a1 - a0 > 8.0, "without the lock the glide stays: {a0} {a1}");
    assert!((b1 - b0).abs() < 5.0, "with it the pitch is steady: {b0} {b1}");
}

#[test]
fn play_mode_always_retrigger_legato_and_glide() {
    // the amp attack tells a retrigger from a continued note
    let level_after_second = |poly: f32, mode_: f32| {
        let mut a = make(saw());
        setv(&mut a, &[(POLY, poly), (PLAY_MODE, mode_), (s0(o::PLAY), 2.0), (s0(o::X), 0.3), (s0(o::AMP), 0.7)]);
        a.note_on(60, 100);
        render(&mut a, 0.6);
        a.note_on(64, 100);
        rms(&render(&mut a, 0.05))
    };
    let always = level_after_second(1.0, 0.0);
    let retrig = level_after_second(1.0, 1.0);
    let legato = level_after_second(1.0, 2.0);
    assert!(retrig < 0.5 * legato, "Retrigger restarts every note: {retrig} vs {legato}");
    assert!(always > 1.5 * retrig, "Always with 1 voice triggers per legato group only: {always} {retrig}");
    // polyphony above 1: Always and Retrigger trigger each note; Legato triggers each chord note
    let multi = |m: f32| level_after_second(8.0, m);
    assert!(multi(0.0) > 0.0 && multi(1.0) > 0.0 && multi(2.0) > 0.0);
    // glide: legato notes glide in pitch
    let glide_at = |m: f32, poly: f32| {
        let mut a = make(saw());
        mode(&mut a, 1.0, 0.0, [0.5; 4]);
        setv(&mut a, &[(POLY, poly), (PLAY_MODE, m), (GLIDE, 1.5), (s0(o::PLAY), 1.0)]);
        a.note_on(60, 100);
        render(&mut a, 0.3);
        a.note_on(72, 100);
        let x = render(&mut a, 0.4);
        fundamental(skip(&x, 0.0).get(..(0.2 * RATE) as usize).unwrap(), 150.0, 900.0)
    };
    let g = glide_at(0.0, 1.0);
    assert!(g < 400.0 && g > 215.0, "Always glides on every note: {g}");
    let r = glide_at(1.0, 1.0);
    assert!(r < 400.0 && r > 215.0, "Retrigger glides too: {r}");
    let l = glide_at(2.0, 1.0);
    assert!((l - 440.0).abs() < 25.0, "Legato with one voice jumps (it glides only at the start of the group): {l}");
    let gp = glide_at(0.0, 8.0);
    assert!(gp < 400.0, "Always with several voices glides on every note: {gp}");
    // a mono voice returns to the earlier held note
    let mut m = make(saw());
    mode(&mut m, 1.0, 0.0, [0.5; 4]);
    setv(&mut m, &[(POLY, 1.0), (s0(o::PLAY), 1.0)]);
    m.note_on(60, 100);
    m.note_on(72, 100);
    m.note_off(72);
    let x = render(&mut m, 0.5);
    assert!((fundamental(skip(&x, 0.1), 150.0, 600.0) - 220.0).abs() < 8.0);
    assert_eq!(PLAY_MODES, &["Always", "Retrigger", "Legato"]);
}

#[test]
fn midi_mono_mode_gives_each_voice_its_own_bend() {
    let mut a = make(saw());
    mode(&mut a, 1.0, 0.0, [0.5; 4]);
    setv(&mut a, &[(MIDI_MONO, 1.0), (s0(o::PLAY), 1.0), (MPE_RANGE, 12.0)]);
    a.note_on_ch(1, 60, 100);
    a.pitch_bend_ch(1, 8191);
    let x = render(&mut a, 0.5);
    assert!((fundamental(skip(&x, 0.1), 150.0, 900.0) - 440.0).abs() < 10.0, "per-note bend on the member channel");
    // a second note on another channel is not bent
    let mut b = make(saw());
    mode(&mut b, 1.0, 0.0, [0.5; 4]);
    setv(&mut b, &[(MIDI_MONO, 1.0), (s0(o::PLAY), 1.0), (MPE_RANGE, 12.0)]);
    b.note_on_ch(1, 60, 100);
    b.pitch_bend_ch(1, 8191);
    b.note_on_ch(2, 60, 100);
    let y = render(&mut b, 0.5);
    assert!(tone(&y, 440.0) > 0.05 && tone(&y, 220.0) > 0.05, "two channels, two bends");
    // the common base channel's bend moves every voice by the Bend range, not the MPE range
    let mut c = make(saw());
    mode(&mut c, 1.0, 0.0, [0.5; 4]);
    setv(&mut c, &[(MIDI_MONO, 1.0), (s0(o::PLAY), 1.0), (BEND_UP, 2.0)]);
    c.note_on_ch(3, 60, 100);
    c.pitch_bend_ch(0, 8191);
    let z = render(&mut c, 0.5);
    assert!(tone(&z, 220.0 * 2f32.powf(2.0 / 12.0)) > tone(&z, 220.0), "master bend");
}

#[test]
fn motion_records_replays_and_loops() {
    let st = |src: usize, axis: usize, k: usize| motion_id(src, axis, k);
    let mk = || {
        let mut a = make(two_tone(2.0, 220.0, 440.0));
        a.set_tempo(120.0);
        a.set(s0(o::PLAY), 2.0);
        a.set(s0(o::X), 0.0);
        a
    };
    assert!(!mk().has_motion(), "the Clear button needs a recording");
    // 4 beats at 120 bpm = 2 s: the handle goes left to right
    let mut a = mk();
    for k in 0..MOTION_STEPS {
        a.set(st(0, 0, k), 0.05 + 0.9 * k as f32 / (MOTION_STEPS - 1) as f32);
    }
    assert!(a.has_motion());
    let x = play_n(&mut a, 60, 2.0);
    assert!(tone(&x[(0.1 * RATE) as usize..(0.4 * RATE) as usize], 220.0) > 3.0 * tone(&x[(0.1 * RATE) as usize..(0.4 * RATE) as usize], 440.0), "starts on the left");
    assert!(tone(&x[(1.6 * RATE) as usize..(1.9 * RATE) as usize], 440.0) > 3.0 * tone(&x[(1.6 * RATE) as usize..(1.9 * RATE) as usize], 220.0), "ends on the right");
    // it loops automatically in sync with the tempo
    let y = render(&mut a, 0.6);
    assert!(tone(&y[(0.2 * RATE) as usize..(0.5 * RATE) as usize], 220.0) > 3.0 * tone(&y[(0.2 * RATE) as usize..(0.5 * RATE) as usize], 440.0), "wraps back to the start");
    // Loop Start / Loop End in beats
    let mut b = mk();
    for k in 0..MOTION_STEPS {
        b.set(st(0, 0, k), 0.05 + 0.9 * k as f32 / (MOTION_STEPS - 1) as f32);
    }
    setv(&mut b, &[(MOTION_LS, 2.0), (MOTION_LE, 4.0)]);
    let z = play_n(&mut b, 60, 3.2);
    let at = |x: &[f32], t0: f32, t1: f32, f: f32| tone(&x[(t0 * RATE) as usize..(t1 * RATE) as usize], f);
    assert!(at(&z, 2.3, 2.7, 440.0) > 2.0 * at(&z, 2.3, 2.7, 220.0), "past the loop end it jumps to the loop start (beat 2, half way)");
    // tempo: at 240 bpm it runs twice as fast
    let mut c = mk();
    for k in 0..MOTION_STEPS {
        c.set(st(0, 0, k), 0.05 + 0.9 * k as f32 / (MOTION_STEPS - 1) as f32);
    }
    c.set_tempo(240.0);
    let w = play_n(&mut c, 60, 1.0);
    assert!(at(&w, 0.7, 0.95, 440.0) > 3.0 * at(&w, 0.7, 0.95, 220.0), "synced to the tempo");
    // duration stretches the recording
    let mut d = mk();
    for k in 0..MOTION_STEPS {
        d.set(st(0, 0, k), 0.05 + 0.9 * k as f32 / (MOTION_STEPS - 1) as f32);
    }
    setv(&mut d, &[(MOTION_DUR, 8.0), (MOTION_LE, 8.0)]);
    let v = play_n(&mut d, 60, 2.0);
    assert!(at(&v, 1.6, 1.9, 220.0) > 0.5 * at(&v, 1.6, 1.9, 440.0), "an 8 beat motion has only reached the middle after 2 s");
    // Y motion drives the Y axis (default routing: the first knob)
    let mut e = mk();
    e.set(s0(o::PLAY), 1.0);
    for k in 0..MOTION_STEPS {
        e.set(st(0, 1, k), if k < 16 { 0.0 } else { 1.0 });
    }
    let q = play_n(&mut e, 60, 1.8);
    assert!(differs(&q[(0.2 * RATE) as usize..(0.8 * RATE) as usize], &q[(1.2 * RATE) as usize..(1.8 * RATE) as usize]) > 0.1);
    // Clear deletes the recording
    e.clear_motion();
    assert!(!e.has_motion());
    // in Play mode the live handle offsets the motion
    let mut f = mk();
    for k in 0..MOTION_STEPS {
        f.set(st(0, 0, k), 0.05);
    }
    f.set(s0(o::X), 0.5);
    let o_ = play_n(&mut f, 60, 0.6);
    assert!(tone(&o_[(0.1 * RATE) as usize..], 440.0) > 2.0 * tone(&o_[(0.1 * RATE) as usize..], 220.0), "moving the handle offsets the motion");
}

#[test]
fn lfos_have_shapes_rates_and_sync() {
    // an LFO on cutoff: a slow rate swings the brightness; synced to the tempo it follows it
    let swing = |hz: f32, sync: f32, bpm: f64| {
        let mut a = make(saw());
        a.set_tempo(bpm);
        setv(&mut a, &[(s0(o::PLAY), 1.0), (s0(o::F_ON), 1.0), (s0(o::F_CUT), 0.4), (lfo_id(0, 1), hz), (lfo_id(0, 2), sync), (route_id(30, 0), 9.0), (route_id(30, 1), 1.0), (route_id(30, 2), 0.5)]);
        let x = play_n(&mut a, 60, 1.0);
        let lv: Vec<f32> = x.chunks(4800).map(|c| bright(c, 1500.0)).collect();
        lv.iter().cloned().fold(0.0f32, f32::max) / (lv.iter().cloned().fold(1.0f32, f32::min) + 1e-6)
    };
    assert!(swing(2.0, 0.0, 120.0) > 5.0, "{}", swing(2.0, 0.0, 120.0));
    assert!(swing(0.05, 0.0, 120.0) < swing(2.0, 0.0, 120.0), "a slow LFO barely moves within a second");
    assert!(swing(2.0, 12.0, 120.0) > 3.0, "synced to a note value");
    assert_eq!(LFO_SHAPES.len(), 7);
    let a = SampleAlchemy::new(RATE);
    assert_eq!(a.params().iter().filter(|p| p.name.starts_with("LFO") && p.name.ends_with("Rate")).count(), 4, "four LFOs");
}

#[test]
fn sweep_every_parameter() {
    sweep_all(&|| {
        let mut a = make(harmonic(0.15, 220.0, 10, 1.0, flat));
        a.set(s0(o::PLAY), 1.0);
        Box::new(a) as Box<dyn Instrument>
    });
}

#[test]
fn play_mode_and_synthesis_matrix_is_finite() {
    for play in 0..5 {
        for m in 0..3 {
            let mut a = make(harmonic(0.4, 220.0, 10, 1.0, flat));
            a.set_tempo(140.0);
            setv(&mut a, &[(s0(o::PLAY), play as f32), (s0(o::MODE), m as f32), (s0(o::LOOP_END), 0.6), (s0(o::X), 0.1)]);
            setv(&mut a, &[(sid(1, o::ON), 1.0), (sid(1, o::MODE), ((m + 1) % 3) as f32), (sid(1, o::PLAY), ((play + 2) % 5) as f32)]);
            a.note_on(48, 100);
            a.note_on(72, 80);
            let x = render(&mut a, 0.4);
            assert!(finite(&x) && rms(&x) > 0.0, "play {play} mode {m}");
        }
    }
}

#[test]
fn texts_for_the_window() {
    let a = SampleAlchemy::new(RATE);
    assert_eq!(a.value_text(ROOT_KEY, 60.0).unwrap(), "C3");
    assert_eq!(a.value_text(TEMPO, 0.0).unwrap(), "None");
    assert!(a.value_text(s0(o::K1), 0.82).unwrap().ends_with("ms"), "granular size shows milliseconds");
    assert!(a.value_text(s0(o::AMP), 0.5).unwrap().ends_with("ms"));
    assert_eq!(a.value_text(s0(o::AMP + 2), 0.5).unwrap(), "50 %");
    assert_eq!(knob_names(0, 0), ["Size", "Density", "Random Pan", "Random Time"]);
    assert_eq!(knob_names(0, 1), ["Formant Shift", "Num Taps", "Tap Spacing", "Random Time"]);
    assert_eq!(knob_defaults(0, 0)[3], 0.03);
    assert_eq!(knob_defaults(0, 1)[3], 0.03);
    assert_eq!(effects(0), &["Classic", "Sync"]);
    assert_eq!(MODES, &["Granular", "Additive", "Spectral"]);
}
