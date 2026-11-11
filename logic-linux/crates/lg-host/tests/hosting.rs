//! Hosting checks with the test plug-ins built from examples/ (cargo test builds them): load,
//! process, parameters, latency, state, the scan in a separate process (a crashing plug-in
//! marked, Studio still running), the editor embedded in a host window (when an X display is
//! there and LG_HOST_GUI_TEST is set: run under `weston --backend=headless --xwayland`).

use std::path::{Path, PathBuf};

use lg_dsp::{Effect, Instrument, Plugin};
use lg_host::{Format, Kind};

fn examples_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().join("examples")
}

fn lib(name: &str) -> PathBuf {
    let p = examples_dir().join(format!("lib{name}.so"));
    assert!(p.exists(), "{} missing: run `cargo test -p lg-host` (it builds the examples)", p.display());
    p
}

/// A fresh folder under the target dir.
fn scratch(name: &str) -> PathBuf {
    let d = examples_dir().parent().unwrap().join("lg-host-tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn clap_bundle(dir: &Path, file: &str) -> PathBuf {
    let p = dir.join(file);
    std::fs::copy(lib("test_clap"), &p).unwrap();
    p
}

fn impulse_through(fx: &mut lg_host::Instance, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut outl, mut outr) = (Vec::new(), Vec::new());
    for b in 0..frames / 64 {
        let mut l = vec![0f32; 64];
        let mut r = vec![0f32; 64];
        if b == 0 {
            l[0] = 1.0;
            r[0] = -1.0;
        }
        fx.process(&mut l, &mut r);
        outl.extend(l);
        outr.extend(r);
    }
    (outl, outr)
}

#[test]
fn clap_effect_and_instrument() {
    let d = scratch("clap");
    let path = clap_bundle(&d, "Test.clap");
    let list = lg_host::clap::describe(&path).unwrap();
    assert_eq!(list.iter().map(|i| (i.name.as_str(), i.kind)).collect::<Vec<_>>(), [("Test Gain", Kind::Effect), ("Test Synth", Kind::Instrument)]);
    let gain = &list[0];
    assert_eq!((gain.vendor.as_str(), gain.inputs, gain.outputs, gain.params, gain.latency, gain.has_editor), ("Bramble Test", 2, 2, 3, 32, true));
    assert_eq!(gain.studio_name(), "Test Gain (CLAP)");

    let mut fx = lg_host::open(gain, 48000.0).unwrap();
    let names: Vec<&str> = fx.params().iter().map(|p| p.name).collect();
    assert_eq!(names, ["Gain", "Mode", "Ticks"]);
    let mode = &fx.params()[1];
    assert_eq!((mode.unit, mode.choices), (lg_dsp::Unit::Menu, &["Clean", "Half", "Mute"][..]));
    assert_eq!(Effect::latency(&fx), 32, "latency goes to the delay compensation");
    // unity: the impulse comes out 32 samples late
    let (l, r) = impulse_through(&mut fx, 256);
    assert_eq!(l.iter().position(|x| x.abs() > 0.5), Some(32));
    assert!((r[32] + 1.0).abs() < 1e-6);
    // -6.02 dB and Mode Half: a quarter
    fx.set(0, -6.0206);
    fx.set(1, 1.0);
    assert!((fx.get(0) + 6.0206).abs() < 1e-4);
    assert_eq!(fx.get(1), 1.0);
    assert_eq!(fx.value_text(0, -6.0206).as_deref(), Some("-6.0 dB"));
    assert_eq!(fx.value_text(1, 2.0).as_deref(), Some("Mute"));
    let (l, _) = impulse_through(&mut fx, 256);
    assert!((l[32] - 0.25).abs() < 1e-3, "got {}", l[32]);

    // the state rides in the floats: a new instance gets the same settings
    let floats = fx.floats();
    let (values, blob) = lg_host::state::decode(&floats);
    assert_eq!(values.len(), 3);
    assert!(String::from_utf8_lossy(blob.as_deref().unwrap()).starts_with("bramble-test -6.02"));
    let mut fx2 = lg_host::open(gain, 48000.0).unwrap();
    fx2.load_floats(&floats);
    assert!((fx2.get(0) + 6.0206).abs() < 1e-3);
    assert_eq!(fx2.get(1), 1.0);
    let (l, _) = impulse_through(&mut fx2, 256);
    assert!((l[32] - 0.25).abs() < 1e-3);
    // a Controls-view edit after the state was taken wins over the state
    let mut edited = floats.clone();
    edited[1] = 2.0;
    fx2.load_floats(&edited);
    assert_eq!(fx2.get(1), 2.0);

    // the instrument: a sine while the note is held
    let mut syn = lg_host::open(&list[1], 48000.0).unwrap();
    assert_eq!(syn.params().iter().map(|p| p.name).collect::<Vec<_>>(), ["Level"]);
    let peak = |s: &mut lg_host::Instance, blocks: usize| {
        let mut m = 0f32;
        for _ in 0..blocks {
            let mut l = vec![0f32; 64];
            let mut r = vec![0f32; 64];
            s.render(&mut l, &mut r);
            m = m.max(l.iter().fold(0f32, |a, x| a.max(x.abs())));
        }
        m
    };
    assert_eq!(peak(&mut syn, 4), 0.0);
    syn.note_on(69, 100);
    let p = peak(&mut syn, 20);
    assert!((p - 0.2).abs() < 0.01, "level 0.8 × 0.25: {p}");
    syn.set(0, 0.4);
    assert!((peak(&mut syn, 20) - 0.1).abs() < 0.01);
    syn.note_off(69);
    assert_eq!(peak(&mut syn, 4), 0.0);
}

