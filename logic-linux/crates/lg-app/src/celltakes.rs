//! Takes in Live Loops cells (Recording Mode Takes): each new cell recording adds a take; the
//! cell shows its active take (upper left) and take count (lower left); a click on the upper-left
//! corner opens the Take Folder pop-up: the takes (choosing one makes it the one the cell plays,
//! also while it plays), Rename Take…, Delete Take, Flatten, Copy Take to ▸, Move Take to ▸ and
//! Unpack Take Cell to ▸ (New Scene(s), Next Empty Cell(s) Right, New Track(s), Next Matching
//! Empty Cell(s) Below). Every edit is one undo step.

use lg_core::events::Note;

use crate::app::App;
use crate::liveloops::{Cell, State};

pub const COMMANDS: &[&str] = &[];

/// One take of a cell.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Take {
    pub name: String,
    pub notes: Vec<Note>,
    pub length: u32,
}

/// Where Copy / Move Take to and Unpack put takes.
pub const TARGETS: [&str; 4] = ["New Scene", "Next Empty Cell Right", "New Track", "Next Matching Empty Cell Below"];
pub const UNPACK: [&str; 4] = ["New Scenes", "Next Empty Cells Right", "New Tracks", "Next Matching Empty Cells Below"];

/// A new recording into `c` as a take (the cell's earlier content becomes take 1); it becomes
/// the active take.
pub fn add_take(c: &mut Cell, notes: Vec<Note>, length: u32) {
    if c.takes.is_empty() && (!c.notes.is_empty() || c.audio.is_some()) {
        c.takes.push(Take { name: "Take 1".into(), notes: c.notes.clone(), length: c.length });
    }
    let n = c.takes.len() + 1;
    c.takes.push(Take { name: format!("Take {n}"), notes, length });
    choose(c, c.takes.len() - 1);
}

/// Makes take `i` the one the cell plays.
pub fn choose(c: &mut Cell, i: usize) {
    let Some(t) = c.takes.get(i).cloned() else { return };
    c.take = i;
    c.notes = t.notes;
    c.length = t.length.max(1);
}

/// The cell holding only take `i` (no take folder).
pub fn single(c: &Cell, i: usize) -> Cell {
    let mut out = Cell { takes: Vec::new(), take: 0, ..c.clone() };
    if let Some(t) = c.takes.get(i) {
        out.notes = t.notes.clone();
        out.length = t.length.max(1);
    }
    out
}

/// Delete Take: the active take goes; one left: no more take folder.
pub fn delete(c: &mut Cell) {
    if c.takes.is_empty() {
        return;
    }
    let i = c.take.min(c.takes.len() - 1);
    c.takes.remove(i);
    if c.takes.is_empty() {
        c.notes.clear();
        return;
    }
    choose(c, i.min(c.takes.len() - 1));
    if c.takes.len() == 1 {
        c.takes.clear();
        c.take = 0;
    }
}

/// Flatten: only the active take stays.
pub fn flatten(c: &mut Cell) {
    let i = c.take;
    *c = single(c, i);
}

fn is_audio_track(app: &App, t: usize) -> Option<bool> {
    app.project.as_ref()?.tracks.get(t).map(|tr| tr.kind == lg_core::TrackKind::Audio)
}

/// The cell a take goes to from (t, sc) by `target` (an index of TARGETS); `skip` cells are
/// taken already (Unpack). A New Track target makes the track (under `t`) first.
fn place(app: &mut App, from: (usize, usize), target: usize, skip: &[(usize, usize)]) -> Option<(usize, usize)> {
    let (t, sc) = from;
    let s = &app.live_loops;
    let free = |k: (usize, usize)| !s.cells.contains_key(&k) && !skip.contains(&k);
    match target {
        0 => {
            let n = s.scenes.max(s.used_scenes()).max(skip.iter().map(|k| k.1 + 1).max().unwrap_or(0));
            Some((t, n))
        }
        1 => (sc + 1..).find(|x| free((t, *x))).map(|x| (t, x)),
        2 => {
            let n = new_track_below(app, t)?;
            Some((n, sc))
        }
        _ => {
            let kind = is_audio_track(app, t);
            let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
            (t + 1..n).find(|x| is_audio_track(app, *x) == kind && free((*x, sc))).map(|x| (x, sc))
        }
    }
}

/// A new track like `t` right under it (its sound and channel strip); the cells of the tracks
/// after it move down with them.
pub fn new_track_below(app: &mut App, t: usize) -> Option<usize> {
    let n = crate::recording::new_track_like(app, t)?;
    if n > t + 1 {
        let rows = crate::trackarea::rows(app);
        if let Some(r) = rows.iter().position(|r| r.t == t + 1) {
            crate::trackarea::move_track(app, n, r);
            shift_cells(&mut app.live_loops, t);
        }
    }
    Some(t + 1)
}

