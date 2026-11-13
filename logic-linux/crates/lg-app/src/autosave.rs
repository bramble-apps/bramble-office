//! Autosave, as Logic's: while a project has unsaved changes it is written every few minutes to
//! an Autosave folder in its alternative (the project file itself is left alone). Opening a
//! project whose autosave is newer than its last save (after a crash) asks whether to use the
//! autosaved version; declining opens the last saved one. Saving removes the autosave.

use std::path::{Path, PathBuf};

use crate::app::App;

/// Seconds between autosaves.
pub const EVERY: f64 = 120.0;

#[derive(Default)]
pub struct State {
    last: Option<f64>,
    /// After opening: the newer autosave found, waiting for the answer.
    pub ask: Option<PathBuf>,
}

pub fn file_of(alt: &Path) -> PathBuf {
    alt.join("Autosave").join("ProjectData")
}

/// An autosave newer than the saved project data.
pub fn newer(alt: &Path) -> Option<PathBuf> {
    let a = file_of(alt);
    let am = std::fs::metadata(&a).ok()?.modified().ok()?;
    let sm = std::fs::metadata(alt.join("ProjectData")).ok().and_then(|m| m.modified().ok());
    (sm.is_none_or(|s| am > s)).then_some(a)
}

fn can(app: &App) -> bool {
    app.project.is_some() && app.file.is_some() && !app.demo && app.pf.band.is_none() && !app.automated
}

/// Each frame: an autosave when due.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    ask_window(app, ctx);
    if !app.dirty || !can(app) {
        app.autosave.last = None;
        return;
    }
    let now = ctx.input(|i| i.time);
    let last = *app.autosave.last.get_or_insert(now);
    if now - last < EVERY {
        return;
    }
    app.autosave.last = Some(now);
    write(app);
}

/// Writes the autosave (the project as Save would write it, into Autosave/).
pub fn write(app: &mut App) -> bool {
    let (Some(p), Some((alt, original))) = (app.project.as_ref(), app.file.as_ref()) else { return false };
    let Ok(saved) = lg_core::save::save(p, original) else { return false };
    let f = file_of(alt);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(&f, saved.data).is_ok()
}

/// Saving: the autosave goes.
pub fn clear(app: &mut App) {
    if let Some((alt, _)) = app.file.as_ref() {
        let _ = std::fs::remove_dir_all(alt.join("Autosave"));
    }
    app.autosave.last = None;
}

/// After opening: a newer autosave is offered.
pub fn after_open(app: &mut App) {
    app.autosave.ask = None;
    if app.automated || app.pf.band.is_some() {
        return;
    }
    if let Some((alt, _)) = app.file.as_ref() {
        app.autosave.ask = newer(alt);
    }
}

fn ask_window(app: &mut App, ctx: &egui::Context) {
    let Some(f) = app.autosave.ask.clone() else { return };
    let mut answer: Option<bool> = None;
    egui::Window::new("Autosave").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.label("This project was autosaved after it was last saved (Studio may have quit unexpectedly). Use the autosaved version?");
        ui.horizontal(|ui| {
            if ui.button("Use Last Saved").clicked() {
                answer = Some(false);
            }
            if ui.button("Use Autosave").clicked() {
                answer = Some(true);
            }
        });
    });
    match answer {
        Some(true) => {
            app.autosave.ask = None;
            if let Ok(d) = std::fs::read(&f) {
                crate::projfile::revert_to(app, d, "Autosave");
            }
        }
        Some(false) => {
            app.autosave.ask = None;
            if let Some(d) = f.parent() {
                let _ = std::fs::remove_dir_all(d);
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_autosave_is_found() {
        let alt = std::env::temp_dir().join(format!("lg-autosave-{}", std::process::id()));
        std::fs::create_dir_all(&alt).unwrap();
        std::fs::write(alt.join("ProjectData"), b"saved").unwrap();
        assert_eq!(newer(&alt), None);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(alt.join("Autosave")).unwrap();
        std::fs::write(file_of(&alt), b"auto").unwrap();
        assert_eq!(newer(&alt), Some(file_of(&alt)));
        let _ = std::fs::remove_dir_all(&alt);
    }
}
