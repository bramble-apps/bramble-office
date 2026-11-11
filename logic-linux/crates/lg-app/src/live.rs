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

/// A message from a MIDI input port: (status, data1, data2).
pub type MidiMsg = (u8, u8, u8);

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
    rx: Option<mpsc::Receiver<MidiMsg>>,
    sustain: bool,
}

impl Live {
    pub fn new() -> Live {
        Live { typing_open: false, octave_c: 60, velocity: 100, held: HashMap::new(), keys_down: HashMap::new(), _inputs: Vec::new(), input_names: Vec::new(), rx: None, sustain: false }
    }

    /// Opens every MIDI input (call once; again to pick up keyboards plugged in later).
    pub fn connect_midi(&mut self, ctx: &egui::Context) {
        if std::env::var_os("LG_SNAP").is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let mut conns = Vec::new();
        let mut names = Vec::new();
        let Ok(probe) = midir::MidiInput::new("Bramble Studio") else { return };
        for port in probe.ports() {
            let Ok(input) = midir::MidiInput::new("Bramble Studio") else { continue };
            let name = input.port_name(&port).unwrap_or_default();
            // (ALSA's own "Midi Through" port only echoes)
            if name.contains("Midi Through") {
                continue;
            }
            let tx = tx.clone();
            let ctx = ctx.clone();
            if let Ok(c) = input.connect(&port, "bramble-studio-in", move |_, msg, _| {
                if msg.len() >= 2 {
                    let _ = tx.send((msg[0], msg[1], msg.get(2).copied().unwrap_or(0)));
                    ctx.request_repaint();
                }
            }, ()) {
                conns.push(c);
                names.push(name);
            }
        }
        self._inputs = conns;
        self.input_names = names;
        self.rx = Some(rx);
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
    let pos = app.position();
    if velocity > 0 {
        app.live.held.insert(pitch, (pos, velocity));
    } else if let Some((start, vel)) = app.live.held.remove(&pitch) {
        let len = (pos - start).max(30.0) as u32;
        let n = Note { channel: 0, pitch, velocity: vel, pos: start.max(0.0) as u32, len };
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
    let msgs: Vec<MidiMsg> = app.live.rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
    let t = app.selected_track.unwrap_or(0);
    for (status, a, b) in msgs {
        match status & 0xF0 {
            0x90 => note(app, a, b),
            0x80 => note(app, a, 0),
            0xB0 => {
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
                if let Ok(mut e) = app.engine.lock() {
                    e.live_control(t, 128, if key == egui::Key::Num1 { 0 } else { 16383 });
                }
            }
            egui::Key::Num3 | egui::Key::Num4 | egui::Key::Num5 | egui::Key::Num6 | egui::Key::Num7 | egui::Key::Num8 => {
                let k = [egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7, egui::Key::Num8].iter().position(|x| *x == key).unwrap_or(0);
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

/// The Musical Typing window: the keyboard with each key's letter, the octave and velocity.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.live.typing_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Musical Typing").open(&mut open).collapsible(false).resizable(false).default_width(560.0).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(540.0, 150.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 8.0, look::lcd_bg());
        let keys = Rect::from_min_max(r.min + vec2(14.0, 40.0), r.max - vec2(14.0, 12.0));
        let whites = [0u8, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17];
        let ww = keys.width() / whites.len() as f32;
        let down: Vec<u8> = app.live.held.keys().copied().collect();
        let label_of = |off: u8| TYPING.iter().find(|(_, o)| *o == off).map(|(k, _)| k.name().to_string()).unwrap_or_default();
        for (i, off) in whites.iter().enumerate() {
            let kr = Rect::from_min_size(pos2(keys.min.x + i as f32 * ww, keys.min.y), vec2(ww - 2.0, keys.height()));
            let lit = down.contains(&(app.live.octave_c + off));
            p.rect_filled(kr, 4.0, if lit { look::accent() } else { Color32::from_gray(0xee) });
            p.text(pos2(kr.center().x, kr.max.y - 12.0), Align2::CENTER_CENTER, label_of(*off), look::semibold(12.0), Color32::from_gray(0x40));
        }
        for off in [1u8, 3, 6, 8, 10, 13, 15] {
            let before = whites.iter().position(|w| *w == off - 1).unwrap_or(0) as f32;
            let kr = Rect::from_min_size(pos2(keys.min.x + (before + 1.0) * ww - ww * 0.3, keys.min.y), vec2(ww * 0.6, keys.height() * 0.6));
            let lit = down.contains(&(app.live.octave_c + off));
            p.rect_filled(kr, 3.0, if lit { look::accent() } else { Color32::from_gray(0x22) });
            p.text(pos2(kr.center().x, kr.max.y - 10.0), Align2::CENTER_CENTER, label_of(off), look::semibold(11.0), Color32::from_gray(0xd0));
        }
        let oct = app.live.octave_c as i32 / 12 - 2;
        p.text(r.min + vec2(14.0, 18.0), Align2::LEFT_CENTER, format!("Octave: C{oct}   (Z ◀  X ▶)"), look::regular(12.0), look::lcd_text());
        p.text(pos2(r.max.x - 14.0, r.min.y + 18.0), Align2::RIGHT_CENTER, format!("Velocity: {}   (C ◀  V ▶)", app.live.velocity), look::regular(12.0), look::lcd_text());
        p.rect_stroke(r, 8.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
        let inputs = if app.live.input_names.is_empty() { "No MIDI keyboard found".to_string() } else { format!("MIDI in: {}", app.live.input_names.join(", ")) };
        ui.label(egui::RichText::new(inputs).color(look::text_dim()).font(look::regular(11.0)));
    });
    if !open {
        app.live.typing_open = false;
    }
}
