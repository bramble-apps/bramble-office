//! The window around the sheet, drawn the way Keydeck draws its own (kn-ui, Keynote 15.4 /
//! macOS 26): menus in the title bar with the window buttons where the desktop puts them, and
//! the toolbar's buttons as icon capsules floating over the panels (liquid glass, translucent
//! or plain, per Keydeck's settings). The toolbar's items and View > Customize Toolbar… are in
//! toolbar.rs; the popovers and the document's name are here.

use egui::{pos2, vec2, Color32, FontId, Rect, Sense, Stroke};
use kn_ui::icons::{draw, Icon};
use kn_ui::theme;

use crate::app::App;

/// The document's name (Numbers, Keydeck): click for Name and Where (rename or move the file),
/// right-click for the folders it is in.
pub(crate) fn title_item(app: &mut App, ui: &mut egui::Ui, name_r: Rect, cy: f32, name: &str, font: FontId, dirty: bool) {
    let ctx = ui.ctx().clone();
    let tw = ui.fonts_mut(|f| f.layout_no_wrap(name.to_string(), font.clone(), Color32::WHITE).size().x);
    // under the name: "Edited", and an Excel workbook's mode
    let mode = app.excel.as_ref().map(|x| if x.compat { "Excel + Values" } else { "Excel" });
    let sub = match (mode, dirty) {
        (Some(m), true) => Some(format!("{m} · Edited")),
        (Some(m), false) => Some(m.to_string()),
        (None, true) => Some("Edited".to_string()),
        (None, false) => None,
    };
    let dirty = sub.is_some();
    let block_h = if dirty { 34.0 } else { 18.0 };
    let hit = Rect::from_min_size(pos2(name_r.min.x - 8.0, cy - (block_h + 14.0) / 2.0), vec2((tw + 36.0).min(name_r.width() + 8.0), block_h + 14.0));
    let resp = ui.interact(hit, ui.scope_id().with("doc-title"), Sense::click());
    let ny = if dirty { cy - 7.0 } else { cy };
    if resp.hovered() || app.doc_title.is_some() {
        ui.painter().rect_filled(hit, 8.0, theme::gray(0x3a));
        draw(ui.painter(), Rect::from_center_size(pos2(hit.max.x - 14.0, ny + 1.0), vec2(11.0, 11.0)), Icon::ChevronDown, theme::TEXT_DIM());
    }
    ui.painter().text(pos2(name_r.min.x, ny), egui::Align2::LEFT_CENTER, name, font, theme::TEXT());
    if let Some(sub) = &sub {
        ui.painter().text(pos2(name_r.min.x, cy + 9.0), egui::Align2::LEFT_CENTER, sub, FontId::proportional(11.0), theme::TEXT_DIM());
    }
    if app.wb.is_none() {
        return;
    }
    let path = app.excel.as_ref().map(|x| x.path.clone()).or_else(|| app.doc().and_then(|d| d.path.clone()));
    // right-click: the folders the file is in (choose one to open it)
    if let Some(path) = path.clone() {
        resp.context_menu(|ui| {
            for (k, a) in path.ancestors().enumerate() {
                let n = a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| if k == 0 { String::new() } else { "/".into() });
                if n.is_empty() {
                    continue;
                }
                if ui.button(format!("{}{n}", if k == 0 { "" } else { "📁 " })).clicked() {
                    let open = if k == 0 { a.parent().unwrap_or(a) } else { a };
                    let _ = kn_ui::open_command(open).stdin(std::process::Stdio::null()).spawn();
                    ui.close();
                }
            }
        });
    }
    if resp.clicked() {
        if app.doc_title.is_some() {
            app.doc_title = None;
        } else {
            let stem = path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.trim_end_matches(".numbers").to_string());
            // the format starts as the document's: 0 Excel, 1 Excel + Values, 2 Numbers
            let f: u8 = match &app.excel {
                Some(x) if x.compat => 1,
                Some(_) => 0,
                None => 2,
            };
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("doc-format"), f));
            let dir = path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf()).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Documents")));
            app.doc_title = Some((stem, dir));
            return;
        }
    }
    let mut apply = false;
    let mut close = false;
    if let Some((name, dir)) = app.doc_title.as_mut() {
        let area = egui::Area::new(egui::Id::unique("doc-title-popover")).order(egui::Order::Foreground).fixed_pos(pos2(hit.min.x, hit.max.y + 6.0));
        let r = area.show(&ctx, |ui| {
            egui::Frame::new().fill(theme::gray(0x2c)).stroke(egui::Stroke::new(1.0, theme::gray(0x48))).corner_radius(10).inner_margin(14).shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(110) }).show(ui, |ui| {
                ui.set_width(300.0);
                egui::Grid::new("doc-title-grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                    ui.label("Name:");
                    let te = ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                    if !te.has_focus() && !te.lost_focus() && ui.memory(|m| m.focused().is_none()) {
                        te.request_focus();
                    }
                    if te.lost_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                        apply = true;
                    }
                    ui.end_row();
                    ui.label("Where:");
                    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                    let mut places: Vec<std::path::PathBuf> = Vec::new();
                    places.extend(dir.clone());
                    for sub in ["Documents", "Desktop", ""] {
                        if let Some(h) = &home {
                            let p = if sub.is_empty() { h.clone() } else { h.join(sub) };
                            if p.is_dir() && !places.contains(&p) {
                                places.push(p);
                            }
                        }
                    }
                    let show = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
                    egui::ComboBox::from_id_salt("doc-where").width(220.0).selected_text(dir.as_deref().map(show).unwrap_or_default()).show_ui(ui, |ui| {
                        for p in places {
                            if ui.selectable_label(dir.as_ref() == Some(&p), format!("📁 {}", show(&p))).clicked() {
                                *dir = Some(p);
                            }
                        }
                        if ui.button("Other…").clicked() {
                            if let Some(f) = crate::app::file_dialog(|d| d.set_title("Where").pick_folder()) {
                                *dir = Some(f);
                            }
                        }
                    });
                    ui.end_row();
                    // Excel mode (an .xlsx stays one), with Values' features kept inside it, or
                    // a Numbers spreadsheet
                    ui.label("Format:");
                    let mut f: u8 = ctx.data(|d| d.get_temp(egui::Id::unique("doc-format"))).unwrap_or(2);
                    let names = ["Excel workbook", "Excel + Values features", "Numbers spreadsheet"];
                    egui::ComboBox::from_id_salt("doc-format-pick").width(220.0).selected_text(names[f as usize]).show_ui(ui, |ui| {
                        for (k, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut f, k as u8, *n);
                        }
                    });
                    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("doc-format"), f));
                    ui.end_row();
                });
                let f: u8 = ctx.data(|d| d.get_temp(egui::Id::unique("doc-format"))).unwrap_or(2);
                let note = match f {
                    0 => "Saves as an Excel workbook. Only what Excel can hold can be added.",
                    1 => "Saves as an Excel workbook that also keeps every Values feature for Values (other apps see the Excel part).",
                    _ => "Saves as a Numbers spreadsheet.",
                };
                ui.add_space(4.0);
                ui.label(egui::RichText::new(note).small().weak());
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        } else if r.response.clicked_elsewhere() && !resp.clicked() && !egui::Popup::is_any_open(&ctx) {
            // like Numbers, closing the popover keeps the name typed
            apply = true;
        }
    }
    if apply || close {
        let f: u8 = ctx.data(|d| d.get_temp(egui::Id::unique("doc-format"))).unwrap_or(2);
        if let (true, Some((name, Some(dir)))) = (apply, app.doc_title.take()) {
            let name = name.trim().trim_end_matches(".numbers").trim_end_matches(".xlsx").to_string();
            if !name.is_empty() && !name.contains('/') {
                let was: u8 = match &app.excel {
                    Some(x) if x.compat => 1,
                    Some(_) => 0,
                    None => 2,
                };
                match (was, f) {
                    // the same format: a rename or move
                    (a, b) if a == b => crate::fileops::relocate(app, dir.join(format!("{name}.{}", if f == 2 { "numbers" } else { "xlsx" }))),
                    // Excel <-> Excel + Values: the same workbook, saved again its new way
                    (0 | 1, 0 | 1) => {
                        if let Some(x) = app.excel.clone() {
                            app.save_excel(&x.path, f == 1);
                        }
                    }
                    // to Numbers: a Numbers spreadsheet beside it (the workbook stays)
                    (_, 2) => app.save_numbers_to(&dir.join(format!("{name}.numbers"))),
                    // to Excel: an Excel workbook beside the spreadsheet
                    _ => app.save_excel(&dir.join(format!("{name}.xlsx")), f == 1),
                }
            }
        }
        app.doc_title = None;
    }
}

