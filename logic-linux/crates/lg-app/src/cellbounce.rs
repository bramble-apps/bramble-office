//! Bounce Cells in Place (Logic Pro 13: ⌃B with cells selected, or the cell menu's Bounce or
//! Join ▸ Bounce Cells in Place…): the selected audio or MIDI cells rendered offline through
//! their tracks (freeze.rs's renderer) into one new audio cell.
//!
//! The range: one pass from the cell's start to its loop end, plus one pass of the loop when the
//! start differs from the loop start; a non-looping cell from its start to its end; several cells
//! until they all reach their loop end together; Bounce Second Loop Pass adds one more loop. The
//! dialog has Logic's options (Name, Destination, Source, Include Instrument Multi-Outputs, Bypass
//! Effect Plug-ins, Bounce Second Loop Pass, Include Audio Tail in File / in Region, Include
//! Volume/Pan Automation, Normalize, Restore Defaults) and remembers them after OK.

use std::sync::Arc;

use lg_core::trackmix::Normalize;
use lg_core::TrackKind;

use crate::app::App;
use crate::liveloops::Cell;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &["Bounce Cells in Place…", "Bounce or Join > Bounce Cells in Place…"];

#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub name: String,
    /// New Track (else Selected Track).
    pub new_track: bool,
    /// 0 Leave, 1 Mute, 2 Delete.
    pub source: u8,
    pub multi_outputs: bool,
    pub bypass_fx: bool,
    pub second_pass: bool,
    pub tail_file: bool,
    pub tail_region: bool,
    pub automation: bool,
    pub normalize: Normalize,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { name: String::new(), new_track: true, source: 1, multi_outputs: false, bypass_fx: false, second_pass: false, tail_file: false, tail_region: false, automation: false, normalize: Normalize::OverloadProtection }
    }
}

/// One cell's bounce length (song ticks): start → loop end, plus a loop pass when the start
/// isn't the loop start; a one-shot from its start to its end.
pub fn cell_range(c: &Cell, second_pass: bool) -> u32 {
    let speed = if c.speed > 0.0 { c.speed } else { 1.0 };
    let st = c.start_pos();
    let cell = if !c.looped {
        c.length.saturating_sub(st)
    } else {
        let first = c.loop_end().saturating_sub(st).max(1);
        let extra = if st != c.loop_start { c.loop_len() } else { 0 } + if second_pass { c.loop_len() } else { 0 };
        first + extra
    };
    ((cell as f32 / speed) as u32).max(1)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Several cells: until they all reach their loop end together (capped at 64 bars).
pub fn range(cells: &[Cell], second_pass: bool, bar: u32) -> u32 {
    let lens: Vec<u64> = cells.iter().map(|c| cell_range(c, second_pass) as u64).collect();
    let l = lens.iter().copied().fold(1u64, |a, b| a / gcd(a, b) * b);
    l.min(bar as u64 * 64).max(lens.iter().copied().max().unwrap_or(1)) as u32
}

/// The bounce's cells: the selected cells with content.
fn sources(app: &App) -> Vec<((usize, usize), Cell)> {
    app.live_loops.selected.iter().filter_map(|k| app.live_loops.cells.get(k).map(|c| (*k, c.clone()))).collect()
}

/// Destination Selected Track is possible only with an audio track selected.
pub fn can_use_selected(app: &App) -> bool {
    app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).is_some_and(|t| t.kind == TrackKind::Audio)
}

/// Opens the dialog (with cells selected).
pub fn command(app: &mut App, name: &str) -> bool {
    if !matches!(name, "Bounce Cells in Place…" | "Bounce Regions/Cells in Place…") || !crate::cellinsp::active(app) {
        return false;
    }
    let srcs = sources(app);
    if srcs.is_empty() {
        return true;
    }
    let mut o = app.live_loops.bounce_opts.clone();
    let first = &srcs[0].1;
    let track = app.project.as_ref().and_then(|p| p.tracks.get(srcs[0].0 .0)).map(|t| t.name.clone()).unwrap_or_default();
    o.name = format!("{}_bip", if first.name.is_empty() { track } else { first.name.clone() });
    if !can_use_selected(app) {
        o.new_track = true;
    }
    app.live_loops.bounce = Some(o);
    true
}

