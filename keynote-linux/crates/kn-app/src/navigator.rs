//! Slide navigator (left sidebar), Light Table and Outline views.

use egui::{pos2, vec2, Color32, Rect, RichText, Sense, Stroke, StrokeKind};

use crate::app::App;
use crate::textures::Budget;
use crate::theme;

/// Draw a slide thumbnail into `rect` (renders it if the frame budget allows).
fn thumb(app: &mut App, ui: &egui::Ui, index: usize, rect: Rect, budget: &mut Budget) {
    let painter = ui.painter();
    let Some(info) = app.slides.get(index).cloned() else { return };
    let rev = app.slide_rev(info.slide);
    let ctx = ui.ctx().clone();
    // a finished thumbnail: no scene needed
    let tex = match app.tex.cached_thumb(info.slide, rev) {
        Some(t) => Some(t),
        // the background worker will have it shortly (and gets it first)
        None if app.thumb_worker.as_ref().is_some_and(|w| w.revs.get(&info.slide) == Some(&rev) && w.pending(info.slide)) => {
            if let Some(w) = app.thumb_worker.as_ref() {
                w.prioritize(info.slide);
            }
            app.tex.any_thumb(info.slide)
        }
        None => {
            let scene = app.scene(index);
            let doc = app.doc.as_ref().unwrap();
            app.tex.thumb(&ctx, &mut app.renderer, doc, || scene, info.slide, rev, budget)
        }
    };
    match tex {
        Some(t) => {
            painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.rect_filled(rect, 2.0, crate::theme::gray(60));
            ctx.request_repaint();
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let mut budget = Budget::new(8);
    let width = ui.available_width();
    let thumb_w = (width - 44.0).max(60.0);
    let aspect = app.doc.as_ref().map(|d| d.slide_size()).map(|s| s.h / s.w).unwrap_or(9.0 / 16.0);
    let thumb_h = thumb_w * aspect;
    let mut clicked: Option<(usize, u8)> = None;
    let mut ctx_action: Option<(usize, &'static str)> = None;
    let mut rows: Vec<(usize, egui::Rect)> = Vec::new();
    let mut drag_from: Option<usize> = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(8.0);
        if app.editing_layouts.is_some() {
            ui.label(RichText::new("  Slide Layouts").strong().color(theme::TEXT_DIM()));
        }
        let n = app.slides.len();
        // slides inside a collapsed group are hidden
        let mut hidden_below: Option<u32> = None;
        for i in 0..n {
            let d = app.slides[i].depth;
            if let Some(h) = hidden_below {
                if d > h {
                    continue;
                }
                hidden_below = None;
            }
            let has_children = app.slides.get(i + 1).is_some_and(|s| s.depth > d);
            if has_children && app.slides[i].collapsed {
                hidden_below = Some(d);
            }
            let skipped = app.slides[i].skipped;
            let depth = app.slides[i].depth as f32;
            let h = if skipped { 14.0 } else { thumb_h + 10.0 };
            let (row, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click_and_drag());
            rows.push((i, row));
            if resp.drag_started() {
                drag_from = Some(i);
            }
            if i == app.cur && app.nav_scrolled_to != Some(i) {
                ui.scroll_to_rect(row, Some(egui::Align::Center));
                app.nav_scrolled_to = Some(i);
            }
            if !ui.is_rect_visible(row) {
                continue;
            }
            let selected = app.slide_sel.contains(&i);
            if selected {
                // Keynote: a mid-grey row in dark mode, a light-grey one in light mode
                let fill = if theme::is_light() { crate::theme::tint(Color32::from_gray(0xd2)) } else { crate::theme::gray(0x3a) };
                ui.painter().rect_filled(row.shrink2(vec2(4.0, 0.0)), 6.0, fill);
            }
            let x0 = row.min.x + 30.0 + depth * 12.0;
            let num_color = if selected { theme::TEXT() } else { theme::TEXT_DIM() };
            ui.painter().text(
                pos2(row.min.x + 24.0 + depth * 12.0, row.max.y - 6.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{}", i + 1),
                egui::FontId::proportional(11.0),
                num_color,
            );
            if skipped {
                let r = Rect::from_min_size(pos2(x0, row.min.y + 3.0), vec2(thumb_w - depth * 12.0, 8.0));
                ui.painter().rect_filled(r, 2.0, crate::theme::gray(140));
            } else {
                let r = Rect::from_min_size(pos2(x0, row.min.y + 5.0), vec2(thumb_w - depth * 12.0, thumb_h - depth * 12.0 * aspect));
                let t0 = std::time::Instant::now();
                thumb(app, ui, i, r, &mut budget);
                if std::env::var_os("KN_PERF").is_some() && t0.elapsed().as_millis() >= 3 {
                    eprintln!("perf: thumb {i} took {:?}", t0.elapsed());
                }
                // Keynote marks the current slide only with the row's grey background
                let stroke = Stroke::new(1.0, crate::theme::gray(70));
                ui.painter().rect_stroke(r, 2.0, stroke, StrokeKind::Outside);
                // blue corner: slide has a transition or builds (as in Keynote)
                if let Some(sc) = app.scene(i) {
                    if sc.transition.is_some() || !sc.builds.is_empty() {
                        let c = r.right_bottom();
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![c, c + vec2(-12.0, 0.0), c + vec2(0.0, -12.0)],
                            theme::ACCENT(),
                            Stroke::NONE,
                        ));
                    }
                }
            }
            let mut toggled = false;
            if has_children {
                let tri = Rect::from_center_size(pos2(row.min.x + 9.0 + depth * 12.0, row.min.y + 12.0), vec2(14.0, 14.0));
                let collapsed = app.slides[i].collapsed;
                crate::theme::centered(ui.painter(), tri.center(), if collapsed { "▶" } else { "▼" }, egui::FontId::proportional(10.0), theme::TEXT_DIM());
                if resp.clicked() && ui.input(|inp| inp.pointer.interact_pos()).is_some_and(|p| tri.contains(p)) {
                    ctx_action = Some((i, if collapsed { "Expand" } else { "Collapse" }));
                    toggled = true;
                }
            }
            if resp.clicked() && !toggled {
                // Ctrl (Keynote's ⌘) adds or removes a slide, Shift selects a run of slides
                clicked = Some((i, ui.input(|i| if i.modifiers.command { 1 } else if i.modifiers.shift { 2 } else { 0 })));
            }
            if resp.double_clicked() {
                ctx_action = Some((i, "Play"));
            }
            if resp.secondary_clicked() {
                // right-click acts on that slide (and the others selected with it)
                if !app.slide_sel.contains(&i) {
                    app.go_to(i);
                }
                app.nav_focus = true;
                app.sel.clear();
            }
            resp.context_menu(|ui| {
                ui.set_min_width(220.0);
                for (label, cmd) in [("Cut", "Cut"), ("Copy", "Copy"), ("Paste", "Paste")] {
                    crate::menus::context_item(app, ui, label, cmd);
                }
                ui.separator();
                crate::menus::context_item(app, ui, "New Slide", "New Slide");
                crate::menus::context_item(app, ui, "Duplicate", "Duplicate Slide");
                crate::menus::context_item(app, ui, "Delete", "Delete Slide");
                ui.separator();
                let skip = if skipped { "Don't Skip Slide" } else { "Skip Slide" };
                crate::menus::context_item(app, ui, skip, skip);
                ui.separator();
                if ui.button("Play from This Slide").clicked() {
                    ctx_action = Some((i, "Play"));
                    ui.close();
                }
            });
        }
        ui.add_space(40.0);
        // drag to reorder: show where the slide will land; dragging right or left also sets how
        // deep it sits (Keynote's slide groups: at most one level below the slide above it)
        if drag_from.is_some() {
            app.nav_drag = drag_from;
            app.nav_drag_x = ui.ctx().pointer_interact_pos().map(|p| p.x).unwrap_or(0.0);
        }
        if let (Some(from), Some(p)) = (app.nav_drag, ui.ctx().pointer_interact_pos()) {
            const STEP: f32 = 24.0;
            let n = app.slides.len();
            let target = rows.iter().find(|(_, r)| p.y < r.center().y).map(|(i, _)| *i).unwrap_or(n);
            // where it lands, and the slide that will sit above it
            let to = if target > from { target - 1 } else { target };
            let above = (0..target).rev().find(|i| *i != from);
            let max_depth = above.and_then(|i| app.slides.get(i)).map(|s| s.depth + 1).unwrap_or(0);
            let own = app.slides.get(from).map(|s| s.depth).unwrap_or(0) as f32;
            let depth = (own + ((p.x - app.nav_drag_x) / STEP).round()).clamp(0.0, max_depth as f32) as u32;
            if let Some((_, r)) = rows.iter().find(|(i, _)| *i == target.min(n.saturating_sub(1))) {
                let y = if target >= n { r.max.y } else { r.min.y };
                let x0 = r.min.x + 26.0 + depth as f32 * 12.0;
                ui.painter().circle_stroke(egui::pos2(x0, y), 4.0, Stroke::new(2.0, theme::ACCENT()));
                ui.painter().hline(x0 + 4.0..=r.max.x - 6.0, y, Stroke::new(3.0, theme::ACCENT()));
            }
            if ui.input(|i| i.pointer.any_released()) {
                app.nav_drag = None;
                let moved = to != from;
                let deeper = app.slides.get(from).is_some_and(|s| s.depth != depth);
                if moved || deeper {
                    if let Some(doc) = app.doc.as_mut() {
                        doc.begin(&mut app.hist, if moved { "Move Slide" } else if depth > own as u32 { "Indent Slide" } else { "Outdent Slide" });
                        let ok = if moved { doc.move_slide(&mut app.hist, from, to).is_ok() } else { true };
                        if ok {
                            doc.set_slide_depth(&mut app.hist, to, depth);
                        }
                        doc.end(&mut app.hist);
                        if ok {
                            app.bump_all();
                            app.go_to(to);
                        }
                    }
                }
            }
        }
    });
    if clicked.is_some() || ctx_action.is_some() {
        app.nav_focus = true;
    }
    // Tab / Shift-Tab indent and outdent the selected slides (Keynote's slide groups)
    if app.nav_focus && app.canvas.editing.is_none() && !ui.ctx().egui_wants_keyboard_input() {
        let (tab, shift) = ui.input(|i| (i.key_pressed(egui::Key::Tab), i.modifiers.shift));
        if tab {
            let mut sel = app.slide_sel.clone();
            sel.sort_unstable();
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, if shift { "Outdent Slide" } else { "Indent Slide" });
                for i in sel {
                    let d = doc.slides().get(i).map(|s| s.depth).unwrap_or(0);
                    let to = if shift { d.saturating_sub(1) } else { d + 1 };
                    doc.set_slide_depth(&mut app.hist, i, to);
                }
                doc.end(&mut app.hist);
            }
            app.reload_slides();
        }
    }
    if let Some((i, mode)) = clicked {
        app.commit_notes();
        match mode {
            1 => {
                if let Some(k) = app.slide_sel.iter().position(|s| *s == i) {
                    if app.slide_sel.len() > 1 {
                        app.slide_sel.remove(k);
                        if app.cur == i {
                            app.cur = app.slide_sel[app.slide_sel.len() - 1];
                        }
                    }
                } else {
                    app.slide_sel.push(i);
                    app.cur = i;
                }
                app.sel.clear();
            }
            2 => {
                let (a, b) = (app.cur.min(i), app.cur.max(i));
                for k in a..=b {
                    if !app.slide_sel.contains(&k) {
                        app.slide_sel.push(k);
                    }
                }
                app.cur = i;
                app.sel.clear();
            }
            _ => app.go_to(i),
        }
    }
    if let Some((i, a)) = ctx_action {
        if a == "Play" {
            app.start_slideshow(i);
        } else {
            if !app.slide_sel.contains(&i) {
                app.go_to(i);
            }
            let cmd = match a {
                "Duplicate" => "Duplicate Slide",
                "Delete" => "Delete Slide",
                other => other,
            };
            app.command(ui.ctx(), cmd);
        }
    }
    // keep thumbnails coming while some are still missing
    if !budget.left() {
        ui.ctx().request_repaint();
    }
}

