//! Smart Tempo multitrack sets (Edit ▸ Tempo, or the region menu's Tempo ▸ Create Smart Tempo
//! Multitrack Set; Edit Smart Tempo Multitrack Set when a selected region is in one): the
//! files are mixed down (those with Contributes to Analysis ticked), the mix analysed once, and
//! the result given to every file in the set, including those left out of the mix. Edits in
//! the Smart Tempo Editor reach all the set's files. The Smart Tempo Multitrack window lists the
//! files (Delete / Backspace removes the selected row), with Analyze and Break Up Set.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use egui::vec2;
use lg_audio::tempo::BeatMap;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &["Create Smart Tempo Multitrack Set", "Edit Smart Tempo Multitrack Set"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Set {
    /// (file, contributes to the analysis)
    pub files: Vec<(PathBuf, bool)>,
}

#[derive(Default)]
pub struct State {
    pub sets: Vec<Set>,
    /// The set the window shows, and its selected row.
    pub open: Option<usize>,
    pub row: Option<usize>,
    pending: Vec<(usize, mpsc::Receiver<Option<BeatMap>>)>,
}

pub fn set_of(app: &App, file: &Path) -> Option<usize> {
    app.stm.sets.iter().position(|s| s.files.iter().any(|f| f.0 == file))
}

fn selected_files(app: &App) -> Vec<PathBuf> {
    let Some(p) = &app.project else { return Vec::new() };
    let mut v: Vec<PathBuf> = Vec::new();
    for (t, r) in app.region_selection() {
        if let Some(a) = p.tracks.get(t).and_then(|tr| tr.regions.get(r)).and_then(|rg| rg.audio.as_ref()) {
            if !v.contains(&a.file) {
                v.push(a.file.clone());
            }
        }
    }
    v
}

/// The menu's label: Edit … when a selected region's file is in a set.
pub fn menu_label(app: &App, label: &str) -> Option<String> {
    if label == "Create Smart Tempo Multitrack Set" && selected_files(app).iter().any(|f| set_of(app, f).is_some()) {
        return Some("Edit Smart Tempo Multitrack Set".into());
    }
    None
}

/// The sum of the contributing files (mono, from their starts), resampled to the first rate.
pub fn downmix(files: &[(Vec<f32>, u32)]) -> (Vec<f32>, u32) {
    let rate = files.first().map_or(48_000, |f| f.1);
    let n = files.iter().map(|f| (f.0.len() as f64 * rate as f64 / f.1.max(1) as f64) as usize).max().unwrap_or(0);
    let mut out = vec![0.0f32; n];
    for (data, r) in files {
        let k = *r as f64 / rate as f64;
        for (i, o) in out.iter_mut().enumerate() {
            if let Some(s) = data.get((i as f64 * k) as usize) {
                *o += *s;
            }
        }
    }
    let g = 1.0 / files.len().max(1) as f32;
    out.iter_mut().for_each(|s| *s *= g);
    (out, rate)
}

/// Analyze: mixes the set's contributing files down and analyses the mix (on a thread).
pub fn analyze(app: &mut App, k: usize) {
    let Some(set) = app.stm.sets.get(k) else { return };
    let mut paths: Vec<PathBuf> = set.files.iter().filter(|f| f.1).map(|f| f.0.clone()).collect();
    if paths.is_empty() {
        paths = set.files.iter().map(|f| f.0.clone()).collect();
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let files: Vec<(Vec<f32>, u32)> = paths.iter().filter_map(|p| lg_audio::AudioFile::read(p).ok().map(|a| (a.mono(), a.rate))).collect();
        let (mono, rate) = downmix(&files);
        let _ = tx.send(lg_audio::tempo::analyze(&mono, rate, false));
    });
    app.stm.pending.push((k, rx));
}

/// Gives every file of set `k` the map `m` (as analysed, or as edited).
pub fn give(app: &mut App, k: usize, m: &BeatMap, fresh: bool) {
    let Some(set) = app.stm.sets.get(k).cloned() else { return };
    for (f, _) in set.files {
        let a = app.audio_edit.st.maps.entry(f).or_insert_with(|| crate::smarttempo::Analysis { map: m.clone(), original: m.clone(), undo: Vec::new(), redo: Vec::new() });
        if fresh {
            a.original = m.clone();
            a.undo.clear();
            a.redo.clear();
        }
        a.map = m.clone();
    }
}