/// The capsules behind the buttons, as Keydeck draws them.
pub(crate) fn paint_capsules(ui: &egui::Ui, idx: egui::layers::ShapeIdx, caps: &[Rect]) {
    let light = theme::is_light();
    // liquid glass (Settings) is Keydeck's, tinted by the colour theme, in both appearances
    if kn_ui::glass::enabled() {
        let ctx = ui.ctx().clone();
        let shapes: Vec<egui::Shape> = caps.iter().flat_map(|c| kn_ui::glass::shapes(&ctx, *c, c.height() / 2.0, true)).collect();
        ui.painter().set(idx, egui::Shape::Vec(shapes));
    } else if kn_ui::glass::classic() {
        ui.painter().set(idx, egui::Shape::Vec(caps.iter().map(|c| egui::Shape::rect_filled(*c, c.height() / 2.0, theme::gray(0x36))).collect()));
    } else if !light {
        // Numbers' dark capsules: solid, a light rim brighter at the top and bottom (Mac N2)
        let mut shapes = Vec::new();
        for c in caps {
            let r = c.height() / 2.0;
            shapes.push(egui::Shape::rect_filled(*c, r, crate::look::capsule_fill()));
            shapes.push(egui::Shape::rect_stroke(*c, r, Stroke::new(1.0, Color32::from_white_alpha(34)), egui::StrokeKind::Inside));
            for y in [c.min.y + 0.5, c.max.y - 0.5] {
                shapes.push(egui::Shape::line_segment([pos2(c.min.x + r, y), pos2(c.max.x - r, y)], Stroke::new(1.0, Color32::from_white_alpha(22))));
            }
        }
        ui.painter().set(idx, egui::Shape::Vec(shapes));
    } else {
        let glass = theme::tint(if light { Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 215) } else { Color32::from_rgba_unmultiplied(0x3a, 0x3a, 0x3c, 225) });
        let edge = if light { Color32::from_black_alpha(22) } else { Color32::from_white_alpha(22) };
        let mut shapes = Vec::new();
        for c in caps {
            let r = c.height() / 2.0;
            shapes.push(egui::Shape::rect_filled(c.expand(1.0), r + 1.0, Color32::from_black_alpha(if light { 16 } else { 60 })));
            shapes.push(egui::Shape::rect_filled(*c, r, glass));
            shapes.push(egui::Shape::rect_stroke(*c, r, Stroke::new(1.0, edge), egui::StrokeKind::Inside));
        }
        ui.painter().set(idx, egui::Shape::Vec(shapes));
    }
}

