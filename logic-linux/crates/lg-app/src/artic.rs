//! Articulation sets, as Logic's: the Articulation Set Editor (Articulations, Switches and
//! Output panes; plus, minus and Revert at the bottom left), the Track inspector's Articulation
//! Set pop-up (Edit, Load, Save, Clear), articulation IDs on notes (the Piano Roll's
//! Articulation pop-up, Set Articulation 1-32 / Next / Previous / None, Select Same
//! Articulation, Set Note Color By Articulation, Separate MIDI Region by Articulation), the
//! switches that change the articulation from MIDI input (Permanent, Momentary and Toggle with
//! their Retrigger and Trigger forms; MIDI Remote, one MIDI channel, an octave offset, local to
//! the set or global), the outputs that play an articulation on instruments without articulation
//! IDs (up to three messages), and the Smart Controls' Keyswitches view.
//!
//! Sets are saved as Studio's own text files in ~/.config/bramble-studio/articulation-sets.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::Note;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Open Articulation Editor", "Select Same Articulation", "Same Articulation", "Set Next Articulation", "Set Previous Articulation", "Set No Articulation",
    "Set Articulation  1", "Set Articulation  2", "Set Articulation  3", "Set Articulation  4", "Set Articulation  5", "Set Articulation  6", "Set Articulation  7", "Set Articulation  8",
    "Set Articulation  9", "Set Articulation 10", "Set Articulation 11", "Set Articulation 12", "Set Articulation 13", "Set Articulation 14", "Set Articulation 15", "Set Articulation 16",
    "Set Articulation 17", "Set Articulation 18", "Set Articulation 19", "Set Articulation 20", "Set Articulation 21", "Set Articulation 22", "Set Articulation 23", "Set Articulation 24",
    "Set Articulation 25", "Set Articulation 26", "Set Articulation 27", "Set Articulation 28", "Set Articulation 29", "Set Articulation 30", "Set Articulation 31", "Set Articulation 32",
    "Show/Hide Keyswitches", "Show/Hide Keyswitches Window", "Toggle MIDI Remote for Articulation Switches", "Set Note Color By Articulation", "Separate MIDI Region by Articulation", "Separate by Articulation",
];

// ---------------------------------------------------------------- the model

/// A MIDI message type (switches and outputs; Velocity for switches only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    NoteOn,
    NoteOff,
    PolyAftertouch,
    Controller,
    Program,
    Aftertouch,
    PitchBend,
    Velocity,
}

impl Msg {
    pub const SWITCH: [Msg; 8] = [Msg::NoteOn, Msg::NoteOff, Msg::PolyAftertouch, Msg::Controller, Msg::Program, Msg::Aftertouch, Msg::PitchBend, Msg::Velocity];
    pub const OUTPUT: [Msg; 7] = [Msg::NoteOn, Msg::NoteOff, Msg::PolyAftertouch, Msg::Controller, Msg::Program, Msg::Aftertouch, Msg::PitchBend];

    pub fn name(self) -> &'static str {
        match self {
            Msg::NoteOn => "Note On",
            Msg::NoteOff => "Note Off",
            Msg::PolyAftertouch => "Poly Aftertouch",
            Msg::Controller => "Controller",
            Msg::Program => "Program",
            Msg::Aftertouch => "Aftertouch",
            Msg::PitchBend => "Pitch Bend",
            Msg::Velocity => "Velocity",
        }
    }

    /// Program, Aftertouch and Pitch Bend have no selector (note or controller number).
    pub fn has_selector(self) -> bool {
        !matches!(self, Msg::Program | Msg::Aftertouch | Msg::PitchBend)
    }

    fn from_name(s: &str) -> Option<Msg> {
        Msg::SWITCH.into_iter().find(|m| m.name() == s)
    }
}

/// How a switch changes the articulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Permanent,
    PermanentRetrigger,
    PermanentTrigger,
    Momentary,
    MomentaryRetrigger,
    MomentaryTrigger,
    Toggle,
    ToggleRetrigger,
    ToggleTrigger,
}

