//! Playing in: Musical Typing (Window > Show Musical Typing, ⌘K) and MIDI keyboards (every
//! input port, as Logic listens to all of them). Notes sound on the selected track at once and,
//! while recording, become the take; when not recording they are kept for Capture as Recording.
//!
//! Musical Typing's keys, as Logic's: A W S E D F T G Y H U J K O L P ; ' play from C, Z / X
//! change the octave, C / V the velocity, Tab holds (sustain), 1 / 2 bend the pitch, 3–8 set
//! the mod wheel.

use std::collections::HashMap;
use std::sync::mpsc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::Note;

use crate::app::App;
use crate::look;

/// A whole MIDI message and the input it came from (index in `input_names`; usize::MAX: the
/// virtual input).
pub type RawMsg = (usize, Vec<u8>);

pub struct Live {
    pub typing_open: bool,
    /// Musical Typing's octave (the A key's C: 60 = C3) and velocity.
    pub octave_c: u8,
    pub velocity: u8,
    /// Notes held: pitch → (start tick, velocity).
    pub held: HashMap<u8, (f64, u8)>,
    /// Musical Typing's keys down (so a key's repeat doesn't retrigger).
    keys_down: HashMap<egui::Key, u8>,
    /// MIDI inputs: their connections and the messages they send.
    _inputs: Vec<midir::MidiInputConnection<()>>,
    pub input_names: Vec<String>,
    /// Every input port found, opened or switched off in Settings ▸ MIDI ▸ Inputs.
    pub all_ports: Vec<String>,
    #[cfg(unix)]
    _virtual: Option<midir::MidiInputConnection<()>>,
    rx: Option<mpsc::Receiver<RawMsg>>,
    sustain: bool,
    /// The window's left button: the onscreen piano instead of the computer keys.
    pub keyboard_mode: bool,
    /// The onscreen piano's lowest C, and the note the mouse holds there.
    pub piano_c: u8,
    mouse_note: Option<u8>,
    /// What Musical Typing last sent: pitch bend (−8192…8191) and the mod key (0 = off … 5 = max).
    pub bend: i16,
    pub modulation: u8,
}