/// Renders and places the bounce. Returns where the new cell went.
pub fn bounce(app: &mut App, o: &Opts) -> Option<(usize, usize)> {
    let srcs = sources(app);
    let (first_key, _) = srcs.first()?.clone();
    let bar = app.bar_ticks().max(1);
    let cells: Vec<Cell> = srcs.iter().map(|s| s.1.clone()).collect();
    let total = range(&cells, o.second_pass, bar);
    // the cells' notes and audio from 0 over the range
    let mut feeds: Vec<(usize, Vec<(u32, u32, u8, u8)>, Vec<(u32, Arc<Vec<f32>>)>)> = Vec::new();
    for ((t, _), c) in &srcs {
        let notes = crate::liveloops::loop_notes(c, 0, Some(total), 0, total);
        let audio = crate::liveloops::cell_audio(app, c, 0, total);
        feeds.push((*t, notes, audio));
    }
    let mut tracks: Vec<usize> = srcs.iter().map(|s| s.0 .0).collect();
    tracks.dedup();
    let tail = o.tail_file || o.tail_region;
    let what = crate::freeze::What { tracks, only: Vec::new(), from: 0, to: total, bypass_fx: o.bypass_fx, fader: o.automation, tail_secs: if tail { 8.0 } else { 0.0 }, until_silent: tail };
    let job = crate::freeze::job_with(app, &what, |pts| {
        // (the cells play instead of the tracks' regions)
        for (t, _, _) in &feeds {
            if let Some(pt) = pts.get_mut(*t) {
                pt.notes.clear();
                pt.audio.clear();
            }
        }
        for (t, notes, audio) in feeds {
            if let Some(pt) = pts.get_mut(t) {
                pt.notes.extend(notes);
                pt.notes.sort_by_key(|n| (n.0, n.2));
                pt.audio.extend(audio);
            }
        }
    });
    let mut samples = job.run();
    o.normalize.apply(&mut samples);
    let name = if o.name.trim().is_empty() { "Bounce".to_string() } else { o.name.trim().to_string() };
    let (path, _) = crate::freeze::write_bounce(app, &name, &samples)?;
    let length = if o.tail_region { crate::freeze::ticks_of(app, 0, samples.len() / 2).max(total) } else { total };
    crate::cellinsp::checkpoint(app, "Bounce Cells in Place");
    // the sources
    for (k, _) in &srcs {
        match o.source {
            1 => {
                if let Some(c) = app.live_loops.cells.get_mut(k) {
                    c.muted = true;
                }
            }
            2 => {
                app.live_loops.cells.remove(k);
            }
            _ => {}
        }
    }
    let scene = first_key.1;
    let dest_track = if o.new_track || !can_use_selected(app) {
        // a new audio track under the (first) source track; cells below move down a row
        let src_t = first_key.0;
        let p = app.project.as_mut()?;
        let src = p.tracks.get(src_t)?.clone();
        let mut nt = lg_core::Track::new(&format!("{}_bip", src.name), TrackKind::Audio);
        nt.extra.depth = src.extra.depth;
        if !o.automation {
            nt.volume_db = src.volume_db;
            nt.pan = src.pan;
        }
        let at = lg_core::arrange::group(&p.tracks, src_t).end;
        p.tracks.insert(at, nt);
        app.programs.insert(at.min(app.programs.len()), 0);
        insert_row(&mut app.live_loops, at);
        at
    } else {
        app.selected_track?
    };
    let cell = Cell { name: name.clone(), audio: Some(path), length: length.max(1), ..Default::default() };
    app.live_loops.cells.insert((dest_track, scene), cell);
    app.live_loops.selected = [(dest_track, scene)].into();
    app.live_loops.scenes = app.live_loops.scenes.max(scene + 1);
    app.after_edit();
    Some((dest_track, scene))
}

/// A track inserted at `at`: the grid's rows from there move down one.
pub fn insert_row(s: &mut crate::liveloops::State, at: usize) {
    let shift = |k: (usize, usize)| if k.0 >= at { (k.0 + 1, k.1) } else { k };
    s.cells = std::mem::take(&mut s.cells).into_iter().map(|(k, c)| (shift(k), c)).collect();
    s.selected = std::mem::take(&mut s.selected).into_iter().map(shift).collect();
    s.plays = std::mem::take(&mut s.plays).into_iter().map(|(t, p)| (if t >= at { t + 1 } else { t }, p)).collect();
}

