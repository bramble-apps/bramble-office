//! The Step Input Keyboard (Window ▸ Show Step Input Keyboard, ⌥⌘K): notes entered one step at
//! a time into the selected MIDI region at the playhead (the Piano Roll and the Score show them
//! at once). Its window is Logic's: the chord button, note values 1/1–1/64 (1/128 by key), dot
//! and triplet, the rest, dynamics ppp–fff, Q (quantize note starts) and the MIDI channel, over
//! a keyboard C0–C7. While it is open its key commands (Logic's "Step Input Keyboard" group) take
//! the keys: A W S E D F T G Y H U J the notes, Space a rest, 1–8 the values, 9 / 0 triplet and
//! dot, Z X the octave, C V B N M , . / the velocity, ` chord mode, ← → step, ⌃⌫ delete, Q.
//! Notes played on a MIDI keyboard are entered too. Each entry is one undo step.

use egui::{pos2, vec2, Align2, Color32, Key, Modifiers, Rect, Sense, Stroke};
use lg_core::events::Note;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Show/Hide Step Input Keyboard", "Show Step Input Keyboard",
    "Note “C”", "Note “C#”", "Note “D”", "Note “D#”", "Note “E”", "Note “F”", "Note “F#”", "Note “G”", "Note “G#”", "Note “A”", "Note “A#”", "Note “B”",
    "Rest", "Next note will be sharp", "Next note will be flat", "Chord Mode", "Delete - Step Input", "Step Backwards", "Step Forward",
    "Octave 0", "Octave 1", "Octave 2", "Octave 3", "Octave 4", "Octave 5", "Octave 6", "Octave - 2", "Octave - 1", "Octave + 1", "Octave + 2",
    "1/1 Note", "1/2 Note", "1/4 Note", "1/8 Note", "1/16 Note", "1/32 Note", "1/64 Note", "1/128 Note",
    "Triplets On/Off", "Dotted note values On/Off",
    "Velocity 16 (ppp)", "Velocity 32 (pp)", "Velocity 48 (p)", "Velocity 64 (mp)", "Velocity 80 (mf)", "Velocity 96 (f)", "Velocity 112 (ff)", "Velocity 127 (fff)",
    "Quantize note starts On/Off",
];

/// Names that other menus use too (the Nudge Value's note lengths): run only from this
/// window's keys and buttons.
const SHARED: [&str; 8] = ["1/1 Note", "1/2 Note", "1/4 Note", "1/8 Note", "1/16 Note", "1/32 Note", "1/64 Note", "1/128 Note"];

const VELOCITIES: [(u8, &str); 8] = [(16, "ppp"), (32, "pp"), (48, "p"), (64, "mp"), (80, "mf"), (96, "f"), (112, "ff"), (127, "fff")];
const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

#[derive(Clone, Debug)]
pub struct State {
    pub open: bool,
    /// Note value index: 0 = 1/1 … 7 = 1/128.
    pub value: usize,
    pub triplet: bool,
    pub dotted: bool,
    /// Velocity index into VELOCITIES.
    pub velocity: usize,
    /// Octave 0–6 (Octave 3 starts at C3, MIDI 60).
    pub octave: i32,
    /// The next note's ±1 (Next note will be sharp / flat).
    pub accidental: i8,
    pub chord: bool,
    pub quantize: bool,
    pub channel: u8,
    /// Notes entered at the last step (Delete removes them).
    last: Vec<(usize, usize, u32, u8)>,
    held: Vec<u8>,
    rect: Option<Rect>,
}

impl Default for State {
    fn default() -> Self {
        State { open: false, value: 3, triplet: false, dotted: false, velocity: 4, octave: 3, accidental: 0, chord: false, quantize: true, channel: 0, last: Vec::new(), held: Vec::new(), rect: None }
    }
}

fn id() -> egui::Id {
    egui::Id::unique("lg-step-input")
}

pub fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(id())).unwrap_or_default()
}

fn with<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<State>(id())))
}

impl State {
    /// The step length in ticks.
    pub fn len(&self) -> u32 {
        let mut l = 3840 >> self.value.min(7);
        if self.dotted {
            l = l * 3 / 2;
        }
        if self.triplet {
            l = l * 2 / 3;
        }
        l.max(1)
    }
}

/// The pitch a note name gives (the octave, the pending sharp or flat, and in the Score with
/// Diatonic Insert on, the key's own alteration for that letter).
pub fn pitch_for(st: &State, pc: u8, diatonic_key: Option<&str>) -> u8 {
    let mut p = (st.octave + 2) * 12 + pc as i32 + st.accidental as i32;
    if let (Some(k), 0) = (diatonic_key, st.accidental) {
        if !NAMES[pc as usize].contains('#') {
            let key = lg_notation::Key::parse(k);
            let letter = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6][pc as usize];
            p += key.letter_alters()[letter] as i32;
        }
    }
    p.clamp(0, 127) as u8
}