impl Live {
    pub fn new() -> Live {
        Live { typing_open: false, octave_c: 60, velocity: 100, held: HashMap::new(), keys_down: HashMap::new(), _inputs: Vec::new(), input_names: Vec::new(), all_ports: Vec::new(), #[cfg(unix)] _virtual: None, rx: None, sustain: false, keyboard_mode: false, piano_c: 36, mouse_note: None, bend: 0, modulation: 0 }
    }

    /// Opens every MIDI input (call once; again to pick up keyboards plugged in later).
    pub fn connect_midi(&mut self, ctx: &egui::Context) {
        if std::env::var_os("LG_SNAP").is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let mut conns = Vec::new();
        let mut names = Vec::new();
        let mut all = Vec::new();
        // (Settings ▸ MIDI ▸ Inputs: the virtual input other apps play into)
        #[cfg(unix)]
        {
            use midir::os::unix::VirtualInput;
            self._virtual = None;
            let virt = crate::settings::Values::load();
            if crate::prefs::virtual_in(&virt) {
                if let Ok(input) = midir::MidiInput::new("Bramble Studio") {
                    let (tx, ctx) = (tx.clone(), ctx.clone());
                    let mut input = input;
                    // (SysEx and time code too: MTC, MMC, control surfaces)
                    input.ignore(midir::Ignore::ActiveSense);
                    self._virtual = input.create_virtual("Bramble Studio Virtual In", move |_, msg, _| {
                        if raw_wanted(msg) {
                            let _ = tx.send((usize::MAX, msg.to_vec()));
                            ctx.request_repaint();
                        }
                    }, ()).ok();
                }
            }
        }
        let Ok(probe) = midir::MidiInput::new("Bramble Studio") else { return };
        for port in probe.ports() {
            let Ok(mut input) = midir::MidiInput::new("Bramble Studio") else { continue };
            input.ignore(midir::Ignore::ActiveSense);
            let name = input.port_name(&port).unwrap_or_default();
            // (ALSA's own "Midi Through" port only echoes)
            if name.contains("Midi Through") || name.contains("Bramble Studio Virtual In") {
                continue;
            }
            all.push(name.clone());
            // (switched off in Settings ▸ MIDI ▸ Inputs: not opened, so not recorded from)
            if crate::prefs::input_off(&name) {
                continue;
            }
            let tx = tx.clone();
            let ctx = ctx.clone();
            let index = names.len();
            if let Ok(c) = input.connect(&port, "bramble-studio-in", move |_, msg, _| {
                if raw_wanted(msg) {
                    let _ = tx.send((index, msg.to_vec()));
                    ctx.request_repaint();
                }
            }, ()) {
                conns.push(c);
                names.push(name);
            }
        }
        self._inputs = Vec::new();
        self._inputs = conns;
        self.input_names = names;
        self.all_ports = all;
        self.rx = Some(rx);
    }
}

/// Messages worth passing on: channel messages, SysEx (MMC, MTC full frames, control surfaces),
/// MTC quarter frames, Song Position (not clock ticks or active sensing: Logic can't receive
/// MIDI Clock).
fn raw_wanted(msg: &[u8]) -> bool {
    match msg.first() {
        Some(&s) if s < 0xF0 => msg.len() >= 2,
        Some(0xF0) | Some(0xF1) | Some(0xF2) => true,
        Some(0xFA) | Some(0xFB) | Some(0xFC) => true,
        _ => false,
    }
}

impl Default for Live {
    fn default() -> Self {
        Live::new()
    }
}

const TYPING: [(egui::Key, u8); 18] = [
    (egui::Key::A, 0),
    (egui::Key::W, 1),
    (egui::Key::S, 2),
    (egui::Key::E, 3),
    (egui::Key::D, 4),
    (egui::Key::F, 5),
    (egui::Key::T, 6),
    (egui::Key::G, 7),
    (egui::Key::Y, 8),
    (egui::Key::H, 9),
    (egui::Key::U, 10),
    (egui::Key::J, 11),
    (egui::Key::K, 12),
    (egui::Key::O, 13),
    (egui::Key::L, 14),
    (egui::Key::P, 15),
    (egui::Key::Semicolon, 16),
    (egui::Key::Quote, 17),
];

/// A note starts (velocity > 0) or stops, from either source.
pub fn note(app: &mut App, pitch: u8, velocity: u8) {
    // (to the record-enabled track, else the selected one)
    let t = app.armed.iter().min().copied().or(app.selected_track).unwrap_or(0);
    if let Ok(mut e) = app.engine.lock() {
        if velocity == 0 && app.live.sustain {
            // held by the sustain: recorded as released now, sounding on
        } else {
            e.live(t, pitch, velocity);
        }
    }
    if velocity > 0 {
        crate::instwin::note_played(app, pitch);
    }
    let pos = app.position();
    if velocity > 0 {
        app.live.held.insert(pitch, (pos, velocity));
    } else if let Some((start, vel)) = app.live.held.remove(&pitch) {
        let len = (pos - start).max(30.0) as u32;
        let n = Note { channel: 0, pitch, velocity: vel, pos: start.max(0.0) as u32, len, muted: false };
        if app.recording() {
            app.recorded.push(n);
        } else if app.playing() {
            app.played.push(n);
            if app.played.len() > 2000 {
                app.played.remove(0);
            }
        }
    }
}

/// Each frame: MIDI messages in, and Musical Typing's keys when its window is open. Returns the
/// keys it used (so they don't also run key commands).
pub fn frame(app: &mut App, ctx: &egui::Context) -> Vec<egui::Key> {
    let raw: Vec<RawMsg> = app.live.rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
    let t = app.selected_track.unwrap_or(0);
    for (port, bytes) in raw {
        // control surfaces, controller assignments, MTC / MMC: taken before the Environment
        // (remote control events aren't passed on or recorded)
        if crate::envhub::midi_raw(app, ctx, port, &bytes) {
            continue;
        }
        let (status, a, b) = (bytes[0], bytes.get(1).copied().unwrap_or(0), bytes.get(2).copied().unwrap_or(0));
        if status >= 0xF0 {
            continue;
        }
        // (a program change: the performance saved under that number loads: mixwin.rs)
        if status & 0xf0 == 0xc0 {
            app.tm.mx.win.program_in = Some(a);
        }
        // (a controller message learned in the Key Commands window runs its command)
        if crate::keycmds::midi_in(app, ctx, status, a, b) {
            continue;
        }
        // the MIDI Environment: Physical Input → … → Sequencer Input (and anything else cabled)
        crate::envwin::physical_in(app, ctx, port, status, a, b);
    }
    crate::envwin::tick(app, ctx);
    keys_frame(app, ctx, t)
}

/// What reaches the Environment's Sequencer Input: the selected (or record-enabled) track plays
/// and records it.
pub fn sequencer_in(app: &mut App, ctx: &egui::Context, status: u8, a: u8, b: u8) {
    let t = app.selected_track.unwrap_or(0);
    let _ = ctx;
    {
        // Project Settings ▸ MIDI ▸ Input Filter
        if !crate::projsettings::input_passes(&app.proj, status) {
            return;
        }
        // an Articulation Set's switches take their messages (artic.rs)
        if crate::artic::midi_in(app, t, status, a, b) {
            return;
        }
        // MIDI ▸ General: CC 7 / 10 move the track's fader and pan
        if status & 0xF0 == 0xB0 && (a == 7 || a == 10) && crate::projsettings::cc_moves_strip(&app.proj) {
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                if a == 7 {
                    tr.volume_db = crate::automation::logic_to_db(b as f32);
                } else {
                    tr.pan = (b as f32 - 64.0) / 63.0;
                }
                app.sync_mix();
            }
            return;
        }
        if matches!(status & 0xF0, 0x80 | 0x90 | 0xD0) {
            if let Ok(mut e) = app.engine.lock() {
                e.live_channel = status & 0x0F;
                if status & 0xF0 == 0xD0 {
                    e.live_message(t, 0xD0, a, 0);
                }
            }
        }
        match status & 0xF0 {
            0x90 => note(app, a, b),
            0x80 => note(app, a, 0),
            0xB0 => {
                // (a controller learned or assigned to a Smart Control moves it)
                if a != 64 && (crate::autoext::midi_cc(app, a, b) || crate::smartcontrols::midi_cc(app, a, b)) {
                    return;
                }
                if a == 64 {
                    let on = b >= 64;
                    if !on && app.live.sustain {
                        app.live.sustain = false;
                    }
                    app.live.sustain = on;
                }
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, a, b as u16);
                }
            }
            0xE0 => {
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, 128, ((b as u16) << 7) | a as u16);
                }
            }
            _ => {}
        }
    }
}

