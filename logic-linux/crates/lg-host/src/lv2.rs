//! LV2 plug-ins, through the system's lilv (liblilv-0.so.0, loaded at run time: nothing to
//! build against, and when it isn't installed LV2 is simply not offered). lilv reads the
//! bundle's Turtle and instantiates the plug-in; Studio connects the ports itself: audio in /
//! out, control ports (the parameters, by port index), an atom port for MIDI (instruments), the
//! latency port. State goes through lilv's state API (the control values and the plug-in's
//! own state extension). Editors: X11 UIs (ui:X11UI) are embedded directly, without suil.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::editor::{HostWindow, WinEvent};
use crate::{Backend, Ev, Format, Idle, Kind, ParamInfo, PluginInfo, MAX_BLOCK};

type P = *mut c_void;
type CP = *const c_void;

/// The lilv functions Studio uses.
#[allow(clippy::type_complexity)]
struct Lilv {
    _lib: libloading::Library,
    world_new: unsafe extern "C" fn() -> P,
    world_free: unsafe extern "C" fn(P),
    world_load_bundle: unsafe extern "C" fn(P, CP),
    world_get_all_plugins: unsafe extern "C" fn(P) -> CP,
    plugins_begin: unsafe extern "C" fn(CP) -> P,
    plugins_get: unsafe extern "C" fn(CP, P) -> CP,
    plugins_next: unsafe extern "C" fn(CP, P) -> P,
    plugins_is_end: unsafe extern "C" fn(CP, P) -> bool,
    plugins_get_by_uri: unsafe extern "C" fn(CP, CP) -> CP,
    plugin_get_uri: unsafe extern "C" fn(CP) -> CP,
    plugin_get_name: unsafe extern "C" fn(CP) -> P,
    plugin_get_author_name: unsafe extern "C" fn(CP) -> P,
    plugin_get_value: unsafe extern "C" fn(CP, CP) -> P,
    plugin_get_num_ports: unsafe extern "C" fn(CP) -> u32,
    plugin_get_port_by_index: unsafe extern "C" fn(CP, u32) -> CP,
    plugin_instantiate: unsafe extern "C" fn(CP, f64, *const *const Feature) -> *mut Instance,
    plugin_get_uis: unsafe extern "C" fn(CP) -> P,
    instance_free: unsafe extern "C" fn(*mut Instance),
    port_is_a: unsafe extern "C" fn(CP, CP, CP) -> bool,
    port_has_property: unsafe extern "C" fn(CP, CP, CP) -> bool,
    port_supports_event: unsafe extern "C" fn(CP, CP, CP) -> bool,
    port_get_name: unsafe extern "C" fn(CP, CP) -> P,
    port_get_symbol: unsafe extern "C" fn(CP, CP) -> CP,
    port_get_range: unsafe extern "C" fn(CP, CP, *mut P, *mut P, *mut P),
    port_get_scale_points: unsafe extern "C" fn(CP, CP) -> P,
    scale_points_free: unsafe extern "C" fn(P),
    scale_points_begin: unsafe extern "C" fn(CP) -> P,
    scale_points_get: unsafe extern "C" fn(CP, P) -> CP,
    scale_points_next: unsafe extern "C" fn(CP, P) -> P,
    scale_points_is_end: unsafe extern "C" fn(CP, P) -> bool,
    scale_point_get_label: unsafe extern "C" fn(CP) -> CP,
    scale_point_get_value: unsafe extern "C" fn(CP) -> CP,
    nodes_begin: unsafe extern "C" fn(CP) -> P,
    nodes_get: unsafe extern "C" fn(CP, P) -> CP,
    nodes_next: unsafe extern "C" fn(CP, P) -> P,
    nodes_is_end: unsafe extern "C" fn(CP, P) -> bool,
    nodes_free: unsafe extern "C" fn(P),
    uis_begin: unsafe extern "C" fn(CP) -> P,
    uis_get: unsafe extern "C" fn(CP, P) -> CP,
    uis_next: unsafe extern "C" fn(CP, P) -> P,
    uis_is_end: unsafe extern "C" fn(CP, P) -> bool,
    uis_free: unsafe extern "C" fn(P),
    ui_get_uri: unsafe extern "C" fn(CP) -> CP,
    ui_is_a: unsafe extern "C" fn(CP, CP) -> bool,
    ui_get_binary_uri: unsafe extern "C" fn(CP) -> CP,
    ui_get_bundle_uri: unsafe extern "C" fn(CP) -> CP,
    new_uri: unsafe extern "C" fn(P, *const c_char) -> P,
    new_file_uri: unsafe extern "C" fn(P, *const c_char, *const c_char) -> P,
    node_free: unsafe extern "C" fn(P),
    node_as_string: unsafe extern "C" fn(CP) -> *const c_char,
    node_as_float: unsafe extern "C" fn(CP) -> f32,
    node_is_float: unsafe extern "C" fn(CP) -> bool,
    node_is_int: unsafe extern "C" fn(CP) -> bool,
    node_as_int: unsafe extern "C" fn(CP) -> i32,
    file_uri_parse: unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> *mut c_char,
    free: unsafe extern "C" fn(P),
    state_new_from_instance: unsafe extern "C" fn(CP, *mut Instance, *mut UridMap, *const c_char, *const c_char, *const c_char, *const c_char, GetValue, P, u32, *const *const Feature) -> P,
    state_to_string: unsafe extern "C" fn(P, *mut UridMap, *mut UridUnmap, CP, *const c_char, *const c_char) -> *mut c_char,
    state_new_from_string: unsafe extern "C" fn(P, *mut UridMap, *const c_char) -> P,
    state_restore: unsafe extern "C" fn(CP, *mut Instance, SetValue, P, u32, *const *const Feature),
    state_free: unsafe extern "C" fn(P),
}
unsafe impl Send for Lilv {}
unsafe impl Sync for Lilv {}

