//! Alchemy's arpeggiator: modes, the pattern sequences (velocity rests, tune, pan, length, ties,
//! swing), Split, Latch, Key Trigger, Cycle Reset, Note Vel, transport stop, Mode as a target.

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::Alchemy;
use lg_synth::alchemy_arp::*;

/// The notes started over `secs` at 120 bpm with 1/8 steps (0.25 s).
fn play(c: &ArpCfg, keys: &[u8], secs: f32) -> Vec<(f32, ArpNote)> {
    let mut a = Arp::default();
    for &k in keys {
        assert!(a.note_on(c, k, 90, 0.0, 120.0, false));
    }
    let mut out = Vec::new();
    let mut t = 0.0f32;
    let mut ev = [ArpEv::Off(0); 32];
    while t < secs {
        let n = a.advance(c, 0.01, 120.0, &mut ev);
        for e in &ev[..n] {
            if let ArpEv::On(nt) = e {
                out.push((t, *nt));
            }
        }
        t += 0.01;
    }
    out
}

fn cfg(mode: u8) -> ArpCfg {
    ArpCfg { mode, ..Default::default() }
}

fn pitches(v: &[(f32, ArpNote)]) -> Vec<u8> {
    v.iter().map(|x| x.1.pitch).collect()
}

#[test]
fn modes_order_the_keys() {
    let keys = [64, 60, 67];
    assert_eq!(pitches(&play(&cfg(1), &keys, 1.45)), [60, 64, 67, 60, 64, 67]);
    assert_eq!(pitches(&play(&cfg(2), &keys, 1.45)), [67, 64, 60, 67, 64, 60]);
    assert_eq!(pitches(&play(&cfg(3), &keys, 1.45)), [60, 64, 67, 64, 60, 64]);
    assert_eq!(pitches(&play(&cfg(4), &keys, 1.45)), [67, 64, 60, 64, 67, 64]);
    assert_eq!(pitches(&play(&cfg(5), &keys, 0.7)), [64, 60, 67], "as played");
    let r = pitches(&play(&cfg(6), &keys, 0.7));
    let mut s = r.clone();
    s.sort();
    assert_eq!(s, [60, 64, 67], "random plays each once per round");
    let ch = play(&cfg(7), &keys, 0.2);
    assert_eq!(ch.len(), 3, "chord plays all at once");
    let mut c = cfg(1);
    c.octaves = 2;
    assert_eq!(pitches(&play(&c, &[60, 64], 0.95)), [60, 64, 72, 76]);
}

#[test]
fn sequences_shape_the_steps() {
    let mut c = cfg(1);
    c.pats[0].steps = 4;
    c.pats[0].s[S_VEL][1] = 0.0; // a rest
    c.pats[0].s[S_TUNE][2] = 12.0;
    c.pats[0].s[S_PAN][3] = -1.0;
    c.note_vel = 0.0;
    c.pats[0].s[S_VEL][0] = 0.5;
    let v = play(&c, &[60], 0.95);
    assert_eq!(v.len(), 3, "the rest doesn't play");
    assert_eq!(v[0].1.vel, 64, "Note Vel 0 %: the step's velocity");
    assert_eq!(v[1].1.pitch, 72, "Tune +12");
    assert_eq!(v[2].1.pan, -1.0);
    // length 0 doesn't play; the Length knob scales
    c.pats[0].s[S_LEN][0] = 0.0;
    assert_eq!(play(&c, &[60], 0.2).len(), 0);
    // Note Vel 100 %: the played velocity
    let mut c = cfg(1);
    c.note_vel = 1.0;
    assert_eq!(play(&c, &[60], 0.1)[0].1.vel, 90);
    // pattern knob picks another pattern
    let mut c = cfg(1);
    c.pats[3].s[S_TUNE][0] = 7.0;
    c.pattern = 3;
    assert_eq!(play(&c, &[60], 0.1)[0].1.pitch, 67);
}

#[test]
fn swing_and_ties() {
    let mut c = cfg(1);
    c.swing = 1.0;
    let v = play(&c, &[60], 0.6);
    assert!(v[1].0 > 0.25 + 0.1, "the second step is late: {}", v[1].0);
    assert!((v[2].0 - 0.5).abs() < 0.02, "the third on time");
    // a tied step makes one longer note: the next step on the same key doesn't restart it
    let mut c = cfg(1);
    c.pats[0].s[S_TIE][0] = 1.0;
    let v = play(&c, &[60], 0.6);
    assert!((v[0].0).abs() < 0.02 && (v[1].0 - 0.5).abs() < 0.02, "steps 1–2 are one note: {:?}", v.iter().map(|x| x.0).collect::<Vec<_>>());
}

