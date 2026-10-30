//! View > Show Object List: the current slide's objects, front-most first (as in Keynote).
//! Click selects (Shift/Ctrl adds); the square button locks (filled) / unlocks.

use kn_model::{Drawable, DrawableKind};

use crate::app::App;
use crate::theme;

fn label(d: &Drawable) -> String {
    match &d.kind {
        DrawableKind::Shape(s) => match s.text.as_ref().map(|t| t.text.trim()).filter(|t| !t.is_empty()) {
            Some(t) => {
                let first: String = t.lines().next().unwrap_or("").chars().take(28).collect();
                format!("Text  “{first}”")
            }
            None if s.is_line => "Line".into(),
            None => "Shape".into(),
        },
        DrawableKind::Image(_) => "Image".into(),
        DrawableKind::Movie(m) => if m.audio_only { "Audio" } else { "Movie" }.into(),
        DrawableKind::Group(g) => format!("Group ({} objects)", g.len()),
        DrawableKind::Table(_) => "Table".into(),
        DrawableKind::Chart(_) => "Chart".into(),
        #[allow(unreachable_patterns)]
        _ => "Object".into(),
    }
}

/// Docked between the navigator and the canvas (Keynote 15.4): one row per object, front-most
/// first — icon, name (text objects show their text; empty placeholders their role, in italics) —
/// and a Search field at the bottom.
pub fn panel(app: &mut App, ui: &mut egui::Ui) {
    let Some(sc) = app.scene(app.cur) else { return };
    let mut pick: Option<(kn_format::ObjId, bool)> = None;
    let mut lock: Option<(kn_format::ObjId, bool)> = None;
    let search_h = 40.0;
    let list_h = (ui.available_height() - search_h).max(40.0);
    let query = app.object_list_query.to_lowercase();
    egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(8.0);
        let w = ui.available_width();
        for d in sc.drawables.iter().filter(|d| !d.from_template).rev() {
            let empty_ph = d.placeholder.is_some() && matches!(&d.kind, DrawableKind::Shape(s) if s.text.as_ref().is_none_or(|t| t.text.trim().is_empty()));
            let name = if empty_ph {
                match d.placeholder {
                    Some(kn_model::PlaceholderKind::Title) => "Title".to_string(),
                    Some(kn_model::PlaceholderKind::Body) => "Body".to_string(),
                    Some(kn_model::PlaceholderKind::SlideNumber) => "Slide Number".to_string(),
                    _ => "Placeholder".to_string(),
                }
            } else if d.placeholder == Some(kn_model::PlaceholderKind::SlideNumber) {
                "Slide Number".to_string()
            } else {
                match &d.kind {
                    DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.text.trim().lines().next().unwrap_or("").to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| label(d)),
                    _ => label(d),
                }
            };
            if !query.is_empty() && !name.to_lowercase().contains(&query) {
                continue;
            }
            let (row, resp) = ui.allocate_exact_size(egui::vec2(w, 32.0), egui::Sense::click());
            let p = ui.painter();
            let selected = app.sel.contains(&d.id);
            if selected {
                p.rect_filled(row.shrink2(egui::vec2(6.0, 1.0)), 6.0, egui::Color32::from_rgb(0x2a, 0x55, 0x9e));
            } else if resp.hovered() {
                p.rect_filled(row.shrink2(egui::vec2(6.0, 1.0)), 6.0, crate::theme::gray(0x33));
            }
            // icon: text frame "A", list lines for bodies, a picture, a shape
            let ic = egui::Rect::from_center_size(egui::pos2(row.min.x + 30.0, row.center().y), egui::vec2(20.0, 16.0));
            let col = crate::theme::gray(0xc8);
            let st = egui::Stroke::new(1.2, col);
            match (&d.kind, d.placeholder) {
                (_, Some(kn_model::PlaceholderKind::Body)) => {
                    for k in 0..3 {
                        let y = ic.min.y + 3.0 + k as f32 * 5.0;
                        p.circle_filled(egui::pos2(ic.min.x + 2.0, y), 1.2, col);
                        p.line_segment([egui::pos2(ic.min.x + 6.0, y), egui::pos2(ic.max.x, y)], st);
                    }
                }
                (DrawableKind::Shape(s), _) if s.text.is_some() => {
                    p.rect_stroke(ic, 2.0, st, egui::StrokeKind::Middle);
                    crate::theme::centered(&p, ic.center(), "A", egui::FontId::proportional(10.0), col);
                }
                (DrawableKind::Image(_) | DrawableKind::Movie(_), _) => {
                    p.rect_stroke(ic, 2.0, st, egui::StrokeKind::Middle);
                    p.line(vec![egui::pos2(ic.min.x + 2.0, ic.max.y - 2.0), egui::pos2(ic.min.x + 8.0, ic.min.y + 7.0), egui::pos2(ic.max.x - 2.0, ic.max.y - 2.0)], st);
                }
                _ => {
                    p.rect_filled(ic.shrink(1.0), 3.0, egui::Color32::from_rgb(0x4a, 0x9c, 0xf5));
                }
            }
            let font = egui::FontId::proportional(14.0);
            let text = if name.chars().count() > 22 { format!("{}…", name.chars().take(21).collect::<String>()) } else { name };
            if empty_ph || d.placeholder == Some(kn_model::PlaceholderKind::SlideNumber) {
                // italic for roles (no italic face loaded: dim instead)
                p.text(egui::pos2(ic.max.x + 12.0, row.center().y), egui::Align2::LEFT_CENTER, text, font, theme::TEXT_DIM());
            } else {
                p.text(egui::pos2(ic.max.x + 12.0, row.center().y), egui::Align2::LEFT_CENTER, text, font, theme::TEXT());
            }
            if d.locked {
                crate::theme::centered(&p, egui::pos2(row.max.x - 16.0, row.center().y), "■", egui::FontId::proportional(10.0), theme::TEXT_DIM());
            }
            if resp.clicked() {
                pick = Some((d.id, ui.input(|i| i.modifiers.shift || i.modifiers.command)));
            }
            resp.context_menu(|ui| {
                if ui.button(if d.locked { "Unlock" } else { "Lock" }).clicked() {
                    lock = Some((d.id, !d.locked));
                    ui.close();
                }
            });
            // a hairline between rows, inset like Keynote's
            p.line_segment([egui::pos2(row.min.x + 50.0, row.max.y), egui::pos2(row.max.x - 10.0, row.max.y)], egui::Stroke::new(1.0, crate::theme::gray(0x2e)));
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.add(egui::TextEdit::singleline(&mut app.object_list_query).hint_text("🔍 Search").desired_width(ui.available_width() - 8.0));
    });
    if let Some((id, add)) = pick {
        crate::textedit::commit(app);
        if add {
            if let Some(p) = app.sel.iter().position(|s| *s == id) {
                app.sel.remove(p);
            } else {
                app.sel.push(id);
            }
        } else {
            app.sel = vec![id];
        }
    }
    if let Some((id, locked)) = lock {
        let slide = app.current_slide().map(|s| s.slide);
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, if locked { "Lock" } else { "Unlock" });
            doc.set_locked(&mut app.hist, id, locked);
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}
