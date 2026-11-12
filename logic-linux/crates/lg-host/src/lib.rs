//! Bramble Studio's plug-in hosting, as Logic hosts Audio Units: Studio hosts the Linux formats,
//! CLAP, VST3 and LV2, instruments and effects.
//!
//! - `scan`: the standard folders (~/.clap, /usr/lib/clap, ~/.vst3, /usr/lib/vst3, ~/.lv2,
//!   /usr/lib/lv2 …), each plug-in file looked at in a separate process (`lg-plugscan`, or
//!   Studio's own binary with `--scan-plugin`) so a crashing plug-in can't take Studio down; the
//!   results (and the Plug-in Manager's enable / disable, crash counts) kept in Studio's settings.
//! - `clap`, `vst3`, `lv2`: a loaded plug-in. Every one is an [`Instance`], which is an
//!   `lg_dsp::Instrument` and an `lg_dsp::Effect`, so the engine, the channel strip slots, the
//!   plug-in windows, automation lanes and Smart Controls treat it as one of Studio's own:
//!   parameters (dense ids 0..n, the plug-in's own ids kept inside), latency (into the plug-in
//!   delay compensation), state (in the slot's saved floats, see [`state`]), its editor.
//! - `editor`: the host window a plug-in's own editor is embedded in (X11, through XWayland on a
//!   Wayland desktop), and the timers / file descriptors its event loop asks for.
//!
//! A hosted plug-in's name in Studio is "Name (CLAP)", "Name (VST3)" or "Name (LV2)"
//! ([`PluginInfo::studio_name`]); [`find`] gives it back from the scanned list.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub mod clap;
pub mod editor;
pub mod lv2;
pub mod scan;
pub mod state;
pub mod vst3;

/// A plug-in library file as it was when loaded (size, modification time, inode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    ino: u64,
}

pub(crate) fn stamp(path: &std::path::Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    let ino = std::os::unix::fs::MetadataExt::ino(&m);
    #[cfg(not(unix))]
    let ino = 0;
    Some(Stamp { len: m.len(), modified: m.modified().ok(), ino })
}

/// Where to load a plug-in library from. The file itself the first time; once it has changed
/// since it was loaded (a plug-in updated while Studio runs), a private copy: rewritten in
/// place, the loaded library's pages now read the new bytes (its relocated data is lost: it
/// lists nothing and crashes), and loading the same file again would only hand back that stale
/// library. The copy can be removed as soon as it's loaded (the mapping keeps it).
pub(crate) fn load_path(path: &std::path::Path, changed: bool) -> Result<PathBuf, String> {
    if !changed {
        return Ok(path.to_path_buf());
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("bramble-studio-plugins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let copy = dir.join(format!("{n}-{}", path.file_name().map(|f| f.to_string_lossy()).unwrap_or_default()));
    std::fs::copy(path, &copy).map_err(|e| e.to_string())?;
    Ok(copy)
}

/// After loading from a private copy: the copy's file is no longer needed.
pub(crate) fn loaded_from(original: &std::path::Path, used: &std::path::Path) {
    if used != original {
        let _ = std::fs::remove_file(used);
    }
}

/// The plug-in formats Studio hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    Clap,
    Vst3,
    Lv2,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Clap, Format::Vst3, Format::Lv2];
    pub fn label(self) -> &'static str {
        match self {
            Format::Clap => "CLAP",
            Format::Vst3 => "VST3",
            Format::Lv2 => "LV2",
        }
    }
    pub fn parse(s: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.label().eq_ignore_ascii_case(s))
    }
}

/// What a plug-in is (where it can go: Logic puts instruments in the instrument slot, effects
/// in the Audio FX slots).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Instrument,
    Effect,
}

/// One plug-in as the scan found it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginInfo {
    pub format: Format,
    /// CLAP: the plug-in id; VST3: the class id (32 hex digits); LV2: the plug-in URI.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: Kind,
    /// The plug-in's own category words (CLAP features, VST3 subcategories, the LV2 class).
    pub category: String,
    /// The file or bundle the scan looked at.
    pub path: PathBuf,
    /// Audio inputs and outputs (channels of the main ports), parameters, latency at load.
    pub inputs: u32,
    pub outputs: u32,
    pub params: u32,
    pub latency: u32,
    /// The plug-in has its own editor window.
    pub has_editor: bool,
}

impl PluginInfo {
    /// The name Studio's slots and menus use: "Name (FORMAT)".
    pub fn studio_name(&self) -> String {
        format!("{} ({})", self.name, self.format.label())
    }
    /// The manufacturer as the menus group it.
    pub fn manufacturer(&self) -> &str {
        if self.vendor.trim().is_empty() { "Unknown" } else { self.vendor.trim() }
    }
}

