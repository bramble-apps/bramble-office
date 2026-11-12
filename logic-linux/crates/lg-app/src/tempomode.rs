//! Smart Tempo's Project Tempo mode as Logic's: Keep, Adapt and Automatic (the LCD's Tempo
//! display: KEEP / ADAPT / AUTO, a click in its middle for the menu; Project Settings ▸ Smart
//! Tempo), what Adapt does (new recordings in an empty project or after the last region, with
//! the metronome off, set the project tempo; moving, trimming or deleting the only region in a
//! range takes its tempo changes along), Automatic (Adapt only without a musical tempo
//! reference: metronome, regions there, Cycle), Free Tempo Recording (⌃⌥⌘R: no count-in or
//! metronome, the selected tracks soloed, then the four choices), the Region inspector's Smart
//! Tempo setting (Off / On / On + Align Bars / On + Align Bars and Beats), the Apply Region
//! Tempo dialog's options, and Move Region to Beat.

use std::path::PathBuf;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Keep Project Tempo",
    "Adapt Project Tempo",
    "Automatic Mode",
    "Toggle Smart Tempo Mode (Keep/Adapt)",
    "Set Smart Tempo Mode to Keep",
    "Set Smart Tempo Mode to Adapt",
    "Set Smart Tempo Mode to Automatic",
    "Free Tempo Recording",
    "Move Region to Beat",
    "To Beat",
    "Apply Region Tempo to Project Tempo…",
];

const KEY: &str = "Smart Tempo /  / Project Tempo Mode:";
const FREE_KEY: &str = "Smart Tempo /  / Free Tempo Recording Default Tempo Behavior:";
pub const MODES: [&str; 3] = ["Keep Project Tempo", "Adapt Project Tempo", "Automatic Mode"];
pub const SMART: [&str; 4] = ["Off", "On", "On + Align Bars", "On + Align Bars and Beats"];
pub const FREE_CHOICES: [&str; 4] = ["Apply region tempo to project", "Apply average region tempo to project", "Apply project tempo to region", "Don't analyze region tempo or change project tempo"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Keep,
    Adapt,
    Auto,
}

#[derive(Default)]
pub struct State {
    /// Recorded files waiting for their analysis, then the project tempo (Adapt).
    adapt: Vec<PathBuf>,
    /// Files whose recording was seen.
    seen: std::collections::HashSet<PathBuf>,
    /// A free tempo recording running: what to put back (count-in, metronome, solos).
    free: Option<(bool, bool, Vec<usize>)>,
    /// The free tempo recording's file, waiting for the choice (and its analysis).
    free_file: Option<PathBuf>,
    free_choice: Option<usize>,
    /// Apply Region Tempo to Project Tempo…: the dialog (align downbeat, keep others relative).
    pub apply_dialog: Option<(bool, bool)>,
    /// Adapt: the audio regions (by file) and tempo changes as last seen.
    last: std::collections::HashMap<PathBuf, (u32, u32)>,
    last_tm: Option<Vec<(u32, f64)>>,
    /// Smart Tempo defaults (stdefaults.rs): new regions waiting for their analysis.
    pub st_pending: Vec<crate::stdefaults::Pending>,
    /// Independent monitoring level (recsettings.rs): faders as they were when armed.
    pub armed_levels: crate::recsettings::Armed,
}

pub fn mode(app: &App) -> Mode {
    match app.proj.get(KEY).as_str() {
        "Keep Project Tempo" => Mode::Keep,
        "Adapt Project Tempo" => Mode::Adapt,
        _ => Mode::Auto,
    }
}

pub fn set_mode(app: &mut App, m: Mode) {
    let v = MODES[m as usize];
    app.proj.0.insert(KEY.to_string(), v.to_string());
    app.dirty = true;
    if m == Mode::Adapt {
        // Logic: the Tempo track opens, Cycle goes off (recording starts at the cycle's start)
        app.global_tracks = true;
        crate::globals::command(app, "Show Tempo Track");
        if !crate::globals::shown(app, crate::globals::TEMPO) || !app.global_tracks {
            crate::globals::command(app, "Show Tempo Track");
        }
        if app.cycle_on() {
            let a = app.cycle.0;
            app.toggle_cycle();
            app.locate(a as f64);
        }
    }
}

