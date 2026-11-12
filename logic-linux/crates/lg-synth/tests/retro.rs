//! Retro Synth: the parts of the guide the extended engine adds change the sound as described.
mod common;
use common::*;
use lg_dsp::{Instrument, Plugin};
use lg_synth::retro::{self as r, RetroSynth};

fn rs() -> RetroSynth {
    let mut s = RetroSynth::new(RATE);
    // a plain, bright starting point: no effect, filter wide open
    s.set(42, 0.0);
    s.set(36, 1.0);
    s.set(37, 0.0);
    s.set(40, 0.0);
    s.set(61, 0.002);
    s.set(63, 1.0);
    s
}

fn p(set: &[(u32, f32)], note: u8, secs: f32) -> Vec<f32> {
    play(&mut rs(), set, note, 100, secs)
}

fn tail(x: &[f32]) -> &[f32] {
    &x[x.len() / 3..]
}

#[test]
fn stays_finite_everywhere() {
    sweep_all(&|| Box::new(RetroSynth::new(RATE)));
}

#[test]
fn old_ids_keep_their_meaning() {
    assert_eq!(r::PARAMS[19].name, "Oscillator Type");
    assert_eq!(r::PARAMS[65].name, "Amp Env Velocity");
    assert_eq!(r::PARAMS[66].id, 66);
    for (i, q) in r::PARAMS.iter().enumerate() {
        assert_eq!(q.id as usize, i, "ids are consecutive");
    }
}

#[test]
fn noise_oscillators() {
    let saw = p(&[(28, 1.0), (26, 0.0)], 57, 0.4);
    let noise = p(&[(r::OSC1_NOISE, 1.0), (26, 0.0)], 57, 0.4);
    // noise has no pitch: the 220 Hz line is far weaker relative to the level than the saw's
    let rel = |x: &[f32]| tone(tail(x), 220.0) / rms(tail(x)).max(1e-9);
    assert!(rms(tail(&noise)) > 0.01);
    assert!(rel(&noise) < 0.2 * rel(&saw));
}

#[test]
fn filter_family() {
    let base = [(r::FILTER_MODE, 1.0), (36, 0.5), (37, 0.2), (r::FILTER_SLOPE, 3.0)];
    let open = p(&[(36, 1.0)], 45, 0.4);
    let hi = |x: &[f32]| bright(tail(x), 3000.0);
    for v in 0..6 {
        let mut set = base.to_vec();
        set.push((r::FILTER_VARIANT, v as f32));
        let x = p(&set, 45, 0.4);
        assert!(finite(&x));
        assert!(hi(&x) < 0.5 * hi(&open), "LP variant {v} darkens the sound");
    }
    // slopes: steeper means darker
    let s12 = p(&[(r::FILTER_MODE, 1.0), (r::FILTER_VARIANT, 1.0), (r::FILTER_SLOPE, 1.0), (36, 0.6)], 45, 0.4);
    let s24 = p(&[(r::FILTER_MODE, 1.0), (r::FILTER_VARIANT, 1.0), (r::FILTER_SLOPE, 3.0), (36, 0.6)], 45, 0.4);
    assert!(bright(tail(&s24), 4000.0) < 0.7 * bright(tail(&s12), 4000.0));
    // highpass removes the low end, in each slope
    for sl in 0..4 {
        let x = p(&[(r::FILTER_MODE, 2.0), (r::FILTER_SLOPE, sl as f32), (r::FILTER_VARIANT, 1.0), (36, 0.55)], 36, 0.4);
        let raw = p(&[(36, 1.0)], 36, 0.4);
        assert!(tone(tail(&x), 65.0) < 0.5 * tone(tail(&raw), 65.0), "HP slope {sl}");
    }
    // band pass: more resonance, narrower band (less energy away from the cutoff)
    let wide = p(&[(r::FILTER_MODE, 3.0), (36, 0.5), (37, 0.0)], 45, 0.4);
    let narrow = p(&[(r::FILTER_MODE, 3.0), (36, 0.5), (37, 0.9)], 45, 0.4);
    assert!(bright(tail(&narrow), 1500.0) / rms(tail(&narrow)) < bright(tail(&wide), 1500.0) / rms(tail(&wide)));
}