type GetValue = unsafe extern "C" fn(*const c_char, P, *mut u32, *mut u32) -> CP;
type SetValue = unsafe extern "C" fn(*const c_char, P, CP, u32, u32);

/// The system's lilv, when installed.
fn lilv() -> Result<&'static Lilv, String> {
    static L: OnceLock<Option<Lilv>> = OnceLock::new();
    L.get_or_init(|| unsafe { load_lilv().ok() }).as_ref().ok_or_else(|| "LV2 needs the system's lilv library (liblilv-0.so.0), which isn't installed".into())
}

/// True when LV2 plug-ins can be hosted here.
pub fn available() -> bool {
    lilv().is_ok()
}

unsafe fn load_lilv() -> Result<Lilv, libloading::Error> {
    let lib = unsafe { libloading::Library::new("liblilv-0.so.0")? };
    macro_rules! f {
        ($n:literal) => {
            *unsafe { lib.get(concat!("lilv_", $n, "\0").as_bytes())? }
        };
    }
    Ok(Lilv {
        world_new: f!("world_new"),
        world_free: f!("world_free"),
        world_load_bundle: f!("world_load_bundle"),
        world_get_all_plugins: f!("world_get_all_plugins"),
        plugins_begin: f!("plugins_begin"),
        plugins_get: f!("plugins_get"),
        plugins_next: f!("plugins_next"),
        plugins_is_end: f!("plugins_is_end"),
        plugins_get_by_uri: f!("plugins_get_by_uri"),
        plugin_get_uri: f!("plugin_get_uri"),
        plugin_get_name: f!("plugin_get_name"),
        plugin_get_author_name: f!("plugin_get_author_name"),
        plugin_get_value: f!("plugin_get_value"),
        plugin_get_num_ports: f!("plugin_get_num_ports"),
        plugin_get_port_by_index: f!("plugin_get_port_by_index"),
        plugin_instantiate: f!("plugin_instantiate"),
        plugin_get_uis: f!("plugin_get_uis"),
        instance_free: f!("instance_free"),
        port_is_a: f!("port_is_a"),
        port_has_property: f!("port_has_property"),
        port_supports_event: f!("port_supports_event"),
        port_get_name: f!("port_get_name"),
        port_get_symbol: f!("port_get_symbol"),
        port_get_range: f!("port_get_range"),
        port_get_scale_points: f!("port_get_scale_points"),
        scale_points_free: f!("scale_points_free"),
        scale_points_begin: f!("scale_points_begin"),
        scale_points_get: f!("scale_points_get"),
        scale_points_next: f!("scale_points_next"),
        scale_points_is_end: f!("scale_points_is_end"),
        scale_point_get_label: f!("scale_point_get_label"),
        scale_point_get_value: f!("scale_point_get_value"),
        nodes_begin: f!("nodes_begin"),
        nodes_get: f!("nodes_get"),
        nodes_next: f!("nodes_next"),
        nodes_is_end: f!("nodes_is_end"),
        nodes_free: f!("nodes_free"),
        uis_begin: f!("uis_begin"),
        uis_get: f!("uis_get"),
        uis_next: f!("uis_next"),
        uis_is_end: f!("uis_is_end"),
        uis_free: f!("uis_free"),
        ui_get_uri: f!("ui_get_uri"),
        ui_is_a: f!("ui_is_a"),
        ui_get_binary_uri: f!("ui_get_binary_uri"),
        ui_get_bundle_uri: f!("ui_get_bundle_uri"),
        new_uri: f!("new_uri"),
        new_file_uri: f!("new_file_uri"),
        node_free: f!("node_free"),
        node_as_string: f!("node_as_string"),
        node_as_float: f!("node_as_float"),
        node_is_float: f!("node_is_float"),
        node_is_int: f!("node_is_int"),
        node_as_int: f!("node_as_int"),
        file_uri_parse: f!("file_uri_parse"),
        free: f!("free"),
        state_new_from_instance: f!("state_new_from_instance"),
        state_to_string: f!("state_to_string"),
        state_new_from_string: f!("state_new_from_string"),
        state_restore: f!("state_restore"),
        state_free: f!("state_free"),
        _lib: lib,
    })
}

// --- LV2's C structs

#[repr(C)]
pub struct Feature {
    uri: *const c_char,
    data: P,
}

#[repr(C)]
pub struct Descriptor {
    uri: *const c_char,
    instantiate: P,
    connect_port: unsafe extern "C" fn(P, u32, P),
    activate: Option<unsafe extern "C" fn(P)>,
    run: unsafe extern "C" fn(P, u32),
    deactivate: Option<unsafe extern "C" fn(P)>,
    cleanup: unsafe extern "C" fn(P),
    extension_data: Option<unsafe extern "C" fn(*const c_char) -> CP>,
}

/// lilv's LilvInstance (public in lilv.h).
#[repr(C)]
pub struct Instance {
    descriptor: *const Descriptor,
    handle: P,
    pimpl: P,
}

#[repr(C)]
pub struct UridMap {
    handle: P,
    map: unsafe extern "C" fn(P, *const c_char) -> u32,
}

#[repr(C)]
pub struct UridUnmap {
    handle: P,
    unmap: unsafe extern "C" fn(P, u32) -> *const c_char,
}

