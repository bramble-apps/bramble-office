//! ES2's Tutorial Settings: each loads, plays, and is built as the guide describes.

use lg_dsp::{Instrument, Plugin};
use lg_synth::es2::{Es2, PARAMS};
use lg_synth::es2_tutorials::{lfo2_sync, mm_ids, setting, source, target, NAMES};

const RATE: f32 = 48000.0;

fn get(s: &[(u32, f32)], id: u32) -> f32 {
    s.iter().find(|e| e.0 == id).map(|e| e.1).unwrap()
}

/// Router channel `ch`: (source, target, via, intensity, via intensity).
fn mm(s: &[(u32, f32)], ch: u32) -> (f32, f32, f32, f32, f32) {
    let [t, sr, v, i, vi] = mm_ids(ch);
    (get(s, sr), get(s, t), get(s, v), get(s, i), get(s, vi))
}

fn play(s: &[(u32, f32)], note: u8, secs: f32) -> f32 {
    let mut e = Es2::new(RATE);
    for &(id, v) in s {
        e.set(id, v);
    }
    e.note_on(note, 100);
    let n = (secs * RATE) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for c in (0..n).step_by(256) {
        let k = (c + 256).min(n);
        e.render(&mut l[c..k], &mut r[c..k]);
    }
    (l.iter().chain(&r).map(|x| x * x).sum::<f32>() / (2 * n) as f32).sqrt()
}

#[test]
fn every_setting_loads_and_plays() {
    for n in NAMES {
        let s = setting(n).unwrap();
        assert_eq!(s.len(), PARAMS.len(), "{n}: every parameter");
        for p in PARAMS {
            let v = get(&s, p.id);
            assert!(v >= p.min - 1e-6 && v <= p.max + 1e-6, "{n}: {} = {v}", p.name);
        }
        assert!(play(&s, 60, 0.4) > 1e-3, "{n} is silent");
    }
    assert!(setting("Nope").is_none());
}

#[test]
fn analog_and_fm() {
    let s = setting("Analog Saw Init").unwrap();
    assert_eq!((get(&s, 14), get(&s, 16), get(&s, 21)), (2.0, 0.0, 0.0), "one sawtooth");
    assert_eq!(mm(&s, 1).0, source("ENV2"), "Env 2 pre-routed as the filter envelope");
    assert_eq!(mm(&s, 1).3, 0.0);
    let s = setting("Analog Bass Clean").unwrap();
    assert_eq!((get(&s, 14), get(&s, 12), get(&s, 30)), (3.0, -12.0, 1.0), "a rectangle an octave down, Filter 2");
    let s = setting("Analog Bass Distorted").unwrap();
    assert!(get(&s, 30) == -1.0 && get(&s, 28) > 0.7 && get(&s, 117) > 0.7);
    let s = setting("Analog Saw Unison").unwrap();
    assert_eq!((get(&s, 8), get(&s, 9)), (1.0, 1.0), "monophonic Unison");
    // Filter 2 Cutoff at 0: the preset filter envelope opens it
    let mut shut = s.clone();
    shut.iter_mut().find(|e| e.0 == 36).unwrap().1 = 0.0;
    let mut no_env = shut.clone();
    no_env.iter_mut().find(|e| e.0 == mm_ids(1)[3]).unwrap().1 = 0.0;
    assert!(play(&shut, 48, 0.3) > play(&no_env, 48, 0.3) * 2.0);
    let s = setting("FM Start").unwrap();
    assert_eq!((get(&s, 14), get(&s, 19), get(&s, 27)), (0.0, 0.0, 1.0), "two sines, oscillator 2 at level 0");
    let s = setting("FM Envelope").unwrap();
    assert_eq!(mm(&s, 1).0, source("ENV2"));
    assert_eq!(mm(&s, 1).1, target("Osc1Wave"), "Env 2 → FM intensity on channel 1");
    assert_eq!(mm(&setting("FM Drive").unwrap(), 1).1, target("Cutoff 2"));
    let s = setting("FM Digiwave").unwrap();
    assert!(get(&s, 20) >= 1.0 && get(&s, 9) == 1.0 && get(&s, 8) == 0.0, "a Digiwave modulator, polyphonic Unison");
    assert_eq!(mm(&s, 2).0, source("LFO2"));
    assert_eq!(lg_synth::synthkit::bipolar_rate(get(&s, 95), 100.0, 120.0).1, Some("2 bars"));
    assert_eq!(lg_synth::synthkit::bipolar_rate(lfo2_sync("1/4"), 100.0, 120.0).1, Some("1/4"));
    let s = setting("FM Out of Tune").unwrap();
    assert_eq!((get(&s, 12), get(&s, 17)), (30.0, 0.0));
}