/// Tracks after `t` moved one down: their cells follow.
pub fn shift_cells(s: &mut State, t: usize) {
    let cells = std::mem::take(&mut s.cells);
    s.cells = cells.into_iter().map(|((tt, sc), c)| (if tt > t { (tt + 1, sc) } else { (tt, sc) }, c)).collect();
    s.selected = s.selected.iter().map(|&(tt, sc)| if tt > t { (tt + 1, sc) } else { (tt, sc) }).collect();
}

/// Copy (or Move) Take to: the active take into the target cell.
pub fn copy_take(app: &mut App, key: (usize, usize), target: usize, moving: bool) -> bool {
    let Some(c) = app.live_loops.cells.get(&key).cloned() else { return false };
    if c.takes.is_empty() {
        return false;
    }
    crate::cellinsp::undoable(app, if moving { "Move Take" } else { "Copy Take" }, |app| {
        let Some(to) = place(app, key, target, &[]) else { return false };
        let i = c.take;
        app.live_loops.cells.insert(to, single(&c, i));
        app.live_loops.scenes = app.live_loops.scenes.max(to.1 + 1);
        if moving {
            if let Some(src) = app.live_loops.cells.get_mut(&key) {
                delete(src);
            }
        }
        app.dirty = true;
        true
    })
}

/// Unpack Take Cell to: the active take stays in the cell, each other take goes to its own
/// scene, cell or track.
pub fn unpack(app: &mut App, key: (usize, usize), target: usize) -> bool {
    let Some(c) = app.live_loops.cells.get(&key).cloned() else { return false };
    if c.takes.len() < 2 {
        return false;
    }
    crate::cellinsp::undoable(app, "Unpack Take Cell", |app| {
        let mut done: Vec<(usize, usize)> = Vec::new();
        let mut from = key;
        for i in (0..c.takes.len()).filter(|i| *i != c.take) {
            // (New Tracks: each under the one before)
            let Some(to) = place(app, from, target, &done) else { continue };
            app.live_loops.cells.insert(to, single(&c, i));
            app.live_loops.scenes = app.live_loops.scenes.max(to.1 + 1);
            done.push(to);
            if target == 2 {
                from = (to.0, key.1);
            }
        }
        if let Some(src) = app.live_loops.cells.get_mut(&key) {
            flatten(src);
        }
        app.dirty = true;
        true
    })
}

/// The Take Folder pop-up's contents.
pub fn menu(app: &mut App, ui: &mut egui::Ui, key: (usize, usize)) {
    let Some(c) = app.live_loops.cells.get(&key).cloned() else { return };
    ui.set_min_width(220.0);
    for (i, t) in c.takes.iter().enumerate() {
        if ui.add(egui::Button::selectable(i == c.take, &t.name)).clicked() {
            crate::cellinsp::undoable(app, "Choose Take", |app| {
                if let Some(c) = app.live_loops.cells.get_mut(&key) {
                    choose(c, i);
                }
                true
            });
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Rename Take…").clicked() {
        let name = c.takes.get(c.take).map(|t| t.name.clone()).unwrap_or_default();
        app.live_loops.take_rename = Some((key, c.take, name));
        ui.close();
    }
    if ui.button("Delete Take").clicked() {
        crate::cellinsp::undoable(app, "Delete Take", |app| {
            if let Some(c) = app.live_loops.cells.get_mut(&key) {
                delete(c);
            }
            true
        });
        ui.close();
    }
    if ui.button("Flatten").clicked() {
        crate::cellinsp::undoable(app, "Flatten", |app| {
            if let Some(c) = app.live_loops.cells.get_mut(&key) {
                flatten(c);
            }
            true
        });
        ui.close();
    }
    ui.separator();
    for (title, moving) in [("Copy Take to", false), ("Move Take to", true)] {
        ui.menu_button(title, |ui| {
            for (k, t) in TARGETS.iter().enumerate() {
                if ui.button(*t).clicked() {
                    copy_take(app, key, k, moving);
                    ui.close();
                }
            }
        });
    }
    ui.menu_button("Unpack Take Cell to", |ui| {
        for (k, t) in UNPACK.iter().enumerate() {
            if ui.button(*t).clicked() {
                unpack(app, key, k);
                ui.close();
            }
        }
    });
}

/// The Rename Take dialog (OK / Cancel).
pub fn dialog(app: &mut App, ctx: &egui::Context) {
    let Some((key, i, mut text)) = app.live_loops.take_rename.clone() else { return };
    let mut done: Option<bool> = None;
    let mut open = true;
    egui::Window::new("Rename Take").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -80.0)).open(&mut open).show(ctx, |ui| {
        let te = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() && ui.input(|x| x.key_pressed(egui::Key::Enter)) {
            done = Some(true);
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("OK").clicked() {
                done = Some(true);
            }
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
        });
    });
    if !open {
        done = Some(false);
    }
    match done {
        Some(true) => {
            app.live_loops.take_rename = None;
            rename(app, key, i, text.trim());
        }
        Some(false) => app.live_loops.take_rename = None,
        None => app.live_loops.take_rename = Some((key, i, text)),
    }
}

