//! Project Settings ▸ MIDI ▸ Chase, as Logic's: at a play start, a locate and (when chosen) a
//! cycle jump, each track gets the last event of every chased kind from before the start point
//! (program change, pitch bend, the three controller ranges, aftertouch, poly aftertouch), notes
//! spanning the start point, and notes that ended but are still held by the sustain pedal.
//! "Chase separate channels" keeps the last event per MIDI channel; "Send full MIDI reset before
//! chasing" sends the Reset Messages (Settings ▸ MIDI) when playback stops.
//!
//! Studio's regions carry no SysEx or text meta events, so those two boxes have nothing to send.

use lg_dsp::midifx::Msg;

use crate::projsettings::Values;

/// The Chase pane's boxes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cfg {
    pub notes: bool,
    pub sustained: bool,
    /// Notes on No Transpose strips (drums, samplers) chased too.
    pub no_transpose: bool,
    pub program: bool,
    pub bend: bool,
    pub cc: bool,
    pub cc_0_15: bool,
    pub cc_64_71: bool,
    pub cc_other: bool,
    pub aftertouch: bool,
    pub poly: bool,
    pub sysex: bool,
    pub text: bool,
    pub separate_channels: bool,
    pub on_cycle_jump: bool,
    pub cycle_notes: bool,
    pub reset_before: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg::of(&Values::default())
    }
}

const P: &str = "MIDI / Chase / ";

impl Cfg {
    pub fn of(v: &Values) -> Cfg {
        let on = |l: &str| v.on(&format!("{P}{l}"));
        Cfg {
            notes: on("Notes"),
            sustained: on("Sustained"),
            no_transpose: on("In No Transpose instrument channel strips"),
            program: on("Program Change"),
            bend: on("Pitch Bend"),
            cc: on("Control Changes"),
            cc_0_15: on("0-15"),
            cc_64_71: on("64-71"),
            cc_other: on("All Other"),
            aftertouch: on("Aftertouch"),
            poly: on("Polyphonic Aftertouch"),
            sysex: on("System Exclusive"),
            text: on("Text Meta Events"),
            separate_channels: on("Chase separate channels in All Channels instruments"),
            on_cycle_jump: on("Chase on cycle jump"),
            cycle_notes: on("Notes (on cycle jump)"),
            reset_before: on("Send full MIDI reset before chasing"),
        }
    }

    /// Whether a message of this kind is chased (only the ticked kinds are).
    pub fn wants(&self, m: &Msg) -> bool {
        match *m {
            Msg::Program { .. } => self.program,
            Msg::Bend { .. } => self.bend,
            Msg::Pressure { .. } => self.aftertouch,
            Msg::PolyAt { .. } => self.poly,
            Msg::Cc { num, .. } => self.cc && if num <= 15 { self.cc_0_15 } else if (64..=71).contains(&num) { self.cc_64_71 } else { self.cc_other },
            Msg::On { .. } | Msg::Off { .. } => false,
        }
    }
}

/// What the engine keeps for chasing: the boxes, which tracks are No Transpose, and whether the
/// pending chase is for a cycle jump.
#[derive(Clone, Debug, Default)]
pub struct Chase {
    pub cfg: Cfg,
    pub no_transpose: Vec<bool>,
    pub jump: bool,
}

/// Which kind of event a message is, for "the last of each kind" (with the channel when chasing
/// channels separately).
fn slot(m: &Msg, separate: bool) -> (u8, u8, u8) {
    let ch = if separate { m.ch() } else { 0 };
    match *m {
        Msg::Cc { num, .. } => (0xB0, num, ch),
        Msg::Bend { .. } => (0xE0, 0, ch),
        Msg::Pressure { .. } => (0xD0, 0, ch),
        Msg::PolyAt { pitch, .. } => (0xA0, pitch, ch),
        Msg::Program { .. } => (0xC0, 0, ch),
        Msg::On { .. } | Msg::Off { .. } => (0x90, 0, ch),
    }
}

/// The last event of each chased kind before `from` (song ticks), in the order they came.
pub fn events(controls: &[(u32, Msg)], from: f64, cfg: &Cfg) -> Vec<Msg> {
    let mut last: Vec<((u8, u8, u8), u32, Msg)> = Vec::new();
    for &(at, m) in controls.iter().filter(|c| (c.0 as f64) < from) {
        if !cfg.wants(&m) {
            continue;
        }
        let s = slot(&m, cfg.separate_channels);
        match last.iter_mut().find(|l| l.0 == s) {
            Some(l) => {
                l.1 = at;
                l.2 = m;
            }
            None => last.push((s, at, m)),
        }
    }
    last.sort_by_key(|l| l.1);
    last.into_iter().map(|l| l.2).collect()
}

