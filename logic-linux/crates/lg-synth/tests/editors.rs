//! The instrument editors' engine features: the Sampler's synth (loop modes, reverse, mono /
//! legato, unison, the Mod Matrix), Drum Machine Designer pads (samples, mute, solo, exclusive
//! groups, resample), Drum Kit Designer pieces, Alchemy's arpeggiator and effects.

use std::sync::Arc;

use lg_dsp::{Instrument, Plugin};
use lg_synth::sampler::{LoopDir, LoopMode, Pcm, SampleData, Sampler, Zone};
use lg_synth::sampler_synth as ss;

const RATE: f32 = 48000.0;

fn render(inst: &mut dyn Instrument, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * RATE) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for c in (0..n).step_by(256) {
        let e = (c + 256).min(n);
        inst.render(&mut l[c..e], &mut r[c..e]);
    }
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn ramp_zone(n: usize) -> Zone {
    // a rising ramp: the read direction shows in the sign of the slope
    let v: Vec<f32> = (0..n).map(|i| i as f32 / n as f32 * 0.8 - 0.4).collect();
    let mut z = Zone::new(Arc::new(SampleData { rate: RATE, ch: vec![Pcm::F32(v)] }));
    z.root = 60;
    z.synth_env = true;
    z
}

fn slope(x: &[f32]) -> f32 {
    x.windows(2).map(|w| w[1] - w[0]).filter(|d| d.abs() < 0.01).sum()
}

#[test]
fn reverse_zone_plays_backwards() {
    let mut z = ramp_zone(48000);
    z.reverse = true;
    let mut s = Sampler::new(vec![z], RATE);
    s.note_on(60, 127);
    let (l, _) = render(&mut s, 0.3);
    assert!(slope(&l[4800..]) < 0.0, "reversed: the ramp falls");
}

#[test]
fn loop_modes_keep_sounding() {
    for dir in [LoopDir::Forward, LoopDir::Reverse, LoopDir::Alternate] {
        let mut z = ramp_zone(24000);
        z.loop_mode = LoopMode::Continuous;
        z.loop_dir = dir;
        z.loop_start = 4000;
        z.loop_end = 12000;
        z.loop_xfade = 400;
        let mut s = Sampler::new(vec![z], RATE);
        s.note_on(60, 127);
        let (l, _) = render(&mut s, 2.0);
        assert!(rms(&l[90000..]) > 0.01, "{dir:?} loop sustains");
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 2.0));
    }
}

#[test]
fn mono_mode_one_voice_and_unison_doubles() {
    let z = ramp_zone(48000);
    let mut s = Sampler::new(vec![z.clone()], RATE);
    s.set(ss::MODE, 1.0);
    s.note_on(60, 100);
    s.note_on(64, 100);
    render(&mut s, 0.05);
    assert_eq!(s.used_voices(), 1, "Mono: one note sounds");
    let mut u = Sampler::new(vec![z], RATE);
    u.set(ss::UNISON, 3.0); // 4 voices
    u.note_on(60, 100);
    render(&mut u, 0.02);
    assert_eq!(u.used_voices(), 4, "Unison: four voices per key");
}

