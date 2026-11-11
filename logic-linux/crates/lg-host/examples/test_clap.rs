//! A tiny CLAP plug-in file for Studio's hosting tests (built by `cargo test`, never installed):
//! two plug-ins in one factory.
//!
//! - "Test Gain" (org.bramble.test-gain), an audio effect: Gain (dB), Mode (Clean / Half /
//!   Mute), a 32-sample latency (the output is delayed by it), state (save / load), an X11
//!   editor that embeds a child window in the host's and registers a timer.
//! - "Test Synth" (org.bramble.test-synth), an instrument: a sine per held note, Level.

#![allow(non_upper_case_globals, clippy::missing_safety_doc)]

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::ext::timer_support::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::stream::*;
use clap_sys::version::CLAP_VERSION;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};

/// How often the editor's timer fired (the tests read it through the plug-in's "Ticks"
/// parameter).
static TICKS: AtomicU32 = AtomicU32::new(0);

#[allow(dead_code)]
struct Ptr<T>(*const T);
unsafe impl<T> Sync for Ptr<T> {}

static GAIN_FEATURES: [Ptr<c_char>; 3] = [Ptr(c"audio-effect".as_ptr()), Ptr(c"utility".as_ptr()), Ptr(std::ptr::null())];
static SYNTH_FEATURES: [Ptr<c_char>; 3] = [Ptr(c"instrument".as_ptr()), Ptr(c"synthesizer".as_ptr()), Ptr(std::ptr::null())];

fn desc(id: &'static CStr, name: &'static CStr, features: &'static [Ptr<c_char>; 3]) -> clap_plugin_descriptor {
    clap_plugin_descriptor { clap_version: CLAP_VERSION, id: id.as_ptr(), name: name.as_ptr(), vendor: c"Bramble Test".as_ptr(), url: c"".as_ptr(), manual_url: c"".as_ptr(), support_url: c"".as_ptr(), version: c"1.0.0".as_ptr(), description: c"Studio's hosting test".as_ptr(), features: features.as_ptr() as *const *const c_char }
}

struct Plug {
    plugin: clap_plugin,
    host: *const clap_host,
    synth: bool,
    rate: f64,
    gain_db: f64,
    mode: f64,
    level: f64,
    ticks_param: f64,
    /// The latency delay line (effect).
    line: Vec<[f32; 2]>,
    at: usize,
    /// Held notes: key, phase (synth).
    notes: Vec<(i16, f64)>,
    gui: Option<(x11rb::rust_connection::RustConnection, u32)>,
    timer: Option<clap_id>,
}

const LATENCY: usize = 32;

unsafe fn me<'a>(p: *const clap_plugin) -> &'a mut Plug {
    unsafe { &mut *((*p).plugin_data as *mut Plug) }
}