#[repr(C)]
struct OptionsOption {
    context: u32,
    subject: u32,
    key: u32,
    size: u32,
    ty: u32,
    value: CP,
}

/// The URIs ↔ numbers table (urid:map / unmap), one for the program.
struct Urids {
    to: HashMap<String, u32>,
    from: Vec<CString>,
}

fn urids() -> &'static Mutex<Urids> {
    static U: OnceLock<Mutex<Urids>> = OnceLock::new();
    U.get_or_init(|| Mutex::new(Urids { to: HashMap::new(), from: Vec::new() }))
}

pub fn urid(uri: &str) -> u32 {
    let mut u = urids().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&x) = u.to.get(uri) {
        return x;
    }
    u.from.push(CString::new(uri).unwrap_or_default());
    let id = u.from.len() as u32;
    u.to.insert(uri.to_string(), id);
    id
}

unsafe extern "C" fn map_fn(_h: P, uri: *const c_char) -> u32 {
    if uri.is_null() {
        return 0;
    }
    urid(&unsafe { CStr::from_ptr(uri) }.to_string_lossy())
}

unsafe extern "C" fn unmap_fn(_h: P, id: u32) -> *const c_char {
    let u = urids().lock().unwrap_or_else(|e| e.into_inner());
    u.from.get(id.wrapping_sub(1) as usize).map_or(std::ptr::null(), |c| c.as_ptr())
}

const LV2: &str = "http://lv2plug.in/ns/lv2core#";
const ATOM: &str = "http://lv2plug.in/ns/ext/atom#";
const UI: &str = "http://lv2plug.in/ns/extensions/ui#";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortKind {
    AudioIn,
    AudioOut,
    ControlIn,
    ControlOut,
    AtomIn,
    AtomOut,
    Other,
}

struct Port {
    kind: PortKind,
    midi: bool,
    latency: bool,
    symbol: String,
}

/// A world with one bundle loaded, and the plug-in in it.
struct World {
    world: P,
    plugin: CP,
}

impl World {
    fn new(bundle: &Path, uri: Option<&str>) -> Result<(World, Vec<CP>), String> {
        let l = lilv()?;
        unsafe {
            let world = (l.world_new)();
            let mut dir = bundle.display().to_string();
            if !dir.ends_with('/') {
                dir.push('/');
            }
            let cdir = CString::new(dir).map_err(|e| e.to_string())?;
            let node = (l.new_file_uri)(world, std::ptr::null(), cdir.as_ptr());
            (l.world_load_bundle)(world, node);
            (l.node_free)(node);
            let all = (l.world_get_all_plugins)(world);
            let mut list = Vec::new();
            let mut it = (l.plugins_begin)(all);
            while !(l.plugins_is_end)(all, it) {
                list.push((l.plugins_get)(all, it));
                it = (l.plugins_next)(all, it);
            }
            let plugin = match uri {
                Some(u) => {
                    let cu = CString::new(u).map_err(|e| e.to_string())?;
                    let n = (l.new_uri)(world, cu.as_ptr());
                    let p = (l.plugins_get_by_uri)(all, n);
                    (l.node_free)(n);
                    p
                }
                None => std::ptr::null(),
            };
            Ok((World { world, plugin }, list))
        }
    }
    fn uri(&self, s: &str) -> Node {
        let l = lilv().unwrap();
        let c = CString::new(s).unwrap_or_default();
        Node(unsafe { (l.new_uri)(self.world, c.as_ptr()) })
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Ok(l) = lilv() {
            unsafe { (l.world_free)(self.world) };
        }
    }
}

/// An owned lilv node.
struct Node(P);
impl Drop for Node {
    fn drop(&mut self) {
        if let (Ok(l), false) = (lilv(), self.0.is_null()) {
            unsafe { (l.node_free)(self.0) };
        }
    }
}

unsafe fn node_str(n: CP) -> String {
    if n.is_null() {
        return String::new();
    }
    let l = lilv().unwrap();
    let s = unsafe { (l.node_as_string)(n) };
    if s.is_null() { String::new() } else { unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned() }
}

unsafe fn node_f(n: CP, d: f32) -> f32 {
    if n.is_null() {
        return d;
    }
    let l = lilv().unwrap();
    unsafe {
        if (l.node_is_float)(n) {
            (l.node_as_float)(n)
        } else if (l.node_is_int)(n) {
            (l.node_as_int)(n) as f32
        } else {
            d
        }
    }
}

