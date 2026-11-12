//! The Mixer's windows and files: the Binaural Panner window, the I/O Labels window (custom
//! names for inputs, outputs and buses, per device, kept with Studio's settings), channel
//! strip settings (Studio's .cst text files per channel strip type: save, load, next /
//! previous, delete, performances recalled by program change), MIDI channel strips for
//! External MIDI tracks (Program, Bank, Assign 1–5, volume as controller 7, pan as 10),
//! Configure Channel Strip Components, and Search and Add Plug-in.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::plugin::Plugin;
use lg_core::trackmix::{Binaural, TrackSet, BINAURAL_SIZE_DEFAULT};
use lg_core::TrackKind;

use crate::app::{App, SendSlot};
use crate::look;
use crate::mixer::AUX;
use crate::mixstate::{self, Kind};

/// An input, an output or a bus (numbered from 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Io {
    Input(usize),
    Output(usize),
    Bus(usize),
}

impl Io {
    fn kind(self) -> char {
        match self {
            Io::Input(_) => 'i',
            Io::Output(_) => 'o',
            Io::Bus(_) => 'b',
        }
    }
    fn n(self) -> usize {
        match self {
            Io::Input(n) | Io::Output(n) | Io::Bus(n) => n,
        }
    }
    /// The label without a custom name: long ("Input 1") or short ("In 1").
    pub fn default_label(self, long: bool) -> String {
        match (self, long) {
            (Io::Input(n), true) => format!("Input {}", n + 1),
            (Io::Input(n), false) => format!("In {}", n + 1),
            (Io::Output(n), true) => format!("Output {}", n + 1),
            (Io::Output(n), false) => format!("Out {}", n + 1),
            (Io::Bus(n), _) => format!("Bus {}", n + 1),
        }
    }
}

/// One channel's label: which column is chosen (0 Channel, 1 Provided by Driver, 2 User) and
/// the user's long and short names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Label {
    pub mode: u8,
    pub long: String,
    pub short: String,
}

/// The I/O labels: per device, kept with Studio's settings (not with a project).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IoLabels {
    pub device: String,
    /// (device, kind, channel) → label.
    pub labels: HashMap<(String, char, usize), Label>,
}

impl IoLabels {
    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("bramble-studio").join("io-labels.txt"))
    }

    pub fn to_text(&self) -> String {
        let mut rows: Vec<String> = self.labels.iter().filter(|(_, l)| **l != Label::default()).map(|((d, k, n), l)| format!("{d}\t{k}\t{n}\t{}\t{}\t{}", l.mode, l.long, l.short)).collect();
        rows.sort();
        format!("device\t{}\n{}\n", self.device, rows.join("\n"))
    }

    pub fn from_text(t: &str) -> IoLabels {
        let mut io = IoLabels::default();
        for l in t.lines() {
            let p: Vec<&str> = l.split('\t').collect();
            match p.as_slice() {
                ["device", d] => io.device = d.to_string(),
                [d, k, n, mode, long, short] => {
                    if let (Some(k), Ok(n), Ok(mode)) = (k.chars().next(), n.parse(), mode.parse()) {
                        io.labels.insert((d.to_string(), k, n), Label { mode, long: long.to_string(), short: short.to_string() });
                    }
                }
                _ => {}
            }
        }
        io
    }

    pub fn load() -> IoLabels {
        IoLabels::path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| IoLabels::from_text(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = IoLabels::path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, self.to_text());
        }
    }

    pub fn label(&self, io: Io, long: bool) -> String {
        match self.labels.get(&(self.device.clone(), io.kind(), io.n())) {
            Some(l) if l.mode == 2 => {
                let s = if long { &l.long } else { &l.short };
                // (one of the two typed: it serves for both)
                let s = if s.is_empty() { if long { &l.short } else { &l.long } } else { s };
                if s.is_empty() { io.default_label(long) } else { s.clone() }
            }
            _ => io.default_label(long),
        }
    }

    /// Reset All / Input / Output / Bus Labels (None: all) for the device shown.
    pub fn reset(&mut self, kind: Option<char>) {
        let d = self.device.clone();
        self.labels.retain(|(dev, k, _), _| *dev != d || kind.is_some_and(|x| x != *k));
    }
}

/// The long (menus) or short (slots) name of an input, output or bus.
pub fn io_label(app: &App, io: Io, long: bool) -> String {
    app.tm.mx.win.io.label(io, long)
}

#[derive(Default)]
pub struct State {
    pub components: bool,
    pub io_open: bool,
    pub io: IoLabels,
    io_loaded: bool,
    io_edit: Option<(char, usize, bool, String)>,
    /// Save Channel Strip Setting as…: the strip, the name typed, as a performance with its
    /// program number.
    pub cst_save: Option<(usize, String, bool, u8)>,
    /// The setting each strip last loaded or saved (for Next / Previous and Delete).
    pub cst_current: HashMap<usize, String>,
    /// Search and Add Plug-in: the text typed.
    pub search: Option<String>,
    /// Program changes received (the live input hands them over; performances load from here).
    pub program_in: Option<u8>,
}

// ---------------------------------------------------------------------------------------------
// channel strip settings

fn kind_folder(k: Kind) -> &'static str {
    match k {
        Kind::Audio => "Audio",
        Kind::Inst | Kind::Midi => "Instrument",
        Kind::Aux | Kind::Bus | Kind::Input => "Aux",
        Kind::Output | Kind::MasterVca => "Output",
    }
}

/// Where channel strip settings are kept: one folder per channel strip type.
pub fn cst_dir(k: Kind) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("bramble-studio").join("Channel Strip Settings").join(kind_folder(k)))
}

/// The saved settings for a strip type: (name, file), then the performances.
pub fn cst_list(k: Kind) -> (Vec<(String, PathBuf)>, Vec<(String, PathBuf)>) {
    let read = |d: PathBuf| -> Vec<(String, PathBuf)> {
        let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "cst")).map(|p| (p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), p)).collect();
        v.sort();
        v
    };
    match cst_dir(k) {
        Some(d) => (read(d.clone()), read(d.join("Performance"))),
        None => (Vec::new(), Vec::new()),
    }
}

fn hex(f: &[f32]) -> String {
    f.iter().map(|x| format!("{:08x}", x.to_bits())).collect()
}