fn metronome(app: &App) -> bool {
    app.engine.lock().map(|e| e.metronome).unwrap_or(false)
}

/// Whether a recording over `range` adapts the project tempo: Adapt (without the metronome as
/// its reference, and with no earlier recording there), or Automatic without a musical tempo
/// reference (metronome, regions in the range, Cycle).
pub fn adapts(m: Mode, metronome: bool, cycle: bool, regions_there: bool) -> bool {
    match m {
        Mode::Keep => false,
        Mode::Adapt => !metronome && !regions_there,
        Mode::Auto => !metronome && !cycle && !regions_there,
    }
}

/// Other regions in `a..b` (or after `a` when the recording reaches past the last region).
fn regions_there(app: &App, file: &std::path::Path, a: u32) -> bool {
    app.project.as_ref().is_some_and(|p| p.tracks.iter().flat_map(|t| t.regions.iter()).any(|r| r.audio.as_ref().is_none_or(|x| x.file != file) && r.start + r.length > a))
}

fn find_region(app: &App, file: &std::path::Path) -> Option<(usize, usize)> {
    let p = app.project.as_ref()?;
    p.tracks.iter().enumerate().find_map(|(t, tr)| tr.regions.iter().position(|r| r.audio.as_ref().is_some_and(|x| x.file == file)).map(|r| (t, r)))
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Keep Project Tempo" | "Set Smart Tempo Mode to Keep" => set_mode(app, Mode::Keep),
        "Adapt Project Tempo" | "Set Smart Tempo Mode to Adapt" => set_mode(app, Mode::Adapt),
        "Automatic Mode" | "Set Smart Tempo Mode to Automatic" => set_mode(app, Mode::Auto),
        "Toggle Smart Tempo Mode (Keep/Adapt)" => {
            let m = if mode(app) == Mode::Keep { Mode::Adapt } else { Mode::Keep };
            set_mode(app, m);
        }
        "Free Tempo Recording" => {
            if app.recording() {
                return false;
            }
            let solos: Vec<usize> = crate::trackarea::selection(app);
            let was = (app.count_in, metronome(app), solos.clone());
            app.count_in = false;
            if let Ok(mut e) = app.engine.lock() {
                e.metronome = false;
            }
            if let Some(p) = app.project.as_mut() {
                for &t in &solos {
                    if let Some(tr) = p.tracks.get_mut(t) {
                        tr.solo = true;
                    }
                }
            }
            app.tm2.free = Some(was);
            crate::recording::record_pressed(app, "Record");
        }
        "Move Region to Beat" | "To Beat" => return move_to_beat(app),
        "Apply Region Tempo to Project Tempo…" => app.tm2.apply_dialog = Some((true, true)),
        _ => return false,
    }
    true
}

/// Move Region to Beat: the region's first detected downbeat to the nearest beat.
fn move_to_beat(app: &mut App) -> bool {
    let Some(rt) = crate::smarttempo::selected(app) else { return false };
    let Some(a) = app.audio_edit.st.maps.get(&rt.file) else {
        app.tell("Move to Beat: the region's file needs its Smart Tempo analysis");
        return true;
    };
    let Some(&first) = a.map.beats.get(a.map.downbeat.min(a.map.beats.len().saturating_sub(1))) else { return true };
    let tick = crate::smarttempo::file_to_tick(app, &rt, first);
    let beat = lg_core::events::PPQ as f64;
    let d = ((tick / beat).round() * beat - tick).round() as i64;
    app.checkpoint("Move Region to Beat");
    if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(rt.track)?.regions.get_mut(rt.region)) {
        r.start = (r.start as i64 + d).max(0) as u32;
    }
    app.after_edit();
    true
}

/// The LCD Tempo display's mode label (KEEP / ADAPT / AUTO): a click opens the mode menu.
pub fn lcd_label(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, at: egui::Pos2) {
    let text = match mode(app) {
        Mode::Keep => "KEEP",
        Mode::Adapt => "ADAPT",
        Mode::Auto => "AUTO",
    };
    p.text(at, egui::Align2::CENTER_CENTER, text, look::semibold(8.0), look::lcd_dim());
    let r = egui::Rect::from_center_size(at, egui::vec2(44.0, 12.0));
    let resp = ui.interact(r, ui.scope_id().with("tempo-mode"), egui::Sense::click()).on_hover_text("Project Tempo mode");
    egui::Popup::menu(&resp).show(|ui| {
        let cur = mode(app);
        for (i, m) in [Mode::Keep, Mode::Adapt, Mode::Auto].into_iter().enumerate() {
            if ui.add(egui::Button::selectable(cur == m, MODES[i])).clicked() {
                set_mode(app, m);
                ui.close();
            }
        }
    });
}