fn keys_frame(app: &mut App, ctx: &egui::Context, t: usize) -> Vec<egui::Key> {
    let mut used = Vec::new();
    if !app.live.typing_open || ctx.egui_wants_keyboard_input() {
        return used;
    }
    let events: Vec<(egui::Key, bool, bool, egui::Modifiers)> = ctx.input(|i| i.events.iter().filter_map(|e| match e {
        egui::Event::Key { key, pressed, repeat, modifiers, .. } => Some((*key, *pressed, *repeat, *modifiers)),
        _ => None,
    }).collect());
    for (key, pressed, repeat, m) in events {
        if m.command || m.ctrl || m.alt {
            continue;
        }
        if let Some((_, off)) = TYPING.iter().find(|(k, _)| *k == key) {
            used.push(key);
            if repeat {
                continue;
            }
            let pitch = (app.live.octave_c as u16 + *off as u16).min(127) as u8;
            if pressed {
                app.live.keys_down.insert(key, pitch);
                note(app, pitch, app.live.velocity);
            } else if let Some(p) = app.live.keys_down.remove(&key) {
                note(app, p, 0);
            }
            continue;
        }
        if !pressed || repeat {
            if matches!(key, egui::Key::Num1 | egui::Key::Num2) && !pressed {
                // (the bend springs back when the key is let go, as a wheel does)
                app.live.bend = 0;
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, 128, 8192);
                }
                used.push(key);
            }
            if matches!(key, egui::Key::Tab) && !pressed {
                app.live.sustain = false;
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, 64, 0);
                }
                used.push(key);
            }
            continue;
        }
        match key {
            egui::Key::Z => app.live.octave_c = app.live.octave_c.saturating_sub(12).max(0),
            egui::Key::X => app.live.octave_c = (app.live.octave_c + 12).min(120),
            egui::Key::C => app.live.velocity = app.live.velocity.saturating_sub(20).max(1),
            egui::Key::V => app.live.velocity = (app.live.velocity + 20).min(127),
            egui::Key::Tab => {
                app.live.sustain = true;
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, 64, 127);
                }
            }
            egui::Key::Num1 | egui::Key::Num2 => {
                app.live.bend = if key == egui::Key::Num1 { -8192 } else { 8191 };
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, 128, if key == egui::Key::Num1 { 0 } else { 16383 });
                }
            }
            egui::Key::Num3 | egui::Key::Num4 | egui::Key::Num5 | egui::Key::Num6 | egui::Key::Num7 | egui::Key::Num8 => {
                let k = [egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7, egui::Key::Num8].iter().position(|x| *x == key).unwrap_or(0);
                app.live.modulation = k as u8;
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, 1, (k as u16 * 127) / 5);
                }
            }
            _ => continue,
        }
        used.push(key);
    }
    used
}