impl Mode {
    pub const ALL: [Mode; 9] = [Mode::Permanent, Mode::PermanentRetrigger, Mode::PermanentTrigger, Mode::Momentary, Mode::MomentaryRetrigger, Mode::MomentaryTrigger, Mode::Toggle, Mode::ToggleRetrigger, Mode::ToggleTrigger];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Permanent => "Permanent",
            Mode::PermanentRetrigger => "Permanent (Retrigger)",
            Mode::PermanentTrigger => "Permanent (Trigger)",
            Mode::Momentary => "Momentary",
            Mode::MomentaryRetrigger => "Momentary (Retrigger)",
            Mode::MomentaryTrigger => "Momentary (Trigger)",
            Mode::Toggle => "Toggle",
            Mode::ToggleRetrigger => "Toggle (Retrigger)",
            Mode::ToggleTrigger => "Toggle (Trigger)",
        }
    }

    fn base(self) -> u8 {
        match self {
            Mode::Permanent | Mode::PermanentRetrigger | Mode::PermanentTrigger => 0,
            Mode::Momentary | Mode::MomentaryRetrigger | Mode::MomentaryTrigger => 1,
            _ => 2,
        }
    }

    fn retrigger(self) -> bool {
        matches!(self, Mode::PermanentRetrigger | Mode::MomentaryRetrigger | Mode::ToggleRetrigger)
    }

    fn trigger(self) -> bool {
        matches!(self, Mode::PermanentTrigger | Mode::MomentaryTrigger | Mode::ToggleTrigger)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Articulation {
    pub id: u8,
    pub name: String,
    /// None: all channels ("-"); else the channel (0-15) that triggers it instead of the ID
    pub channel: Option<u8>,
    /// The Score Editor symbol (index into [`SYMBOLS`])
    pub symbol: u8,
}

pub const SYMBOLS: [&str; 9] = ["None", "Staccato", "Staccatissimo", "Tenuto", "Accent", "Marcato", "Fermata", "Tremolo", "Trill"];

#[derive(Clone, Debug, PartialEq)]
pub struct Switch {
    pub kind: Msg,
    pub selector: u8,
    pub start: u8,
    pub end: u8,
    pub mode: Mode,
    /// the articulation ID it switches to
    pub artic: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutMsg {
    pub kind: Msg,
    pub channel: Option<u8>,
    pub selector: u8,
    pub start: u8,
    pub end: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub artic: u8,
    /// one, or up to three with Activate Multiple Outputs
    pub msgs: Vec<OutMsg>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    pub name: String,
    pub artics: Vec<Articulation>,
    pub switches: Vec<Switch>,
    pub outputs: Vec<Output>,
    /// MIDI Remote, MIDI Channel (None: all), Octave Offset (-10..10), and whether these are
    /// local to the set (else the global ones apply)
    pub remote: bool,
    pub remote_channel: Option<u8>,
    pub octave: i8,
    pub local: bool,
    pub multiple_outputs: bool,
}

impl Default for Set {
    fn default() -> Self {
        Set { name: "Untitled".into(), artics: Vec::new(), switches: Vec::new(), outputs: Vec::new(), remote: true, remote_channel: None, octave: 0, local: false, multiple_outputs: false }
    }
}

impl Set {
    pub fn artic(&self, id: u8) -> Option<&Articulation> {
        self.artics.iter().find(|a| a.id == id)
    }

    /// The next free articulation ID.
    pub fn next_id(&self) -> u8 {
        (1..=255).find(|i| self.artic(*i).is_none()).unwrap_or(255)
    }

    /// The messages that play an articulation (its Output row), MIDI channel resolved.
    pub fn outputs_for(&self, id: u8) -> Vec<OutMsg> {
        let mut v: Vec<OutMsg> = self.outputs.iter().filter(|o| o.artic == id).flat_map(|o| o.msgs.iter().copied()).collect();
        if !self.multiple_outputs {
            v.truncate(1);
        }
        v.truncate(3);
        v
    }

    /// The text form saved to a file.
    pub fn to_text(&self) -> String {
        let ch = |c: Option<u8>| c.map_or("-".to_string(), |c| (c + 1).to_string());
        let mut s = format!("set\t{}\nremote\t{}\t{}\t{}\t{}\t{}\n", self.name, self.remote as u8, ch(self.remote_channel), self.octave, self.local as u8, self.multiple_outputs as u8);
        for a in &self.artics {
            s += &format!("artic\t{}\t{}\t{}\t{}\n", a.id, a.name, ch(a.channel), a.symbol);
        }
        for w in &self.switches {
            s += &format!("switch\t{}\t{}\t{}\t{}\t{}\t{}\n", w.kind.name(), w.selector, w.start, w.end, w.mode.name(), w.artic);
        }
        for o in &self.outputs {
            for m in &o.msgs {
                s += &format!("output\t{}\t{}\t{}\t{}\t{}\t{}\n", o.artic, m.kind.name(), ch(m.channel), m.selector, m.start, m.end);
            }
        }
        s
    }

    pub fn from_text(t: &str) -> Option<Set> {
        let ch = |s: &str| s.parse::<u8>().ok().map(|c| c.saturating_sub(1).min(15));
        let mut set = Set::default();
        let mut any = false;
        for l in t.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
            match f.first().copied() {
                Some("set") => {
                    set.name = f.get(1).unwrap_or(&"").to_string();
                    any = true;
                }
                Some("remote") => {
                    set.remote = n(1) != 0;
                    set.remote_channel = ch(f.get(2).unwrap_or(&"-"));
                    set.octave = n(3).clamp(-10, 10) as i8;
                    set.local = n(4) != 0;
                    set.multiple_outputs = n(5) != 0;
                }
                Some("artic") => set.artics.push(Articulation { id: n(1).clamp(1, 255) as u8, name: f.get(2).unwrap_or(&"").to_string(), channel: ch(f.get(3).unwrap_or(&"-")), symbol: n(4) as u8 }),
                Some("switch") => set.switches.push(Switch { kind: Msg::from_name(f.get(1).unwrap_or(&"")).unwrap_or(Msg::NoteOn), selector: n(2) as u8, start: n(3) as u8, end: n(4) as u8, mode: Mode::ALL.into_iter().find(|m| m.name() == *f.get(5).unwrap_or(&"")).unwrap_or(Mode::Permanent), artic: n(6) as u8 }),
                Some("output") => {
                    let m = OutMsg { kind: Msg::from_name(f.get(2).unwrap_or(&"")).unwrap_or(Msg::NoteOn), channel: ch(f.get(3).unwrap_or(&"-")), selector: n(4) as u8, start: n(5) as u8, end: n(6) as u8 };
                    let id = n(1) as u8;
                    match set.outputs.iter_mut().find(|o| o.artic == id) {
                        Some(o) => o.msgs.push(m),
                        None => set.outputs.push(Output { artic: id, msgs: vec![m] }),
                    }
                }
                _ => {}
            }
        }
        any.then_some(set)
    }
}

/// What a switch did to the articulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub artic: Option<u8>,
    /// stop and retrigger the sounding note with the new articulation
    pub retrigger: bool,
    /// apply it to the sounding note (stop notes when released)
    pub trigger: bool,
}

/// The articulation a track plays, as MIDI input switches it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Switcher {
    pub current: Option<u8>,
    /// before a Momentary switch / a Toggle
    before: Option<u8>,
    held: Option<usize>,
}