/// The Region inspector's Smart Tempo setting of an audio region (On: flexed to follow the
/// project tempo, the Align settings at every downbeat / beat).
pub fn smart_tempo(app: &App, t: usize, r: usize) -> usize {
    let x = crate::audioedit::extras(app, t, r);
    if x.follow {
        x.smart_tempo.clamp(1, 3) as usize
    } else {
        0
    }
}

pub fn set_smart_tempo(app: &mut App, t: usize, r: usize, v: usize) {
    let mut x = crate::audioedit::extras(app, t, r);
    x.smart_tempo = v as u8;
    x.follow = v > 0;
    crate::audioedit::set_extras(app, t, r, x);
    if v > 0 {
        if let Some(f) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)?.audio.clone()) {
            if !app.audio_edit.st.maps.contains_key(&f.file) && !crate::smarttempo::analysing(app, &f.file) {
                crate::smarttempo::analyze(app, &f.file);
            }
        }
        if !app.flex.tracks.get(&t).is_some_and(|x| x.on) {
            let mut tf = app.flex.tracks.get(&t).cloned().unwrap_or_default();
            tf.on = true;
            app.flex.tracks.insert(t, tf);
        }
    }
    app.after_edit();
}

/// Each frame: recordings seen (Adapt), analyses done, the free tempo recording's end, dialogs.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    // new recordings
    let new: Vec<(PathBuf, f64)> = app.rec.recorded_at.iter().filter(|(p, _)| !app.tm2.seen.contains(*p)).map(|(p, t)| (p.clone(), *t)).collect();
    for (f, at) in new {
        app.tm2.seen.insert(f.clone());
        if app.tm2.free.is_some() {
            app.tm2.free_file = Some(f.clone());
            continue;
        }
        crate::stdefaults::recorded(app, &f);
        let there = regions_there(app, &f, at as u32);
        if adapts(mode(app), metronome(app), app.cycle_on(), there) {
            crate::smarttempo::analyze(app, &f);
            app.tm2.adapt.push(f);
        }
    }
    // a free tempo recording stopped: settings back, then the choice
    if app.tm2.free.is_some() && !app.recording() && app.tm2.free_file.is_some() {
        let (ci, met, solos) = app.tm2.free.take().unwrap_or_default();
        app.count_in = ci;
        if let Ok(mut e) = app.engine.lock() {
            e.metronome = met;
        }
        if let Some(p) = app.project.as_mut() {
            for t in solos {
                if let Some(tr) = p.tracks.get_mut(t) {
                    tr.solo = false;
                }
            }
        }
        let f = app.tm2.free_file.clone().unwrap_or_default();
        crate::smarttempo::analyze(app, &f);
        let dflt = FREE_CHOICES.iter().position(|c| *c == app.proj.get(FREE_KEY));
        app.tm2.free_choice = dflt;
        if dflt.is_none() {
            app.tm2.free_choice = Some(usize::MAX);
        }
    }
    // analyses done: the project tempo
    let ready: Vec<PathBuf> = app.tm2.adapt.iter().filter(|f| app.audio_edit.st.maps.contains_key(*f)).cloned().collect();
    for f in ready {
        app.tm2.adapt.retain(|x| *x != f);
        if let Some((t, r)) = find_region(app, &f) {
            app.selected_region = Some((t, r));
            app.selected_track = Some(t);
            crate::smarttempo::apply_region_tempo(app);
        }
    }
    crate::stdefaults::frame(app);
    crate::recsettings::frame(app);
    follow_regions(app);
    free_dialog(app, ctx);
    apply_dialog(app, ctx);
}

