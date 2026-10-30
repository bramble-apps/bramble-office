//! File menu extras: Revert To > Browse All Versions (the old file is kept each time it is
//! saved over, as Keydeck does), Rename / Move, Duplicate, a new window, Export To…, Print.

use std::path::{Path, PathBuf};

use crate::app::App;

fn versions_dir(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).hash(&mut h);
    Some(base.join("letters").join("versions").join(format!("{:016x}", h.finish())))
}

/// Before saving over `path`: keep the old file as a version (the latest 30).
pub fn keep_version(path: &Path) {
    if !path.is_file() {
        return;
    }
    let Some(dir) = versions_dir(path) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = std::fs::copy(path, dir.join(format!("{secs}.pages")));
    let mut all = list(path);
    while all.len() > 30 {
        if let Some((p, _)) = all.pop() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Kept versions, newest first: (file, seconds since 1970).
pub fn list(path: &Path) -> Vec<(PathBuf, u64)> {
    let Some(dir) = versions_dir(path) else { return Vec::new() };
    let mut v: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| Some((e.path(), e.path().file_stem()?.to_str()?.parse().ok()?))).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn when(secs: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    match now.saturating_sub(secs) {
        a @ 0..=59 => if a < 10 { "Just now".into() } else { format!("{a} seconds ago") },
        a @ 60..=3599 => format!("{} minutes ago", a / 60),
        a @ 3600..=86_399 => format!("{} hours ago", a / 3600),
        a => format!("{} days ago", a / 86_400),
    }
}

/// Revert To > Browse All Versions: the kept versions, newest first; Restore puts one back
/// (the file as it is now is kept as a version first).
pub fn versions_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_versions {
        return;
    }
    let Some(path) = app.doc().and_then(|d| d.path.clone()) else {
        app.show_versions = false;
        return;
    };
    let mut open = true;
    let mut restore: Option<PathBuf> = None;
    egui::Window::new("All Versions").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut open).show(ctx, |ui| {
        ui.set_width(340.0);
        let all = list(&path);
        if all.is_empty() {
            ui.label("No earlier versions yet: a version is kept each time the document is saved over.");
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (p, secs) in all {
                ui.horizontal(|ui| {
                    ui.label(when(secs));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Restore").clicked() {
                            restore = Some(p.clone());
                        }
                    });
                });
            }
        });
    });
    app.show_versions = open;
    if let Some(v) = restore {
        keep_version(&path);
        match std::fs::copy(&v, &path) {
            Ok(_) => {
                app.open(&path);
                app.show_versions = false;
                app.notify("Restored the earlier version");
            }
            Err(e) => app.notify(format!("Couldn't restore: {e}")),
        }
    }
}

/// The document saved as `new` and the file it was in removed (rename or move); a document
/// never saved is just saved there.
pub fn relocate(app: &mut App, new: PathBuf) {
    let old = app.doc().and_then(|d| d.path.clone());
    if old.as_ref() == Some(&new) {
        return;
    }
    if new.exists() {
        app.notify(format!("“{}” is already there; choose another name.", new.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        return;
    }
    app.save_to(&new);
    if app.doc().and_then(|d| d.path.clone()).as_ref() == Some(&new) {
        if let Some(old) = old.filter(|o| o.exists() && *o != new) {
            let _ = if old.is_dir() { std::fs::remove_dir_all(&old) } else { std::fs::remove_file(&old) };
        }
    }
}

/// Another Letters window (File > New with a document open, Duplicate). The child gets none
/// of the automated-check variables (no runaway windows).
fn spawn(args: &[&std::ffi::OsStr]) {
    if std::env::var_os("CS_SNAP").is_some() || std::env::var_os("CS_SCRIPT").is_some() {
        return;
    }
    let Ok(mut exe) = std::env::current_exe() else { return };
    // the binary was rebuilt / updated while this one runs: Linux names the old one
    // "… (deleted)"; start the file now at that path
    if let Some(live) = exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")).map(std::path::PathBuf::from) {
        exe = live;
    }
    let mut c = std::process::Command::new(exe);
    c.args(args);
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("CS_") {
            c.env_remove(k);
        }
    }
    let _ = c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

/// Window > Move Tab to New Window: the saved document in a window of its own.
pub fn open_in_new_window(p: &std::path::Path) {
    spawn(&[p.as_os_str()]);
}

/// File > Duplicate: a copy of the document in a new window ("Name copy.pages" beside it, or
/// in the temporary folder for an unsaved document).
pub fn duplicate(app: &mut App) {
    crate::text::finish_typing(app);
    let Some(w) = app.wb.as_mut() else { return };
    let stem = app.untitled_name.clone().or_else(|| w.doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string())).unwrap_or_else(|| "Untitled".into());
    let dir = w.doc.path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf()).unwrap_or_else(std::env::temp_dir);
    let mut copy = dir.join(format!("{stem} copy.pages"));
    let mut n = 2;
    while copy.exists() {
        copy = dir.join(format!("{stem} copy {n}.pages"));
        n += 1;
    }
    let mut k = w.doc.file.clone();
    pg_core::gc::sweep(&mut k);
    let bytes = k.to_zip();
    match bytes.map_err(|e| e.to_string()).and_then(|b| std::fs::write(&copy, b).map_err(|e| e.to_string())) {
        Ok(()) => crate::tabs::open_in_tab(app, &copy),
        Err(e) => app.notify(format!("Couldn't duplicate: {e}")),
    }
}

/// File > Export To > …: choose where, then write.
pub fn export_dialog(app: &mut App, what: &str) {
    crate::text::finish_typing(app);
    let (ext, filter) = match what {
        "PDF…" => ("pdf", "PDF"),
        "Word…" => ("docx", "Word document"),
        "EPUB…" => ("epub", "EPUB book"),
        "Plain Text…" => ("txt", "Plain text"),
        "Rich Text Format…" => ("rtf", "Rich Text"),
        "Images…" => ("png", "PNG images (one per page)"),
        _ => ("html", "Web page"),
    };
    let stem = app.title().trim_end_matches(".pages").to_string();
    let Some(path) = crate::app::file_dialog(|d| d.add_filter(filter, &[ext]).set_file_name(format!("{stem}.{ext}")).save_file()) else { return };
    export_to(app, &path);
}

pub fn export_to(app: &mut App, path: &Path) {
    let Some(w) = app.wb.as_ref() else { return };
    match pg_core::export::export(w, path) {
        Ok(()) => app.notify(format!("Exported {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        Err(e) => app.notify(format!("Couldn't export: {e}")),
    }
}

/// File > Print…: the pages as a PDF, opened in the desktop's viewer to print from (or sent
/// straight to the default printer when there is no viewer).
pub fn print(app: &mut App) {
    crate::text::finish_typing(app);
    let Some(w) = app.wb.as_ref() else { return };
    let out = std::env::temp_dir().join(format!("letters-print-{}.pdf", std::process::id()));
    match pg_core::export::to_pdf(w, &out) {
        Ok(n) => {
            if std::env::var_os("CS_SNAP").is_none() {
                let viewer = kn_ui::open_command(&out).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
                if viewer.is_err() {
                    let o = out.clone();
                    std::thread::spawn(move || {
                        let _ = std::process::Command::new("lp").arg(&o).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
                    });
                }
            }
            app.notify(format!("Prepared {n} {} for printing", if n == 1 { "page" } else { "pages" }));
        }
        Err(e) => app.notify(format!("Couldn't print: {e}")),
    }
}
