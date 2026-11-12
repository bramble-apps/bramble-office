//! Sample Alchemy: render() must not allocate, in every play mode, synthesis mode and filter
//! (loading a sample, notes and parameter changes may set things up).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;
static ARMED: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn render_after_loading_a_sample_is_allocation_free() {
    use lg_dsp::{Instrument, Plugin};
    use lg_synth::sample_alchemy::*;
    let rate = 48000.0;
    let sample: Vec<f32> = (0..48000).map(|i| (1..8).map(|k| (std::f32::consts::TAU * 220.0 * k as f32 * i as f32 / rate).sin() / k as f32).sum::<f32>() * 0.2).collect();
    let mut a = SampleAlchemy::new(rate);
    a.set_sample(sample, rate);
    a.set_tempo(120.0);
    // a motion, a second source, the global filter
    for k in 0..MOTION_STEPS {
        a.set(motion_id(0, 0, k), k as f32 / MOTION_STEPS as f32);
    }
    a.set(sid(1, o::ON), 1.0);
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    for play in 0..5 {
        for mode in 0..3 {
            for fx in 0..effects(mode).len() {
                for ft in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
                    for s in 0..2 {
                        a.set(sid(s, o::PLAY), play as f32);
                        a.set(sid(s, o::MODE), mode as f32);
                        a.set(sid(s, o::EFFECT), fx as f32);
                        a.set(sid(s, o::F_ON), 1.0);
                        a.set(sid(s, o::F_TYPE), ft);
                    }
                    a.set(lfo_id(0, 1), 5.0);
                    a.set(route_id(10, 0), 9.0);
                    a.set(route_id(10, 1), 1.0);
                    a.set(route_id(10, 2), 0.5);
                    a.note_on(48, 100);
                    a.note_on(64, 90);
                    COUNT.store(0, Ordering::Relaxed);
                    ARMED.store(true, Ordering::Relaxed);
                    for _ in 0..4 {
                        a.render(&mut l, &mut r);
                    }
                    ARMED.store(false, Ordering::Relaxed);
                    assert_eq!(COUNT.load(Ordering::Relaxed), 0, "play {play} mode {mode} effect {fx} filter {ft} allocated while rendering");
                    a.note_off(48);
                    a.note_off(64);
                    a.reset();
                }
            }
        }
    }
}