/// A MIDI port's name as Logic shows it: the device's port, without the ALSA client prefix
/// ("Client:") or the "128:0" address ("Launchkey Mini:Launchkey Mini MIDI 1 20:0" →
/// "Launchkey Mini MIDI 1"). Display only: settings and connections keep the full name.
pub fn port_label(name: &str) -> String {
    let mut n = name.trim();
    // the address at the end: " 20:0"
    if let Some((head, tail)) = n.rsplit_once(' ') {
        if let Some((a, b)) = tail.split_once(':') {
            if !a.is_empty() && !b.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) {
                n = head.trim_end();
            }
        }
    }
    // "Client:Port": the port's own name
    if let Some((client, port)) = n.split_once(':') {
        let port = port.trim();
        if !port.is_empty() && !client.trim().is_empty() {
            return port.to_string();
        }
    }
    n.to_string()
}

/// The Window menu's keyboard commands this file runs (build.rs and the checklist read it).
pub const COMMANDS: &[&str] = &["Show Keyboard", "Show/Hide Keyboard"];

/// Window ▸ Show Keyboard: the Musical Typing window in its onscreen keyboard mode (again: hides it).
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Show Keyboard" | "Show/Hide Keyboard" => {
            if app.live.typing_open && app.live.keyboard_mode {
                app.live.typing_open = false;
            } else {
                app.live.typing_open = true;
                app.live.keyboard_mode = true;
            }
            true
        }
        _ => false,
    }
}

/// A note's name as Logic writes it (middle C = C3).
fn c_name(n: u8) -> String {
    format!("C{}", n as i32 / 12 - 2)
}

const IS_BLACK: [bool; 12] = [false, true, false, true, false, false, true, false, true, false, true, false];

/// The Musical Typing window, as Logic's: the two buttons (onscreen keyboard | Musical Typing)
/// and the keyboard overview with the range played, then either the computer keys (Pitch Bend
/// and Modulation keys, sustain, the notes, Octave and Velocity) or an onscreen piano.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.live.typing_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Musical Typing").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let (r, _) = ui.allocate_exact_size(vec2(690.0, 262.0), Sense::hover());
        let x0 = r.min.x - 8.0;
        let at = |x: f32, y: f32| pos2(x0 + x, r.min.y + y);
        let p = ui.painter().clone();
        toolbar(app, ui, &p, r, &at);
        // (the toolbar's bottom line runs to the window's edges)
        p.hline((r.min.x - 8.0)..=(r.max.x + 8.0), r.min.y + 29.5, Stroke::new(1.0, Color32::from_black_alpha(160)));
        if app.live.keyboard_mode {
            piano(app, ui, &p, Rect::from_min_max(at(14.0, 44.0), at(692.0, 254.0)));
        } else {
            typing_keys(app, &p, &at);
        }
    });
    if !open {
        app.live.typing_open = false;
        if let Some(n) = app.live.mouse_note.take() {
            note(app, n, 0);
        }
    }
}

