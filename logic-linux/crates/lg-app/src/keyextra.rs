//! More of Logic's key commands, run by their Logic names (keycmds.rs / keymap.rs bind the keys):
//! Undo / Redo as commands (so they follow a reassigned key), Set Next / Previous Tool, Hide/Show
//! Selected Tracks, Move Selected Regions to Focused Track, Color Tracks by Region/Cell Color,
//! Waveform Vertical Zoom In / Out, Activate Next / Previous Track Alternative, Next / Previous
//! Patch, Plug-in Setting or Sampler Instrument, Cycle Through Windows, Open in External Sample
//! Editor, and Logic's other names for commands Studio already has (Drum Replacement/Doubling,
//! Show/Hide Staff Style Window, Show/Hide Selection-Based Processing).

use crate::app::App;
use crate::edit::Tool;

pub const COMMANDS: &[&str] = &[
    "Undo",
    "Redo",
    "Set Next Tool or Edit Mode",
    "Set Previous Tool or Edit Mode",
    "Hide/Show Selected Tracks",
    "Move Selected Regions to Focused Track",
    "Color Tracks by Region/Cell Color",
    "Waveform Vertical Zoom In",
    "Waveform Vertical Zoom Out",
    "Activate Next Track Alternative",
    "Activate Previous Track Alternative",
    "Rename Track Alternative",
    "Next Patch, Plug-in Setting or Sampler Instrument",
    "Previous Patch, Plug-in Setting or Sampler Instrument",
    "Cycle Through Windows",
    "Cycle Through Windows (counter-clockwise)",
    "Open in External Sample Editor",
    "Drum Replacement/Doubling",
    "Show/Hide Staff Style Window",
    "Show/Hide Selection-Based Processing",
    "Undo History…",
    "Delete Undo History",
    "Transpose Chords +1 Semitone",
    "Transpose Chords -1 Semitone",
    "Flashback Capture as Recording",
    // (handled in edit.rs by their first words; listed here with Logic's full names)
    "Select Previous Region/Cell/Event or Move Marquee End (or Marquee Point) to Previous Transient",
    "Toggle Previous Region/Cell/Event or Move Marquee Start (or Extend Marquee Selection) to Previous Transient",
    "Toggle Next Region/Cell/Event or Move Marquee Start (or Extend Marquee Selection) to Next Transient",
    "Transpose KeySignature/Chord/Region/Event +1 Semitone, Nudge Automation Up 1 Step or Move Marquee Selection Up 1 Track",
    "Transpose KeySignature/Chord/Region/Event -1 Semitone, Nudge Automation Down 1 Step or Move Marquee Selection Down 1 Track",
    "Transpose Region/Event +12 Semitone, Nudge Automation Up 10 Steps or Move Marquee Bottom Up 1 Track",
    "Transpose Region/Event -12 Semitone, Nudge Automation Down 10 Steps or Move Marquee Bottom Down 1 Track",
];

/// Logic's waveform zoom steps (the Tracks area's audio regions; 1: unzoomed).
pub const WAVE_ZOOM: [f32; 9] = [1.0, 1.4, 2.0, 2.8, 4.0, 5.6, 8.0, 11.3, 16.0];

#[derive(Clone, Debug)]
pub struct State {
    /// Waveform Vertical Zoom: how much the Tracks area's waveforms are magnified.
    pub wave_zoom: f32,
    /// The Undo History window is open.
    pub history: bool,
}

impl Default for State {
    fn default() -> Self {
        State { wave_zoom: 1.0, history: false }
    }
}