fn unhex(s: &str) -> Vec<f32> {
    (0..s.len() / 8).filter_map(|k| u32::from_str_radix(&s[8 * k..8 * k + 8], 16).ok()).map(f32::from_bits).collect()
}

/// A strip's setting as text: its type, the instrument and effects with their parameters, the
/// Channel EQ, the sends, and the strip's panner and input format.
pub fn strip_setting_text(app: &App, s: usize) -> String {
    let mut out = format!("bramble-channel-strip-setting 1\ntype\t{}\n", kind_folder(mixstate::kind_of(app, s)));
    let plugins: Vec<Plugin> = if s < AUX {
        app.project.as_ref().and_then(|p| p.tracks.get(s)).map(|t| t.plugins.iter().filter(|u| u.slot >= 2 && !u.name.is_empty() && u.name != "Channel EQ").cloned().collect()).unwrap_or_default()
    } else {
        mixstate::inserts(app, s)
    };
    for u in plugins {
        out += &format!("plugin\t{}\t{}\t{}\t{}\t{}\n", u.slot, u.bypass as u8, u.preset, hex(&u.params), u.name);
    }
    if let Some(e) = app.eqs.get(&s) {
        out += &format!("eq\t{}\n", e.bands.iter().map(|b| format!("{},{},{},{}", b.freq, b.gain, b.q, b.on as u8)).collect::<Vec<_>>().join(";"));
    }
    for snd in app.sends.get(&s).into_iter().flatten() {
        out += &format!("send\t{}\t{}\t{}\n", snd.bus, snd.level_db, snd.pre as u8);
    }
    let set = TrackSet { strip: crate::tm::strip(app, s), ..Default::default() }.to_line();
    if !set.is_empty() {
        out += &format!("set\t{set}\n");
    }
    out
}

/// Loads a setting onto strip `s`: its plug-ins, EQ and sends replace the strip's. False for
/// a text that isn't a setting.
pub fn apply_strip_setting(app: &mut App, s: usize, text: &str) -> bool {
    if !text.starts_with("bramble-channel-strip-setting") {
        return false;
    }
    let mut plugins: Vec<Plugin> = Vec::new();
    let mut sends: Vec<SendSlot> = Vec::new();
    let mut eq: Option<crate::eq::Params> = None;
    let mut set = None;
    for l in text.lines() {
        let p: Vec<&str> = l.splitn(6, '\t').collect();
        match p.as_slice() {
            ["plugin", slot, bypass, preset, floats, name] => plugins.push(Plugin { name: name.to_string(), preset: preset.to_string(), params: unhex(floats), channel: 0, slot: slot.parse().unwrap_or(3), bypass: *bypass == "1" }),
            ["eq", bands] => {
                let mut e = crate::eq::Params::default();
                for (k, b) in bands.split(';').take(8).enumerate() {
                    let v: Vec<f32> = b.split(',').filter_map(|x| x.parse().ok()).collect();
                    if v.len() == 4 {
                        e.bands[k] = crate::eq::Band { freq: v[0], gain: v[1], q: v[2], on: v[3] != 0.0 };
                    }
                }
                eq = Some(e);
            }
            ["send", bus, level, pre] => sends.push(SendSlot { bus: bus.parse().unwrap_or(0), level_db: level.parse().unwrap_or(0.0), pre: *pre == "1" }),
            ["set", ..] => set = Some(TrackSet::from_line(&l[4..]).strip),
            _ => {}
        }
    }
    mixstate::checkpoint(app, "Load Channel Strip Setting");
    if s < AUX {
        let Some(t) = app.project.as_mut().and_then(|p| p.tracks.get_mut(s)) else { return false };
        let audio = t.kind == TrackKind::Audio;
        t.plugins.retain(|u| u.slot < 2);
        // (an audio strip takes the effects only)
        t.plugins.extend(plugins.into_iter().filter(|u| !(audio && u.slot == 2)));
        t.plugins.sort_by_key(|u| u.slot);
        if let Some(name) = t.plugins.iter().find(|u| u.slot == 2).map(|u| u.name.clone()) {
            app.instrument_choice.insert(s, name);
        }
    } else {
        let fx: Vec<Plugin> = plugins.into_iter().filter(|u| u.slot >= 3).collect();
        mixstate::edit_inserts(app, s, |v| *v = fx);
    }
    match eq {
        Some(e) => {
            app.eqs.insert(s, e);
        }
        None => {
            app.eqs.remove(&s);
        }
    }
    for b in sends.iter().map(|x| x.bus).collect::<Vec<_>>() {
        if !app.auxes.iter().any(|a| a.bus == b) {
            mixstate::new_aux(app, Some(b));
        }
    }
    if sends.is_empty() {
        app.sends.remove(&s);
    } else {
        app.sends.insert(s, sends);
    }
    if let Some(st) = set {
        let color = crate::tm::strip(app, s).color;
        crate::tm::edit_strip(app, s, |x| *x = lg_core::trackmix::StripSet { color, ..st });
    }
    app.dirty = true;
    app.load_engine();
    true
}

/// Saves strip `s`'s setting under `name` (as a performance with its program number in the
/// Performance folder); the file.
pub fn save_strip_setting(app: &mut App, s: usize, name: &str, performance: Option<u8>) -> Option<PathBuf> {
    let dir = cst_dir(mixstate::kind_of(app, s))?;
    let (dir, file) = match performance {
        Some(n) => (dir.join("Performance"), format!("{n:03} {name}.cst")),
        None => (dir, format!("{name}.cst")),
    };
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(file);
    std::fs::write(&path, strip_setting_text(app, s)).ok()?;
    app.tm.mx.win.cst_current.insert(s, path.file_stem()?.to_string_lossy().into_owned());
    Some(path)
}

pub fn load_strip_setting(app: &mut App, s: usize, name: &str, path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    if !apply_strip_setting(app, s, &text) {
        return false;
    }
    app.tm.mx.win.cst_current.insert(s, name.to_string());
    true
}

/// Next / Previous Channel Strip Setting for strip `s`.
pub fn step_strip_setting(app: &mut App, s: usize, next: bool) -> bool {
    let (list, _) = cst_list(mixstate::kind_of(app, s));
    if list.is_empty() {
        return false;
    }
    let cur = app.tm.mx.win.cst_current.get(&s).and_then(|n| list.iter().position(|x| x.0 == *n));
    let k = match (cur, next) {
        (Some(k), true) => (k + 1) % list.len(),
        (Some(k), false) => (k + list.len() - 1) % list.len(),
        (None, true) => 0,
        (None, false) => list.len() - 1,
    };
    let (name, path) = list[k].clone();
    load_strip_setting(app, s, &name, &path)
}