/// The two mode buttons (a joined pill) and the overview of all 128 notes with the range shown.
fn toolbar(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, at: &dyn Fn(f32, f32) -> egui::Pos2) {
    // the pill: piano (onscreen keyboard) | computer keys (Musical Typing)
    let pill = Rect::from_min_max(at(14.0, 0.0), at(78.0, 21.0));
    p.rect_filled(pill, 5.0, Color32::from_gray(0x3a));
    for (i, (icon, kb)) in [(crate::glyphs::ph::PIANO_KEYS, true), (crate::glyphs::ph::KEYBOARD, false)].into_iter().enumerate() {
        let b = Rect::from_min_size(pill.min + vec2(1.0 + i as f32 * 31.0, 1.0), vec2(31.0, 19.0));
        let on = app.live.keyboard_mode == kb;
        if on {
            p.rect_filled(b, 4.0, look::accent());
        }
        crate::glyphs::paint(p, b, icon, 16.0, if on { Color32::WHITE } else { Color32::from_gray(0xd8) });
        let resp = ui.interact(b, ui.scope_id().with(("mt-mode", i)), Sense::click()).on_hover_text(if kb { "Keyboard" } else { "Musical Typing" });
        if resp.clicked() {
            app.live.keyboard_mode = kb;
        }
    }
    // the overview: the range played is the blue box; click to move it, the arrows move it an octave
    let strip = Rect::from_min_max(at(239.0, 0.0), at(466.0, 21.0));
    p.rect_filled(strip, 5.0, Color32::from_gray(0x2c));
    let keys = strip.shrink2(vec2(4.0, 3.0));
    let whites: Vec<u8> = (0..128u8).filter(|n| !IS_BLACK[*n as usize % 12]).collect();
    let ww = keys.width() / whites.len() as f32;
    let x_of = |n: u8| keys.min.x + (0..n).filter(|m| !IS_BLACK[*m as usize % 12]).count() as f32 * ww;
    let (lo, hi) = if app.live.keyboard_mode { (app.live.piano_c, (app.live.piano_c as u16 + 36).min(127) as u8) } else { (app.live.octave_c, (app.live.octave_c as u16 + 17).min(127) as u8) };
    let range = Rect::from_min_max(pos2(x_of(lo) - 1.0, strip.min.y), pos2(x_of(hi) + ww + 1.0, strip.max.y));
    p.rect_filled(range, 0.0, Color32::from_rgb(0x2f, 0x7c, 0xe8));
    for (i, n) in whites.iter().enumerate() {
        let inside = *n >= lo && *n <= hi;
        let kr = Rect::from_min_size(pos2(keys.min.x + i as f32 * ww, keys.min.y), vec2((ww - 0.6).max(0.5), keys.height()));
        p.rect_filled(kr, 0.0, if inside { Color32::from_rgb(0xd6, 0xe6, 0xfc) } else { Color32::from_gray(0xe8) });
    }
    for n in (0..128u8).filter(|n| IS_BLACK[*n as usize % 12]) {
        let kr = Rect::from_min_size(pos2(x_of(n) - ww * 0.3, keys.min.y), vec2(ww * 0.6, keys.height() * 0.6));
        p.rect_filled(kr, 0.0, Color32::from_gray(0x18));
    }
    let resp = ui.interact(strip, ui.scope_id().with("mt-overview"), Sense::click_and_drag());
    if let Some(pt) = resp.interact_pointer_pos().filter(|_| resp.clicked() || resp.dragged()) {
        let w = (((pt.x - keys.min.x) / ww).floor().max(0.0) as usize).min(whites.len() - 1);
        let span = if app.live.keyboard_mode { 36 } else { 17 };
        // (the range is centred on the click, starting on a C)
        let c = ((whites[w] as i32 - span / 2 + 6) / 12 * 12).clamp(0, 120) as u8;
        set_range(app, c);
    }
    for (dx, icon, d) in [(228.0, crate::glyphs::ph::CARET_LEFT, -12i32), (477.0, crate::glyphs::ph::CARET_RIGHT, 12)] {
        let b = Rect::from_center_size(at(dx, 10.5), vec2(14.0, 21.0));
        let resp = ui.interact(b, ui.scope_id().with(("mt-shift", d)), Sense::click());
        crate::glyphs::paint(p, b, icon, 13.0, if resp.hovered() { Color32::WHITE } else { Color32::from_gray(0xa0) });
        if resp.clicked() {
            let cur = if app.live.keyboard_mode { app.live.piano_c } else { app.live.octave_c } as i32;
            set_range(app, (cur + d).clamp(0, 120) as u8);
        }
    }
    let _ = r;
}

