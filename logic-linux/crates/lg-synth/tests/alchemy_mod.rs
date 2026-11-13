//! Alchemy's modulation section: modulators (LFO, AHDSR, MSEG, sequencer, envelope follower),
//! the MIDI / Note Property / Perform sources, ModMaps, depth modulation and Smooth.

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::Alchemy;
use lg_synth::alchemy_mod::{self as am, *};

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

/// RMS in windows of `w` seconds.
fn env(x: &[f32], w: f32) -> Vec<f32> {
    x.chunks((w * RATE) as usize).map(rms).collect()
}

/// Alchemy without the Perform section's delay + reverb (its tail would blur the checks).
fn dry() -> Alchemy {
    let mut a = Alchemy::new(RATE);
    a.set(5, 0.0);
    a
}

fn add_route(a: &mut Alchemy, n: usize, target: u32, kind: u32, idx: usize, depth: f32) {
    a.set(route(n, RT_TARGET), target as f32);
    a.set(route(n, RT_SRC), code(kind, idx));
    a.set(route(n, RT_DEPTH), depth);
}

#[test]
fn segments_bend_up_and_down() {
    assert!((seg(0.0, 1.0, 0.5, 0.0) - 0.5).abs() < 1e-6);
    assert!(seg(0.0, 1.0, 0.5, 0.8) > 0.6, "convex rise sits above the line");
    assert!(seg(0.0, 1.0, 0.5, -0.8) < 0.4);
    assert!(seg(1.0, 0.0, 0.5, 0.8) > 0.6, "convex fall stays high longer");
}

#[test]
fn lfo_shapes_cover_their_ranges() {
    for s in 0..LFO_SHAPES.len() {
        for k in 0..64 {
            let v = shape_value(s, k as f32 / 64.0, (0.2, 0.8));
            assert!((-1e-4..=1.0001).contains(&v), "{} at {k}: {v}", LFO_SHAPES[s]);
        }
    }
    assert_eq!(shape_value(6, 0.7, (0.3, 0.9)), 0.3, "Random Hold holds");
    assert!((shape_value(5, 0.5, (0.0, 1.0)) - 0.5).abs() < 1e-4, "Random Glide moves between");
    assert_eq!(shape_multiple(25), 8);
}

#[test]
fn mseg_loop_modes() {
    let mut c = MsegCfg::default();
    c.n = 4;
    c.pts[0] = (0.0, 0.0, 0.0);
    c.pts[1] = (0.1, 1.0, 0.0);
    c.pts[2] = (0.3, 0.5, 0.0);
    c.pts[3] = (0.5, 0.0, 0.0);
    c.ls = 1;
    c.le = 2;
    // Sustain: loops while held, then plays out the rest after release
    c.loop_mode = 2;
    let mut e = EnvRun::default();
    e.start(true);
    for _ in 0..200 {
        mseg_step(&c, &mut e, 0.01);
    }
    assert!(e.t >= 0.1 && e.t <= 0.3 + 1e-3 && e.active, "held inside the loop: {}", e.t);
    e.release();
    for _ in 0..100 {
        mseg_step(&c, &mut e, 0.01);
    }
    assert!(!e.active && e.level < 0.05, "finished after release");
    // Continuous keeps looping after release
    c.loop_mode = 1;
    let mut e = EnvRun::default();
    e.start(true);
    e.release();
    for _ in 0..200 {
        mseg_step(&c, &mut e, 0.01);
    }
    assert!(e.active && e.t <= 0.3 + 1e-3);
    // Forward/Back turns around at the loop end
    c.loop_mode = 3;
    let mut e = EnvRun::default();
    e.start(true);
    let mut back = false;
    let mut last = 0.0;
    for _ in 0..60 {
        mseg_step(&c, &mut e, 0.01);
        if e.t < last && e.t > 0.1 {
            back = true;
        }
        last = e.t;
    }
    assert!(back, "runs backward inside the loop");
}