/// After an edit of `file`'s analysis: the rest of its set follows.
pub fn propagate(app: &mut App, file: &Path) {
    let Some(k) = set_of(app, file) else { return };
    let Some(m) = app.audio_edit.st.maps.get(file).map(|a| a.map.clone()) else { return };
    give(app, k, &m, false);
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Create Smart Tempo Multitrack Set" | "Edit Smart Tempo Multitrack Set" => {
            let files = selected_files(app);
            if let Some(k) = files.iter().find_map(|f| set_of(app, f)) {
                // Edit: the window; files selected now and not in it are added
                for f in files {
                    if set_of(app, &f).is_none() {
                        app.stm.sets[k].files.push((f, true));
                    }
                }
                app.stm.open = Some(k);
                return true;
            }
            if files.len() < 2 {
                app.tell("Select audio regions of two or more files for a Smart Tempo Multitrack Set");
                return true;
            }
            app.stm.sets.push(Set { files: files.into_iter().map(|f| (f, true)).collect() });
            let k = app.stm.sets.len() - 1;
            analyze(app, k);
            true
        }
        _ => false,
    }
}

/// Each frame: finished analyses, and the window.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let mut done = Vec::new();
    app.stm.pending.retain(|(k, rx)| match rx.try_recv() {
        Ok(m) => {
            done.push((*k, m));
            false
        }
        Err(mpsc::TryRecvError::Empty) => true,
        Err(_) => false,
    });
    for (k, m) in done {
        match m {
            Some(m) => give(app, k, &m, true),
            None => app.tell("Smart Tempo found no tempo in the multitrack mix"),
        }
    }
    if !app.stm.pending.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let Some(k) = app.stm.open else { return };
    if k >= app.stm.sets.len() {
        app.stm.open = None;
        return;
    }
    let mut open = true;
    let mut set = app.stm.sets[k].clone();
    let mut row = app.stm.row;
    let (mut run, mut breakup) = (false, false);
    let busy = app.stm.pending.iter().any(|p| p.0 == k);
    egui::Window::new("Smart Tempo Multitrack").id(egui::Id::unique("st-multitrack")).open(&mut open).resizable(false).default_width(440.0).show(ctx, |ui| {
        egui::Grid::new("stm-grid").num_columns(2).striped(true).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
            ui.label(egui::RichText::new("File").font(look::semibold(12.0)));
            ui.label(egui::RichText::new("Contributes to Analysis").font(look::semibold(12.0)));
            ui.end_row();
            for (i, (f, on)) in set.files.iter_mut().enumerate() {
                let name = f.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                if ui.add(egui::Button::selectable(row == Some(i), name).min_size(vec2(260.0, 0.0))).clicked() {
                    row = Some(i);
                }
                ui.checkbox(on, "");
                ui.end_row();
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) && !ui.ctx().egui_wants_keyboard_input() {
            if let Some(i) = row.filter(|&i| i < set.files.len()) {
                set.files.remove(i);
                row = None;
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Break Up Set").clicked() {
                breakup = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!busy, egui::Button::new(if busy { "Analyzing…" } else { "Analyze" })).clicked() {
                    run = true;
                }
            });
        });
    });
    app.stm.row = row;
    app.stm.sets[k] = set;
    if run {
        analyze(app, k);
    }
    if breakup || app.stm.sets[k].files.len() < 2 {
        app.stm.sets.remove(k);
        app.stm.open = None;
        return;
    }
    if !open {
        app.stm.open = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_sums_and_scales() {
        let (m, r) = downmix(&[(vec![1.0, 1.0, 1.0], 48_000), (vec![1.0], 48_000)]);
        assert_eq!(r, 48_000);
        assert_eq!(m, vec![1.0, 0.5, 0.5]);
        // a file at half the rate is stretched to the first one's
        let (m, _) = downmix(&[(vec![0.0; 4], 48_000), (vec![2.0, 4.0], 24_000)]);
        assert_eq!(m, vec![1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn edits_reach_the_whole_set() {
        let ctx = egui::Context::default();
        let mut app = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        let (a, b) = (PathBuf::from("/x/a.wav"), PathBuf::from("/x/b.wav"));
        app.stm.sets.push(Set { files: vec![(a.clone(), true), (b.clone(), false)] });
        let m = BeatMap::constant(100.0, 0.0, 10.0, 4);
        give(&mut app, 0, &m, true);
        assert_eq!(app.audio_edit.st.maps[&b].map, m);
        crate::smarttempo::edit(&mut app, &a, |m| m.double());
        assert_eq!(app.audio_edit.st.maps[&b].map.beats.len(), app.audio_edit.st.maps[&a].map.beats.len());
        assert_eq!(set_of(&app, &b), Some(0));
    }
}
