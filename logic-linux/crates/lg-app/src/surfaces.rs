//! Control surfaces (Logic Pro ▸ Control Surfaces ▸): the Control Surfaces Setup window (devices
//! in groups, Install and Scan, the Device and Control Surface Group parameters), Mackie Control
//! (and its extenders) and HUI over MIDI ports — faders, V-Pots, buttons, banking, transport,
//! with motor fader, LED, LCD and time code feedback — and the Controller Assignments window
//! (Easy and Expert views, Learn, zones and modes) for any MIDI controller (lg_core::ctrlsurf).
//!
//! Everything is kept in ~/.config/bramble-studio/control-surfaces.txt (Logic's
//! com.apple.logic.pro.cs) and saved as it changes.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::ctrlsurf::{self as cs, mcu, Assignment, Assignments, Class, Hui, Mcu, Strip, StripParam, ValueMode};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Control Surfaces Setup…", "Setup…", "Controller Assignments…", "Open Controller Assignments", "Learn new Controller Assignment", "Learn Assignment for “Solo”",
    "Rebuild Defaults", "Bypass All Control Surfaces", "Toggle Bypass All Control Surfaces", "Open Control Surfaces Settings…", "Settings…",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Model {
    MackieControl,
    MackieExtender,
    Hui,
    Generic,
}

impl Model {
    pub const ALL: [Model; 4] = [Model::MackieControl, Model::MackieExtender, Model::Hui, Model::Generic];
    pub fn name(&self) -> &'static str {
        match self {
            Model::MackieControl => "Mackie Control",
            Model::MackieExtender => "Mackie Control XT",
            Model::Hui => "HUI",
            Model::Generic => "Generic MIDI Controller",
        }
    }
    pub fn module(&self) -> &'static str {
        match self {
            Model::MackieControl | Model::MackieExtender => "Mackie Control",
            Model::Hui => "HUI",
            Model::Generic => "Learned",
        }
    }
    fn faders(&self) -> i32 {
        match self {
            Model::Generic => 0,
            _ => 8,
        }
    }
    fn tag(&self) -> &'static str {
        match self {
            Model::MackieControl => "mcu",
            Model::MackieExtender => "mcx",
            Model::Hui => "hui",
            Model::Generic => "gen",
        }
    }
}

/// A control surface: model, ports, place in the Setup window (row = group, order in it).
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub model: Model,
    pub in_port: String,
    pub out_port: String,
    pub row: usize,
    pub col: usize,
    pub color: u32,
    pub version: String,
    pub transport_click: bool,
    pub touchless: bool,
    pub backlight_min: i32,
}

/// A control surface group's parameters (one per row).
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// Off, Duplicate, Swap, Mute.
    pub flip: u8,
    /// Name or Value on the display.
    pub show_value: bool,
    /// Beats or SMPTE.
    pub smpte: bool,
    /// Arrange, All, Tracks, Single.
    pub view: u8,
    pub fader_bank: i32,
    /// What the V-Pots control: Volume, Pan.
    pub strip_param: u8,
    pub track_lock: Option<usize>,
    pub track_name_number: bool,
    pub page_by_one: bool,
    /// Coarse, Full, Fine.
    pub relative: u8,
    pub send_slot: i32,
}

impl Default for Group {
    fn default() -> Group {
        Group { flip: 0, show_value: false, smpte: false, view: 0, fader_bank: 0, strip_param: 1, track_lock: None, track_name_number: false, page_by_one: false, relative: 0, send_slot: 1 }
    }
}

pub const FLIP: [&str; 4] = ["Off", "Duplicate", "Swap", "Mute"];
pub const VIEWS: [&str; 4] = ["Arrange", "All", "Tracks", "Single"];
pub const STRIP_PARAMS: [&str; 8] = ["Volume", "Pan", "Format", "Input", "Output", "Automation", "Group", "Displayed Par."];

/// MCU state per device: what was sent (feedback is sent on change), modifiers held, V-Pot
/// assignment mode, faders touched.
#[derive(Clone, Debug, Default)]
struct Live {
    faders: [Option<u16>; 9],
    leds: BTreeMap<u8, u8>,
    lcd: String,
    tc: String,
    rings: [Option<u8>; 8],
    touched: [bool; 9],
    zoom: bool,
    hui_zone: u8,
    hui_hi: [u8; 8],
    last_ping: Option<Instant>,
}

pub struct State {
    pub devices: Vec<Device>,
    pub groups: Vec<Group>,
    pub assign: Assignments,
    live: Vec<Live>,
    pub setup_open: bool,
    install_open: bool,
    install_sel: Vec<usize>,
    pub assign_open: bool,
    pub expert: bool,
    pub learn: bool,
    /// What Learn assigns (Easy view's Channel Strip and Parameter).
    learn_class: Class,
    pub ignore_notes: bool,
    pub pinned_new: bool,
    pub selected: Option<usize>,
    sel_dev: Option<usize>,
    sel_zone: Option<String>,
    pub bypass: bool,
    pub link: bool,
    /// The last message in (port name, bytes) for the Input message field and Link.
    last_msg: Option<(String, Vec<u8>)>,
    /// A learned assignment waiting: the conflict dialog (Reassign / Keep both / Cancel) or
    /// the series dialog.
    pending: Option<Pending>,
    /// Modifiers held on Mackie surfaces (Shift, Control, Option, Command bits).
    mods: u8,
    loaded: bool,
    dirty_at: Option<Instant>,
    alert: Option<String>,
    pub ext: crate::surfext::Ext,
}

#[derive(Clone, Debug)]
enum Pending {
    Conflict(Assignment),
    Series(Assignment, Vec<Vec<cs::Tok>>),
}

impl Default for State {
    fn default() -> State {
        State { devices: Vec::new(), groups: vec![Group::default()], assign: Assignments::default(), live: Vec::new(), setup_open: false, install_open: false, install_sel: Vec::new(), assign_open: false, expert: false, learn: false, learn_class: Class::Strip { strip: Strip::Selected, param: StripParam::Volume }, ignore_notes: true, pinned_new: false, selected: None, sel_dev: None, sel_zone: None, bypass: false, link: false, last_msg: None, pending: None, mods: 0, loaded: false, dirty_at: None, alert: None, ext: Default::default() }
    }
}

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("control-surfaces.txt"))
}

fn load(s: &mut State) {
    s.loaded = true;
    if std::env::var_os("LG_SNAP").is_some() {
        return;
    }
    let Some(t) = file().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    let mut a_text = String::from("bramble-assignments 1\n");
    for l in t.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        match f.first().copied() {
            Some("device") if f.len() >= 11 => {
                let model = Model::ALL.iter().copied().find(|m| m.tag() == f[1]).unwrap_or(Model::Generic);
                s.devices.push(Device { model, in_port: f[2].into(), out_port: f[3].into(), row: f[4].parse().unwrap_or(0), col: f[5].parse().unwrap_or(0), color: u32::from_str_radix(f[6], 16).unwrap_or(0x3a8ee6), version: f[7].into(), transport_click: f[8] == "1", touchless: f[9] == "1", backlight_min: f[10].parse().unwrap_or(30) });
            }
            Some("group") if f.len() >= 12 => {
                let n = |i: usize| f[i].parse::<i32>().unwrap_or(0);
                s.groups.push(Group { flip: n(1) as u8, show_value: f[2] == "1", smpte: f[3] == "1", view: n(4) as u8, fader_bank: n(5), strip_param: n(6) as u8, track_lock: f[7].parse().ok(), track_name_number: f[8] == "1", page_by_one: f[9] == "1", relative: n(10) as u8, send_slot: n(11).max(1) });
            }
            Some("bypass") => s.bypass = f.get(1) == Some(&"1"),
            Some("gx") if f.len() >= 4 => {
                let g: usize = f[1].parse().unwrap_or(0);
                while s.ext.gx.len() <= g {
                    s.ext.gx.push(Default::default());
                }
                s.ext.gx[g].insert(f[2].to_string(), f[3].parse().unwrap_or(0));
            }
            Some("zone") | Some("a") => {
                a_text += l;
                a_text.push('\n');
            }
            _ => {}
        }
    }
    if s.groups.len() > 1 {
        s.groups.remove(0);
    }
    s.assign = Assignments::from_text(&a_text);
}

fn save(s: &State) {
    if std::env::var_os("LG_SNAP").is_some() {
        return;
    }
    let Some(f) = file() else { return };
    let mut t = String::from("bramble-control-surfaces 1\n");
    for d in &s.devices {
        t += &format!("device\t{}\t{}\t{}\t{}\t{}\t{:06x}\t{}\t{}\t{}\t{}\n", d.model.tag(), d.in_port, d.out_port, d.row, d.col, d.color, d.version, d.transport_click as u8, d.touchless as u8, d.backlight_min);
    }
    for g in &s.groups {
        t += &format!("group\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", g.flip, g.show_value as u8, g.smpte as u8, g.view, g.fader_bank, g.strip_param, g.track_lock.map_or(String::new(), |x| x.to_string()), g.track_name_number as u8, g.page_by_one as u8, g.relative, g.send_slot);
    }
    t += &format!("bypass\t{}\n", s.bypass as u8);
    for (g, m) in s.ext.gx.iter().enumerate() {
        for (k, v) in m {
            t += &format!("gx\t{g}\t{k}\t{v}\n");
        }
    }
    t += &s.assign.to_text().lines().skip(1).map(|l| format!("{l}\n")).collect::<String>();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, t);
}

fn changed(app: &mut App) {
    app.hub.surf.dirty_at = Some(Instant::now());
}

fn ensure(app: &mut App) {
    if !app.hub.surf.loaded {
        load(&mut app.hub.surf);
    }
    let n = app.hub.surf.devices.len();
    app.hub.surf.live.resize(n, Live::default());
    let rows = app.hub.surf.devices.iter().map(|d| d.row + 1).max().unwrap_or(1);
    while app.hub.surf.groups.len() < rows {
        app.hub.surf.groups.push(Group::default());
    }
}

/// The input port's name for an input index.
fn port_name(app: &App, port: usize) -> String {
    if port == usize::MAX {
        return "Bramble Studio Virtual In".into();
    }
    app.live.input_names.get(port).cloned().unwrap_or_default()
}

/// The strip's fader bank offset within its group: the faders of devices to its left.
fn device_offset(s: &State, d: usize) -> i32 {
    let dev = &s.devices[d];
    s.devices.iter().filter(|o| o.row == dev.row && o.col < dev.col).map(|o| o.model.faders()).sum()
}

// ---- MIDI in ----

pub fn midi_in(app: &mut App, ctx: &egui::Context, port: usize, bytes: &[u8]) -> bool {
    ensure(app);
    let name = port_name(app, port);
    app.hub.surf.last_msg = Some((name.clone(), bytes.to_vec()));
    // Learn: the message becomes an assignment (notes ignored unless asked)
    if app.hub.surf.learn && bytes[0] < 0xF0 {
        let note = matches!(bytes[0] & 0xF0, 0x80 | 0x90);
        if !(note && app.hub.surf.ignore_notes) {
            // (a note's release isn't a new control)
            if crate::surfext::relearn(app, bytes) {
                changed(app);
            } else if !(note && (bytes[0] & 0xF0 == 0x80 || bytes.get(2) == Some(&0))) {
                learn(app, ctx, &name, bytes);
            }
            return true;
        }
    }
    if app.hub.surf.bypass {
        return false;
    }
    // Active Sensing: taken by a Global Active Sense assignment
    if bytes[0] == 0xFE && app.hub.surf.assign.list.iter().any(|a| matches!(&a.class, Class::Global { param, .. } if param == "Active Sense")) {
        app.hub.surf.ext.sensed = Some(Instant::now());
        return true;
    }
    // Mackie Control device query reply: the device is there (serial / version)
    if bytes.len() > 6 && bytes[..5] == [0xF0, 0x00, 0x00, 0x66, 0x14] && bytes[5] == 0x01 {
        if let Some(d) = app.hub.surf.devices.iter_mut().find(|d| d.in_port == name) {
            d.version = String::from_utf8_lossy(&bytes[6..bytes.len() - 1]).trim().to_string();
        }
        return true;
    }
    // supported devices on this input
    if let Some(d) = app.hub.surf.devices.iter().position(|d| d.in_port == name && d.model != Model::Generic) {
        match app.hub.surf.devices[d].model {
            Model::Hui => hui_in(app, ctx, d, bytes),
            _ => mcu_in(app, ctx, d, bytes),
        }
        // (their learned / custom assignments too)
        assignments_in(app, ctx, &name, bytes);
        return true;
    }
    // controller assignments (remote control events stop here)
    assignments_in(app, ctx, &name, bytes)
}