/// Splits "Name (FORMAT)" into the name and the format.
pub fn parse_studio_name(n: &str) -> Option<(&str, Format)> {
    let n = n.trim();
    let open = n.rfind(" (")?;
    let f = Format::parse(n[open + 2..].strip_suffix(')')?)?;
    Some((&n[..open], f))
}

/// The plug-ins Studio knows (the Plug-in Manager's list, kept in Studio's settings).
fn catalog() -> &'static Mutex<Vec<PluginInfo>> {
    static C: OnceLock<Mutex<Vec<PluginInfo>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(scan::load_cache().into_iter().filter(|e| e.status.ok()).filter_map(|e| e.info).collect()))
}

/// Replaces the list of plug-ins Studio can load (after a scan).
pub fn set_catalog(list: Vec<PluginInfo>) {
    *catalog().lock().unwrap_or_else(|e| e.into_inner()) = list;
}

/// Every loadable plug-in (the scan's good results).
pub fn plugins() -> Vec<PluginInfo> {
    catalog().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The plug-in a Studio name ("Name (CLAP)") stands for.
pub fn find(studio_name: &str) -> Option<PluginInfo> {
    let (name, f) = parse_studio_name(studio_name)?;
    catalog().lock().unwrap_or_else(|e| e.into_inner()).iter().find(|p| p.format == f && p.name == name).cloned()
}

/// A note or controller for a plug-in, at a frame of the next block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    NoteOn { key: u8, velocity: u8 },
    NoteOff { key: u8 },
    Cc { cc: u8, value: u8 },
    /// -8192..8191.
    Bend(i16),
}

/// A parameter as the plug-in describes it (its own id, plain values).
#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub native: u32,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// Number of steps for a stepped / list parameter (0: continuous).
    pub steps: u32,
    pub hidden: bool,
    /// The step names, when the plug-in names them.
    pub choices: Vec<String>,
}

/// What a format's loaded plug-in does; [`Instance`] wraps it.
#[doc(hidden)]
pub trait Backend: Send {
    fn param_infos(&mut self) -> Vec<ParamInfo>;
    /// The value now (cached: callable on any thread).
    fn get(&self, native: u32) -> f64;
    /// Sets a value (sent with the next block).
    fn set(&mut self, native: u32, v: f64);
    fn text(&self, native: u32, v: f64) -> Option<String>;
    /// Renders one block: `inp` is read (instruments get silence), `out` written.
    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2], events: &[Ev], tempo: f64);
    /// Samples of delay the plug-in reports (cached).
    fn latency(&self) -> u32;
    fn tail(&self) -> f32 {
        0.0
    }
    fn reset(&mut self);
    fn save_state(&mut self) -> Option<Vec<u8>>;
    fn load_state(&mut self, data: &[u8]) -> bool;
    /// Main-thread housekeeping (the plug-in's callbacks, timers, file descriptors, editor
    /// window events).
    fn idle(&mut self) -> Idle;
    fn has_editor(&self) -> bool;
    fn open_editor(&mut self, title: &str) -> Result<(), String>;
    fn close_editor(&mut self);
    fn editor_open(&self) -> bool;
}

/// What happened in the plug-in since the last idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Idle {
    /// Parameter values changed in the plug-in (its editor): the saved setting is stale.
    pub changed: bool,
    /// The parameter list changed (names, ranges, count).
    pub params_rescanned: bool,
    /// The editor window was closed by its own close button.
    pub editor_closed: bool,
}

const MAX_BLOCK: usize = 4096;

/// A loaded plug-in.
pub struct Instance {
    pub info: PluginInfo,
    backend: Box<dyn Backend>,
    params: Vec<lg_dsp::Param>,
    natives: Vec<u32>,
    /// Each parameter's plug-in range and steps (a menu's values are its step numbers).
    ranges: Vec<(f64, f64, u32)>,
    events: Vec<Ev>,
    inl: Vec<f32>,
    inr: Vec<f32>,
    outl: Vec<f32>,
    outr: Vec<f32>,
    tempo: f64,
    /// Set by `idle` when the plug-in's own editor changed something; cleared by
    /// [`Instance::take_changed`].
    changed: bool,
}

