//! Studio's controls, drawn as Logic's: the knob (Smart Controls, plug-in windows, the pan knob),
//! the value slot (Region inspector rows: drag up or down, double-click to type, a menu for
//! choices) and the disclosure header. Logic's gestures (interactions/DRAGS.txt): drag up or
//! down to change, Shift for fine steps, Option-click for the default, double-click to type.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::look;

/// A knob with its label under it and the value above while it's dragged. Returns true when the
/// value changed.
pub fn knob(ui: &mut egui::Ui, r: Rect, id: egui::Id, value: &mut f32, min: f32, max: f32, default: f32, label: &str, shown: &dyn Fn(f32) -> String) -> bool {
    let p = ui.painter().clone();
    let d = r.width().min(r.height() - 18.0) - 4.0;
    let c = pos2(r.center().x, r.min.y + d / 2.0 + 2.0);
    let kr = Rect::from_center_size(c, vec2(d, d));
    let resp = ui.interact(kr, id, Sense::click_and_drag()).on_hover_text(format!("{label}: {}", shown(*value)));
    let mut changed = false;
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let span = max - min;
        let step = -resp.drag_delta().y / if fine { 600.0 } else { 150.0 } * span;
        let v = (*value + step).clamp(min, max);
        if v != *value {
            *value = v;
            changed = true;
        }
    }
    if resp.double_clicked() || (resp.clicked() && ui.input(|i| i.modifiers.alt)) {
        *value = default;
        changed = true;
    }
    let frac = if max > min { (*value - min) / (max - min) } else { 0.0 };
    // the ring: 300° of travel, lit from the start (or from the centre for bipolar values)
    let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
    let sweep = std::f32::consts::PI * 5.0 / 3.0;
    let ring = |a0: f32, a1: f32, col: Color32, w: f32| {
        let n = 32;
        let pts: Vec<egui::Pos2> = (0..=n).map(|k| {
            let a = a0 + (a1 - a0) * k as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * (d / 2.0 - 1.5)
        }).collect();
        p.add(egui::Shape::line(pts, Stroke::new(w, col)));
    };
    ring(start, start + sweep, look::lcd_bg(), 3.0);
    let bipolar = min < 0.0 && max > 0.0;
    let zero = if bipolar { -min / (max - min) } else { 0.0 };
    let (a0, a1) = (start + sweep * zero.min(frac), start + sweep * zero.max(frac));
    if a1 > a0 + 0.001 {
        ring(a0, a1, look::accent(), 3.0);
    }
    p.circle_filled(c, d / 2.0 - 5.0, Color32::from_gray(if resp.hovered() || resp.dragged() { 0x5a } else { 0x4a }));
    let a = start + sweep * frac;
    p.line_segment([c + vec2(a.cos(), a.sin()) * (d / 2.0 - 12.0).max(2.0), c + vec2(a.cos(), a.sin()) * (d / 2.0 - 6.0)], Stroke::new(2.0, Color32::WHITE));
    let text = if resp.dragged() || resp.hovered() { shown(*value) } else { label.to_string() };
    p.with_clip_rect(r.expand2(vec2(6.0, 0.0))).text(pos2(r.center().x, r.max.y - 8.0), Align2::CENTER_CENTER, text, look::regular(11.0), if resp.dragged() { look::text() } else { look::text_dim() });
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    changed
}

/// An inspector row's value: drag up or down (Shift: fine), double-click to type, Option-click
/// for the default. Returns true when changed.
pub fn value_slot(ui: &mut egui::Ui, r: Rect, id: egui::Id, value: &mut f32, min: f32, max: f32, default: f32, step: f32, shown: &dyn Fn(f32) -> String) -> bool {
    let typing_id = id.with("typing");
    let mut typing: Option<String> = ui.data(|d| d.get_temp(typing_id));
    let mut changed = false;
    if let Some(t) = typing.as_mut() {
        let te = ui.put(r, egui::TextEdit::singleline(t).font(look::regular(12.0)).align(egui::Align2::RIGHT_CENTER));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            if let Ok(v) = t.trim().trim_end_matches('%').trim().parse::<f32>() {
                *value = v.clamp(min, max);
                changed = true;
            }
            ui.data_mut(|d| d.remove::<String>(typing_id));
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, t.clone()));
        }
        return changed;
    }
    let resp = ui.interact(r, id, Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.painter().rect_filled(r, 3.0, look::group_bg());
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    ui.painter().text(pos2(r.max.x - 4.0, r.center().y), Align2::RIGHT_CENTER, shown(*value), look::regular(12.0), look::text());
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let acc_id = id.with("acc");
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0);
        acc += -resp.drag_delta().y / if fine { 12.0 } else { 4.0 };
        let steps = acc.trunc();
        acc -= steps;
        ui.data_mut(|d| d.insert_temp(acc_id, acc));
        if steps != 0.0 {
            *value = (*value + steps * step).clamp(min, max);
            changed = true;
        }
    }
    if resp.clicked() && ui.input(|i| i.modifiers.alt) {
        *value = default;
        changed = true;
    }
    if resp.double_clicked() {
        ui.data_mut(|d| d.insert_temp(typing_id, shown(*value)));
    }
    changed
}