/// Adapt: a region alone in its range moved, trimmed or deleted takes its tempo changes along
/// (an undo, which puts the tempo back too, is only noted).
fn follow_regions(app: &mut App) {
    if mode(app) != Mode::Adapt {
        app.tm2.last_tm = None;
        return;
    }
    let Some(p) = app.project.as_ref() else { return };
    let spans: Vec<(PathBuf, u32, u32)> = p.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| Some((r.audio.as_ref()?.file.clone(), r.start, r.start + r.length))).collect();
    let mut now: std::collections::HashMap<PathBuf, (u32, u32)> = Default::default();
    for (f, a, b) in &spans {
        now.entry(f.clone()).or_insert((*a, *b));
    }
    let all: Vec<(u32, u32)> = p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| (r.start, r.start + r.length)).collect();
    if app.tm2.last_tm.as_ref() == Some(&p.tempo_map) {
        let last = app.tm2.last.clone();
        for (f, (a, b)) in last {
            let after = now.get(&f).copied();
            if after == Some((a, b)) {
                continue;
            }
            // alone in its range: no other region overlapping where it was
            let others = all.iter().filter(|r| r.0 < b && r.1 > a).count();
            let alone = others <= after.map_or(0, |x| (x.0 < b && x.1 > a) as usize);
            if !alone {
                continue;
            }
            let Some(p) = app.project.as_mut() else { return };
            match after {
                Some((s, e)) if e - s == b - a => carry_tempo(p, (a, b), Some(s), e),
                Some((s, e)) => p.tempo_map.retain(|x| x.0 == 0 || !(x.0 > a && x.0 < b) || (x.0 >= s && x.0 < e)),
                None => carry_tempo(p, (a, b), None, 0),
            }
        }
    }
    app.tm2.last = now;
    app.tm2.last_tm = app.project.as_ref().map(|p| p.tempo_map.clone());
}

fn free_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(c) = app.tm2.free_choice else { return };
    let Some(f) = app.tm2.free_file.clone() else {
        app.tm2.free_choice = None;
        return;
    };
    let mut pick = (c != usize::MAX).then_some(c);
    if pick.is_none() {
        egui::Window::new("Free Tempo Recording").id(egui::Id::unique("free-tempo")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.label("What should happen with the tempo of the recording?");
            ui.add_space(6.0);
            for (i, n) in FREE_CHOICES.iter().enumerate() {
                if ui.button(*n).clicked() {
                    pick = Some(i);
                }
            }
        });
    }
    let Some(i) = pick else { return };
    // the analysis first (except for "Don't analyze")
    if i < 3 && !app.audio_edit.st.maps.contains_key(&f) {
        app.tm2.free_choice = Some(i);
        return;
    }
    app.tm2.free_choice = None;
    app.tm2.free_file = None;
    let Some((t, r)) = find_region(app, &f) else { return };
    app.selected_region = Some((t, r));
    app.selected_track = Some(t);
    match i {
        0 => {
            crate::smarttempo::apply_region_tempo(app);
            set_smart_tempo(app, t, r, 1);
        }
        1 => {
            let avg = app.audio_edit.st.maps.get(&f).and_then(|a| average_bpm(&a.map.beats, a.map.beat_unit));
            if let Some(bpm) = avg {
                app.checkpoint("Apply Average Region Tempo");
                app.set_tempo(bpm);
                if let Some(p) = app.project.as_mut() {
                    p.tempo_map.retain(|x| x.0 == 0);
                }
            }
            set_smart_tempo(app, t, r, 3);
        }
        2 => {
            crate::smarttempo::apply_project_tempo(app);
            set_smart_tempo(app, t, r, 3);
        }
        _ => set_smart_tempo(app, t, r, 0),
    }
}

/// The average tempo of beats (seconds) in quarter notes per minute.
pub fn average_bpm(beats: &[f64], beat_unit: u8) -> Option<f64> {
    if beats.len() < 2 {
        return None;
    }
    let span = beats[beats.len() - 1] - beats[0];
    (span > 0.0).then(|| (60.0 * (beats.len() - 1) as f64 / span * 4.0 / beat_unit.max(1) as f64 * 100.0).round() / 100.0)
}