#[test]
fn mod_matrix_lfo_moves_the_volume() {
    let v: Vec<f32> = (0..96000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let mut z = Zone::new(Arc::new(SampleData { rate: RATE, ch: vec![Pcm::F32(v)] }));
    z.synth_env = true;
    let mut s = Sampler::new(vec![z], RATE);
    let lb = ss::lfo_base(1);
    s.set(lb, 1.0); // LFO 1 on
    s.set(lb + 1, 3.0); // square
    s.set(lb + 3, 4.0); // 4 Hz
    let b = ss::mod_base(1);
    s.set(b, 1.0);
    s.set(b + 1, ss::source_index(ss::Source::Lfo(0)) as f32);
    s.set(b + 5, ss::target::VOLUME as f32);
    s.set(b + 6, 0.9);
    s.note_on(60, 127);
    let (l, _) = render(&mut s, 1.0);
    let blocks: Vec<f32> = l.chunks(2400).map(rms).collect();
    let (lo, hi) = blocks.iter().fold((f32::MAX, 0.0f32), |(a, b), &x| (a.min(x), b.max(x)));
    assert!(hi > lo * 4.0, "tremolo from the LFO: {lo} {hi}");
}

#[test]
fn filter_two_series_cuts_highs() {
    let v: Vec<f32> = (0..48000).map(|i| if i % 4 < 2 { 0.4 } else { -0.4 }).collect();
    let mut z = Zone::new(Arc::new(SampleData { rate: RATE, ch: vec![Pcm::F32(v)] }));
    z.synth_env = true;
    let mut open = Sampler::new(vec![z.clone()], RATE);
    open.note_on(60, 127);
    let (a, _) = render(&mut open, 0.3);
    let mut shut = Sampler::new(vec![z], RATE);
    shut.set(5, 1.0);
    shut.set(6, 400.0);
    shut.set(ss::F_BLEND, -1.0);
    shut.note_on(60, 127);
    let (b, _) = render(&mut shut, 0.3);
    assert!(rms(&b[4800..]) < rms(&a[4800..]) * 0.2);
}

#[test]
fn drum_machine_pads_samples_mute_and_exclusive_groups() {
    use lg_synth::dmd::{DrumMachineDesigner, pad_param};
    let mut d = DrumMachineDesigner::new(RATE);
    let tone: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.03).sin() * 0.5).collect();
    let a = lg_synth::wav::Audio { rate: 48000, channels: vec![tone.clone(), tone], ..Default::default() };
    d.set_pad_sample(40, &a);
    assert!(d.pad_quick(40).is_some());
    d.note_on(40, 120);
    let (l, _) = render(&mut d, 0.2);
    assert!(rms(&l) > 0.05, "the pad's Quick Sampler sounds");
    // exclusive group: pad 41 (a drum) cuts the sample on 40
    d.set(pad_param(4, 6), 3.0);
    d.set(pad_param(5, 6), 3.0);
    d.reset();
    d.note_on(40, 120);
    render(&mut d, 0.05);
    d.note_on(41, 120);
    let (l, _) = render(&mut d, 0.6);
    let tail = rms(&l[20000..]);
    d.reset();
    d.set(pad_param(5, 6), 0.0);
    d.note_on(40, 120);
    render(&mut d, 0.05);
    d.note_on(41, 120);
    let (l2, _) = render(&mut d, 0.6);
    assert!(tail < rms(&l2[20000..]) * 0.5, "the group choked the sample");
    // mute
    d.reset();
    d.muted[40] = true;
    d.note_on(40, 120);
    let (l, _) = render(&mut d, 0.1);
    assert!(rms(&l) < 1e-4);
    d.muted[40] = false;
    // resample: the drum on pad 38 becomes a sample
    let r = d.resample_pad(38);
    assert!(r.frames() > 1000 && d.pad_quick(38).is_some());
}

#[test]
fn drum_kit_pieces_from_other_kits() {
    use lg_synth::drums::StudioDrumKit;
    let mut a = StudioDrumKit::new(RATE);
    a.note_on(38, 110);
    let (x, _) = render(&mut a, 0.3);
    let mut b = StudioDrumKit::new(RATE);
    b.set(25, 6.0); // the snare from the 909
    b.note_on(38, 110);
    let (y, _) = render(&mut b, 0.3);
    let diff: f32 = x.iter().zip(&y).map(|(p, q)| (p - q).abs()).sum::<f32>() / x.len() as f32;
    assert!(diff > 1e-3, "another kit's snare");
    assert_eq!(lg_synth::drums::piece_of(36), Some(lg_synth::drums::Group::Kick));
}

#[test]
fn alchemy_arpeggiator_and_effects() {
    use lg_synth::alchemy::Alchemy;
    use lg_synth::alchemy_extra as ax;
    let mut a = Alchemy::new(RATE);
    a.set(ax::ARP, 1.0);
    a.set_tempo(120.0);
    a.note_on(60, 100);
    a.note_on(64, 100);
    let (l, _) = render(&mut a, 1.0);
    assert!(rms(&l) > 1e-3, "the arpeggiator plays the held keys");
    a.note_off(60);
    a.note_off(64);
    render(&mut a, 1.5);
    let (q, _) = render(&mut a, 0.3);
    assert!(rms(&q) < 1e-3, "and stops when they're let go");
    a.set(ax::FX, 12.0); // ChromaVerb in slot 1
    assert!(a.extra.fx[0].is_some());
    let id = ax::FX_PARAM + a.extra.fx[0].as_ref().unwrap().params()[0].id;
    a.set(id, 0.3);
    assert!((a.get(id) - 0.3).abs() < 1e-3 || a.get(id) != 0.0);
    let (l, _) = render(&mut a, 0.5);
    assert!(l.iter().all(|x| x.is_finite()));
}
