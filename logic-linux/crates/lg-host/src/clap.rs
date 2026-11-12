//! CLAP plug-ins (clap-sys, the C API). A `.clap` file is a shared library exporting
//! `clap_entry`; its factory makes the plug-ins. Studio is the host: it gives the plug-in a
//! `clap_host` with the extensions Linux plug-ins need (thread-check, log, params, state,
//! latency, gui, timer-support, posix-fd-support, audio-ports, note-ports), activates it at the
//! engine's rate and processes its blocks; main-thread work runs in `idle`.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::ThreadId;

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::log::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::posix_fd_support::*;
use clap_sys::ext::state::*;
use clap_sys::ext::tail::*;
use clap_sys::ext::thread_check::*;
use clap_sys::ext::timer_support::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::clap_process;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::id::clap_id;

use crate::editor::{Fds, HostWindow, Timers, WinEvent};
use crate::{Backend, Ev, Format, Idle, Kind, ParamInfo, PluginInfo, MAX_BLOCK};

/// A loaded `.clap` file (kept loaded for the program's life: unloading plug-in libraries is
/// what crashes hosts).
struct Module {
    _lib: libloading::Library,
    factory: *const clap_plugin_factory,
}
unsafe impl Send for Module {}
unsafe impl Sync for Module {}

fn module(path: &Path) -> Result<&'static Module, String> {
    // (each file once, as it was when loaded: a changed file is loaded again, see load_path)
    static M: OnceLock<Mutex<HashMap<PathBuf, (Option<crate::Stamp>, &'static Module)>>> = OnceLock::new();
    let mut map = M.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let now = crate::stamp(path);
    if let Some((s, m)) = map.get(path) {
        if *s == now {
            return Ok(m);
        }
    }
    let from = crate::load_path(path, map.contains_key(path))?;
    unsafe {
        let lib = libloading::Library::new(&from);
        crate::loaded_from(path, &from);
        let lib = lib.map_err(|e| e.to_string())?;
        let entry: *const clap_plugin_entry = *lib.get::<*const clap_plugin_entry>(b"clap_entry\0").map_err(|_| "not a CLAP plug-in (no clap_entry)".to_string())?;
        if entry.is_null() || (*entry).clap_version.major < 1 {
            return Err("unsupported CLAP version".into());
        }
        let cpath = CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        let init = (*entry).init.ok_or("no init")?;
        if !init(cpath.as_ptr()) {
            return Err("the plug-in's init failed".into());
        }
        let get = (*entry).get_factory.ok_or("no factory")?;
        let factory = get(CLAP_PLUGIN_FACTORY_ID.as_ptr()) as *const clap_plugin_factory;
        if factory.is_null() {
            return Err("no plug-in factory".into());
        }
        let m: &'static Module = Box::leak(Box::new(Module { _lib: lib, factory }));
        map.insert(path.to_path_buf(), (now, m));
        Ok(m)
    }
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

fn chars(b: &[c_char]) -> String {
    let bytes: Vec<u8> = b.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The host side of one plug-in instance: what its callbacks set (read on the main thread).
struct HostData {
    host: clap_host,
    main: ThreadId,
    audio: Mutex<Option<ThreadId>>,
    callback: AtomicBool,
    restart: AtomicBool,
    rescan: AtomicBool,
    latency_changed: AtomicBool,
    dirty: AtomicBool,
    gui_closed: AtomicBool,
    resize: Mutex<Option<(u32, u32)>>,
    timers: Mutex<Timers>,
    fds: Mutex<Fds>,
}

unsafe fn hd<'a>(h: *const clap_host) -> &'a HostData {
    unsafe { &*((*h).host_data as *const HostData) }
}

static NAME: &CStr = c"Bramble Studio";
static VENDOR: &CStr = c"Bramble";
static URL: &CStr = c"https://bramble-apps.github.io";
static VERSION: &CStr = c"0.4";

unsafe extern "C" fn h_get_extension(_h: *const clap_host, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    static THREAD: clap_host_thread_check = clap_host_thread_check { is_main_thread: Some(h_is_main), is_audio_thread: Some(h_is_audio) };
    static LOG: clap_host_log = clap_host_log { log: Some(h_log) };
    static PARAMS: clap_host_params = clap_host_params { rescan: Some(h_params_rescan), clear: Some(h_params_clear), request_flush: Some(h_request_process) };
    static STATE: clap_host_state = clap_host_state { mark_dirty: Some(h_mark_dirty) };
    static LATENCY: clap_host_latency = clap_host_latency { changed: Some(h_latency_changed) };
    static GUI: clap_host_gui = clap_host_gui { resize_hints_changed: Some(h_noop), request_resize: Some(h_request_resize), request_show: Some(h_true), request_hide: Some(h_true), closed: Some(h_gui_closed) };
    static TIMER: clap_host_timer_support = clap_host_timer_support { register_timer: Some(h_register_timer), unregister_timer: Some(h_unregister_timer) };
    static FD: clap_host_posix_fd_support = clap_host_posix_fd_support { register_fd: Some(h_register_fd), modify_fd: Some(h_register_fd), unregister_fd: Some(h_unregister_fd) };
    static PORTS: clap_host_audio_ports = clap_host_audio_ports { is_rescan_flag_supported: Some(h_flag_false), rescan: Some(h_ports_rescan) };
    static NOTES: clap_host_note_ports = clap_host_note_ports { supported_dialects: Some(h_dialects), rescan: Some(h_ports_rescan) };
    let p: *const c_void = if id == CLAP_EXT_THREAD_CHECK {
        &THREAD as *const _ as _
    } else if id == CLAP_EXT_LOG {
        &LOG as *const _ as _
    } else if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as _
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as _
    } else if id == CLAP_EXT_LATENCY {
        &LATENCY as *const _ as _
    } else if id == CLAP_EXT_GUI {
        &GUI as *const _ as _
    } else if id == CLAP_EXT_TIMER_SUPPORT {
        &TIMER as *const _ as _
    } else if id == CLAP_EXT_POSIX_FD_SUPPORT {
        &FD as *const _ as _
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &PORTS as *const _ as _
    } else if id == CLAP_EXT_NOTE_PORTS {
        &NOTES as *const _ as _
    } else {
        std::ptr::null()
    };
    p
}
unsafe extern "C" fn h_request_restart(h: *const clap_host) {
    unsafe { hd(h) }.restart.store(true, Ordering::Release);
}
unsafe extern "C" fn h_request_process(_h: *const clap_host) {}
unsafe extern "C" fn h_request_callback(h: *const clap_host) {
    unsafe { hd(h) }.callback.store(true, Ordering::Release);
}
unsafe extern "C" fn h_is_main(h: *const clap_host) -> bool {
    std::thread::current().id() == unsafe { hd(h) }.main
}
unsafe extern "C" fn h_is_audio(h: *const clap_host) -> bool {
    let d = unsafe { hd(h) };
    let a = *d.audio.lock().unwrap_or_else(|e| e.into_inner());
    a.map_or(std::thread::current().id() != d.main, |a| a == std::thread::current().id())
}
unsafe extern "C" fn h_log(_h: *const clap_host, sev: clap_log_severity, msg: *const c_char) {
    if sev >= CLAP_LOG_WARNING && std::env::var_os("LG_HOST_LOG").is_some() {
        eprintln!("CLAP plug-in: {}", unsafe { cstr(msg) });
    }
}
unsafe extern "C" fn h_params_rescan(h: *const clap_host, _flags: clap_param_rescan_flags) {
    unsafe { hd(h) }.rescan.store(true, Ordering::Release);
}
unsafe extern "C" fn h_params_clear(_h: *const clap_host, _id: clap_id, _flags: clap_param_clear_flags) {}
unsafe extern "C" fn h_mark_dirty(h: *const clap_host) {
    unsafe { hd(h) }.dirty.store(true, Ordering::Release);
}
unsafe extern "C" fn h_latency_changed(h: *const clap_host) {
    unsafe { hd(h) }.latency_changed.store(true, Ordering::Release);
}
unsafe extern "C" fn h_noop(_h: *const clap_host) {}
unsafe extern "C" fn h_true(_h: *const clap_host) -> bool {
    true
}
unsafe extern "C" fn h_flag_false(_h: *const clap_host, _f: u32) -> bool {
    false
}
unsafe extern "C" fn h_ports_rescan(h: *const clap_host, _f: u32) {
    unsafe { hd(h) }.restart.store(true, Ordering::Release);
}
unsafe extern "C" fn h_dialects(_h: *const clap_host) -> clap_note_dialect {
    CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI
}
unsafe extern "C" fn h_request_resize(h: *const clap_host, w: u32, hh: u32) -> bool {
    *unsafe { hd(h) }.resize.lock().unwrap_or_else(|e| e.into_inner()) = Some((w, hh));
    true
}
unsafe extern "C" fn h_gui_closed(h: *const clap_host, _destroyed: bool) {
    unsafe { hd(h) }.gui_closed.store(true, Ordering::Release);
}
unsafe extern "C" fn h_register_timer(h: *const clap_host, ms: u32, id: *mut clap_id) -> bool {
    let t = unsafe { hd(h) }.timers.lock().unwrap_or_else(|e| e.into_inner()).register(ms);
    unsafe { *id = t as clap_id };
    true
}
unsafe extern "C" fn h_unregister_timer(h: *const clap_host, id: clap_id) -> bool {
    unsafe { hd(h) }.timers.lock().unwrap_or_else(|e| e.into_inner()).unregister(id as u64)
}
unsafe extern "C" fn h_register_fd(h: *const clap_host, fd: i32, flags: clap_posix_fd_flags) -> bool {
    unsafe { hd(h) }.fds.lock().unwrap_or_else(|e| e.into_inner()).register(fd, flags, 0);
    true
}
unsafe extern "C" fn h_unregister_fd(h: *const clap_host, fd: i32) -> bool {
    unsafe { hd(h) }.fds.lock().unwrap_or_else(|e| e.into_inner()).unregister(fd, None)
}

fn new_host() -> Box<HostData> {
    let mut d = Box::new(HostData {
        host: clap_host { clap_version: CLAP_VERSION, host_data: std::ptr::null_mut(), name: NAME.as_ptr(), vendor: VENDOR.as_ptr(), url: URL.as_ptr(), version: VERSION.as_ptr(), get_extension: Some(h_get_extension), request_restart: Some(h_request_restart), request_process: Some(h_request_process), request_callback: Some(h_request_callback) },
        main: std::thread::current().id(),
        audio: Mutex::new(None),
        callback: AtomicBool::new(false),
        restart: AtomicBool::new(false),
        rescan: AtomicBool::new(false),
        latency_changed: AtomicBool::new(false),
        dirty: AtomicBool::new(false),
        gui_closed: AtomicBool::new(false),
        resize: Mutex::new(None),
        timers: Mutex::new(Timers::default()),
        fds: Mutex::new(Fds::default()),
    });
    d.host.host_data = &mut *d as *mut HostData as *mut c_void;
    d
}

/// One event for the plug-in's input list.
#[repr(C)]
#[derive(Clone, Copy)]
union EvStore {
    header: clap_event_header,
    note: clap_event_note,
    midi: clap_event_midi,
    param: clap_event_param_value,
    transport: clap_event_transport,
}

unsafe extern "C" fn in_size(list: *const clap_input_events) -> u32 {
    unsafe { (*((*list).ctx as *const Vec<EvStore>)).len() as u32 }
}
unsafe extern "C" fn in_get(list: *const clap_input_events, i: u32) -> *const clap_event_header {
    let v = unsafe { &*((*list).ctx as *const Vec<EvStore>) };
    v.get(i as usize).map_or(std::ptr::null(), |e| e as *const EvStore as *const clap_event_header)
}
unsafe extern "C" fn out_push(list: *const clap_output_events, ev: *const clap_event_header) -> bool {
    let me = unsafe { &mut *((*list).ctx as *mut ClapPlugin) };
    let e = unsafe { &*ev };
    if e.space_id == CLAP_CORE_EVENT_SPACE_ID && e.type_ == CLAP_EVENT_PARAM_VALUE {
        let p = unsafe { &*(ev as *const clap_event_param_value) };
        if let Some(&i) = me.index.get(&p.param_id) {
            me.values[i] = p.value;
            me.host.dirty.store(true, Ordering::Release);
        }
    }
    true
}

fn header(size: usize, ty: u16, time: u32) -> clap_event_header {
    clap_event_header { size: size as u32, time, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: ty, flags: 0 }
}

struct Ports {
    /// Channel count of each port, and the scratch channels for ports Studio doesn't use.
    counts: Vec<u32>,
    scratch: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
    bufs: Vec<clap_audio_buffer>,
}

impl Ports {
    fn new(counts: Vec<u32>) -> Ports {
        let total: usize = counts.iter().map(|c| *c as usize).sum();
        Ports { counts, scratch: (0..total).map(|_| vec![0.0; MAX_BLOCK]).collect(), ptrs: vec![std::ptr::null_mut(); total], bufs: Vec::new() }
    }
}

/// Reads the main audio ports' channel counts.
unsafe fn port_counts(plugin: *const clap_plugin, ext: *const clap_plugin_audio_ports, input: bool) -> Vec<u32> {
    if ext.is_null() {
        return if input { vec![2] } else { vec![2] };
    }
    let e = unsafe { &*ext };
    let (Some(count), Some(get)) = (e.count, e.get) else { return vec![2] };
    let n = unsafe { count(plugin, input) };
    (0..n).map(|i| {
        let mut info: clap_audio_port_info = unsafe { std::mem::zeroed() };
        if unsafe { get(plugin, i, input, &mut info) } { info.channel_count } else { 2 }
    }).collect()
}

pub struct ClapPlugin {
    host: Box<HostData>,
    plugin: *const clap_plugin,
    params: *const clap_plugin_params,
    state: *const clap_plugin_state,
    latency_ext: *const clap_plugin_latency,
    tail_ext: *const clap_plugin_tail,
    gui: *const clap_plugin_gui,
    timer: *const clap_plugin_timer_support,
    fd: *const clap_plugin_posix_fd_support,
    ports_ext: *const clap_plugin_audio_ports,
    infos: Vec<ParamInfo>,
    index: HashMap<u32, usize>,
    cookies: Vec<*mut c_void>,
    values: Vec<f64>,
    pending: Vec<(u32, f64)>,
    events: Vec<EvStore>,
    ins: Ports,
    outs: Ports,
    midi_only: bool,
    active: bool,
    processing: bool,
    latency: u32,
    steady: i64,
    window: Option<HostWindow>,
    gui_created: bool,
    rate: f64,
}

unsafe impl Send for ClapPlugin {}

impl ClapPlugin {
    /// Makes the plug-in `id` from the file `path`, initialised and activated at `rate`.
    pub fn open(path: &Path, id: &str, rate: f64) -> Result<ClapPlugin, String> {
        let m = module(path)?;
        let host = new_host();
        let cid = CString::new(id).map_err(|e| e.to_string())?;
        unsafe {
            let f = &*m.factory;
            let create = f.create_plugin.ok_or("no create_plugin")?;
            let plugin = create(m.factory, &host.host, cid.as_ptr());
            if plugin.is_null() {
                return Err(format!("the factory made no plug-in for {id}"));
            }
            if !((*plugin).init.ok_or("no init")?)(plugin) {
                ((*plugin).destroy.ok_or("no destroy")?)(plugin);
                return Err("the plug-in's init failed".into());
            }
            let ext = |n: &CStr| (*plugin).get_extension.map_or(std::ptr::null(), |g| g(plugin, n.as_ptr()));
            let params = ext(CLAP_EXT_PARAMS) as *const clap_plugin_params;
            let ports_ext = ext(CLAP_EXT_AUDIO_PORTS) as *const clap_plugin_audio_ports;
            let notes = ext(CLAP_EXT_NOTE_PORTS) as *const clap_plugin_note_ports;
            let mut midi_only = false;
            if !notes.is_null() {
                if let (Some(count), Some(get)) = ((*notes).count, (*notes).get) {
                    if count(plugin, true) > 0 {
                        let mut info: clap_note_port_info = std::mem::zeroed();
                        if get(plugin, 0, true, &mut info) {
                            midi_only = info.supported_dialects & CLAP_NOTE_DIALECT_CLAP == 0 && info.supported_dialects & CLAP_NOTE_DIALECT_MIDI != 0;
                        }
                    }
                }
            }
            let ins = Ports::new(if ports_ext.is_null() { vec![2] } else { port_counts(plugin, ports_ext, true) });
            let outs = Ports::new(if ports_ext.is_null() { vec![2] } else { port_counts(plugin, ports_ext, false) });
            let mut me = ClapPlugin {
                host,
                plugin,
                params,
                state: ext(CLAP_EXT_STATE) as _,
                latency_ext: ext(CLAP_EXT_LATENCY) as _,
                tail_ext: ext(CLAP_EXT_TAIL) as _,
                gui: ext(CLAP_EXT_GUI) as _,
                timer: ext(CLAP_EXT_TIMER_SUPPORT) as _,
                fd: ext(CLAP_EXT_POSIX_FD_SUPPORT) as _,
                ports_ext,
                infos: Vec::new(),
                index: HashMap::new(),
                cookies: Vec::new(),
                values: Vec::new(),
                pending: Vec::with_capacity(1024),
                events: Vec::with_capacity(2048),
                ins,
                outs,
                midi_only,
                active: false,
                processing: false,
                latency: 0,
                steady: 0,
                window: None,
                gui_created: false,
                rate,
            };
            me.read_params();
            me.activate()?;
            Ok(me)
        }
    }

    fn activate(&mut self) -> Result<(), String> {
        unsafe {
            let act = (*self.plugin).activate.ok_or("no activate")?;
            if !act(self.plugin, self.rate, 1, MAX_BLOCK as u32) {
                return Err("the plug-in would not activate".into());
            }
        }
        self.active = true;
        self.processing = false;
        self.read_latency();
        Ok(())
    }

    fn deactivate(&mut self) {
        unsafe {
            if self.processing {
                if let Some(f) = (*self.plugin).stop_processing {
                    f(self.plugin);
                }
                self.processing = false;
            }
            if self.active {
                if let Some(f) = (*self.plugin).deactivate {
                    f(self.plugin);
                }
                self.active = false;
            }
        }
    }

    fn read_latency(&mut self) {
        self.latency = if self.latency_ext.is_null() { 0 } else { unsafe { (*self.latency_ext).get.map_or(0, |g| g(self.plugin)) } };
    }

    fn read_params(&mut self) {
        self.infos.clear();
        self.index.clear();
        self.cookies.clear();
        self.values.clear();
        if self.params.is_null() {
            return;
        }
        unsafe {
            let p = &*self.params;
            let (Some(count), Some(get_info)) = (p.count, p.get_info) else { return };
            for i in 0..count(self.plugin) {
                let mut info: clap_param_info = std::mem::zeroed();
                if !get_info(self.plugin, i, &mut info) {
                    continue;
                }
                let stepped = info.flags & (CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM) != 0;
                let steps = if stepped { (info.max_value - info.min_value).round().max(0.0) as u32 } else { 0 };
                let mut choices = Vec::new();
                if stepped && steps <= 64 {
                    for k in 0..=steps {
                        match self.text_of(info.id, info.min_value + k as f64) {
                            Some(t) => choices.push(t),
                            None => {
                                choices.clear();
                                break;
                            }
                        }
                    }
                }
                let mut v = info.default_value;
                if let Some(g) = p.get_value {
                    g(self.plugin, info.id, &mut v);
                }
                self.index.insert(info.id, self.infos.len());
                self.cookies.push(info.cookie);
                self.values.push(v);
                self.infos.push(ParamInfo { native: info.id, name: chars(&info.name), min: info.min_value, max: info.max_value, default: info.default_value, steps, hidden: info.flags & CLAP_PARAM_IS_HIDDEN != 0 || info.flags & CLAP_PARAM_IS_READONLY != 0, choices });
            }
        }
    }

    fn text_of(&self, id: u32, v: f64) -> Option<String> {
        if self.params.is_null() {
            return None;
        }
        unsafe {
            let f = (*self.params).value_to_text?;
            let mut buf = [0 as c_char; 256];
            if f(self.plugin, id, v, buf.as_mut_ptr(), buf.len() as u32) { Some(chars(&buf)) } else { None }
        }
    }

    /// The plug-in's own editor: embedded in a Studio window (X11), else its floating window.
    fn gui_open(&mut self, title: &str) -> Result<(), String> {
        if self.gui.is_null() {
            return Err("the plug-in has no editor".into());
        }
        unsafe {
            let g = &*self.gui;
            let supported = |api: &CStr, floating: bool| g.is_api_supported.is_some_and(|f| f(self.plugin, api.as_ptr(), floating));
            let x11 = crate::editor::x11_available();
            if x11 && supported(CLAP_WINDOW_API_X11, false) {
                if !(g.create.ok_or("no create")?)(self.plugin, CLAP_WINDOW_API_X11.as_ptr(), false) {
                    return Err("the plug-in could not make its editor".into());
                }
                self.gui_created = true;
                if let Some(s) = g.set_scale {
                    s(self.plugin, 1.0);
                }
                let (mut w, mut h) = (400u32, 300u32);
                if let Some(gs) = g.get_size {
                    gs(self.plugin, &mut w, &mut h);
                }
                let resizable = g.can_resize.is_some_and(|f| f(self.plugin));
                let win = HostWindow::new(title, w, h, resizable)?;
                let cw = clap_window { api: CLAP_WINDOW_API_X11.as_ptr(), specific: clap_window_handle { x11: win.id() as _ } };
                if !g.set_parent.is_some_and(|f| f(self.plugin, &cw)) {
                    self.gui_close();
                    return Err("the plug-in's editor would not go in the window".into());
                }
                win.show();
                if let Some(f) = g.show {
                    f(self.plugin);
                }
                self.window = Some(win);
                return Ok(());
            }
            for (api, ok) in [(CLAP_WINDOW_API_X11, x11), (CLAP_WINDOW_API_WAYLAND, std::env::var_os("WAYLAND_DISPLAY").is_some())] {
                if ok && supported(api, true) {
                    if !(g.create.ok_or("no create")?)(self.plugin, api.as_ptr(), true) {
                        continue;
                    }
                    self.gui_created = true;
                    let t = CString::new(title).unwrap_or_default();
                    if let Some(f) = g.suggest_title {
                        f(self.plugin, t.as_ptr());
                    }
                    if let Some(f) = g.show {
                        f(self.plugin);
                    }
                    return Ok(());
                }
            }
            Err(if x11 { "the plug-in's editor needs a window system Studio can't give it".into() } else { "the plug-in's editor needs X11 (XWayland): DISPLAY isn't set".into() })
        }
    }

    fn gui_close(&mut self) {
        if self.gui_created && !self.gui.is_null() {
            unsafe {
                let g = &*self.gui;
                if let Some(f) = g.hide {
                    f(self.plugin);
                }
                if let Some(f) = g.destroy {
                    f(self.plugin);
                }
            }
        }
        self.gui_created = false;
        self.window = None;
    }
}

impl Backend for ClapPlugin {
    fn param_infos(&mut self) -> Vec<ParamInfo> {
        self.infos.clone()
    }
    fn get(&self, native: u32) -> f64 {
        self.index.get(&native).map_or(0.0, |&i| self.values[i])
    }
    fn set(&mut self, native: u32, v: f64) {
        if let Some(&i) = self.index.get(&native) {
            let p = &self.infos[i];
            let v = v.clamp(p.min.min(p.max), p.max.max(p.min));
            self.values[i] = v;
            self.pending.retain(|x| x.0 != native);
            self.pending.push((native, v));
        }
    }
    fn text(&self, native: u32, v: f64) -> Option<String> {
        self.text_of(native, v)
    }
    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2], events: &[Ev], tempo: f64) {
        let n = out[0].len().min(out[1].len()).min(MAX_BLOCK);
        if !self.active || n == 0 {
            return;
        }
        unsafe {
            if !self.processing {
                *self.host.audio.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::thread::current().id());
                self.processing = (*self.plugin).start_processing.is_none_or(|f| f(self.plugin));
                if !self.processing {
                    return;
                }
            }
        }
        // the events: parameter changes, then the notes (the transport goes on its own)
        self.events.clear();
        let mut tr: clap_event_transport = unsafe { std::mem::zeroed() };
        tr.header = header(std::mem::size_of::<clap_event_transport>(), CLAP_EVENT_TRANSPORT, 0);
        tr.flags = CLAP_TRANSPORT_HAS_TEMPO;
        tr.tempo = tempo;
        tr.tsig_num = 4;
        tr.tsig_denom = 4;
        for (id, v) in self.pending.drain(..) {
            let cookie = self.index.get(&id).map_or(std::ptr::null_mut(), |&i| self.cookies[i]);
            self.events.push(EvStore { param: clap_event_param_value { header: header(std::mem::size_of::<clap_event_param_value>(), CLAP_EVENT_PARAM_VALUE, 0), param_id: id, cookie, note_id: -1, port_index: -1, channel: -1, key: -1, value: v } });
        }
        for e in events {
            let note = |ty, key: u8, vel: f64| EvStore { note: clap_event_note { header: header(std::mem::size_of::<clap_event_note>(), ty, 0), note_id: -1, port_index: 0, channel: 0, key: key as i16, velocity: vel } };
            let midi = |d: [u8; 3]| EvStore { midi: clap_event_midi { header: header(std::mem::size_of::<clap_event_midi>(), CLAP_EVENT_MIDI, 0), port_index: 0, data: d } };
            let ev = match (*e, self.midi_only) {
                (Ev::NoteOn { key, velocity }, false) => note(CLAP_EVENT_NOTE_ON, key, velocity as f64 / 127.0),
                (Ev::NoteOff { key }, false) => note(CLAP_EVENT_NOTE_OFF, key, 0.0),
                (Ev::NoteOn { key, velocity }, true) => midi([0x90, key, velocity]),
                (Ev::NoteOff { key }, true) => midi([0x80, key, 0]),
                (Ev::Cc { cc, value }, _) => midi([0xB0, cc, value]),
                (Ev::Bend(b), _) => {
                    let v = (b as i32 + 8192).clamp(0, 16383) as u16;
                    midi([0xE0, (v & 0x7F) as u8, (v >> 7) as u8])
                }
            };
            if self.events.len() < self.events.capacity() {
                self.events.push(ev);
            }
        }
        let in_list = clap_input_events { ctx: &self.events as *const Vec<EvStore> as *mut c_void, size: Some(in_size), get: Some(in_get) };
        let out_list = clap_output_events { ctx: self as *mut ClapPlugin as *mut c_void, try_push: Some(out_push) };
        // the audio: the main ports carry Studio's stereo (a mono input the sum, a mono output
        // copied to both sides); other ports get silent scratch channels. Inputs are copies (a
        // plug-in shouldn't write them, but some do).
        let [ol, or] = out;
        {
            let ports = &mut self.ins;
            ports.bufs.clear();
            let mut k = 0;
            for (pi, &c) in ports.counts.iter().enumerate() {
                let first = k;
                for ch in 0..c as usize {
                    let buf = &mut ports.scratch[k][..n];
                    match (pi, ch, c) {
                        (0, 0, 1) => {
                            for (j, x) in buf.iter_mut().enumerate() {
                                *x = 0.5 * (inp[0][j] + inp[1][j]);
                            }
                        }
                        (0, 0 | 1, _) => buf.copy_from_slice(&inp[ch][..n]),
                        _ => buf.fill(0.0),
                    }
                    ports.ptrs[k] = ports.scratch[k].as_mut_ptr();
                    k += 1;
                }
                ports.bufs.push(clap_audio_buffer { data32: unsafe { ports.ptrs.as_mut_ptr().add(first) }, data64: std::ptr::null_mut(), channel_count: c, latency: 0, constant_mask: 0 });
            }
        }
        {
            let ports = &mut self.outs;
            ports.bufs.clear();
            let mut k = 0;
            for (pi, &c) in ports.counts.iter().enumerate() {
                let first = k;
                for ch in 0..c as usize {
                    ports.ptrs[k] = match (pi, ch) {
                        (0, 0) => ol.as_mut_ptr(),
                        (0, 1) => or.as_mut_ptr(),
                        _ => {
                            ports.scratch[k][..n].fill(0.0);
                            ports.scratch[k].as_mut_ptr()
                        }
                    };
                    k += 1;
                }
                ports.bufs.push(clap_audio_buffer { data32: unsafe { ports.ptrs.as_mut_ptr().add(first) }, data64: std::ptr::null_mut(), channel_count: c, latency: 0, constant_mask: 0 });
            }
        }
        let mono_out = self.outs.counts.first().is_some_and(|c| *c == 1);
        let proc = clap_process {
            steady_time: self.steady,
            frames_count: n as u32,
            transport: &tr,
            audio_inputs: self.ins.bufs.as_ptr(),
            audio_outputs: self.outs.bufs.as_mut_ptr(),
            audio_inputs_count: self.ins.bufs.len() as u32,
            audio_outputs_count: self.outs.bufs.len() as u32,
            in_events: &in_list,
            out_events: &out_list,
        };
        unsafe {
            if let Some(f) = (*self.plugin).process {
                f(self.plugin, &proc);
            }
        }
        if mono_out {
            or[..n].copy_from_slice(&ol[..n]);
        }
        if self.outs.counts.is_empty() {
            ol[..n].fill(0.0);
            or[..n].fill(0.0);
        }
        self.steady += n as i64;
    }
    fn latency(&self) -> u32 {
        self.latency
    }
    fn tail(&self) -> f32 {
        if self.tail_ext.is_null() {
            return 0.0;
        }
        let t = unsafe { (*self.tail_ext).get.map_or(0, |g| g(self.plugin)) };
        if t == u32::MAX { 2.0 } else { (t as f64 / self.rate).min(10.0) as f32 }
    }
    fn reset(&mut self) {
        unsafe {
            if self.active {
                if let Some(f) = (*self.plugin).reset {
                    f(self.plugin);
                }
            }
        }
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        if self.state.is_null() {
            return None;
        }
        unsafe extern "C" fn write(s: *const clap_ostream, buf: *const c_void, size: u64) -> i64 {
            let v = unsafe { &mut *((*s).ctx as *mut Vec<u8>) };
            v.extend_from_slice(unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) });
            size as i64
        }
        let mut out: Vec<u8> = Vec::new();
        let s = clap_ostream { ctx: &mut out as *mut Vec<u8> as *mut c_void, write: Some(write) };
        let ok = unsafe { (*self.state).save.is_some_and(|f| f(self.plugin, &s)) };
        ok.then_some(out)
    }
    fn load_state(&mut self, data: &[u8]) -> bool {
        if self.state.is_null() {
            return false;
        }
        struct Cur<'a>(&'a [u8], usize);
        unsafe extern "C" fn read(s: *const clap_istream, buf: *mut c_void, size: u64) -> i64 {
            let c = unsafe { &mut *((*s).ctx as *mut Cur) };
            let n = (size as usize).min(c.0.len() - c.1);
            unsafe { std::ptr::copy_nonoverlapping(c.0.as_ptr().add(c.1), buf as *mut u8, n) };
            c.1 += n;
            n as i64
        }
        let mut cur = Cur(data, 0);
        let s = clap_istream { ctx: &mut cur as *mut Cur as *mut c_void, read: Some(read) };
        let ok = unsafe { (*self.state).load.is_some_and(|f| f(self.plugin, &s)) };
        if ok {
            // the values now (a loaded state changes them all)
            unsafe {
                if let Some(g) = (!self.params.is_null()).then(|| (*self.params).get_value).flatten() {
                    for (i, p) in self.infos.iter().enumerate() {
                        let mut v = self.values[i];
                        if g(self.plugin, p.native, &mut v) {
                            self.values[i] = v;
                        }
                    }
                }
            }
            self.pending.clear();
        }
        ok
    }
    fn idle(&mut self) -> Idle {
        let mut r = Idle::default();
        unsafe {
            if self.host.callback.swap(false, Ordering::AcqRel) {
                if let Some(f) = (*self.plugin).on_main_thread {
                    f(self.plugin);
                }
            }
            if self.host.restart.swap(false, Ordering::AcqRel) && self.active {
                self.deactivate();
                let _ = self.activate();
            }
            if self.host.latency_changed.swap(false, Ordering::AcqRel) {
                self.read_latency();
            }
            if self.host.rescan.swap(false, Ordering::AcqRel) {
                self.read_params();
                r.params_rescanned = true;
            }
            if self.host.dirty.swap(false, Ordering::AcqRel) {
                r.changed = true;
            }
            let due = self.host.timers.lock().unwrap_or_else(|e| e.into_inner()).due();
            if !self.timer.is_null() {
                if let Some(f) = (*self.timer).on_timer {
                    for id in due {
                        f(self.plugin, id as clap_id);
                    }
                }
            }
            let ready = self.host.fds.lock().unwrap_or_else(|e| e.into_inner()).ready();
            if !self.fd.is_null() {
                if let Some(f) = (*self.fd).on_fd {
                    for (fd, flags, _) in ready {
                        f(self.plugin, fd, flags);
                    }
                }
            }
            if let Some((w, h)) = self.host.resize.lock().unwrap_or_else(|e| e.into_inner()).take() {
                if let Some(win) = self.window.as_mut() {
                    win.resize(w, h);
                }
            }
            let evs = self.window.as_mut().map(|w| w.poll()).unwrap_or_default();
            for e in evs {
                match e {
                    WinEvent::Closed => {
                        self.gui_close();
                        r.editor_closed = true;
                    }
                    WinEvent::Resized(w, h) => {
                        if !self.gui.is_null() {
                            let g = &*self.gui;
                            if g.can_resize.is_some_and(|f| f(self.plugin)) {
                                let (mut w2, mut h2) = (w, h);
                                if let Some(a) = g.adjust_size {
                                    a(self.plugin, &mut w2, &mut h2);
                                }
                                if let Some(s) = g.set_size {
                                    s(self.plugin, w2, h2);
                                }
                            }
                        }
                    }
                }
            }
            if self.host.gui_closed.swap(false, Ordering::AcqRel) && self.gui_created {
                self.gui_close();
                r.editor_closed = true;
            }
        }
        r
    }
    fn has_editor(&self) -> bool {
        !self.gui.is_null()
    }
    fn open_editor(&mut self, title: &str) -> Result<(), String> {
        if self.gui_created {
            return Ok(());
        }
        self.gui_open(title)
    }
    fn close_editor(&mut self) {
        self.gui_close();
    }
    fn editor_open(&self) -> bool {
        self.gui_created
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        self.gui_close();
        self.deactivate();
        unsafe {
            if let Some(f) = (*self.plugin).destroy {
                f(self.plugin);
            }
        }
    }
}