/// Logic's other names for commands Studio has under another name.
fn alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "Drum Replacement/Doubling" => "Replace or Double Drum Track…",
        "Show/Hide Staff Style Window" => "Open Staff Style Window",
        // (Studio's capture keeps the MIDI played; audio isn't kept yet)
        "Flashback Capture as Recording" => "Capture as Recording",
        _ => return None,
    })
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if let Some(a) = alias(name) {
        return app.key_command(ctx, a);
    }
    match name {
        "Undo" => app.undo(),
        "Redo" => app.redo(),
        "Set Next Tool or Edit Mode" | "Set Previous Tool or Edit Mode" => {
            let list: &[Tool] = if Tool::TRACKS.contains(&app.tool) { &Tool::TRACKS } else { &Tool::PIANO_ROLL };
            let at = list.iter().position(|&t| t == app.tool).unwrap_or(0);
            let n = list.len();
            let next = if name.contains("Next") { (at + 1) % n } else { (at + n - 1) % n };
            app.tool = list[next];
            app.tell(list[next].name());
        }
        "Hide/Show Selected Tracks" => return hide_show_selected(app),
        "Move Selected Regions to Focused Track" => return move_to_focused(app),
        "Color Tracks by Region/Cell Color" => return color_tracks_by_regions(app),
        "Waveform Vertical Zoom In" | "Waveform Vertical Zoom Out" => {
            app.keyx.wave_zoom = step_zoom(app.keyx.wave_zoom, name.ends_with("In"));
            ctx.request_repaint();
        }
        "Rename Track Alternative" => {
            // (the name field over the track header's alternatives pop-up, as its Rename item)
            let Some(t) = app.selected_track else { return false };
            let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return false };
            let label = if tr.extra.alt_name.is_empty() { "A".to_string() } else { tr.extra.alt_name.clone() };
            app.ta.show_alternatives = true;
            app.ta.renaming_alt = Some((t, label));
        }
        "Activate Next Track Alternative" | "Activate Previous Track Alternative" => return step_alternative(app, name.contains("Next")),
        "Next Patch, Plug-in Setting or Sampler Instrument" | "Previous Patch, Plug-in Setting or Sampler Instrument" => {
            let up = name.starts_with("Next");
            // the topmost plug-in window: its settings; an open Sampler: its instruments
            if let Some((s, k)) = app.mix.fx_window {
                crate::pluginwin::step_setting(app, s, k, if up { 1 } else { -1 });
                return true;
            }
            if crate::instwin::command(app, ctx, if up { "Next Plug-in Setting or Sampler Instrument" } else { "Previous Plug-in Setting or Sampler Instrument" }) {
                return true;
            }
            return step_patch(app, up);
        }
        "Cycle Through Windows" | "Cycle Through Windows (counter-clockwise)" => return cycle_windows(ctx, !name.contains("counter")),
        "Open in External Sample Editor" => return external_editor(app),
        "Show/Hide Selection-Based Processing" => app.sbp.open = !app.sbp.open,
        "Undo History…" => app.keyx.history = !app.keyx.history,
        "Delete Undo History" => {
            app.undo.clear();
            app.redo.clear();
        }
        "Transpose Chords +1 Semitone" | "Transpose Chords -1 Semitone" => {
            if app.chord_text.trim().is_empty() {
                return false;
            }
            app.chord_text = transpose_chords(&app.chord_text, if name.contains('+') { 1 } else { -1 });
            app.dirty = true;
        }
        _ => return false,
    }
    true
}

/// The next waveform zoom step up or down.
pub fn step_zoom(z: f32, up: bool) -> f32 {
    if up {
        WAVE_ZOOM.iter().copied().find(|&s| s > z + 0.01).unwrap_or(z)
    } else {
        WAVE_ZOOM.iter().rev().copied().find(|&s| s < z - 0.01).unwrap_or(1.0)
    }
}

/// The selected tracks (the focused one when none is selected).
fn selected_tracks(app: &App) -> Vec<usize> {
    let mut v = app.ta.selected.clone();
    if v.is_empty() {
        v.extend(app.selected_track);
    }
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    v.retain(|&t| t < n);
    v
}

/// Logic's ⌃H: the selected tracks' Hide buttons on (or, when all are on, off). Hidden tracks
/// disappear while Hide View is on, as the H buttons do.
fn hide_show_selected(app: &mut App) -> bool {
    let sel = selected_tracks(app);
    let Some(p) = app.project.as_mut() else { return false };
    if sel.is_empty() {
        return false;
    }
    let hide = !sel.iter().all(|&t| p.tracks[t].extra.hidden);
    for &t in &sel {
        p.tracks[t].extra.hidden = hide;
    }
    app.dirty = true;
    true
}

/// Moves the selected regions onto the focused (selected) track, keeping their positions.
fn move_to_focused(app: &mut App) -> bool {
    let Some(to) = app.selected_track else { return false };
    let mut sel = app.region_selection();
    sel.retain(|&(t, _)| t != to);
    if sel.is_empty() || app.project.as_ref().is_none_or(|p| to >= p.tracks.len()) {
        return false;
    }
    app.checkpoint("Move Selected Regions to Focused Track");
    // (from the highest index down, so the others keep theirs)
    sel.sort_by(|a, b| b.cmp(a));
    let mut moved = Vec::new();
    if let Some(p) = app.project.as_mut() {
        for &(t, r) in &sel {
            if r < p.tracks[t].regions.len() {
                moved.push(p.tracks[t].regions.remove(r));
            }
        }
        moved.reverse();
        let base = p.tracks[to].regions.len();
        let n = moved.len();
        p.tracks[to].regions.extend(moved);
        app.selected_regions = (base..base + n).map(|r| (to, r)).collect();
        app.selected_region = app.selected_regions.first().copied();
    }
    app.after_edit();
    true
}