fn learn(app: &mut App, ctx: &egui::Context, port: &str, bytes: &[u8]) {
    let msg = cs::learn_pattern(bytes);
    let mods = ctx.input(|i| (i.modifiers.shift as u8) | ((i.modifiers.ctrl as u8) << 1) | ((i.modifiers.alt as u8) << 2) | ((i.modifiers.command as u8) << 3));
    let mut class = app.hub.surf.learn_class.clone();
    // (new track assignments pinned: to the selected track)
    if app.hub.surf.pinned_new {
        if let Class::Strip { strip: Strip::Selected, param } = &class {
            class = Class::Strip { strip: Strip::Pinned(app.selected_track.unwrap_or(0) as i32), param: param.clone() };
        }
    }
    let mut a = Assignment::learned(port, msg.clone(), class);
    a.modifiers = mods;
    // learned in Expert view: the zone and mode selected
    if app.hub.surf.expert {
        if let Some(z) = app.hub.surf.sel_zone.clone() {
            a.mode = app.hub.surf.assign.active.get(&z).cloned().unwrap_or_default();
            a.zone = z;
        }
    }
    let s = &mut app.hub.surf;
    // the same control already active: Reassign / Keep both / Cancel
    let clash = (0..s.assign.list.len()).any(|i| s.assign.is_active(i) && s.assign.list[i].msg == msg && s.assign.list[i].port == port && s.assign.list[i].modifiers == mods);
    if clash {
        s.pending = Some(Pending::Conflict(a));
        return;
    }
    // a series: the previous learned assignment was the first of similar parameters
    if let (Some(prev), Class::Strip { strip: Strip::FaderBank(n2), param: p2 }) = (s.selected.and_then(|i| s.assign.list.get(i)).cloned(), &a.class) {
        if let Class::Strip { strip: Strip::FaderBank(n1), param: p1 } = &prev.class {
            if p1 == p2 && n2 > n1 && n2 - n1 >= 2 {
                if let Some(fill) = cs::fill_series(&prev.msg, &a.msg, (n2 - n1 + 1) as usize) {
                    s.pending = Some(Pending::Series(a, fill));
                    return;
                }
            }
        }
    }
    add_assignment(app, a);
    // a modifier held: releasing it ends Learn
    if mods != 0 {
        app.hub.surf.learn = false;
    }
}

fn add_assignment(app: &mut App, a: Assignment) {
    let name = a.short_param();
    let s = &mut app.hub.surf;
    s.assign.list.push(a);
    s.selected = Some(s.assign.list.len() - 1);
    changed(app);
    app.tell(format!("Learned: {name}"));
}

/// Incoming messages against the controller assignments; true when one took it.
fn assignments_in(app: &mut App, ctx: &egui::Context, port: &str, bytes: &[u8]) -> bool {
    let mods = app.hub.surf.mods | ctx.input(|i| (i.modifiers.shift as u8) | ((i.modifiers.ctrl as u8) << 1) | ((i.modifiers.alt as u8) << 2) | ((i.modifiers.command as u8) << 3));
    let mut hits = app.hub.surf.assign.matching(port, bytes, mods);
    if hits.is_empty() && mods != 0 {
        hits = app.hub.surf.assign.matching(port, bytes, 0);
    }
    if hits.is_empty() {
        return false;
    }
    if app.hub.surf.link {
        app.hub.surf.selected = Some(hits[0].0);
    }
    for (i, v) in hits {
        let a = app.hub.surf.assign.list[i].clone();
        crate::surfext::input(app, i, &a, v);
        // (Key Repeat: the press runs now, the release only stops repeating)
        if a.key_repeat && v != a.max {
            continue;
        }
        run_assignment(app, ctx, &a, v);
    }
    true
}

/// An assignment run with a value (Key Repeat).
pub fn run(app: &mut App, ctx: &egui::Context, a: &Assignment, v: i32) {
    run_assignment(app, ctx, a, v);
}

/// A channel strip parameter of a track set as an assignment says (automation groups).
pub fn run_on_track(app: &mut App, t: usize, param: &StripParam, a: &Assignment, v: i32) {
    let res = if a.global_resolution { 128.0 } else { a.resolution.max(1) as f32 };
    let cur = param_value(app, t, param);
    let steps = if param.is_switch() { 1.0 } else if *param == StripParam::Automation { 4.0 } else { res };
    if let Some(nv) = a.apply(v, cur, steps) {
        set_param(app, t, param, nv);
    }
}

/// A strip assignment's current value 0–1 (feedback).
pub fn strip_value(app: &App, strip: &Strip, param: &StripParam) -> Option<f32> {
    strip_track(app, strip).map(|t| param_value(app, t, param))
}

/// The track a channel strip assignment addresses.
fn strip_track(app: &App, strip: &Strip) -> Option<usize> {
    let p = app.project.as_ref()?;
    let n = p.tracks.len();
    let bank = app.hub.surf.groups.first().map_or(0, |g| g.fader_bank);
    let nth_of = |kind: Option<lg_core::TrackKind>, k: i32| -> Option<usize> { p.tracks.iter().enumerate().filter(|(_, t)| kind.is_none_or(|kk| t.kind == kk)).nth(k.max(0) as usize).map(|x| x.0) };
    let t = match strip {
        Strip::Selected => app.hub.surf.groups.first().and_then(|g| g.track_lock).or(app.selected_track)?,
        Strip::FaderBank(k) => (bank + k) as usize,
        Strip::Index(k) | Strip::Pinned(k) => *k as usize,
        Strip::Audio(k) => nth_of(Some(lg_core::TrackKind::Audio), *k)?,
        Strip::Instrument(k) => nth_of(Some(lg_core::TrackKind::Instrument), *k)?,
        Strip::Aux(_) | Strip::Output(_) | Strip::Master => return None,
    };
    (t < n).then_some(t)
}

/// A parameter's value 0–1 (for relative and toggle changes, and feedback).
fn param_value(app: &App, t: usize, p: &StripParam) -> f32 {
    let Some(tr) = app.project.as_ref().and_then(|pr| pr.tracks.get(t)) else { return 0.0 };
    match p {
        StripParam::Volume => crate::automation::db_to_logic(tr.volume_db) / 127.0,
        StripParam::Pan => (tr.pan + 1.0) / 2.0,
        StripParam::Mute => tr.mute as u8 as f32,
        StripParam::Solo => tr.solo as u8 as f32,
        StripParam::RecordEnable => app.armed.contains(&t) as u8 as f32,
        StripParam::Select => (app.selected_track == Some(t)) as u8 as f32,
        StripParam::Automation => app.auto_mode.get(&t).map_or(1, |m| crate::automation::AutoMode::ALL.iter().position(|x| x == m).unwrap_or(1)) as f32 / 4.0,
        StripParam::Send(k) => app.sends.get(&t).and_then(|v| v.get(*k as usize)).map_or(0.0, |s| crate::automation::db_to_logic(s.level_db) / 127.0),
        _ => 0.0,
    }
}

fn set_param(app: &mut App, t: usize, p: &StripParam, v: f32) {
    let v = v.clamp(0.0, 1.0);
    match p {
        StripParam::Volume | StripParam::Pan | StripParam::Mute | StripParam::Solo => {
            if let Some(tr) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)) {
                match p {
                    StripParam::Volume => tr.volume_db = crate::automation::logic_to_db(v * 127.0),
                    StripParam::Pan => tr.pan = v * 2.0 - 1.0,
                    StripParam::Mute => tr.mute = v > 0.5,
                    _ => tr.solo = v > 0.5,
                }
            }
            app.sync_mix();
        }
        StripParam::RecordEnable => {
            if v > 0.5 {
                app.armed.insert(t);
            } else {
                app.armed.remove(&t);
            }
        }
        StripParam::Select => {
            if v > 0.5 {
                app.selected_track = Some(t);
            }
        }
        StripParam::Automation => {
            let k = (v * 4.0).round() as usize;
            app.auto_mode.insert(t, crate::automation::AutoMode::ALL[k.min(4)]);
        }
        // a send's level (the send slot counted from the top)
        StripParam::Send(k) => {
            if let Some(s) = app.sends.get_mut(&t).and_then(|l| l.get_mut(*k as usize)) {
                s.level_db = crate::automation::logic_to_db(v * 127.0).min(6.0);
                app.dirty = true;
                app.load_engine();
            }
        }
        _ => {}
    }
}

fn run_assignment(app: &mut App, ctx: &egui::Context, a: &Assignment, v: i32) {
    let res = if a.global_resolution { app.logic_settings.get("Control Surfaces / General / Resolution of Relative Controls:").parse::<f32>().unwrap_or(128.0).max(1.0) } else { a.resolution.max(1) as f32 };
    match &a.class {
        Class::Strip { strip, param } => {
            let Some(t) = strip_track(app, strip) else { return };
            let cur = param_value(app, t, param);
            let steps = if param.is_switch() { 1.0 } else if *param == StripParam::Automation { 4.0 } else { res };
            if let Some(nv) = a.apply(v, cur, steps) {
                set_param(app, t, param, nv);
            }
        }
        Class::KeyCommand(c) => {
            if v != 0 || a.value_mode != ValueMode::Toggle {
                let _ = app.key_command(ctx, c);
            }
        }
        Class::Key(k) => {
            if v != 0 {
                if let Some(key) = egui::Key::from_name(k) {
                    ctx.input_mut(|i| i.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }));
                }
            }
        }
        Class::ModeChange { mode, how } => {
            let zone = a.zone.clone();
            if v != 0 {
                app.hub.surf.assign.change_mode(&zone, mode, *how, (v as f32 * a.multiply) as i32);
            }
        }
        Class::Global { param, number } => global(app, ctx, param, *number, v, a),
        Class::Group { param } => {
            let d = (v as f32 * a.multiply).round() as i32;
            const OWN: [&str; 7] = ["Flip Mode", "Display Mode", "Clock Display", "Channel Strip View Mode", "Channel Strip Parameter", "Track Lock", "Send Slot"];
            if param == "Fader Bank for Current View" {
                let nb = crate::surfext::current_bank(app, a, app.hub.surf.groups[0].fader_bank, d);
                let view = app.hub.surf.groups[0].view as usize;
                app.hub.surf.groups[0].fader_bank = nb;
                crate::surfext::set_gx(app, 0, &format!("Fader Bank for {} View", VIEWS[view.min(3)]), nb);
                return;
            }
            if !OWN.contains(&param.as_str()) {
                let _ = crate::surfext::group(app, param, a, v);
                return;
            }
            let g = &mut app.hub.surf.groups[0];
            match param.as_str() {
                "Flip Mode" => g.flip = if g.flip == 0 { 2 } else { 0 },
                "Display Mode" => g.show_value = !g.show_value,
                "Clock Display" => g.smpte = !g.smpte,
                "Channel Strip View Mode" => g.view = (g.view + 1) % 4,
                "Channel Strip Parameter" => g.strip_param = (g.strip_param + 1) % 2,
                "Track Lock" => g.track_lock = if g.track_lock.is_some() { None } else { app.selected_track },
                _ => g.send_slot = (g.send_slot + d).clamp(1, 8),
            }
        }
        Class::AutomationGroup { param, group } => crate::surfext::automation_group(app, a, param, *group, v),
    }
}