#[test]
fn split_latch_and_cycle_reset() {
    let mut c = cfg(1);
    c.split = 64;
    let mut a = Arp::default();
    assert!(!a.note_on(&c, 72, 90, 0.0, 120.0, false), "above Split plays directly");
    // Latch Hold: keys up, the pattern goes on; a new chord replaces it
    c.latch = 1;
    a.note_on(&c, 60, 90, 0.0, 120.0, false);
    a.note_off(&c, 60);
    assert_eq!(a.held(), 1);
    a.note_on(&c, 62, 90, 0.0, 120.0, false);
    assert_eq!(a.held(), 1, "Hold replaces");
    c.latch = 2;
    a.note_off(&c, 62);
    a.note_on(&c, 64, 90, 0.0, 120.0, false);
    assert_eq!(a.held(), 2, "Add appends");
    // Cycle Reset: each pattern repeat starts again from the first note
    let mut c = cfg(1);
    c.pats[0].steps = 2;
    let v = pitches(&play(&c, &[60, 64, 67], 1.2));
    assert_eq!(&v[..4], &[60, 64, 60, 64]);
    c.cycle_reset = false;
    let v = pitches(&play(&c, &[60, 64, 67], 1.2));
    assert_eq!(&v[..4], &[60, 64, 67, 60]);
}

#[test]
fn key_trigger_waits_for_the_grid() {
    let mut c = cfg(1);
    c.key_trig = 3; // quarter notes
    let mut a = Arp::default();
    // the song is a third of a beat past a quarter: the pattern waits for the next one
    a.note_on(&c, 60, 90, 10.333, 120.0, true);
    let mut ev = [ArpEv::Off(0); 32];
    let mut first: Option<f32> = None;
    let mut t = 0.0f32;
    while t < 1.0 && first.is_none() {
        if a.advance(&c, 0.01, 120.0, &mut ev) > 0 {
            first = Some(t);
        }
        t += 0.01;
    }
    let f = first.unwrap();
    assert!((f - 0.333).abs() < 0.03, "starts on the grid: {f}");
}

fn render(a: &mut Alchemy, secs: f32) -> Vec<f32> {
    let n = (secs * 48000.0) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for c in (0..n).step_by(256) {
        let e = (c + 256).min(n);
        a.render(&mut l[c..e], &mut r[c..e]);
    }
    l
}

#[test]
fn in_alchemy_transport_and_mode_as_target() {
    let mut a = Alchemy::new(48000.0);
    a.set(5, 0.0);
    a.set(MODE, 1.0);
    a.set_transport(lg_dsp::Transport { beats: 0.0, bpm: 120.0, playing: true });
    a.note_on(60, 100);
    let l = render(&mut a, 0.5);
    assert!(l.iter().any(|x| x.abs() > 1e-3));
    a.set_transport(lg_dsp::Transport { beats: 1.0, bpm: 120.0, playing: false });
    render(&mut a, 1.0);
    let q = render(&mut a, 0.3);
    assert!(q.iter().all(|x| x.abs() < 1e-3), "stopping the transport stops the arpeggio");
    // Mode as a modulation target: Control 1 (Perform) turns it off when down
    use lg_synth::alchemy_mod::*;
    let mut a = Alchemy::new(48000.0);
    a.set(5, 0.0);
    a.set(MODE, 1.0);
    a.set(route(0, RT_TARGET), MODE as f32);
    a.set(route(0, RT_SRC), code(K_PERF, 0));
    a.set(route(0, RT_DEPTH), -1.0);
    a.set(1, 0.0);
    a.note_on(60, 100);
    let l = render(&mut a, 0.6);
    assert!(l.iter().any(|x| x.abs() > 1e-3), "Control 1 down: it plays");
    a.set(1, 1.0);
    render(&mut a, 1.0);
    let q = render(&mut a, 0.6);
    assert!(q.iter().all(|x| x.abs() < 1e-3), "Control 1 up: the arpeggiator is off");
}

#[test]
fn import_midi_sets_pitch_velocity_swing() {
    let v = import_midi(&[(0, 60, 127), (480, 67, 64)], 960, 0.25);
    assert!(v.contains(&(usize::MAX, 0, 3.0)));
    assert!(v.contains(&(S_TUNE, 2, 7.0)) && v.contains(&(S_VEL, 1, 0.0)));
}

#[test]
fn overlapping_repeats_and_legacy_mode() {
    // a key played again while still held joins the arpeggio again …
    let c = cfg(1);
    assert_eq!(pitches(&play(&c, &[60, 64, 60], 0.95)), [60, 60, 64, 60]);
    // … unless Arp Overlap Legacy Mode leaves it out
    let mut c = cfg(1);
    c.overlap_legacy = true;
    assert_eq!(pitches(&play(&c, &[60, 64, 60], 0.95)), [60, 64, 60, 64]);
    // the extended parameter sets it
    let mut a = Alchemy::new(48000.0);
    a.set(OVERLAP_LEGACY, 1.0);
    assert!(a.arp_cfg.overlap_legacy);
}