/// Each track with selected regions takes the colour of its first selected region that has one.
fn color_tracks_by_regions(app: &mut App) -> bool {
    let sel = app.region_selection();
    let mut colors: Vec<(usize, u8)> = Vec::new();
    for &(t, r) in &sel {
        let c = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)?.props.color);
        if let Some(c) = c {
            if !colors.iter().any(|x| x.0 == t) {
                colors.push((t, c));
            }
        }
    }
    if colors.is_empty() {
        return false;
    }
    app.checkpoint("Color Tracks by Region Color");
    for (t, c) in colors {
        if let Some(s) = crate::tm::set_mut(app, t) {
            s.strip.color = Some(c);
        }
    }
    app.after_edit();
    true
}

/// The track alternatives' names in order (Logic's A, B, C…), the active one's index among them.
pub fn alternative_order(t: &lg_core::Track) -> (Vec<String>, usize) {
    let mut names: Vec<String> = t.extra.alternatives.iter().map(|a| a.name.clone()).collect();
    names.push(t.extra.alt_name.clone());
    names.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
    let at = names.iter().position(|n| *n == t.extra.alt_name).unwrap_or(0);
    (names, at)
}

/// Activates the next (or previous) alternative of the selected track, wrapping around.
fn step_alternative(app: &mut App, next: bool) -> bool {
    let Some(t) = app.selected_track else { return false };
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return false };
    if tr.extra.alternatives.is_empty() {
        return false;
    }
    let (names, at) = alternative_order(tr);
    let n = names.len();
    let want = &names[if next { (at + 1) % n } else { (at + n - 1) % n }];
    let Some(k) = tr.extra.alternatives.iter().position(|a| a.name == *want) else { return false };
    app.checkpoint("Switch Track Alternative");
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        lg_core::arrange::switch_alternative(tr, k);
    }
    app.selected_region = None;
    app.selected_regions.clear();
    app.after_edit();
    true
}

/// The selected track's next (or previous) patch in the Library's list of its category.
fn step_patch(app: &mut App, up: bool) -> bool {
    let Some(t) = app.selected_track else { return false };
    let all = crate::browser::patches();
    let cur = app.instrument_choice.get(&t).cloned().or_else(|| app.project.as_ref()?.tracks.get(t).map(|tr| tr.name.clone()));
    let Some(cur) = cur else { return false };
    let Some(i) = all.iter().position(|p| p.2 == cur) else { return false };
    let cat = all[i].0.clone();
    let list: Vec<usize> = (0..all.len()).filter(|&k| all[k].0 == cat).collect();
    let at = list.iter().position(|&k| k == i).unwrap_or(0);
    let n = list.len();
    let k = list[if up { (at + 1) % n } else { (at + n - 1) % n }];
    let name = all[k].2.clone();
    app.choose_patch(t, &cat, &name);
    app.tell(format!("Patch: {name}"));
    true
}

/// Brings the bottom floating window to the front (clockwise), or sends the front one to the
/// back (counter-clockwise).
fn cycle_windows(ctx: &egui::Context, clockwise: bool) -> bool {
    let wins: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().filter(|l| l.order == egui::Order::Middle && m.areas().is_visible(l)).collect());
    if wins.len() < 2 {
        return false;
    }
    if clockwise {
        ctx.move_to_top(wins[0]);
    } else {
        for &l in &wins[..wins.len() - 1] {
            ctx.move_to_top(l);
        }
    }
    true
}

/// Opens the selected audio region's file in Settings ▸ Audio ▸ File Editor's External Sample
/// Editor (the desktop's editor for the file when none is chosen).
fn external_editor(app: &mut App) -> bool {
    let file = app.region_selection().into_iter().find_map(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.audio.as_ref().map(|a| a.file.clone()));
    let Some(f) = file else {
        app.tell("Select an audio region");
        return true;
    };
    let ext = app.logic_settings.get("Audio / File Editor / External Sample Editor:");
    let prog = if ext.trim().is_empty() { "xdg-open".to_string() } else { ext.trim().to_string() };
    match std::process::Command::new(&prog).arg(&f).stdin(std::process::Stdio::null()).spawn() {
        Ok(_) => app.tell(format!("Opened {} in {prog}", f.display())),
        Err(e) => app.tell(format!("Couldn't open an editor ({prog}): {e}")),
    }
    true
}