/// A program change came in: the performance with that number loads onto the focused strip.
pub fn program_change(app: &mut App, program: u8) -> bool {
    let Some(s) = app.selected_track else { return false };
    let (_, perfs) = cst_list(mixstate::kind_of(app, s));
    let Some((name, path)) = perfs.into_iter().find(|(n, _)| n.split(' ').next().and_then(|x| x.parse::<u8>().ok()) == Some(program)) else { return false };
    load_strip_setting(app, s, &name, &path)
}

/// The Setting button's further items: the saved settings to load, Next / Previous, Save as,
/// Save as Performance, Delete, and the removals.
pub fn setting_menu(app: &mut App, ui: &mut egui::Ui, s: usize) {
    let kind = mixstate::kind_of(app, s);
    let (list, perfs) = cst_list(kind);
    let cur = app.tm.mx.win.cst_current.get(&s).cloned();
    ui.separator();
    if ui.add_enabled(!list.is_empty(), egui::Button::new("Next Channel Strip Setting").shortcut_text("⇧]")).clicked() {
        step_strip_setting(app, s, true);
        ui.close();
    }
    if ui.add_enabled(!list.is_empty(), egui::Button::new("Previous Channel Strip Setting").shortcut_text("⇧[")).clicked() {
        step_strip_setting(app, s, false);
        ui.close();
    }
    ui.separator();
    if ui.button("Save Channel Strip Setting as…").clicked() {
        app.tm.mx.win.cst_save = Some((s, cur.clone().unwrap_or_else(|| mixstate::strip_name(app, s)), false, 0));
        ui.close();
    }
    if ui.button("Save as Performance…").clicked() {
        app.tm.mx.win.cst_save = Some((s, cur.clone().unwrap_or_else(|| mixstate::strip_name(app, s)), true, perfs.len() as u8));
        ui.close();
    }
    if ui.add_enabled(cur.is_some(), egui::Button::new(match &cur { Some(n) => format!("Delete Channel Strip Setting “{n}”"), None => "Delete Channel Strip Setting".into() })).clicked() {
        delete_current(app, s);
        ui.close();
    }
    ui.separator();
    for c in ["Remove All Effect Plug-ins", "Remove Empty Insert Slots", "Remove All Sends"] {
        if ui.button(c).clicked() {
            app.mix.selected = [s].into_iter().collect();
            crate::mixext::command(app, &ui.ctx().clone(), c);
            ui.close();
        }
    }
    if !list.is_empty() || !perfs.is_empty() {
        ui.separator();
    }
    for (name, path) in &list {
        if ui.add(egui::Button::selectable(cur.as_deref() == Some(name), name.as_str())).clicked() {
            load_strip_setting(app, s, name, path);
            ui.close();
        }
    }
    if !perfs.is_empty() {
        ui.menu_button("Performance", |ui| {
            for (name, path) in &perfs {
                if ui.add(egui::Button::selectable(cur.as_deref() == Some(name), name.as_str())).clicked() {
                    load_strip_setting(app, s, name, path);
                    ui.close();
                }
            }
        });
    }
}

fn delete_current(app: &mut App, s: usize) -> bool {
    let Some(name) = app.tm.mx.win.cst_current.remove(&s) else { return false };
    let (list, perfs) = cst_list(mixstate::kind_of(app, s));
    match list.into_iter().chain(perfs).find(|x| x.0 == name) {
        Some((_, path)) => std::fs::remove_file(path).is_ok(),
        None => false,
    }
}

// ---------------------------------------------------------------------------------------------
// MIDI channel strips

fn cc(app: &App, e: &lg_core::trackmix::External, num: u8, value: u8) {
    crate::trackops::midi_send(app, &e.port, &[0xb0 | (e.channel.saturating_sub(1) & 15), num, value.min(127)]);
}

fn send_program(app: &App, e: &lg_core::trackmix::External) {
    if let Some(pg) = e.program {
        // the upper Bank field is controller 0, the lower one controller 32
        cc(app, e, 0, e.bank.0);
        cc(app, e, 32, e.bank.1);
        crate::trackops::midi_send(app, &e.port, &[0xc0 | (e.channel.saturating_sub(1) & 15), pg.min(127)]);
    }
}

fn vol_cc(db: f32) -> u8 {
    crate::automation::db_to_logic(db).round().clamp(0.0, 127.0) as u8
}

fn pan_cc(pan: f32) -> u8 {
    (64.0 + pan * 63.0).round().clamp(0.0, 127.0) as u8
}

/// Options ▸ Send All MIDI Mixer Data (and after a project opens): every MIDI channel strip's
/// program, bank, volume, pan and Assign controllers go to its device.
pub fn send_all_midi(app: &App) -> usize {
    let Some(p) = app.project.as_ref() else { return 0 };
    let mut n = 0;
    for t in &p.tracks {
        let Some(e) = &t.extra.set.external else { continue };
        send_program(app, e);
        cc(app, e, 7, vol_cc(t.volume_db));
        cc(app, e, 10, pan_cc(t.pan));
        for (num, val) in e.assigns.iter().filter(|a| a.0 > 0) {
            cc(app, e, *num, *val);
        }
        n += 1;
    }
    n
}