/// Loads a plug-in for Studio's engine (at `rate` Hz).
pub fn open(info: &PluginInfo, rate: f32) -> Result<Instance, String> {
    let backend: Box<dyn Backend> = match info.format {
        Format::Clap => Box::new(clap::ClapPlugin::open(&info.path, &info.id, rate as f64)?),
        Format::Vst3 => Box::new(vst3::Vst3Plugin::open(&info.path, &info.id, rate as f64)?),
        Format::Lv2 => Box::new(lv2::Lv2Plugin::open(&info.path, &info.id, rate as f64)?),
    };
    let mut inst = Instance { info: info.clone(), backend, params: Vec::new(), natives: Vec::new(), ranges: Vec::new(), events: Vec::with_capacity(256), inl: vec![0.0; MAX_BLOCK], inr: vec![0.0; MAX_BLOCK], outl: vec![0.0; MAX_BLOCK], outr: vec![0.0; MAX_BLOCK], tempo: 120.0, changed: false };
    inst.rescan_params();
    Ok(inst)
}

/// Loads the plug-in a Studio name stands for.
pub fn open_by_name(studio_name: &str, rate: f32) -> Option<Instance> {
    let info = find(studio_name)?;
    if scan::disabled().contains(&scan::key(&info)) {
        return None;
    }
    match open(&info, rate) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("Bramble Studio: {}: {e}", info.studio_name());
            None
        }
    }
}

/// Keeps a string for the life of the program (parameter names are `&'static str` in lg-dsp),
/// each distinct one once.
fn intern(s: &str) -> &'static str {
    static S: OnceLock<Mutex<std::collections::HashSet<&'static str>>> = OnceLock::new();
    let mut set = S.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = set.get(s) {
        return x;
    }
    let x: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(x);
    x
}

fn intern_list(v: &[String]) -> &'static [&'static str] {
    static S: OnceLock<Mutex<std::collections::HashMap<Vec<String>, &'static [&'static str]>>> = OnceLock::new();
    let mut map = S.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = map.get(v) {
        return x;
    }
    let list: Vec<&'static str> = v.iter().map(|s| intern(s)).collect();
    let x: &'static [&'static str] = Box::leak(list.into_boxed_slice());
    map.insert(v.to_vec(), x);
    x
}

impl Instance {
    /// Reads the plug-in's parameter list again (dense ids in the plug-in's order; hidden ones
    /// left out).
    fn rescan_params(&mut self) {
        let infos = self.backend.param_infos();
        self.params.clear();
        self.natives.clear();
        self.ranges.clear();
        for p in infos.into_iter().filter(|p| !p.hidden) {
            let id = self.params.len() as u32;
            let name = intern(if p.name.trim().is_empty() { "Parameter" } else { p.name.trim() });
            let menu = p.steps > 0 && p.steps <= 64 && (p.choices.len() == p.steps as usize + 1 || p.steps == 1);
            let param = if menu {
                let names: Vec<String> = if p.choices.len() == p.steps as usize + 1 { p.choices.clone() } else { vec!["Off".into(), "On".into()] };
                let mut q = lg_dsp::Param::menu(id, name, intern_list(&names), ((p.default - p.min) / (p.max - p.min).max(1e-12) * p.steps as f64).round() as f32);
                if p.steps == 1 && p.choices.len() != 2 {
                    q.unit = lg_dsp::Unit::OnOff;
                }
                q
            } else {
                lg_dsp::Param::new(id, name, p.min as f32, p.max as f32, p.default as f32, lg_dsp::Unit::None)
            };
            self.params.push(param);
            self.natives.push(p.native);
            self.ranges.push((p.min, p.max, p.steps));
        }
    }

    fn range_of(&self, id: u32) -> (f64, f64, u32) {
        self.ranges.get(id as usize).copied().unwrap_or((0.0, 1.0, 1))
    }

    /// The plug-in's own id of a Studio parameter id.
    pub fn native_id(&self, id: u32) -> Option<u32> {
        self.natives.get(id as usize).copied()
    }

    /// The plug-in's state (its own blob), for saving.
    pub fn save_state(&mut self) -> Option<Vec<u8>> {
        self.backend.save_state()
    }

    pub fn load_state(&mut self, data: &[u8]) -> bool {
        self.backend.load_state(data)
    }

    /// The saved floats for this plug-in now: the parameter values and the state (see
    /// [`state::encode`]).
    pub fn floats(&mut self) -> Vec<f32> {
        let values: Vec<f32> = (0..self.params.len() as u32).map(|id| lg_dsp::Plugin::get(self, id)).collect();
        let blob = self.save_state();
        state::encode(&values, blob.as_deref())
    }

    /// Main-thread housekeeping; call often (each frame) from the thread that loaded it.
    pub fn idle(&mut self) -> Idle {
        let r = self.backend.idle();
        if r.params_rescanned {
            self.rescan_params();
        }
        if r.changed {
            self.changed = true;
        }
        r
    }