pub fn light_table(app: &mut App, ui: &mut egui::Ui) {
    let mut budget = Budget::new(10);
    let aspect = app.doc.as_ref().map(|d| d.slide_size()).map(|s| s.h / s.w).unwrap_or(9.0 / 16.0);
    let tw = 240.0;
    let th = tw * aspect;
    let cols = ((ui.available_width() - 20.0) / (tw + 24.0)).floor().max(1.0) as usize;
    let mut go = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(16.0);
        let n = app.slides.len();
        for row in 0..n.div_ceil(cols) {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                for c in 0..cols {
                    let i = row * cols + c;
                    if i >= n {
                        break;
                    }
                    let (r, resp) = ui.allocate_exact_size(vec2(tw + 24.0, th + 30.0), Sense::click());
                    let img = Rect::from_min_size(r.min + vec2(12.0, 4.0), vec2(tw, th));
                    if ui.is_rect_visible(r) {
                        thumb(app, ui, i, img, &mut budget);
                        let st = if i == app.cur { Stroke::new(3.0, theme::ACCENT()) } else { Stroke::new(1.0, crate::theme::gray(70)) };
                        ui.painter().rect_stroke(img, 2.0, st, StrokeKind::Outside);
                        if app.slides[i].skipped {
                            ui.painter().rect_filled(img, 2.0, Color32::from_black_alpha(140));
                        }
                        crate::theme::centered(ui.painter(), img.center_bottom() + vec2(0.0, 14.0), format!("{}", i + 1), egui::FontId::proportional(12.0), theme::TEXT_DIM());
                    }
                    if resp.clicked() {
                        // Ctrl (Keynote's ⌘) adds or removes a slide from the selection
                        if ui.input(|inp| inp.modifiers.command) {
                            if let Some(k) = app.slide_sel.iter().position(|s| *s == i) {
                                if app.slide_sel.len() > 1 {
                                    app.slide_sel.remove(k);
                                }
                            } else {
                                app.slide_sel.push(i);
                                app.cur = i;
                            }
                        } else {
                            app.go_to(i);
                        }
                    }
                    if resp.double_clicked() {
                        go = Some(i);
                    }
                }
            });
        }
    });
    if let Some(i) = go {
        app.go_to(i);
        app.view = crate::app::ViewMode::Navigator;
    }
    if !budget.left() {
        ui.ctx().request_repaint();
    }
}