unsafe extern "C" fn init(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    drop(unsafe { Box::from_raw((*p).plugin_data as *mut Plug) });
}
unsafe extern "C" fn activate(p: *const clap_plugin, rate: f64, _min: u32, _max: u32) -> bool {
    let m = unsafe { me(p) };
    m.rate = rate;
    m.line = vec![[0.0; 2]; LATENCY];
    true
}
unsafe extern "C" fn deactivate(_p: *const clap_plugin) {}
unsafe extern "C" fn start(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn stop(_p: *const clap_plugin) {}
unsafe extern "C" fn reset(p: *const clap_plugin) {
    let m = unsafe { me(p) };
    m.line.iter_mut().for_each(|x| *x = [0.0; 2]);
    m.notes.clear();
}

unsafe extern "C" fn process(p: *const clap_plugin, pr: *const clap_process) -> clap_process_status {
    let m = unsafe { me(p) };
    let pr = unsafe { &*pr };
    let n = pr.frames_count as usize;
    // the events: parameter values and notes
    let ev = unsafe { &*pr.in_events };
    let count = unsafe { (ev.size.unwrap())(ev) };
    for i in 0..count {
        let h = unsafe { &*(ev.get.unwrap())(ev, i) };
        if h.space_id != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        match h.type_ {
            CLAP_EVENT_PARAM_VALUE => {
                let e = unsafe { &*(h as *const _ as *const clap_event_param_value) };
                match e.param_id {
                    0 => m.gain_db = e.value,
                    1 => m.mode = e.value,
                    2 => m.level = e.value,
                    _ => {}
                }
            }
            CLAP_EVENT_NOTE_ON => {
                let e = unsafe { &*(h as *const _ as *const clap_event_note) };
                m.notes.push((e.key, 0.0));
            }
            CLAP_EVENT_NOTE_OFF => {
                let e = unsafe { &*(h as *const _ as *const clap_event_note) };
                m.notes.retain(|x| x.0 != e.key);
            }
            _ => {}
        }
    }
    let out = unsafe { &*pr.audio_outputs };
    let (ol, or) = unsafe { (std::slice::from_raw_parts_mut(*out.data32, n), std::slice::from_raw_parts_mut(*out.data32.add(1), n)) };
    if m.synth {
        ol.fill(0.0);
        or.fill(0.0);
        for (key, ph) in &mut m.notes {
            let f = 440.0 * 2f64.powf((*key as f64 - 69.0) / 12.0);
            for k in 0..n {
                let s = (*ph * std::f64::consts::TAU).sin() * m.level * 0.25;
                ol[k] += s as f32;
                or[k] += s as f32;
                *ph = (*ph + f / m.rate).fract();
            }
        }
    } else {
        let inp = unsafe { &*pr.audio_inputs };
        let (il, ir) = unsafe { (std::slice::from_raw_parts(*inp.data32, n), std::slice::from_raw_parts(*inp.data32.add(1), n)) };
        let g = match m.mode.round() as i32 {
            1 => 0.5,
            2 => 0.0,
            _ => 1.0,
        } * 10f64.powf(m.gain_db / 20.0);
        for k in 0..n {
            let d = m.line[m.at];
            m.line[m.at] = [il[k], ir[k]];
            m.at = (m.at + 1) % LATENCY;
            ol[k] = d[0] * g as f32;
            or[k] = d[1] * g as f32;
        }
    }
    // the timer count, sent back as a parameter change (shows output events reach the host)
    let t = TICKS.load(Ordering::Relaxed) as f64;
    if t != m.ticks_param {
        m.ticks_param = t;
        let e = clap_event_param_value { header: clap_event_header { size: std::mem::size_of::<clap_event_param_value>() as u32, time: 0, space_id: 0, type_: CLAP_EVENT_PARAM_VALUE, flags: 0 }, param_id: 3, cookie: std::ptr::null_mut(), note_id: -1, port_index: -1, channel: -1, key: -1, value: t };
        let o = unsafe { &*pr.out_events };
        unsafe { (o.try_push.unwrap())(o, &e.header) };
    }
    CLAP_PROCESS_CONTINUE
}

// --- params
unsafe extern "C" fn p_count(p: *const clap_plugin) -> u32 {
    if unsafe { me(p) }.synth { 1 } else { 3 }
}
unsafe extern "C" fn p_info(p: *const clap_plugin, i: u32, info: *mut clap_param_info) -> bool {
    let m = unsafe { me(p) };
    let (id, name, min, max, def, flags): (u32, &[u8], f64, f64, f64, u32) = match (m.synth, i) {
        (false, 0) => (0, b"Gain", -60.0, 12.0, 0.0, CLAP_PARAM_IS_AUTOMATABLE),
        (false, 1) => (1, b"Mode", 0.0, 2.0, 0.0, CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM),
        (false, 2) => (3, b"Ticks", 0.0, 1e9, 0.0, 0),
        (true, 0) => (2, b"Level", 0.0, 1.0, 0.8, CLAP_PARAM_IS_AUTOMATABLE),
        _ => return false,
    };
    let info = unsafe { &mut *info };
    *info = unsafe { std::mem::zeroed() };
    info.id = id;
    info.flags = flags;
    for (k, b) in name.iter().enumerate() {
        info.name[k] = *b as c_char;
    }
    info.min_value = min;
    info.max_value = max;
    info.default_value = def;
    true
}
unsafe extern "C" fn p_value(p: *const clap_plugin, id: clap_id, v: *mut f64) -> bool {
    let m = unsafe { me(p) };
    let x = match id {
        0 => m.gain_db,
        1 => m.mode,
        2 => m.level,
        3 => m.ticks_param,
        _ => return false,
    };
    unsafe { *v = x };
    true
}
unsafe extern "C" fn p_text(_p: *const clap_plugin, id: clap_id, v: f64, buf: *mut c_char, cap: u32) -> bool {
    let s = match id {
        0 => format!("{v:.1} dB"),
        1 => ["Clean", "Half", "Mute"].get(v.round() as usize).unwrap_or(&"?").to_string(),
        2 => format!("{:.0} %", v * 100.0),
        _ => format!("{v:.0}"),
    };
    let b = s.as_bytes();
    let n = b.len().min(cap as usize - 1);
    for k in 0..n {
        unsafe { *buf.add(k) = b[k] as c_char };
    }
    unsafe { *buf.add(n) = 0 };
    true
}
unsafe extern "C" fn p_from_text(_p: *const clap_plugin, _id: clap_id, _t: *const c_char, _v: *mut f64) -> bool {
    false
}
unsafe extern "C" fn p_flush(p: *const clap_plugin, ev: *const clap_input_events, _o: *const clap_output_events) {
    let m = unsafe { me(p) };
    let ev = unsafe { &*ev };
    for i in 0..unsafe { (ev.size.unwrap())(ev) } {
        let h = unsafe { &*(ev.get.unwrap())(ev, i) };
        if h.type_ == CLAP_EVENT_PARAM_VALUE {
            let e = unsafe { &*(h as *const _ as *const clap_event_param_value) };
            match e.param_id {
                0 => m.gain_db = e.value,
                1 => m.mode = e.value,
                2 => m.level = e.value,
                _ => {}
            }
        }
    }
}
static PARAMS: clap_plugin_params = clap_plugin_params { count: Some(p_count), get_info: Some(p_info), get_value: Some(p_value), value_to_text: Some(p_text), text_to_value: Some(p_from_text), flush: Some(p_flush) };

// --- state: "gain mode level" as text
unsafe extern "C" fn s_save(p: *const clap_plugin, s: *const clap_ostream) -> bool {
    let m = unsafe { me(p) };
    let t = format!("bramble-test {} {} {}", m.gain_db, m.mode, m.level);
    let s = unsafe { &*s };
    unsafe { (s.write.unwrap())(s, t.as_ptr() as *const c_void, t.len() as u64) == t.len() as i64 }
}
unsafe extern "C" fn s_load(p: *const clap_plugin, s: *const clap_istream) -> bool {
    let m = unsafe { me(p) };
    let s = unsafe { &*s };
    let mut buf = vec![0u8; 256];
    let mut all = Vec::new();
    loop {
        let n = unsafe { (s.read.unwrap())(s, buf.as_mut_ptr() as *mut c_void, buf.len() as u64) };
        if n <= 0 {
            break;
        }
        all.extend_from_slice(&buf[..n as usize]);
    }
    let t = String::from_utf8_lossy(&all);
    let w: Vec<&str> = t.split_whitespace().collect();
    if w.len() != 4 || w[0] != "bramble-test" {
        return false;
    }
    m.gain_db = w[1].parse().unwrap_or(0.0);
    m.mode = w[2].parse().unwrap_or(0.0);
    m.level = w[3].parse().unwrap_or(0.8);
    true
}
static STATE: clap_plugin_state = clap_plugin_state { save: Some(s_save), load: Some(s_load) };

// --- latency
unsafe extern "C" fn l_get(p: *const clap_plugin) -> u32 {
    if unsafe { me(p) }.synth { 0 } else { LATENCY as u32 }
}
static LAT: clap_plugin_latency = clap_plugin_latency { get: Some(l_get) };

// --- ports
unsafe extern "C" fn a_count(p: *const clap_plugin, input: bool) -> u32 {
    if input && unsafe { me(p) }.synth { 0 } else { 1 }
}
unsafe extern "C" fn a_get(_p: *const clap_plugin, i: u32, input: bool, info: *mut clap_audio_port_info) -> bool {
    if i != 0 {
        return false;
    }
    let info = unsafe { &mut *info };
    *info = unsafe { std::mem::zeroed() };
    info.id = if input { 0 } else { 1 };
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = u32::MAX;
    true
}
static PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports { count: Some(a_count), get: Some(a_get) };
unsafe extern "C" fn n_count(p: *const clap_plugin, input: bool) -> u32 {
    if input && unsafe { me(p) }.synth { 1 } else { 0 }
}
unsafe extern "C" fn n_get(_p: *const clap_plugin, i: u32, input: bool, info: *mut clap_note_port_info) -> bool {
    if i != 0 || !input {
        return false;
    }
    let info = unsafe { &mut *info };
    *info = unsafe { std::mem::zeroed() };
    info.supported_dialects = CLAP_NOTE_DIALECT_CLAP;
    info.preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
    true
}
static NOTES: clap_plugin_note_ports = clap_plugin_note_ports { count: Some(n_count), get: Some(n_get) };

// --- gui: an X11 child window inside the host's
unsafe extern "C" fn g_supported(_p: *const clap_plugin, api: *const c_char, floating: bool) -> bool {
    !floating && unsafe { CStr::from_ptr(api) } == CLAP_WINDOW_API_X11
}
unsafe extern "C" fn g_preferred(_p: *const clap_plugin, api: *mut *const c_char, floating: *mut bool) -> bool {
    unsafe {
        *api = CLAP_WINDOW_API_X11.as_ptr();
        *floating = false;
    }
    true
}
unsafe extern "C" fn g_create(_p: *const clap_plugin, _api: *const c_char, floating: bool) -> bool {
    !floating
}
unsafe extern "C" fn g_destroy(p: *const clap_plugin) {
    let m = unsafe { me(p) };
    if let Some((c, w)) = m.gui.take() {
        let _ = c.destroy_window(w);
        let _ = c.flush();
    }
    if let Some(t) = m.timer.take() {
        if let Some(ts) = host_ext::<clap_host_timer_support>(m.host, CLAP_EXT_TIMER_SUPPORT) {
            unsafe { (ts.unregister_timer.unwrap())(m.host, t) };
        }
    }
}
unsafe extern "C" fn g_scale(_p: *const clap_plugin, _s: f64) -> bool {
    false
}
unsafe extern "C" fn g_size(_p: *const clap_plugin, w: *mut u32, h: *mut u32) -> bool {
    unsafe {
        *w = 320;
        *h = 200;
    }
    true
}
unsafe extern "C" fn g_can_resize(_p: *const clap_plugin) -> bool {
    false
}
unsafe extern "C" fn g_hints(_p: *const clap_plugin, _h: *mut clap_gui_resize_hints) -> bool {
    false
}
unsafe extern "C" fn g_adjust(_p: *const clap_plugin, _w: *mut u32, _h: *mut u32) -> bool {
    false
}
unsafe extern "C" fn g_set_size(_p: *const clap_plugin, _w: u32, _h: u32) -> bool {
    false
}
fn host_ext<T>(host: *const clap_host, id: &CStr) -> Option<&'static T> {
    let h = unsafe { &*host };
    let p = unsafe { (h.get_extension?)(host, id.as_ptr()) } as *const T;
    (!p.is_null()).then(|| unsafe { &*p })
}
unsafe extern "C" fn g_parent(p: *const clap_plugin, w: *const clap_window) -> bool {
    let m = unsafe { me(p) };
    let parent = unsafe { (*w).specific.x11 } as u32;
    let Ok((c, _)) = x11rb::connect(None) else { return false };
    let Ok(id) = c.generate_id() else { return false };
    if c.create_window(x11rb::COPY_DEPTH_FROM_PARENT, id, parent, 0, 0, 320, 200, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new().background_pixel(0x3366cc)).is_err() {
        return false;
    }
    let _ = c.map_window(id);
    let _ = c.flush();
    m.gui = Some((c, id));
    if let Some(ts) = host_ext::<clap_host_timer_support>(m.host, CLAP_EXT_TIMER_SUPPORT) {
        let mut t = 0;
        if unsafe { (ts.register_timer.unwrap())(m.host, 10, &mut t) } {
            m.timer = Some(t);
        }
    }
    true
}
unsafe extern "C" fn g_transient(_p: *const clap_plugin, _w: *const clap_window) -> bool {
    false
}
unsafe extern "C" fn g_title(_p: *const clap_plugin, _t: *const c_char) {}
unsafe extern "C" fn g_show(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn g_hide(_p: *const clap_plugin) -> bool {
    true
}
static GUI: clap_plugin_gui = clap_plugin_gui { is_api_supported: Some(g_supported), get_preferred_api: Some(g_preferred), create: Some(g_create), destroy: Some(g_destroy), set_scale: Some(g_scale), get_size: Some(g_size), can_resize: Some(g_can_resize), get_resize_hints: Some(g_hints), adjust_size: Some(g_adjust), set_size: Some(g_set_size), set_parent: Some(g_parent), set_transient: Some(g_transient), suggest_title: Some(g_title), show: Some(g_show), hide: Some(g_hide) };
unsafe extern "C" fn t_on_timer(_p: *const clap_plugin, _id: clap_id) {
    TICKS.fetch_add(1, Ordering::Relaxed);
}
static TIMER: clap_plugin_timer_support = clap_plugin_timer_support { on_timer: Some(t_on_timer) };

unsafe extern "C" fn get_extension(p: *const clap_plugin, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    let synth = unsafe { me(p) }.synth;
    if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as _
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as _
    } else if id == CLAP_EXT_LATENCY {
        &LAT as *const _ as _
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &PORTS as *const _ as _
    } else if id == CLAP_EXT_NOTE_PORTS {
        &NOTES as *const _ as _
    } else if id == CLAP_EXT_GUI && !synth {
        &GUI as *const _ as _
    } else if id == CLAP_EXT_TIMER_SUPPORT && !synth {
        &TIMER as *const _ as _
    } else {
        std::ptr::null()
    }
}
unsafe extern "C" fn on_main(_p: *const clap_plugin) {}

