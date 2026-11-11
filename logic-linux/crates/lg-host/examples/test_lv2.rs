//! A tiny LV2 plug-in library for Studio's hosting tests (built by `cargo test`, never
//! installed; the test writes the bundle's Turtle next to a copy of it):
//!
//! - urn:bramble:test-lv2-gain, an effect: Gain (dB), Mode (an enumeration: Clean / Half /
//!   Mute), a reported latency of 8 samples (the output is delayed by it), and an X11 UI in the
//!   same library (a child window in the host's; on its first idle it sets Gain to -12 dB
//!   through the host's write function).
//! - urn:bramble:test-lv2-synth, an instrument: MIDI in (an atom sequence), a sine per note,
//!   Level.

#![allow(clippy::missing_safety_doc)]

use std::ffi::{CStr, c_char, c_void};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};

type P = *mut c_void;

#[repr(C)]
pub struct Descriptor {
    uri: *const c_char,
    instantiate: unsafe extern "C" fn(*const Descriptor, f64, *const c_char, *const *const Feature) -> P,
    connect_port: unsafe extern "C" fn(P, u32, P),
    activate: Option<unsafe extern "C" fn(P)>,
    run: unsafe extern "C" fn(P, u32),
    deactivate: Option<unsafe extern "C" fn(P)>,
    cleanup: unsafe extern "C" fn(P),
    extension_data: Option<unsafe extern "C" fn(*const c_char) -> *const c_void>,
}
unsafe impl Sync for Descriptor {}

#[repr(C)]
pub struct Feature {
    uri: *const c_char,
    data: P,
}

#[repr(C)]
struct UridMap {
    handle: P,
    map: unsafe extern "C" fn(P, *const c_char) -> u32,
}

const LATENCY: usize = 8;

struct Plug {
    synth: bool,
    rate: f64,
    ports: [*mut f32; 8],
    line: Vec<[f32; 2]>,
    at: usize,
    notes: Vec<(u8, f64)>,
    midi_urid: u32,
}

unsafe extern "C" fn instantiate(d: *const Descriptor, rate: f64, _b: *const c_char, features: *const *const Feature) -> P {
    let synth = unsafe { CStr::from_ptr((*d).uri) }.to_bytes().ends_with(b"synth");
    let mut midi_urid = 0;
    let mut f = features;
    while !f.is_null() && !unsafe { *f }.is_null() {
        let ft = unsafe { &**f };
        if unsafe { CStr::from_ptr(ft.uri) }.to_bytes() == b"http://lv2plug.in/ns/ext/urid#map" {
            let m = unsafe { &*(ft.data as *const UridMap) };
            midi_urid = unsafe { (m.map)(m.handle, c"http://lv2plug.in/ns/ext/midi#MidiEvent".as_ptr()) };
        }
        f = unsafe { f.add(1) };
    }
    if synth && midi_urid == 0 {
        // (urid:map is required)
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Plug { synth, rate, ports: [std::ptr::null_mut(); 8], line: vec![[0.0; 2]; LATENCY], at: 0, notes: Vec::new(), midi_urid })) as P
}
unsafe extern "C" fn connect(h: P, port: u32, data: P) {
    let p = unsafe { &mut *(h as *mut Plug) };
    if (port as usize) < 8 {
        p.ports[port as usize] = data as *mut f32;
    }
}
unsafe extern "C" fn cleanup(h: P) {
    drop(unsafe { Box::from_raw(h as *mut Plug) });
}
unsafe extern "C" fn run(h: P, n: u32) {
    let p = unsafe { &mut *(h as *mut Plug) };
    let n = n as usize;
    if p.synth {
        // port 0: MIDI sequence in; 1, 2: out; 3: level
        let seq = p.ports[0] as *const u8;
        let size = unsafe { *(seq as *const u32) } as usize;
        let mut at = 16;
        while at + 16 <= size + 8 {
            let ev_size = unsafe { *(seq.add(at + 8) as *const u32) } as usize;
            let ev_type = unsafe { *(seq.add(at + 12) as *const u32) };
            if ev_type == p.midi_urid && ev_size >= 3 {
                let m = unsafe { std::slice::from_raw_parts(seq.add(at + 16), 3) };
                match m[0] & 0xF0 {
                    0x90 if m[2] > 0 => p.notes.push((m[1], 0.0)),
                    0x80 | 0x90 => p.notes.retain(|x| x.0 != m[1]),
                    _ => {}
                }
            }
            at += 16 + ev_size.div_ceil(8) * 8;
        }
        let level = unsafe { *p.ports[3] } as f64;
        let (l, r) = unsafe { (std::slice::from_raw_parts_mut(p.ports[1], n), std::slice::from_raw_parts_mut(p.ports[2], n)) };
        l.fill(0.0);
        r.fill(0.0);
        for (key, ph) in &mut p.notes {
            let f = 440.0 * 2f64.powf((*key as f64 - 69.0) / 12.0);
            for k in 0..n {
                let s = ((*ph * std::f64::consts::TAU).sin() * level * 0.25) as f32;
                l[k] += s;
                r[k] += s;
                *ph = (*ph + f / p.rate).fract();
            }
        }
    } else {
        // 0, 1: in; 2, 3: out; 4 gain dB; 5 mode; 6 latency (out)
        let g = 10f32.powf(unsafe { *p.ports[4] } / 20.0) * match unsafe { *p.ports[5] }.round() as i32 {
            1 => 0.5,
            2 => 0.0,
            _ => 1.0,
        };
        unsafe { *p.ports[6] = LATENCY as f32 };
        for k in 0..n {
            let x = unsafe { [*p.ports[0].add(k), *p.ports[1].add(k)] };
            let y = p.line[p.at];
            p.line[p.at] = x;
            p.at = (p.at + 1) % LATENCY;
            unsafe {
                *p.ports[2].add(k) = y[0] * g;
                *p.ports[3].add(k) = y[1] * g;
            }
        }
    }
}