#[test]
fn scan_runs_in_a_separate_process() {
    let d = scratch("scan");
    clap_bundle(&d, "Test.clap");
    clap_bundle(&d, "crash-me.clap");
    std::fs::write(d.join("not-a-plugin.clap"), b"hello").unwrap();
    unsafe {
        std::env::set_var("LG_PLUGIN_PATH", &d);
        std::env::set_var("LG_PLUGIN_CACHE", d.join("cache.txt"));
        std::env::set_var("LG_PLUGSCAN", env!("CARGO_BIN_EXE_lg-plugscan"));
        std::env::set_var("HOME", &d);
    }
    let files: Vec<PathBuf> = lg_host::scan::find_files().into_iter().filter(|(f, _)| *f == Format::Clap).map(|(_, p)| p).collect();
    assert_eq!(files.len(), 3);
    let all = lg_host::scan::scan_all(&Default::default(), None);
    let of = |n: &str| all.iter().filter(|e| e.path.file_name().unwrap() == n).cloned().collect::<Vec<_>>();
    assert_eq!(of("Test.clap").len(), 2);
    assert!(of("Test.clap").iter().all(|e| e.status.ok()));
    assert_eq!(of("crash-me.clap")[0].status, lg_host::scan::Status::Crashed(1), "the crash took only the scanner down");
    assert!(matches!(of("not-a-plugin.clap")[0].status, lg_host::scan::Status::Failed(_)));
    // the loadable list, by Studio name
    assert!(lg_host::find("Test Synth (CLAP)").is_some());
    assert!(lg_host::find("Test Gain (VST3)").is_none());
    // the cache: read back the same; a rescan of the crasher counts again
    let cached = lg_host::scan::load_cache();
    assert_eq!(cached.len(), all.len());
    let force = [d.join("crash-me.clap")].into_iter().collect();
    let again = lg_host::scan::scan_all(&force, None);
    assert_eq!(again.iter().find(|e| e.path.ends_with("crash-me.clap")).unwrap().status, lg_host::scan::Status::Crashed(2));
    // ignored in the Plug-in Manager: not scanned, not loaded
    let mut c = lg_host::scan::choices();
    c.ignored.insert(d.join("crash-me.clap").display().to_string());
    lg_host::scan::save_choices(&c);
    let third = lg_host::scan::scan_all(&Default::default(), None);
    assert_eq!(third.iter().find(|e| e.path.ends_with("crash-me.clap")).unwrap().status, lg_host::scan::Status::Ignored);
    assert!(lg_host::open_by_name("Test Gain (CLAP)", 48000.0).is_some());
}