/// Every chord of a Chord track text (one per bar, spaces between) moved by `by` semitones,
/// its bass note too; words that aren't chords stay.
pub fn transpose_chords(text: &str, by: i32) -> String {
    text.split(' ')
        .map(|w| match lg_players::chords::Chord::parse(w) {
            Some(mut c) if !w.is_empty() => {
                c.root = ((c.root as i32 + by).rem_euclid(12)) as u8;
                c.bass = c.bass.map(|b| ((b as i32 + by).rem_euclid(12)) as u8);
                c.name()
            }
            _ => w.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The Undo History's rows in time order (the oldest first): the steps done, then the steps
/// undone (dimmed); the index of the latest step done.
pub fn history_rows(app: &App) -> (Vec<(String, bool)>, Option<usize>) {
    let mut rows: Vec<(String, bool)> = app.undo.iter().map(|u| (u.0.clone(), true)).collect();
    let cur = rows.len().checked_sub(1);
    rows.extend(app.redo.iter().rev().map(|u| (u.0.clone(), false)));
    (rows, cur)
}

/// Makes row `i` the latest step done: undoes the steps after it, or redoes the dimmed steps up
/// to it.
pub fn history_go(app: &mut App, i: usize) {
    let n = app.undo.len();
    if i + 1 < n {
        for _ in 0..n - 1 - i {
            app.undo();
        }
    } else if i >= n {
        for _ in 0..=i - n {
            app.redo();
        }
    }
}

/// Logic's Undo History window (Edit ▸ Undo History…, ⌃⌥⌘Z): every undoable edit in time order,
/// the latest done highlighted, undone steps dimmed below it; a click goes back (or forward) to
/// that step; Undo and Redo buttons step once.
pub fn history_window(app: &mut App, ctx: &egui::Context) {
    use crate::look;
    use egui::{pos2, vec2, Align2, Color32, Sense};
    if !app.keyx.history {
        return;
    }
    let mut open = true;
    let (rows, cur) = history_rows(app);
    let mut go = None;
    let mut step = 0i32;
    egui::Window::new("Undo History").id(egui::Id::unique("undo-history")).open(&mut open).default_size(vec2(300.0, 320.0)).resizable(true).collapsible(false).show(ctx, |ui| {
        let list_h = (ui.available_height() - 34.0).max(80.0);
        egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            if rows.is_empty() {
                ui.label(egui::RichText::new("No edits to undo").color(look::text_dim()));
            }
            for (i, (what, done)) in rows.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click());
                let p = ui.painter();
                let on = Some(i) == cur;
                if on {
                    p.rect_filled(r, 3.0, look::accent());
                } else if resp.hovered() {
                    p.rect_filled(r, 3.0, look::accent().gamma_multiply(0.25));
                }
                let col = if on { Color32::WHITE } else if *done { ui.visuals().text_color() } else { look::text_dim().gamma_multiply(0.7) };
                p.text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, what, look::regular(12.0), col);
                if resp.clicked() {
                    go = Some(i);
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let w = 64.0;
            ui.add_space((ui.available_width() - 2.0 * w - 8.0).max(0.0));
            if ui.add_enabled(cur.is_some(), egui::Button::new("Undo").min_size(vec2(w, 20.0))).clicked() {
                step = -1;
            }
            if ui.add_enabled(!app.redo.is_empty(), egui::Button::new("Redo").min_size(vec2(w, 20.0))).clicked() {
                step = 1;
            }
        });
    });
    if let Some(i) = go {
        history_go(app, i);
    }
    match step {
        -1 => app.undo(),
        1 => app.redo(),
        _ => {}
    }
    if !open {
        app.keyx.history = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps() {
        assert_eq!(step_zoom(1.0, true), 1.4);
        assert_eq!(step_zoom(16.0, true), 16.0);
        assert_eq!(step_zoom(1.4, false), 1.0);
        assert_eq!(step_zoom(1.0, false), 1.0);
    }

    #[test]
    fn alternatives_order_by_letter() {
        let mut t = lg_core::Track::new("A", lg_core::TrackKind::Instrument);
        lg_core::arrange::new_alternative(&mut t, false);
        lg_core::arrange::new_alternative(&mut t, false);
        // active C, inactive A and B
        let (names, at) = alternative_order(&t);
        assert_eq!(names, vec!["A", "B", "C"]);
        assert_eq!(names[at], t.extra.alt_name);
    }

    #[test]
    fn chords_transpose() {
        assert_eq!(transpose_chords("C Am F/A G7", 1), "C# Bbm F#/Bb Ab7");
        assert_eq!(transpose_chords("C x", -1), "B x");
    }

    #[test]
    fn names_are_logics() {
        let all = crate::keymap::all_commands();
        for n in COMMANDS {
            assert!(all.iter().any(|c| c.1 == *n), "not a Logic key command: {n:?}");
        }
    }
}