impl Switcher {
    /// A MIDI message in (kind, channel 0-15, selector / note, value / velocity); the octave
    /// offset moves keyswitches. Returns the change, if any.
    pub fn input(&mut self, set: &Set, global: (bool, Option<u8>, i8), kind: Msg, channel: u8, selector: u8, value: u8) -> Option<Change> {
        let (remote, rch, oct) = if set.local { (set.remote, set.remote_channel, set.octave) } else { global };
        if !remote || rch.is_some_and(|c| c != channel) {
            return None;
        }
        // a key's note number, the octave offset taken off
        let sel = if matches!(kind, Msg::NoteOn | Msg::NoteOff | Msg::PolyAftertouch | Msg::Velocity) { (selector as i32 - oct as i32 * 12).clamp(0, 127) as u8 } else { selector };
        // a note off releases a momentary switch held by its note on
        if kind == Msg::NoteOff {
            if let Some(i) = self.held {
                let w = &set.switches[i];
                if w.kind == Msg::NoteOn && w.selector == sel && w.mode.base() == 1 {
                    self.held = None;
                    self.current = self.before;
                    return Some(Change { artic: self.current, retrigger: w.mode.retrigger(), trigger: w.mode.trigger() });
                }
            }
        }
        for (i, w) in set.switches.iter().enumerate() {
            let hit_kind = w.kind == kind || (w.kind == Msg::Velocity && kind == Msg::NoteOn);
            let sel_ok = !w.kind.has_selector() || w.kind == Msg::Velocity || w.selector == sel;
            if !hit_kind || !sel_ok {
                continue;
            }
            let (lo, hi) = (w.start.min(w.end), w.start.max(w.end));
            let inside = (lo..=hi).contains(&value);
            match w.mode.base() {
                0 if inside => {
                    self.current = Some(w.artic);
                }
                1 => {
                    if inside && self.held.is_none() {
                        self.before = self.current;
                        self.current = Some(w.artic);
                        self.held = Some(i);
                    } else if !inside && self.held == Some(i) {
                        self.held = None;
                        self.current = self.before;
                    } else {
                        continue;
                    }
                }
                2 if inside => {
                    if self.current == Some(w.artic) {
                        self.current = self.before;
                    } else {
                        self.before = self.current;
                        self.current = Some(w.artic);
                    }
                }
                _ => continue,
            }
            return Some(Change { artic: self.current, retrigger: w.mode.retrigger(), trigger: w.mode.trigger() });
        }
        None
    }
}

// ---------------------------------------------------------------- state

#[derive(Default)]
pub struct State {
    /// The sets saved in the config folder (loaded once).
    pub library: Vec<Set>,
    loaded: bool,
    /// Each track's set.
    pub tracks: HashMap<usize, Set>,
    /// The Articulation Set Editor: the track, its working copy and the saved one, the pane.
    pub editor: Option<(usize, Set, Set, usize)>,
    pub editor_sel: Option<usize>,
    /// Smart Controls Keyswitches view, and as a floating window.
    pub keys_view: bool,
    pub keys_window: bool,
    /// Global MIDI Remote / channel / octave offset (sets not local use these).
    pub global: (bool, Option<u8>, i8),
    pub global_set: bool,
    /// The live articulation of each track (keyswitches, the plug-in header).
    pub live: HashMap<usize, Switcher>,
    /// Note colours by articulation (Set Note Color By Articulation).
    pub color_by: bool,
}

impl State {
    pub fn global(&self) -> (bool, Option<u8>, i8) {
        if self.global_set { self.global } else { (true, None, 0) }
    }
}

fn dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("articulation-sets"))
}

fn load(app: &mut App) {
    if app.artic.loaded {
        return;
    }
    app.artic.loaded = true;
    let Some(d) = dir() else { return };
    let Ok(rd) = std::fs::read_dir(d) else { return };
    let mut v: Vec<Set> = rd.flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).filter_map(|t| Set::from_text(&t)).collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    app.artic.library = v;
}

fn save_set(set: &Set) {
    let Some(d) = dir() else { return };
    let _ = std::fs::create_dir_all(&d);
    let name: String = set.name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let _ = std::fs::write(d.join(format!("{name}.txt")), set.to_text());
}

/// A note's articulation ID in a region.
pub fn note_artic(r: &lg_core::Region, n: &Note) -> Option<u8> {
    let rel = n.pos.saturating_sub(r.start);
    r.ext.artics.iter().find(|a| a.0 == rel && a.1 == n.pitch).map(|a| a.2)
}

/// Sets (or clears) a note's articulation ID.
pub fn set_note_artic(r: &mut lg_core::Region, k: usize, id: Option<u8>) {
    let Some(n) = r.notes.get(k).copied() else { return };
    let rel = n.pos.saturating_sub(r.start);
    r.ext.artics.retain(|a| !(a.0 == rel && a.1 == n.pitch));
    if let Some(id) = id {
        r.ext.artics.push((rel, n.pitch, id));
    }
}

/// A colour per articulation ID.
pub fn color(id: Option<u8>) -> Color32 {
    const C: [(u8, u8, u8); 8] = [(0xe8, 0x4c, 0x3c), (0xf0, 0x9c, 0x30), (0x9c, 0xd0, 0x40), (0x40, 0xc8, 0xa8), (0x3b, 0x8b, 0xf0), (0x9c, 0x5c, 0xe0), (0xd0, 0x5c, 0xc8), (0xb0, 0x8c, 0x6c)];
    match id {
        None => Color32::from_gray(0x9a),
        Some(i) => {
            let (r, g, b) = C[(i as usize + 7) % 8];
            Color32::from_rgb(r, g, b)
        }
    }
}

// ---------------------------------------------------------------- commands

