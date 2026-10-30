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
    // under the name: "Edited", and a Word file's mode
    let mode = app.word.as_ref().map(|w| w.label());
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
    let path = app.word.as_ref().map(|w| w.path.clone()).or_else(|| app.doc().and_then(|d| d.path.clone()));
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
            let stem = path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.trim_end_matches(".pages").trim_end_matches(".docx").to_string());
            let dir = path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf()).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Documents")));
            app.doc_title = Some((stem, dir));
            return;
        }
    }
    let mut apply = false;
    let mut close = false;
    let mut format_change: Option<(usize, String, Option<std::path::PathBuf>)> = None;
    if let Some((name, dir)) = app.doc_title.clone().as_mut() {
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
                    // Format (as Values' Excel modes): Pages, Word, or Word with the Letters copy
                    ui.label("Format:");
                    let cur = match &app.word {
                        Some(w) if w.hybrid => 2,
                        Some(_) => 1,
                        None => 0,
                    };
                    let names = ["Pages", "Word", "Word + Letters"];
                    let mut pick = cur;
                    egui::ComboBox::from_id_salt("doc-format").width(220.0).selected_text(names[cur]).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut pick, i, *n);
                        }
                    });
                    if pick != cur {
                        format_change = Some((pick, name.clone(), dir.clone()));
                    }
                    ui.end_row();
                });
                let note = match &app.word {
                    Some(w) if w.hybrid => "A Word file anyone can open, with the whole Letters document kept inside it.",
                    Some(_) => "A plain Word file: what Word can't hold isn't kept (those features are greyed).",
                    None => "A Pages document.",
                };
                ui.label(egui::RichText::new(note).small().color(theme::TEXT_DIM()));
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        } else if r.response.clicked_elsewhere() && !resp.clicked() && !egui::Popup::is_any_open(&ctx) {
            // like Numbers, closing the popover keeps the name typed
            apply = true;
        }
        // (the name and folder typed so far)
        if app.doc_title.is_some() {
            app.doc_title = Some((name.clone(), dir.clone()));
        }
    }
    if let Some((pick, name, dir)) = format_change {
        let name = name.trim().trim_end_matches(".pages").trim_end_matches(".docx").to_string();
        match pick {
            // to Word: saved as a .docx beside it now (Word + Letters keeps everything)
            1 | 2 => {
                let hybrid = pick == 2;
                match app.word.as_mut() {
                    Some(w) => {
                        w.hybrid = hybrid;
                        if let Some(d) = app.wb.as_mut() {
                            d.doc.dirty = true;
                        }
                    }
                    None => {
                        let dir = dir.or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Documents")));
                        if let Some(dir) = dir {
                            let target = dir.join(format!("{}.docx", if name.is_empty() { "Untitled" } else { &name }));
                            app.save_word(&target, hybrid);
                        }
                    }
                }
            }
            // to Pages: the next save writes a .pages document
            _ => {
                if app.word.take().is_some() {
                    if let Some(d) = app.wb.as_mut() {
                        d.doc.path = None;
                        d.doc.dirty = true;
                        // laid out as Pages lays it out from now on
                        d.word_layout = false;
                    }
                    app.untitled_name = Some(name);
                    app.notify("Save to keep it as a Pages document.");
                }
            }
        }
    }
    if apply || close {
        if let (true, Some((name, Some(dir)))) = (apply, app.doc_title.take()) {
            let name = name.trim().trim_end_matches(".pages").trim_end_matches(".docx").to_string();
            if !name.is_empty() && !name.contains('/') {
                // a Word file keeps its extension (and its mode)
                if let Some(w) = app.word.clone() {
                    let target = dir.join(format!("{name}.docx"));
                    if target != w.path {
                        app.save_word(&target, w.hybrid);
                        if target.exists() && w.path.exists() {
                            let _ = std::fs::remove_file(&w.path);
                        }
                    }
                } else {
                    crate::fileops::relocate(app, dir.join(format!("{name}.pages")));
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

/// The page-fitting zoom modes of Pages' Zoom menu.
pub const FITS: [&str; 4] = ["One Page", "Two Pages", "Fit Width", "Fit Page"];

pub fn zoom_menu(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    for f in FITS {
        if ui.button(f).clicked() {
            crate::menus::run(app, &ctx, &format!("Zoom > {f}"));
            ui.close();
        }
    }
    ui.separator();
    for z in ZOOMS {
        if ui.radio((app.canvas.zoom - z).abs() < 1e-3, format!("{:.0}%", z * 100.0)).clicked() {
            app.canvas.animate_zoom(z);
            ui.close();
        }
    }
}

/// The toolbar's View menu (Pages: Enter Screen View, the sidebar modes, comments, find, word
/// count, ruler).
pub fn view_menu(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let item = |ui: &mut egui::Ui, app: &mut App, label: &str, cmd: &str, on: Option<bool>| {
        let mut b = egui::Button::new(label);
        if on == Some(true) {
            b = b.selected(true);
        }
        if ui.add(b).clicked() {
            crate::menus::run(app, &ctx, cmd);
            ui.close();
        }
    };
    item(ui, app, if app.screen_view { "Exit Screen View" } else { "Enter Screen View" }, "Enter Screen View", None);
    ui.separator();
    item(ui, app, "Page Thumbnails", "Page Thumbnails", Some(app.sidebar == crate::app::Sidebar::Thumbnails));
    item(ui, app, "Table of Contents", "Table of Contents", Some(app.sidebar == crate::app::Sidebar::Toc));
    item(ui, app, "Document Only", "Document Only", Some(app.sidebar == crate::app::Sidebar::None));
    ui.separator();
    item(ui, app, if app.comments_pane { "Hide Comments Pane" } else { "Show Comments Pane" }, "Show Comments Pane", None);
    item(ui, app, if app.show_comments { "Hide Comments" } else { "Show Comments" }, "Comments & Changes > Hide Comments", None);
    ui.separator();
    item(ui, app, "Show Find & Replace", "Find > Find…", None);
    item(ui, app, if app.word_count { "Hide Word Count" } else { "Show Word Count" }, "Show Word Count", None);
    item(ui, app, if app.ruler { "Hide Ruler" } else { "Show Ruler" }, "Show Ruler", None);
    ui.separator();
    let insp = if app.inspector { "Hide Inspector" } else { "Show Inspector" };
    item(ui, app, insp, "Inspector > Hide Inspector", None);
}

/// The toolbar's Insert menu (Pages: breaks, fields, footnote, equation, link, bookmark, TOC).
pub fn insert_menu(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let item = |ui: &mut egui::Ui, app: &mut App, label: &str, cmd: &str| {
        if ui.add_enabled(crate::menus::enabled(app, cmd), egui::Button::new(label)).clicked() {
            crate::menus::run(app, &ctx, cmd);
            ui.close();
        }
    };
    for (l, c) in [("Page Break", "Page Break"), ("Line Break", "Line Break"), ("Section Break", "Section Break"), ("Column Break", "Column Break")] {
        item(ui, app, l, c);
    }
    ui.separator();
    ui.menu_button("Page Number", |ui| crate::hfnumber::menu_items(app, ui));
    for (l, c) in [("Page Count", "Page Count"), ("Date & Time", "Date & Time")] {
        item(ui, app, l, c);
    }
    ui.separator();
    for (l, c) in [("Footnote", "Footnote"), ("Equation…", "Equation…"), ("Link", "Add Link > Webpage"), ("Bookmark", "Bookmark")] {
        item(ui, app, l, c);
    }
    ui.separator();
    item(ui, app, "Table of Contents", "Table of Contents > Document");
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

/// The toolbar's Table popover (Pages: Headers, Basic, Plain, Sums), drawn as small tables.
pub(crate) fn table_popover(app: &mut App, ui: &mut egui::Ui) {
    let mut pick = None;
    egui::Grid::new("table-popover").spacing([12.0, 12.0]).show(ui, |ui| {
        for (k, name) in ["Headers", "Basic", "Plain", "Sums"].iter().enumerate() {
            let (clicked, r) = tile(ui, 120.0, name);
            let p = ui.painter();
            let (cols, rows) = (4, 5);
            let (cw, rh) = (r.width() / cols as f32, r.height() / rows as f32);
            let head = Color32::from_gray(0xc4);
            p.rect_filled(r, 0.0, Color32::from_gray(0xee));
            let (hr, hc, foot) = match k {
                0 => (true, true, false),
                1 => (true, false, false),
                2 => (false, false, false),
                _ => (true, false, true),
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
            if clicked {
                pick = Some(k);
            }
            if k % 2 == 1 {
                ui.end_row();
            }
        }
    });
    if let Some(k) = pick {
        insert_table_style(app, k);
        ui.close();
    }
}

/// A table in one of the popover's looks: the Basic table with its headers set.
pub fn insert_table_style(app: &mut App, k: usize) {
    crate::insert::insert_object(app, pg_core::objects::NewObject::Table);
    let Some(t) = app.canvas.obj.filter(|o| app.table(*o).is_some()) else { return };
    use pg_core::kn_model::tablerows::HeaderKind;
    let (rows, cols, foot) = match k {
        0 => (1, 1, 0),
        2 => (0, 0, 0),
        3 => (1, 0, 1),
        _ => (1, 0, 0),
    };
    app.edit("Table Style", |d, h| {
        let _ = d.set_table_header_count(h, t, HeaderKind::Rows, rows);
        let _ = d.set_table_header_count(h, t, HeaderKind::Columns, cols);
        let _ = d.set_table_header_count(h, t, HeaderKind::Footer, foot);
    });
}

/// The toolbar's Shape popover: Pages' basic shapes and lines, drawn as the shapes.
pub(crate) fn shape_popover(app: &mut App, ui: &mut egui::Ui) {
    use pg_core::kn_model::insert::BasicShape;
    enum Pick {
        Text,
        Line(u8),
        Shape(BasicShape),
    }
    let mut pick: Option<Pick> = None;
    let fill = kn_ui::contenthub::ink();
    egui::Grid::new("shape-popover").spacing([10.0, 10.0]).show(ui, |ui| {
        // a text box, a line, a line with an arrowhead, then the shapes
        let (c, r) = tile(ui, 64.0, "Text Box");
        crate::chrome::centered_text(ui.painter(), r.center(), "Text", fill);
        if c {
            pick = Some(Pick::Text);
        }
        for arrows in [0u8, 1] {
            let (c, r) = tile(ui, 64.0, if arrows == 0 { "Line" } else { "Line with Arrowhead" });
            let (a, b) = (r.left_bottom(), r.right_top());
            ui.painter().line_segment([a, b], Stroke::new(2.5, fill));
            if arrows == 1 {
                ui.painter().arrow(a, b - a, Stroke::new(2.5, fill));
            }
            if c {
                pick = Some(Pick::Line(arrows));
            }
        }
        let mut k = 3;
        for s in BasicShape::ALL {
            if k % 4 == 0 {
                ui.end_row();
            }
            let (clicked, r) = tile(ui, 64.0, s.name());
            draw_shape(ui.painter(), r, s, fill);
            if clicked {
                pick = Some(Pick::Shape(s));
            }
            k += 1;
        }
    });
    // the shape library and My Shapes are in the Content Hub, as on the Mac
    ui.add_space(8.0);
    if ui.add_sized([ui.available_width(), 30.0], egui::Button::new("Browse Content Hub").fill(kn_ui::contenthub::tile_bg()).corner_radius(15.0)).clicked() {
        ui.close();
        app.hub.show_tab(kn_ui::contenthub::Tab::Shapes);
        return;
    }
    match pick {
        Some(Pick::Text) => crate::insert::insert_object(app, pg_core::objects::NewObject::TextBox),
        Some(Pick::Line(a)) => {
            // (the Line > command inserts the line itself: one line, with or without its arrow)
            let ctx = ui.ctx().clone();
            crate::menus::run(app, &ctx, if a > 0 { "Line > Line with Arrowhead" } else { "Line > Line" });
        }
        Some(Pick::Shape(s)) => crate::insert::insert_object(app, pg_core::objects::NewObject::Shape(s)),
        None => return,
    }
    ui.close();
}

/// The Content Hub's Shapes: Text, the lines and the basic shapes, then My Shapes.
pub(crate) fn hub_shapes() -> (Vec<kn_ui::contenthub::AppShape>, Vec<kn_ui::contenthub::AppShape>) {
    use kn_ui::contenthub::{AppShape, Glyph};
    use pg_core::kn_model::insert::BasicShape;
    let mut basic = vec![
        AppShape { key: "text".into(), name: "Text Box".into(), glyph: Glyph::Text },
        AppShape { key: "line".into(), name: "Line".into(), glyph: Glyph::Line { arrow: false } },
        AppShape { key: "arrow".into(), name: "Line with Arrowhead".into(), glyph: Glyph::Line { arrow: true } },
    ];
    for (i, s) in BasicShape::ALL.iter().enumerate() {
        let path = pg_core::kn_model::path::from_path_source(&s.path_source(), 100.0, 100.0);
        basic.push(AppShape { key: format!("basic:{i}"), name: s.name().to_string(), glyph: Glyph::Path { path, w: 100.0, h: 100.0 } });
    }
    let mine = crate::editing::my_shapes()
        .into_iter()
        .map(|(name, ps, w, h)| AppShape { key: format!("mine:{name}"), name, glyph: Glyph::Path { path: pg_core::kn_model::path::from_path_source(&ps, w, h), w, h } })
        .collect();
    (basic, mine)
}

/// Inserts what the Content Hub returned.
pub(crate) fn insert_from_hub(app: &mut App, ctx: &egui::Context, p: kn_ui::contenthub::Picked) {
    use kn_ui::contenthub::Picked;
    use pg_core::kn_model::insert::BasicShape;
    match p {
        Picked::Image(pic) => crate::insert::insert_image_bytes(app, pic.bytes, &pic.name),
        Picked::Library(s) => crate::editing::insert_path_source(app, s.path_source(), s.w, s.h),
        Picked::App(key) => match key.as_str() {
            "text" => crate::insert::insert_object(app, pg_core::objects::NewObject::TextBox),
            "line" => crate::menus::run(app, ctx, "Line > Line"),
            "arrow" => crate::menus::run(app, ctx, "Line > Line with Arrowhead"),
            k => {
                if let Some(s) = k.strip_prefix("basic:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| BasicShape::ALL.get(i)) {
                    crate::insert::insert_object(app, pg_core::objects::NewObject::Shape(*s));
                } else if let Some(name) = k.strip_prefix("mine:")
                    && let Some((_, ps, w, h)) = crate::editing::my_shapes().into_iter().find(|m| m.0 == name)
                {
                    crate::editing::insert_path_source(app, ps, w, h);
                }
            }
        },
        Picked::Remove(key) => {
            if let Some(name) = key.strip_prefix("mine:") {
                crate::editing::remove_my_shape(name);
                app.hub.refresh_shapes();
            }
        }
    }
}

pub(crate) fn centered_text(p: &egui::Painter, c: egui::Pos2, t: &str, col: Color32) {
    p.text(c, egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(15.0), col);
}

/// A simple polygon (concave allowed) filled by ear clipping (egui fills only convex ones).
fn fill_polygon(p: &egui::Painter, poly: &[egui::Pos2], fill: Color32) {
    let mut pts: Vec<egui::Pos2> = poly.to_vec();
    // drop a closing duplicate and repeats
    pts.dedup_by(|a, b| a.distance(*b) < 0.05);
    if pts.len() > 2 && pts[0].distance(pts[pts.len() - 1]) < 0.05 {
        pts.pop();
    }
    if pts.len() < 3 {
        return;
    }
    let area: f32 = (0..pts.len()).map(|i| {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        a.x * b.y - b.x * a.y
    }).sum();
    let ccw = area > 0.0;
    let cross = |a: egui::Pos2, b: egui::Pos2, c: egui::Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let inside = |p0: egui::Pos2, a: egui::Pos2, b: egui::Pos2, c: egui::Pos2| {
        let (d1, d2, d3) = (cross(a, b, p0), cross(b, c, p0), cross(c, a, p0));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    };
    let mut mesh = egui::Mesh::default();
    for q in &pts {
        mesh.colored_vertex(*q, fill);
    }
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    let mut guard = 0;
    while idx.len() > 3 && guard < 10_000 {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let (ia, ib, ic) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            let c2 = cross(a, b, c);
            if (ccw && c2 <= 0.0) || (!ccw && c2 >= 0.0) {
                continue;
            }
            if idx.iter().any(|&j| j != ia && j != ib && j != ic && inside(pts[j], a, b, c)) {
                continue;
            }
            mesh.add_triangle(ia as u32, ib as u32, ic as u32);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        mesh.add_triangle(idx[0] as u32, idx[1] as u32, idx[2] as u32);
    }
    p.add(egui::Shape::mesh(mesh));
}

/// A preset shape's outline in `r` (the suite's path for it, scaled into the tile), drawn as a
/// thick outline (egui fills only convex polygons; stars and arrows aren't).
fn draw_shape(p: &egui::Painter, r: Rect, s: pg_core::kn_model::insert::BasicShape, fill: Color32) {
    use pg_core::kn_model::PathEl;
    let ps = s.path_source();
    let path = pg_core::kn_model::path::from_path_source(&ps, r.width(), r.height());
    let at = |x: f32, y: f32| pos2(r.min.x + x, r.min.y + y);
    let mut polys: Vec<Vec<egui::Pos2>> = Vec::new();
    let mut cur: Vec<egui::Pos2> = Vec::new();
    let mut last = pos2(0.0, 0.0);
    for el in &path.els {
        match el {
            PathEl::Move(q) => {
                if cur.len() > 1 {
                    polys.push(std::mem::take(&mut cur));
                }
                cur.clear();
                last = pos2(q.x, q.y);
                cur.push(at(q.x, q.y));
            }
            PathEl::Line(q) => {
                last = pos2(q.x, q.y);
                cur.push(at(q.x, q.y));
            }
            PathEl::Quad(c, q) => {
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    cur.push(at(u * u * last.x + 2.0 * u * t * c.x + t * t * q.x, u * u * last.y + 2.0 * u * t * c.y + t * t * q.y));
                }
                last = pos2(q.x, q.y);
            }
            PathEl::Cubic(c1, c2, q) => {
                for i in 1..=10 {
                    let t = i as f32 / 10.0;
                    let u = 1.0 - t;
                    let x = u * u * u * last.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * q.x;
                    let y = u * u * u * last.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * q.y;
                    cur.push(at(x, y));
                }
                last = pos2(q.x, q.y);
            }
            PathEl::Close => {
                if let Some(f) = cur.first().copied() {
                    cur.push(f);
                }
            }
        }
    }
    if cur.len() > 1 {
        polys.push(cur);
    }
    // filled, as Pages / Keydeck draw them (a fan from the centre: the presets are star-shaped
    // around it), each outline once more on top for crisp edges
    for poly in polys {
        fill_polygon(p, &poly, fill);
        p.add(egui::Shape::line(poly, Stroke::new(1.0, fill)));
    }
}

/// Minimize the window, except while CS_SCRIPT drives it (a minimized window gets no frames,
/// so the script would never finish).
pub fn minimize(ctx: &egui::Context) {
    if std::env::var_os("CS_SCRIPT").is_none() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}