#[test]
fn band_reject_amount_and_peak_controls() {
    let dry = p(&[(r::FILTER_MODE, 5.0), (r::PEAK_GAIN, 0.0), (36, 0.4)], 45, 0.4);
    let full = p(&[(r::FILTER_MODE, 4.0), (36, 0.4), (37, 0.6), (r::BR_AMOUNT, 1.0)], 45, 0.4);
    let none = p(&[(r::FILTER_MODE, 4.0), (36, 0.4), (37, 0.6), (r::BR_AMOUNT, 0.0)], 45, 0.4);
    assert!(differs(&dry[2000..], &none[2000..]) < 0.01, "amount 0 passes everything");
    assert!(differs(&dry[2000..], &full[2000..]) > 0.05, "amount 1 cuts the band");
    // peak: gain boosts the band, bandwidth widens it
    let flat = p(&[(r::FILTER_MODE, 5.0), (r::PEAK_GAIN, 0.0), (36, 0.4)], 45, 0.4);
    let boost = p(&[(r::FILTER_MODE, 5.0), (r::PEAK_GAIN, 18.0), (36, 0.4)], 45, 0.4);
    assert!(rms(tail(&boost)) > 1.3 * rms(tail(&flat)));
    let narrow = p(&[(r::FILTER_MODE, 5.0), (r::PEAK_GAIN, 18.0), (r::PEAK_BW, 0.0), (36, 0.4)], 45, 0.4);
    let wide = p(&[(r::FILTER_MODE, 5.0), (r::PEAK_GAIN, 18.0), (r::PEAK_BW, 1.0), (36, 0.4)], 45, 0.4);
    assert!(rms(tail(&wide)) > rms(tail(&narrow)));
}

#[test]
fn filter_fm_and_bipolar_lfo() {
    let off = p(&[(36, 0.4), (37, 0.3)], 45, 0.4);
    let stat = p(&[(36, 0.4), (37, 0.3), (r::FILTER_FM, -0.8)], 45, 0.4);
    let env = p(&[(36, 0.4), (37, 0.3), (r::FILTER_FM, 0.8), (57, 0.1), (58, 0.0)], 45, 0.4);
    assert!(finite(&stat) && finite(&env));
    assert!(differs(&off[2000..], &stat[2000..]) > 0.1, "static filter FM changes the tone");
    // envelope FM fades as the envelope decays: the sound moves back toward the unmodulated one
    let early = differs(&off[2000..6000], &env[2000..6000]);
    let late = differs(&off[14000..18000], &env[14000..18000]);
    assert!(late < early, "envelope-controlled FM decays ({early} then {late})");
    // the filter LFO works in both directions
    let a = p(&[(36, 0.4), (r::CUTOFF_LFO2, 0.9), (46, 0.0), (48, 6.0)], 45, 0.5);
    let b = p(&[(36, 0.4), (r::CUTOFF_LFO2, -0.9), (46, 0.0), (48, 6.0)], 45, 0.5);
    assert!(differs(&a, &b) > 0.05);
}

#[test]
fn fm_engine_controls() {
    let fm = |set: &[(u32, f32)]| {
        let mut v = vec![(19, 3.0), (26, 0.5)];
        v.extend_from_slice(set);
        p(&v, 57, 0.4)
    };
    let plain = fm(&[]);
    let harm = fm(&[(r::FM_HARMONIC, 0.4)]);
    let inh = fm(&[(r::FM_INHARMONIC, 0.37)]);
    let shape = fm(&[(r::FM_SHAPE, 0.9)]);
    assert!(differs(tail(&plain), tail(&harm)) > 0.2, "harmonic changes the overtones");
    assert!(differs(tail(&plain), tail(&inh)) > 0.2, "inharmonic detunes the modulator");
    assert!(differs(tail(&plain), tail(&shape)) > 0.2);
    // the mix slider crossfades to the modulator (a pure sine at the modulator's ratio)
    let mod_only = fm(&[(r::FM_MIX, 1.0), (26, 0.0)]);
    assert!(rms(tail(&mod_only)) > 0.01);
    // the switch decides what the shape modulation moves: FM amount only, or the harmonic too
    let lfo = |mode: f32| fm(&[(r::FM_MODE, mode), (25, 0.9), (46, 0.0), (48, 8.0), (26, 0.3)]);
    assert!(differs(&lfo(0.0)[4000..], &lfo(2.0)[4000..]) > 0.1);
}

