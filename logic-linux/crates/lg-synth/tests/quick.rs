//! Quick Sampler: modes, loop modes, slicing, Flex, Recorder and the Action menu's analysis.

use lg_dsp::{Instrument, Plugin};
use lg_synth::quick::*;
use lg_synth::wav::Audio;

const RATE: f32 = 48000.0;

fn render(q: &mut QuickSampler, secs: f32) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        q.render(cl, cr);
    }
    l
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn tone(secs: f32, hz: f32) -> Audio {
    let n = (secs * RATE) as usize;
    let c: Vec<f32> = (0..n).map(|i| (i as f32 / RATE * hz * std::f32::consts::TAU).sin() * 0.5).collect();
    Audio { rate: RATE as u32, channels: vec![c], loop_points: None, root: None }
}

fn clicks(n: usize, gap: f32) -> Audio {
    let len = (gap * RATE) as usize;
    let mut c = vec![0.0f32; len * n];
    for k in 0..n {
        for i in 0..4000 {
            let t = i as f32 / RATE;
            c[k * len + i] = (t * 440.0 * std::f32::consts::TAU).sin() * (-t * 30.0).exp() * 0.5;
        }
    }
    Audio { rate: RATE as u32, channels: vec![c], loop_points: None, root: None }
}

#[test]
fn classic_loop_keeps_sounding_and_no_loop_stops() {
    let mut q = QuickSampler::new(RATE);
    q.set_audio(tone(0.3, 440.0));
    q.set(LOOP_MODE, 1.0);
    q.set(LOOP_START, 0.4);
    q.set(LOOP_END, 0.9);
    q.note_on(69, 100);
    let l = render(&mut q, 1.0);
    assert!(rms(&l[(0.8 * RATE) as usize..]) > 0.05, "forward loop holds the note");
    let mut q = QuickSampler::new(RATE);
    q.set_audio(tone(0.3, 440.0));
    q.note_on(69, 100);
    let l = render(&mut q, 1.0);
    assert!(rms(&l[(0.6 * RATE) as usize..]) < 1e-3, "no loop ends at the end marker");
    // Alternate and Reverse play too
    for m in [2.0, 3.0, 4.0] {
        let mut q = QuickSampler::new(RATE);
        q.set_audio(tone(0.3, 440.0));
        q.set(LOOP_MODE, m);
        q.note_on(69, 100);
        let l = render(&mut q, 0.8);
        assert!(rms(&l[(0.6 * RATE) as usize..]) > 0.05, "loop mode {m}");
    }
}

#[test]
fn one_shot_ignores_note_off() {
    let mut q = QuickSampler::new(RATE);
    q.set_audio(tone(0.5, 440.0));
    q.set(MODE, 1.0);
    q.note_on(69, 100);
    render(&mut q, 0.05);
    q.note_off(69);
    let l = render(&mut q, 0.3);
    assert!(rms(&l[(0.1 * RATE) as usize..(0.25 * RATE) as usize]) > 0.05);
}

#[test]
fn slices_equal_divisions_and_key_mapping() {
    let mut q = QuickSampler::new(RATE);
    q.set_audio(clicks(4, 0.25));
    q.set(MODE, 2.0);
    q.set(SLICE_MODE, 2.0);
    q.set(SLICES, 8.0);
    assert_eq!(q.slices.len(), 8);
    assert_eq!(slice_keys(36, 1, 4), vec![36, 38, 40, 41]);
    assert_eq!(slice_keys(36, 2, 3), vec![36, 37, 39]);
    // manual slicing: only the start, then a marker by hand (orange: ignores sensitivity)
    q.set(SLICE_MODE, 3.0);
    assert_eq!(q.slices.len(), 1);
    q.set_slice_edits(SliceEdits { manual: vec![12000], removed: vec![] });
    assert_eq!(q.slices, vec![0, 12000]);
    assert_eq!(q.slice_manual, vec![false, true]);
    // transient: a removed detected marker stays away, a kept one survives low sensitivity
    q.set(SLICE_MODE, 0.0);
    q.set_slice_edits(SliceEdits::default());
    assert_eq!(q.slices.len(), 4);
    let third = q.slices[2];
    q.set_slice_edits(SliceEdits { manual: vec![], removed: vec![third] });
    assert_eq!(q.slices.len(), 3);
}