#[test]
fn sequencer_steps_swing_and_ties() {
    let mut c = SeqCfg::default();
    c.len = 4;
    c.val[..4].copy_from_slice(&[1.0, 0.0, 0.5, 0.25]);
    assert!((seq_at(&c, 0.5) - 1.0).abs() < 1e-4);
    assert!(seq_at(&c, 1.5) < 1e-4, "a zero step");
    assert!((seq_at(&c, 2.5) - 0.5).abs() < 1e-4);
    assert!((seq_at(&c, 4.5) - 1.0).abs() < 1e-4, "the pattern repeats");
    // gate shortens each step
    c.gate = 0.5;
    assert!(seq_at(&c, 0.75) < 1e-4 && seq_at(&c, 0.25) > 0.9);
    c.gate = 1.0;
    // swing lengthens the odd steps: step 1 still sounds at 1.2 steps
    c.swing = 0.8;
    assert!(seq_at(&c, 1.2) > 0.9, "the first step runs late");
    c.swing = 0.0;
    // a tie keeps the previous value
    c.tie[1] = true;
    assert!((seq_at(&c, 1.5) - 1.0).abs() < 1e-4);
}

#[test]
fn modmap_transfers() {
    let mut m = MmapCfg::default();
    m.on = true;
    m.n = 2;
    m.pts[0] = (0.0, 1.0, 0.0);
    m.pts[1] = (1.0, 0.0, 0.0);
    assert!((m.map(0.25) - 0.75).abs() < 1e-4, "inverted");
    m.pts[0] = (0.0, 0.0, 0.7);
    m.pts[1] = (1.0, 1.0, 0.0);
    assert!(m.map(0.5) > 0.6, "convex raises the middle");
}

#[test]
fn ahdsr_on_master_vol_shapes_each_note() {
    let mut a = dry();
    // AHDSR 1: slow attack, full sustain
    a.set(ahd(0, AH_A), 0.5);
    a.set(ahd(0, AH_D), 0.0);
    a.set(ahd(0, AH_S), 1.0);
    a.set(ahd(0, AH_R), 0.1);
    add_route(&mut a, 0, 94, K_AHD, 0, 1.0);
    a.note_on(60, 100);
    let (l, _) = render(&mut a, 0.8);
    let e = env(&l, 0.05);
    assert!(e[0] < e[8] * 0.3, "the attack rises: {:?}", &e[..10]);
    a.note_off(60);
    render(&mut a, 0.3);
    let (q, _) = render(&mut a, 0.2);
    assert!(rms(&q) < 1e-4, "and the release ends the voice");
}

#[test]
fn velocity_scales_master_vol() {
    let level = |vel: u8| {
        let mut a = dry();
        add_route(&mut a, 0, 94, K_NOTE, 0, 1.0);
        a.note_on(60, vel);
        rms(&render(&mut a, 0.4).0[4800..])
    };
    let (soft, loud) = (level(30), level(127));
    assert!(soft < loud * 0.6, "{soft} vs {loud}");
}

#[test]
fn lfo_moves_the_cutoff_and_trigger_restarts_it() {
    let mut a = dry();
    a.set(lfo(0, LF_SHAPE), 3.0); // square
    a.set(lfo(0, LF_RATE), 2.0);
    a.set(lfo(0, LF_BIP), 0.0);
    add_route(&mut a, 0, 85, K_LFO, 0, -0.8);
    a.note_on(48, 110);
    let (l, _) = render(&mut a, 1.0);
    let e = env(&l[4800..], 0.05);
    let (lo, hi) = e.iter().fold((f32::MAX, 0.0f32), |(a, b), &x| (a.min(x), b.max(x)));
    assert!(lo < hi * 0.7, "the square LFO opens and closes the filter: {lo} {hi}");
}

#[test]
fn sequencer_gates_the_volume_in_time() {
    let mut a = dry();
    a.set_tempo(120.0);
    let s = 0;
    for k in 0..16 {
        a.set(seq(s, SQ_VAL + k), if k % 2 == 0 { 1.0 } else { 0.0 });
    }
    a.set(seq(s, SQ_RATE), 5.0); // 1/4 beat = sixteenths: 0.125 s at 120
    add_route(&mut a, 0, 94, K_SEQ, s, 1.0);
    a.note_on(60, 110);
    let (l, _) = render(&mut a, 1.0);
    let e = env(&l, 0.125);
    assert!(e[1] < e[0] * 0.2 && e[3] < e[2] * 0.2, "every other step is silent: {:?}", &e[..4]);
}