fn global(app: &mut App, ctx: &egui::Context, param: &str, number: i32, v: i32, a: &Assignment) {
    if crate::surfext::global(app, ctx, param, number, v, a) {
        return;
    }
    let d = (v as f32 * a.multiply) as i32;
    let bar = app.bar_ticks() as f64;
    match param {
        "Playhead" | "Playhead (Beats)" | "Playhead (Time Code)" | "Playhead (Beats, Scrubbing)" => {
            let step = if param.contains("Time Code") { app.tempo() / 60.0 * lg_core::events::PPQ as f64 / crate::projsettings::smpte(&app.proj).1 } else { lg_core::events::PPQ as f64 / 4.0 };
            let p = app.position();
            app.locate((p + d as f64 * step).max(0.0));
        }
        "Cycle" if v != 0 => app.toggle_cycle(),
        "Autopunch" if v != 0 => {
            let _ = app.key_command(ctx, "Toggle Autopunch");
        }
        "Go to Marker" if v != 0 => {
            let _ = app.key_command(ctx, &format!("Go to Marker Number {number}"));
        }
        "Move Locators" => {
            let (l, r) = app.cycle;
            let delta = (d as f64 * bar) as i64;
            let l2 = (l as i64 + delta).max(0) as u32;
            app.set_cycle((l2, l2 + (r - l)));
        }
        "Left Locator" => {
            let (l, r) = app.cycle;
            app.set_cycle(((l as i64 + d as i64 * bar as i64).max(0) as u32, r));
        }
        "Right Locator" => {
            let (l, r) = app.cycle;
            app.set_cycle((l, (r as i64 + d as i64 * bar as i64).max(l as i64 + 1) as u32));
        }
        "Horizontal Zoom" => {
            let _ = app.key_command(ctx, if d > 0 { "Zoom Horizontal In" } else { "Zoom Horizontal Out" });
        }
        "Vertical Zoom" => {
            let _ = app.key_command(ctx, if d > 0 { "Zoom Vertical In" } else { "Zoom Vertical Out" });
        }
        "Automation of all tracks" => {
            let k = (v.clamp(0, 4)) as usize;
            let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
            for t in 0..n {
                app.auto_mode.insert(t, crate::automation::AutoMode::ALL[k]);
            }
        }
        _ => {}
    }
}

// ---- Mackie Control ----

fn group_of(app: &App, d: usize) -> Group {
    let row = app.hub.surf.devices[d].row;
    app.hub.surf.groups.get(row).cloned().unwrap_or_default()
}

/// The tracks the connected control surfaces reach now, with the device's colour (control
/// surface bars in the Tracks area and Mixer).
pub fn controlled_tracks(app: &App) -> BTreeMap<usize, u32> {
    let mut m = BTreeMap::new();
    if app.hub.surf.bypass {
        return m;
    }
    for d in 0..app.hub.surf.devices.len() {
        if app.hub.surf.devices[d].model == Model::Generic {
            continue;
        }
        for k in 0..8u8 {
            if let Some(t) = strip_of(app, d, k) {
                m.entry(t).or_insert(app.hub.surf.devices[d].color);
            }
        }
    }
    m
}

fn strip_of(app: &App, d: usize, k: u8) -> Option<usize> {
    let g = group_of(app, d);
    // the group's Channel Strip View Mode: Arrange merges tracks on one channel strip, Tracks
    // shows each, All the strip types Filter for All View lets through, Single the selected one
    let p = app.project.as_ref()?;
    let tracks: Vec<(u32, u8)> = p.tracks.iter().map(|t| (t.extra.set.same_strip.unwrap_or(t.extra.id) as u32, if t.kind == lg_core::TrackKind::Audio { 2 } else { 3 })).collect();
    let row = app.hub.surf.devices[d].row;
    let filter = crate::surfext::gx(app, row, "Filter for All View") as u8;
    let sel = g.track_lock.or(app.selected_track);
    cs::view_strips(g.view, &tracks, sel, filter, g.fader_bank + device_offset(&app.hub.surf, d), k as usize + 1).get(k as usize).copied().flatten()
}

fn mcu_in(app: &mut App, ctx: &egui::Context, d: usize, bytes: &[u8]) {
    let Some(m) = cs::mcu_parse(bytes) else { return };
    let g = group_of(app, d);
    let row = app.hub.surf.devices[d].row;
    let pickup = app.logic_settings.get("Control Surfaces / General / Pickup mode") != "0";
    match m {
        Mcu::Fader(8, v) => {
            // the master fader: Studio's master level
            let _ = pickup;
            if let Ok(mut e) = app.engine.lock() {
                e.master = 10f32.powf(crate::automation::logic_to_db(v * 127.0) / 20.0);
            }
        }
        Mcu::Fader(k, v) => {
            if g.flip == 3 {
                return;
            }
            let Some(t) = strip_of(app, d, k) else { return };
            let p = if g.flip == 2 { StripParam::Pan } else { StripParam::Volume };
            set_param(app, t, &p, v);
            if g.flip == 1 {
                let q = if g.strip_param == 0 { StripParam::Volume } else { StripParam::Pan };
                set_param(app, t, &q, v);
            }
            app.hub.surf.live[d].faders[k as usize] = Some((v * 16383.0) as u16);
        }
        Mcu::VPot(k, n) if g.strip_param == 2 && g.flip != 2 => {
            let Some(t) = strip_of(app, d, k) else { return };
            let slot = (g.send_slot.max(1) - 1) as u8;
            match crate::surfext::gx(app, row, "Send Parameter") {
                // Position: Pre / Post
                2 => {
                    if let Some(s) = app.sends.get_mut(&t).and_then(|l| l.get_mut(slot as usize)) {
                        s.pre = n < 0;
                        app.load_engine();
                    }
                }
                _ => {
                    let p = StripParam::Send(slot);
                    let cur = param_value(app, t, &p);
                    set_param(app, t, &p, cur + n as f32 / 64.0);
                }
            }
        }
        Mcu::VPot(k, n) => {
            let Some(t) = strip_of(app, d, k) else { return };
            let p = if g.flip == 2 || g.strip_param == 0 { StripParam::Volume } else { StripParam::Pan };
            // Relative Change Mode: Coarse, Full (to the end, stopping at the default), Fine
            let step = match g.relative {
                1 => 1.0,
                2 => 1.0 / 1000.0,
                _ => 1.0 / 64.0,
            };
            let cur = param_value(app, t, &p);
            let mut nv = cur + n as f32 * step;
            if g.relative == 1 {
                let def = if p == StripParam::Pan { 0.5 } else { crate::automation::db_to_logic(0.0) / 127.0 };
                if (cur < def && nv > def) || (cur > def && nv < def) {
                    nv = def;
                }
            }
            set_param(app, t, &p, nv);
        }
        Mcu::Jog(n) => {
            let step = lg_core::events::PPQ as f64 / 4.0;
            let p = app.position();
            app.locate((p + n as f64 * step).max(0.0));
        }
        Mcu::Touch(k, on) => {
            app.hub.surf.live[d].touched[k.min(8) as usize] = on;
            if on && k < 8 && app.logic_settings.on("Control Surfaces / General / Touching fader selects track") {
                if let Some(t) = strip_of(app, d, k) {
                    app.selected_track = Some(t);
                }
            }
        }
        Mcu::Button(n, down) => {
            // modifiers held
            let bit = match n {
                mcu::SHIFT => 1,
                mcu::CONTROL => 2,
                mcu::OPTION => 4,
                mcu::CMD_ALT => 8,
                _ => 0,
            };
            if bit != 0 {
                if down { app.hub.surf.mods |= bit } else { app.hub.surf.mods &= !bit }
                return;
            }
            if !down {
                return;
            }
            let strip = |base: u8| (n >= base && n < base + 8).then(|| n - base);
            let grp = &mut app.hub.surf.groups[row];
            match n {
                mcu::BANK_LEFT => grp.fader_bank = (grp.fader_bank - 8).max(0),
                mcu::BANK_RIGHT => grp.fader_bank += 8,
                mcu::CHANNEL_LEFT => grp.fader_bank = (grp.fader_bank - 1).max(0),
                mcu::CHANNEL_RIGHT => grp.fader_bank += 1,
                mcu::FLIP => grp.flip = if grp.flip == 0 { 2 } else { 0 },
                mcu::NAME_VALUE => grp.show_value = !grp.show_value,
                mcu::SMPTE_BEATS => grp.smpte = !grp.smpte,
                mcu::ASSIGN_PAN => grp.strip_param = 1,
                // Send: the V-Pots control the group's Send Slot (Send Parameter: Level, Position)
                mcu::ASSIGN_SEND => grp.strip_param = 2,
                mcu::ASSIGN_TRACK => grp.strip_param = 0,
                _ => {}
            }
            let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len()) as i32;
            let grp = &mut app.hub.surf.groups[row];
            grp.fader_bank = grp.fader_bank.min((n_tracks - 1).max(0));
            if let Some(k) = strip(mcu::REC) {
                if let Some(t) = strip_of(app, d, k) {
                    let v = param_value(app, t, &StripParam::RecordEnable);
                    set_param(app, t, &StripParam::RecordEnable, 1.0 - v);
                }
            } else if let Some(k) = strip(mcu::SOLO) {
                if let Some(t) = strip_of(app, d, k) {
                    let v = param_value(app, t, &StripParam::Solo);
                    set_param(app, t, &StripParam::Solo, 1.0 - v);
                }
            } else if let Some(k) = strip(mcu::MUTE) {
                if let Some(t) = strip_of(app, d, k) {
                    let v = param_value(app, t, &StripParam::Mute);
                    set_param(app, t, &StripParam::Mute, 1.0 - v);
                }
            } else if let Some(k) = strip(mcu::SELECT) {
                if let Some(t) = strip_of(app, d, k) {
                    app.selected_track = Some(t);
                }
            } else if let Some(k) = strip(mcu::VPOT_PUSH) {
                // a V-Pot pushed: back to the parameter's default
                if let Some(t) = strip_of(app, d, k) {
                    let p = if app.hub.surf.groups[row].strip_param == 0 { StripParam::Volume } else { StripParam::Pan };
                    let def = if p == StripParam::Pan { 0.5 } else { crate::automation::db_to_logic(0.0) / 127.0 };
                    set_param(app, t, &p, def);
                }
            } else if (mcu::F1..mcu::F8).contains(&n) {
                // F1–F7: screensets 1–7
                let _ = app.key_command(ctx, &format!("Recall Screenset {}", n - mcu::F1 + 1));
            } else {
                let cmd = match n {
                    mcu::F8 => Some("Close Window F8"),
                    mcu::PLAY => Some("Play"),
                    mcu::STOP => Some("Stop"),
                    mcu::RECORD => Some("Record"),
                    mcu::CYCLE => Some("Cycle Mode"),
                    mcu::CLICK => Some("Toggle Metronome Click"),
                    mcu::UNDO => Some("Undo"),
                    mcu::SAVE => Some("Save"),
                    mcu::MARKER => Some("Create Marker"),
                    mcu::REWIND => Some("Rewind"),
                    mcu::FORWARD => Some("Forward"),
                    mcu::UP => Some(if app.hub.surf.live[d].zoom { "Zoom Vertical Out" } else { "Select Previous Track" }),
                    mcu::DOWN => Some(if app.hub.surf.live[d].zoom { "Zoom Vertical In" } else { "Select Next Track" }),
                    mcu::LEFT => Some(if app.hub.surf.live[d].zoom { "Zoom Horizontal Out" } else { "Rewind" }),
                    mcu::RIGHT => Some(if app.hub.surf.live[d].zoom { "Zoom Horizontal In" } else { "Forward" }),
                    mcu::READ | mcu::WRITE | mcu::TOUCH | mcu::LATCH | mcu::TRIM => {
                        if let Some(t) = app.selected_track {
                            let m = match n {
                                mcu::READ => crate::automation::AutoMode::Read,
                                mcu::WRITE => crate::automation::AutoMode::Write,
                                mcu::TOUCH => crate::automation::AutoMode::Touch,
                                mcu::LATCH => crate::automation::AutoMode::Latch,
                                _ => crate::automation::AutoMode::Off,
                            };
                            app.auto_mode.insert(t, m);
                        }
                        None
                    }
                    mcu::ZOOM => {
                        app.hub.surf.live[d].zoom = !app.hub.surf.live[d].zoom;
                        None
                    }
                    _ => None,
                };
                match cmd {
                    Some("Play") => {
                        if !app.playing() {
                            app.play_stop();
                        }
                    }
                    Some("Stop") => app.stop(),
                    Some("Cycle Mode") => app.toggle_cycle(),
                    Some("Undo") => app.undo(),
                    Some("Close Window F8") => close_top_window(app),
                    Some(c) => {
                        let _ = app.key_command(ctx, c);
                    }
                    None => {}
                }
            }
        }
    }
}

