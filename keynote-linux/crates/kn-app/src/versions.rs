//! File > Revert To > Browse All Versions (a copy of the file is kept each time it is saved
//! over), File > Save Theme…, File > Advanced > Change File Type and Language & Region.

use std::path::{Path, PathBuf};

use egui::RichText;

use crate::app::App;
use crate::theme;

#[derive(Default)]
pub struct Dialogs {
    pub versions: bool,
    pub save_theme: Option<String>,
    pub language: bool,
    pub password_prompt: Option<PasswordPrompt>,
    /// File > Set Password / Change Password: (password, verify, hint).
    pub set_password: Option<(String, String, String)>,
    /// Define as Text / Media Placeholder: (object, tag, display name, media).
    pub define_placeholder: Option<(kn_format::ObjId, String, String, bool)>,
}

/// "This presentation is password protected": the file, its hint, what is typed.
pub struct PasswordPrompt {
    path: PathBuf,
    hint: String,
    typed: String,
    show_hint: bool,
}

impl PasswordPrompt {
    pub fn new(path: PathBuf, hint: String) -> PasswordPrompt {
        PasswordPrompt { path, hint, typed: String::new(), show_hint: false }
    }
}

fn versions_dir(doc: &Path) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in doc.to_string_lossy().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    Some(base.join("keydeck").join("versions").join(format!("{h:016x}")))
}

/// Before saving over `path`: keep the old file as a version (the latest 30).
pub fn keep_version(path: &Path) {
    if !path.is_file() {
        return;
    }
    let Some(dir) = versions_dir(path) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = std::fs::copy(path, dir.join(format!("{secs}.key")));
    let mut all = list(path);
    while all.len() > 30 {
        if let Some((p, _)) = all.pop() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Saved versions, newest first: (file, seconds since 1970).
fn list(path: &Path) -> Vec<(PathBuf, u64)> {
    let Some(dir) = versions_dir(path) else { return Vec::new() };
    let mut v: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| Some((e.path(), e.path().file_stem()?.to_str()?.parse().ok()?))).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn when(secs: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let ago = now.saturating_sub(secs);
    match ago {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{} minutes ago", ago / 60),
        3600..=86_399 => format!("{} hours ago", ago / 3600),
        _ => format!("{} days ago", ago / 86_400),
    }
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    if let Some((id, mut tag, mut display, media)) = app.dialogs.define_placeholder.take() {
        let mut ok = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::unique("define-placeholder")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(theme::bold(if media { "Define as Media Placeholder" } else { "Define as Text Placeholder" }));
            egui::Grid::new("ph-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Tag:");
                ui.text_edit_singleline(&mut tag);
                ui.end_row();
                ui.label("Display name:");
                ui.text_edit_singleline(&mut display);
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ok = ui.add_enabled(!tag.trim().is_empty(), egui::Button::new("Define")).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        });
        if ok {
            let layout = app.current_slide().map(|s| s.slide);
            let mut res = None;
            if let (Some(l), Some(doc)) = (layout, app.doc.as_mut()) {
                doc.begin(&mut app.hist, "Define Placeholder");
                res = Some(doc.define_placeholder(&mut app.hist, l, id, tag.trim(), display.trim()));
                doc.end(&mut app.hist);
            }
            match res {
                Some(Ok(n)) => app.notify(format!("Placeholder defined; {n} slide(s) use this layout")),
                Some(Err(e)) => app.notify(format!("{e}")),
                None => {}
            }
            app.bump_all();
        } else if !cancel {
            app.dialogs.define_placeholder = Some((id, tag, display, media));
        }
    }
    if let Some(mut p) = app.dialogs.password_prompt.take() {
        let mut open_it = false;
        let mut cancel = false;
        let name = p.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        egui::Modal::new(egui::Id::unique("password-prompt")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(theme::bold(&format!("\u{201c}{name}\u{201d} is password protected.")));
            ui.label("Enter the password to open it.");
            let r = ui.add(egui::TextEdit::singleline(&mut p.typed).password(true).desired_width(f32::INFINITY));
            r.request_focus();
            if p.show_hint && !p.hint.is_empty() {
                ui.label(RichText::new(format!("Hint: {}", p.hint)).color(theme::TEXT_DIM()));
            }
            ui.horizontal(|ui| {
                if !p.hint.is_empty() && ui.button("Show Hint").clicked() {
                    p.show_hint = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    open_it = ui.button("Open").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    cancel = ui.button("Cancel").clicked();
                });
            });
        });
        if open_it {
            app.open_path_with(p.path.clone(), Some((p.typed.clone(), p.hint.clone())));
        } else if !cancel {
            app.dialogs.password_prompt = Some(p);
        }
    }
    if let Some((mut pw, mut verify, mut hint)) = app.dialogs.set_password.take() {
        let mut done = false;
        let mut cancel = false;
        let has = app.doc.as_ref().is_some_and(|d| d.password.is_some());
        egui::Modal::new(egui::Id::unique("set-password")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(theme::bold(if has { "Change the password" } else { "Require a password to open this presentation" }));
            egui::Grid::new("pw-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Password:");
                ui.add(egui::TextEdit::singleline(&mut pw).password(true));
                ui.end_row();
                ui.label("Verify:");
                ui.add(egui::TextEdit::singleline(&mut verify).password(true));
                ui.end_row();
                ui.label("Password Hint:");
                ui.text_edit_singleline(&mut hint);
                ui.end_row();
            });
            let mismatch = !verify.is_empty() && pw != verify;
            if mismatch {
                ui.label(RichText::new("The passwords don\u{2019}t match.").color(egui::Color32::from_rgb(0xff, 0x45, 0x3a)));
            }
            ui.horizontal(|ui| {
                if has && ui.button("Remove Password").clicked() {
                    pw.clear();
                    verify.clear();
                    done = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    done |= ui.add_enabled(!pw.is_empty() && pw == verify, egui::Button::new("Set Password")).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        });
        if done {
            if let Some(doc) = app.doc.as_mut() {
                doc.password = (!pw.is_empty()).then(|| (pw.clone(), hint.clone()));
                doc.dirty = true;
            }
            app.notify(if pw.is_empty() { "The password is removed when you save" } else { "The password is set when you save" });
        } else if !cancel {
            app.dialogs.set_password = Some((pw, verify, hint));
        }
    }
    if app.dialogs.versions {
        let mut open = true;
        let mut restore = None;
        let path = app.doc.as_ref().and_then(|d| d.path.clone());
        egui::Window::new("Versions").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).default_width(360.0).open(&mut open).show(ctx, |ui| {
            let Some(p) = &path else {
                ui.label(RichText::new("Save the presentation first.").color(theme::TEXT_DIM()));
                return;
            };
            let v = list(p);
            if v.is_empty() {
                ui.label(RichText::new("No earlier versions yet. A version is kept each time you save.").color(theme::TEXT_DIM()));
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (f, t) in v {
                    ui.horizontal(|ui| {
                        ui.label(when(t));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Restore").clicked() {
                                restore = Some(f.clone());
                            }
                        });
                    });
                }
            });
        });
        app.dialogs.versions = open;
        if let (Some(f), Some(p)) = (restore, path) {
            match kn_model::Document::open(&f) {
                Ok(mut d) => {
                    // the restored version replaces this one (saved over the file when you save)
                    d.path = Some(p);
                    d.dirty = true;
                    app.replace_document(d);
                    app.dialogs.versions = false;
                    app.notify("Restored an earlier version");
                }
                Err(e) => app.notify(format!("Couldn't open that version: {e}")),
            }
        }
    }
    if let Some(mut name) = app.dialogs.save_theme.take() {
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Save Theme").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label("Theme name:");
            let r = ui.text_edit_singleline(&mut name);
            r.request_focus();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                cancel = ui.button("Cancel").clicked();
                save = ui.button("Add to Theme Chooser").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
        });
        if save && !name.trim().is_empty() {
            save_theme(app, name.trim());
        } else if open && !cancel {
            app.dialogs.save_theme = Some(name);
        }
    }
    if app.dialogs.language {
        let mut open = true;
        let cur = app.doc.as_ref().and_then(|d| d.file.first_of("KN.DocumentArchive")).and_then(|(_, m)| m.find_super("TSA.DocumentArchive").and_then(|t| t.str("document_language")).map(str::to_string)).unwrap_or_else(|| "en".into());
        let mut pick = None;
        egui::Window::new("Language & Region").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label("Language of this presentation (spelling, quotes, number and date formats):");
            for (code, name) in [("en", "English"), ("fr", "French"), ("es", "Spanish"), ("de", "German"), ("it", "Italian"), ("pt", "Portuguese"), ("nl", "Dutch"), ("ko", "Korean"), ("ja", "Japanese"), ("zh-Hans", "Chinese (Simplified)")] {
                if ui.radio(cur == code, name).clicked() && cur != code {
                    pick = Some(code);
                }
            }
        });
        app.dialogs.language = open;
        if let Some(code) = pick {
            if let Some(doc) = app.doc.as_mut() {
                if let Some((id, _)) = doc.file.first_of("KN.DocumentArchive") {
                    doc.begin(&mut app.hist, "Language & Region");
                    doc.touch(&mut app.hist, id);
                    if let Some(t) = doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSA.DocumentArchive")) {
                        t.set_str("document_language", code);
                    }
                    doc.end(&mut app.hist);
                }
            }
        }
    }
}