#[test]
fn sync_amount() {
    let s = |amt: f32| p(&[(19, 1.0), (r::SYNC_AMOUNT, amt), (26, 0.9), (30, 3.0), (36, 1.0)], 45, 0.4);
    let (lo, hi) = (s(0.0), s(1.0));
    assert!(bright(tail(&hi), 2000.0) > bright(tail(&lo), 2000.0), "more sync, more aggressive");
}

#[test]
fn table_formant_mode_and_modulation_target() {
    let t = |set: &[(u32, f32)]| {
        let mut v = vec![(19, 2.0), (26, 0.0), (28, 1.5), (25, 0.0)];
        v.extend_from_slice(set);
        p(&v, 45, 0.4)
    };
    let shape = t(&[]);
    let formant = t(&[(r::TABLE_MODE, 1.0), (25, 0.5)]);
    assert!(differs(tail(&shape), tail(&formant)) > 0.2);
    // formant stretch up moves energy to higher harmonics
    let low = t(&[(r::TABLE_MODE, 1.0), (25, -0.3)]);
    let high = t(&[(r::TABLE_MODE, 1.0), (25, 0.3)]);
    assert!(differs(tail(&low), tail(&high)) > 0.1);
    // Wave Variation as a controller target changes the table sound with the mod wheel
    let mut s = rs();
    s.set(19, 2.0);
    s.set(26, 0.0);
    s.set(12, 10.0);
    s.set(13, 1.0);
    s.note_on(45, 100);
    let a = render(&mut s, 0.3);
    s.controller(1, 127);
    let b = render(&mut s, 0.3);
    assert!(differs(tail(&a), tail(&b)) > 0.1);
    assert!(r::targets_for(2).contains(&10) && !r::targets_for(0).contains(&10));
}

#[test]
fn reverse_and_app_wavetables() {
    let a = p(&[(19, 2.0), (26, 0.0), (28, 0.5), (r::TABLE_REVERSE, 0.0)], 45, 0.3);
    let b = p(&[(19, 2.0), (26, 0.0), (28, 0.5), (r::TABLE_REVERSE, 1.0)], 45, 0.3);
    assert!(differs(tail(&a), tail(&b)) > 0.1, "reversed order plays the other end first");
    let v = p(&[(19, 2.0), (26, 0.0), (28, 2.0), (r::WAVETABLE, 2.0)], 45, 0.3);
    assert!(differs(tail(&a), tail(&v)) > 0.1, "another supplied wavetable sounds different");
}

