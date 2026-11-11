//! The window's own title bar (the suite's: Letters' and Notes' chrome.rs): the menus, a drag
//! area with the project's name and the desktop's window buttons; and the window's edges for
//! resizing without system decorations.

use egui::{pos2, vec2, Rect, Sense};
use kn_ui::theme;

use crate::app::App;

pub const TITLE_H: f32 = 30.0;

pub fn title_bar(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let row = ui.available_rect_before_wrap();
    let row = Rect::from_min_size(row.min, vec2(row.width(), TITLE_H));
    let draw_button = |ui: &mut egui::Ui, what: &str, r: Rect| {
        let resp = ui.interact(r, ui.scope_id().with(("winbtn", what.to_string())), Sense::click());
        let c = r.center();
        let bg = if resp.hovered() { theme::gray(0x55) } else { theme::gray(0x3c) };
        ui.painter().circle_filled(c, 11.0, bg);
        let icon = match what {
            "minimize" => crate::glyphs::ph::MINUS,
            "maximize" => crate::glyphs::ph::SQUARE,
            _ => crate::glyphs::ph::X,
        };
        crate::glyphs::paint(ui.painter(), Rect::from_center_size(c, vec2(16.0, 16.0)), icon, 14.0, theme::TEXT());
        if resp.clicked() {
            match what {
                "minimize" => minimize(&ctx),
                "maximize" => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    };
    let layout = app.buttons.clone();
    let mut x = row.max.x - 4.0;
    for what in layout.right.iter().rev() {
        draw_button(ui, what, Rect::from_min_max(pos2(x - 30.0, row.min.y), pos2(x, row.max.y)));
        x -= 32.0;
    }
    let buttons_left = x + 2.0;
    let mut lx = row.min.x + 2.0;
    for what in &layout.left {
        draw_button(ui, what, Rect::from_min_max(pos2(lx, row.min.y), pos2(lx + 30.0, row.max.y)));
        lx += 32.0;
    }
    let row = Rect::from_min_max(pos2(if layout.left.is_empty() { row.min.x } else { lx + 6.0 }, row.min.y), row.max);
    let menus_w = app.menu_bar_width.unwrap_or(620.0).min(buttons_left - row.min.x);
    let menus_rect = Rect::from_min_max(row.min, pos2(row.min.x + menus_w, row.max.y));
    let used = ui.scope_builder(egui::UiBuilder::new().max_rect(menus_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| crate::menus::menu_bar(app, ui)).inner;
    app.menu_bar_width = Some(used - row.min.x + 4.0);
    // the project's name in the middle, as Logic's window title ("Untitled.logicx - Tracks")
    let title = format!("{} - Tracks", app.project_title());
    ui.painter().text(pos2(row.center().x.max(used + 120.0), row.center().y), egui::Align2::CENTER_CENTER, title, crate::look::semibold(13.0), theme::TEXT_DIM());
    let drag_rect = Rect::from_min_max(pos2(used.min(buttons_left) + 8.0, row.min.y), pos2(buttons_left - 8.0, row.max.y));
    if drag_rect.width() > 10.0 {
        let drag = ui.interact(drag_rect, ui.scope_id().with("titlebar-drag"), Sense::click());
        let press = drag.hovered() && ui.input(|i| i.pointer.primary_pressed() && i.pointer.press_origin().is_some_and(|p| drag_rect.contains(p)));
        if press {
            let now = ui.input(|i| i.time);
            if now - app.title_press_at < 0.4 {
                app.title_press_at = -1.0;
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            } else {
                app.title_press_at = now;
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
    }
    ui.expand_to_include_rect(row);
}

/// Without system decorations the window's edges are ours: resize cursors near them, and the
/// press goes to the compositor to resize.
pub fn window_edges(ctx: &egui::Context, r: Rect) {
    use egui::viewport::ResizeDirection as D;
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let m = 6.0;
    if !r.expand(12.0).contains(p) {
        return;
    }
    let (l, rt, t, b) = (p.x - r.min.x < m, r.max.x - p.x < m, p.y - r.min.y < m, r.max.y - p.y < m);
    let dir = match (l, rt, t, b) {
        (true, _, true, _) => Some(D::NorthWest),
        (_, true, true, _) => Some(D::NorthEast),
        (true, _, _, true) => Some(D::SouthWest),
        (_, true, _, true) => Some(D::SouthEast),
        (true, ..) => Some(D::West),
        (_, true, ..) => Some(D::East),
        (_, _, true, _) => Some(D::North),
        (_, _, _, true) => Some(D::South),
        _ => None,
    };
    let Some(dir) = dir else { return };
    ctx.set_cursor_icon(match dir {
        D::North | D::South => egui::CursorIcon::ResizeVertical,
        D::East | D::West => egui::CursorIcon::ResizeHorizontal,
        D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
        _ => egui::CursorIcon::ResizeNeSw,
    });
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
}

pub fn minimize(ctx: &egui::Context) {
    if std::env::var_os("LG_SCRIPT").is_none() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}