/// Mackie F8: the topmost of Studio's floating windows closes.
fn close_top_window(app: &mut App) {
    let s = &mut app.hub.surf;
    if s.assign_open {
        s.assign_open = false;
    } else if s.setup_open {
        s.setup_open = false;
    } else if app.hub.env.sub.is_some() {
        app.hub.env.sub = None;
    } else if app.hub.env.wins[1].open {
        app.hub.env.wins[1].open = false;
    } else if app.hub.env.wins[0].open {
        app.hub.env.wins[0].open = false;
    } else if app.hub.movie.window {
        app.hub.movie.window = false;
    }
}

fn hui_in(app: &mut App, ctx: &egui::Context, d: usize, bytes: &[u8]) {
    let live = &mut app.hub.surf.live[d];
    let Some(h) = cs::hui_parse(bytes, &mut live.hui_zone, &mut live.hui_hi) else { return };
    match h {
        Hui::Ping => {}
        Hui::Fader(k, v) => {
            if let Some(t) = strip_of(app, d, k) {
                set_param(app, t, &StripParam::Volume, v);
            }
        }
        Hui::VPot(k, n) => {
            if let Some(t) = strip_of(app, d, k) {
                let cur = param_value(app, t, &StripParam::Pan);
                set_param(app, t, &StripParam::Pan, cur + n as f32 / 64.0);
            }
        }
        Hui::Switch(zone, port, true) if zone < 8 => {
            let Some(t) = strip_of(app, d, zone) else { return };
            let p = match port {
                1 => StripParam::Select,
                2 => StripParam::Mute,
                3 => StripParam::Solo,
                7 => StripParam::RecordEnable,
                _ => return,
            };
            let v = param_value(app, t, &p);
            set_param(app, t, &p, if p == StripParam::Select { 1.0 } else { 1.0 - v });
        }
        Hui::Switch(cs::HUI_TRANSPORT, port, true) => match port {
            1 => {
                let _ = app.key_command(ctx, "Rewind");
            }
            2 => {
                let _ = app.key_command(ctx, "Forward");
            }
            3 => app.stop(),
            4 => {
                if !app.playing() {
                    app.play_stop();
                }
            }
            5 => {
                let _ = app.key_command(ctx, "Record");
            }
            _ => {}
        },
        _ => {}
    }
}

// ---- feedback ----

pub fn frame(app: &mut App, ctx: &egui::Context) {
    ensure(app);
    // saved a moment after changes (and so on quit)
    if app.hub.surf.dirty_at.is_some_and(|t| t.elapsed() > Duration::from_millis(500)) {
        app.hub.surf.dirty_at = None;
        save(&app.hub.surf);
    }
    if app.hub.surf.bypass {
        return;
    }
    crate::surfext::frame(app, ctx);
    if app.hub.surf.devices.is_empty() {
        return;
    }
    let focused = ctx.input(|i| i.focused);
    if !focused && app.logic_settings.on("Control Surfaces / General / Bypass all while in background") {
        return;
    }
    for d in 0..app.hub.surf.devices.len() {
        let dev = app.hub.surf.devices[d].clone();
        if dev.out_port.is_empty() || dev.model == Model::Generic {
            continue;
        }
        let g = group_of(app, d);
        let mut out: Vec<Vec<u8>> = Vec::new();
        let ext = dev.model == Model::MackieExtender;
        let mut names = String::new();
        let mut values = String::new();
        for k in 0..8u8 {
            let t = strip_of(app, d, k);
            let tr = t.and_then(|t| app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|x| (t, x.clone())));
            // motor faders (not while touched, not in Flip Mute)
            let fv = match &tr {
                Some((t, _)) if g.flip != 3 => param_value(app, *t, if g.flip == 2 { &StripParam::Pan } else { &StripParam::Volume }),
                _ => 0.0,
            };
            let x = (fv * 16383.0).round() as u16;
            let live = &mut app.hub.surf.live[d];
            if !live.touched[k as usize] && !dev.touchless && live.faders[k as usize].is_none_or(|old| (old as i32 - x as i32).abs() > 8) {
                live.faders[k as usize] = Some(x);
                match dev.model {
                    Model::Hui => out.extend(cs::hui_fader(k, fv).iter().map(|m| m.to_vec())),
                    _ => out.push(cs::mcu_fader(k, fv).to_vec()),
                }
            }
            // LEDs: rec, solo, mute, select
            let flash = app.logic_settings.get("Control Surfaces / General / Flash Mute and Solo buttons") != "0";
            let (rec, solo, mute, sel) = tr.as_ref().map_or((false, false, false, false), |(t, x)| (app.armed.contains(t), x.solo, x.mute, app.selected_track == Some(*t)));
            let states = [(mcu::REC + k, rec as u8), (mcu::SOLO + k, if solo && flash { 2 } else { solo as u8 }), (mcu::MUTE + k, if mute && flash { 2 } else { mute as u8 }), (mcu::SELECT + k, sel as u8)];
            for (note, st) in states {
                let live = &mut app.hub.surf.live[d];
                if live.leds.get(&note) != Some(&st) {
                    live.leds.insert(note, st);
                    match dev.model {
                        Model::Hui => {
                            let port = match note - k {
                                mcu::REC => 7,
                                mcu::SOLO => 3,
                                mcu::MUTE => 2,
                                _ => 1,
                            };
                            out.extend(cs::hui_led(k, port, st > 0).iter().map(|m| m.to_vec()));
                        }
                        _ => out.push(cs::mcu_led(note, st).to_vec()),
                    }
                }
            }
            // V-Pot rings: pan from the centre, volume as a bar
            if dev.model != Model::Hui {
                let pan = tr.as_ref().map_or(0.5, |(t, _)| param_value(app, *t, &StripParam::Pan));
                let ring = if g.strip_param == 1 && g.flip != 2 { cs::mcu_ring(k, 1, pan, false) } else { cs::mcu_ring(k, 2, fv, false) };
                let live = &mut app.hub.surf.live[d];
                if live.rings[k as usize] != Some(ring[2]) {
                    live.rings[k as usize] = Some(ring[2]);
                    out.push(ring.to_vec());
                }
            }
            let name = tr.as_ref().map_or(String::new(), |(t, x)| if g.track_name_number { format!("{}{}", t + 1, x.name) } else { x.name.clone() });
            names += &cs::lcd_cell(&name);
            let val = tr.as_ref().map_or(String::new(), |(_, x)| if g.strip_param == 1 { format!("{:+.0}", x.pan * 64.0) } else { format!("{:.1}", x.volume_db) });
            values += &cs::lcd_cell(&val);
        }
        // the LCD: names above; values below (Display Mode Value: both rows show values)
        if dev.model != Model::Hui {
            let lcd = if g.show_value { format!("{values}{values}") } else { format!("{names}{values}") };
            if app.hub.surf.live[d].lcd != lcd {
                app.hub.surf.live[d].lcd = lcd.clone();
                out.push(cs::mcu_lcd(0, &lcd, ext));
            }
            if !ext {
                // transport LEDs, Flip, SMPTE / Beats, and the time display
                let (playing, recording, cycle, click) = (app.playing(), app.recording(), app.cycle_on(), app.engine.lock().map(|e| e.metronome).unwrap_or(false));
                for (note, st) in [(mcu::PLAY, playing as u8), (mcu::STOP, (!playing) as u8), (mcu::RECORD, recording as u8), (mcu::CYCLE, cycle as u8), (mcu::CLICK, click as u8), (mcu::FLIP, (g.flip != 0) as u8), (mcu::SMPTE_LED, g.smpte as u8), (mcu::BEATS_LED, (!g.smpte) as u8), (mcu::ZOOM, app.hub.surf.live[d].zoom as u8)] {
                    let live = &mut app.hub.surf.live[d];
                    if live.leds.get(&note) != Some(&st) {
                        live.leds.insert(note, st);
                        out.push(cs::mcu_led(note, st).to_vec());
                    }
                }
                let tc = time_text(app, g.smpte);
                if app.hub.surf.live[d].tc != tc {
                    app.hub.surf.live[d].tc = tc.clone();
                    out.extend(cs::mcu_timecode(&tc).iter().map(|m| m.to_vec()));
                }
            }
        } else {
            // HUI: ping once a second
            let live = &mut app.hub.surf.live[d];
            if live.last_ping.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
                live.last_ping = Some(Instant::now());
                out.push(cs::HUI_PING.to_vec());
            }
        }
        for m in out {
            app.hub.outs.send(&dev.out_port, &m);
        }
    }
    if app.playing() {
        ctx.request_repaint_after(Duration::from_millis(30));
    }
}

/// The position as the Mackie display shows it: bars beats sub-divisions ticks, or SMPTE.
fn time_text(app: &App, smpte: bool) -> String {
    let pos = app.position();
    if smpte {
        let (off, fps, drop) = crate::projsettings::smpte(&app.proj);
        let tc = lg_core::sync::Tc::from_secs(app.seconds_at(pos) + off, lg_core::sync::Rate { fps, drop });
        format!("{:02}{:02}{:02}{:02}", tc.h, tc.m, tc.s, tc.f)
    } else {
        let bar = app.bar_ticks() as f64;
        let ppq = lg_core::events::PPQ as f64;
        let b = (pos / bar).floor() as i64 + 1;
        let beat = ((pos % bar) / ppq).floor() as i64 + 1;
        let sub = ((pos % ppq) / (ppq / 4.0)).floor() as i64 + 1;
        let tick = (pos % (ppq / 4.0)).floor() as i64;
        format!("{b:>3}{beat:>2}{sub:>2}{tick:>3}")
    }
}

// ---- commands ----

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    ensure(app);
    let _ = ctx;
    match name {
        "Control Surfaces Setup…" | "Setup…" => app.hub.surf.setup_open = true,
        "Controller Assignments…" | "Open Controller Assignments" => app.hub.surf.assign_open = true,
        n if n == "Learn new Controller Assignment" || n.starts_with("Learn Assignment for") => {
            let s = &mut app.hub.surf;
            s.assign_open = true;
            s.expert = false;
            s.learn = !s.learn;
        }
        "Rebuild Defaults" => {
            let s = &mut app.hub.surf;
            for g in s.groups.iter_mut() {
                *g = Group::default();
            }
            for l in s.live.iter_mut() {
                *l = Live::default();
            }
            // Mackie devices without ports: found again by name
            scan(app, None);
            changed(app);
            app.tell("Control surface support rebuilt");
        }
        "Bypass All Control Surfaces" | "Toggle Bypass All Control Surfaces" => {
            app.hub.surf.bypass = !app.hub.surf.bypass;
            changed(app);
        }
        "Open Control Surfaces Settings…" | "Settings…" => {
            // (the Logic Pro ▸ Control Surfaces ▸ Settings… item and its key command)
            app.settings_tab = "Control Surfaces".into();
            app.settings_sub = "General".into();
            app.show_settings = true;
        }
        _ => return false,
    }
    true
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    match label {
        "Bypass All Control Surfaces" => Some(app.hub.surf.bypass),
        _ => None,
    }
}

