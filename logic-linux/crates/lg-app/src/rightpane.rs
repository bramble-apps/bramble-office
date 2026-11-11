//! The panes on the window's right, as Logic's:
//! - List Editors (D): Events (the selected region's notes: position, note, channel, velocity,
//!   length, each editable by dragging or typing), Markers, Tempo, Signature.
//! - Notes (⌥⌘P): the project's and the track's notes.
//! - Loop Browser (O): the audio loops in the usual folders (yours, and Apple's when installed);
//!   a click previews, double-click adds it to the selected track at the playhead.
//! - Browsers (F), All Files: audio and MIDI files from your folders; double-click imports.

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};

use crate::app::{App, RightPane};
use crate::look;
use crate::widgets;

const ROW: f32 = 22.0;

/// Tabs along a pane's top, Logic's segmented control.
fn tabs(ui: &mut egui::Ui, r: Rect, id: &str, names: &[&str], cur: &mut usize) {
    let p = ui.painter();
    p.rect_filled(r, r.height() / 2.0, look::group_bg());
    let w = r.width() / names.len() as f32;
    for (i, n) in names.iter().enumerate() {
        let c = Rect::from_min_size(pos2(r.min.x + i as f32 * w, r.min.y), vec2(w, r.height())).shrink(2.0);
        let resp = ui.interact(c, ui.scope_id().with((id, i)), Sense::click());
        if *cur == i {
            p.rect_filled(c, c.height() / 2.0, look::accent());
        } else if resp.hovered() {
            p.rect_filled(c, c.height() / 2.0, look::button_on().gamma_multiply(0.5));
        }
        p.text(c.center(), Align2::CENTER_CENTER, *n, look::regular(12.0), if *cur == i { egui::Color32::WHITE } else { look::text() });
        if resp.clicked() {
            *cur = i;
        }
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, pane: RightPane) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.min.x, area.y_range(), Stroke::new(1.0, look::line()));
    let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(area));
    ui.set_clip_rect(area);
    match pane {
        RightPane::Lists => lists(app, ui, area),
        RightPane::Notes => notes(app, ui, area),
        RightPane::Loops => files(app, ui, area, true),
        RightPane::Files => files(app, ui, area, false),
    }
}

// ---- List Editors

fn lists(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let tr = Rect::from_min_size(area.min + vec2(10.0, 8.0), vec2(area.width() - 20.0, 24.0));
    let mut tab = app.list_tab;
    tabs(ui, tr, "list-tab", &["Events", "Markers", "Tempo", "Signature"], &mut tab);
    app.list_tab = tab;
    let body = Rect::from_min_max(pos2(area.min.x, tr.max.y + 8.0), area.max);
    match tab {
        0 => event_list(app, ui, body),
        1 => markers(app, ui, body),
        2 => tempo_list(app, ui, body),
        _ => signature_list(app, ui, body),
    }
}

fn header_row(p: &egui::Painter, r: Rect, cols: &[(&str, f32)]) {
    p.rect_filled(r, 0.0, look::header_bg());
    let mut x = r.min.x + 8.0;
    for (name, w) in cols {
        p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, *name, look::semibold(11.0), look::text_dim());
        x += w;
    }
    p.hline(r.x_range(), r.max.y, Stroke::new(1.0, look::line()));
}

/// Logic's position text: bar beat division tick.
pub fn bbdt(app: &App, t: u32) -> String {
    let bar = app.bar_ticks();
    let beats = app.project.as_ref().map_or(4, |p| p.signature.0.max(1)) as u32;
    let beat = bar / beats;
    let div = (beat / 4).max(1);
    format!("{} {} {} {}", t / bar + 1, (t % bar) / beat + 1, (t % beat) / div + 1, t % div + 1)
}