/// Runs an articulation command; false when it isn't one.
pub fn command(app: &mut App, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    load(app);
    match name {
        "Open Articulation Editor" => {
            if let Some(t) = app.selected_track {
                let set = app.artic.tracks.get(&t).cloned().unwrap_or_default();
                app.artic.editor = Some((t, set.clone(), set, 0));
            }
        }
        "Show/Hide Keyswitches" => app.artic.keys_view = !app.artic.keys_view,
        "Show/Hide Keyswitches Window" => app.artic.keys_window = !app.artic.keys_window,
        "Toggle MIDI Remote for Articulation Switches" => {
            let g = app.artic.global();
            app.artic.global = (!g.0, g.1, g.2);
            app.artic.global_set = true;
        }
        "Set Note Color By Articulation" => app.artic.color_by = !app.artic.color_by,
        "Select Same Articulation" | "Same Articulation" => {
            let Some((t, r)) = app.selected_region else { return true };
            let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { return true };
            let ids: Vec<Option<u8>> = app.selected_notes.iter().filter_map(|k| rg.notes.get(*k)).map(|n| note_artic(rg, n)).collect();
            if !ids.is_empty() {
                app.selected_notes = rg.notes.iter().enumerate().filter(|(_, n)| ids.contains(&note_artic(rg, n))).map(|(k, _)| k).collect();
            }
        }
        "Separate MIDI Region by Articulation" | "Separate by Articulation" => separate(app),
        _ => {
            // Set Articulation n / Next / Previous / None on the selected notes
            let Some((t, r)) = app.selected_region else { return true };
            let ids: Vec<u8> = app.artic.tracks.get(&t).map(|s| s.artics.iter().map(|a| a.id).collect()).unwrap_or_default();
            app.checkpoint("Set Articulation");
            let sel = app.selected_notes.clone();
            let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) else { return true };
            for k in sel {
                let Some(n) = rg.notes.get(k).copied() else { continue };
                let cur = note_artic(rg, &n);
                let new = match name {
                    "Set No Articulation" => None,
                    "Set Next Articulation" | "Set Previous Articulation" => {
                        if ids.is_empty() {
                            cur
                        } else {
                            let i = cur.and_then(|c| ids.iter().position(|x| *x == c));
                            let j = match (name, i) {
                                ("Set Next Articulation", Some(i)) => (i + 1) % ids.len(),
                                ("Set Next Articulation", None) => 0,
                                (_, Some(i)) => (i + ids.len() - 1) % ids.len(),
                                (_, None) => ids.len() - 1,
                            };
                            Some(ids[j])
                        }
                    }
                    _ => name.trim_start_matches("Set Articulation").trim().parse::<u8>().ok(),
                };
                set_note_artic(rg, k, new);
            }
            app.dirty = true;
        }
    }
    true
}

/// Separate MIDI Region by Articulation: a region per articulation ID, on the same track.
fn separate(app: &mut App) {
    let Some((t, r)) = app.selected_region else { return };
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
    let mut ids: Vec<Option<u8>> = rg.notes.iter().map(|n| note_artic(&rg, n)).collect();
    ids.sort();
    ids.dedup();
    if ids.len() < 2 {
        return;
    }
    app.checkpoint("Separate MIDI Region by Articulation");
    let names = app.artic.tracks.get(&t).cloned().unwrap_or_default();
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    tr.regions.remove(r);
    for id in ids {
        let mut n = rg.clone();
        n.notes = rg.notes.iter().filter(|x| note_artic(&rg, x) == id).copied().collect();
        n.ext.artics.retain(|a| Some(a.2) == id);
        n.name = format!("{} {}", rg.name, id.and_then(|i| names.artic(i)).map_or("(no articulation)".to_string(), |a| a.name.clone()));
        n.source = None;
        tr.regions.push(n);
    }
    app.selected_notes.clear();
    app.after_edit();
}

/// Playback: each note with an articulation sends its Output's Note On keyswitch just before
/// it (instruments without articulation IDs); other output types are not sent yet.
pub fn for_playback(app: &App, i: usize, notes: &mut Vec<(u32, u32, u8, u8)>) {
    let Some(set) = app.artic.tracks.get(&i) else { return };
    if set.outputs.is_empty() {
        return;
    }
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(i)) else { return };
    let mut add = Vec::new();
    for rg in tr.regions.iter().filter(|r| !r.muted) {
        for n in &rg.notes {
            let Some(id) = note_artic(rg, n) else { continue };
            for m in set.outputs_for(id).into_iter().filter(|m| m.kind == Msg::NoteOn) {
                let at = n.pos.saturating_sub(4);
                add.push((at, at + 2, m.selector, m.start.max(1)));
            }
        }
    }
    notes.extend(add);
    notes.sort_by_key(|n| n.0);
}

/// MIDI input on the selected track: its set's switches change the live articulation; the
/// switch's own message is taken (true), others play on.
pub fn midi_in(app: &mut App, t: usize, status: u8, a: u8, b: u8) -> bool {
    let Some(set) = app.artic.tracks.get(&t).cloned() else { return false };
    if set.switches.is_empty() {
        return false;
    }
    let ch = status & 0x0F;
    let (kind, sel, val) = match status & 0xF0 {
        0x90 if b > 0 => (Msg::NoteOn, a, b),
        0x90 | 0x80 => (Msg::NoteOff, a, b),
        0xA0 => (Msg::PolyAftertouch, a, b),
        0xB0 => (Msg::Controller, a, b),
        0xC0 => (Msg::Program, 0, a),
        0xD0 => (Msg::Aftertouch, 0, a),
        0xE0 => (Msg::PitchBend, 0, b),
        _ => return false,
    };
    let global = app.artic.global();
    let mut sw = app.artic.live.get(&t).cloned().unwrap_or_default();
    let changed = sw.input(&set, global, kind, ch, sel, val);
    app.artic.live.insert(t, sw);
    // a keyswitch note (the octave offset applied) never sounds
    let oct = if set.local { set.octave } else { global.2 };
    let key = (a as i32 - oct as i32 * 12).clamp(0, 127) as u8;
    let is_key = matches!(kind, Msg::NoteOn | Msg::NoteOff) && set.switches.iter().any(|w| w.kind == Msg::NoteOn && w.selector == key);
    changed.is_some() && !matches!(kind, Msg::NoteOn | Msg::NoteOff) || is_key
}