pub fn key_press(app: &mut App, _ctx: &egui::Context, key: egui::Key, m: egui::Modifiers) -> bool {
    let s = &mut app.hub.surf;
    if !s.assign_open {
        return false;
    }
    // ⌘L toggles Learn; Delete removes the selected assignment
    if key == egui::Key::L && m.command {
        s.learn = !s.learn;
        return true;
    }
    if matches!(key, egui::Key::Delete | egui::Key::Backspace) && !m.any() {
        if let Some(i) = s.selected.filter(|i| *i < s.assign.list.len()) {
            s.assign.list.remove(i);
            s.selected = None;
            changed(app);
            return true;
        }
    }
    false
}

/// Finds control surfaces on the MIDI ports by name (and asks Mackie devices to answer):
/// Mackie Control / Logic Control / X-Touch, extenders (XT), HUI. `only`: these models.
fn scan(app: &mut App, only: Option<&[Model]>) -> usize {
    let ins = app.live.input_names.clone();
    let outs = app.hub.outs.ports();
    let mut found = 0;
    for name in &ins {
        let low = name.to_lowercase();
        let model = if low.contains("hui") {
            Model::Hui
        } else if low.contains(" xt") || low.contains("extender") || low.contains("ext ") {
            Model::MackieExtender
        } else if low.contains("mackie") || low.contains("mcu") || low.contains("x-touch") || low.contains("logic control") || low.contains("faderport") {
            Model::MackieControl
        } else {
            continue;
        };
        if only.is_some_and(|o| !o.contains(&model)) {
            continue;
        }
        if app.hub.surf.devices.iter().any(|d| &d.in_port == name) {
            continue;
        }
        // the output with the same name (or the closest)
        let out = outs.iter().find(|o| *o == name).or_else(|| outs.iter().find(|o| o.to_lowercase().split_whitespace().next() == low.split_whitespace().next())).cloned().unwrap_or_default();
        let col = app.hub.surf.devices.iter().filter(|d| d.row == 0).count();
        app.hub.surf.devices.push(Device { model, in_port: name.clone(), out_port: out.clone(), row: 0, col, color: 0x3a8ee6, version: String::new(), transport_click: false, touchless: false, backlight_min: 30 });
        if model != Model::Hui && !out.is_empty() {
            app.hub.outs.send(&out, &[0xF0, 0x00, 0x00, 0x66, if model == Model::MackieExtender { 0x15 } else { 0x14 }, 0x00, 0xF7]);
        }
        found += 1;
    }
    ensure(app);
    changed(app);
    found
}

// ---- windows ----

pub fn windows(app: &mut App, ctx: &egui::Context) {
    ensure(app);
    if app.hub.surf.setup_open {
        setup_window(app, ctx);
    }
    if app.hub.surf.install_open {
        install_window(app, ctx);
    }
    if app.hub.surf.assign_open {
        assign_window(app, ctx);
    }
    pending_dialog(app, ctx);
    if let Some(msg) = app.hub.surf.alert.clone() {
        let mut close = false;
        egui::Window::new("Control Surfaces").id(egui::Id::unique("cs-alert")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
            ui.label(msg);
            if ui.button("OK").clicked() {
                close = true;
            }
        });
        if close {
            app.hub.surf.alert = None;
        }
    }
}

fn col32(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// The Control Surfaces Setup window: the device and group parameters on the left, the device
/// area (one row per group) on the right; New ▸ Install… / Scan All, Edit ▸ Delete.
fn setup_window(app: &mut App, ctx: &egui::Context) {
    let mut open = true;
    egui::Window::new("Control Surfaces Setup").id(egui::Id::unique("cs-setup")).collapsible(false).open(&mut open).resizable(true).default_size(vec2(760.0, 460.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.menu_button("New ⌄", |ui| {
                if ui.button("Install…").clicked() {
                    app.hub.surf.install_open = true;
                    ui.close();
                }
                if ui.button("Scan All").clicked() {
                    let n = scan(app, None);
                    app.tell(if n == 0 { "No control surfaces found on the MIDI ports".to_string() } else { format!("{n} control surface{} found", if n == 1 { "" } else { "s" }) });
                    ui.close();
                }
            });
            ui.menu_button("Edit ⌄", |ui| {
                let on = app.hub.surf.sel_dev.is_some();
                if ui.add_enabled(on, egui::Button::new("Delete")).clicked() {
                    if let Some(d) = app.hub.surf.sel_dev.take() {
                        app.hub.surf.devices.remove(d);
                        app.hub.surf.live = Vec::new();
                        changed(app);
                    }
                    ui.close();
                }
            });
            let mut b = app.hub.surf.bypass;
            if ui.checkbox(&mut b, "Bypass All Control Surfaces").changed() {
                app.hub.surf.bypass = b;
                changed(app);
            }
        });
        ui.separator();
        let avail = ui.available_rect_before_wrap();
        let left = Rect::from_min_size(avail.min, vec2(300.0, avail.height()));
        let right = Rect::from_min_max(pos2(left.max.x + 8.0, avail.min.y), avail.max);
        ui.allocate_rect(avail, Sense::hover());
        let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left));
        egui::ScrollArea::vertical().id_salt("cs-insp").show(&mut lui, |ui| setup_inspector(app, ui));
        device_area(app, ui, right);
    });
    if !open {
        app.hub.surf.setup_open = false;
    }
}

fn setup_inspector(app: &mut App, ui: &mut egui::Ui) {
    let Some(d) = app.hub.surf.sel_dev.filter(|d| *d < app.hub.surf.devices.len()) else {
        ui.label(egui::RichText::new("Select a device in the device area").color(look::text_dim()));
        return;
    };
    let ins = app.live.input_names.clone();
    let outs = app.hub.outs.ports();
    let mut dev = app.hub.surf.devices[d].clone();
    let before = dev.clone();
    egui::CollapsingHeader::new(format!("Device: {}", dev.model.name())).default_open(true).show(ui, |ui| {
        egui::Grid::new("cs-dev").num_columns(2).show(ui, |ui| {
            ui.label("Output Port:");
            egui::ComboBox::from_id_salt("cs-out").selected_text(if dev.out_port.is_empty() { "Off".to_string() } else { dev.out_port.clone() }).width(170.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut dev.out_port, String::new(), "Off");
                for p in &outs {
                    ui.selectable_value(&mut dev.out_port, p.clone(), p);
                }
            });
            ui.end_row();
            ui.label("Input Port:");
            egui::ComboBox::from_id_salt("cs-in").selected_text(if dev.in_port.is_empty() { "Off".to_string() } else { dev.in_port.clone() }).width(170.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut dev.in_port, String::new(), "Off");
                for p in &ins {
                    ui.selectable_value(&mut dev.in_port, p.clone(), p);
                }
            });
            ui.end_row();
            ui.label("Module:");
            ui.label(dev.model.module());
            ui.end_row();
            ui.label("Model:");
            ui.label(dev.model.name());
            ui.end_row();
            ui.label("Version:");
            ui.label(if dev.version.is_empty() { "—".to_string() } else { dev.version.clone() });
            ui.end_row();
            ui.label("Color:");
            let mut c = [(dev.color >> 16) as u8, (dev.color >> 8) as u8, dev.color as u8];
            if ui.color_edit_button_srgb(&mut c).changed() {
                dev.color = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
            }
            ui.end_row();
            ui.label("Fader Bank Offset:");
            ui.label(format!("{}", device_offset(&app.hub.surf, d)));
            ui.end_row();
            if matches!(dev.model, Model::MackieControl | Model::MackieExtender) {
                ui.label("Transport button click:");
                ui.checkbox(&mut dev.transport_click, "");
                ui.end_row();
                ui.label("Touchless moveable faders:");
                ui.checkbox(&mut dev.touchless, "");
                ui.end_row();
                ui.label("Backlight Saver Timeout:");
                ui.add(egui::DragValue::new(&mut dev.backlight_min).range(0..=60).suffix(" min"));
                ui.end_row();
            }
        });
    });
    if dev != before {
        if dev.in_port != before.in_port {
            // (the assignments learned on that input follow it)
            for a in app.hub.surf.assign.list.iter_mut().filter(|a| a.port == before.in_port) {
                a.port = dev.in_port.clone();
            }
        }
        app.hub.surf.devices[d] = dev.clone();
        changed(app);
    }
    let row = dev.row;
    let mut g = app.hub.surf.groups.get(row).cloned().unwrap_or_default();
    let gb = g.clone();
    egui::CollapsingHeader::new(format!("Control Surface Group {}", row + 1)).default_open(true).show(ui, |ui| {
        egui::Grid::new("cs-grp").num_columns(2).show(ui, |ui| {
            let pop = |ui: &mut egui::Ui, id: &str, v: &mut u8, opts: &[&str]| {
                egui::ComboBox::from_id_salt(id).selected_text(opts[*v as usize]).show_ui(ui, |ui| {
                    for (k, o) in opts.iter().enumerate() {
                        ui.selectable_value(v, k as u8, *o);
                    }
                });
            };
            ui.label("Flip Mode:");
            pop(ui, "cs-flip", &mut g.flip, &FLIP);
            ui.end_row();
            ui.label("Display Mode:");
            let mut dm = g.show_value as u8;
            pop(ui, "cs-disp", &mut dm, &["Name", "Value"]);
            g.show_value = dm == 1;
            ui.end_row();
            ui.label("Clock Display:");
            let mut cd = g.smpte as u8;
            pop(ui, "cs-clock", &mut cd, &["Beats", "SMPTE"]);
            g.smpte = cd == 1;
            ui.end_row();
            ui.label("Channel Strip View Mode:");
            pop(ui, "cs-view", &mut g.view, &VIEWS);
            ui.end_row();
            ui.label("Fader Bank for Tracks View:");
            ui.add(egui::DragValue::new(&mut g.fader_bank).range(0..=999));
            ui.end_row();
            ui.label("Channel Strip Parameter:");
            pop(ui, "cs-param", &mut g.strip_param, &STRIP_PARAMS[..2]);
            ui.end_row();
            ui.label("Send Slot:");
            ui.add(egui::DragValue::new(&mut g.send_slot).range(1..=8));
            ui.end_row();
            ui.label("Track Lock:");
            let mut lock = g.track_lock.is_some();
            if ui.checkbox(&mut lock, "").changed() {
                g.track_lock = if lock { app.selected_track } else { None };
            }
            ui.end_row();
            ui.label("Track Name Format:");
            let mut tn = g.track_name_number as u8;
            pop(ui, "cs-tn", &mut tn, &["Name", "Number and Name"]);
            g.track_name_number = tn == 1;
            ui.end_row();
            ui.label("Parameter Page Shift Mode:");
            let mut ps = g.page_by_one as u8;
            pop(ui, "cs-ps", &mut ps, &["By Page", "By One"]);
            g.page_by_one = ps == 1;
            ui.end_row();
            ui.label("Relative Change Mode:");
            pop(ui, "cs-rel", &mut g.relative, &["Coarse", "Full", "Fine"]);
            ui.end_row();
        });
    });
    if g != gb {
        if let Some(x) = app.hub.surf.groups.get_mut(row) {
            *x = g;
        }
        changed(app);
    }
}