fn event_list(app: &mut App, ui: &mut egui::Ui, body: Rect) {
    let p = ui.painter().clone();
    let Some((t, r)) = app.selected_region else {
        p.text(body.center(), Align2::CENTER_CENTER, "Select a MIDI region to list its events", look::regular(12.0), look::text_dim());
        return;
    };
    let Some(region) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
    let w = body.width() - 16.0;
    // the Event Editor's view options (eventedit.rs): Relative Position, Length as Absolute Position
    let ev = crate::eventedit::state(ui.ctx());
    crate::eventedit::with(ui.ctx(), |s| s.rect = Some(body));
    let cols = [(if ev.relative { "Rel. Position" } else { "Position" }, w * 0.34), ("Status", w * 0.17), ("Ch", w * 0.09), ("Num", w * 0.14), ("Val", w * 0.11), (if ev.abs_length { "End" } else { "Length" }, w * 0.15)];
    let hr = Rect::from_min_size(body.min, vec2(body.width(), ROW));
    header_row(&p, hr, &cols);
    let list = Rect::from_min_max(pos2(body.min.x, hr.max.y + 1.0), body.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    let mut edit: Option<(usize, lg_core::events::Note)> = None;
    let selected = app.selected_notes.clone();
    egui::ScrollArea::vertical().id_salt("events").auto_shrink([false, false]).show_rows(&mut child, ROW, region.notes.len(), |ui, range| {
        for i in range {
            let n = region.notes[i];
            let (row, resp) = ui.allocate_exact_size(vec2(list.width(), ROW), Sense::click());
            let p = ui.painter();
            if selected.contains(&i) {
                p.rect_filled(row, 0.0, look::accent().gamma_multiply(0.45));
            } else if i % 2 == 1 {
                p.rect_filled(row, 0.0, look::lane_alt());
            }
            if resp.clicked() {
                if ui.input(|x| x.modifiers.shift || x.modifiers.command) {
                    if !app.selected_notes.contains(&i) {
                        app.selected_notes.push(i);
                    }
                } else {
                    app.selected_notes = vec![i];
                    app.preview_note(t, n.pitch, n.velocity);
                }
            }
            let mut x = row.min.x + 8.0;
            let cell = |x: f32, w: f32| Rect::from_min_size(pos2(x - 4.0, row.min.y + 1.0), vec2(w - 2.0, ROW - 2.0));
            let shown = if ev.relative { n.pos.saturating_sub(region.start) } else { n.pos };
            p.text(pos2(x, row.center().y), Align2::LEFT_CENTER, bbdt(app, shown), look::regular(12.0), look::text());
            x += cols[0].1;
            p.text(pos2(x, row.center().y), Align2::LEFT_CENTER, "Note", look::regular(12.0), look::text());
            x += cols[1].1;
            p.text(pos2(x, row.center().y), Align2::LEFT_CENTER, format!("{}", n.channel + 1), look::regular(12.0), look::text());
            x += cols[2].1;
            let mut pitch = n.pitch as f32;
            if widgets::value_slot(ui, cell(x, cols[3].1), ui.scope_id().with(("ev-pitch", i)), &mut pitch, 0.0, 127.0, n.pitch as f32, 1.0, &|v| crate::app::note_name(v as u8)) {
                edit = Some((i, lg_core::events::Note { pitch: pitch as u8, ..n }));
            }
            x += cols[3].1;
            let mut vel = n.velocity as f32;
            if widgets::value_slot(ui, cell(x, cols[4].1), ui.scope_id().with(("ev-vel", i)), &mut vel, 1.0, 127.0, n.velocity as f32, 1.0, &|v| format!("{v:.0}")) {
                edit = Some((i, lg_core::events::Note { velocity: vel as u8, ..n }));
            }
            x += cols[4].1;
            let mut len = n.len as f32;
            let beat = 960.0;
            if ev.abs_length {
                // Length as Absolute Position: the note's end
                ui.painter().text(pos2(x, row.center().y), Align2::LEFT_CENTER, bbdt(app, n.pos + n.len), look::regular(12.0), look::text());
            } else if widgets::value_slot(ui, cell(x, cols[5].1), ui.scope_id().with(("ev-len", i)), &mut len, 1.0, 3840.0 * 64.0, n.len as f32, 10.0, &move |v| format!("{} {}", (v / beat).floor() as u32, (v % beat) as u32)) {
                edit = Some((i, lg_core::events::Note { len: len as u32, ..n }));
            }
        }
    });
    // Numerical Edit of Event Position: the position typed
    if let Some((i, mut text)) = ev.typing.clone() {
        let mut done = None;
        egui::Area::new(egui::Id::unique("lg-ev-typing")).fixed_pos(body.min + vec2(10.0, 30.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label("Position (bar beat division tick):");
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0)).request_focus();
                if ui.input(|x| x.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|x| x.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        crate::eventedit::with(ui.ctx(), |s| s.typing = if done.is_some() { None } else { Some((i, text.clone())) });
        if done == Some(true) {
            crate::eventedit::set_position(app, i, &text);
        }
    }
    if let Some((i, n)) = edit {
        app.checkpoint("Change Event");
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            if let Some(slot) = rg.notes.get_mut(i) {
                *slot = n;
            }
        }
        app.after_edit();
    }
}

fn markers(app: &mut App, ui: &mut egui::Ui, body: Rect) {
    let p = ui.painter().clone();
    let w = body.width() - 16.0;
    let cols = [("Position", w * 0.4), ("Marker Name", w * 0.6)];
    let hr = Rect::from_min_size(body.min, vec2(body.width(), ROW));
    header_row(&p, hr, &cols);
    let mut y = hr.max.y + 1.0;
    let markers = app.markers.clone();
    for (i, (pos, name)) in markers.iter().enumerate() {
        let row = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), ROW));
        if i % 2 == 1 {
            p.rect_filled(row, 0.0, look::lane_alt());
        }
        p.text(pos2(row.min.x + 8.0, row.center().y), Align2::LEFT_CENTER, bbdt(app, *pos), look::regular(12.0), look::text());
        p.text(pos2(row.min.x + 8.0 + cols[0].1, row.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text());
        if ui.interact(row, ui.scope_id().with(("marker", i)), Sense::click()).double_clicked() {
            app.locate(*pos as f64);
        }
        y += ROW;
    }
    let add = Rect::from_min_size(pos2(body.min.x + 10.0, y + 8.0), vec2(120.0, 24.0));
    if ui.put(add, egui::Button::new("Create Marker")).clicked() {
        app.key_command(ui.ctx(), "Create Marker");
    }
}