    /// True once after the plug-in's own editor changed its settings.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    pub fn has_editor(&self) -> bool {
        self.backend.has_editor()
    }

    pub fn open_editor(&mut self, title: &str) -> Result<(), String> {
        self.backend.open_editor(title)
    }

    pub fn close_editor(&mut self) {
        self.backend.close_editor()
    }

    pub fn editor_open(&self) -> bool {
        self.backend.editor_open()
    }

    fn native_value(&self, id: u32, v: f32) -> f64 {
        let Some(p) = self.params.get(id as usize) else { return v as f64 };
        if p.unit == lg_dsp::Unit::Menu || p.unit == lg_dsp::Unit::OnOff {
            let (lo, hi, steps) = self.range_of(id);
            lo + (hi - lo) * (v as f64 / steps.max(1) as f64)
        } else {
            v as f64
        }
    }

    fn studio_value(&self, id: u32, v: f64) -> f32 {
        let Some(p) = self.params.get(id as usize) else { return v as f32 };
        if p.unit == lg_dsp::Unit::Menu || p.unit == lg_dsp::Unit::OnOff {
            let (lo, hi, steps) = self.range_of(id);
            (((v - lo) / (hi - lo).max(1e-12)) * steps as f64).round() as f32
        } else {
            v as f32
        }
    }
}

impl lg_dsp::Plugin for Instance {
    fn params(&self) -> &[lg_dsp::Param] {
        &self.params
    }
    fn get(&self, id: u32) -> f32 {
        match self.natives.get(id as usize) {
            Some(&n) => self.studio_value(id, self.backend.get(n)),
            None => 0.0,
        }
    }
    fn set(&mut self, id: u32, value: f32) {
        if let Some(&n) = self.natives.get(id as usize) {
            let v = self.native_value(id, value);
            self.backend.set(n, v);
        }
    }
    fn reset(&mut self) {
        self.events.clear();
        self.backend.reset();
    }
    fn tail(&self) -> f32 {
        self.backend.tail()
    }
    /// The parameter values, and the plug-in's state when the floats carry one (applied first,
    /// then the values that differ from it: edits made in the Controls view after the state
    /// was taken).
    fn load_floats(&mut self, floats: &[f32]) {
        let (values, blob) = state::decode(floats);
        if let Some(b) = blob {
            self.backend.load_state(&b);
            let _ = self.backend.idle();
        }
        for (id, v) in values.iter().enumerate().take(self.params.len()) {
            let now = lg_dsp::Plugin::get(self, id as u32);
            let p = &self.params[id];
            if (now - v).abs() > (p.max - p.min).abs() * 1e-5 {
                lg_dsp::Plugin::set(self, id as u32, *v);
            }
        }
    }
    fn value_text(&self, id: u32, value: f32) -> Option<String> {
        let n = *self.natives.get(id as usize)?;
        let v = self.native_value(id, value);
        // (a main-thread call in the formats: the plug-in windows ask from it)
        self.backend.text(n, v)
    }
    fn hosted(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl lg_dsp::Instrument for Instance {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if self.events.len() < self.events.capacity() {
            self.events.push(Ev::NoteOn { key: pitch, velocity });
        }
    }
    fn note_off(&mut self, pitch: u8) {
        if self.events.len() < self.events.capacity() {
            self.events.push(Ev::NoteOff { key: pitch });
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if self.events.len() < self.events.capacity() {
            self.events.push(Ev::Cc { cc, value });
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        if self.events.len() < self.events.capacity() {
            self.events.push(Ev::Bend(value));
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut at = 0;
        while at < n {
            let m = (n - at).min(MAX_BLOCK);
            self.inl[..m].fill(0.0);
            self.inr[..m].fill(0.0);
            let (ol, or) = (&mut self.outl[..m], &mut self.outr[..m]);
            ol.fill(0.0);
            or.fill(0.0);
            let ev = if at == 0 { &self.events[..] } else { &[][..] };
            self.backend.process([&self.inl[..m], &self.inr[..m]], [ol, or], ev, self.tempo);
            for k in 0..m {
                left[at + k] += self.outl[k];
                right[at + k] += self.outr[k];
            }
            at += m;
        }
        self.events.clear();
    }
    fn latency(&self) -> usize {
        self.backend.latency() as usize
    }
}

impl lg_dsp::Effect for Instance {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut at = 0;
        while at < n {
            let m = (n - at).min(MAX_BLOCK);
            self.inl[..m].copy_from_slice(&left[at..at + m]);
            self.inr[..m].copy_from_slice(&right[at..at + m]);
            let ev = if at == 0 { &self.events[..] } else { &[][..] };
            self.backend.process([&self.inl[..m], &self.inr[..m]], [&mut left[at..at + m], &mut right[at..at + m]], ev, self.tempo);
            at += m;
        }
        self.events.clear();
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.tempo = bpm;
    }
    fn latency(&self) -> usize {
        self.backend.latency() as usize
    }
}