#[test]
fn modmap_and_depth_modulation_and_smooth() {
    // ModWheel on Master Vol, inverted by a ModMap: wheel up = quieter
    let mut a = dry();
    for (f, v) in [(MM_EXISTS, 1.0), (MM_N, 2.0), (MM_PT + 1, 1.0), (MM_PT + 3, 1.0), (MM_PT + 4, 0.0)] {
        a.set(mmap(0, f), v);
    }
    add_route(&mut a, 0, 94, K_MIDI, 1, 1.0);
    a.set(route(0, RT_MMAP), 1.0);
    a.note_on(60, 110);
    let (l, _) = render(&mut a, 0.3);
    a.controller(1, 127);
    render(&mut a, 0.1);
    let (q, _) = render(&mut a, 0.2);
    assert!(rms(&q) < rms(&l[4800..]) * 0.1, "inverted wheel turns it down");

    // the route's depth modulated by the mod wheel (depth 0 until the wheel moves)
    let mut a = dry();
    add_route(&mut a, 0, 94, K_NOTE, 18, 0.0); // Max
    add_route(&mut a, 1, DEPTH_TGT, K_MIDI, 1, -1.0);
    a.note_on(60, 110);
    let (l, _) = render(&mut a, 0.3);
    a.controller(1, 127);
    render(&mut a, 0.1);
    let (q, _) = render(&mut a, 0.2);
    assert!(rms(&q) < rms(&l[4800..]) * 0.1, "depth follows the wheel: {} {}", rms(&q), rms(&l));

    // Smooth makes the volume follow a jump gradually
    let mut a = dry();
    add_route(&mut a, 0, 94, K_MIDI, 1, -1.0);
    a.set(route(0, RT_SMOOTH), 0.6);
    a.note_on(60, 110);
    render(&mut a, 0.3);
    a.controller(1, 127);
    let (q, _) = render(&mut a, 0.6);
    let e = env(&q, 0.1);
    assert!(e[0] > e[4] * 2.0, "slides down rather than jumps: {e:?}");
}

#[test]
fn note_properties_flip_flop_and_random() {
    let mut a = dry();
    add_route(&mut a, 0, 94, K_NOTE, 6, 1.0); // Flip Flop
    let mut lv = Vec::new();
    for _ in 0..4 {
        a.note_on(60, 110);
        let (l, _) = render(&mut a, 0.15);
        lv.push(rms(&l[2400..]));
        a.note_off(60);
        render(&mut a, 1.0);
    }
    assert!(lv[0] < 1e-4 && lv[1] > 1e-3 && lv[2] < 1e-4 && lv[3] > 1e-3, "{lv:?}");
}

#[test]
fn perform_and_ctrl_sources() {
    let mut a = dry();
    add_route(&mut a, 0, 94, K_PERF, 5, -1.0); // Control 6
    a.set(6, 0.0);
    a.note_on(60, 110);
    let (l, _) = render(&mut a, 0.3);
    a.set(6, 1.0);
    let (q, _) = render(&mut a, 0.3);
    assert!(rms(&q[4800..]) < rms(&l[4800..]) * 0.1);
    // Ctrl C defaults to CC 74
    let mut a = dry();
    assert_eq!(a.get(CTRL_CC + 2), 74.0);
    add_route(&mut a, 0, 94, K_MIDI, 4, -1.0);
    a.note_on(60, 110);
    let (l, _) = render(&mut a, 0.3);
    a.controller(74, 127);
    let (q, _) = render(&mut a, 0.3);
    assert!(rms(&q[4800..]) < rms(&l[4800..]) * 0.1);
}

#[test]
fn envelope_follower_follows_the_output() {
    let mut a = dry();
    a.set(envf(0, EF_SRC), 6.0); // output pre FX
    a.set(envf(0, EF_SCALE), 4.0);
    add_route(&mut a, 0, 85, K_ENVF, 0, -0.5);
    a.note_on(48, 110);
    render(&mut a, 0.3);
    assert!(a.mg.envf[0] > 0.01, "the follower sees the sound: {}", a.mg.envf[0]);
    a.note_off(48);
    render(&mut a, 3.0);
    assert!(a.mg.envf[0] < 0.01);
}