fn tempo_list(app: &mut App, ui: &mut egui::Ui, body: Rect) {
    let p = ui.painter().clone();
    let w = body.width() - 16.0;
    let cols = [("Position", w * 0.5), ("Tempo", w * 0.5)];
    let hr = Rect::from_min_size(body.min, vec2(body.width(), ROW));
    header_row(&p, hr, &cols);
    let Some(proj) = app.project.clone() else { return };
    let mut events: Vec<(u32, f64)> = vec![(0, proj.tempo)];
    events.extend(proj.tempo_map.iter().copied().filter(|(t, _)| *t > 0));
    let mut y = hr.max.y + 1.0;
    let mut change: Option<(usize, f64)> = None;
    for (i, (pos, bpm)) in events.iter().enumerate() {
        let row = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), ROW));
        if i % 2 == 1 {
            p.rect_filled(row, 0.0, look::lane_alt());
        }
        p.text(pos2(row.min.x + 8.0, row.center().y), Align2::LEFT_CENTER, bbdt(app, *pos), look::regular(12.0), look::text());
        let mut v = *bpm as f32;
        let cell = Rect::from_min_size(pos2(row.min.x + 8.0 + cols[0].1 - 4.0, row.min.y + 1.0), vec2(cols[1].1, ROW - 2.0));
        if widgets::value_slot(ui, cell, ui.scope_id().with(("tempo-ev", i)), &mut v, 5.0, 990.0, *bpm as f32, 1.0, &|v| format!("{v:.4}")) {
            change = Some((i, v as f64));
        }
        y += ROW;
    }
    if let Some((i, v)) = change {
        app.checkpoint("Change Tempo");
        if i == 0 {
            app.set_tempo(v);
        } else if let Some(pr) = app.project.as_mut() {
            let pos = events[i].0;
            if let Some(e) = pr.tempo_map.iter_mut().find(|e| e.0 == pos) {
                e.1 = v;
            }
            app.after_edit();
        }
    }
}