/// Whether the sustain pedal (controller 64) is down at `at` (the last value before it ≥ 64).
fn pedal_down(controls: &[(u32, Msg)], at: f64) -> bool {
    controls.iter().filter(|c| (c.0 as f64) <= at).filter_map(|c| if let Msg::Cc { num: 64, val, .. } = c.1 { Some(val >= 64) } else { None }).last().unwrap_or(false)
}

/// Chase Sustained: notes that ended before `from` while the pedal was down and that the pedal
/// still holds at `from` (pitch, velocity).
pub fn sustained(notes: &[(u32, u32, u8, u8)], controls: &[(u32, Msg)], from: f64) -> Vec<(u8, u8)> {
    if !pedal_down(controls, from) {
        return Vec::new();
    }
    // the pedal's last press before `from`: notes ending after it are still held
    let pressed = controls.iter().filter(|c| (c.0 as f64) < from).fold(None, |acc, c| match c.1 {
        Msg::Cc { num: 64, val, .. } if val >= 64 => acc.or(Some(c.0)),
        Msg::Cc { num: 64, .. } => None,
        _ => acc,
    });
    let Some(p) = pressed else { return Vec::new() };
    notes.iter().filter(|n| (n.1 as f64) <= from && n.1 >= p && (n.0 as f64) < from).map(|n| (n.2, n.3)).collect()
}

/// The engine's chase at a play start, locate or cycle jump (audio.rs calls this before it
/// chases the notes under way): the chased events and the pedal-held notes, per audible track.
/// Returns whether notes are chased this time.
pub fn run(e: &mut crate::audio::Engine, from: f64, audible: &[bool]) -> bool {
    let jump = std::mem::take(&mut e.chase.jump);
    let cfg = e.chase.cfg;
    for t in 0..e.tracks.len() {
        if !audible.get(t).copied().unwrap_or(true) {
            continue;
        }
        let Some(cs) = e.fx.controls.get(t) else { continue };
        let msgs = events(cs, from, &cfg);
        let held = if cfg.notes && cfg.sustained && (!jump || cfg.cycle_notes) && notes_wanted(&e.chase, t) { sustained(&e.tracks[t].notes, cs, from) } else { Vec::new() };
        for m in msgs {
            match m {
                Msg::Cc { num, val, .. } => e.control_out(t, num, val as u16),
                Msg::Bend { val, .. } => e.control_out(t, 128, (val as i32 + 8192).clamp(0, 16383) as u16),
                Msg::Pressure { val, .. } => e.live_message(t, 0xD0, val, 0),
                Msg::PolyAt { pitch, val, .. } => e.live_message(t, 0xA0, pitch, val),
                Msg::Program { num, .. } => e.live_message(t, 0xC0, num, 0),
                _ => {}
            }
        }
        // (struck and released at once: the pedal, chased above, holds them)
        for (p, v) in held {
            e.live_note(t, p, v);
            e.live_note(t, p, 0);
        }
    }
    cfg.notes && (!jump || cfg.cycle_notes)
}

/// Notes on this track are chased (In No Transpose instrument channel strips).
pub fn notes_wanted(c: &Chase, t: usize) -> bool {
    c.cfg.no_transpose || !c.no_transpose.get(t).copied().unwrap_or(false)
}