/// A choice row's value with a menu (Logic's pop-up slots: Quantize, Loop…).
pub fn choice_slot(ui: &mut egui::Ui, r: Rect, id: egui::Id, current: usize, choices: &[&str]) -> Option<usize> {
    let resp = ui.interact(r, id, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, look::group_bg());
    }
    let text = choices.get(current).copied().unwrap_or("");
    ui.painter().text(pos2(r.max.x - 16.0, r.center().y), Align2::RIGHT_CENTER, text, look::regular(12.0), look::text());
    let m = pos2(r.max.x - 7.0, r.center().y);
    for dy in [-2.5f32, 2.5] {
        let s = dy.signum();
        ui.painter().line_segment([m + vec2(-3.0, dy - s * 1.5), m + vec2(0.0, dy + s * 1.5)], Stroke::new(1.2, look::text_dim()));
        ui.painter().line_segment([m + vec2(0.0, dy + s * 1.5), m + vec2(3.0, dy - s * 1.5)], Stroke::new(1.2, look::text_dim()));
    }
    let mut out = None;
    egui::Popup::menu(&resp).show(|ui| {
        for (i, c) in choices.iter().enumerate() {
            if *c == "---" {
                ui.separator();
                continue;
            }
            if ui.add(egui::Button::selectable(i == current, *c)).clicked() {
                out = Some(i);
                ui.close();
            }
        }
    });
    out
}

/// A checkbox row's value (Logic's small square box at the right).
pub fn check_slot(ui: &mut egui::Ui, r: Rect, id: egui::Id, on: &mut bool) -> bool {
    let b = Rect::from_center_size(pos2(r.max.x - 10.0, r.center().y), vec2(14.0, 14.0));
    let resp = ui.interact(r, id, Sense::click());
    let p = ui.painter();
    p.rect_filled(b, 3.0, if *on { look::accent() } else { look::lcd_bg() });
    p.rect_stroke(b, 3.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
    if *on {
        p.line_segment([b.center() + vec2(-3.5, 0.0), b.center() + vec2(-1.0, 3.0)], Stroke::new(1.8, Color32::WHITE));
        p.line_segment([b.center() + vec2(-1.0, 3.0), b.center() + vec2(4.0, -3.5)], Stroke::new(1.8, Color32::WHITE));
    }
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
}

/// A disclosure header: the label, a chevron at the right (› closed, ⌄ open). Returns true when
/// clicked.
pub fn disclosure(ui: &mut egui::Ui, r: Rect, id: egui::Id, label: &str, value: &str, open: bool) -> bool {
    let p = ui.painter();
    let resp = ui.interact(r, id, Sense::click());
    p.text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, label, look::bold(13.0), look::text());
    let lw = p.layout_no_wrap(label.to_string(), look::bold(13.0), look::text()).size().x;
    p.with_clip_rect(r.shrink2(vec2(8.0, 0.0))).text(pos2(r.min.x + 16.0 + lw, r.center().y), Align2::LEFT_CENTER, value, look::regular(13.0), look::text_dim());
    let m = pos2(r.max.x - 18.0, r.center().y);
    let col = if resp.hovered() { look::text() } else { look::text_dim() };
    crate::glyphs::paint(&p, Rect::from_center_size(m, vec2(14.0, 14.0)), if open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 13.0, col);
    p.hline(r.x_range(), r.max.y, Stroke::new(1.0, look::line()));
    resp.clicked()
}

/// A typing field that has just opened takes the keyboard once; after that it keeps it only
/// while it has it, so a click elsewhere ends the typing (and Escape too), as Logic's fields do.
/// (Asking for focus every frame would take it straight back and the field would never close.)
pub fn focus_once(ui: &egui::Ui, te: &egui::Response) {
    let key = te.id.with("focused-once");
    let done: bool = ui.data(|d| d.get_temp(key)).unwrap_or(false);
    if !done {
        te.request_focus();
        ui.data_mut(|d| d.insert_temp(key, true));
    }
    if te.lost_focus() {
        ui.data_mut(|d| d.remove::<bool>(key));
    }
}
