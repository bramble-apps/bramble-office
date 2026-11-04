//! Window > Open Note in New Window (and a double-click on a note in the list): the note in a
//! window of its own, with an editor of its own (caret, selection, undo: editor::show_in). Window >
//! Float on Top keeps the note window used last above the others. Menu commands stay the main
//! window's, as on the Mac.

use crate::app::App;

pub struct NoteWindow {
    pub id: egui::ViewportId,
    pub note: String,
    pub on_top: bool,
}

/// The note's window: brought forward if it has one, else opened.
pub fn open(app: &mut App, ctx: &egui::Context, note: &str) {
    if let Some(w) = app.note_windows.iter().find(|w| w.note == note) {
        ctx.send_viewport_cmd_to(w.id, egui::ViewportCommand::Focus);
        return;
    }
    app.window_seq += 1;
    let id = egui::ViewportId::from_hash_of(("note-window", app.window_seq));
    app.note_windows.push(NoteWindow { id, note: note.to_string(), on_top: false });
    app.last_note_window = Some(id);
}

/// Window > Float on Top: the note window used last (ticked when it floats).
pub fn float_on_top(app: &mut App, ctx: &egui::Context) {
    let Some(w) = app.last_note_window.and_then(|id| app.note_windows.iter_mut().find(|w| w.id == id)) else { return };
    w.on_top = !w.on_top;
    ctx.send_viewport_cmd_to(w.id, egui::ViewportCommand::WindowLevel(if w.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
}

pub fn floating(app: &App) -> Option<bool> {
    app.last_note_window.and_then(|id| app.note_windows.iter().find(|w| w.id == id)).map(|w| w.on_top)
}

/// Each frame: the note windows drawn (each its own viewport); closed ones let go.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let windows = std::mem::take(&mut app.note_windows);
    let mut keep = Vec::new();
    for w in windows {
        let key = egui::Id::new(w.id);
        // (its note gone or deleted: the window closes)
        let Some(title) = app.note_by_id(&w.note).filter(|n| !n.deleted).map(crate::list::note_title) else {
            crate::editor::close_window(key);
            continue;
        };
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_app_id(std::env::var("FLATPAK_ID").unwrap_or_else(|_| "bramble-notes".into())) // (in a Flatpak, its app id, so the desktop finds the icon)
            .with_inner_size([620.0, 720.0])
            .with_min_inner_size([320.0, 240.0])
            .with_window_level(if w.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal });
        let mut closed = false;
        let mut used = false;
        ctx.show_viewport_immediate(w.id, builder, |ctx, _class| {
            let _t = crate::PartTimer::start("note window");
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(crate::look::content_bg())).show(ctx, |ui| {
                crate::editor::show_in(app, ui, &w.note, key);
            });
            used = ctx.input(|i| i.pointer.any_pressed() || !i.events.is_empty() && i.focused);
            closed = ctx.input(|i| i.viewport().close_requested());
        });
        if used {
            app.last_note_window = Some(w.id);
        }
        if closed {
            crate::editor::close_window(key);
            if app.last_note_window == Some(w.id) {
                app.last_note_window = None;
            }
        } else {
            keep.push(w);
        }
    }
    // (any opened while these were drawn come after them)
    keep.append(&mut app.note_windows);
    app.note_windows = keep;
}