/// The device area: one row of icons per group; drag an icon to another row or place.
fn device_area(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, Color32::from_gray(32));
    let rows = app.hub.surf.groups.len().max(app.hub.surf.devices.iter().map(|d| d.row + 1).max().unwrap_or(0)) + 1;
    let row_h = 86.0;
    for k in 0..rows.min(20) {
        let y = r.min.y + 8.0 + k as f32 * row_h;
        p.text(pos2(r.min.x + 6.0, y + 4.0), Align2::LEFT_TOP, format!("Group {}", k + 1), look::regular(10.0), Color32::from_gray(120));
        p.hline(r.x_range(), y + row_h - 4.0, Stroke::new(1.0, Color32::from_gray(48)));
    }
    let drag_key = egui::Id::unique("cs-drag");
    let dragging: Option<usize> = ui.data(|d| d.get_temp(drag_key));
    for (i, d) in app.hub.surf.devices.clone().iter().enumerate() {
        let ir = Rect::from_min_size(pos2(r.min.x + 70.0 + d.col as f32 * 92.0, r.min.y + 18.0 + d.row as f32 * row_h), vec2(80.0, 56.0));
        let resp = ui.interact(ir, egui::Id::unique(("cs-dev", i)), Sense::click_and_drag());
        let sel = app.hub.surf.sel_dev == Some(i);
        p.rect_filled(ir, 6.0, if sel { look::accent().gamma_multiply(0.5) } else { Color32::from_gray(58) });
        p.rect_filled(Rect::from_min_size(ir.min, vec2(ir.width(), 5.0)), 2.0, col32(d.color));
        crate::glyphs::paint(&p, Rect::from_center_size(ir.center() - vec2(0.0, 8.0), vec2(26.0, 26.0)), if d.model == Model::Generic { crate::glyphs::ph::PIANO_KEYS } else { crate::glyphs::ph::SLIDERS }, 22.0, Color32::WHITE);
        p.text(pos2(ir.center().x, ir.max.y - 8.0), Align2::CENTER_CENTER, d.model.name(), look::regular(9.5), Color32::WHITE);
        if resp.clicked() || resp.drag_started() {
            app.hub.surf.sel_dev = Some(i);
        }
        if resp.drag_started() {
            ui.data_mut(|x| x.insert_temp(drag_key, i));
        }
    }
    if let Some(i) = dragging {
        if ui.input(|x| x.pointer.any_released()) {
            if let Some(pp) = ui.input(|x| x.pointer.latest_pos()).filter(|pp| r.contains(*pp)) {
                let row = (((pp.y - r.min.y - 8.0) / row_h).floor().max(0.0) as usize).min(19);
                let col = ((pp.x - r.min.x - 70.0) / 92.0).round().max(0.0) as usize;
                let s = &mut app.hub.surf;
                s.devices[i].row = row;
                // (left-to-right order: the others in that row shift)
                let mut order: Vec<usize> = (0..s.devices.len()).filter(|k| s.devices[*k].row == row && *k != i).collect();
                order.sort_by_key(|k| s.devices[*k].col);
                order.insert(col.min(order.len()), i);
                for (c, k) in order.into_iter().enumerate() {
                    s.devices[k].col = c;
                }
                // tidy the rows left behind
                let max_row = s.devices.iter().map(|d| d.row).max().unwrap_or(0);
                for rr in 0..=max_row {
                    let mut o: Vec<usize> = (0..s.devices.len()).filter(|k| s.devices[*k].row == rr).collect();
                    o.sort_by_key(|k| s.devices[*k].col);
                    for (c, k) in o.into_iter().enumerate() {
                        s.devices[k].col = c;
                    }
                }
                ensure(app);
                changed(app);
            }
            ui.data_mut(|x| x.remove::<usize>(drag_key));
        }
    }
    if app.hub.surf.devices.is_empty() {
        p.text(r.center(), Align2::CENTER_CENTER, "No control surfaces: New ▸ Install… or Scan All", look::regular(12.0), look::text_dim());
    }
}

/// New ▸ Install…: the supported models; Scan finds them on the ports, Add adds one to set up
/// by hand.
fn install_window(app: &mut App, ctx: &egui::Context) {
    let mut open = true;
    egui::Window::new("Install").id(egui::Id::unique("cs-install")).open(&mut open).resizable(false).show(ctx, |ui| {
        ui.label("Manufacturer / Model");
        for (k, m) in Model::ALL.iter().enumerate() {
            let on = app.hub.surf.install_sel.contains(&k);
            let r = ui.selectable_label(on, format!("{} — {}", if *m == Model::Generic { "Generic" } else { "Mackie Designs" }, m.name()));
            if r.clicked() {
                let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                let s = &mut app.hub.surf.install_sel;
                if cmd {
                    if on { s.retain(|x| *x != k) } else { s.push(k) }
                } else {
                    *s = vec![k];
                }
            }
            if r.double_clicked() {
                app.hub.surf.install_sel = vec![k];
                let n = scan(app, Some(&[*m]));
                app.tell(format!("Scan: {n} found"));
            }
        }
        ui.horizontal(|ui| {
            let sel: Vec<Model> = app.hub.surf.install_sel.iter().map(|k| Model::ALL[*k]).collect();
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("Scan")).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter)) && !sel.is_empty()) {
                let n = scan(app, Some(&sel));
                app.tell(format!("Scan: {n} found"));
            }
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("Add")).clicked() {
                for m in sel {
                    if app.hub.surf.devices.iter().any(|d| d.model == m) {
                        app.hub.surf.alert = Some(format!("A {} is in the setup already: another one was added. Set its MIDI In and Out ports in the Device parameters.", m.name()));
                    }
                    let col = app.hub.surf.devices.iter().filter(|d| d.row == 0).count();
                    app.hub.surf.devices.push(Device { model: m, in_port: String::new(), out_port: String::new(), row: 0, col, color: 0x3a8ee6, version: String::new(), transport_click: false, touchless: false, backlight_min: 30 });
                    app.hub.surf.sel_dev = Some(app.hub.surf.devices.len() - 1);
                }
                ensure(app);
                changed(app);
            }
        });
    });
    if !open {
        app.hub.surf.install_open = false;
    }
}

fn param_choices() -> Vec<StripParam> {
    let mut v = vec![StripParam::Volume, StripParam::Pan, StripParam::Mute, StripParam::Solo, StripParam::RecordEnable, StripParam::Select, StripParam::Automation];
    v.extend((0..8).map(StripParam::Send));
    v
}

/// The Controller Assignments window: Easy view and Expert view.
fn assign_window(app: &mut App, ctx: &egui::Context) {
    let mut open = true;
    let title = if app.hub.surf.expert { "Controller Assignments — Expert View" } else { "Controller Assignments" };
    egui::Window::new(title).id(egui::Id::unique("cs-assign")).collapsible(false).open(&mut open).resizable(true).default_size(vec2(if app.hub.surf.expert { 820.0 } else { 520.0 }, 420.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let s = &mut app.hub.surf;
            // Easy view's Back / Forward step through the assignments
            if !s.expert {
                if ui.button("◀").on_hover_text("Back").clicked() {
                    s.selected = Some(s.selected.unwrap_or(0).saturating_sub(1)).filter(|_| !s.assign.list.is_empty());
                }
                if ui.button("▶").on_hover_text("Forward").clicked() {
                    s.selected = Some((s.selected.map_or(0, |x| x + 1)).min(s.assign.list.len().saturating_sub(1))).filter(|_| !s.assign.list.is_empty());
                }
            }
            ui.toggle_value(&mut s.link, "Link").on_hover_text("Selects the assignment of the last controller moved");
            let learn = s.learn;
            let b = egui::Button::new(if learn { "Learn Mode ●" } else { "Learn Mode" }).fill(if learn { Color32::from_rgb(0xc8, 0x32, 0x32) } else { Color32::from_gray(60) });
            if ui.add(b).on_hover_text("⌘L").clicked() {
                s.learn = !learn;
            }
            ui.menu_button("Action ⌄", |ui| {
                ui.checkbox(&mut app.hub.surf.ignore_notes, "Ignore MIDI Notes While Learning");
                ui.checkbox(&mut app.hub.surf.pinned_new, "New Track Assignments Are Pinned to Track");
            });
            let s = &mut app.hub.surf;
            if s.expert {
                if ui.button("Easy View").clicked() {
                    // (only when a track or plug-in parameter assignment is selected)
                    let ok = s.selected.and_then(|i| s.assign.list.get(i)).is_none_or(|a| matches!(a.class, Class::Strip { .. }));
                    if ok {
                        s.expert = false;
                    } else {
                        s.alert = Some("Easy View shows track and plug-in parameter assignments: select one first.".into());
                    }
                }
            } else if ui.button("Expert View").clicked() {
                s.expert = true;
            }
        });
        ui.separator();
        if app.hub.surf.link {
            if let Some((port, bytes)) = app.hub.surf.last_msg.clone() {
                if let Some((i, _)) = app.hub.surf.assign.matching(&port, &bytes, 0).first() {
                    app.hub.surf.selected = Some(*i);
                }
            }
        }
        if app.hub.surf.expert {
            expert_view(app, ui);
        } else {
            easy_view(app, ui);
        }
    });
    if !open {
        app.hub.surf.assign_open = false;
        app.hub.surf.learn = false;
    }
}

fn easy_view(app: &mut App, ui: &mut egui::Ui) {
    let tracks: Vec<String> = app.project.as_ref().map(|p| p.tracks.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
    // what Learn assigns (or the selected assignment's)
    let sel = app.hub.surf.selected.filter(|i| *i < app.hub.surf.assign.list.len());
    let mut class = match sel {
        Some(i) => app.hub.surf.assign.list[i].class.clone(),
        None => app.hub.surf.learn_class.clone(),
    };
    egui::Grid::new("cs-easy").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        if let Class::Strip { strip, param } = &mut class {
            ui.label("Parameter:");
            egui::ComboBox::from_id_salt("cs-easy-param").selected_text(param.name()).show_ui(ui, |ui| {
                for p in param_choices() {
                    let n = p.name();
                    ui.selectable_value(param, p, n);
                }
            });
            ui.end_row();
            ui.label("Channel Strip:");
            ui.horizontal(|ui| {
                let mut k = if *strip == Strip::Selected { 0 } else { 1 };
                egui::ComboBox::from_id_salt("cs-easy-strip").selected_text(if k == 0 { "Selected Track" } else { "Channel Strip" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut k, 0, "Selected Track");
                    ui.selectable_value(&mut k, 1, "Channel Strip");
                });
                if k == 0 {
                    *strip = Strip::Selected;
                } else {
                    let mut n = match strip {
                        Strip::Index(n) | Strip::Pinned(n) | Strip::FaderBank(n) => *n,
                        _ => 0,
                    };
                    ui.add(egui::DragValue::new(&mut n).range(0..=999).custom_formatter(|x, _| format!("{}", x as i64 + 1)));
                    if let Some(t) = tracks.get(n as usize) {
                        ui.label(t);
                    }
                    *strip = if matches!(strip, Strip::Pinned(_)) { Strip::Pinned(n) } else { Strip::Index(n) };
                }
            });
            ui.end_row();
        } else {
            ui.label("Parameter:");
            ui.label(sel.map(|i| app.hub.surf.assign.list[i].short_param()).unwrap_or_default());
            ui.end_row();
        }
        ui.label("Input message:");
        let msg = match sel {
            Some(i) => cs::msg_text(&app.hub.surf.assign.list[i].msg),
            None => app.hub.surf.last_msg.as_ref().map(|m| m.1.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")).unwrap_or_default(),
        };
        ui.monospace(msg);
        ui.end_row();
    });
    match sel {
        Some(i) => {
            if app.hub.surf.assign.list[i].class != class {
                app.hub.surf.assign.list[i].class = class;
                changed(app);
            }
        }
        None => app.hub.surf.learn_class = class,
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(sel.is_some(), egui::Button::new("Delete")).clicked() {
            if let Some(i) = sel {
                app.hub.surf.assign.list.remove(i);
                app.hub.surf.selected = None;
                changed(app);
            }
        }
        if ui.button("New").on_hover_text("Deselect: Learn makes a new assignment").clicked() {
            app.hub.surf.selected = None;
        }
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new(if app.hub.surf.learn { "Learn Mode: move a control on your MIDI controller. Learn stays on for more; ⌘L ends it." } else { "Turn on Learn Mode (⌘L), then move a control on your MIDI controller." }).small().color(look::text_dim()));
    ui.label(egui::RichText::new(format!("{} assignment{}", app.hub.surf.assign.list.len(), if app.hub.surf.assign.list.len() == 1 { "" } else { "s" })).small().color(look::text_dim()));
}

