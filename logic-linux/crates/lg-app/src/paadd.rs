//! The Project Audio Browser's Add Audio File dialog (Audio File ▸ Add Audio File…, ⌃F, or the
//! Import Audio button), as Logic's: a file list to browse, Play (Stop while previewing), Add and
//! Add All into the list of files to add below, Remove and Remove All, and Done, which adds them
//! to the project. Double-clicking a file adds it to the list.

use std::path::{Path, PathBuf};

use egui::vec2;

use crate::app::App;
use crate::look;

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub dir: Option<PathBuf>,
    pub picked: Option<PathBuf>,
    pub to_add: Vec<PathBuf>,
    pub picked_add: Option<usize>,
}

/// A folder's subfolders and audio files (folders first, by name).
pub fn entries(d: &Path) -> Vec<(PathBuf, bool)> {
    let mut v: Vec<(PathBuf, bool)> = std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).filter_map(|p| if p.is_dir() { Some((p, true)) } else if crate::soundlib::is_audio(&p) { Some((p, false)) } else { None }).collect()).unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().map(|n| n.to_string_lossy().to_lowercase()).cmp(&b.0.file_name().map(|n| n.to_string_lossy().to_lowercase()))));
    v
}

/// Adds files to the list (each once).
pub fn add_to(list: &mut Vec<PathBuf>, files: impl IntoIterator<Item = PathBuf>) {
    for f in files {
        if !list.contains(&f) {
            list.push(f);
        }
    }
}

pub fn open(app: &mut App) {
    app.paadd.open = true;
    if app.paadd.dir.is_none() {
        app.paadd.dir = std::env::var_os("HOME").map(PathBuf::from);
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.paadd.open {
        return;
    }
    let dir = app.paadd.dir.clone().unwrap_or_else(|| PathBuf::from("/"));
    let list = entries(&dir);
    let mut open = true;
    let mut done = false;
    let mut go: Option<PathBuf> = None;
    egui::Window::new("Add Audio File").open(&mut open).collapsible(false).resizable(true).default_size(vec2(520.0, 460.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("↑").on_hover_text("Enclosing folder").clicked() {
                go = dir.parent().map(Path::to_path_buf);
            }
            ui.label(egui::RichText::new(dir.display().to_string()).color(look::text_dim()));
        });
        egui::Frame::new().stroke(egui::Stroke::new(1.0, look::line())).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("paadd-files").max_height(200.0).auto_shrink([false, false]).show(ui, |ui| {
                for (p, is_dir) in &list {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let label = if *is_dir { format!("▸ {name}") } else { name };
                    let r = ui.add(egui::Button::selectable(app.paadd.picked.as_ref() == Some(p), label));
                    if r.clicked() {
                        app.paadd.picked = Some(p.clone());
                    }
                    if r.double_clicked() {
                        if *is_dir {
                            go = Some(p.clone());
                        } else {
                            add_to(&mut app.paadd.to_add, [p.clone()]);
                        }
                    }
                }
            });
        });
        // Play / Stop, Add, Add All
        ui.horizontal(|ui| {
            let now = ui.input(|i| i.time);
            let pick = app.paadd.picked.clone().filter(|p| p.is_file());
            let playing = pick.as_ref().is_some_and(|p| app.preview.is_playing(&p.display().to_string(), now));
            if ui.add_enabled(pick.is_some(), egui::Button::new(if playing { "Stop" } else { "Play" })).clicked() {
                if let Some(p) = &pick {
                    crate::preview::toggle(app, &p.display().to_string(), p, 0.8, None, false);
                }
            }
            if ui.add_enabled(pick.is_some(), egui::Button::new("Add")).clicked() {
                add_to(&mut app.paadd.to_add, pick.clone());
            }
            if ui.button("Add All").clicked() {
                add_to(&mut app.paadd.to_add, list.iter().filter(|e| !e.1).map(|e| e.0.clone()));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(format!("Files to add: {}", app.paadd.to_add.len())).color(look::text_dim()));
        egui::ScrollArea::vertical().id_salt("paadd-list").max_height(110.0).auto_shrink([false, false]).show(ui, |ui| {
            for (k, p) in app.paadd.to_add.clone().iter().enumerate() {
                if ui.add(egui::Button::selectable(app.paadd.picked_add == Some(k), p.file_name().unwrap_or_default().to_string_lossy())).clicked() {
                    app.paadd.picked_add = Some(k);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(app.paadd.picked_add.is_some(), egui::Button::new("Remove")).clicked() {
                if let Some(k) = app.paadd.picked_add.take() {
                    if k < app.paadd.to_add.len() {
                        app.paadd.to_add.remove(k);
                    }
                }
            }
            if ui.add_enabled(!app.paadd.to_add.is_empty(), egui::Button::new("Remove All")).clicked() {
                app.paadd.to_add.clear();
                app.paadd.picked_add = None;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
        });
    });
    if let Some(d) = go {
        app.paadd.dir = Some(d);
        app.paadd.picked = None;
    }
    if done {
        let files = std::mem::take(&mut app.paadd.to_add);
        if !files.is_empty() {
            crate::projaudio::add_files(app, files);
        }
        app.paadd.open = false;
    }
    if !open {
        app.paadd.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_adds_once() {
        let d = std::env::temp_dir().join(format!("lg-paadd-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("b.wav"), crate::audio::wav_bytes(&[0.0; 8])).unwrap();
        std::fs::write(d.join("a.txt"), "x").unwrap();
        let e = entries(&d);
        assert_eq!(e.len(), 2);
        assert!(e[0].1);
        let mut l = Vec::new();
        add_to(&mut l, [d.join("b.wav"), d.join("b.wav")]);
        assert_eq!(l.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