impl ClapPlugin {
    /// The embedded editor's host window (checks).
    pub fn editor_window(&self) -> Option<&HostWindow> {
        self.window.as_ref()
    }
}

/// Every plug-in in a `.clap` file, each made once to read its ports, parameters, latency and
/// editor (the scan, in its own process).
pub fn describe(path: &Path) -> Result<Vec<PluginInfo>, String> {
    let m = module(path)?;
    let mut out = Vec::new();
    unsafe {
        let f = &*m.factory;
        let n = f.get_plugin_count.map_or(0, |c| c(m.factory));
        for i in 0..n {
            let d: *const clap_plugin_descriptor = f.get_plugin_descriptor.map_or(std::ptr::null(), |g| g(m.factory, i));
            if d.is_null() {
                continue;
            }
            let d = &*d;
            let mut feats = Vec::new();
            if !d.features.is_null() {
                let mut p = d.features;
                while !(*p).is_null() {
                    feats.push(cstr(*p));
                    p = p.add(1);
                }
            }
            let id = cstr(d.id);
            let kind = if feats.iter().any(|f| f == "instrument") { Kind::Instrument } else { Kind::Effect };
            let mut info = PluginInfo { format: Format::Clap, id: id.clone(), name: cstr(d.name), vendor: cstr(d.vendor), version: cstr(d.version), kind, category: feats.join(", "), path: path.to_path_buf(), inputs: 0, outputs: 0, params: 0, latency: 0, has_editor: false };
            let p = ClapPlugin::open(path, &id, 48000.0)?;
            info.inputs = p.ins.counts.first().copied().unwrap_or(0);
            info.outputs = p.outs.counts.first().copied().unwrap_or(0);
            info.params = p.infos.iter().filter(|x| !x.hidden).count() as u32;
            info.latency = p.latency;
            info.has_editor = !p.gui.is_null();
            if p.ports_ext.is_null() && kind == Kind::Instrument {
                info.inputs = 0;
            }
            drop(p);
            // (a note effect with no audio out is a MIDI effect: not for the audio slots)
            if info.outputs == 0 {
                continue;
            }
            out.push(info);
        }
    }
    Ok(out)
}