static GAIN: Descriptor = Descriptor { uri: c"urn:bramble:test-lv2-gain".as_ptr(), instantiate, connect_port: connect, activate: None, run, deactivate: None, cleanup, extension_data: None };
static SYNTH: Descriptor = Descriptor { uri: c"urn:bramble:test-lv2-synth".as_ptr(), instantiate, connect_port: connect, activate: None, run, deactivate: None, cleanup, extension_data: None };

#[unsafe(no_mangle)]
pub extern "C" fn lv2_descriptor(i: u32) -> *const Descriptor {
    match i {
        0 => &GAIN,
        1 => &SYNTH,
        _ => std::ptr::null(),
    }
}

// --- the X11 UI

type WriteFn = unsafe extern "C" fn(P, u32, u32, u32, *const c_void);

#[repr(C)]
pub struct UiDescriptor {
    uri: *const c_char,
    instantiate: unsafe extern "C" fn(*const UiDescriptor, *const c_char, *const c_char, WriteFn, P, *mut P, *const *const Feature) -> P,
    cleanup: unsafe extern "C" fn(P),
    port_event: Option<unsafe extern "C" fn(P, u32, u32, u32, *const c_void)>,
    extension_data: Option<unsafe extern "C" fn(*const c_char) -> *const c_void>,
}
unsafe impl Sync for UiDescriptor {}

struct Ui {
    write: WriteFn,
    ctl: P,
    conn: x11rb::rust_connection::RustConnection,
    win: u32,
    idles: u32,
    /// The last Gain the host sent (port_event).
    gain: f32,
}

unsafe extern "C" fn ui_instantiate(_d: *const UiDescriptor, _uri: *const c_char, _b: *const c_char, write: WriteFn, ctl: P, widget: *mut P, features: *const *const Feature) -> P {
    let mut parent = 0u32;
    let mut f = features;
    while !f.is_null() && !unsafe { *f }.is_null() {
        let ft = unsafe { &**f };
        if unsafe { CStr::from_ptr(ft.uri) }.to_bytes() == b"http://lv2plug.in/ns/extensions/ui#parent" {
            parent = ft.data as usize as u32;
        }
        f = unsafe { f.add(1) };
    }
    let Ok((conn, _)) = x11rb::connect(None) else { return std::ptr::null_mut() };
    let Ok(win) = conn.generate_id() else { return std::ptr::null_mut() };
    if conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, win, parent, 0, 0, 200, 100, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new().background_pixel(0x33aa66)).is_err() {
        return std::ptr::null_mut();
    }
    let _ = conn.map_window(win);
    let _ = conn.flush();
    unsafe { *widget = win as usize as P };
    Box::into_raw(Box::new(Ui { write, ctl, conn, win, idles: 0, gain: f32::NAN })) as P
}
unsafe extern "C" fn ui_cleanup(h: P) {
    let u = unsafe { Box::from_raw(h as *mut Ui) };
    let _ = u.conn.destroy_window(u.win);
    let _ = u.conn.flush();
}
unsafe extern "C" fn ui_port_event(h: P, port: u32, size: u32, format: u32, buf: *const c_void) {
    let u = unsafe { &mut *(h as *mut Ui) };
    if port == 4 && size == 4 && format == 0 {
        u.gain = unsafe { *(buf as *const f32) };
    }
}
#[repr(C)]
struct Idle {
    idle: unsafe extern "C" fn(P) -> i32,
}
unsafe extern "C" fn ui_idle(h: P) -> i32 {
    let u = unsafe { &mut *(h as *mut Ui) };
    u.idles += 1;
    if u.idles == 1 && u.gain == 0.0 {
        let v: f32 = -12.0;
        unsafe { (u.write)(u.ctl, 4, 4, 0, &v as *const f32 as *const c_void) };
    }
    0
}
static IDLE: Idle = Idle { idle: ui_idle };
unsafe extern "C" fn ui_ext(uri: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(uri) }.to_bytes() == b"http://lv2plug.in/ns/extensions/ui#idleInterface" { &IDLE as *const Idle as *const c_void } else { std::ptr::null() }
}

static UI: UiDescriptor = UiDescriptor { uri: c"urn:bramble:test-lv2-gain#ui".as_ptr(), instantiate: ui_instantiate, cleanup: ui_cleanup, port_event: Some(ui_port_event), extension_data: Some(ui_ext) };

#[unsafe(no_mangle)]
pub extern "C" fn lv2ui_descriptor(i: u32) -> *const UiDescriptor {
    if i == 0 { &UI } else { std::ptr::null() }
}