fn expert_view(app: &mut App, ui: &mut egui::Ui) {
    let avail = ui.available_rect_before_wrap();
    ui.allocate_rect(avail, Sense::hover());
    let w3 = 170.0;
    let zr = Rect::from_min_size(avail.min, vec2(w3, avail.height()));
    let mr = Rect::from_min_size(pos2(zr.max.x + 6.0, avail.min.y), vec2(w3, avail.height()));
    let lr = Rect::from_min_size(pos2(mr.max.x + 6.0, avail.min.y), vec2(220.0, avail.height()));
    let pr = Rect::from_min_max(pos2(lr.max.x + 6.0, avail.min.y), avail.max);
    // Zone list
    let mut zui = ui.new_child(egui::UiBuilder::new().max_rect(zr));
    zui.strong("Zone");
    let zones: Vec<String> = std::iter::once(String::new()).chain(app.hub.surf.assign.zones.iter().map(|z| z.0.clone())).collect();
    egui::ScrollArea::vertical().id_salt("cs-zones").max_height(zr.height() - 50.0).show(&mut zui, |ui| {
        for z in &zones {
            let on = app.hub.surf.sel_zone.clone().unwrap_or_default() == *z;
            let r = ui.selectable_label(on, if z.is_empty() { "No Zone" } else { z });
            if r.clicked() {
                app.hub.surf.sel_zone = (!z.is_empty()).then(|| z.clone());
            }
            if r.double_clicked() && !z.is_empty() {
                ui.data_mut(|d| d.insert_temp(egui::Id::unique("cs-rename-zone"), (z.clone(), z.clone())));
            }
        }
    });
    if zui.button("+").on_hover_text("Add a zone").clicked() {
        let n = format!("Zone {}", app.hub.surf.assign.zones.len() + 1);
        app.hub.surf.assign.zones.push((n.clone(), Vec::new()));
        app.hub.surf.sel_zone = Some(n.clone());
        zui.data_mut(|d| d.insert_temp(egui::Id::unique("cs-rename-zone"), (n.clone(), n)));
        changed(app);
    }
    rename_field(app, &mut zui, true);
    // Mode list
    let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(mr));
    mui.strong("Mode");
    let zone = app.hub.surf.sel_zone.clone();
    let modes: Vec<String> = zone.as_ref().and_then(|z| app.hub.surf.assign.zones.iter().find(|x| &x.0 == z)).map(|x| x.1.clone()).unwrap_or_default();
    let active = zone.as_ref().and_then(|z| app.hub.surf.assign.active.get(z).cloned());
    let sel_mode_key = egui::Id::unique("cs-sel-mode");
    let mut sel_mode: String = mui.data(|d| d.get_temp(sel_mode_key)).unwrap_or_default();
    egui::ScrollArea::vertical().id_salt("cs-modes").max_height(mr.height() - 50.0).show(&mut mui, |ui| {
        for m in std::iter::once(String::new()).chain(modes.iter().cloned()) {
            let label = if m.is_empty() { "No Mode".to_string() } else if active.as_ref() == Some(&m) { format!("● {m}") } else { m.clone() };
            let r = ui.selectable_label(sel_mode == m, label);
            if r.clicked() {
                sel_mode = m.clone();
                // selecting a mode makes it the zone's active one
                if let (Some(z), false) = (&zone, m.is_empty()) {
                    app.hub.surf.assign.active.insert(z.clone(), m.clone());
                    changed(app);
                }
            }
            if r.double_clicked() && !m.is_empty() {
                ui.data_mut(|d| d.insert_temp(egui::Id::unique("cs-rename-mode"), (m.clone(), m.clone())));
            }
        }
    });
    if mui.add_enabled(zone.is_some(), egui::Button::new("+")).on_hover_text("Add a mode").clicked() {
        if let Some(z) = &zone {
            if let Some(x) = app.hub.surf.assign.zones.iter_mut().find(|x| &x.0 == z) {
                let n = format!("Mode {}", x.1.len() + 1);
                x.1.push(n.clone());
                sel_mode = n.clone();
                mui.data_mut(|d| d.insert_temp(egui::Id::unique("cs-rename-mode"), (n.clone(), n)));
                changed(app);
            }
        }
    }
    mui.data_mut(|d| d.insert_temp(sel_mode_key, sel_mode.clone()));
    rename_field(app, &mut mui, false);
    // Control / Parameter list
    let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(lr));
    lui.strong("Control / Parameter");
    let z = zone.clone().unwrap_or_default();
    let rows: Vec<usize> = (0..app.hub.surf.assign.list.len()).filter(|i| {
        let a = &app.hub.surf.assign.list[*i];
        a.zone == z && a.mode == sel_mode
    }).collect();
    egui::ScrollArea::vertical().id_salt("cs-list").max_height(lr.height() - 50.0).show(&mut lui, |ui| {
        for i in rows {
            let a = &app.hub.surf.assign.list[i];
            let on = app.hub.surf.selected == Some(i);
            if ui.selectable_label(on, format!("{:<12} {}", a.control_name, a.short_param())).clicked() {
                app.hub.surf.selected = Some(i);
            }
        }
    });
    lui.horizontal(|ui| {
        if ui.button("+").on_hover_text("A new blank assignment").clicked() {
            let mut a = Assignment::learned("", Vec::new(), Class::Strip { strip: Strip::Selected, param: StripParam::Volume });
            a.zone = z.clone();
            a.mode = sel_mode.clone();
            a.control_name = "New".into();
            app.hub.surf.assign.list.push(a);
            app.hub.surf.selected = Some(app.hub.surf.assign.list.len() - 1);
            changed(app);
        }
        if ui.add_enabled(app.hub.surf.selected.is_some(), egui::Button::new("Delete")).clicked() {
            if let Some(i) = app.hub.surf.selected.take() {
                if i < app.hub.surf.assign.list.len() {
                    app.hub.surf.assign.list.remove(i);
                    changed(app);
                }
            }
        }
    });
    // the assignment's parameters
    let mut pui = ui.new_child(egui::UiBuilder::new().max_rect(pr));
    egui::ScrollArea::vertical().id_salt("cs-params").show(&mut pui, |ui| assignment_params(app, ui));
}

fn rename_field(app: &mut App, ui: &mut egui::Ui, zone: bool) {
    let key = egui::Id::unique(if zone { "cs-rename-zone" } else { "cs-rename-mode" });
    let Some((old, mut new)) = ui.data(|d| d.get_temp::<(String, String)>(key)) else { return };
    let r = ui.text_edit_singleline(&mut new);
    r.request_focus();
    if r.lost_focus() {
        let s = &mut app.hub.surf;
        if zone {
            if let Some(z) = s.assign.zones.iter_mut().find(|z| z.0 == old) {
                z.0 = new.clone();
            }
            for a in s.assign.list.iter_mut().filter(|a| a.zone == old) {
                a.zone = new.clone();
            }
            if let Some(m) = s.assign.active.remove(&old) {
                s.assign.active.insert(new.clone(), m);
            }
            s.sel_zone = Some(new);
        } else {
            let zname = s.sel_zone.clone().unwrap_or_default();
            if let Some(z) = s.assign.zones.iter_mut().find(|z| z.0 == zname) {
                for m in z.1.iter_mut().filter(|m| **m == old) {
                    *m = new.clone();
                }
            }
            for a in s.assign.list.iter_mut().filter(|a| a.zone == zname && a.mode == old) {
                a.mode = new.clone();
            }
            if s.assign.active.get(&zname) == Some(&old) {
                s.assign.active.insert(zname, new);
            }
        }
        ui.data_mut(|d| d.remove::<(String, String)>(key));
        changed(app);
    } else {
        ui.data_mut(|d| d.insert_temp(key, (old, new)));
    }
}