/// An External MIDI track's channel strip: a remote for the device's mixing parameters.
pub fn midi_strip(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize) {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(i)).cloned() else { return };
    let Some(mut e) = t.extra.set.external.clone() else { return };
    let selected = app.selected_track == Some(i) || app.mix.selected.contains(&i);
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, if selected { look::track_selected() } else { look::header_bg() });
    crate::mixext::strip_bg(app, ui, r, i);
    let before = e.clone();
    let (mut vol, mut pan, mut mute) = (vol_cc(t.volume_db) as i32, pan_cc(t.pan) as i32, t.mute);
    let (vol0, pan0, mute0) = (vol, pan, mute);
    let mut rename = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(vec2(5.0, 6.0))).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        let small = |s: &str| egui::RichText::new(s).size(10.0);
        if crate::mixext::midi_comp(app, "Instrument Name") {
            ui.label(small(&if e.port.is_empty() { "Virtual Out".to_string() } else { crate::live::port_label(&e.port) }).color(look::text_dim()));
        }
        ui.label(small(&format!("Ch {}", e.channel)).color(look::text_dim()));
        if crate::mixext::midi_comp(app, "Program") {
            // the Program button: the sound names (General MIDI's)
            let label = e.program.map_or("Program".to_string(), |pg| crate::tracks::ellipsize(lg_core::environment::GM_PROGRAMS[pg.min(127) as usize], 11));
            ui.menu_button(small(&label), |ui| {
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    if ui.add(egui::Button::selectable(e.program.is_none(), "No Program Change")).clicked() {
                        e.program = None;
                        ui.close();
                    }
                    for (k, name) in lg_core::environment::GM_PROGRAMS.iter().enumerate() {
                        if ui.add(egui::Button::selectable(e.program == Some(k as u8), format!("{}  {name}", k + 1))).clicked() {
                            e.program = Some(k as u8);
                            ui.close();
                        }
                    }
                });
            });
        }
        if crate::mixext::midi_comp(app, "Bank") {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut e.bank.0).range(0..=127).prefix("M ")).on_hover_text("Bank (controller 0)");
                ui.add(egui::DragValue::new(&mut e.bank.1).range(0..=127).prefix("L ")).on_hover_text("Bank (controller 32)");
            });
        }
        for k in 0..5 {
            if !crate::mixext::midi_comp(app, &format!("Assign {}", k + 1)) {
                continue;
            }
            ui.horizontal(|ui| {
                // the label picks the controller, the value sends it
                let (num, val) = &mut e.assigns[k];
                ui.menu_button(small(&if *num == 0 { format!("Assign {}", k + 1) } else { format!("CC {num}") }), |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for c in 0..=127u8 {
                            if ui.add(egui::Button::selectable(*num == c, if c == 0 { "Off".to_string() } else { format!("Controller {c}") })).clicked() {
                                *num = c;
                                ui.close();
                            }
                        }
                    });
                });
                ui.add_enabled(*num > 0, egui::DragValue::new(val).range(0..=127));
            });
        }
        ui.add(egui::DragValue::new(&mut pan).range(0..=127).prefix("Pan ")).on_hover_text("Pan (controller 10)");
        let fh = (ui.available_height() - 58.0).max(40.0);
        ui.spacing_mut().slider_width = fh;
        ui.add(egui::Slider::new(&mut vol, 0..=127).vertical().show_value(true)).on_hover_text("Volume (controller 7)");
        if ui.add(egui::Button::new(egui::RichText::new("M").strong().color(if mute { Color32::BLACK } else { look::text() })).fill(if mute { Color32::from_rgb(0x4f, 0xa8, 0xe8) } else { look::group_bg() }).min_size(vec2(26.0, 20.0))).on_hover_text("Mute: the strip stops sending MIDI").clicked() {
            mute = !mute;
        }
        let name = ui.add(egui::Label::new(small(&crate::tracks::ellipsize(&t.name, 12))).sense(Sense::click()));
        if name.clicked() {
            mixstate::select(app, i, ui.input(|x| x.modifiers.shift || x.modifiers.command));
        }
        rename = name.double_clicked();
    });
    if rename {
        app.tm.mx.renaming = Some((i, t.name.clone()));
        app.bottom = crate::app::Bottom::Mixer;
    }
    if e != before || vol != vol0 || pan != pan0 || mute != mute0 {
        // what changed goes out to the device, and into the track (the Track inspector's)
        if e.program != before.program || e.bank != before.bank {
            send_program(app, &e);
        }
        for k in 0..5 {
            if e.assigns[k] != before.assigns[k] && e.assigns[k].0 > 0 {
                cc(app, &e, e.assigns[k].0, e.assigns[k].1);
            }
        }
        if vol != vol0 {
            cc(app, &e, 7, vol as u8);
        }
        if pan != pan0 {
            cc(app, &e, 10, pan as u8);
        }
        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)) {
            tr.extra.set.external = Some(e);
            if vol != vol0 {
                tr.volume_db = crate::automation::logic_to_db(vol as f32);
            }
            if pan != pan0 {
                tr.pan = ((pan as f32 - 64.0) / 63.0).clamp(-1.0, 1.0);
            }
            tr.mute = mute;
        }
        app.dirty = true;
        if mute != mute0 {
            app.load_engine();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// windows

pub fn command(app: &mut App, _ctx: &egui::Context, name: &str) -> bool {
    let target = app.mix.selected.iter().next().copied().or(app.selected_track);
    match name {
        "I/O Labels…" => {
            ensure_io(app);
            app.tm.mx.win.io_open = true;
        }
        "Configure Channel Strip Components…" => app.tm.mx.win.components = !app.tm.mx.win.components,
        "Store as User Default" => {
            if let Some(p) = components_path() {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let mut parts: Vec<&String> = app.mix.hidden_parts.iter().collect();
                parts.sort();
                let _ = std::fs::write(p, parts.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
                app.tell("Channel strip components stored");
            }
        }
        "Apply User Default" => {
            if let Some(t) = components_path().and_then(|p| std::fs::read_to_string(p).ok()) {
                app.mix.hidden_parts = t.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect();
            }
        }
        "Revert to Default" => app.mix.hidden_parts.clear(),
        "Send All MIDI Mixer Data" => {
            let n = send_all_midi(app);
            app.tell(format!("MIDI mixer data sent for {n} channel strips"));
        }
        "Search and Add Plug-in…" => app.tm.mx.win.search = Some(String::new()),
        "Next Channel Strip Setting of focused Track" | "Next Channel Strip Setting" => return target.is_some_and(|s| step_strip_setting(app, s, true)),
        "Previous Channel Strip Setting of focused Track" | "Previous Channel Strip Setting" => return target.is_some_and(|s| step_strip_setting(app, s, false)),
        "Save Channel Strip Setting as…" | "Save as Performance…" => {
            let Some(s) = target else { return false };
            app.tm.mx.win.cst_save = Some((s, mixstate::strip_name(app, s), name.contains("Performance"), 0));
        }
        "Delete Channel Strip Setting" => return target.is_some_and(|s| delete_current(app, s)),
        _ => return false,
    }
    true
}

fn components_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("channel-strip-components.txt"))
}

fn ensure_io(app: &mut App) {
    if !app.tm.mx.win.io_loaded {
        app.tm.mx.win.io = IoLabels::load();
        app.tm.mx.win.io_loaded = true;
    }
}