/// The computer key that plays `n` (Musical Typing's letters on the onscreen piano, as Logic shows them).
fn typing_label(octave_c: u8, n: u8) -> Option<&'static str> {
    let off = n.checked_sub(octave_c)?;
    const L: [&str; 18] = ["A", "W", "S", "E", "D", "F", "T", "G", "Y", "H", "U", "J", "K", "O", "L", "P", ";", "'"];
    L.get(off as usize).copied()
}

fn set_range(app: &mut App, c: u8) {
    if app.live.keyboard_mode {
        app.live.piano_c = c.min(84);
    } else {
        app.live.octave_c = c.min(120);
    }
}

/// One key cap: rounded, its key bottom-left and what it does top-right.
fn cap(p: &egui::Painter, kr: Rect, fill: Color32, key: &str, hint: &str, key_col: Color32, hint_col: Color32) {
    p.rect_filled(kr, 5.0, fill);
    p.text(pos2(kr.min.x + 9.0, kr.max.y - 12.0), Align2::LEFT_CENTER, key, look::regular(13.0), key_col);
    if !hint.is_empty() {
        p.text(pos2(kr.max.x - 9.0, kr.min.y + 12.0), Align2::RIGHT_CENTER, hint, look::regular(12.0), hint_col);
    }
}

/// Musical Typing's keys, laid out as Logic draws them (points from the window's left edge).
fn typing_keys(app: &mut App, p: &egui::Painter, at: &dyn Fn(f32, f32) -> egui::Pos2) {
    let label = Color32::from_gray(0x9a);
    let value = Color32::from_gray(0xf0);
    let lit = |c: Color32| look::mix(c, Color32::WHITE, 0.35);
    let dark = Color32::from_gray(0x14);
    // Pitch Bend 1 2 · Modulation 3–8
    p.text(at(33.0, 53.0), Align2::LEFT_CENTER, "Pitch Bend:", look::regular(12.0), label);
    p.text(at(112.5, 53.0), Align2::CENTER_CENTER, app.live.bend.to_string(), look::semibold(12.0), value);
    let blue = Color32::from_rgb(0x2e, 0x6c, 0xb5);
    let purple = Color32::from_rgb(0x8b, 0x39, 0xb8);
    let purple_off = Color32::from_rgb(0x6b, 0x2c, 0x94);
    for i in 0..8usize {
        let kr = Rect::from_min_size(at(124.0 + i as f32 * 48.0, 40.5), vec2(46.0, 50.0));
        let (fill, hint) = match i {
            0 => (blue, "-"),
            1 => (blue, "+"),
            2 => (purple_off, "off"),
            7 => (purple, "max"),
            _ => (purple, ""),
        };
        let on = match i {
            0 => app.live.bend < 0,
            1 => app.live.bend > 0,
            _ => app.live.modulation as usize == i - 2 && i > 2,
        };
        let (kc, hc) = if i == 2 { (Color32::WHITE, Color32::WHITE) } else { (dark, if i == 1 { Color32::WHITE } else { dark }) };
        cap(p, kr, if on { lit(fill) } else { fill }, &(i + 1).to_string(), hint, kc, hc);
    }
    p.text(at(514.0, 53.0), Align2::LEFT_CENTER, "Modulation", look::regular(12.0), label);
    // sustain (Tab)
    let green = Color32::from_rgb(0x3a, 0x8a, 0x3d);
    cap(p, Rect::from_min_max(at(75.0, 95.0), at(151.0, 141.5)), if app.live.sustain { lit(green) } else { green }, "tab", "sustain", Color32::from_rgb(0x1c, 0x4e, 0x1e), dark);
    // the notes: A S D F G H J K L ; ' and W E · T Y U · O P
    let down: Vec<u8> = app.live.held.keys().copied().collect();
    let accent = Color32::from_rgb(0x2f, 0x7c, 0xe8);
    let whites = [(0u8, "A"), (2, "S"), (4, "D"), (5, "F"), (7, "G"), (9, "H"), (11, "J"), (12, "K"), (14, "L"), (16, ";"), (17, "'")];
    for (i, (off, k)) in whites.iter().enumerate() {
        let kr = Rect::from_min_size(at(167.0 + i as f32 * 47.0, 95.0), vec2(45.0, 95.5));
        let on = down.contains(&(app.live.octave_c.saturating_add(*off)));
        cap(p, kr, if on { accent } else { Color32::WHITE }, k, "", if on { Color32::WHITE } else { Color32::BLACK }, dark);
    }
    for (before, off, k) in [(0.0, 1u8, "W"), (1.0, 3, "E"), (3.0, 6, "T"), (4.0, 8, "Y"), (5.0, 10, "U"), (7.0, 13, "O"), (8.0, 15, "P")] {
        let kr = Rect::from_min_size(at(167.0 + (before + 0.70) * 47.0, 95.0), vec2(45.0, 50.0));
        let on = down.contains(&(app.live.octave_c.saturating_add(off)));
        // (a thin gap of the window's colour around each black key, as the white keys show it)
        p.rect_filled(kr.expand(1.0), 6.0, look::window_bg());
        cap(p, kr, if on { accent } else { Color32::BLACK }, k, "", Color32::WHITE, dark);
    }
    // Octave Z X · Velocity C V
    p.text(at(156.0, 205.0), Align2::RIGHT_CENTER, "Octave:", look::regular(12.0), label);
    p.text(at(166.5, 205.0), Align2::LEFT_CENTER, c_name(app.live.octave_c), look::semibold(12.0), value);
    let gold = Color32::from_rgb(0xc2, 0x95, 0x30);
    let orange = Color32::from_rgb(0xc3, 0x67, 0x2b);
    for (i, (k, hint, fill)) in [("Z", "-", gold), ("X", "+", gold), ("C", "-", orange), ("V", "+", orange)].into_iter().enumerate() {
        let kr = Rect::from_min_size(at(189.0 + i as f32 * 47.0, 195.0), vec2(45.0, 46.5));
        cap(p, kr, fill, k, hint, dark, if i % 2 == 0 { dark } else { Color32::WHITE });
    }
    p.text(at(453.0, 205.0), Align2::RIGHT_CENTER, "Velocity:", look::regular(12.0), label);
    p.text(at(465.0, 205.0), Align2::LEFT_CENTER, app.live.velocity.to_string(), look::semibold(12.0), value);
}