/// Apply Region Tempo to Project Tempo…: Align downbeat to nearest project downbeat, Maintain
/// relative position of all other regions.
fn apply_dialog(app: &mut App, ctx: &egui::Context) {
    let Some((mut align, mut keep)) = app.tm2.apply_dialog else { return };
    let mut close = false;
    let mut go = false;
    egui::Window::new("Apply Region Tempo to Project Tempo").id(egui::Id::unique("apply-region-tempo")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.checkbox(&mut align, "Align downbeat to nearest project downbeat");
        ui.checkbox(&mut keep, "Maintain relative position of all other regions");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if ui.button("Apply").clicked() {
                go = true;
            }
        });
    });
    app.tm2.apply_dialog = if close || go { None } else { Some((align, keep)) };
    if !go {
        return;
    }
    let Some((t, r)) = app.selected_region else { return };
    let before = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r).map(|x| x.start));
    let secs = before.map(|s| app.seconds_at(s as f64));
    if !crate::smarttempo::apply_region_tempo(app) {
        return;
    }
    let Some(p) = app.project.as_mut() else { return };
    let Some(now) = p.tracks.get(t).and_then(|x| x.regions.get(r)).map(|x| x.start) else { return };
    let _ = secs;
    if !align {
        // (the region stays where it was on the ruler)
        if let (Some(b), Some(x)) = (before, p.tracks.get_mut(t).and_then(|x| x.regions.get_mut(r))) {
            x.start = b;
        }
    } else if keep {
        // everything else moves with the region
        let d = now as i64 - before.unwrap_or(now) as i64;
        for (ti, tr) in p.tracks.iter_mut().enumerate() {
            for (ri, rg) in tr.regions.iter_mut().enumerate() {
                if (ti, ri) != (t, r) {
                    rg.start = (rg.start as i64 + d).max(0) as u32;
                }
            }
            for l in tr.automation.iter_mut() {
                for pt in l.points.iter_mut() {
                    pt.0 = (pt.0 as i64 + d).max(0) as u32;
                }
            }
        }
    }
    app.after_edit();
}

/// Adapt behaviour: a region alone in its range carries its tempo changes when it moves, and
/// they go with it when it is trimmed or deleted. `range`: the region before; `to`: its start
/// after (None: deleted); `end`: its end after.
pub fn carry_tempo(p: &mut lg_core::Project, range: (u32, u32), to: Option<u32>, end: u32) {
    let (a, b) = range;
    let inside: Vec<(u32, f64)> = p.tempo_map.iter().copied().filter(|x| x.0 > a && x.0 < b).collect();
    if inside.is_empty() {
        return;
    }
    p.tempo_map.retain(|x| !(x.0 > a && x.0 < b));
    if let Some(s) = to {
        let d = s as i64 - a as i64;
        for (t, bpm) in inside {
            let n = (t as i64 + d).max(1) as u32;
            if n < end {
                p.tempo_map.push((n, bpm));
            }
        }
        p.tempo_map.sort_by_key(|x| x.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapt_and_auto_decide_by_reference() {
        assert!(!adapts(Mode::Keep, false, false, false));
        assert!(adapts(Mode::Adapt, false, true, false));
        assert!(!adapts(Mode::Adapt, true, false, false), "the metronome is the reference");
        assert!(!adapts(Mode::Adapt, false, false, true), "an earlier recording's range");
        assert!(adapts(Mode::Auto, false, false, false));
        assert!(!adapts(Mode::Auto, false, true, false), "Cycle on: a reference");
        assert!(!adapts(Mode::Auto, true, false, false));
    }

    #[test]
    fn modes_and_average() {
        let mut app = crate::tm::test_app();
        assert_eq!(mode(&app), Mode::Auto);
        command(&mut app, "Set Smart Tempo Mode to Adapt");
        assert_eq!(mode(&app), Mode::Adapt);
        assert!(app.global_tracks && crate::globals::shown(&app, crate::globals::TEMPO));
        command(&mut app, "Toggle Smart Tempo Mode (Keep/Adapt)");
        assert_eq!(mode(&app), Mode::Keep);
        assert_eq!(average_bpm(&[0.0, 0.5, 1.0, 1.5], 4), Some(120.0));
    }

    #[test]
    fn tempo_changes_travel_with_the_region() {
        let mut p = crate::demo::empty();
        p.tempo_map = vec![(0, 120.0), (1000, 100.0), (5000, 90.0)];
        carry_tempo(&mut p, (500, 2000), Some(3500), 5000);
        assert_eq!(p.tempo_map, vec![(0, 120.0), (4000, 100.0), (5000, 90.0)]);
        carry_tempo(&mut p, (3500, 4500), None, 0);
        assert_eq!(p.tempo_map, vec![(0, 120.0), (5000, 90.0)]);
    }
}