pub const ZOOMS: [f32; 9] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

pub fn zoom_menu(app: &mut App, ui: &mut egui::Ui) {
    for z in ZOOMS {
        if ui.radio((app.canvas.zoom - z).abs() < 1e-3, format!("{:.0}%", z * 100.0)).clicked() {
            app.canvas.animate_zoom(z);
            ui.close();
        }
    }
}

/// The toolbar's View menu (Numbers: the same items as View in the menu bar's first part).
pub fn view_menu(app: &mut App, ui: &mut egui::Ui) {
    let insp = if app.inspector { "Hide Inspector" } else { "Show Inspector" };
    if ui.button(insp).clicked() {
        app.inspector = !app.inspector;
        ui.close();
    }
    if ui.button("Show Find & Replace").clicked() {
        app.find = Some((String::new(), Vec::new(), 0));
        ui.close();
    }
    ui.separator();
    let c = if app.show_comments { "Hide Comments" } else { "Show Comments" };
    if ui.button(c).clicked() {
        app.show_comments = !app.show_comments;
        ui.close();
    }
}

/// Insert (the toolbar's menu and the menu bar's).
pub fn insert_menu(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button("Table", |ui| {
        for (i, name) in nm_core::sheets::TABLE_STYLES.iter().enumerate() {
            if ui.button(*name).clicked() {
                app.insert_table(i);
            }
        }
    });
    ui.menu_button("Chart", |ui| {
        for (i, (name, _)) in nm_core::charts::CHART_TYPES.iter().enumerate() {
            if ui.button(*name).clicked() {
                app.insert_chart(i, None);
            }
        }
    });
    if ui.button("Text Box").clicked() {
        app.insert_text_box();
        ui.close();
    }
    ui.menu_button("Shape", |ui| {
        for (i, name) in nm_core::sheets::SHAPES.iter().enumerate() {
            if ui.button(*name).clicked() {
                app.insert_shape(i);
            }
        }
    });
    if ui.button("Choose…").on_hover_text("Insert an image").clicked() {
        app.insert_image_dialog();
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(app.sel.is_some_and(|s| s.anchor == s.cursor), egui::Button::new("Comment")).clicked() {
        app.add_comment();
        ui.close();
    }
    ui.separator();
    if ui.button("Sheet").clicked() {
        app.add_sheet();
        ui.close();
    }
}

/// The title bar: window buttons where the desktop puts them, the menus, and the empty middle
/// to drag (double-click maximizes) — Keydeck's title_bar.
pub fn title_bar(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let row = ui.available_rect_before_wrap();
    let row = Rect::from_min_size(row.min, vec2(row.width(), 30.0));
    let draw_button = |ui: &mut egui::Ui, what: &str, r: Rect| {
        let resp = ui.interact(r, ui.scope_id().with(("winbtn", what.to_string())), Sense::click());
        let c = r.center();
        let bg = if resp.hovered() { theme::gray(0x55) } else { theme::gray(0x3c) };
        ui.painter().circle_filled(c, 11.0, bg);
        let st = Stroke::new(1.3, theme::TEXT());
        match what {
            "minimize" => {
                ui.painter().line_segment([c + vec2(-4.0, 0.0), c + vec2(4.0, 0.0)], st);
            }
            "maximize" => {
                ui.painter().rect_stroke(Rect::from_center_size(c, vec2(8.0, 8.0)), 1.0, st, egui::StrokeKind::Middle);
            }
            _ => {
                ui.painter().line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], st);
                ui.painter().line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], st);
            }
        }
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
    // the menus get only the width they took last frame, so they can't cover the drag area
    let menus_w = app.menu_bar_width.unwrap_or(620.0).min(buttons_left - row.min.x);
    let menus_rect = Rect::from_min_max(row.min, pos2(row.min.x + menus_w, row.max.y));
    let used = ui.scope_builder(egui::UiBuilder::new().max_rect(menus_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| crate::menus::menu_bar(app, ui)).inner;
    app.menu_bar_width = Some(used - row.min.x + 4.0);
    let drag_rect = Rect::from_min_max(pos2(used.min(buttons_left) + 8.0, row.min.y), pos2(buttons_left - 8.0, row.max.y));
    if drag_rect.width() > 10.0 {
        let _drag = ui.interact(drag_rect, ui.scope_id().with("titlebar-drag"), Sense::click());
        let press = ui.input(|i| i.pointer.primary_pressed() && i.pointer.press_origin().is_some_and(|p| drag_rect.contains(p)));
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

/// A popover tile: a dark rounded square, highlighted under the pointer. Returns (clicked,
/// the drawing area).
fn tile(ui: &mut egui::Ui, size: f32, tip: &str) -> (bool, Rect) {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let bg = if resp.hovered() { kn_ui::contenthub::tile_hover() } else { kn_ui::contenthub::tile_bg() };
    ui.painter().rect_filled(r, 18.0, bg);
    if resp.hovered() {
        ui.painter().rect_stroke(r, 18.0, Stroke::new(2.0, theme::ACCENT()), egui::StrokeKind::Inside);
    }
    let clicked = resp.on_hover_text(tip).clicked();
    (clicked, r.shrink(size * 0.2))
}

/// The toolbar's Table popover (Numbers 15.4, N-ui-5-toolbar-Table): the six table styles as
/// small tables in a 2 x 3 grid.
pub(crate) fn table_popover(app: &mut App, ui: &mut egui::Ui) {
    let mut pick = None;
    egui::Grid::new("table-popover").spacing([12.0, 12.0]).show(ui, |ui| {
        for (k, name) in nm_core::sheets::TABLE_STYLES.iter().enumerate() {
            let (clicked, r) = tile(ui, 120.0, name);
            let p = ui.painter();
            let (cols, rows) = (4, 5);
            let (cw, rh) = (r.width() / cols as f32, r.height() / rows as f32);
            let head = Color32::from_gray(0xc4);
            p.rect_filled(r, 0.0, Color32::from_gray(0xee));
            // header row / header column / footer as each style has them
            let (hr, hc, foot) = match k {
                0 => (true, true, false),
                1 => (true, false, false),
                2 => (false, false, false),
                3 => (true, true, true),
                _ => (true, false, false),
            };
            if hr {
                p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), rh)), 0.0, head);
            }
            if hc {
                p.rect_filled(Rect::from_min_size(r.min, vec2(cw, r.height())), 0.0, head);
            }
            if foot {
                p.rect_filled(Rect::from_min_size(pos2(r.min.x, r.max.y - rh), vec2(r.width(), rh)), 0.0, head);
            }
            let line = Stroke::new(0.8, Color32::from_gray(0xa8));
            for i in 1..rows {
                p.hline(r.x_range(), r.min.y + rh * i as f32, line);
            }
            for i in 1..cols {
                p.vline(r.min.x + cw * i as f32, r.y_range(), line);
            }
            if k == 4 {
                // Checklist: checkboxes down the first column
                for i in 1..rows {
                    let b = Rect::from_center_size(pos2(r.min.x + cw / 2.0, r.min.y + rh * (i as f32 + 0.5)), vec2(9.0, 9.0));
                    p.rect_stroke(b, 1.5, Stroke::new(1.0, Color32::from_gray(0x60)), egui::StrokeKind::Inside);
                    if i % 2 == 1 {
                        p.line(vec![pos2(b.min.x + 2.0, b.center().y), pos2(b.center().x - 0.5, b.max.y - 2.0), pos2(b.max.x - 1.5, b.min.y + 1.5)], Stroke::new(1.4, Color32::BLACK));
                    }
                }
            }
            if k == 5 {
                // Stocks: up / down arrows
                for i in 1..rows {
                    for j in 1..cols {
                        let c = pos2(r.min.x + cw * (j as f32 + 0.5), r.min.y + rh * (i as f32 + 0.5));
                        let up = (i + j) % 2 == 0;
                        let (col, d) = if up { (Color32::from_rgb(0x4c, 0xb0, 0x4c), -3.0) } else { (Color32::from_rgb(0xd0, 0x40, 0x40), 3.0) };
                        p.add(egui::Shape::convex_polygon(vec![pos2(c.x, c.y + d), pos2(c.x - 3.5, c.y - d), pos2(c.x + 3.5, c.y - d)], col, Stroke::NONE));
                    }
                }
            }
            if clicked {
                pick = Some(k);
            }
            if k % 2 == 1 {
                ui.end_row();
            }
        }
    });
    if let Some(k) = pick {
        app.insert_table(k);
        ui.close();
    }
}