/// Outline view: each slide's title and body text, editable in place (like Keynote's outline).
pub fn outline(app: &mut App, ui: &mut egui::Ui) {
    let mut go = None;
    let mut commits: Vec<(usize, kn_format::ObjId, String)> = Vec::new();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.set_max_width(900.0);
        for i in 0..app.slides.len() {
            let Some(sc) = app.scene(i) else { continue };
            // title placeholder first, then body, then every other text box on the slide
            let mut texts: Vec<(u8, kn_format::ObjId, String)> = Vec::new();
            for d in &sc.drawables {
                if d.from_template {
                    continue;
                }
                if let kn_model::DrawableKind::Shape(s) = &d.kind {
                    if let Some(t) = &s.text {
                        let rank = match d.placeholder {
                            Some(kn_model::PlaceholderKind::Title) => 0,
                            Some(kn_model::PlaceholderKind::Body) => 1,
                            Some(kn_model::PlaceholderKind::SlideNumber) => continue,
                            _ => 2,
                        };
                        if rank < 2 || !t.text.trim().is_empty() {
                            texts.push((rank, t.storage, t.text.clone()));
                        }
                    }
                }
            }
            texts.sort_by_key(|t| t.0);
            let title = (!texts.is_empty()).then(|| texts.remove(0)).map(|(_, st, t)| (st, t));
            let selected = i == app.cur;
            ui.horizontal(|ui| {
                let num = ui.add(egui::Label::new(RichText::new(format!("{:>3}", i + 1)).color(theme::TEXT_DIM()).monospace()).sense(Sense::click()));
                let icon = ui.add(egui::Label::new(RichText::new("▭").color(if selected { theme::ACCENT() } else { theme::TEXT_DIM() })).sense(Sense::click()));
                if num.clicked() || icon.clicked() {
                    go = Some(i);
                }
                match &title {
                    Some((st, text)) => {
                        let buf = app.outline_bufs.entry(*st).or_insert_with(|| text.clone());
                        let r = ui.add(egui::TextEdit::singleline(buf).font(egui::FontId::proportional(17.0)).frame(egui::Frame::NONE).desired_width(f32::INFINITY).hint_text("Title"));
                        if r.gained_focus() {
                            go = Some(i);
                        }
                        if r.lost_focus() {
                            commits.push((i, *st, buf.clone()));
                        }
                    }
                    None => {
                        ui.label(RichText::new("(no title)").color(theme::TEXT_DIM()).italics());
                    }
                }
            });
            for (_, st, text) in &texts {
                ui.horizontal(|ui| {
                    ui.add_space(64.0);
                    let buf = app.outline_bufs.entry(*st).or_insert_with(|| text.clone());
                    let r = ui.add(egui::TextEdit::multiline(buf).frame(egui::Frame::NONE).desired_rows(1).desired_width(f32::INFINITY).hint_text("Text"));
                    if r.gained_focus() {
                        go = Some(i);
                    }
                    if r.lost_focus() {
                        commits.push((i, *st, buf.clone()));
                    }
                });
            }
            ui.add_space(8.0);
        }
    });
    for (i, st, text) in commits {
        app.outline_bufs.remove(&st);
        let Some(doc) = app.doc.as_mut() else { continue };
        doc.begin(&mut app.hist, "Typing");
        let changed = doc.set_text_by_diff(&mut app.hist, st, &text);
        doc.end(&mut app.hist);
        if changed {
            if let Some(s) = app.slides.get(i).map(|s| s.slide) {
                app.bump(s);
            }
        }
    }
    if let Some(i) = go {
        if i != app.cur {
            app.cur = i;
            app.slide_sel = vec![i];
        }
    }
}