fn assignment_params(app: &mut App, ui: &mut egui::Ui) {
    let Some(i) = app.hub.surf.selected.filter(|i| *i < app.hub.surf.assign.list.len()) else {
        ui.label(egui::RichText::new("Select an assignment").color(look::text_dim()));
        return;
    };
    let mut a = app.hub.surf.assign.list[i].clone();
    let before = a.clone();
    let ins = app.live.input_names.clone();
    let zones: Vec<(String, Vec<String>)> = app.hub.surf.assign.zones.clone();
    egui::Grid::new("cs-ap").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        ui.label("Control Name:");
        ui.text_edit_singleline(&mut a.control_name);
        ui.end_row();
        ui.label("Label:");
        ui.text_edit_singleline(&mut a.label).on_hover_text("@ placeholders: @t# track number, @tn track name, @s# send slot…");
        ui.end_row();
        ui.label("Flip Group:");
        ui.add(egui::DragValue::new(&mut a.flip_group).range(0..=999).custom_formatter(|x, _| if x == 0.0 { "none".into() } else { format!("{x:.0}") }));
        ui.end_row();
        ui.label("Exclusive:");
        ui.checkbox(&mut a.exclusive, "");
        ui.end_row();
        ui.label("Class:");
        let mut ci = a.class.index();
        egui::ComboBox::from_id_salt("cs-class").selected_text(Class::NAMES[ci]).show_ui(ui, |ui| {
            for (k, n) in Class::NAMES.iter().enumerate() {
                ui.selectable_value(&mut ci, k, *n);
            }
        });
        if ci != a.class.index() {
            a.class = Class::with_index(ci);
        }
        ui.end_row();
        match &mut a.class {
            Class::ModeChange { mode, how } => {
                ui.label("Mode:");
                let zm: Vec<String> = zones.iter().find(|z| z.0 == a.zone).map(|z| z.1.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("cs-mc-mode").selected_text(mode.clone()).show_ui(ui, |ui| {
                    for m in &zm {
                        ui.selectable_value(mode, m.clone(), m);
                    }
                });
                ui.end_row();
                ui.label("Change:");
                egui::ComboBox::from_id_salt("cs-mc-how").selected_text(["Direct", "Toggle", "Relative", "Rotate"][*how as usize % 4]).show_ui(ui, |ui| {
                    for (k, n) in ["Direct", "Toggle", "Relative", "Rotate"].iter().enumerate() {
                        ui.selectable_value(how, k as u8, *n);
                    }
                });
                ui.end_row();
            }
            Class::Global { param, number } => {
                ui.label("Parameter:");
                egui::ComboBox::from_id_salt("cs-glob").selected_text(param.clone()).width(200.0).show_ui(ui, |ui| {
                    for g in cs::GLOBALS {
                        ui.selectable_value(param, g.to_string(), g);
                    }
                });
                ui.end_row();
                // (extra menus: Nudge Value's and Scrub Status's options)
                let opts: &[&str] = match param.as_str() {
                    "Nudge Value" => &cs::NUDGE_VALUES,
                    "Scrub Status" => &cs::SCRUB_STATUS,
                    _ => &[],
                };
                if !opts.is_empty() {
                    ui.label("Value:");
                    let mut k = (*number).clamp(0, opts.len() as i32 - 1) as usize;
                    egui::ComboBox::from_id_salt("cs-glob-opt").selected_text(opts[k]).show_ui(ui, |ui| {
                        for (j, n) in opts.iter().enumerate() {
                            ui.selectable_value(&mut k, j, *n);
                        }
                    });
                    *number = k as i32;
                    ui.end_row();
                }
                if param == "Go to Marker" {
                    ui.label("Number:");
                    ui.add(egui::DragValue::new(number).range(1..=999));
                    ui.end_row();
                }
            }
            Class::Strip { strip, param } => {
                ui.label("Channel Strip:");
                let mut si = strip.index();
                let mut n = strip.number();
                egui::ComboBox::from_id_salt("cs-strip").selected_text(Strip::NAMES[si]).show_ui(ui, |ui| {
                    for (k, nm) in Strip::NAMES.iter().enumerate() {
                        ui.selectable_value(&mut si, k, *nm);
                    }
                });
                ui.end_row();
                if !matches!(si, 0 | 7) {
                    ui.label("Number:");
                    ui.add(egui::DragValue::new(&mut n).range(0..=999));
                    ui.end_row();
                }
                *strip = Strip::with(si, n);
                ui.label("Parameter:");
                egui::ComboBox::from_id_salt("cs-sp").selected_text(param.name()).show_ui(ui, |ui| {
                    for p in param_choices() {
                        let nm = p.name();
                        ui.selectable_value(param, p, nm);
                    }
                });
                ui.end_row();
            }
            Class::Key(k) => {
                ui.label("Key:");
                ui.text_edit_singleline(k);
                ui.end_row();
            }
            Class::KeyCommand(c) => {
                ui.label("Command:");
                ui.text_edit_singleline(c).on_hover_text("A key command's name, as the Key Commands window lists it");
                ui.end_row();
                ui.label("");
                if ui.button("Show").clicked() {
                    app.keycmds.open_with(c);
                }
                ui.end_row();
            }
            Class::Group { param } => {
                ui.label("Parameter:");
                let shown = param.split('|').next().unwrap_or("").to_string();
                egui::ComboBox::from_id_salt("cs-gp").selected_text(shown.clone()).height(360.0).show_ui(ui, |ui| {
                    for g in cs::GROUP_PARAMS {
                        if ui.selectable_label(shown == g, g).clicked() {
                            *param = g.to_string();
                        }
                    }
                });
                ui.end_row();
                // Filter for All View: the eight strip types (Direct sets them, X-OR toggles)
                if shown == "Filter for All View" {
                    let mut mask = param.split('|').nth(1).and_then(|x| x.parse::<u8>().ok()).unwrap_or(0xff);
                    ui.label("Types:");
                    ui.horizontal_wrapped(|ui| {
                        for (k, n) in cs::STRIP_TYPES.iter().enumerate() {
                            let mut on = mask & (1 << k) != 0;
                            if ui.checkbox(&mut on, *n).changed() {
                                mask ^= 1 << k;
                            }
                        }
                    });
                    *param = format!("Filter for All View|{mask}");
                    ui.end_row();
                }
                if shown.starts_with("Fader Bank") || shown.contains("Page") || shown == "Live Loops Column Bank" {
                    ui.label("Bank Type:");
                    let mut bt = a.bank_type as usize;
                    egui::ComboBox::from_id_salt("cs-bt").selected_text(cs::BANK_TYPES[bt.min(2)]).show_ui(ui, |ui| {
                        for (k, n) in cs::BANK_TYPES.iter().enumerate() {
                            ui.selectable_value(&mut bt, k, *n);
                        }
                    });
                    a.bank_type = bt as u8;
                    ui.end_row();
                }
            }
            Class::AutomationGroup { param, group } => {
                ui.label("Parameter:");
                ui.text_edit_singleline(param);
                ui.end_row();
                ui.label("Group:");
                ui.add(egui::DragValue::new(group).range(0..=32));
                ui.end_row();
            }
        }
    });
    ui.separator();
    ui.strong("MIDI");
    egui::Grid::new("cs-midi").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        ui.label("Input:");
        egui::ComboBox::from_id_salt("cs-in-port").selected_text(if a.port.is_empty() { "Any".to_string() } else { a.port.clone() }).width(180.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut a.port, String::new(), "Any");
            for p in &ins {
                ui.selectable_value(&mut a.port, p.clone(), p);
            }
        });
        ui.end_row();
        ui.label("Value Change:");
        let mut t = cs::msg_text(&a.msg);
        if ui.add(egui::TextEdit::singleline(&mut t).font(egui::TextStyle::Monospace)).changed() {
            a.msg = cs::parse_msg(&t);
        }
        ui.end_row();
        ui.label("");
        ui.label(egui::RichText::new(cs::msg_meaning(&a.msg)).small().color(look::text_dim()));
        ui.end_row();
        ui.label("Lo7 Min / Max:");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut a.min).range(-8192..=16383));
            ui.add(egui::DragValue::new(&mut a.max).range(-8192..=16383));
            ui.checkbox(&mut a.only, "Only");
        });
        ui.end_row();
        ui.label("Format:");
        let mut fi = a.format.index();
        egui::ComboBox::from_id_salt("cs-fmt").selected_text(cs::Format::NAMES[fi]).show_ui(ui, |ui| {
            for (k, n) in cs::Format::NAMES.iter().enumerate() {
                ui.selectable_value(&mut fi, k, *n);
            }
        });
        a.format = cs::Format::from_index(fi);
        ui.end_row();
        ui.label("Touch/Release:");
        let mut tt = cs::msg_text(&a.touch);
        if ui.text_edit_singleline(&mut tt).changed() {
            a.touch = cs::parse_msg(&tt);
        }
        ui.end_row();
    });
    ui.label(egui::RichText::new("OSC and Game Controller: Studio talks MIDI only.").small().color(look::text_dim()));
    ui.separator();
    ui.strong("Value");
    egui::Grid::new("cs-val").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        ui.label("Multiply:");
        ui.add(egui::DragValue::new(&mut a.multiply).speed(0.01).range(-100.0..=100.0).fixed_decimals(2));
        ui.end_row();
        ui.label("Mode:");
        let mut vm = a.value_mode.index();
        egui::ComboBox::from_id_salt("cs-vm").selected_text(ValueMode::NAMES[vm]).show_ui(ui, |ui| {
            for (k, n) in ValueMode::NAMES.iter().enumerate() {
                ui.selectable_value(&mut vm, k, *n);
            }
        });
        a.value_mode = ValueMode::from_index(vm);
        ui.end_row();
        ui.label("Resolution:");
        ui.horizontal(|ui| {
            ui.add_enabled(!a.global_resolution, egui::DragValue::new(&mut a.resolution).range(1..=10000));
            ui.checkbox(&mut a.global_resolution, "Use Global Resolution");
        });
        ui.end_row();
        ui.label("Key Repeat:");
        let can_repeat = matches!(a.class, Class::KeyCommand(_) | Class::Key(_)) || a.value_mode == ValueMode::Relative;
        ui.add_enabled(can_repeat, egui::Checkbox::new(&mut a.key_repeat, ""));
        ui.end_row();
        ui.label("Feedback:");
        egui::ComboBox::from_id_salt("cs-fb").selected_text(cs::FEEDBACK[a.feedback.min(10)]).show_ui(ui, |ui| {
            for (k, n) in cs::FEEDBACK.iter().enumerate() {
                ui.selectable_value(&mut a.feedback, k, *n);
            }
        });
        ui.end_row();
        ui.label("");
        ui.horizontal(|ui| {
            ui.checkbox(&mut a.text_feedback, "Text Feedback");
            ui.checkbox(&mut a.local_feedback, "Local Feedback (Fader/Knob)");
        });
        ui.end_row();
    });
    if let Some((_, b)) = &app.hub.surf.last_msg {
        if let Some(v) = a.value_of("", b) {
            ui.label(egui::RichText::new(format!("Incoming value: {v}")).small().color(look::text_dim()));
        }
    }
    // the label as the surface shows it (placeholders expanded)
    if a.label.contains('@') {
        let lc = label_ctx(app);
        let shown = cs::expand_label(&a.label, |k, v| cs::label_value(&lc, k, v));
        ui.label(egui::RichText::new(format!("Label shows: {shown}")).small().color(look::text_dim()));
    }
    let relearn = a.key_repeat && !before.key_repeat;
    if a != before {
        app.hub.surf.assign.list[i] = a;
        changed(app);
    }
    if relearn {
        crate::surfext::start_relearn(app, i);
    }
}

/// What label placeholders stand for now (the selected track, the group's slots and pages).
fn label_ctx(app: &App) -> cs::LabelCtx {
    let g = app.hub.surf.groups.first().cloned().unwrap_or_default();
    let t = g.track_lock.or(app.selected_track);
    let tr = t.and_then(|t| app.project.as_ref().and_then(|p| p.tracks.get(t)));
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len()) as i32;
    cs::LabelCtx {
        track_no: t.map_or(0, |t| t as i32 + 1),
        track_name: tr.map_or(String::new(), |x| x.name.clone()),
        surround: cs::SURROUND_PARAMS[crate::surfext::gx(app, 0, "Surround Parameter").clamp(0, 7) as usize].into(),
        send_slot: g.send_slot,
        sends: t.and_then(|t| app.sends.get(&t)).map_or(0, |v| v.len() as i32),
        eq_band: crate::surfext::gx(app, 0, "EQ Band"),
        eqs: 8,
        insert_slot: crate::surfext::gx(app, 0, "Insert Slot"),
        insert_name: tr.and_then(|x| x.plugins.get(crate::surfext::gx(app, 0, "Insert Slot").max(1) as usize - 1)).map_or(String::new(), |p| p.name.clone()),
        inst_name: tr.and_then(|x| x.plugins.first()).map_or(String::new(), |p| p.name.clone()),
        param: String::new(),
        first_param: String::new(),
        offset: crate::surfext::gx(app, 0, "Plug-in Parameter Page"),
        max_offset: 0,
        bank: g.fader_bank / 8,
        banks: (n + 7) / 8,
    }
}

fn pending_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(p) = app.hub.surf.pending.clone() else { return };
    let mut done = false;
    egui::Window::new("Controller Assignments").id(egui::Id::unique("cs-pending")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| match p {
        Pending::Conflict(a) => {
            ui.label("This control is assigned already.");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = true;
                }
                if ui.button("Keep both").clicked() {
                    add_assignment(app, a.clone());
                    done = true;
                }
                if ui.button("Reassign").clicked() {
                    let s = &mut app.hub.surf;
                    let keep: Vec<bool> = (0..s.assign.list.len()).map(|i| !(s.assign.is_active(i) && s.assign.list[i].msg == a.msg && s.assign.list[i].port == a.port)).collect();
                    let mut k = 0;
                    s.assign.list.retain(|_| {
                        let r = keep[k];
                        k += 1;
                        r
                    });
                    add_assignment(app, a.clone());
                    done = true;
                }
            });
        }
        Pending::Series(a, fill) => {
            ui.label(format!("Fill up the {} assignments in between?", fill.len().saturating_sub(2)));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    add_assignment(app, a.clone());
                    done = true;
                }
                if ui.button("OK").clicked() {
                    let Class::Strip { strip: Strip::FaderBank(last), param } = a.class.clone() else { return };
                    let first = last - fill.len() as i32 + 1;
                    for (k, m) in fill.iter().enumerate().skip(1) {
                        let mut x = a.clone();
                        x.msg = m.clone();
                        x.class = Class::Strip { strip: Strip::FaderBank(first + k as i32), param: param.clone() };
                        app.hub.surf.assign.list.push(x);
                    }
                    changed(app);
                    done = true;
                }
            });
        }
    });
    if done {
        app.hub.surf.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_offsets_follow_rows() {
        let mut s = State::default();
        let dev = |row, col, model| Device { model, in_port: String::new(), out_port: String::new(), row, col, color: 0, version: String::new(), transport_click: false, touchless: false, backlight_min: 0 };
        s.devices = vec![dev(0, 0, Model::MackieControl), dev(0, 1, Model::MackieExtender), dev(1, 0, Model::MackieControl)];
        // the XT to the right of the Logic Control: channels 9–16
        assert_eq!(device_offset(&s, 1), 8);
        assert_eq!(device_offset(&s, 2), 0);
    }
}