pub fn frame(app: &mut App, _ctx: &egui::Context) {
    ensure_io(app);
    if let Some(pg) = app.tm.mx.win.program_in.take() {
        program_change(app, pg);
    }
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    binaural_window(app, ctx);
    io_window(app, ctx);
    components_dialog(app, ctx);
    cst_save_dialog(app, ctx);
    search_window(app, ctx);
}

fn cst_save_dialog(app: &mut App, ctx: &egui::Context) {
    let Some((s, mut name, perf, mut number)) = app.tm.mx.win.cst_save.clone() else { return };
    let (mut save, mut cancel) = (false, false);
    let folder = cst_dir(mixstate::kind_of(app, s)).map(|d| if perf { d.join("Performance") } else { d });
    egui::Window::new(if perf { "Save as Performance" } else { "Save Channel Strip Setting as" }).id(egui::Id::unique("tm-cst-save")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.horizontal(|ui| {
            ui.label("Save As:");
            ui.add(egui::TextEdit::singleline(&mut name).desired_width(250.0)).request_focus();
        });
        if perf {
            ui.horizontal(|ui| {
                ui.label("Program Change Number:");
                ui.add(egui::DragValue::new(&mut number).range(0..=127));
            });
        }
        if let Some(f) = &folder {
            ui.label(egui::RichText::new(format!("In {}", f.display())).color(look::text_dim()).small());
        }
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("Save").color(Color32::WHITE)).fill(look::accent())).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
    });
    if save && !name.trim().is_empty() {
        let ok = save_strip_setting(app, s, name.trim(), perf.then_some(number)).is_some();
        app.tell(if ok { "Channel strip setting saved" } else { "Couldn’t save the channel strip setting" });
        app.tm.mx.win.cst_save = None;
    } else if cancel {
        app.tm.mx.win.cst_save = None;
    } else {
        app.tm.mx.win.cst_save = Some((s, name, perf, number));
    }
}

/// Configure Channel Strip Components: which controls the strips show; the More menu stores
/// and applies the user default; Revert restores the defaults.
fn components_dialog(app: &mut App, ctx: &egui::Context) {
    if !app.tm.mx.win.components {
        return;
    }
    let mut open = true;
    let mut cmd: Option<&str> = None;
    egui::Window::new("Channel Strip Components").id(egui::Id::unique("tm-components")).open(&mut open).collapsible(false).resizable(false).default_pos(pos2(420.0, 160.0)).show(ctx, |ui| {
        ui.set_width(260.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Show on the channel strips").color(look::text_dim()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("More", |ui| {
                    for c in ["Store as User Default", "Apply User Default", "Revert to Default"] {
                        if ui.button(c).clicked() {
                            cmd = Some(c);
                            ui.close();
                        }
                    }
                });
            });
        });
        ui.separator();
        for group in [&["Audio Device Controls", "Type and Number Label", "Setting Menu", "Gain Reduction Meter", "EQ Thumbnail"][..], &["MIDI Effects", "Input / Instrument", "Audio Effects", "Sends", "Output"], &["Group", "Automation", "Icon", "Pan Knob", "VCA"], &["Track Color", "Track Stacks", "Track Name", "Track Number", "Control Surface Bars", "Track Notes"]] {
            for label in group {
                let Some(key) = crate::mixext::comp_key(label) else { continue };
                let mut on = crate::mixext::comp(app, key);
                if ui.checkbox(&mut on, *label).changed() {
                    crate::mixext::set_comp(app, key, on);
                }
            }
            ui.separator();
        }
        if ui.button("Revert").clicked() {
            cmd = Some("Revert to Default");
        }
    });
    if let Some(c) = cmd {
        command(app, ctx, c);
    }
    if !open {
        app.tm.mx.win.components = false;
    }
}