/// The dialog (each frame).
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut o) = app.live_loops.bounce.take() else { return };
    let selected_ok = can_use_selected(app);
    let mut open = true;
    let mut done: Option<bool> = None;
    egui::Window::new("Bounce Cells in Place").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).open(&mut open).show(ctx, |ui| {
        egui::Grid::new("cell-bip").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut o.name);
            ui.end_row();
            ui.label("Destination:");
            ui.vertical(|ui| {
                ui.radio_value(&mut o.new_track, true, "New Track");
                ui.add_enabled_ui(selected_ok, |ui| {
                    ui.radio_value(&mut o.new_track, false, "Selected Track");
                });
            });
            ui.end_row();
            ui.label("Source:");
            ui.vertical(|ui| {
                ui.radio_value(&mut o.source, 0, "Leave");
                ui.radio_value(&mut o.source, 1, "Mute");
                ui.radio_value(&mut o.source, 2, "Delete");
            });
            ui.end_row();
            ui.label("");
            ui.vertical(|ui| {
                ui.checkbox(&mut o.multi_outputs, "Include Instrument Multi-Outputs");
                ui.checkbox(&mut o.bypass_fx, "Bypass Effect Plug-ins");
                ui.checkbox(&mut o.second_pass, "Bounce Second Loop Pass");
                ui.checkbox(&mut o.tail_file, "Include Audio Tail in File");
                ui.checkbox(&mut o.tail_region, "Include Audio Tail in Region");
                ui.checkbox(&mut o.automation, "Include Volume/Pan Automation");
            });
            ui.end_row();
            ui.label("Normalize:");
            let names = ["Off", "Overload Protection Only", "On"];
            let cur = match o.normalize {
                Normalize::Off => 0,
                Normalize::OverloadProtection => 1,
                Normalize::On => 2,
            };
            egui::ComboBox::from_id_salt("cell-bip-norm").selected_text(names[cur]).show_ui(ui, |ui| {
                for (k, n) in names.iter().enumerate() {
                    if ui.selectable_label(k == cur, *n).clicked() {
                        o.normalize = [Normalize::Off, Normalize::OverloadProtection, Normalize::On][k];
                    }
                }
            });
            ui.end_row();
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Restore Defaults").clicked() {
                o = Opts { name: o.name.clone(), ..Opts::default() };
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
    });
    match done {
        Some(true) => {
            app.live_loops.bounce_opts = Opts { name: String::new(), ..o.clone() };
            if bounce(app, &o).is_none() {
                app.tell("Couldn't bounce the cells");
            }
        }
        Some(false) => {}
        None if open => app.live_loops.bounce = Some(o),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounce_ranges() {
        let c = Cell { length: 7680, ..Default::default() };
        assert_eq!(cell_range(&c, false), 7680);
        assert_eq!(cell_range(&c, true), 15360);
        // start before the loop: start → loop end, then one loop pass
        let s = Cell { length: 7680, start: Some(960), loop_start: 3840, ..Default::default() };
        assert_eq!(cell_range(&s, false), (7680 - 960) + 3840);
        // one-shot: start → end
        let o = Cell { length: 7680, looped: false, start: Some(1920), ..Default::default() };
        assert_eq!(cell_range(&o, false), 5760);
        // 1 bar and 3 bars play out together after 3 bars
        let a = Cell { length: 3840, ..Default::default() };
        let b = Cell { length: 3 * 3840, ..Default::default() };
        assert_eq!(range(&[a, b], false, 3840), 3 * 3840);
    }

    #[test]
    fn rows_move_down_for_a_new_track() {
        let mut s = crate::liveloops::State::default();
        s.cells.insert((0, 0), Cell::default());
        s.cells.insert((2, 1), Cell::default());
        insert_row(&mut s, 1);
        assert!(s.cells.contains_key(&(0, 0)) && s.cells.contains_key(&(3, 1)));
    }
}