/// The Chase pane (Project Settings ▸ MIDI ▸ Chase), laid out as Logic's: the event kinds
/// under "Chase Events", the controller ranges under Control Changes, then the cycle-jump and
/// reset boxes.
pub fn pane(ui: &mut egui::Ui, v: &mut Values) {
    ui.label(egui::RichText::new("Chase Events:").strong());
    ui.indent("chase-events", |ui| {
        let notes = v.on(&format!("{P}Notes"));
        cb(ui, v, "Notes", "Notes", true);
        ui.indent("chase-notes", |ui| {
            cb(ui, v, "Sustained", "Sustained", notes);
            cb(ui, v, "In No Transpose instrument channel strips", "In No Transpose instrument channel strips", notes);
        });
        cb(ui, v, "Program Change", "Program Change", true);
        cb(ui, v, "Pitch Bend", "Pitch Bend", true);
        let ccs = v.on(&format!("{P}Control Changes"));
        cb(ui, v, "Control Changes", "Control Changes", true);
        ui.indent("chase-cc", |ui| {
            ui.horizontal(|ui| {
                cb(ui, v, "0-15", "0–15", ccs);
                cb(ui, v, "64-71", "64–71", ccs);
                cb(ui, v, "All Other", "All Other", ccs);
            });
        });
        cb(ui, v, "Aftertouch", "Aftertouch", true);
        cb(ui, v, "Polyphonic Aftertouch", "Polyphonic Aftertouch", true);
        cb(ui, v, "System Exclusive", "System Exclusive", true);
        cb(ui, v, "Text Meta Events", "Text Meta Events", true);
    });
    ui.add_space(6.0);
    cb(ui, v, "Chase separate channels in All Channels instruments", "Chase separate channels in All Channels instruments", true);
    let jump = v.on(&format!("{P}Chase on cycle jump"));
    cb(ui, v, "Chase on cycle jump", "Chase on cycle jump", true);
    ui.indent("chase-jump", |ui| cb(ui, v, "Notes (on cycle jump)", "Notes", jump));
    cb(ui, v, "Send full MIDI reset before chasing", "Send full MIDI reset before chasing", true);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Chased at a play start, a locate and (when chosen) each cycle jump. The full reset sends what Settings ▸ MIDI ▸ Reset Messages ticks when playback stops. Studio's regions hold no SysEx or text events to chase.").small().color(crate::look::text_dim()));
}

fn cb(ui: &mut egui::Ui, v: &mut Values, l: &str, text: &str, enabled: bool) {
    let k = format!("{P}{l}");
    let mut on = v.on(&k);
    if ui.add_enabled(enabled, egui::Checkbox::new(&mut on, text)).changed() {
        crate::projsettings::set(v, &k, if on { "1" } else { "0" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(num: u8, val: u8) -> Msg {
        Msg::Cc { ch: 0, num, val }
    }

    #[test]
    fn last_of_each_kind() {
        let cs = vec![(0, cc(7, 90)), (10, Msg::Program { ch: 0, num: 5 }), (20, cc(7, 60)), (30, cc(1, 40)), (40, Msg::Bend { ch: 0, val: 100 }), (900, cc(7, 10))];
        let c = Cfg::default();
        let ev = events(&cs, 500.0, &c);
        assert_eq!(ev, vec![Msg::Program { ch: 0, num: 5 }, cc(7, 60), cc(1, 40), Msg::Bend { ch: 0, val: 100 }]);
        // only the ticked kinds
        let c2 = Cfg { program: false, cc_0_15: false, ..c };
        assert_eq!(events(&cs, 500.0, &c2), vec![Msg::Bend { ch: 0, val: 100 }]);
        let c3 = Cfg { cc: false, ..c };
        assert!(events(&cs, 500.0, &c3).iter().all(|m| !matches!(m, Msg::Cc { .. })));
    }

    #[test]
    fn ranges_and_channels() {
        let c = Cfg::default();
        assert!(c.wants(&cc(64, 127)) && c.wants(&cc(100, 1)) && c.wants(&cc(3, 1)));
        let c2 = Cfg { cc_64_71: false, ..c };
        assert!(!c2.wants(&cc(64, 127)) && c2.wants(&cc(72, 0)));
        let cs = vec![(0, Msg::Cc { ch: 0, num: 7, val: 1 }), (5, Msg::Cc { ch: 1, num: 7, val: 2 })];
        assert_eq!(events(&cs, 10.0, &Cfg { separate_channels: false, ..c }).len(), 1);
        assert_eq!(events(&cs, 10.0, &Cfg { separate_channels: true, ..c }).len(), 2);
    }

    #[test]
    fn pedal_holds_ended_notes() {
        let notes = vec![(0, 100, 60, 90), (150, 200, 64, 80), (300, 700, 67, 70)];
        let cs = vec![(120, cc(64, 127))];
        // C ended before the pedal went down; E ended under it
        assert_eq!(sustained(&notes, &cs, 400.0), vec![(64, 80)]);
        let up = vec![(120, cc(64, 127)), (250, cc(64, 0))];
        assert!(sustained(&notes, &up, 400.0).is_empty());
    }

    #[test]
    fn defaults_as_logic() {
        let c = Cfg::default();
        assert!(c.notes && c.sustained && c.program && c.bend && c.cc && c.aftertouch && c.poly);
        assert!(!c.on_cycle_jump && !c.reset_before);
    }
}