// ---------------------------------------------------------------- UI

/// The Track inspector's Articulation Set row (software instrument tracks); returns the next y.
pub fn inspector_row(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, area: Rect, y: f32, t: usize) -> f32 {
    load(app);
    p.text(pos2(area.min.x + 10.0, y + 9.0), Align2::LEFT_CENTER, "Articulation Set", look::regular(11.0), look::text_dim());
    let name = app.artic.tracks.get(&t).map_or("None".to_string(), |s| s.name.clone());
    let r = Rect::from_min_size(pos2(area.min.x + area.width() * 0.48, y), vec2(area.width() * 0.5 - 8.0, 18.0));
    let resp = ui.interact(r, ui.scope_id().with(("artic-set", t)), Sense::click());
    p.text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, format!("{name} ⌄"), look::regular(11.0), look::text());
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(200.0);
        if ui.button("Edit…").clicked() {
            let set = app.artic.tracks.get(&t).cloned().unwrap_or_default();
            app.artic.editor = Some((t, set.clone(), set, 0));
            ui.close();
        }
        ui.separator();
        let lib = app.artic.library.clone();
        if lib.is_empty() {
            ui.add_enabled(false, egui::Button::new("No saved sets"));
        }
        for s in lib {
            if ui.button(&s.name).clicked() {
                app.artic.tracks.insert(t, s);
                app.load_engine();
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(app.artic.tracks.contains_key(&t), egui::Button::new("Save")).clicked() {
            if let Some(s) = app.artic.tracks.get(&t).cloned() {
                save_set(&s);
                app.artic.library.retain(|x| x.name != s.name);
                app.artic.library.push(s);
            }
            ui.close();
        }
        if ui.add_enabled(app.artic.tracks.contains_key(&t), egui::Button::new("Clear")).clicked() {
            app.artic.tracks.remove(&t);
            app.load_engine();
            ui.close();
        }
    });
    y + 20.0
}

/// The Piano Roll's Articulation pop-up (when the track has a set): the selected notes'
/// articulation; choosing one sets them.
pub fn roll_popup(app: &mut App, ui: &mut egui::Ui) {
    let Some((t, r)) = app.selected_region else { return };
    let Some(set) = app.artic.tracks.get(&t).cloned() else { return };
    let cur = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).and_then(|rg| app.selected_notes.first().and_then(|k| rg.notes.get(*k)).map(|n| note_artic(rg, n)));
    let label = match cur {
        Some(Some(id)) => set.artic(id).map_or(format!("ID {id}"), |a| a.name.clone()),
        Some(None) => "No Articulation".into(),
        None => "Articulation".into(),
    };
    ui.menu_button(egui::RichText::new(format!("{label} ⌄")).size(11.5), |ui| {
        if ui.button("No Articulation").clicked() {
            command(app, "Set No Articulation");
            ui.close();
        }
        for a in &set.artics {
            if ui.button(format!("{} {}", a.id, a.name)).clicked() {
                command(app, &format!("Set Articulation {:>2}", a.id));
                set_selected(app, Some(a.id));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Select Same Articulation").clicked() {
            command(app, "Select Same Articulation");
            ui.close();
        }
        if ui.add(egui::Button::selectable(app.artic.color_by, "Color Notes by Articulation")).clicked() {
            app.artic.color_by = !app.artic.color_by;
            ui.close();
        }
    });
}

fn set_selected(app: &mut App, id: Option<u8>) {
    let Some((t, r)) = app.selected_region else { return };
    let sel = app.selected_notes.clone();
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        for k in sel {
            set_note_artic(rg, k, id);
        }
    }
}

