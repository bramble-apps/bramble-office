//! "Open Event List…" (⌘7) and "Open Signature List…" as Logic's: each opens its list in a
//! window of its own (ui-event-list.png, ui-signature-list.png: 452 pt wide, titled
//! "<project> - <track> - Event List"), not in the List Editors pane.

use crate::app::App;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
#[allow(dead_code)]
pub const COMMANDS: &[&str] = &["Open Event List…", "Open Signature List…"];

#[derive(Clone, Copy, Default)]
struct Open {
    events: bool,
    signature: bool,
}

fn id() -> egui::Id {
    egui::Id::unique("lg-list-windows")
}

fn get(ctx: &egui::Context) -> Open {
    ctx.data(|d| d.get_temp::<Open>(id())).unwrap_or_default()
}

fn put(ctx: &egui::Context, o: Open) {
    ctx.data_mut(|d| d.insert_temp(id(), o));
}

pub fn command(_app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let mut o = get(ctx);
    match name {
        "Open Event List…" => o.events = true,
        "Open Signature List…" => o.signature = true,
        _ => return false,
    }
    put(ctx, o);
    true
}

/// The windows' titles: the project's file name, the track, the list.
fn title(app: &App, list: &str) -> String {
    let project = app.project.as_ref().map_or("Untitled".to_string(), |p| p.name.clone());
    match app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)) {
        Some(t) if list == "Event List" => format!("{project}.logicx - {} - {list}", t.name),
        _ => format!("{project}.logicx - {list}"),
    }
}

/// Draws the open list windows (every frame).
pub fn windows(app: &mut App, ctx: &egui::Context) {
    let mut o = get(ctx);
    if o.events {
        let mut open = true;
        egui::Window::new(title(app, "Event List")).id(egui::Id::unique("lg-event-list-window")).open(&mut open).collapsible(false).resizable(true).default_size([452.0, 700.0]).min_width(360.0).show(ctx, |ui| {
            let r = ui.available_rect_before_wrap();
            let body = egui::Rect::from_min_size(r.min, egui::vec2(r.width(), r.height().max(300.0)));
            ui.allocate_rect(body, egui::Sense::hover());
            crate::evlist::show(app, ui, body);
        });
        o.events = open;
    }
    if o.signature {
        let mut open = true;
        egui::Window::new(title(app, "Signature List")).id(egui::Id::unique("lg-signature-list-window")).open(&mut open).collapsible(false).resizable(true).default_size([452.0, 500.0]).show(ctx, |ui| {
            crate::arranging::signature_list(app, ui);
        });
        o.signature = open;
    }
    put(ctx, o);
}