#[test]
fn clap_editor_embeds_in_a_host_window() {
    // (only on a headless display made for the check, never the desktop's)
    if std::env::var_os("LG_HOST_GUI_TEST").is_none() || !lg_host::editor::x11_available() {
        eprintln!("editor check skipped: set LG_HOST_GUI_TEST=1 and DISPLAY to a headless X (weston --backend=headless --xwayland)");
        return;
    }
    let d = scratch("editor");
    let path = clap_bundle(&d, "Test.clap");
    let mut p = lg_host::clap::ClapPlugin::open(&path, "org.bramble.test-gain", 48000.0).unwrap();
    use lg_host::Backend as _;
    p.open_editor("Test Gain").unwrap();
    assert!(p.editor_open());
    std::thread::sleep(std::time::Duration::from_millis(50));
    let win = p.editor_window().expect("embedded in Studio's window");
    assert_eq!(win.size, (320, 200));
    assert_eq!(win.children().len(), 1, "the plug-in's own window is inside");
    // the plug-in's timer runs from idle, and its output events reach the host
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_millis(300) {
        p.idle();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut l = vec![0f32; 64];
    let mut r = vec![0f32; 64];
    let (il, ir) = (l.clone(), r.clone());
    p.process([&il, &ir], [&mut l, &mut r], &[], 120.0);
    assert!(p.get(3) > 5.0, "timer ticks: {}", p.get(3));
    assert!(p.idle().changed, "output events mark the setting changed");
    p.close_editor();
    assert!(!p.editor_open());
}

pub fn vst3_bundle(dir: &Path, name: &str) -> PathBuf {
    let b = dir.join(format!("{name}.vst3"));
    let so = b.join("Contents").join(format!("{}-linux", std::env::consts::ARCH));
    std::fs::create_dir_all(&so).unwrap();
    std::fs::copy(lib("test_vst3"), so.join(format!("{name}.so"))).unwrap();
    b
}

#[test]
fn vst3_effect() {
    let d = scratch("vst3");
    let path = vst3_bundle(&d, "TestGain");
    let list = lg_host::vst3::describe(&path).unwrap();
    assert_eq!(list.len(), 1);
    let i = &list[0];
    assert_eq!((i.name.as_str(), i.vendor.as_str(), i.kind, i.category.as_str()), ("Test VST3 Gain", "Bramble Test", Kind::Effect, "Fx|Tools"));
    assert_eq!((i.inputs, i.outputs, i.params, i.latency, i.has_editor), (2, 2, 2, 16, true), "the read-only Meter is left out");
    let mut fx = lg_host::open(i, 48000.0).unwrap();
    assert_eq!(fx.params().iter().map(|p| p.name).collect::<Vec<_>>(), ["Gain", "Polarity"]);
    assert_eq!(fx.params()[1].choices, ["Normal", "Inverted"]);
    assert_eq!(Effect::latency(&fx), 16);
    // the component and the controller met through the host's IMessage
    let so = lg_host::vst3::library_of(&path).unwrap();
    let probe = unsafe { libloading::Library::new(&so).unwrap() };
    let msg: libloading::Symbol<extern "C" fn() -> u32> = unsafe { probe.get(b"bramble_test_message\0").unwrap() };
    assert_eq!(msg(), 42);
    // unity (0 dB = 60/72 normalized), 16 samples late
    let (l, _) = impulse_through(&mut fx, 128);
    assert_eq!(l.iter().position(|x| x.abs() > 0.5), Some(16));
    assert_eq!(fx.value_text(0, 54.0 / 72.0).as_deref(), Some("-6.0 dB"));
    // -6 dB and inverted
    fx.set(0, 54.0 / 72.0);
    fx.set(1, 1.0);
    let (l, _) = impulse_through(&mut fx, 128);
    assert!((l[16] + 0.501).abs() < 0.01, "{}", l[16]);
    // state round trip into a fresh instance
    let floats = fx.floats();
    assert!(lg_host::state::decode(&floats).1.is_some());
    let mut fx2 = lg_host::open(i, 48000.0).unwrap();
    fx2.load_floats(&floats);
    assert!((fx2.get(0) - 54.0 / 72.0).abs() < 1e-4);
    assert_eq!(fx2.get(1), 1.0);
    let (l, _) = impulse_through(&mut fx2, 128);
    assert!((l[16] + 0.501).abs() < 0.01);
}

#[test]
fn vst3_editor_embeds_and_runs_its_timer() {
    if std::env::var_os("LG_HOST_GUI_TEST").is_none() || !lg_host::editor::x11_available() {
        eprintln!("editor check skipped (see clap_editor_embeds_in_a_host_window)");
        return;
    }
    let d = scratch("vst3-editor");
    let path = vst3_bundle(&d, "TestGain");
    let i = lg_host::vst3::describe(&path).unwrap().remove(0);
    let mut p = lg_host::vst3::Vst3Plugin::open(&path, &i.id, 48000.0).unwrap();
    use lg_host::Backend as _;
    p.open_editor("Test VST3 Gain").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let win = p.editor_window().unwrap();
    assert_eq!(win.size, (240, 120));
    assert_eq!(win.children().len(), 1);
    let start = std::time::Instant::now();
    let mut changed = false;
    while start.elapsed() < std::time::Duration::from_millis(300) {
        changed |= p.idle().changed;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(changed, "the editor's performEdit (from its IRunLoop timer) reached the host");
    assert!((p.get(0) - 0.25).abs() < 1e-9);
    p.close_editor();
    assert!(!p.editor_open());
}

/// An LV2 bundle: the library and its Turtle.
pub fn lv2_bundle(dir: &Path) -> PathBuf {
    let b = dir.join("test.lv2");
    std::fs::create_dir_all(&b).unwrap();
    std::fs::copy(lib("test_lv2"), b.join("test.so")).unwrap();
    std::fs::write(b.join("manifest.ttl"), r#"@prefix lv2: <http://lv2plug.in/ns/lv2core#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ui: <http://lv2plug.in/ns/extensions/ui#> .
<urn:bramble:test-lv2-gain> a lv2:Plugin ; lv2:binary <test.so> ; rdfs:seeAlso <test.ttl> .
<urn:bramble:test-lv2-synth> a lv2:Plugin ; lv2:binary <test.so> ; rdfs:seeAlso <test.ttl> .
<urn:bramble:test-lv2-gain#ui> a ui:X11UI ; ui:binary <test.so> ; rdfs:seeAlso <test.ttl> .
"#).unwrap();
    std::fs::write(b.join("test.ttl"), r#"@prefix lv2: <http://lv2plug.in/ns/lv2core#> .
@prefix doap: <http://usefulinc.com/ns/doap#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix atom: <http://lv2plug.in/ns/ext/atom#> .
@prefix urid: <http://lv2plug.in/ns/ext/urid#> .
@prefix ui: <http://lv2plug.in/ns/extensions/ui#> .
<urn:bramble:test-lv2-gain> a lv2:Plugin, lv2:AmplifierPlugin ;
  doap:name "Test LV2 Gain" ; doap:maintainer [ foaf:name "Bramble Test" ] ;
  ui:ui <urn:bramble:test-lv2-gain#ui> ;
  lv2:port [ a lv2:AudioPort, lv2:InputPort ; lv2:index 0 ; lv2:symbol "in_l" ; lv2:name "In L" ] ,
    [ a lv2:AudioPort, lv2:InputPort ; lv2:index 1 ; lv2:symbol "in_r" ; lv2:name "In R" ] ,
    [ a lv2:AudioPort, lv2:OutputPort ; lv2:index 2 ; lv2:symbol "out_l" ; lv2:name "Out L" ] ,
    [ a lv2:AudioPort, lv2:OutputPort ; lv2:index 3 ; lv2:symbol "out_r" ; lv2:name "Out R" ] ,
    [ a lv2:ControlPort, lv2:InputPort ; lv2:index 4 ; lv2:symbol "gain" ; lv2:name "Gain" ; lv2:default 0.0 ; lv2:minimum -60.0 ; lv2:maximum 12.0 ] ,
    [ a lv2:ControlPort, lv2:InputPort ; lv2:index 5 ; lv2:symbol "mode" ; lv2:name "Mode" ; lv2:default 0 ; lv2:minimum 0 ; lv2:maximum 2 ;
      lv2:portProperty lv2:integer, lv2:enumeration ;
      lv2:scalePoint [ rdfs:label "Clean" ; rdf:value 0 ] , [ rdfs:label "Half" ; rdf:value 1 ] , [ rdfs:label "Mute" ; rdf:value 2 ] ] ,
    [ a lv2:ControlPort, lv2:OutputPort ; lv2:index 6 ; lv2:symbol "latency" ; lv2:name "Latency" ; lv2:portProperty lv2:reportsLatency ] .
<urn:bramble:test-lv2-synth> a lv2:Plugin, lv2:InstrumentPlugin ;
  doap:name "Test LV2 Synth" ; doap:maintainer [ foaf:name "Bramble Test" ] ;
  lv2:requiredFeature urid:map ;
  lv2:port [ a atom:AtomPort, lv2:InputPort ; atom:bufferType atom:Sequence ; atom:supports <http://lv2plug.in/ns/ext/midi#MidiEvent> ; lv2:index 0 ; lv2:symbol "midi" ; lv2:name "MIDI" ] ,
    [ a lv2:AudioPort, lv2:OutputPort ; lv2:index 1 ; lv2:symbol "out_l" ; lv2:name "Out L" ] ,
    [ a lv2:AudioPort, lv2:OutputPort ; lv2:index 2 ; lv2:symbol "out_r" ; lv2:name "Out R" ] ,
    [ a lv2:ControlPort, lv2:InputPort ; lv2:index 3 ; lv2:symbol "level" ; lv2:name "Level" ; lv2:default 0.8 ; lv2:minimum 0.0 ; lv2:maximum 1.0 ] .
"#.replace("@prefix ui:", "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n@prefix ui:")).unwrap();
    b
}

#[test]
fn lv2_effect_and_instrument() {
    if !lg_host::lv2::available() {
        eprintln!("no liblilv-0.so.0 here: LV2 check skipped");
        return;
    }
    let d = scratch("lv2");
    let b = lv2_bundle(&d);
    let mut list = lg_host::lv2::describe(&b).unwrap();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(list.iter().map(|i| (i.name.as_str(), i.vendor.as_str(), i.kind)).collect::<Vec<_>>(), [("Test LV2 Gain", "Bramble Test", Kind::Effect), ("Test LV2 Synth", "Bramble Test", Kind::Instrument)]);
    let g = &list[0];
    assert_eq!((g.inputs, g.outputs, g.params, g.latency, g.has_editor, g.category.as_str()), (2, 2, 2, 8, true, "Amplifier"));
    let mut fx = lg_host::open(g, 48000.0).unwrap();
    assert_eq!(fx.params().iter().map(|p| p.name).collect::<Vec<_>>(), ["Gain", "Mode"]);
    assert_eq!(fx.params()[1].choices, ["Clean", "Half", "Mute"]);
    assert_eq!(Effect::latency(&fx), 8);
    let (l, _) = impulse_through(&mut fx, 128);
    assert_eq!(l.iter().position(|x| x.abs() > 0.5), Some(8));
    fx.set(0, -6.0206);
    fx.set(1, 1.0);
    assert_eq!(fx.value_text(1, 2.0).as_deref(), Some("Mute"));
    let (l, _) = impulse_through(&mut fx, 128);
    assert!((l[8] - 0.25).abs() < 1e-3, "{}", l[8]);
    // lilv's state: the control values
    let floats = fx.floats();
    let blob = lg_host::state::decode(&floats).1.unwrap();
    assert!(String::from_utf8_lossy(&blob).contains("gain"));
    let mut fx2 = lg_host::open(g, 48000.0).unwrap();
    fx2.load_floats(&floats);
    assert!((fx2.get(0) + 6.0206).abs() < 1e-3);
    assert_eq!(fx2.get(1), 1.0);
    // the instrument: MIDI through the atom port
    let mut syn = lg_host::open(&list[1], 48000.0).unwrap();
    syn.note_on(69, 100);
    let mut peak = 0f32;
    for _ in 0..20 {
        let (mut l, mut r) = (vec![0f32; 64], vec![0f32; 64]);
        syn.render(&mut l, &mut r);
        peak = peak.max(l.iter().fold(0f32, |a, x| a.max(x.abs())));
    }
    assert!((peak - 0.2).abs() < 0.01, "{peak}");
    syn.note_off(69);
    let (mut l, mut r) = (vec![0f32; 64], vec![0f32; 64]);
    syn.render(&mut l, &mut r);
    let (mut l, mut r2) = (vec![0f32; 64], vec![0f32; 64]);
    syn.render(&mut l, &mut r2);
    assert!(l.iter().all(|x| *x == 0.0));
}

#[test]
fn lv2_x11_ui_embeds() {
    if std::env::var_os("LG_HOST_GUI_TEST").is_none() || !lg_host::editor::x11_available() || !lg_host::lv2::available() {
        eprintln!("LV2 editor check skipped");
        return;
    }
    let d = scratch("lv2-editor");
    let b = lv2_bundle(&d);
    let mut p = lg_host::lv2::Lv2Plugin::open(&b, "urn:bramble:test-lv2-gain", 48000.0).unwrap();
    use lg_host::Backend as _;
    p.open_editor("Test LV2 Gain").unwrap();
    // (the UI made its window on its own X connection: give the server a moment)
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(p.editor_window().unwrap().children().len(), 1);
    assert!(p.idle().changed, "the UI wrote Gain through the host");
    assert_eq!(p.get(4), -12.0);
    p.close_editor();
}
