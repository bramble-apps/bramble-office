//! Note Repeat (Logic's toolbar button and the Note Repeat key command): while its window is
//! open, a held note — from a MIDI keyboard or Musical Typing — plays again and again at the
//! chosen Rate, with the Velocity it was played with ("As played") or a fixed one, each repeat
//! lasting the Gate's share of the rate. Played while recording, the repeats are what gets
//! recorded.

use egui::vec2;

use crate::app::App;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &["Note Repeat"];

/// The Rate pop-up: name and ticks (a quarter note is 960).
pub const RATES: [(&str, u32); 10] = [
    ("1/4", 960), ("1/4T", 640), ("1/8", 480), ("1/8T", 320), ("1/16", 240), ("1/16T", 160), ("1/32", 120), ("1/32T", 80), ("1/64", 60), ("1/64T", 40),
];

/// The window's settings (Velocity None: As played; Gate in percent) and the extended
/// settings' limits.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub rate: u32,
    pub velocity: Option<u8>,
    pub gate: u8,
    pub extended: bool,
    pub rate_min: u32,
    pub rate_max: u32,
    pub gate_min: u8,
    pub gate_max: u8,
    pub vel_min: i8,
    pub vel_max: i8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { rate: 240, velocity: None, gate: 50, extended: false, rate_min: 60, rate_max: 960, gate_min: 1, gate_max: 100, vel_min: -64, vel_max: 63 }
    }
}

/// A held note being repeated: when it was struck, its velocity, the next hit and when the
/// sounding one ends (seconds), whether it sounds, and where recording put the first hit.
#[derive(Clone, Debug, PartialEq)]
struct Voice {
    pitch: u8,
    velocity: u8,
    next: f64,
    off_at: f64,
    sounding: bool,
    first_pos: u32,
}

#[derive(Clone, Debug, Default)]
struct State {
    open: bool,
    settings: Settings,
    voices: Vec<Voice>,
    track: usize,
}

fn id() -> egui::Id {
    egui::Id::new("lg-note-repeat")
}

fn with<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<State>(id())))
}

pub fn active(ctx: &egui::Context) -> bool {
    with(ctx, |s| s.open)
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if name != "Note Repeat" {
        return false;
    }
    let stop = with(ctx, |s| {
        s.open = !s.open;
        if s.open { Vec::new() } else { std::mem::take(&mut s.voices) }
    });
    silence(app, &stop);
    true
}

fn silence(app: &mut App, voices: &[Voice]) {
    let t = app.armed.iter().min().copied().or(app.selected_track).unwrap_or(0);
    if let Ok(mut e) = app.engine.lock() {
        for v in voices.iter().filter(|v| v.sounding) {
            e.live(t, v.pitch, 0);
        }
    }
}

/// One repeat's velocity: as played or the window's.
pub fn hit_velocity(s: &Settings, played: u8) -> u8 {
    s.velocity.unwrap_or(played).clamp(1, 127)
}

/// Seconds between repeats at `bpm`.
pub fn interval(s: &Settings, bpm: f64) -> f64 {
    s.rate as f64 / 960.0 * 60.0 / bpm.max(1.0)
}

/// What happens between `from` and `to` (seconds) to a voice: the note-offs and new hits, in
/// order — (time, true = hit / false = off).
fn advance(v: &mut Voice, s: &Settings, every: f64, to: f64) -> Vec<(f64, bool)> {
    let mut out = Vec::new();
    loop {
        let off_first = v.sounding && v.off_at <= v.next;
        if off_first && v.off_at <= to {
            out.push((v.off_at, false));
            v.sounding = false;
        } else if v.next <= to {
            if v.sounding {
                out.push((v.next, false));
            }
            out.push((v.next, true));
            v.sounding = true;
            v.off_at = v.next + every * s.gate.clamp(1, 100) as f64 / 100.0;
            v.next += every;
        } else {
            break;
        }
    }
    out
}

/// Each frame: repeats the held notes while the window is open.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    if !active(ctx) {
        return;
    }
    let now = ctx.input(|i| i.time);
    let bpm = app.tempo();
    let held: Vec<(u8, u8, f64)> = app.live.held.iter().map(|(&p, &(pos, v))| (p, v, pos)).collect();
    let t = app.armed.iter().min().copied().or(app.selected_track).unwrap_or(0);
    let (settings, mut voices) = with(ctx, |s| (s.settings.clone(), std::mem::take(&mut s.voices)));
    let every = interval(&settings, bpm);
    let tick_len = (settings.rate as f64 * settings.gate.clamp(1, 100) as f64 / 100.0).max(1.0) as u32;
    // notes released: their voices end (and a recorded note keeps only its first hit's length)
    let (gone, mut keep): (Vec<Voice>, Vec<Voice>) = voices.drain(..).partition(|v| !held.iter().any(|h| h.0 == v.pitch));
    silence(app, &gone);
    if app.recording() {
        for v in &gone {
            if let Some(n) = app.recorded.iter_mut().rev().find(|n| n.pitch == v.pitch && n.pos == v.first_pos) {
                n.len = n.len.min(tick_len);
            }
        }
    }
    // new notes: the press is the first hit
    for (pitch, vel, pos) in held {
        if !keep.iter().any(|v| v.pitch == pitch) {
            keep.push(Voice { pitch, velocity: vel, next: now + every, off_at: now + every * settings.gate.clamp(1, 100) as f64 / 100.0, sounding: true, first_pos: pos.max(0.0) as u32 });
        }
    }
    let pos = app.position().max(0.0) as u32;
    let recording = app.recording();
    for v in keep.iter_mut() {
        for (_, hit) in advance(v, &settings, every, now) {
            if let Ok(mut e) = app.engine.lock() {
                e.live(t, v.pitch, if hit { hit_velocity(&settings, v.velocity) } else { 0 });
            }
            if hit && recording {
                app.recorded.push(lg_core::events::Note { channel: 0, pitch: v.pitch, velocity: hit_velocity(&settings, v.velocity), pos, len: tick_len, muted: false });
            }
        }
    }
    let busy = !keep.is_empty();
    with(ctx, |s| {
        s.voices = keep;
        s.track = t;
    });
    if busy {
        ctx.request_repaint();
    }
}