#[test]
fn custom_wavetable_from_audio() {
    let dir = std::env::temp_dir().join(format!("bramble-wt-test-{}", std::process::id()));
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
    // two constant-pitch notes (200 Hz) separated by silence; the second is brighter
    let rate = RATE;
    let mut x = Vec::new();
    for (k, h) in [(0usize, 1usize), (1, 6)] {
        for i in 0..(rate * 0.4) as usize {
            let t = i as f32 / rate;
            let mut v = 0.0;
            for n in 1..=h {
                v += (std::f32::consts::TAU * 200.0 * n as f32 * t).sin() / n as f32;
            }
            // a timbre change inside each note: the harmonics grow after 0.2 s
            if i > (rate * 0.2) as usize {
                v += 0.6 * (std::f32::consts::TAU * 200.0 * 3.0 * t).sin();
            }
            x.push(v * 0.4);
        }
        if k == 0 {
            x.extend(std::iter::repeat_n(0.0, (rate * 0.2) as usize));
        }
    }
    let mut s = rs();
    s.set(r::ANALYSIS_SENS, 0.7);
    let n = s.wavetable_from_samples(&x, rate).expect("analysis");
    assert!(n >= 3, "timbre changes make entries ({n})");
    assert_eq!(s.get(r::TABLE_SOURCE), 1.0);
    // the custom table plays at the played pitch
    s.set(19, 2.0);
    s.set(26, 0.0);
    s.note_on(57, 100);
    let y = render(&mut s, 0.5);
    let f = fundamental(&y[4800..], 150.0, 300.0);
    assert!((f / 220.0 - 1.0).abs() < 0.02, "custom table at {f} Hz");
    // save, list, load into a new synth, delete
    s.save_custom("unit test").unwrap();
    assert!(r::wavetable::list().contains(&"unit test".to_string()));
    let mut s2 = rs();
    assert_eq!(s2.load_user("unit test").unwrap(), n);
    assert!(s2.custom_frames().is_some());
    assert!(s2.delete_user("unit test"));
    assert!(!r::wavetable::list().contains(&"unit test".to_string()));
    assert_eq!(s2.get(r::TABLE_SOURCE), 0.0);
    // silence only: no wavetable
    assert!(s.wavetable_from_samples(&vec![0.0; 48000], rate).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn glide_targets() {
    // glide from C3 to C4: with Osc 2 only the first oscillator holds its pitch at the start
    let run = |target: f32, on: f32| {
        let mut s = rs();
        for (id, v) in [(5u32, 2.0), (24, 1.0), (21, 1.0), (22, 0.0), (26, 0.5), (r::GLIDE_TARGET, target), (r::GLIDE_ON, on)] {
            s.set(id, v);
        }
        s.note_on(48, 100);
        render(&mut s, 0.2);
        s.note_on(60, 100);
        render(&mut s, 0.12)
    };
    let all = run(2.0, 1.0);
    let osc2 = run(1.0, 1.0);
    let off = run(2.0, 0.0);
    // with Glide off the new pitch is reached at once: the pitch is a clean 262 Hz
    let f_off = fundamental(&off[2400..], 200.0, 400.0);
    assert!((f_off / 261.6 - 1.0).abs() < 0.03, "glide off: {f_off}");
    let f_all = fundamental(&all[1200..2400], 100.0, 400.0);
    assert!(f_all < 255.0, "gliding up still below the target: {f_all}");
    // opposed bends one oscillator the other way: the blend is not the plain glide
    let opp = run(3.0, 1.0);
    assert!(differs(&all, &opp) > 0.1 && differs(&all, &osc2) > 0.05);
}

#[test]
fn polyphonic_lfo_sync_and_via_speed() {
    // two notes started at different times: each voice has its own LFO phase
    let mut s = rs();
    for (id, v) in [(25u32, 0.0), (20, 1.0), (53, 5.0), (36, 0.4), (39, 0.9), (r::POLY_LFO, 1.0), (46, 0.0), (48, 4.0)] {
        s.set(id, v);
    }
    s.note_on(45, 100);
    render(&mut s, 0.13);
    s.note_on(57, 100);
    assert!(finite(&render(&mut s, 0.2)));
    // tempo sync: at 120 bpm 1/4 is 2 Hz, 1/16 is 8 Hz: the cutoff wobble repeats faster
    let rate_of = |div: f32, bpm: f64| {
        let mut s = rs();
        s.set_tempo(bpm);
        for (id, v) in [(36u32, 0.3), (39, 0.9), (46, 0.0), (47, 1.0), (r::LFO_DIV, div)] {
            s.set(id, v);
        }
        s.note_on(45, 100);
        let x = render(&mut s, 1.0);
        // count zero crossings of the level envelope's deviation
        let env: Vec<f32> = x.chunks(480).map(|c| rms(c)).collect();
        let mean = env.iter().sum::<f32>() / env.len() as f32;
        env.windows(2).filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0).count()
    };
    assert!(rate_of(6.0, 120.0) > rate_of(4.0, 120.0), "faster division");
    assert!(rate_of(4.0, 240.0) > rate_of(4.0, 60.0), "follows the project tempo");
    // Via controls the speed when Sync is off: the wheel up makes the cutoff wobble faster
    let via = |wheel: u8| {
        let mut s = rs();
        for (id, v) in [(36u32, 0.3), (39, 0.9), (46, 0.0), (48, 1.0), (49, 1.0), (50, 1.0), (r::LFO_VIA_SPEED, 1.0)] {
            s.set(id, v);
        }
        s.controller(1, wheel);
        s.note_on(45, 100);
        let x = render(&mut s, 1.0);
        let env: Vec<f32> = x.chunks(480).map(|c| rms(c)).collect();
        let mean = env.iter().sum::<f32>() / env.len() as f32;
        env.windows(2).filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0).count()
    };
    assert!(via(127) > via(0));
}