#[test]
fn sync_pwm_ring() {
    let s = setting("Sync Start").unwrap();
    assert_eq!((get(&s, 21), get(&s, 26), get(&s, 27)), (0.0, 0.0, 0.0), "only oscillator 2");
    assert_eq!(mm(&s, 1).0, source("ModWhl"));
    assert_eq!(mm(&s, 2).0, source("ENV1"));
    assert_eq!(mm(&s, 2).1, target("Pitch 2"));
    let s = setting("PWM Slow").unwrap();
    assert_eq!(get(&s, 92), 0.23);
    assert_eq!((mm(&s, 1).0, mm(&s, 1).1), (source("LFO1"), target("Osc2Wave")));
    assert_eq!((mm(&s, 2).0, mm(&s, 2).1), (source("Kybd"), target("LFO1Rate")));
    assert_eq!(get(&setting("Ringmod Start").unwrap(), 19), 6.0);
}

#[test]
fn performance_settings() {
    let s = setting("Crescendo Brass").unwrap();
    assert_eq!((get(&s, 14), get(&s, 19)), (2.0, 3.0));
    assert!(get(&s, 191) < 0.15, "near square");
    assert!(get(&s, 25) >= 1.0, "oscillator 3 a Digiwave");
    assert_eq!((mm(&s, 4).0, mm(&s, 4).1, mm(&s, 4).2), (source("LFO1"), target("Osc2Wave"), source("Kybd")));
    assert_eq!((mm(&s, 3).0, mm(&s, 3).1), (source("ENV1"), target("Osc3Wave")));
    assert!(mm(&s, 5).3 > 0.0 && mm(&s, 6).3 < 0.0 && mm(&s, 5).1 == target("Pitch 2") && mm(&s, 6).1 == target("Pitch 3"));
    assert_eq!((mm(&s, 7).0, mm(&s, 7).1), (source("ModWhl"), target("Amp")));
    assert_eq!((mm(&s, 8).0, mm(&s, 8).2), (source("LFO2"), source("ModWhl")));
    assert!(mm(&s, 9).0 == source("Touch") && mm(&s, 9).3 < 0.0);
    let s = setting("MW-Pad-Creator").unwrap();
    assert!(get(&s, 15) >= 1.0 && get(&s, 25) >= 1.0 && get(&s, 19) == 3.0);
    assert_eq!((mm(&s, 3).0, mm(&s, 3).1), (source("ModWhl"), target("OscWaves")));
    assert_eq!((mm(&s, 4).0, mm(&s, 4).1), (source("ModWhl"), target("LPF FM")));
    assert_eq!(mm(&s, 9).0, source("Touch"));
    assert_eq!(mm(&s, 10).2, source("Touch"));
    let s = setting("Slapped StratENV").unwrap();
    assert!(get(&s, 15) >= 1.0 && get(&s, 25) >= 1.0 && get(&s, 19) == 5.0);
    assert_eq!(mm(&s, 3).2, source("Whl+To"));
    assert!(get(&s, 107) > 0.8 && get(&s, 102) > 0.0);
    let s = setting("Wheelrocker").unwrap();
    assert_eq!((mm(&s, 1).0, mm(&s, 1).1, mm(&s, 1).2), (source("ENV2"), target("Cutoff 1"), source("Kybd")));
    assert!(mm(&s, 2).3 == -mm(&s, 3).3 && mm(&s, 2).0 == source("LFO1"));
    assert_eq!(mm(&s, 4).1, target("OscWaves"));
    assert!(mm(&s, 5).1 == target("Amp") && mm(&s, 5).3 < 0.0);
    assert!(mm(&s, 6).3 == -mm(&s, 7).3);
    assert_eq!((mm(&s, 8).0, mm(&s, 8).1), (source("LFO1"), target("Pan")));
    assert_eq!(mm(&s, 10).1, target("Cutoff 1"));
    let s = setting("Wheelsyncer").unwrap();
    assert_eq!((get(&s, 11), get(&s, 19), get(&s, 17)), (0.0, 5.0, -3.0));
    assert_eq!((mm(&s, 7).0, mm(&s, 7).1, mm(&s, 7).3), (source("ModWhl"), target("Pitch 2"), 1.0));
    assert_eq!(mm(&s, 6).1, target("Cut 1+2"));
    assert_eq!(mm(&s, 8).1, target("Pan"));
    assert_eq!(mm(&s, 9).1, target("LFO1Rate"));
}

#[test]
fn vector_settings() {
    let pts = |n: &str| lg_synth::es2::vec_points(&|id| get(&setting(n).unwrap(), id));
    assert!((pts("Vector Start")[1][0] - 0.498).abs() < 1e-6);
    let k = pts("Vector Kick");
    assert!((k[1][0] - 0.009).abs() < 1e-6 && (k[2][0] - 0.303).abs() < 1e-6);
    let s = setting("Vector Loop").unwrap();
    let l = pts("Vector Loop");
    assert!(l[1..5].iter().all(|p| p[0] == 0.25), "four 25 % segments");
    assert_eq!((get(&s, 132), get(&s, 138), get(&s, 9)), (0.09, 1.0, 1.0));
    let s = setting("Vector Envelope").unwrap();
    assert_eq!((get(&s, 124), get(&s, 125), get(&s, 126), get(&s, 127)), (target("Cutoff 2"), 0.5, target("Pan"), 0.5));
}