/// The I/O Labels window: Channel | Provided by Driver | User | Long | Short, per device.
fn io_window(app: &mut App, ctx: &egui::Context) {
    if !app.tm.mx.win.io_open {
        return;
    }
    let mut open = true;
    let mut io = app.tm.mx.win.io.clone();
    let mut edit = app.tm.mx.win.io_edit.take();
    let devices: Vec<String> = {
        let mut d = crate::prefs::devices(true);
        for x in crate::prefs::devices(false) {
            if !d.contains(&x) {
                d.push(x);
            }
        }
        d
    };
    if io.device.is_empty() {
        io.device = devices.first().cloned().unwrap_or_else(|| "System Default".into());
    }
    egui::Window::new("I/O Labels").id(egui::Id::unique("tm-io-labels")).open(&mut open).collapsible(false).default_size(vec2(560.0, 460.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Device:");
            egui::ComboBox::from_id_salt("io-device").selected_text(io.device.clone()).show_ui(ui, |ui| {
                for d in &devices {
                    ui.selectable_value(&mut io.device, d.clone(), d);
                }
            });
            ui.menu_button("Reset", |ui| {
                for (label, k) in [("Reset All Labels", None), ("Reset Input Labels", Some('i')), ("Reset Output Labels", Some('o')), ("Reset Bus Labels", Some('b'))] {
                    if ui.button(label).clicked() {
                        io.reset(k);
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("io-grid").striped(true).num_columns(5).spacing(vec2(16.0, 3.0)).show(ui, |ui| {
                for h in ["Channel", "Provided by Driver", "User", "Long", "Short"] {
                    ui.label(egui::RichText::new(h).strong());
                }
                ui.end_row();
                let rows = (0..16).map(Io::Input).chain((0..16).map(Io::Output)).chain((0..64).map(Io::Bus));
                for ch in rows {
                    let key = (io.device.clone(), ch.kind(), ch.n());
                    let mut l = io.labels.get(&key).cloned().unwrap_or_default();
                    let before = l.clone();
                    // the three columns' buttons: which label the channel uses
                    if ui.radio(l.mode == 0, ch.default_label(true)).clicked() {
                        l.mode = 0;
                    }
                    // (the system's devices supply no channel names here: a hyphen)
                    if ui.radio(l.mode == 1, "-").clicked() {
                        l.mode = 1;
                    }
                    if ui.radio(l.mode == 2, "").clicked() {
                        l.mode = 2;
                    }
                    for long in [true, false] {
                        let text = if long { &mut l.long } else { &mut l.short };
                        match edit.as_mut().filter(|e| e.0 == ch.kind() && e.1 == ch.n() && e.2 == long) {
                            Some(e) => {
                                let te = ui.add(egui::TextEdit::singleline(&mut e.3).desired_width(if long { 140.0 } else { 70.0 }));
                                te.request_focus();
                                if te.lost_focus() {
                                    *text = e.3.trim().to_string();
                                    edit = None;
                                }
                            }
                            None => {
                                let shown = if text.is_empty() { "-".to_string() } else { text.clone() };
                                let lab = ui.add_enabled(l.mode == 2, egui::Label::new(shown).sense(Sense::click()));
                                if lab.double_clicked() {
                                    edit = Some((ch.kind(), ch.n(), long, text.clone()));
                                }
                            }
                        }
                    }
                    ui.end_row();
                    if l != before {
                        io.labels.insert(key, l);
                    }
                }
            });
        });
    });
    if io != app.tm.mx.win.io {
        io.save();
        app.tm.mx.win.io = io;
    }
    app.tm.mx.win.io_edit = edit;
    if !open {
        app.tm.mx.win.io_open = false;
    }
}

/// Mix ▸ Search and Add Plug-in…: the chosen effect goes into the focused strip's first free
/// Audio FX slot; on an instrument strip with no instrument the list is the instruments, and
/// the choice fills the Instrument slot.
fn search_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut q) = app.tm.mx.win.search.clone() else { return };
    let Some(s) = app.mix.selected.iter().next().copied().or(app.selected_track) else {
        app.tm.mx.win.search = None;
        return;
    };
    let needs_instrument = s < AUX && app.project.as_ref().and_then(|p| p.tracks.get(s)).is_some_and(|t| t.kind == TrackKind::Instrument && !t.plugins.iter().any(|u| u.slot == 2 && !u.name.is_empty()) && t.extra.set.external.is_none());
    let mut names: Vec<String> = if needs_instrument { lg_synth::list().iter().filter_map(|i| i.logic_names.first().map(|n| n.to_string())).collect() } else { crate::units::effect_names().into_iter().map(|n| n.to_string()).collect() };
    names.sort();
    names.dedup();
    let ql = q.to_lowercase();
    names.retain(|n| ql.is_empty() || n.to_lowercase().contains(&ql));
    let mut pick = None;
    let mut close = false;
    let screen = ctx.content_rect();
    let resp = egui::Area::new(egui::Id::unique("tm-search-plugin")).order(egui::Order::Foreground).fixed_pos(pos2(screen.center().x - 160.0, screen.min.y + 130.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(320.0);
            ui.label(egui::RichText::new(if needs_instrument { "Search and Add Instrument" } else { "Search and Add Plug-in" }).color(look::text_dim()));
            ui.add(egui::TextEdit::singleline(&mut q).hint_text("Plug-in name").desired_width(300.0)).request_focus();
            ui.separator();
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for (k, n) in names.iter().enumerate() {
                    if ui.add(egui::Button::selectable(k == 0, n.as_str()).min_size(vec2(300.0, 0.0))).clicked() {
                        pick = Some(n.clone());
                    }
                }
            });
        });
    });
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Enter) {
            pick = names.first().cloned();
        }
        if i.key_pressed(egui::Key::Escape) || (i.pointer.any_pressed() && i.pointer.latest_pos().is_some_and(|p| !resp.response.rect.expand(4.0).contains(p))) {
            close = true;
        }
    });
    if let Some(n) = pick {
        add_plugin(app, s, &n, needs_instrument);
        close = true;
    }
    app.tm.mx.win.search = if close { None } else { Some(q) };
}

/// Adds a plug-in to strip `s`: an instrument into the Instrument slot, an effect into the
/// first free Audio FX slot.
pub fn add_plugin(app: &mut App, s: usize, name: &str, instrument: bool) {
    if instrument {
        crate::hosting::set_instrument(app, s, Some(name.to_string()));
        app.instrument_choice.insert(s, name.to_string());
    } else {
        mixstate::checkpoint(app, "Insert Plug-in");
        mixstate::edit_inserts(app, s, |v| v.push(mixstate::new_plugin(name)));
        if app.logic_settings.on(crate::prefs::OPEN_ON_INSERT) {
            app.mix.fx_window = Some((s, mixstate::inserts(app, s).len() - 1));
        }
    }
    app.load_engine();
}