struct Descs {
    gain: clap_plugin_descriptor,
    synth: clap_plugin_descriptor,
}
unsafe impl Sync for Descs {}
static DESCS: std::sync::OnceLock<Descs> = std::sync::OnceLock::new();
fn descs() -> &'static Descs {
    DESCS.get_or_init(|| Descs { gain: desc(c"org.bramble.test-gain", c"Test Gain", &GAIN_FEATURES), synth: desc(c"org.bramble.test-synth", c"Test Synth", &SYNTH_FEATURES) })
}

unsafe extern "C" fn f_count(_f: *const clap_plugin_factory) -> u32 {
    2
}
unsafe extern "C" fn f_desc(_f: *const clap_plugin_factory, i: u32) -> *const clap_plugin_descriptor {
    match i {
        0 => &descs().gain,
        1 => &descs().synth,
        _ => std::ptr::null(),
    }
}
unsafe extern "C" fn f_create(_f: *const clap_plugin_factory, host: *const clap_host, id: *const c_char) -> *const clap_plugin {
    let id = unsafe { CStr::from_ptr(id) };
    let synth = match id.to_bytes() {
        b"org.bramble.test-gain" => false,
        b"org.bramble.test-synth" => true,
        _ => return std::ptr::null(),
    };
    let d: *const clap_plugin_descriptor = if synth { &descs().synth } else { &descs().gain };
    let b = Box::into_raw(Box::new(Plug {
        plugin: clap_plugin { desc: d, plugin_data: std::ptr::null_mut(), init: Some(init), destroy: Some(destroy), activate: Some(activate), deactivate: Some(deactivate), start_processing: Some(start), stop_processing: Some(stop), reset: Some(reset), process: Some(process), get_extension: Some(get_extension), on_main_thread: Some(on_main) },
        host,
        synth,
        rate: 48000.0,
        gain_db: 0.0,
        mode: 0.0,
        level: 0.8,
        ticks_param: 0.0,
        line: vec![[0.0; 2]; LATENCY],
        at: 0,
        notes: Vec::new(),
        gui: None,
        timer: None,
    }));
    unsafe {
        (*b).plugin.plugin_data = b as *mut c_void;
        &(*b).plugin
    }
}
static FACTORY: clap_plugin_factory = clap_plugin_factory { get_plugin_count: Some(f_count), get_plugin_descriptor: Some(f_desc), create_plugin: Some(f_create) };

unsafe extern "C" fn e_init(path: *const c_char) -> bool {
    // a copy named "…crash…" takes the process down (the scanner's crash test)
    if unsafe { CStr::from_ptr(path) }.to_string_lossy().contains("crash") {
        std::process::abort();
    }
    true
}
unsafe extern "C" fn e_deinit() {}
unsafe extern "C" fn e_factory(id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID { &FACTORY as *const _ as _ } else { std::ptr::null() }
}

#[unsafe(no_mangle)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry { clap_version: CLAP_VERSION, init: Some(e_init), deinit: Some(e_deinit), get_factory: Some(e_factory) };