unsafe fn plugin_types(w: &World, plugin: CP) -> Vec<String> {
    let l = lilv().unwrap();
    let ty = w.uri("http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
    let mut out = Vec::new();
    unsafe {
        let nodes = (l.plugin_get_value)(plugin, ty.0);
        if !nodes.is_null() {
            let mut it = (l.nodes_begin)(nodes);
            while !(l.nodes_is_end)(nodes, it) {
                out.push(node_str((l.nodes_get)(nodes, it)));
                it = (l.nodes_next)(nodes, it);
            }
            (l.nodes_free)(nodes);
        }
    }
    out
}

/// The plug-in's X11 UI: (UI URI, binary path, bundle path).
unsafe fn x11_ui(w: &World, plugin: CP) -> Option<(String, PathBuf, PathBuf)> {
    let l = lilv().ok()?;
    let class = w.uri(&format!("{UI}X11UI"));
    unsafe {
        let uis = (l.plugin_get_uis)(plugin);
        if uis.is_null() {
            return None;
        }
        let mut found = None;
        let mut it = (l.uis_begin)(uis);
        while !(l.uis_is_end)(uis, it) {
            let ui = (l.uis_get)(uis, it);
            if (l.ui_is_a)(ui, class.0) {
                let path = |n: CP| {
                    let s = CString::new(node_str(n)).unwrap_or_default();
                    let p = (l.file_uri_parse)(s.as_ptr(), std::ptr::null_mut());
                    if p.is_null() {
                        return PathBuf::new();
                    }
                    let r = PathBuf::from(CStr::from_ptr(p).to_string_lossy().into_owned());
                    (l.free)(p as P);
                    r
                };
                found = Some((node_str((l.ui_get_uri)(ui)), path((l.ui_get_binary_uri)(ui)), path((l.ui_get_bundle_uri)(ui))));
                break;
            }
            it = (l.uis_next)(uis, it);
        }
        (l.uis_free)(uis);
        found
    }
}

#[repr(C)]
struct UiDescriptor {
    uri: *const c_char,
    instantiate: unsafe extern "C" fn(*const UiDescriptor, *const c_char, *const c_char, WriteFn, P, *mut P, *const *const Feature) -> P,
    cleanup: unsafe extern "C" fn(P),
    port_event: Option<unsafe extern "C" fn(P, u32, u32, u32, CP)>,
    extension_data: Option<unsafe extern "C" fn(*const c_char) -> CP>,
}

type WriteFn = unsafe extern "C" fn(P, u32, u32, u32, CP);

#[repr(C)]
struct UiIdle {
    idle: unsafe extern "C" fn(P) -> i32,
}

#[repr(C)]
struct UiResize {
    handle: P,
    ui_resize: unsafe extern "C" fn(P, i32, i32) -> i32,
}

/// What the UI writes back (control ports by index), read on the main thread.
struct UiShared {
    writes: Mutex<Vec<(u32, f32)>>,
    resize: Mutex<Option<(i32, i32)>>,
}

unsafe extern "C" fn ui_write(ctl: P, port: u32, size: u32, protocol: u32, buf: CP) {
    if protocol != 0 || size != 4 || buf.is_null() {
        return;
    }
    let s = unsafe { &*(ctl as *const UiShared) };
    s.writes.lock().unwrap_or_else(|e| e.into_inner()).push((port, unsafe { *(buf as *const f32) }));
}

unsafe extern "C" fn ui_resize(h: P, w: i32, hh: i32) -> i32 {
    let s = unsafe { &*(h as *const UiShared) };
    *s.resize.lock().unwrap_or_else(|e| e.into_inner()) = Some((w, hh));
    0
}

struct Ui {
    _lib: libloading::Library,
    desc: *const UiDescriptor,
    handle: P,
    idle: *const UiIdle,
    window: HostWindow,
    shared: Box<UiShared>,
    _resize: Box<UiResize>,
    sent: Vec<f32>,
}

pub struct Lv2Plugin {
    world: World,
    instance: *mut Instance,
    ports: Vec<Port>,
    /// Every port's float (control ports' values live here; the plug-in reads them in run).
    values: Vec<f32>,
    infos: Vec<ParamInfo>,
    labels: HashMap<u32, Vec<(f32, String)>>,
    audio: Vec<Vec<f32>>,
    atoms: Vec<Vec<u64>>,
    latency_port: Option<usize>,
    features: Vec<Feature>,
    feature_ptrs: Vec<*const Feature>,
    map: Box<UridMap>,
    unmap: Box<UridUnmap>,
    _options: Box<[OptionsOption; 4]>,
    _option_values: Box<(i32, i32, f32)>,
    active: bool,
    ui: Option<Ui>,
    ui_info: Option<(String, PathBuf, PathBuf)>,
    plugin_uri: CString,
}

unsafe impl Send for Lv2Plugin {}

const ATOM_BYTES: usize = 16 * 1024;

impl Lv2Plugin {
    pub fn open(bundle: &Path, uri: &str, rate: f64) -> Result<Lv2Plugin, String> {
        let l = lilv()?;
        let (world, _) = World::new(bundle, Some(uri))?;
        if world.plugin.is_null() {
            return Err(format!("no plug-in {uri} in the bundle"));
        }
        let p = world.plugin;
        unsafe {
            let audio = world.uri(&format!("{LV2}AudioPort"));
            let control = world.uri(&format!("{LV2}ControlPort"));
            let input = world.uri(&format!("{LV2}InputPort"));
            let output = world.uri(&format!("{LV2}OutputPort"));
            let atom = world.uri(&format!("{ATOM}AtomPort"));
            let midi = world.uri("http://lv2plug.in/ns/ext/midi#MidiEvent");
            let reports = world.uri(&format!("{LV2}reportsLatency"));
            let integer = world.uri(&format!("{LV2}integer"));
            let toggled = world.uri(&format!("{LV2}toggled"));
            let enumeration = world.uri(&format!("{LV2}enumeration"));
            let not_gui = world.uri("http://lv2plug.in/ns/ext/port-props#notOnGUI");
            let n = (l.plugin_get_num_ports)(p);
            let mut ports = Vec::new();
            let mut values = vec![0f32; n as usize];
            let mut infos = Vec::new();
            let mut labels = HashMap::new();
            for i in 0..n {
                let port = (l.plugin_get_port_by_index)(p, i);
                let is = |c: &Node| (l.port_is_a)(p, port, c.0);
                let kind = match (is(&audio), is(&control), is(&atom), is(&input), is(&output)) {
                    (true, _, _, true, _) => PortKind::AudioIn,
                    (true, _, _, _, true) => PortKind::AudioOut,
                    (_, true, _, true, _) => PortKind::ControlIn,
                    (_, true, _, _, true) => PortKind::ControlOut,
                    (_, _, true, true, _) => PortKind::AtomIn,
                    (_, _, true, _, true) => PortKind::AtomOut,
                    _ => PortKind::Other,
                };
                let symbol = node_str((l.port_get_symbol)(p, port));
                let is_midi = matches!(kind, PortKind::AtomIn) && (l.port_supports_event)(p, port, midi.0);
                let latency = kind == PortKind::ControlOut && (l.port_has_property)(p, port, reports.0);
                if kind == PortKind::ControlIn {
                    let (mut d, mut lo, mut hi) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                    (l.port_get_range)(p, port, &mut d, &mut lo, &mut hi);
                    let (min, max) = (node_f(lo, 0.0), node_f(hi, 1.0));
                    let def = node_f(d, min).clamp(min.min(max), max.max(min));
                    for x in [d, lo, hi] {
                        if !x.is_null() {
                            (l.node_free)(x);
                        }
                    }
                    values[i as usize] = def;
                    let name_node = (l.port_get_name)(p, port);
                    let name = node_str(name_node);
                    if !name_node.is_null() {
                        (l.node_free)(name_node);
                    }
                    // scale points: an enumeration's names
                    let mut pts: Vec<(f32, String)> = Vec::new();
                    let sp = (l.port_get_scale_points)(p, port);
                    if !sp.is_null() {
                        let mut it = (l.scale_points_begin)(sp);
                        while !(l.scale_points_is_end)(sp, it) {
                            let s = (l.scale_points_get)(sp, it);
                            pts.push((node_f((l.scale_point_get_value)(s), 0.0), node_str((l.scale_point_get_label)(s))));
                            it = (l.scale_points_next)(sp, it);
                        }
                        (l.scale_points_free)(sp);
                    }
                    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let toggle = (l.port_has_property)(p, port, toggled.0);
                    let stepped = toggle || (l.port_has_property)(p, port, integer.0) || (l.port_has_property)(p, port, enumeration.0);
                    let (min, max) = if toggle { (0.0, 1.0) } else { (min, max) };
                    let steps = if stepped { (max - min).round().max(0.0) as u32 } else { 0 };
                    let choices: Vec<String> = if steps > 0 && pts.len() == steps as usize + 1 && pts.iter().enumerate().all(|(k, (v, _))| (*v - (min + k as f32)).abs() < 1e-3) { pts.iter().map(|x| x.1.clone()).collect() } else { Vec::new() };
                    labels.insert(i, pts);
                    infos.push(ParamInfo { native: i, name: if name.is_empty() { symbol.clone() } else { name }, min: min as f64, max: max as f64, default: def as f64, steps, hidden: (l.port_has_property)(p, port, not_gui.0), choices });
                }
                ports.push(Port { kind, midi: is_midi, latency, symbol });
            }
            let latency_port = ports.iter().position(|x| x.latency);
            let audio_bufs = ports.iter().map(|x| if matches!(x.kind, PortKind::AudioIn | PortKind::AudioOut | PortKind::Other) { vec![0f32; MAX_BLOCK] } else { Vec::new() }).collect();
            let atoms = ports.iter().map(|x| if matches!(x.kind, PortKind::AtomIn | PortKind::AtomOut) { vec![0u64; ATOM_BYTES / 8] } else { Vec::new() }).collect();
            let mut map = Box::new(UridMap { handle: std::ptr::null_mut(), map: map_fn });
            let mut unmap = Box::new(UridUnmap { handle: std::ptr::null_mut(), unmap: unmap_fn });
            let option_values = Box::new((1i32, MAX_BLOCK as i32, rate as f32));
            let (int, float) = (urid(&format!("{ATOM}Int")), urid(&format!("{ATOM}Float")));
            let bs = "http://lv2plug.in/ns/ext/buf-size#";
            let options = Box::new([
                OptionsOption { context: 0, subject: 0, key: urid(&format!("{bs}minBlockLength")), size: 4, ty: int, value: &option_values.0 as *const i32 as CP },
                OptionsOption { context: 0, subject: 0, key: urid(&format!("{bs}maxBlockLength")), size: 4, ty: int, value: &option_values.1 as *const i32 as CP },
                OptionsOption { context: 0, subject: 0, key: urid("http://lv2plug.in/ns/lv2core#sampleRate"), size: 4, ty: float, value: &option_values.2 as *const f32 as CP },
                OptionsOption { context: 0, subject: 0, key: 0, size: 0, ty: 0, value: std::ptr::null() },
            ]);
            let features = vec![
                Feature { uri: c"http://lv2plug.in/ns/ext/urid#map".as_ptr(), data: &mut *map as *mut UridMap as P },
                Feature { uri: c"http://lv2plug.in/ns/ext/urid#unmap".as_ptr(), data: &mut *unmap as *mut UridUnmap as P },
                Feature { uri: c"http://lv2plug.in/ns/ext/options#options".as_ptr(), data: options.as_ptr() as P },
                Feature { uri: c"http://lv2plug.in/ns/ext/buf-size#boundedBlockLength".as_ptr(), data: std::ptr::null_mut() },
            ];
            let mut feature_ptrs: Vec<*const Feature> = features.iter().map(|f| f as *const Feature).collect();
            feature_ptrs.push(std::ptr::null());
            let instance = (l.plugin_instantiate)(p, rate, feature_ptrs.as_ptr());
            if instance.is_null() {
                return Err("the plug-in would not instantiate (a feature it needs, or its library)".into());
            }
            let ui_info = x11_ui(&world, p);
            let mut me = Lv2Plugin { world, instance, ports, values, infos, labels, audio: audio_bufs, atoms, latency_port, features, feature_ptrs, map, unmap, _options: options, _option_values: option_values, active: false, ui: None, ui_info, plugin_uri: CString::new(uri).unwrap_or_default() };
            me.connect_all();
            let d = &*(*me.instance).descriptor;
            if let Some(a) = d.activate {
                a((*me.instance).handle);
            }
            me.active = true;
            // one silent block: the latency port has its value
            let z = vec![0f32; 64];
            let (mut a, mut b) = (vec![0f32; 64], vec![0f32; 64]);
            me.process([&z, &z], [&mut a, &mut b], &[], 120.0);
            Ok(me)
        }
    }

    fn connect_all(&mut self) {
        unsafe {
            let d = &*(*self.instance).descriptor;
            let h = (*self.instance).handle;
            for i in 0..self.ports.len() {
                let ptr: P = match self.ports[i].kind {
                    PortKind::ControlIn | PortKind::ControlOut => &mut self.values[i] as *mut f32 as P,
                    PortKind::AtomIn | PortKind::AtomOut => self.atoms[i].as_mut_ptr() as P,
                    _ => self.audio[i].as_mut_ptr() as P,
                };
                (d.connect_port)(h, i as u32, ptr);
            }
        }
    }

    fn handle(&self) -> P {
        unsafe { (*self.instance).handle }
    }

    fn ui_open(&mut self, title: &str) -> Result<(), String> {
        let (uri, binary, bundle) = self.ui_info.clone().ok_or("the plug-in has no X11 editor (Studio embeds LV2 X11 UIs; GTK and Qt UIs need suil)")?;
        if !crate::editor::x11_available() {
            return Err("the plug-in's editor needs X11 (XWayland): DISPLAY isn't set".into());
        }
        unsafe {
            let lib = libloading::Library::new(&binary).map_err(|e| e.to_string())?;
            let get = *lib.get::<unsafe extern "C" fn(u32) -> *const UiDescriptor>(b"lv2ui_descriptor\0").map_err(|_| "the UI has no lv2ui_descriptor".to_string())?;
            let mut desc = std::ptr::null();
            for i in 0..64 {
                let d = get(i);
                if d.is_null() {
                    break;
                }
                if CStr::from_ptr((*d).uri).to_string_lossy() == uri {
                    desc = d;
                    break;
                }
            }
            if desc.is_null() {
                return Err("the UI library doesn't have the plug-in's UI".into());
            }
            let mut window = HostWindow::new(title, 400, 300, true)?;
            let shared = Box::new(UiShared { writes: Mutex::new(Vec::new()), resize: Mutex::new(None) });
            let mut resize = Box::new(UiResize { handle: &*shared as *const UiShared as P, ui_resize });
            let parent = Feature { uri: c"http://lv2plug.in/ns/extensions/ui#parent".as_ptr(), data: window.id() as usize as P };
            let resize_f = Feature { uri: c"http://lv2plug.in/ns/extensions/ui#resize".as_ptr(), data: &mut *resize as *mut UiResize as P };
            let access = Feature { uri: c"http://lv2plug.in/ns/ext/instance-access".as_ptr(), data: self.handle() };
            let feats: Vec<*const Feature> = vec![&self.features[0], &self.features[1], &self.features[2], &parent, &resize_f, &access, std::ptr::null()];
            let mut widget: P = std::ptr::null_mut();
            let bpath = CString::new(format!("{}/", bundle.display())).unwrap_or_default();
            let handle = ((*desc).instantiate)(desc, self.plugin_uri.as_ptr(), bpath.as_ptr(), ui_write, &*shared as *const UiShared as P, &mut widget, feats.as_ptr());
            if handle.is_null() {
                return Err("the plug-in's editor would not start".into());
            }
            let idle = (*desc).extension_data.map_or(std::ptr::null(), |e| e(c"http://lv2plug.in/ns/extensions/ui#idleInterface".as_ptr())) as *const UiIdle;
            if let Some(r) = shared.resize.lock().unwrap_or_else(|e| e.into_inner()).take() {
                window.resize(r.0.max(1) as u32, r.1.max(1) as u32);
            }
            window.show();
            self.ui = Some(Ui { _lib: lib, desc, handle, idle, window, shared, _resize: resize, sent: Vec::new() });
            self.ui_send_values();
        }
        Ok(())
    }

    /// The control values the UI hasn't seen yet.
    fn ui_send_values(&mut self) {
        let Some(ui) = self.ui.as_mut() else { return };
        ui.sent.resize(self.values.len(), f32::NAN);
        unsafe {
            if let Some(pe) = (*ui.desc).port_event {
                for (i, p) in self.ports.iter().enumerate() {
                    if matches!(p.kind, PortKind::ControlIn | PortKind::ControlOut) && ui.sent[i] != self.values[i] {
                        ui.sent[i] = self.values[i];
                        pe(ui.handle, i as u32, 4, 0, &self.values[i] as *const f32 as CP);
                    }
                }
            }
        }
    }

    fn ui_close(&mut self) {
        if let Some(ui) = self.ui.take() {
            unsafe { ((*ui.desc).cleanup)(ui.handle) };
        }
    }

    pub fn editor_window(&self) -> Option<&HostWindow> {
        self.ui.as_ref().map(|u| &u.window)
    }
}

unsafe extern "C" fn get_port_value(symbol: *const c_char, data: P, size: *mut u32, ty: *mut u32) -> CP {
    let me = unsafe { &*(data as *const Lv2Plugin) };
    let s = unsafe { CStr::from_ptr(symbol) }.to_string_lossy();
    match me.ports.iter().position(|p| p.kind == PortKind::ControlIn && p.symbol == s) {
        Some(i) => unsafe {
            *size = 4;
            *ty = urid(&format!("{ATOM}Float"));
            &me.values[i] as *const f32 as CP
        },
        None => unsafe {
            *size = 0;
            *ty = 0;
            std::ptr::null()
        },
    }
}

unsafe extern "C" fn set_port_value(symbol: *const c_char, data: P, value: CP, size: u32, ty: u32) {
    let me = unsafe { &mut *(data as *mut Lv2Plugin) };
    let s = unsafe { CStr::from_ptr(symbol) }.to_string_lossy();
    if let Some(i) = me.ports.iter().position(|p| p.kind == PortKind::ControlIn && p.symbol == s) {
        let v = if ty == urid(&format!("{ATOM}Float")) && size == 4 {
            unsafe { *(value as *const f32) }
        } else if ty == urid(&format!("{ATOM}Double")) && size == 8 {
            unsafe { *(value as *const f64) as f32 }
        } else if ty == urid(&format!("{ATOM}Int")) && size == 4 {
            unsafe { *(value as *const i32) as f32 }
        } else {
            return;
        };
        me.values[i] = v;
    }
}

impl Backend for Lv2Plugin {
    fn param_infos(&mut self) -> Vec<ParamInfo> {
        self.infos.clone()
    }
    fn get(&self, native: u32) -> f64 {
        self.values.get(native as usize).copied().unwrap_or(0.0) as f64
    }
    fn set(&mut self, native: u32, v: f64) {
        if let Some(info) = self.infos.iter().find(|p| p.native == native) {
            let v = v.clamp(info.min.min(info.max), info.max.max(info.min));
            self.values[native as usize] = v as f32;
        }
    }
    fn text(&self, native: u32, v: f64) -> Option<String> {
        let pts = self.labels.get(&native)?;
        pts.iter().find(|(x, _)| (*x as f64 - v).abs() < 1e-3).map(|x| x.1.clone())
    }
    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2], events: &[Ev], _tempo: f64) {
        let n = out[0].len().min(out[1].len()).min(MAX_BLOCK);
        if !self.active || n == 0 {
            return;
        }
        let seq = urid(&format!("{ATOM}Sequence"));
        let chunk = urid(&format!("{ATOM}Chunk"));
        let midi_ev = urid("http://lv2plug.in/ns/ext/midi#MidiEvent");
        let mut ai = 0;
        for i in 0..self.ports.len() {
            match self.ports[i].kind {
                PortKind::AudioIn => {
                    let buf = &mut self.audio[i][..n];
                    match ai {
                        0 | 1 => buf.copy_from_slice(&inp[ai][..n]),
                        _ => buf.fill(0.0),
                    }
                    ai += 1;
                }
                PortKind::AtomIn => {
                    // a sequence: header (size, type), body (unit, pad), then the events
                    let buf = &mut self.atoms[i];
                    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, ATOM_BYTES) };
                    let mut at = 16;
                    if self.ports[i].midi {
                        for e in events {
                            let msg: [u8; 3] = match *e {
                                Ev::NoteOn { key, velocity } => [0x90, key, velocity],
                                Ev::NoteOff { key } => [0x80, key, 0],
                                Ev::Cc { cc, value } => [0xB0, cc, value],
                                Ev::Bend(b) => {
                                    let v = (b as i32 + 8192).clamp(0, 16383) as u16;
                                    [0xE0, (v & 0x7F) as u8, (v >> 7) as u8]
                                }
                            };
                            if at + 32 > ATOM_BYTES {
                                break;
                            }
                            bytes[at..at + 8].copy_from_slice(&0i64.to_le_bytes());
                            bytes[at + 8..at + 12].copy_from_slice(&3u32.to_le_bytes());
                            bytes[at + 12..at + 16].copy_from_slice(&midi_ev.to_le_bytes());
                            bytes[at + 16..at + 19].copy_from_slice(&msg);
                            at += 16 + 8;
                        }
                    }
                    bytes[0..4].copy_from_slice(&((at - 8) as u32).to_le_bytes());
                    bytes[4..8].copy_from_slice(&seq.to_le_bytes());
                    bytes[8..16].fill(0);
                }
                PortKind::AtomOut => {
                    let bytes = unsafe { std::slice::from_raw_parts_mut(self.atoms[i].as_mut_ptr() as *mut u8, ATOM_BYTES) };
                    bytes[0..4].copy_from_slice(&((ATOM_BYTES - 8) as u32).to_le_bytes());
                    bytes[4..8].copy_from_slice(&chunk.to_le_bytes());
                }
                _ => {}
            }
        }
        unsafe {
            let d = &*(*self.instance).descriptor;
            (d.run)(self.handle(), n as u32);
        }
        let [ol, or] = out;
        let mut outs = (0..self.ports.len()).filter(|&i| self.ports[i].kind == PortKind::AudioOut);
        match (outs.next(), outs.next()) {
            (None, _) => {
                ol[..n].fill(0.0);
                or[..n].fill(0.0);
            }
            (Some(m), None) => {
                ol[..n].copy_from_slice(&self.audio[m][..n]);
                or[..n].copy_from_slice(&self.audio[m][..n]);
            }
            (Some(a), Some(b)) => {
                ol[..n].copy_from_slice(&self.audio[a][..n]);
                or[..n].copy_from_slice(&self.audio[b][..n]);
            }
        }
    }
    fn latency(&self) -> u32 {
        self.latency_port.map_or(0, |i| self.values[i].max(0.0) as u32)
    }
    fn reset(&mut self) {
        unsafe {
            let d = &*(*self.instance).descriptor;
            if let (Some(de), Some(a)) = (d.deactivate, d.activate) {
                de(self.handle());
                a(self.handle());
            }
        }
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        let l = lilv().ok()?;
        unsafe {
            let me = self as *mut Lv2Plugin as P;
            let st = (l.state_new_from_instance)(self.world.plugin, self.instance, &mut *self.map, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), get_port_value, me, 0, self.feature_ptrs.as_ptr());
            if st.is_null() {
                return None;
            }
            let s = (l.state_to_string)(self.world.world, &mut *self.map, &mut *self.unmap, st, c"urn:bramble:state".as_ptr(), std::ptr::null());
            (l.state_free)(st);
            if s.is_null() {
                return None;
            }
            let out = CStr::from_ptr(s).to_bytes().to_vec();
            (l.free)(s as P);
            Some(out)
        }
    }
    fn load_state(&mut self, data: &[u8]) -> bool {
        let Ok(l) = lilv() else { return false };
        let Ok(c) = CString::new(data) else { return false };
        unsafe {
            let st = (l.state_new_from_string)(self.world.world, &mut *self.map, c.as_ptr());
            if st.is_null() {
                return false;
            }
            let me = self as *mut Lv2Plugin as P;
            (l.state_restore)(st, self.instance, set_port_value, me, 0, self.feature_ptrs.as_ptr());
            (l.state_free)(st);
        }
        true
    }
    fn idle(&mut self) -> Idle {
        let mut r = Idle::default();
        let Some(ui) = self.ui.as_mut() else { return r };
        // the UI's own idle first (it may write values), then what it wrote
        if !ui.idle.is_null() && unsafe { ((*ui.idle).idle)(ui.handle) } != 0 {
            self.ui_close();
            r.editor_closed = true;
            return r;
        }
        let writes = std::mem::take(&mut *ui.shared.writes.lock().unwrap_or_else(|e| e.into_inner()));
        for (port, v) in writes {
            if self.ports.get(port as usize).is_some_and(|p| p.kind == PortKind::ControlIn) {
                self.values[port as usize] = v;
                ui.sent.resize(self.values.len(), f32::NAN);
                ui.sent[port as usize] = v;
                r.changed = true;
            }
        }
        if let Some((w, h)) = ui.shared.resize.lock().unwrap_or_else(|e| e.into_inner()).take() {
            ui.window.resize(w.max(1) as u32, h.max(1) as u32);
        }
        if ui.window.poll().contains(&WinEvent::Closed) {
            self.ui_close();
            r.editor_closed = true;
            return r;
        }
        self.ui_send_values();
        r
    }
    fn has_editor(&self) -> bool {
        self.ui_info.is_some()
    }
    fn open_editor(&mut self, title: &str) -> Result<(), String> {
        if self.ui.is_some() {
            return Ok(());
        }
        self.ui_open(title)
    }
    fn close_editor(&mut self) {
        self.ui_close();
    }
    fn editor_open(&self) -> bool {
        self.ui.is_some()
    }
}