/// The Binaural Panner window: the panning plane with its pucks, Angle / Elevation / Distance /
/// Spread, the Planar and Spherical modes with the Mode image, Vertical Offset and Tilt (planar
/// only), Doppler, Size, and Diffuse-Field Compensation behind the disclosure arrow.
fn binaural_window(app: &mut App, ctx: &egui::Context) {
    let Some(s) = app.tm.mx.bin_window else { return };
    let set = crate::tm::strip(app, s);
    let mut b = set.binaural;
    let stereo = set.format.is_stereo();
    let mut open = true;
    let mut extended = app.tm.mx.bin_extended;
    let before = b;
    egui::Window::new(format!("Binaural Panner — {}", mixstate::strip_name(app, s))).id(egui::Id::unique("tm-binaural")).open(&mut open).collapsible(false).resizable(false).default_pos(pos2(460.0, 150.0)).show(ctx, |ui| {
        ui.set_width(430.0);
        ui.horizontal(|ui| {
            // the fields follow the pucks (and can be dragged themselves)
            ui.add(egui::DragValue::new(&mut b.angle).range(-180.0..=180.0).speed(0.5).prefix("Angle ").suffix("°").fixed_decimals(1));
            ui.add(egui::DragValue::new(&mut b.elevation).range(-90.0..=90.0).speed(0.5).prefix("Elevation ").suffix("°").fixed_decimals(1));
            ui.add(egui::DragValue::new(&mut b.distance).range(0.0..=b.size).speed(0.01).prefix("Distance ").suffix(" m").fixed_decimals(2));
            // (Spread: stereo strips only)
            ui.add_enabled(stereo, egui::DragValue::new(&mut b.spread).range(0.0..=180.0).speed(0.5).prefix("Spread ").suffix("°").fixed_decimals(1));
        });
        ui.horizontal(|ui| {
            let (plane, _) = ui.allocate_exact_size(vec2(240.0, 240.0), Sense::hover());
            let p = ui.painter_at(plane);
            let c = plane.center();
            let rad = 110.0;
            p.circle_filled(c, rad, look::lcd_bg());
            p.circle_stroke(c, rad, Stroke::new(1.0, look::text_dim()));
            for f in [0.33, 0.66] {
                p.circle_stroke(c, rad * f, Stroke::new(0.5, look::line()));
            }
            p.line_segment([c - vec2(rad, 0.0), c + vec2(rad, 0.0)], Stroke::new(0.5, look::line()));
            p.line_segment([c - vec2(0.0, rad), c + vec2(0.0, rad)], Stroke::new(0.5, look::line()));
            p.text(c - vec2(0.0, rad - 8.0), Align2::CENTER_CENTER, if b.planar { "front" } else { "front · overhead at the centre" }, look::regular(9.0), look::text_dim());
            // the listening position
            p.circle_filled(c, 3.0, Color32::from_rgb(0x4c, 0xd9, 0x64));
            let at = |x: f32, y: f32| c + vec2(x, -y) * rad;
            let puck_of = |b: &Binaural, angle: f32| {
                let t = Binaural { angle, ..*b };
                t.puck()
            };
            let (px, py) = b.puck();
            let dir = at(px, py);
            let id = ui.scope_id().with("bin-plane");
            let resp = ui.interact(plane, id, Sense::click_and_drag());
            let three = stereo && b.spread > 0.0;
            let (la, ra) = b.sides();
            let (lp, rp) = (puck_of(&b, la), puck_of(&b, ra));
            if resp.drag_started() {
                mixstate::checkpoint(app, "Binaural Pan");
                let o = ui.input(|i| i.pointer.press_origin()).unwrap_or(dir);
                let part: u8 = if three && o.distance(at(lp.0, lp.1)) < 9.0 { 1 } else if three && o.distance(at(rp.0, rp.1)) < 9.0 { 2 } else { 0 };
                ui.data_mut(|d| d.insert_temp(id, part));
            }
            if resp.dragged() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    let v = (pp - c) / rad;
                    let part: u8 = ui.data(|d| d.get_temp(id)).unwrap_or(0);
                    match part {
                        // a side puck: wider or narrower around the direction
                        1 | 2 => {
                            let a = v.x.atan2(-v.y).to_degrees();
                            b.spread = (2.0 * lg_core::trackmix::wrap_deg(a - b.angle).abs()).clamp(0.0, 180.0);
                        }
                        // the direction puck: everything moves with it
                        _ => b.set_puck(v.x, -v.y),
                    }
                }
            }
            if resp.clicked() && ui.input(|i| i.modifiers.alt) {
                mixstate::checkpoint(app, "Binaural Pan");
                b.reset_position();
            }
            if three {
                p.circle_filled(at(lp.0, lp.1), 6.0, Color32::from_rgb(0x5a, 0x9a, 0xd8));
                p.text(at(lp.0, lp.1), Align2::CENTER_CENTER, "L", look::bold(8.0), Color32::WHITE);
                p.circle_filled(at(rp.0, rp.1), 6.0, Color32::from_rgb(0x5a, 0x9a, 0xd8));
                p.text(at(rp.0, rp.1), Align2::CENTER_CENTER, "R", look::bold(8.0), Color32::WHITE);
            }
            p.circle_filled(dir, 7.0, Color32::from_rgb(0x9c, 0xc8, 0xf0));
            p.circle_stroke(dir, 7.0, Stroke::new(1.0, Color32::WHITE));
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Panning Mode").color(look::text_dim()));
                ui.horizontal(|ui| {
                    if ui.selectable_label(b.planar, "Planar").clicked() {
                        b.planar = true;
                        b.apply_tilt();
                    }
                    if ui.selectable_label(!b.planar, "Spherical").clicked() {
                        b.planar = false;
                        b.elevation = b.elevation.max(0.0);
                    }
                });
                // the Mode image: the result from the side — the listener (green), the source
                let (img, _) = ui.allocate_exact_size(vec2(160.0, 90.0), Sense::hover());
                let ip = ui.painter_at(img);
                ip.rect_filled(img, 4.0, look::lcd_bg());
                let ic = pos2(img.center().x, img.max.y - 22.0);
                ip.circle_filled(ic, 3.0, Color32::from_rgb(0x4c, 0xd9, 0x64));
                if b.planar {
                    // the plane's tilt and height
                    let slope = b.tilt_amount.to_radians().tan() * 0.5;
                    let h = -b.vertical_offset * 30.0;
                    ip.line_segment([ic + vec2(-66.0, h + slope * 66.0), ic + vec2(66.0, h - slope * 66.0)], Stroke::new(1.5, look::text_dim()));
                } else {
                    let pts: Vec<egui::Pos2> = (0..=24).map(|k| {
                        let a = std::f32::consts::PI * k as f32 / 24.0;
                        ic + vec2(a.cos(), -a.sin()) * 60.0
                    }).collect();
                    ip.add(egui::Shape::line(pts, Stroke::new(1.5, look::text_dim())));
                }
                let el = b.elevation.to_radians();
                let front = b.angle.to_radians().cos();
                let d = (b.distance / b.size.max(0.01)).clamp(0.05, 1.0) * 60.0;
                ip.circle_filled(ic + vec2(front * el.cos() * d, -el.sin() * d), 4.0, Color32::from_rgb(0x9c, 0xc8, 0xf0));
                let ir = ui.interact(img, ui.scope_id().with("bin-mode"), Sense::drag());
                if ir.dragged() && b.planar {
                    // dragging the image: up and down the height, sideways the tilt
                    let dd = ir.drag_delta();
                    b.vertical_offset = (b.vertical_offset - dd.y / 60.0).clamp(-1.0, 1.0);
                    b.tilt_amount = (b.tilt_amount + dd.x / 2.0).clamp(-90.0, 90.0);
                    b.apply_tilt();
                }
                if b.planar {
                    let mut ch = false;
                    ch |= ui.add(egui::DragValue::new(&mut b.vertical_offset).range(-1.0..=1.0).speed(0.01).prefix("Vertical Offset ").fixed_decimals(2)).changed();
                    ch |= ui.add(egui::DragValue::new(&mut b.tilt_amount).range(-90.0..=90.0).speed(0.5).prefix("Tilt Amount ").suffix("°")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut b.tilt_direction).range(-180.0..=180.0).speed(0.5).prefix("Tilt Direction ").suffix("°")).changed();
                    if ch {
                        b.apply_tilt();
                    }
                }
                if ui.selectable_label(b.doppler, "Doppler").on_hover_text("The pitch shifts as the source moves").clicked() {
                    b.doppler = !b.doppler;
                }
                ui.horizontal(|ui| {
                    // Size: the plane's (or sphere's) radius; Option-click: 1.50 m
                    let r = ui.add(egui::DragValue::new(&mut b.size).range(0.3..=10.0).speed(0.01).prefix("Size ").suffix(" m").fixed_decimals(2));
                    if r.clicked() && ui.input(|i| i.modifiers.alt) {
                        b.size = BINAURAL_SIZE_DEFAULT;
                    }
                });
                ui.add(egui::Slider::new(&mut b.size, 0.3..=10.0).show_value(false));
            });
        });
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(if extended { "▾" } else { "▸" }).frame(false)).on_hover_text("Extended parameters").clicked() {
                extended = !extended;
            }
            if extended {
                ui.checkbox(&mut b.diffuse_field, "Diffuse-Field Compensation").on_hover_text("A neutral sound for headphones");
            }
        });
    });
    app.tm.mx.bin_extended = extended;
    if b != before {
        b.distance = b.distance.min(b.size);
        crate::tm::edit_strip(app, s, |x| x.binaural = b);
    }
    if !open {
        app.tm.mx.bin_window = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_labels_per_device() {
        let mut io = IoLabels { device: "Interface".into(), ..Default::default() };
        assert_eq!(io.label(Io::Input(0), true), "Input 1");
        assert_eq!(io.label(Io::Input(0), false), "In 1");
        assert_eq!(io.label(Io::Bus(4), true), "Bus 5");
        io.labels.insert(("Interface".into(), 'i', 0), Label { mode: 2, long: "Vocal Microphone".into(), short: "Vox".into() });
        io.labels.insert(("Interface".into(), 'b', 4), Label { mode: 2, long: "Reverb".into(), short: String::new() });
        io.labels.insert(("Interface".into(), 'o', 1), Label { mode: 0, long: "Unused".into(), short: "Un".into() });
        assert_eq!(io.label(Io::Input(0), true), "Vocal Microphone");
        assert_eq!(io.label(Io::Input(0), false), "Vox");
        assert_eq!(io.label(Io::Bus(4), false), "Reverb", "one name serves for both");
        assert_eq!(io.label(Io::Output(1), true), "Output 2", "the Channel column: the default label");
        let back = IoLabels::from_text(&io.to_text());
        assert_eq!(back, io);
        // another device has its own labels
        let other = IoLabels { device: "Other".into(), ..io.clone() };
        assert_eq!(other.label(Io::Input(0), true), "Input 1");
        io.reset(Some('i'));
        assert_eq!(io.label(Io::Input(0), true), "Input 1");
        assert_eq!(io.label(Io::Bus(4), true), "Reverb");
        io.reset(None);
        assert_eq!(io.label(Io::Bus(4), true), "Bus 5");
    }

    #[test]
    fn channel_strip_settings_round_trip() {
        let mut app = crate::tm::test_app();
        let t = app.project.as_ref().unwrap().tracks.iter().position(|x| x.kind != TrackKind::Audio).unwrap();
        mixstate::edit_inserts(&mut app, t, |v| {
            v.clear();
            let mut c = mixstate::new_plugin("Compressor");
            c.params = vec![1.0, -20.5, 4.0];
            c.bypass = true;
            v.push(c);
        });
        app.add_send(t, 5);
        app.eqs.insert(t, crate::eq::Params::default());
        crate::tm::edit_strip(&mut app, t, |x| x.pan_mode = lg_core::trackmix::PanMode::StereoPan);
        let text = strip_setting_text(&app, t);
        assert!(text.starts_with("bramble-channel-strip-setting 1\ntype\tInstrument\n"), "{text}");
        // onto another instrument strip
        let o = app.project.as_ref().unwrap().tracks.iter().enumerate().position(|(k, x)| k != t && x.kind != TrackKind::Audio).unwrap();
        assert!(apply_strip_setting(&mut app, o, &text));
        let fx = mixstate::inserts(&app, o);
        assert_eq!(fx.len(), 1);
        assert_eq!((fx[0].name.as_str(), fx[0].bypass, fx[0].params.clone()), ("Compressor", true, vec![1.0, -20.5, 4.0]));
        assert_eq!(app.sends[&o][0].bus, 5);
        assert!(app.eqs.contains_key(&o));
        assert_eq!(crate::tm::strip(&app, o).pan_mode, lg_core::trackmix::PanMode::StereoPan);
        assert!(!apply_strip_setting(&mut app, o, "not a setting"));
        // Mixer Undo takes the load back
        assert!(mixstate::undo(&mut app));
    }

    #[test]
    fn midi_strip_values() {
        assert_eq!(pan_cc(0.0), 64);
        assert_eq!(pan_cc(-1.0), 1);
        assert_eq!(pan_cc(1.0), 127);
        assert_eq!(vol_cc(0.0), crate::automation::db_to_logic(0.0).round() as u8);
        assert!(vol_cc(-96.0) == 0);
        assert_eq!(kind_folder(Kind::Audio), "Audio");
        assert_eq!(kind_folder(Kind::Aux), "Aux");
    }

    #[test]
    fn external_tracks_send_their_mixer_data() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        crate::trackops::command(&mut app, &ctx, "New External MIDI Track");
        let t = app.selected_track.unwrap();
        if let Some(e) = crate::tm::set_mut(&mut app, t).and_then(|s| s.external.as_mut()) {
            e.port = "Test Port".into();
            e.program = Some(4);
            e.bank = (1, 2);
            e.assigns[0] = (74, 99);
        }
        if let Ok(mut i) = app.hub.outs.0.lock() {
            i.sent.clear();
        }
        assert_eq!(send_all_midi(&app), 1);
        let sent: Vec<Vec<u8>> = app.hub.outs.0.lock().unwrap().sent.iter().filter(|x| x.0 == "Test Port").map(|x| x.1.clone()).collect();
        assert!(sent.contains(&vec![0xb0, 0, 1]) && sent.contains(&vec![0xb0, 32, 2]), "bank: controllers 0 and 32");
        assert!(sent.contains(&vec![0xc0, 4]));
        assert!(sent.iter().any(|m| m[0] == 0xb0 && m[1] == 7) && sent.iter().any(|m| m[0] == 0xb0 && m[1] == 10));
        assert!(sent.contains(&vec![0xb0, 74, 99]));
    }
}