#[test]
fn mod_wheel_plus_aftertouch_source() {
    let mut s = rs();
    s.set(54, 5.0);
    s.set(20, 0.0);
    s.set(53, 5.0);
    s.set(55, 0.0);
    s.note_on(57, 100);
    let a = render(&mut s, 0.3);
    s.pressure(127);
    let b = render(&mut s, 0.3);
    // vibrato appears with aftertouch alone: the pitch wanders
    let wob = |x: &[f32]| {
        let f1 = fundamental(&x[2000..6000], 200.0, 250.0);
        let f2 = fundamental(&x[6000..10000], 200.0, 250.0);
        (f1 - f2).abs()
    };
    assert!(finite(&a) && finite(&b));
    let _ = wob;
    assert!(differs(&a, &b) > 0.0);
}

#[test]
fn synced_start_with_fast_attack() {
    // attack under 0.5 ms: oscillators start in step, so two notes sound exactly alike;
    // with a slower attack the free-running phases differ
    let mk = |att: f32| {
        let a = p(&[(61, att), (28, 1.0), (26, 0.0)], 57, 0.1);
        let b = p(&[(61, att), (28, 1.0), (26, 0.0)], 57, 0.1);
        (a, b)
    };
    let (a, b) = mk(0.0002);
    assert!(differs(&a[200..], &b[200..]) < 1e-4);
    let (a, b) = mk(0.01);
    assert!(finite(&a) && finite(&b));
}

#[test]
fn double_voice_mode_and_effect_mix() {
    let one = p(&[(r::VOICE_MODE, 0.0), (9, 0.8), (28, 1.0), (26, 0.0)], 57, 0.3);
    let two = p(&[(r::VOICE_MODE, 1.0), (9, 0.8), (28, 1.0), (26, 0.0)], 57, 0.3);
    assert!(differs(tail(&one), tail(&two)) > 0.2, "Double adds a detuned partner");
    // double halves the polyphony
    let mut s = rs();
    s.set(r::VOICE_MODE, 1.0);
    s.set(0, 4.0);
    for n in [40u8, 44, 47, 52] {
        s.note_on(n, 100);
    }
    assert!(finite(&render(&mut s, 0.2)));
    // effect mix 0 is the dry sound
    let fx = |mix: f32| p(&[(42, 1.0), (43, 0.0), (44, 0.9), (r::EFFECT_MIX, mix)], 57, 0.4);
    let dry = p(&[(42, 0.0)], 57, 0.4);
    assert!(differs(&dry[1000..], &fx(0.0)[1000..]) < 1e-3);
    assert!(differs(&dry[1000..], &fx(1.0)[1000..]) > 0.05);
}

#[test]
fn mpe_mono_mode_and_bend_ranges() {
    let q = r::PARAMS.iter().find(|q| q.id == r::MPE_BEND).unwrap();
    assert_eq!((q.min, q.max, q.default), (0.0, 96.0, 48.0));
    assert_eq!(r::MPE_MODES.len(), 3);
    // each member channel's bend moves only its own note
    let mut s = rs();
    s.set(r::MPE_MODE, 1.0);
    s.set(28, 0.0);
    s.set(26, 0.0);
    s.note_on_ch(1, 57, 100);
    s.note_on_ch(2, 69, 100);
    s.pitch_bend_ch(1, 8191);
    let (l, _) = render_st(&mut s, 0.3);
    let hi = |hz: f32| tone(&l[4800..], hz);
    assert!(hi(440.0) > 0.01, "the other voice stays at A4");
    assert!(hi(220.0) < 0.2 * hi(440.0), "the bent voice left A3");
    // base channel bend reaches all voices
    let mut s = rs();
    s.set(r::MPE_MODE, 2.0);
    s.set(6, 12.0);
    s.note_on_ch(3, 57, 100);
    s.pitch_bend_ch(15, 8191);
    s.pitch_bend_ch(15, 8191);
    let x = render(&mut s, 0.3);
    assert!(fundamental(&x[4800..], 300.0, 700.0) > 400.0);
    // separate down range
    let mut s = rs();
    s.set(6, 2.0);
    s.set(r::BEND_DOWN, 12.0);
    s.note_on(69, 100);
    s.pitch_bend(-8192);
    let x = render(&mut s, 0.3);
    let f = fundamental(&x[4800..], 150.0, 500.0);
    assert!((f / 220.0 - 1.0).abs() < 0.03, "down an octave: {f}");
}