impl Drop for Lv2Plugin {
    fn drop(&mut self) {
        self.ui_close();
        if let Ok(l) = lilv() {
            unsafe {
                let d = &*(*self.instance).descriptor;
                if self.active {
                    if let Some(de) = d.deactivate {
                        de(self.handle());
                    }
                }
                (l.instance_free)(self.instance);
            }
        }
    }
}

/// The plug-ins in an LV2 bundle (the scan, in its own process).
pub fn describe(bundle: &Path) -> Result<Vec<PluginInfo>, String> {
    let l = lilv()?;
    let (world, list) = World::new(bundle, None)?;
    let mut out = Vec::new();
    for p in list {
        unsafe {
            let uri = node_str((l.plugin_get_uri)(p));
            let name_n = (l.plugin_get_name)(p);
            let name = node_str(name_n);
            if !name_n.is_null() {
                (l.node_free)(name_n);
            }
            let auth_n = (l.plugin_get_author_name)(p);
            let vendor = node_str(auth_n);
            if !auth_n.is_null() {
                (l.node_free)(auth_n);
            }
            let types = plugin_types(&world, p);
            let classes: Vec<String> = types.iter().filter_map(|t| t.strip_prefix(LV2)).filter(|t| *t != "Plugin").map(|t| t.trim_end_matches("Plugin").to_string()).collect();
            let plug = Lv2Plugin::open(bundle, &uri, 48000.0)?;
            let ins = plug.ports.iter().filter(|x| x.kind == PortKind::AudioIn).count() as u32;
            let outs = plug.ports.iter().filter(|x| x.kind == PortKind::AudioOut).count() as u32;
            let takes_midi = plug.ports.iter().any(|x| x.midi);
            let kind = if types.iter().any(|t| t.ends_with("#InstrumentPlugin")) || (takes_midi && ins == 0) { Kind::Instrument } else { Kind::Effect };
            out.push(PluginInfo { format: Format::Lv2, id: uri, name, vendor, version: String::new(), kind, category: classes.join(", "), path: bundle.to_path_buf(), inputs: ins.min(2), outputs: outs.min(2), params: plug.infos.iter().filter(|x| !x.hidden).count() as u32, latency: Backend::latency(&plug), has_editor: plug.ui_info.is_some() });
        }
    }
    Ok(out.into_iter().filter(|i| i.outputs > 0).collect())
}