/// The Keyswitches view (Smart Controls, or its own window): MIDI Remote, Octave Offset, the
/// mini keyboard of octaves, and the keys; a key with an articulation switches to it.
pub fn keyswitch_view(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize) {
    let Some(set) = app.artic.tracks.get(&t).cloned() else {
        ui.painter().text(area.center(), Align2::CENTER_CENTER, "No Articulation Set on this track", look::regular(13.0), look::text_dim());
        return;
    };
    let p = ui.painter_at(area);
    let mut g = if set.local { (set.remote, set.remote_channel, set.octave) } else { app.artic.global() };
    // MIDI Remote (top left), the Octave Offset pop-up
    let rr = Rect::from_min_size(area.min + vec2(10.0, 8.0), vec2(96.0, 22.0));
    let rresp = ui.interact(rr, ui.scope_id().with("ks-remote"), Sense::click());
    p.rect_filled(rr, 5.0, if g.0 { look::accent() } else { look::group_bg() });
    p.text(rr.center(), Align2::CENTER_CENTER, "MIDI Remote", look::regular(12.0), Color32::WHITE);
    if rresp.clicked() {
        g.0 = !g.0;
    }
    let or = Rect::from_min_size(pos2(rr.max.x + 12.0, rr.min.y), vec2(140.0, 22.0));
    let oresp = ui.interact(or, ui.scope_id().with("ks-oct"), Sense::click());
    p.text(pos2(or.min.x, or.center().y), Align2::LEFT_CENTER, format!("Octave Offset: {:+} ⌄", g.2), look::regular(12.0), look::text());
    egui::Popup::menu(&oresp).show(|ui| {
        for o in -10..=10i8 {
            if ui.add(egui::Button::selectable(g.2 == o, format!("{o:+}"))).clicked() {
                g.2 = o;
                ui.close();
            }
        }
    });
    // the octaves (mini keyboards): one per octave, the offset's lit
    let mini_y = rr.max.y + 10.0;
    let mw = ((area.width() - 20.0) / 11.0).min(60.0);
    for (k, o) in (-5..=5i8).enumerate() {
        let mr = Rect::from_min_size(pos2(area.min.x + 10.0 + k as f32 * mw, mini_y), vec2(mw - 4.0, 14.0));
        let resp = ui.interact(mr, ui.scope_id().with(("ks-mini", k)), Sense::click()).on_hover_text(format!("Octave {o:+}"));
        p.rect_filled(mr, 3.0, if g.2 == o { look::accent() } else { Color32::from_gray(0xe0) });
        for j in [1.0, 2.0, 4.0, 5.0, 6.0] {
            p.rect_filled(Rect::from_min_size(pos2(mr.min.x + mr.width() * j / 7.0 - 1.5, mr.min.y), vec2(3.0, 8.0)), 0.0, Color32::from_gray(0x30));
        }
        if resp.clicked() {
            g.2 = o;
        }
    }
    // the keys: two octaves from C1 shifted by the offset, articulations named on their keys
    let kb = Rect::from_min_max(pos2(area.min.x + 10.0, mini_y + 24.0), pos2(area.max.x - 10.0, area.max.y - 10.0));
    let base = (24i32 + g.2 as i32 * 12).clamp(0, 103) as u8;
    let whites: Vec<u8> = (base..base.saturating_add(24)).filter(|n| ![1, 3, 6, 8, 10].contains(&(n % 12))).collect();
    let ww = kb.width() / whites.len() as f32;
    let live = app.artic.live.get(&t).and_then(|s| s.current);
    let mut clicked: Option<u8> = None;
    for (k, &n) in whites.iter().enumerate() {
        let r = Rect::from_min_size(pos2(kb.min.x + k as f32 * ww, kb.min.y), vec2(ww - 1.0, kb.height()));
        let sw = set.switches.iter().find(|w| w.kind == Msg::NoteOn && w.selector == n.wrapping_sub((g.2 as i32 * 12) as u8));
        let on = sw.is_some_and(|w| Some(w.artic) == live);
        p.rect_filled(r, 2.0, if on { look::accent() } else { Color32::from_gray(0xf2) });
        if let Some(w) = sw {
            let name = set.artic(w.artic).map_or(String::new(), |a| a.name.clone());
            p.text(pos2(r.center().x, r.max.y - 12.0), Align2::CENTER_CENTER, name.chars().take(8).collect::<String>(), look::regular(9.5), Color32::BLACK);
        }
        if ui.interact(r, ui.scope_id().with(("ks-key", n)), Sense::click()).clicked() {
            clicked = Some(n);
        }
    }
    if let Some(n) = clicked {
        let mut sw = app.artic.live.get(&t).cloned().unwrap_or_default();
        let gl = app.artic.global();
        sw.input(&set, if set.local { (true, None, set.octave) } else { (true, None, gl.2) }, Msg::NoteOn, 0, n, 100);
        app.artic.live.insert(t, sw);
    }
    if set.local {
        if let Some(s) = app.artic.tracks.get_mut(&t) {
            s.remote = g.0;
            s.octave = g.2;
        }
    } else if g != app.artic.global() {
        app.artic.global = g;
        app.artic.global_set = true;
    }
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
}