fn signature_list(app: &mut App, ui: &mut egui::Ui, body: Rect) {
    let p = ui.painter().clone();
    let w = body.width() - 16.0;
    let cols = [("Position", w * 0.5), ("Value", w * 0.5)];
    let hr = Rect::from_min_size(body.min, vec2(body.width(), ROW));
    header_row(&p, hr, &cols);
    let Some(proj) = app.project.as_ref() else { return };
    let rows = [(0u32, format!("Time Signature {}/{}", proj.signature.0, proj.signature.1)), (0, format!("Key Signature {}", proj.key))];
    let mut y = hr.max.y + 1.0;
    for (pos, v) in rows {
        let row = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), ROW));
        p.text(pos2(row.min.x + 8.0, row.center().y), Align2::LEFT_CENTER, bbdt(app, pos), look::regular(12.0), look::text());
        p.text(pos2(row.min.x + 8.0 + cols[0].1, row.center().y), Align2::LEFT_CENTER, v, look::regular(12.0), look::text());
        y += ROW;
    }
    let _ = ui;
}

// ---- Notes

fn notes(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let tr = Rect::from_min_size(area.min + vec2(10.0, 8.0), vec2(area.width() - 20.0, 24.0));
    let mut tab = app.notes_tab;
    tabs(ui, tr, "notes-tab", &["Project", "Track"], &mut tab);
    app.notes_tab = tab;
    let body = Rect::from_min_max(pos2(area.min.x + 10.0, tr.max.y + 10.0), area.max - vec2(10.0, 10.0));
    let text = if tab == 0 {
        &mut app.project_notes
    } else {
        let t = app.selected_track.unwrap_or(0);
        app.track_notes.entry(t).or_default()
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
    let before = text.clone();
    egui::ScrollArea::vertical().id_salt(("notes", tab)).show(&mut child, |ui| {
        ui.add_sized(vec2(body.width(), body.height().max(200.0)), egui::TextEdit::multiline(text).font(look::regular(13.0)).frame(egui::Frame::NONE).hint_text(if tab == 0 { "Notes about this project" } else { "Notes about this track" }));
    });
    if *text != before {
        app.dirty = true;
    }
}

// ---- Loop Browser and All Files

const AUDIO_EXT: &[&str] = &["wav", "aif", "aiff", "caf", "mp3", "flac", "ogg", "m4a"];
const MIDI_EXT: &[&str] = &["mid", "midi", "smf", "kar"];

fn loop_folders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut v = vec![home.join("Music/Loops"), home.join("Music/Apple Loops"), home.join("Music/Audio Music Apps/Apple Loops"), home.join("Library/Audio/Apple Loops"), PathBuf::from("/Library/Audio/Apple Loops"), home.join("Music/Samples")];
    v.retain(|p| p.is_dir());
    v
}