/// Enters a note (or chord member) at the playhead into the selected region.
fn enter(app: &mut App, ctx: &egui::Context, pitch: u8, velocity: Option<u8>) -> bool {
    let Some((t, r)) = app.selected_region else {
        app.tell("Select a MIDI region for step input");
        return true;
    };
    if app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_none_or(|rg| rg.audio.is_some()) {
        return true;
    }
    let st = state(ctx);
    let len = st.len();
    let mut at = app.position().max(0.0) as u32;
    if st.quantize {
        at = (at + len / 2) / len * len;
    }
    // chord mode: the chord stays at the step just entered
    if st.chord {
        if let Some(&(_, _, pos, _)) = st.last.first() {
            at = pos;
        }
    }
    let vel = velocity.unwrap_or(VELOCITIES[st.velocity].0);
    app.checkpoint("Step Input");
    let mut idx = 0;
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        if at < rg.start {
            at = rg.start;
        }
        // past the region's end: the region grows to take it
        if at + len > rg.start + rg.length {
            rg.length = at + len - rg.start;
        }
        rg.notes.push(Note { channel: st.channel, pitch, velocity: vel, pos: at, len, muted: false });
        rg.notes.sort_by_key(|n| (n.pos, n.pitch));
        idx = rg.notes.iter().position(|n| n.pos == at && n.pitch == pitch).unwrap_or(0);
    }
    app.selected_notes = vec![idx];
    app.after_edit();
    crate::pianoroll::preview(app, t, pitch);
    with(ctx, |s| {
        if !s.chord || s.last.first().is_none_or(|l| l.2 != at) {
            s.last.clear();
        }
        s.last.push((t, r, at, pitch));
        s.accidental = 0;
    });
    if !st.chord {
        app.locate((at + len) as f64);
    }
    true
}

fn run(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let st = state(ctx);
    if let Some(rest) = name.strip_prefix("Note “") {
        let n = rest.trim_end_matches('”');
        let Some(pc) = NAMES.iter().position(|x| *x == n) else { return false };
        let diatonic = crate::score::state(ctx).diatonic && crate::score::editor_tab(app, ctx) == crate::score::EditorTab::Score;
        let key = app.project.as_ref().map(|p| p.key.clone());
        let pitch = pitch_for(&st, pc as u8, if diatonic { key.as_deref() } else { None });
        return enter(app, ctx, pitch, None);
    }
    if let Some(v) = SHARED.iter().position(|x| *x == name) {
        with(ctx, |s| s.value = v);
        return true;
    }
    if let Some(v) = VELOCITIES.iter().position(|(v, n)| name == format!("Velocity {v} ({n})")) {
        with(ctx, |s| s.velocity = v);
        return true;
    }
    match name {
        "Rest" => {
            let len = st.len();
            let at = app.position().max(0.0) as u32;
            with(ctx, |s| s.last.clear());
            app.locate((at + len) as f64);
        }
        "Next note will be sharp" => with(ctx, |s| s.accidental = 1),
        "Next note will be flat" => with(ctx, |s| s.accidental = -1),
        "Chord Mode" => {
            let chord = with(ctx, |s| {
                s.chord = !s.chord;
                s.chord
            });
            // leaving chord mode moves on past the chord
            if !chord {
                if let Some(&(_, _, pos, _)) = st.last.first() {
                    app.locate((pos + st.len()) as f64);
                }
            }
        }
        "Delete - Step Input" => {
            // the last step's notes go, and the playhead goes back to them
            let last = st.last.clone();
            if last.is_empty() {
                let len = st.len();
                app.locate((app.position() - len as f64).max(0.0));
                return true;
            }
            app.checkpoint("Delete - Step Input");
            for &(t, r, pos, pitch) in &last {
                if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    if let Some(i) = rg.notes.iter().position(|n| n.pos == pos && n.pitch == pitch) {
                        rg.notes.remove(i);
                    }
                }
            }
            app.selected_notes.clear();
            app.after_edit();
            app.locate(last[0].2 as f64);
            with(ctx, |s| s.last.clear());
        }
        "Step Backwards" | "Step Forward" => {
            let len = st.len() as f64;
            let p = app.position();
            app.locate(if name == "Step Forward" { p + len } else { (p - len).max(0.0) });
            with(ctx, |s| s.last.clear());
        }
        "Octave - 2" | "Octave - 1" | "Octave + 1" | "Octave + 2" => {
            let d: i32 = name[name.len() - 1..].parse().unwrap_or(1) * if name.contains('-') { -1 } else { 1 };
            with(ctx, |s| s.octave = (s.octave + d).clamp(-2, 8));
        }
        _ if name.starts_with("Octave ") => {
            let n: i32 = name[7..].parse().unwrap_or(3);
            with(ctx, |s| s.octave = n);
        }
        "Triplets On/Off" => with(ctx, |s| s.triplet = !s.triplet),
        "Dotted note values On/Off" => with(ctx, |s| s.dotted = !s.dotted),
        "Quantize note starts On/Off" => with(ctx, |s| s.quantize = !s.quantize),
        _ => return false,
    }
    true
}