/// The onscreen piano: three octaves from `piano_c`; a click plays (lower on a key = louder),
/// the computer keys' notes light up too.
fn piano(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, area: Rect) {
    let lo = app.live.piano_c;
    let notes: Vec<u8> = (lo..=(lo as u16 + 36).min(127) as u8).collect();
    let whites: Vec<u8> = notes.iter().copied().filter(|n| !IS_BLACK[*n as usize % 12]).collect();
    let ww = area.width() / whites.len() as f32;
    let down: Vec<u8> = app.live.held.keys().copied().collect();
    let accent = Color32::from_rgb(0x2f, 0x7c, 0xe8);
    let mut rects: Vec<(u8, Rect)> = Vec::new();
    for (i, n) in whites.iter().enumerate() {
        let kr = Rect::from_min_size(pos2(area.min.x + i as f32 * ww, area.min.y), vec2(ww - 1.5, area.height()));
        p.rect_filled(kr, egui::CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 }, if down.contains(n) { accent } else { Color32::from_gray(0xee) });
        if n % 12 == 0 {
            p.text(pos2(kr.center().x, kr.max.y - 14.0), Align2::CENTER_CENTER, c_name(*n), look::semibold(11.0), Color32::from_gray(0x80));
        }
        if let Some(k) = typing_label(app.live.octave_c, *n) {
            p.text(pos2(kr.center().x, kr.max.y - 44.0), Align2::CENTER_CENTER, k, look::semibold(12.0), Color32::from_gray(0x70));
        }
        rects.push((*n, kr));
    }
    let mut blacks: Vec<(u8, Rect)> = Vec::new();
    for n in notes.iter().copied().filter(|n| IS_BLACK[*n as usize % 12]) {
        let i = whites.iter().position(|w| *w == n - 1).unwrap_or(0) as f32;
        let kr = Rect::from_min_size(pos2(area.min.x + (i + 1.0) * ww - ww * 0.32 - 0.75, area.min.y), vec2(ww * 0.64, area.height() * 0.62));
        p.rect_filled(kr, egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, if down.contains(&n) { accent } else { Color32::from_gray(0x1c) });
        if let Some(k) = typing_label(app.live.octave_c, n) {
            p.text(pos2(kr.center().x, kr.max.y - 12.0), Align2::CENTER_CENTER, k, look::semibold(11.0), Color32::from_gray(0xc8));
        }
        blacks.push((n, kr));
    }
    let resp = ui.interact(area, ui.scope_id().with("mt-piano"), Sense::click_and_drag());
    let hit = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()).and_then(|pt| {
        blacks.iter().chain(rects.iter()).find(|(_, kr)| kr.contains(pt)).map(|(n, kr)| (*n, ((pt.y - kr.min.y) / kr.height()).clamp(0.0, 1.0)))
    });
    let now = hit.map(|h| h.0);
    if now != app.live.mouse_note {
        if let Some(n) = app.live.mouse_note.take() {
            note(app, n, 0);
        }
        if let Some((n, y)) = hit {
            // (higher on the key plays softer, lower louder)
            note(app, n, (20.0 + y * 107.0).round().clamp(1.0, 127.0) as u8);
            app.live.mouse_note = Some(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piano_letters_follow_the_typing_keys() {
        // every Musical Typing key's letter sits on the note it plays, from the octave's C
        for (key, off) in TYPING {
            let want = match key {
                egui::Key::Semicolon => ";".to_string(),
                egui::Key::Quote => "'".to_string(),
                k => k.name().to_string(),
            };
            assert_eq!(typing_label(48, 48 + off), Some(want.as_str()));
        }
        assert_eq!(typing_label(48, 47), None);
        assert_eq!(typing_label(48, 66), None);
        assert_eq!(c_name(60), "C3");
        assert_eq!(c_name(0), "C-2");
    }

    #[test]
    fn port_names_lose_the_alsa_parts() {
        assert_eq!(port_label("Launchkey Mini:Launchkey Mini MIDI 1 20:0"), "Launchkey Mini MIDI 1");
        assert_eq!(port_label("Bramble Studio:Bramble Studio Virtual Out 128:0"), "Bramble Studio Virtual Out");
        assert_eq!(port_label("USB Keystation 61"), "USB Keystation 61");
        assert_eq!(port_label("Port 2 24:1"), "Port 2");
        assert_eq!(port_label("IAC Driver Bus 1"), "IAC Driver Bus 1");
    }
}