/// The toolbar's Shape popover (Numbers 15.4: Line, Rounded Rectangle, Circle, Square, Arrow
/// among its first tiles), drawn as the shapes.
/// Numbers 15.4's Shape popover tiles, in order.
const SHAPE_TILES: [&str; 12] = ["Text", "Line", "Curve", "Rounded Rectangle", "Circle", "Triangle", "Pentagon", "Square", "Diamond", "Capsule", "Parallelogram", "Arrow"];

pub(crate) fn shape_popover(app: &mut App, ui: &mut egui::Ui) {
    // Numbers 15.4's Shape popover (N-ui-5-toolbar-Shape): Text, Line, Curve; Rounded
    // Rectangle, Circle, Triangle; Pentagon, Square, Diamond; Capsule, Parallelogram, Arrow;
    // then Draw with Pen
    const TILES: [&str; 12] = SHAPE_TILES;
    let mut pick = None;
    // light grey glyphs on the dark tiles, mid grey on light ones
    let fill = kn_ui::contenthub::ink();
    egui::Grid::new("shape-popover").spacing([10.0, 10.0]).show(ui, |ui| {
        for (k, name) in TILES.iter().enumerate() {
            let (clicked, r) = tile(ui, 76.0, name);
            let p = ui.painter();
            let (c, w, h) = (r.center(), r.width(), r.height());
            let at = |x: f32, y: f32| pos2(r.min.x + x * w, r.min.y + y * h);
            match *name {
                "Text" => {
                    theme::centered(p, c, "Text", egui::FontId::proportional(15.0), fill);
                }
                "Line" => {
                    p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(2.5, fill));
                }
                "Curve" => {
                    let pts: Vec<egui::Pos2> = (0..=20).map(|i| {
                        let t = i as f32 / 20.0;
                        let a = std::f32::consts::FRAC_PI_2 * t;
                        at(1.0 - a.cos(), 1.0 - a.sin())
                    }).collect();
                    p.add(egui::Shape::line(pts, Stroke::new(2.5, fill)));
                    p.circle_filled(at(0.0, 1.0), 3.0, fill);
                    p.circle_filled(at(1.0, 0.0), 3.0, fill);
                }
                "Rounded Rectangle" => {
                    p.rect_filled(r, 8.0, fill);
                }
                "Circle" => {
                    p.circle_filled(c, w / 2.0, fill);
                }
                "Triangle" => {
                    p.add(egui::Shape::convex_polygon(vec![at(0.5, 0.05), at(1.0, 0.95), at(0.0, 0.95)], fill, Stroke::NONE));
                }
                "Pentagon" => {
                    let pts = (0..5).map(|i| {
                        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / 5.0;
                        c + vec2(a.cos(), a.sin()) * (w / 2.0)
                    }).collect();
                    p.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
                }
                "Square" => {
                    p.rect_filled(r, 0.0, fill);
                }
                "Diamond" => {
                    p.add(egui::Shape::convex_polygon(vec![at(0.5, 0.0), at(1.0, 0.5), at(0.5, 1.0), at(0.0, 0.5)], fill, Stroke::NONE));
                }
                "Capsule" => {
                    p.rect_filled(Rect::from_center_size(c, vec2(w, h * 0.45)), h * 0.225, fill);
                }
                "Parallelogram" => {
                    p.add(egui::Shape::convex_polygon(vec![at(0.25, 0.25), at(1.0, 0.25), at(0.75, 0.75), at(0.0, 0.75)], fill, Stroke::NONE));
                }
                _ => {
                    let (x0, x1, xm) = (r.min.x, r.max.x, r.min.x + r.width() * 0.5);
                    let (y0, y1, ya, yb) = (r.min.y, r.max.y, r.min.y + r.height() * 0.3, r.min.y + r.height() * 0.7);
                    p.add(egui::Shape::convex_polygon(vec![pos2(xm, y0), pos2(x1, r.center().y), pos2(xm, y1)], fill, Stroke::NONE));
                    p.rect_filled(Rect::from_min_max(pos2(x0, ya), pos2(xm + 1.0, yb)), 0.0, fill);
                }
            }
            if clicked {
                pick = Some(*name);
            }
            if k % 3 == 2 {
                ui.end_row();
            }
        }
    });
    ui.add_space(6.0);
    let pen = ui.add_sized([ui.available_width(), 32.0], egui::Button::new("✎  Draw with Pen").fill(kn_ui::contenthub::tile_bg()).corner_radius(16.0)).clicked();
    if pen {
        crate::menus::run(app, ui.ctx(), "Line > Draw With Pen");
        ui.close();
        return;
    }
    // the shape library and My Shapes are in the Content Hub, as on the Mac
    if ui.add_sized([ui.available_width(), 32.0], egui::Button::new("Browse Content Hub").fill(kn_ui::contenthub::tile_bg()).corner_radius(16.0)).clicked() {
        ui.close();
        app.hub.show_tab(kn_ui::contenthub::Tab::Shapes);
        return;
    }
    let Some(name) = pick else { return };
    insert_tile(app, name);
    ui.close();
}