/// The Articulation Set Editor window and the Keyswitches window.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if app.artic.keys_window {
        let mut open = true;
        let t = app.selected_track;
        egui::Window::new("Keyswitches").open(&mut open).default_size(vec2(560.0, 200.0)).show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(400.0), 180.0), Sense::hover());
            if let Some(t) = t {
                keyswitch_view(app, ui, rect, t);
            }
        });
        app.artic.keys_window = open;
    }
    let Some((t, mut set, saved, mut pane)) = app.artic.editor.clone() else { return };
    let mut open = true;
    let mut apply = false;
    let track_name = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|x| x.name.clone()).unwrap_or_default();
    egui::Window::new(format!("Articulation Set: {}", set.name)).id(egui::Id::unique("artic-editor")).open(&mut open).default_size(vec2(640.0, 380.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut set.name);
            ui.label(egui::RichText::new(format!("Track: {track_name}")).color(look::text_dim()));
        });
        ui.horizontal(|ui| {
            for (i, n) in ["Articulations", "Switches", "Output"].iter().enumerate() {
                if ui.add(egui::Button::selectable(pane == i, *n)).clicked() {
                    pane = i;
                }
            }
        });
        ui.separator();
        let mut sel = app.artic.editor_sel;
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| match pane {
            0 => {
                egui::Grid::new("artic-a").striped(true).show(ui, |ui| {
                    for h in ["", "Name", "Articulation ID", "Channel", "Symbol"] {
                        ui.label(egui::RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, a) in set.artics.iter_mut().enumerate() {
                        ui.radio_value(&mut sel, Some(i), "");
                        ui.add(egui::TextEdit::singleline(&mut a.name).desired_width(170.0));
                        ui.add(egui::DragValue::new(&mut a.id).range(1..=255));
                        channel_combo(ui, ("a-ch", i), &mut a.channel);
                        egui::ComboBox::from_id_salt(("a-sym", i)).selected_text(SYMBOLS[(a.symbol as usize).min(SYMBOLS.len() - 1)]).show_ui(ui, |ui| {
                            for (k, s) in SYMBOLS.iter().enumerate() {
                                ui.selectable_value(&mut a.symbol, k as u8, *s);
                            }
                        });
                        ui.end_row();
                    }
                });
            }
            1 => {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut set.local, "Local to this set");
                    ui.add_enabled_ui(set.local, |ui| {
                        ui.checkbox(&mut set.remote, "MIDI Remote");
                        channel_combo(ui, "s-ch", &mut set.remote_channel);
                        ui.add(egui::DragValue::new(&mut set.octave).range(-10..=10).prefix("Octave Offset "));
                    });
                });
                let names: Vec<(u8, String)> = set.artics.iter().map(|a| (a.id, a.name.clone())).collect();
                egui::Grid::new("artic-s").striped(true).show(ui, |ui| {
                    for h in ["", "Type", "Selector", "Value Start", "Value End", "Mode", "Articulation"] {
                        ui.label(egui::RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, w) in set.switches.iter_mut().enumerate() {
                        ui.radio_value(&mut sel, Some(i), "");
                        msg_combo(ui, ("s-t", i), &mut w.kind, &Msg::SWITCH);
                        selector(ui, w.kind, &mut w.selector);
                        ui.add(egui::DragValue::new(&mut w.start).range(0..=127));
                        ui.add(egui::DragValue::new(&mut w.end).range(0..=127));
                        egui::ComboBox::from_id_salt(("s-m", i)).selected_text(w.mode.name()).show_ui(ui, |ui| {
                            for m in Mode::ALL {
                                ui.selectable_value(&mut w.mode, m, m.name());
                            }
                        });
                        egui::ComboBox::from_id_salt(("s-a", i)).selected_text(names.iter().find(|x| x.0 == w.artic).map_or(format!("{}", w.artic), |x| x.1.clone())).show_ui(ui, |ui| {
                            for (id, n) in &names {
                                ui.selectable_value(&mut w.artic, *id, n);
                            }
                        });
                        ui.end_row();
                    }
                });
            }
            _ => {
                ui.checkbox(&mut set.multiple_outputs, "Activate Multiple Outputs (up to three messages)");
                // a row per articulation
                for a in set.artics.clone() {
                    if !set.outputs.iter().any(|o| o.artic == a.id) {
                        set.outputs.push(Output { artic: a.id, msgs: vec![OutMsg { kind: Msg::NoteOn, channel: None, selector: 24, start: 100, end: 100 }] });
                    }
                }
                let multi = set.multiple_outputs;
                egui::Grid::new("artic-o").striped(true).show(ui, |ui| {
                    for h in ["Name", "Type", "Channel", "Selector", "Value Start", "Value End"] {
                        ui.label(egui::RichText::new(h).strong());
                    }
                    ui.end_row();
                    let names: Vec<(u8, String)> = set.artics.iter().map(|a| (a.id, a.name.clone())).collect();
                    for (i, o) in set.outputs.iter_mut().enumerate() {
                        if multi && o.msgs.len() < 3 {
                            let last = *o.msgs.last().unwrap_or(&OutMsg { kind: Msg::NoteOn, channel: None, selector: 24, start: 100, end: 100 });
                            o.msgs.resize(3, OutMsg { kind: last.kind, ..last });
                        }
                        let shown = if multi { 3 } else { 1 };
                        for (k, m) in o.msgs.iter_mut().take(shown).enumerate() {
                            ui.label(if k == 0 { names.iter().find(|x| x.0 == o.artic).map_or(String::new(), |x| x.1.clone()) } else { String::new() });
                            msg_combo(ui, ("o-t", i, k), &mut m.kind, &Msg::OUTPUT);
                            channel_combo(ui, ("o-ch", i, k), &mut m.channel);
                            selector(ui, m.kind, &mut m.selector);
                            ui.add(egui::DragValue::new(&mut m.start).range(0..=127));
                            ui.add(egui::DragValue::new(&mut m.end).range(0..=127));
                            ui.end_row();
                        }
                    }
                });
                set.outputs.retain(|o| set.artics.iter().any(|a| a.id == o.artic));
            }
        });
        ui.separator();
        // the plus, minus and Revert buttons at the bottom left; Apply at the right
        ui.horizontal(|ui| {
            if ui.button("+").on_hover_text("Add").clicked() {
                match pane {
                    0 => {
                        let id = set.next_id();
                        set.artics.push(Articulation { id, name: format!("Articulation {id}"), channel: None, symbol: 0 });
                    }
                    1 => {
                        let artic = set.artics.first().map_or(1, |a| a.id);
                        let sel = 24 + set.switches.len() as u8;
                        set.switches.push(Switch { kind: Msg::NoteOn, selector: sel, start: 1, end: 127, mode: Mode::Permanent, artic });
                    }
                    _ => {}
                }
            }
            if ui.add_enabled(sel.is_some() && pane < 2, egui::Button::new("−")).on_hover_text("Remove the selected row").clicked() {
                if let Some(i) = sel {
                    if pane == 0 && i < set.artics.len() {
                        set.artics.remove(i);
                    } else if pane == 1 && i < set.switches.len() {
                        set.switches.remove(i);
                    }
                }
                sel = None;
            }
            if ui.button("Revert").on_hover_text("This pane back to the saved set").clicked() {
                match pane {
                    0 => set.artics = saved.artics.clone(),
                    1 => {
                        set.switches = saved.switches.clone();
                        (set.remote, set.remote_channel, set.octave, set.local) = (saved.remote, saved.remote_channel, saved.octave, saved.local);
                    }
                    _ => {
                        set.outputs = saved.outputs.clone();
                        set.multiple_outputs = saved.multiple_outputs;
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Save to Library").clicked() {
                    save_set(&set);
                    apply = true;
                }
                if ui.button("Apply").clicked() {
                    apply = true;
                }
            });
        });
        app.artic.editor_sel = sel;
    });
    if apply {
        app.checkpoint("Articulation Set");
        app.artic.library.retain(|x| x.name != set.name);
        if dir().is_some_and(|d| d.join(format!("{}.txt", set.name)).exists()) {
            app.artic.library.push(set.clone());
        }
        app.artic.tracks.insert(t, set.clone());
        app.load_engine();
        app.artic.editor = if open { Some((t, set.clone(), set, pane)) } else { None };
    } else {
        app.artic.editor = if open { Some((t, set, saved, pane)) } else { None };
    }
}