#[test]
fn slice_gate_stops_on_release() {
    let mut q = QuickSampler::new(RATE);
    q.set_audio(tone(1.0, 220.0));
    q.set(MODE, 2.0);
    q.set(SLICE_MODE, 2.0);
    q.set(SLICES, 2.0);
    q.set(GATE, 1.0);
    q.note_on(36, 100);
    render(&mut q, 0.05);
    q.note_off(36);
    let l = render(&mut q, 0.3);
    assert!(rms(&l[(0.25 * RATE) as usize..]) < 1e-3, "gate: released");
}

#[test]
fn flex_keeps_the_length_at_another_pitch() {
    let mut q = QuickSampler::new(RATE);
    q.set_audio(tone(0.4, 440.0));
    q.set(MODE, 1.0);
    q.set(FLEX, 1.0);
    q.note_on(81, 100); // an octave up: without Flex half as long
    let l = render(&mut q, 0.6);
    assert!(rms(&l[(0.25 * RATE) as usize..(0.35 * RATE) as usize]) > 0.05, "still sounding at 0.3 s");
    assert!(rms(&l[(0.5 * RATE) as usize..]) < 0.01, "ended near 0.4 s");
}

#[test]
fn recorder_takes_the_side_chain() {
    let mut q = QuickSampler::new(RATE);
    q.set(MODE, 3.0);
    q.set(RECORD_START, 1.0);
    q.set(THRESHOLD, -20.0);
    q.record_start(2.0);
    let quiet = vec![0.01f32; 512];
    q.side_chain(&quiet, &quiet);
    assert!(!q.rec.running, "waits for the threshold");
    let loud: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    for c in loud.chunks(480) {
        q.side_chain(c, c);
    }
    let a = q.record_stop().expect("recorded");
    assert_eq!(a.frames(), 4800);
}

#[test]
fn analysis_auto_loop_gain_tempo_and_write_loop() {
    let a = tone(1.0, 440.0);
    let (ls, le) = auto_loop(&a, 0, a.frames(), None, 0).expect("a loop");
    assert!(le > ls);
    let period = RATE / 440.0;
    let cycles = (le - ls) as f32 / period;
    assert!((cycles - cycles.round()).abs() < 0.1, "whole periods: {cycles}");
    let g = optimize_gain(&a, 0, a.frames());
    assert!((g - (-1.0 + 6.02)).abs() < 0.2, "gain {g}");
    assert_eq!(tempo_from_length(48000 * 2, 48000), Some(120.0));
    let dir = std::env::temp_dir().join(format!("qs-loop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("t.wav");
    lg_synth::wav::write_wav_24(&f, 48000, &a.channels[0], &a.channels[0]).unwrap();
    write_wav_loop(&f, 69, (1000, 5000)).unwrap();
    let b = lg_synth::wav::read_audio(&f).unwrap();
    assert_eq!(b.loop_points, Some((1000, 5000)));
    assert_eq!(b.root, Some(69));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pitch_env_depth_moves_the_pitch() {
    let mut q = QuickSampler::new(RATE);
    q.set_audio(tone(1.0, 440.0));
    q.set(LOOP_MODE, 1.0);
    q.set(PITCH_ENV_DEPTH, 12.0);
    // envelope 2: full sustain, so the pitch sits an octave up
    q.set(lg_synth::sampler_synth::env_base(PITCH_ENV) + 3, 0.0);
    q.set(lg_synth::sampler_synth::env_base(PITCH_ENV) + 7, 1.0);
    q.note_on(69, 100);
    let l = render(&mut q, 0.3);
    let x = &l[(0.1 * RATE) as usize..(0.3 * RATE) as usize];
    let zc = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 / 0.2;
    assert!((zc - 880.0).abs() < 30.0, "pitch {zc}");
}
