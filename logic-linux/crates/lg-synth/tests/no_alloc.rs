//! render() must not allocate: a counting global allocator watches every instrument's render
//! calls (note on/off and controllers may set things up, rendering may not).

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
fn render_is_allocation_free() {
    let rate = 48000.0;
    let dir = std::env::temp_dir().join(format!("lg-synth-noalloc-{}", std::process::id()));
    let sfz = lg_synth::sampler::write_test_instrument(&dir).unwrap();
    let mut insts: Vec<(String, Box<dyn lg_dsp::Instrument>)> = Vec::new();
    for name in [
        "Vintage Electric Piano",
        "Vintage B3 Organ",
        "Vintage Clav",
        "Retro Synth",
        "Piano",
        "Drum Kit Designer",
        "Bass",
        "Strings",
        "Guitar",
        "ES2",
        "Alchemy",
        "ES P",
        "ES1",
        "ES E",
        "ES M",
        "EFM1",
        "Sculpture",
        "Vintage Mellotron",
        "Drum Machine Designer",
        "Studio Horns",
    ] {
        insts.push((name.into(), lg_synth::by_logic_name(name, rate).unwrap()));
    }
    insts.push((
        "sampler".into(),
        Box::new(lg_synth::sampler::Sampler::from_sfz(&sfz, rate).unwrap()),
    ));
    for flag in [293u32, 309, 313] {
        let mut a = lg_synth::alchemy::Alchemy::new(rate);
        lg_dsp::Plugin::set(&mut a, flag, 1.0);
        insts.push((format!("alchemy source {flag}"), Box::new(a)));
    }
    // the bundled sampled instruments (when the library is installed), synthesized fallbacks too
    for name in ["Piano", "Drum Kit Designer", "Strings", "Guitar", "Bass", "Studio Horns"] {
        insts.push((format!("synth {name}"), lg_synth::synthesized_by_logic_name(name, rate).unwrap()));
    }
    for s in lg_sounds::index() {
        if let Some(i) = lg_synth::sampled::SampledInstrument::load(s.id, rate) {
            insts.push((format!("sampled {}", s.id), Box::new(i)));
        }
    }
    let sf2 = std::path::Path::new("/usr/share/sounds/sf2/TimGM6mb.sf2");
    if sf2.exists() {
        insts.push((
            "sf2 native".into(),
            Box::new(lg_synth::sampler::SoundFontPlayer::new(sf2, 0, 0, rate).unwrap()),
        ));
    }
    let mut l = vec![0.0f32; 1024];
    let mut r = vec![0.0f32; 1024];
    for (name, inst) in insts.iter_mut() {
        for n in [36u8, 38, 42, 48, 60, 64, 67, 72] {
            inst.note_on(n, 100);
        }
        inst.controller(1, 127);
        inst.pitch_bend(3000);
        COUNT.store(0, Ordering::SeqCst);
        ARMED.store(true, Ordering::SeqCst);
        for block in [1usize, 17, 64, 128, 1024] {
            for _ in 0..20 {
                inst.render(&mut l[..block], &mut r[..block]);
            }
        }
        ARMED.store(false, Ordering::SeqCst);
        let c = COUNT.load(Ordering::SeqCst);
        assert_eq!(c, 0, "{name}: {c} allocations while rendering");
    }
    std::fs::remove_dir_all(&dir).ok();
}