/// The Note Repeat window.
pub fn window(app: &mut App, ctx: &egui::Context) {
    let (mut open, mut s) = with(ctx, |st| (st.open, st.settings.clone()));
    if !open {
        return;
    }
    egui::Window::new("Note Repeat").open(&mut open).collapsible(false).resizable(false).default_size(vec2(260.0, 0.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let arrow = if s.extended { "▾" } else { "▸" };
            if ui.add(egui::Button::new(arrow).frame(false)).on_hover_text("Extended settings").clicked() {
                s.extended = !s.extended;
            }
            ui.add_space(4.0);
            egui::Grid::new("nr-grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label("Rate");
                let name = RATES.iter().find(|r| r.1 == s.rate).map_or("1/16", |r| r.0);
                egui::ComboBox::from_id_salt("nr-rate").selected_text(name).show_ui(ui, |ui| {
                    for (n, v) in RATES {
                        ui.selectable_value(&mut s.rate, v, n);
                    }
                });
                ui.end_row();
                ui.label("Velocity");
                let mut as_played = s.velocity.is_none();
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut as_played, "As played").changed() {
                        s.velocity = if as_played { None } else { Some(100) };
                    }
                    if let Some(v) = s.velocity.as_mut() {
                        ui.add(egui::DragValue::new(v).range(1..=127));
                    }
                });
                ui.end_row();
                ui.label("Gate");
                ui.add(egui::DragValue::new(&mut s.gate).range(1..=100).suffix(" %"));
                ui.end_row();
            });
        });
        if s.extended {
            ui.separator();
            egui::Grid::new("nr-ext").num_columns(3).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label("");
                ui.label("Min");
                ui.label("Max");
                ui.end_row();
                ui.label("Rate");
                for v in [&mut s.rate_min, &mut s.rate_max] {
                    let name = RATES.iter().find(|r| r.1 == *v).map_or("1/16", |r| r.0);
                    egui::ComboBox::from_id_salt(("nr-rr", v as *const u32 as usize)).selected_text(name).width(70.0).show_ui(ui, |ui| {
                        for (n, x) in RATES {
                            ui.selectable_value(v, x, n);
                        }
                    });
                }
                ui.end_row();
                ui.label("Gate");
                ui.add(egui::DragValue::new(&mut s.gate_min).range(1..=100).suffix(" %"));
                ui.add(egui::DragValue::new(&mut s.gate_max).range(1..=100).suffix(" %"));
                ui.end_row();
                ui.label("Velocity offset");
                ui.add(egui::DragValue::new(&mut s.vel_min).range(-127..=0));
                ui.add(egui::DragValue::new(&mut s.vel_max).range(0..=127));
                ui.end_row();
            });
            // the limits keep the main values inside them
            s.gate = s.gate.clamp(s.gate_min.min(s.gate_max), s.gate_max.max(s.gate_min));
        }
    });
    let stop = with(ctx, |st| {
        st.settings = s;
        st.open = open;
        if open { Vec::new() } else { std::mem::take(&mut st.voices) }
    });
    silence(app, &stop);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_at_the_rate_with_the_gate() {
        let s = Settings::default();
        // 1/16 at 120 bpm: every 0.125 s; the gate is half of it
        let every = interval(&s, 120.0);
        assert!((every - 0.125).abs() < 1e-9);
        let mut v = Voice { pitch: 60, velocity: 90, next: every, off_at: every / 2.0, sounding: true, first_pos: 0 };
        let ev = advance(&mut v, &s, every, 0.5);
        let hits: Vec<f64> = ev.iter().filter(|e| e.1).map(|e| e.0).collect();
        assert_eq!(hits.len(), 4);
        assert!((hits[1] - 0.25).abs() < 1e-9);
        // each hit is followed by its off half a step later
        let offs: Vec<f64> = ev.iter().filter(|e| !e.1).map(|e| e.0).collect();
        assert!((offs[0] - 0.0625).abs() < 1e-9 && (offs[1] - 0.1875).abs() < 1e-9);
        // velocity: as played by default, else the fixed one
        assert_eq!(hit_velocity(&s, 77), 77);
        assert_eq!(hit_velocity(&Settings { velocity: Some(110), ..s.clone() }, 77), 110);
        // a 100% gate: the off comes with the next hit
        let full = Settings { gate: 100, ..s };
        let mut v = Voice { pitch: 60, velocity: 90, next: every, off_at: every, sounding: true, first_pos: 0 };
        let ev = advance(&mut v, &full, every, every * 2.0 + 0.01);
        assert_eq!(ev.iter().filter(|e| e.1).count(), 2);
        assert_eq!(ev.iter().filter(|e| !e.1).count(), 2);
    }
}