/// File > Save Theme…: the presentation becomes a theme in the theme chooser (with a preview).
fn save_theme(app: &mut App, name: &str) {
    let Some(dir) = crate::themes::theme_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let clean: String = name.chars().map(|c| if "/\\\0".contains(c) { '_' } else { c }).collect();
    let path = dir.join(format!("{clean}.key"));
    let Some(doc) = app.doc.as_ref() else { return };
    if let Err(e) = doc.file.save(&path) {
        app.notify(format!("Couldn't save the theme: {e}"));
        return;
    }
    // preview: the first slide
    if let Some(sc) = doc.scene(&app.slides, 0) {
        if let Some(pm) = app.renderer.render_scene(doc, &sc, 480) {
            let _ = pm.save_png(path.with_extension("png"));
        }
    }
    app.notify(format!("Added \u{201c}{name}\u{201d} to the theme chooser"));
}

/// File > Advanced > Change File Type.
pub fn set_package(app: &mut App, package: bool) {
    let Some(doc) = app.doc.as_mut() else { return };
    if doc.package == package {
        return;
    }
    doc.package = package;
    match doc.path.clone() {
        Some(p) if p.exists() => {
            doc.end(&mut app.hist);
            match doc.save_as(&p) {
                Ok(()) => app.notify(if package { "The presentation is now a package" } else { "The presentation is now a single file" }),
                Err(e) => app.notify(format!("Couldn't change the file type: {e}")),
            }
        }
        _ => {}
    }
}
