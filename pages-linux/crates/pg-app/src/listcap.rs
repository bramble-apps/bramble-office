//! The Text inspector's Bullets & Lists and Drop Cap sections, as in Pages 15.4:
//! - Bullets & Lists: a button with the list style's name opening "List Styles" (each style
//!   with its label beside the name: •, 1., A., I., –, a picture bullet's ○), then for a list the
//!   label kind (No Bullet / Text Bullet / Image Bullet / Numbers), the bullet character or
//!   number format, tiered numbers, the label's size and the bullet / text indents, for the
//!   caret paragraph's list level.
//! - Drop Cap: the checkbox and a thumbnail button opening "Drop Cap Styles" (the document's
//!   drop cap styles drawn as Pages draws them: the letter, how many lines it takes, raised,
//!   bold, on a shape), then lines, characters, raised lines, size, text wrap and the shape
//!   (fill colour, corner radius).

use egui::{vec2, Color32, Rect, RichText, Sense, Stroke};
use kn_ui::theme;
use pg_core::edit::{DropCapSettings, ListLevel};
use pg_core::kn_format::ObjId;
use pg_core::kn_model;

use crate::app::App;
use crate::textinsp::Target;

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

fn c32(c: kn_model::Color) -> Color32 {
    Color32::from_rgba_unmultiplied((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8)
}

// ------------------------------------------------------------------ bullets & lists

const BULLETS: [&str; 12] = ["•", "◦", "▪", "▫", "■", "□", "–", "-", "✓", "★", "➤", "◆"];
/// (TSWP.ListStyleArchive.NumberType, how it shows)
const NUMBER_FORMATS: [(u64, &str); 12] = [
    (0, "1. 2. 3."),
    (2, "1) 2) 3)"),
    (1, "(1) (2) (3)"),
    (3, "I. II. III."),
    (5, "I) II) III)"),
    (6, "i. ii. iii."),
    (8, "i) ii) iii)"),
    (9, "A. B. C."),
    (11, "A) B) C)"),
    (12, "a. b. c."),
    (14, "a) b) c)"),
    (13, "(a) (b) (c)"),
];

pub fn bullets_section(app: &mut App, ui: &mut egui::Ui, t: &Target) {
    let (list, level) = app.doc().map(|d| pg_core::edit::list_at(d, t.storage, t.a)).unwrap_or((None, 0));
    let lists = app.doc().map(pg_core::edit::list_styles).unwrap_or_default();
    let name = list.and_then(|id| app.doc().map(|d| pg_core::edit::named_style(d, Some(id)).1)).unwrap_or_else(|| "None".into());
    let open = theme::disclosure(ui, "bullets-lists", "Bullets & Lists", None);
    // the list style button and its List Styles popover (shown whether or not the section is open)
    let mut pick: Option<Option<ObjId>> = None;
    let markers: Vec<(ObjId, String, String)> = lists.iter().map(|(id, n)| (*id, n.clone(), app.doc().map(|d| pg_core::edit::list_marker(d, *id)).unwrap_or_default())).collect();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let b = ui.add(egui::Button::new(RichText::new(format!("{name}  ⌄"))).min_size(vec2(150.0, 24.0)));
            egui::Popup::from_toggle_button_response(&b).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(230.0);
                ui.vertical_centered(|ui| ui.label(theme::bold("List Styles")));
                ui.separator();
                for (id, n, mark) in &markers {
                    let base = list.and_then(|l| app.doc().map(|d| pg_core::edit::named_style(d, Some(l)).1));
                    let on = base.as_deref() == Some(n.as_str()) || (list.is_none() && n == "None");
                    let (r, resp) = ui.allocate_exact_size(vec2(230.0, 26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 4.0, theme::ACCENT().gamma_multiply(0.35));
                    }
                    let p = ui.painter();
                    if on {
                        p.text(r.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, "✓", egui::FontId::proportional(13.0), theme::TEXT());
                    }
                    p.text(r.left_center() + vec2(58.0, 0.0), egui::Align2::RIGHT_CENTER, mark, egui::FontId::proportional(13.0), theme::TEXT());
                    p.text(r.left_center() + vec2(68.0, 0.0), egui::Align2::LEFT_CENTER, n, egui::FontId::proportional(13.0), theme::TEXT());
                    if resp.clicked() {
                        pick = Some(if n == "None" { None } else { Some(*id) });
                        ui.close();
                    }
                }
            });
        });
    });
    if let Some(l) = pick {
        let st = t.storage;
        app.edit("List", |d, h| pg_core::edit::set_list_style(d, h, st, t.a, t.b, l));
        return;
    }
    let Some(list) = list.filter(|_| open && name != "None") else { return };
    let cur = app.doc().map(|d| pg_core::edit::list_level(d, list, level)).unwrap_or(ListLevel { kind: 0, string: "•".into(), number: 0, indent: 0.0, text_indent: 18.0, scale: 1.0, tiered: false });
    let mut change: Option<ListLevel> = None;
    let mut next = cur.clone();
    let kinds = [(0u64, "No Bullet"), (2, "Text Bullet"), (1, "Image Bullet"), (3, "Numbers")];
    let kind_name = kinds.iter().find(|k| k.0 == cur.kind).map(|k| k.1).unwrap_or("No Bullet");
    row(ui, "", |ui| {
        theme::combo("list-kind").selected_text(kind_name).width(150.0).show_ui(ui, |ui| {
            for (k, n) in kinds {
                if ui.selectable_label(k == cur.kind, n).clicked() {
                    next.kind = k;
                    change = Some(next.clone());
                }
            }
        });
    });
    match cur.kind {
        2 => {
            row(ui, "Bullet", |ui| {
                theme::combo("list-bullet").selected_text(&cur.string).width(80.0).show_ui(ui, |ui| {
                    egui::Grid::new("bullet-grid").show(ui, |ui| {
                        for (i, b) in BULLETS.iter().enumerate() {
                            if ui.selectable_label(cur.string == *b, RichText::new(*b).size(16.0)).clicked() {
                                next.string = b.to_string();
                                change = Some(next.clone());
                            }
                            if i % 4 == 3 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
        }
        3 => {
            let fmt_name = NUMBER_FORMATS.iter().find(|f| f.0 == cur.number).map(|f| f.1).unwrap_or("1. 2. 3.");
            row(ui, "Format", |ui| {
                theme::combo("list-number").selected_text(fmt_name).width(120.0).show_ui(ui, |ui| {
                    for (k, n) in NUMBER_FORMATS {
                        if ui.selectable_label(k == cur.number, n).clicked() {
                            next.number = k;
                            change = Some(next.clone());
                        }
                    }
                });
            });
            let mut tiered = cur.tiered;
            if theme::checkbox(ui, &mut tiered, "Tiered Numbers").changed() {
                next.tiered = tiered;
                change = Some(next.clone());
            }
        }
        _ => {}
    }
    if cur.kind != 0 {
        let mut size = f64::from(cur.scale * 100.0);
        row(ui, "Size", |ui| {
            if theme::stepper(ui, &mut size, 5.0, 10.0..=400.0, " %", 0) {
                next.scale = size as f32 / 100.0;
                change = Some(next.clone());
            }
        });
    }
    ui.label(theme::bold("Indents"));
    let (mut bi, mut ti) = (f64::from(cur.indent), f64::from(cur.text_indent));
    row(ui, "Bullet", |ui| {
        if theme::stepper(ui, &mut bi, 1.0, 0.0..=500.0, " pt", 0) {
            next.indent = bi as f32;
            change = Some(next.clone());
        }
    });
    row(ui, "Text", |ui| {
        if theme::stepper(ui, &mut ti, 1.0, 0.0..=500.0, " pt", 0) {
            next.text_indent = ti as f32;
            change = Some(next.clone());
        }
    });
    if let Some(n) = change {
        let st = t.storage;
        app.edit("List", |d, h| pg_core::edit::set_list_level(d, h, st, t.a, t.b, level, &|l| *l = n.clone()));
    }
}

// ------------------------------------------------------------------ drop caps

/// A drop cap style as Pages' thumbnails show it: a big "A" beside grey text lines, as many
/// lines tall as it takes (raised ones above), bold or not, white on a shape when it has one.
fn paint_thumb(p: &egui::Painter, r: Rect, s: &DropCapSettings) {
    p.rect_filled(r, 4.0, Color32::from_gray(0xf2));
    let inner = r.shrink2(vec2(7.0, 6.0));
    let rows = 5usize;
    let lh = inner.height() / rows as f32;
    let lines = s.lines.clamp(1, 4) as usize;
    let raised = (s.raised as usize).min(lines.saturating_sub(1));
    let cap_h = lh * lines as f32 - lh * 0.3;
    let font = egui::FontId::new((cap_h / 0.72 * s.scale.clamp(0.4, 1.2)).max(6.0), if s.bold.unwrap_or(lines >= 3) { egui::FontFamily::Name("bold".into()) } else { egui::FontFamily::Proportional });
    let ink = if s.shape { s.color.map(c32).unwrap_or(Color32::WHITE) } else { s.color.map(c32).unwrap_or(Color32::BLACK) };
    let g = p.layout_no_wrap("A".into(), font, ink);
    let cap_top = inner.min.y - raised as f32 * lh * 0.0 + if raised > 0 { 0.0 } else { lh * 0.15 };
    let pad = if s.shape { 3.0 } else { 0.0 };
    let cap_w = g.size().x + 2.0 * pad;
    if s.shape {
        let fill = s.fill.map(c32).unwrap_or(Color32::BLACK);
        let box_r = Rect::from_min_size(egui::pos2(inner.min.x, cap_top), vec2(cap_w, cap_h + lh * 0.3));
        p.rect_filled(box_r, (s.corner_radius * box_r.width().min(box_r.height())).clamp(0.0, 6.0), fill);
    }
    p.galley(egui::pos2(inner.min.x + pad, cap_top + (cap_h + lh * 0.3 - g.size().y) / 2.0 + if s.shape { 0.0 } else { lh * 0.05 }), g, ink);
    // the text lines: short beside the cap, full width below it
    let grey = Color32::from_gray(0xa8);
    for i in 0..rows {
        let y = inner.min.y + lh * (i as f32 + 0.5);
        let beside = i < lines.saturating_sub(raised).max(1) + raised && i < lines;
        let x0 = if beside && !(raised > 0 && i < raised) { inner.min.x + cap_w + 4.0 } else if beside { inner.min.x + cap_w + 4.0 } else { inner.min.x };
        p.rect_filled(Rect::from_min_max(egui::pos2(x0, y - lh * 0.18), egui::pos2(inner.max.x, y + lh * 0.18)), 1.0, grey);
    }
}

pub fn drop_cap_section(app: &mut App, ui: &mut egui::Ui, t: &Target) {
    let styles = app.doc().map(pg_core::edit::drop_cap_styles).unwrap_or_default();
    let current = app.doc().and_then(|d| pg_core::edit::drop_cap_at(d, t.storage, t.a));
    let presets: Vec<(ObjId, DropCapSettings)> = styles.iter().filter_map(|(id, _)| app.doc().map(|d| (*id, pg_core::edit::drop_cap_settings(d, *id)))).collect();
    let cur = current.and_then(|id| app.doc().map(|d| pg_core::edit::drop_cap_settings(d, id)));
    let mut on = current.is_some();
    let mut pick: Option<Option<ObjId>> = None;
    ui.horizontal(|ui| {
        if theme::checkbox(ui, &mut on, theme::bold("Drop Cap")).changed() {
            pick = Some(if on { presets.first().map(|p| p.0) } else { None });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(67.0, 33.0), Sense::click());
            let shown = cur.clone().or_else(|| presets.first().map(|p| p.1.clone()));
            if let Some(s) = &shown {
                paint_thumb(ui.painter(), r, s);
            }
            ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, if resp.hovered() { theme::ACCENT() } else { theme::gray(0x50) }), egui::StrokeKind::Outside);
            egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.vertical_centered(|ui| ui.label(theme::bold("Drop Cap Styles")));
                ui.add_space(4.0);
                egui::Grid::new("drop-cap-grid").spacing(vec2(8.0, 8.0)).show(ui, |ui| {
                    for (i, (id, s)) in presets.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(112.0, 100.0), Sense::click());
                        paint_thumb(ui.painter(), r, s);
                        if resp.hovered() || current == Some(*id) {
                            ui.painter().rect_stroke(r, 4.0, Stroke::new(2.0, theme::ACCENT()), egui::StrokeKind::Outside);
                        }
                        if resp.clicked() {
                            pick = Some(Some(*id));
                            ui.close();
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            });
        });
    });
    if let Some(p) = pick {
        let st = t.storage;
        app.edit("Drop Cap", |d, h| pg_core::edit::set_drop_cap(d, h, st, t.a, t.b, p));
        return;
    }
    let (Some(s), Some(default)) = (cur, presets.first().map(|p| p.0)) else { return };
    let mut n = s.clone();
    let mut changed = false;
    let mut num = |ui: &mut egui::Ui, label: &str, v: &mut u32, range: std::ops::RangeInclusive<f64>| {
        let mut f = f64::from(*v);
        row(ui, label, |ui| {
            if theme::stepper(ui, &mut f, 1.0, range, "", 0) {
                *v = f as u32;
                changed = true;
            }
        });
    };
    num(ui, "Lines", &mut n.lines, 1.0..=10.0);
    num(ui, "Characters", &mut n.chars, 1.0..=10.0);
    num(ui, "Raised Lines", &mut n.raised, 0.0..=9.0);
    let mut size = f64::from(n.scale * 100.0);
    row(ui, "Size", |ui| {
        if theme::stepper(ui, &mut size, 5.0, 20.0..=200.0, " %", 0) {
            n.scale = size as f32 / 100.0;
            changed = true;
        }
    });
    let wraps = ["Rectangular", "Contour", "None"];
    row(ui, "Text Wrap", |ui| {
        theme::combo("drop-cap-wrap").selected_text(wraps[(n.wrap as usize).min(2)]).width(120.0).show_ui(ui, |ui| {
            for (i, w) in wraps.iter().enumerate() {
                if ui.selectable_label(n.wrap as usize == i, *w).clicked() {
                    n.wrap = i as u64;
                    changed = true;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        if theme::checkbox(ui, &mut n.shape, "Shape").changed() {
            if n.shape && n.fill.is_none() {
                n.fill = Some(kn_model::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
            }
            changed = true;
        }
        if n.shape {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(c) = crate::objinsp::color_button(ui, n.fill.unwrap_or(kn_model::Color::BLACK)) {
                    n.fill = Some(c);
                    changed = true;
                }
            });
        }
    });
    if n.shape {
        let mut rad = f64::from(n.corner_radius * 100.0);
        row(ui, "Corner Radius", |ui| {
            if theme::stepper(ui, &mut rad, 5.0, 0.0..=50.0, " %", 0) {
                n.corner_radius = rad as f32 / 100.0;
                changed = true;
            }
        });
    }
    if changed {
        let st = t.storage;
        app.edit("Drop Cap", |d, h| pg_core::edit::set_drop_cap_settings(d, h, st, t.a, t.b, default, &|x| *x = n.clone()));
    }
}