/// Runs a Step Input command by name (menus and key commands); the window's own commands only
/// while it is open.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if matches!(name, "Show/Hide Step Input Keyboard" | "Show Step Input Keyboard") {
        with(ctx, |s| s.open = !s.open);
        return true;
    }
    if !COMMANDS.contains(&name) || SHARED.contains(&name) || !state(ctx).open {
        return false;
    }
    run(app, ctx, name)
}

/// A key press while the window is open: its bindings first.
pub fn key_press(app: &mut App, ctx: &egui::Context, key: Key, m: Modifiers) -> bool {
    if !state(ctx).open {
        return false;
    }
    let ctrl = m.ctrl || m.command;
    let found = crate::keymap::bindings().iter().find(|b| b.area == "Step Input Keyboard" && b.key == key && b.cmd == ctrl && b.alt == m.alt && b.shift == m.shift).map(|b| b.command.clone());
    match found {
        Some(c) => run(app, ctx, &c),
        None => false,
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let st = state(ctx);
    if !st.open {
        return;
    }
    // notes played on a MIDI keyboard (or Musical Typing) are entered as they start
    let held: Vec<(u8, u8)> = app.live.held.iter().map(|(&p, &(_, v))| (p, v)).collect();
    let fresh: Vec<(u8, u8)> = held.iter().copied().filter(|(p, _)| !st.held.contains(p)).collect();
    with(ctx, |s| s.held = held.iter().map(|x| x.0).collect());
    if !fresh.is_empty() {
        // several at once: a chord
        let chord = st.chord;
        if fresh.len() > 1 {
            with(ctx, |s| s.chord = true);
        }
        for (p, v) in &fresh {
            enter(app, ctx, *p, Some(*v));
        }
        if fresh.len() > 1 && !chord {
            with(ctx, |s| s.chord = false);
            app.locate(app.position() + st.len() as f64);
        }
    }
    let title = format!("{} - Step Input Keyboard", app.project.as_ref().map_or("Untitled".into(), |p| p.name.clone()));
    let mut open = true;
    let mut fired: Option<String> = None;
    let mut piano: Option<u8> = None;
    let resp = egui::Window::new(title).id(egui::Id::unique("lg-step-input-window")).open(&mut open).resizable(false).collapsible(false).default_size(vec2(800.0, 220.0)).show(ctx, |ui| {
        let st = state(ui.ctx());
        ui.horizontal(|ui| {
            let on = |ui: &mut egui::Ui, sel: bool, text: egui::RichText, tip: &str| ui.add(egui::Button::new(text).selected(sel).min_size(vec2(30.0, 30.0))).on_hover_text(tip).clicked();
            if on(ui, st.chord, egui::RichText::new("\u{E1D5}\u{E1D5}").font(look::music(18.0)), "Chord Mode (`)") {
                fired = Some("Chord Mode".into());
            }
            ui.add_space(8.0);
            let glyphs = ["\u{E1D2}", "\u{E1D3}", "\u{E1D5}", "\u{E1D7}", "\u{E1D9}", "\u{E1DB}", "\u{E1DD}"];
            for (k, g) in glyphs.iter().enumerate() {
                if on(ui, st.value == k, egui::RichText::new(*g).font(look::music(20.0)), SHARED[k]) {
                    with(ui.ctx(), |s| s.value = k);
                }
            }
            ui.add_space(8.0);
            if on(ui, st.dotted, egui::RichText::new("•").size(16.0), "Dotted note values On/Off (0)") {
                fired = Some("Dotted note values On/Off".into());
            }
            if on(ui, st.triplet, egui::RichText::new("3").italics().size(15.0), "Triplets On/Off (9)") {
                fired = Some("Triplets On/Off".into());
            }
            ui.add_space(8.0);
            if on(ui, false, egui::RichText::new("▬→").size(14.0), "Rest (Space)") {
                fired = Some("Rest".into());
            }
            ui.add_space(8.0);
            for (k, (_, n)) in VELOCITIES.iter().enumerate() {
                if on(ui, st.velocity == k, egui::RichText::new(*n).italics().strong().size(15.0), &format!("Velocity {}", VELOCITIES[k].0)) {
                    with(ui.ctx(), |s| s.velocity = k);
                }
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                if on(ui, st.quantize, egui::RichText::new("Q").strong(), "Quantize note starts On/Off (Q)") {
                    fired = Some("Quantize note starts On/Off".into());
                }
                let mut ch = st.channel as f32 + 1.0;
                ui.horizontal(|ui| {
                    ui.label("Ch:");
                    if ui.add(egui::DragValue::new(&mut ch).range(1.0..=16.0)).changed() {
                        with(ui.ctx(), |s| s.channel = ch as u8 - 1);
                    }
                });
            });
        });
        ui.add_space(6.0);
        // the keyboard: C0 to C7, the current octave marked
        let w = ui.available_width().max(600.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 90.0), Sense::click());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 6.0, look::group_bg());
        let keys_r = rect.shrink2(vec2(10.0, 8.0));
        let whites: Vec<u8> = (24u8..=108).filter(|k| ![1, 3, 6, 8, 10].contains(&(k % 12))).collect();
        let ww = keys_r.width() / whites.len() as f32;
        let oct_lo = ((st.octave + 2) * 12) as u8;
        for (i, &k) in whites.iter().enumerate() {
            let r = Rect::from_min_size(pos2(keys_r.min.x + i as f32 * ww, keys_r.min.y), vec2(ww - 1.0, keys_r.height() - 16.0));
            let mine = k >= oct_lo && k < oct_lo + 12;
            p.rect_filled(r, 2.0, if mine { Color32::from_rgb(0xdd, 0xe8, 0xff) } else { Color32::WHITE });
            if k % 12 == 0 {
                p.text(pos2(r.center().x, keys_r.max.y - 6.0), Align2::CENTER_CENTER, format!("C{}", k / 12 - 2), look::regular(11.0), look::text());
            }
        }
        let mut blacks = Vec::new();
        for (i, &k) in whites.iter().enumerate() {
            if [0, 2, 5, 7, 9].contains(&(k % 12)) && k < 108 {
                let r = Rect::from_min_size(pos2(keys_r.min.x + (i as f32 + 0.65) * ww, keys_r.min.y), vec2(ww * 0.7, (keys_r.height() - 16.0) * 0.6));
                p.rect_filled(r, 2.0, Color32::from_gray(0x18));
                blacks.push((k + 1, r));
            }
        }
        p.rect_stroke(keys_r, 2.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
        if resp.clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                piano = blacks.iter().find(|(_, r)| r.contains(pp)).map(|x| x.0).or_else(|| {
                    let i = ((pp.x - keys_r.min.x) / ww).floor().max(0.0) as usize;
                    whites.get(i).copied()
                });
            }
        }
    });
    if let Some(r) = resp.as_ref().map(|r| r.response.rect) {
        with(ctx, |s| s.rect = Some(r));
    }
    if let Some(c) = fired {
        run(app, ctx, &c);
    }
    if let Some(k) = piano {
        enter(app, ctx, k, None);
    }
    if !open {
        with(ctx, |s| s.open = false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_and_pitches() {
        let mut st = State::default();
        // an eighth by default; dotted and triplet
        assert_eq!(st.len(), 480);
        st.dotted = true;
        assert_eq!(st.len(), 720);
        st.dotted = false;
        st.triplet = true;
        assert_eq!(st.len(), 320);
        st.value = 7;
        st.triplet = false;
        assert_eq!(st.len(), 30);
        // octave 3: “C” is middle C (C3 = 60); a pending sharp
        let mut st = State::default();
        assert_eq!(pitch_for(&st, 0, None), 60);
        st.accidental = 1;
        assert_eq!(pitch_for(&st, 4, None), 65);
        // Diatonic Insert in D major: “F” is F♯, “C” is C♯
        st.accidental = 0;
        assert_eq!(pitch_for(&st, 5, Some("D major")), 66);
        assert_eq!(pitch_for(&st, 0, Some("D major")), 61);
        st.octave = 0;
        assert_eq!(pitch_for(&st, 0, None), 24);
    }

    #[test]
    fn every_logic_step_input_command_is_listed() {
        let data = include_str!("../data/commands.txt");
        for l in data.lines().filter_map(|l| l.strip_prefix("Step Input Keyboard\t")) {
            let c = l.split('\t').next().unwrap_or("");
            assert!(COMMANDS.contains(&c), "{c}");
        }
    }
}