pub fn rename(app: &mut App, key: (usize, usize), i: usize, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    crate::cellinsp::undoable(app, "Rename Take", |app| {
        let Some(t) = app.live_loops.cells.get_mut(&key).and_then(|c| c.takes.get_mut(i)) else { return false };
        t.name = name.to_string();
        app.dirty = true;
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(pos: u32, pitch: u8) -> Note {
        Note { channel: 0, pitch, velocity: 100, pos, len: 100, muted: false }
    }

    #[test]
    fn takes_choose_delete_flatten() {
        let mut c = Cell { notes: vec![n(0, 60)], length: 3840, ..Default::default() };
        add_take(&mut c, vec![n(0, 62)], 3840);
        add_take(&mut c, vec![n(0, 64), n(960, 65)], 7680);
        assert_eq!(c.takes.len(), 3);
        assert_eq!((c.take, c.notes.len(), c.length), (2, 2, 7680));
        choose(&mut c, 0);
        assert_eq!(c.notes[0].pitch, 60);
        delete(&mut c);
        assert_eq!(c.takes.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["Take 2", "Take 3"]);
        assert_eq!(c.notes[0].pitch, 62);
        delete(&mut c);
        assert!(c.takes.is_empty());
        assert_eq!(c.notes[0].pitch, 64);
        add_take(&mut c, vec![n(0, 70)], 3840);
        choose(&mut c, 0);
        flatten(&mut c);
        assert!(c.takes.is_empty() && c.notes[0].pitch == 64);
    }

    #[test]
    fn copy_move_and_unpack() {
        let mut app = crate::tm::test_app();
        crate::liveloops::sync(&mut app);
        let mut c = Cell { notes: vec![n(0, 60)], length: 3840, ..Default::default() };
        add_take(&mut c, vec![n(0, 62)], 3840);
        add_take(&mut c, vec![n(0, 64)], 3840);
        app.live_loops.cells.insert((0, 0), c.clone());
        app.live_loops.cells.insert((0, 1), Cell::default());
        // Next Empty Cell Right skips the full one
        assert!(copy_take(&mut app, (0, 0), 1, false));
        assert_eq!(app.live_loops.cells[&(0, 2)].notes[0].pitch, 64);
        assert!(app.live_loops.cells[&(0, 2)].takes.is_empty());
        // New Scene: after the last
        assert!(copy_take(&mut app, (0, 0), 0, true));
        assert_eq!(app.live_loops.cells[&(0, 3)].notes[0].pitch, 64);
        assert_eq!(app.live_loops.cells[&(0, 0)].takes.len(), 2);
        // Unpack to Next Empty Cells Right: the other take beside it, the cell flattened
        app.live_loops.cells.insert((0, 0), c.clone());
        assert!(unpack(&mut app, (0, 0), 1));
        assert!(app.live_loops.cells[&(0, 0)].takes.is_empty());
        assert_eq!(app.live_loops.cells[&(0, 4)].notes[0].pitch, 60);
        assert_eq!(app.live_loops.cells[&(0, 5)].notes[0].pitch, 62);
        // New Track: a track right under it, the cells below moved down
        app.live_loops.cells.insert((1, 0), Cell::default());
        let before = app.project.as_ref().unwrap().tracks.len();
        app.live_loops.cells.insert((0, 0), c.clone());
        assert!(copy_take(&mut app, (0, 0), 2, false));
        assert_eq!(app.project.as_ref().unwrap().tracks.len(), before + 1);
        assert_eq!(app.live_loops.cells[&(1, 0)].notes[0].pitch, 64);
        assert!(app.live_loops.cells.contains_key(&(2, 0)));
        // rename
        assert!(rename(&mut app, (0, 0), 1, "Best"));
        assert_eq!(app.live_loops.cells[&(0, 0)].takes[1].name, "Best");
    }
}