fn channel_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, ch: &mut Option<u8>) {
    egui::ComboBox::from_id_salt(id).width(46.0).selected_text(ch.map_or("-".to_string(), |c| (c + 1).to_string())).show_ui(ui, |ui| {
        ui.selectable_value(ch, None, "-");
        for c in 0..16u8 {
            ui.selectable_value(ch, Some(c), (c + 1).to_string());
        }
    });
}

fn msg_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, m: &mut Msg, list: &[Msg]) {
    egui::ComboBox::from_id_salt(id).selected_text(m.name()).show_ui(ui, |ui| {
        for k in list {
            ui.selectable_value(m, *k, k.name());
        }
    });
}

/// The Selector: a note name for notes, a number for controllers, empty for Program,
/// Aftertouch and Pitch Bend.
fn selector(ui: &mut egui::Ui, kind: Msg, v: &mut u8) {
    if !kind.has_selector() {
        ui.label("");
    } else if matches!(kind, Msg::NoteOn | Msg::NoteOff | Msg::PolyAftertouch) {
        ui.add(egui::DragValue::new(v).range(0..=127).custom_formatter(|x, _| crate::app::note_name(x as u8)));
    } else {
        ui.add(egui::DragValue::new(v).range(0..=127));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> Set {
        Set {
            artics: vec![Articulation { id: 1, name: "Legato".into(), channel: None, symbol: 0 }, Articulation { id: 2, name: "Staccato".into(), channel: None, symbol: 1 }],
            switches: vec![
                Switch { kind: Msg::NoteOn, selector: 24, start: 1, end: 127, mode: Mode::Permanent, artic: 1 },
                Switch { kind: Msg::NoteOn, selector: 25, start: 1, end: 127, mode: Mode::Momentary, artic: 2 },
                Switch { kind: Msg::Controller, selector: 1, start: 64, end: 127, mode: Mode::Toggle, artic: 2 },
            ],
            outputs: vec![Output { artic: 2, msgs: vec![OutMsg { kind: Msg::NoteOn, channel: None, selector: 30, start: 100, end: 100 }, OutMsg { kind: Msg::Controller, channel: Some(0), selector: 32, start: 5, end: 5 }] }],
            ..Set::default()
        }
    }

    #[test]
    fn switch_modes() {
        let s = set();
        let g = (true, None, 0);
        let mut w = Switcher::default();
        assert_eq!(w.input(&s, g, Msg::NoteOn, 0, 24, 90).unwrap().artic, Some(1), "permanent");
        assert_eq!(w.input(&s, g, Msg::NoteOn, 0, 25, 90).unwrap().artic, Some(2), "momentary on");
        assert_eq!(w.input(&s, g, Msg::NoteOff, 0, 25, 0).unwrap().artic, Some(1), "momentary off: back");
        assert_eq!(w.input(&s, g, Msg::Controller, 0, 1, 100).unwrap().artic, Some(2), "toggle");
        assert_eq!(w.input(&s, g, Msg::Controller, 0, 1, 100).unwrap().artic, Some(1), "toggle back");
        assert!(w.input(&s, (false, None, 0), Msg::NoteOn, 0, 24, 90).is_none(), "MIDI Remote off");
        assert!(w.input(&s, (true, Some(3), 0), Msg::NoteOn, 0, 24, 90).is_none(), "another channel");
        assert_eq!(w.input(&s, (true, None, 1), Msg::NoteOn, 0, 36, 90).unwrap().artic, Some(1), "an octave up");
    }

    #[test]
    fn outputs_and_text() {
        let mut s = set();
        assert_eq!(s.outputs_for(2).len(), 1, "one message unless Activate Multiple Outputs");
        s.multiple_outputs = true;
        assert_eq!(s.outputs_for(2).len(), 2);
        let back = Set::from_text(&s.to_text()).unwrap();
        assert_eq!(back, s);
        assert_eq!(s.next_id(), 3);
        assert!(!Msg::PitchBend.has_selector() && Msg::Controller.has_selector());
    }

    #[test]
    fn note_ids() {
        let mut r = lg_core::Region { name: "r".into(), start: 960, length: 3840, notes: vec![Note { channel: 0, pitch: 60, velocity: 90, pos: 960, len: 100, muted: false }], audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        set_note_artic(&mut r, 0, Some(2));
        assert_eq!(note_artic(&r, &r.notes[0].clone()), Some(2));
        set_note_artic(&mut r, 0, None);
        assert_eq!(note_artic(&r, &r.notes[0].clone()), None);
    }
}

/// Whether track `t` has an articulation set (the Score's note menu shows Articulation ▸).
pub fn has_set(app: &mut App, t: usize) -> bool {
    load(app);
    app.artic.tracks.get(&t).is_some_and(|s| !s.artics.is_empty())
}

/// The Score's note Control-click Articulation submenu: the track's set, for the selected notes.
pub fn note_menu(app: &mut App, ui: &mut egui::Ui, t: usize) {
    let Some(set) = app.artic.tracks.get(&t).cloned() else { return };
    if ui.button("No Articulation").clicked() {
        command(app, "Set No Articulation");
        ui.close();
    }
    for a in &set.artics {
        if ui.button(format!("{} {}", a.id, a.name)).clicked() {
            command(app, &format!("Set Articulation {:>2}", a.id));
            ui.close();
        }
    }
}