/// The Content Hub's Shapes: the popover's tiles, then My Shapes (by name).
pub(crate) fn hub_shapes() -> (Vec<kn_ui::contenthub::AppShape>, Vec<kn_ui::contenthub::AppShape>) {
    use kn_ui::contenthub::{AppShape, Glyph};
    use nm_core::kn_model::insert::BasicShape;
    let basic = SHAPE_TILES
        .iter()
        .map(|name| {
            let basic = |b: BasicShape| Glyph::Path { path: nm_core::kn_model::path::from_path_source(&b.path_source(), 100.0, 100.0), w: 100.0, h: 100.0 };
            let glyph = match *name {
                "Text" => Glyph::Text,
                "Line" => Glyph::Line { arrow: false },
                "Curve" => Glyph::Curve,
                "Rounded Rectangle" => basic(BasicShape::RoundedRectangle),
                "Circle" => basic(BasicShape::Oval),
                "Triangle" => basic(BasicShape::Triangle),
                "Pentagon" => basic(BasicShape::Pentagon),
                "Square" => basic(BasicShape::Rectangle),
                "Diamond" => basic(BasicShape::Diamond),
                "Arrow" => basic(BasicShape::Arrow),
                n => match nm_core::sheets::extra_shape(n) {
                    Some((els, (w, h))) => {
                        use nm_core::kn_model::{Point, PathEl as E};
                        let p = |(x, y): (f32, f32)| Point { x, y };
                        let els = els
                            .into_iter()
                            .map(|e| match e {
                                nm_core::sheets::PathEl::Move(x, y) => E::Move(p((x, y))),
                                nm_core::sheets::PathEl::Line(x, y) => E::Line(p((x, y))),
                                nm_core::sheets::PathEl::Curve(a, b, c) => E::Cubic(p(a), p(b), p(c)),
                                nm_core::sheets::PathEl::Close => E::Close,
                            })
                            .collect();
                        Glyph::Path { path: nm_core::kn_model::Path { els }, w, h }
                    }
                    None => Glyph::Label,
                },
            };
            AppShape { key: format!("tile:{name}"), name: name.to_string(), glyph }
        })
        .collect();
    let mine = crate::myshapes::list().into_iter().map(|(name, _)| AppShape { key: format!("mine:{name}"), name, glyph: Glyph::Label }).collect();
    (basic, mine)
}

