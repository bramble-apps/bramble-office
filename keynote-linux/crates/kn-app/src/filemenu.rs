//! File > Duplicate, Rename…, Move To….

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use crate::app::App;

#[derive(Default)]
pub struct FileOps {
    /// Rename…: the name being typed.
    rename: Option<String>,
    focus: bool,
    /// Move To…: folder picked in the background.
    move_to: Option<Receiver<Option<PathBuf>>>,
    /// Name shown for an untitled copy (File > Duplicate).
    pub untitled_name: Option<String>,
}

fn stem(app: &App) -> String {
    app.doc
        .as_ref()
        .and_then(|d| d.path.as_ref())
        .and_then(|p| p.file_stem())
        .map(|n| n.to_string_lossy().to_string())
        .or_else(|| app.fileops.untitled_name.clone())
        .unwrap_or_else(|| "Untitled".into())
}

/// File > Duplicate: the presentation as it is now (unsaved edits too) opens as an untitled
/// copy in a new window.
pub fn duplicate(app: &mut App) {
    crate::textedit::commit(app);
    app.commit_notes();
    let name = format!("{} copy", stem(app));
    let Some(dir) = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/keydeck/duplicates")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join(format!("{}-{}.key", name, std::process::id()));
    let Some(doc) = app.doc.as_mut() else { return };
    doc.end(&mut app.hist);
    let path = doc.path.clone();
    let dirty = doc.dirty;
    doc.declare_external_refs();
    let r = crate::textures::guarded("duplicate", || doc.file.save(&tmp));
    // writing the copy must not change this document's file or state
    doc.path = path;
    doc.dirty = dirty;
    match r {
        Some(Ok(())) => {
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("keydeck"));
            let mut cmd = std::process::Command::new(exe);
            cmd.arg(&tmp).env("KN_UNTITLED", &name).stdin(std::process::Stdio::null());
            // never pass on automated-check settings (KN_CMD=Duplicate would make the copy duplicate itself)
            for (k, _) in std::env::vars_os() {
                if k.to_string_lossy().starts_with("KN_") {
                    cmd.env_remove(k);
                }
            }
            cmd.env("KN_UNTITLED", &name);
            match cmd.spawn() {
                Ok(_) => app.notify(format!("Opened “{name}”")),
                Err(e) => app.notify(format!("Couldn't open the copy: {e}")),
            }
        }
        Some(Err(e)) => app.notify(format!("Couldn't duplicate: {e}")),
        None => app.notify("Couldn't duplicate: internal error"),
    }
}

/// Window > Move Tab to New Window: the active tab opens in a window of its own (a new process,
/// with unsaved changes carried over through a temporary copy) and leaves this one.
pub fn tab_to_window(app: &mut App, ctx: &egui::Context) {
    if app.tabs.len() < 2 {
        return;
    }
    crate::textedit::commit(app);
    app.commit_notes();
    let Some(doc) = app.doc.as_mut() else { return };
    doc.end(&mut app.hist);
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("keydeck")));
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("KN_") {
            cmd.env_remove(k);
        }
    }
    match doc.path.clone().filter(|p| !doc.dirty && p.is_file()) {
        Some(p) => {
            cmd.arg(p);
        }
        None => {
            let name = app.fileops.untitled_name.clone().or_else(|| doc.path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "Untitled".into());
            let Some(dir) = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/keydeck/duplicates")) else { return };
            let _ = std::fs::create_dir_all(&dir);
            let tmp = dir.join(format!("{}-{}.key", name, std::process::id()));
            let (path, dirty) = (doc.path.clone(), doc.dirty);
            doc.declare_external_refs();
            let r = crate::textures::guarded("move tab", || doc.file.save(&tmp));
            doc.path = path;
            doc.dirty = dirty;
            if !matches!(r, Some(Ok(()))) {
                app.notify("Couldn't move the tab to a new window");
                return;
            }
            cmd.arg(&tmp).env("KN_UNTITLED", &name);
        }
    }
    match cmd.stdin(std::process::Stdio::null()).spawn() {
        Ok(_) => {
            // the other window has it now: close here without asking
            if let Some(d) = app.doc.as_mut() {
                d.dirty = false;
            }
            let i = app.active_tab;
            app.close_tab(ctx, i);
        }
        Err(e) => app.notify(format!("Couldn't open a new window: {e}")),
    }
}

pub fn start_rename(app: &mut App) {
    if app.doc.as_ref().and_then(|d| d.path.as_ref()).is_none() {
        app.pick_save();
        return;
    }
    app.fileops.rename = Some(stem(app));
    app.fileops.focus = true;
}

pub fn start_move(app: &mut App) {
    if app.doc.as_ref().and_then(|d| d.path.as_ref()).is_none() {
        app.pick_save();
        return;
    }
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(kn_ui::files::FileDialog::new().set_title("Move To").pick_folder());
    });
    app.fileops.move_to = Some(rx);
}

pub(crate) fn relocate(app: &mut App, ctx: &egui::Context, to: PathBuf) {
    let Some(from) = app.doc.as_ref().and_then(|d| d.path.clone()) else { return };
    if to == from {
        return;
    }
    if to.exists() {
        app.notify(format!("“{}” already exists", to.display()));
        return;
    }
    // rename(2) fails across file systems: copy then remove
    let r = std::fs::rename(&from, &to).or_else(|_| std::fs::copy(&from, &to).and_then(|_| std::fs::remove_file(&from)));
    match r {
        Ok(()) => {
            if let Some(d) = app.doc.as_mut() {
                d.path = Some(to.clone());
            }
            if app.pptx_hybrid.remove(&from) {
                app.pptx_hybrid.insert(to.clone());
            }
            app.remember_path(&to);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — Keydeck", to.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
            app.notify(format!("Now at {}", to.display()));
        }
        Err(e) => app.notify(format!("Couldn't move the file: {e}")),
    }
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    if let Some(rx) = &app.fileops.move_to {
        match rx.try_recv() {
            Ok(Some(dir)) => {
                app.fileops.move_to = None;
                let name = app.doc.as_ref().and_then(|d| d.path.as_ref()).and_then(|p| p.file_name()).map(|n| n.to_os_string());
                if let Some(n) = name {
                    relocate(app, ctx, dir.join(n));
                }
            }
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => app.fileops.move_to = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }
    let Some(mut buf) = app.fileops.rename.take() else { return };
    let mut open = true;
    let mut done = false;
    egui::Window::new("Rename").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(280.0));
        if std::mem::take(&mut app.fileops.focus) {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            done = ui.button("Rename").clicked() || enter;
        });
    });
    // the file keeps its kind (.key / .pptx)
    let ext = app.doc.as_ref().and_then(|d| d.path.as_ref()).and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "key".into());
    let name = buf.trim().trim_end_matches(&format!(".{ext}")).to_string();
    if done && !name.is_empty() && !name.contains('/') {
        if let Some(dir) = app.doc.as_ref().and_then(|d| d.path.as_ref()).and_then(|p| p.parent()).map(|p| p.to_path_buf()) {
            relocate(app, ctx, dir.join(format!("{name}.{ext}")));
        }
    } else if open && !done {
        app.fileops.rename = Some(buf);
    }
}