fn files(app: &mut App, ui: &mut egui::Ui, area: Rect, loops: bool) {
    let p = ui.painter().clone();
    let top = Rect::from_min_size(area.min + vec2(10.0, 8.0), vec2(area.width() - 20.0, 24.0));
    // the search field
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(top));
    child.add_sized(top.size(), egui::TextEdit::singleline(&mut app.files_search).hint_text(if loops { "Search Loops" } else { "Search" }).font(look::regular(13.0)));
    let mut y = top.max.y + 8.0;
    // where we are (All Files: a folder you navigate; Loops: every loop folder found)
    let dir = if loops {
        None
    } else {
        let d = app.files_dir.clone().or_else(|| std::env::var_os("HOME").map(PathBuf::from)).unwrap_or_default();
        let bar = Rect::from_min_size(pos2(area.min.x + 10.0, y), vec2(area.width() - 20.0, 22.0));
        let up = Rect::from_min_size(bar.min, vec2(22.0, 22.0));
        if ui.put(up, egui::Button::new("‹")).on_hover_text("Enclosing folder").clicked() {
            if let Some(parent) = d.parent() {
                app.files_dir = Some(parent.to_path_buf());
            }
        }
        p.with_clip_rect(bar).text(pos2(bar.min.x + 30.0, bar.center().y), Align2::LEFT_CENTER, d.display().to_string(), look::regular(12.0), look::text_dim());
        y = bar.max.y + 6.0;
        Some(d)
    };
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    let list = Rect::from_min_max(pos2(area.min.x, y + 1.0), area.max);
    let q = app.files_search.to_lowercase();
    let entries: Vec<(PathBuf, bool)> = match &dir {
        Some(d) => list_dir(d),
        None => {
            let mut v = Vec::new();
            for f in loop_folders() {
                walk(&f, &mut v, 3);
            }
            if v.is_empty() {
                p.text(list.center(), Align2::CENTER_CENTER, "No loops found\n(put loops in ~/Music/Loops)", look::regular(12.0), look::text_dim());
            }
            v.into_iter().map(|p| (p, false)).collect()
        }
    };
    let entries: Vec<(PathBuf, bool)> = entries.into_iter().filter(|(p, _)| q.is_empty() || p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&q))).collect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    let mut open_dir = None;
    let mut add = None;
    egui::ScrollArea::vertical().id_salt(("files", loops)).auto_shrink([false, false]).show_rows(&mut child, ROW, entries.len(), |ui, range| {
        for i in range {
            let (path, is_dir) = &entries[i];
            let (row, resp) = ui.allocate_exact_size(vec2(list.width(), ROW), Sense::click());
            let pp = ui.painter();
            let sel = app.files_selected.as_ref() == Some(path);
            if sel {
                pp.rect_filled(row, 0.0, look::accent().gamma_multiply(0.45));
            } else if resp.hovered() {
                pp.rect_filled(row, 0.0, look::lane_alt());
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let glyph = if *is_dir { "▸" } else if is_midi(path) { "♪" } else { "∿" };
            pp.text(pos2(row.min.x + 12.0, row.center().y), Align2::LEFT_CENTER, glyph, look::regular(12.0), look::text_dim());
            pp.text(pos2(row.min.x + 30.0, row.center().y), Align2::LEFT_CENTER, name, look::regular(12.5), look::text());
            if resp.clicked() {
                app.files_selected = Some(path.clone());
                if !*is_dir && !is_midi(path) {
                    app.preview_file(path);
                }
            }
            if resp.double_clicked() {
                if *is_dir {
                    open_dir = Some(path.clone());
                } else {
                    add = Some(path.clone());
                }
            }
        }
    });
    if let Some(d) = open_dir {
        app.files_dir = Some(d);
        app.files_search.clear();
    }
    if let Some(f) = add {
        app.import_file(&f);
    }
}

fn is_midi(p: &Path) -> bool {
    p.extension().is_some_and(|e| MIDI_EXT.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

fn wanted(p: &Path) -> bool {
    p.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_lowercase();
        AUDIO_EXT.contains(&e.as_str()) || MIDI_EXT.contains(&e.as_str())
    })
}

fn list_dir(d: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(d) else { return Vec::new() };
    let mut v: Vec<(PathBuf, bool)> = rd.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).map(|e| (e.path(), e.path().is_dir())).filter(|(p, dir)| *dir || wanted(p)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().cmp(&b.0.file_name())));
    v
}

fn walk(d: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    let Ok(rd) = std::fs::read_dir(d) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                walk(&p, out, depth - 1);
            }
        } else if wanted(&p) && out.len() < 5000 {
            out.push(p);
        }
    }
    out.sort();
}