/// Inserts what the Content Hub returned.
pub(crate) fn insert_from_hub(app: &mut App, p: kn_ui::contenthub::Picked) {
    use kn_ui::contenthub::Picked;
    let mine = |name: &str| crate::myshapes::list().into_iter().find(|m| m.0 == name).map(|m| m.1);
    match p {
        Picked::Image(pic) => {
            crate::media::insert_picture(app, pic.bytes, &pic.name);
        }
        Picked::Library(s) => {
            let (ps, w, h) = (s.path_source(), s.w, s.h);
            app.insert_shape_then(3, move |d, hist, id| d.set_shape_path_source(hist, id, ps, w, h));
        }
        Picked::App(key) => {
            if let Some(name) = key.strip_prefix("tile:")
                && let Some(t) = SHAPE_TILES.iter().find(|t| **t == name)
            {
                insert_tile(app, t);
            } else if let Some(f) = key.strip_prefix("mine:").and_then(mine) {
                crate::myshapes::insert(app, &f);
            }
        }
        Picked::Remove(key) => {
            if let Some(f) = key.strip_prefix("mine:").and_then(mine) {
                crate::myshapes::remove(&f);
                app.hub.refresh_shapes();
            }
        }
    }
}

/// Inserts one of the Shape popover's tiles.
fn insert_tile(app: &mut App, name: &'static str) {
    use nm_core::kn_model::insert::BasicShape;
    match name {
        "Text" => app.insert_text_box(),
        "Line" => app.insert_shape(0),
        "Rounded Rectangle" => app.insert_shape(1),
        "Circle" => app.insert_shape(2),
        "Square" => app.insert_shape(3),
        "Arrow" => app.insert_shape(4),
        "Triangle" | "Pentagon" | "Diamond" => {
            let b = match name {
                "Triangle" => BasicShape::Triangle,
                "Pentagon" => BasicShape::Pentagon,
                _ => BasicShape::Diamond,
            };
            app.insert_shape_then(3, |d, h, id| {
                nm_core::sheets::set_basic_shape(d, h, id, b);
            });
        }
        // outlines of our own: a square (or, for the curve, a line) given the path
        _ => {
            let Some((els, size)) = nm_core::sheets::extra_shape(name) else { return };
            let curve = name == "Curve";
            app.insert_shape_then(3, |d, h, id| {
                if let Some(mut g) = d.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry) {
                    g.w = size.0;
                    g.h = size.1;
                    d.set_frame(h, id, g);
                }
                nm_core::sheets::set_path(d, h, id, &els, size);
                // the curve is a line: no fill, a 1 pt black stroke
                if curve {
                    d.set_fill(h, id, &nm_core::kn_model::format::FillSpec::None);
                    let st = nm_core::kn_model::format::StrokeSpec { color: nm_core::kn_model::Color::BLACK, width: 1.0, dash: nm_core::kn_model::format::Dash::Solid };
                    d.set_stroke(h, id, Some(&st));
                }
            });
        }
    }
}

/// Minimize the window, except while NM_SCRIPT drives it (a minimized window gets no frames,
/// so the script would never finish).
pub fn minimize(ctx: &egui::Context) {
    if std::env::var_os("NM_SCRIPT").is_none() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}