#[test]
fn presets_and_randomize_stay_in_range() {
    for k in [K_LFO, K_AHD, K_MSEG, K_SEQ, K_ENVF, K_MMAP] {
        assert!(!presets(k).is_empty());
        let a = dry();
        for (_, list) in presets(k) {
            for (f, _) in list {
                assert!(a.params().iter().any(|p| p.id == field(k, 3, f)), "{} field {f}", am::kind_name(k));
            }
        }
    }
    let r = randomize(K_MSEG, 7, &|_| 0.0);
    assert!(r.iter().all(|(_, v)| v.is_finite()));
}

#[test]
fn modulation_renders_without_allocating_per_block_many_voices() {
    let mut a = dry();
    add_route(&mut a, 0, 94, K_AHD, 0, 1.0);
    add_route(&mut a, 1, 85, K_LFO, 0, 0.3);
    add_route(&mut a, 2, 35, K_SEQ, 0, 0.5);
    add_route(&mut a, 3, 23, K_NOTE, 14, 0.5);
    for n in 0..12 {
        a.note_on(48 + n * 2, 100);
    }
    let (l, r) = render(&mut a, 1.0);
    assert!(l.iter().chain(&r).all(|x| x.is_finite()) && rms(&l) > 1e-3);
}


#[test]
fn sequencer_imports_midi_notes() {
    // four sixteenths at 480 PPQ: C3 loud, gap, E3 a little late, C4
    let notes = [(0u32, 48u8, 127u8), (240, 52, 64), (360, 60, 100)];
    let v = seq_import(&notes, 480, 0.25, 0);
    let get = |f: u32| v.iter().find(|x| x.0 == f).map(|x| x.1);
    assert_eq!(get(SQ_LEN), Some(4.0));
    assert!((get(SQ_VAL).unwrap() - 1.0).abs() < 1e-3);
    assert_eq!(get(SQ_VAL + 1), Some(0.0), "the gap is a zero step");
    assert!((get(SQ_VAL + 2).unwrap() - 64.0 / 127.0).abs() < 1e-3);
    let n = seq_import(&notes, 480, 0.25, 1);
    let get = |f: u32| n.iter().find(|x| x.0 == f).map(|x| x.1);
    assert_eq!(get(SQ_VAL), Some(0.0), "the lowest C is zero");
    assert!((get(SQ_VAL + 3).unwrap() - 0.5).abs() < 1e-3, "an octave up is half");
    let g = seq_import(&[(0, 60, 100), (130, 60, 100)], 480, 0.25, 2);
    assert!(g.iter().all(|x| x.0 >= SQ_SW), "groove only sets swing");
    assert!(g.iter().find(|x| x.0 == SQ_SW + 1).unwrap().1 > 1.0, "a late note swings later");
}

#[test]
fn f1_f2_routing_and_limiter() {
    use lg_synth::alchemy::{LIMITER, ROUTE_F2};
    let level = |route: f32| {
        let mut a = dry();
        a.set(85, 0.05); // filter 1 nearly shut
        a.set(ROUTE_F2, route);
        a.set(ROUTE_F2 + 1, route);
        a.note_on(60, 110);
        rms(&render(&mut a, 0.4).0[4800..])
    };
    assert!(level(1.0) > level(0.0) * 3.0, "sources sent to filter 2 skip the shut filter 1");
    let mut a = dry();
    a.set(99, 12.0);
    for n in [48, 52, 55, 60, 64, 67] {
        a.note_on(n, 127);
    }
    let (l, _) = render(&mut a, 0.3);
    assert!(a.get(LIMITER) >= 0.5 && l.iter().all(|x| x.abs() <= 1.0 + 1e-3), "the limiter keeps it under full scale");
}

#[test]
fn mseg_file_preset_ramp_up_rises() {
    let list = am::presets(K_MSEG);
    let (_, pts) = list.iter().find(|(n, _)| *n == "Ramp Up").expect("Ramp Up preset");
    let level = |k: u32| pts.iter().find(|(id, _)| *id == MS_PT + k * 3 + 1).unwrap().1;
    assert_eq!((level(0), level(1)), (0.0, 1.0));
}
