//! Pedalboard's Router, above the Pedal area: Bus A and Bus B as lines (two parallel lines for
//! the stereo signal), each element's name with its stereo icon (linked circles), and the rules
//! the guide gives. It appears once a pedal is in, when the pointer is just above the Pedal
//! area, and stays while a second bus exists. With two buses, dots on the lines mark each
//! pedal's output: clicking one sets the split point (a cable shows where Bus B leaves Bus A);
//! double-clicking the split marker replaces it with a Splitter, double-clicking a Splitter's
//! label puts a split point back in its place; dragging the Splitter or the Mixer moves it along
//! the chain (where the rules allow); deleting both removes the second bus. Clicking a pedal's
//! name moves it between Bus A and Bus B.

use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_fx::pedalboard::*;

use crate::look;

type Set = Vec<(u32, f32)>;

fn v(params: &[Param], values: &[f32], id: u32) -> f32 {
    params.iter().position(|p| p.id == id).map_or(0.0, |i| values[i])
}

/// Where a slot's settings come from when the board is rearranged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Src {
    Old(usize),
    New(usize),
    Empty,
}

/// Rearranges the slots: new slot k takes `order[k]`.
pub fn arrange(params: &[Param], values: &[f32], order: &[Src], set: &mut Set) {
    let slot = |k: usize| -> Vec<(u32, f32)> { params.iter().zip(values).filter(|(p, _)| p.id >= BASE + STRIDE * k as u32 && p.id < BASE + STRIDE * (k as u32 + 1)).map(|(p, v)| (p.id - BASE - STRIDE * k as u32, *v)).collect() };
    let old: Vec<Vec<(u32, f32)>> = (0..SLOTS).map(slot).collect();
    // the pedals first (so their parameters exist), then their values
    for (k, s) in order.iter().enumerate().take(SLOTS) {
        let kind = match s {
            Src::Old(o) => old[*o].iter().find(|x| x.0 == 0).map_or(0.0, |x| x.1),
            Src::New(kind) => *kind as f32,
            Src::Empty => 0.0,
        };
        set.push((BASE + STRIDE * k as u32, kind));
    }
    for (k, s) in order.iter().enumerate().take(SLOTS) {
        match s {
            Src::Old(o) => {
                for (off, val) in old[*o].iter().filter(|x| x.0 != 0) {
                    set.push((BASE + STRIDE * k as u32 + off, *val));
                }
            }
            Src::New(_) => set.push((BASE + STRIDE * k as u32 + 1, 0.0)),
            Src::Empty => {}
        }
    }
}

/// The used slots, compacted, as a new order with `f` applied to the list of sources.
fn reorder(params: &[Param], values: &[f32], used: &[usize], f: impl FnOnce(&mut Vec<Src>), set: &mut Set) {
    let mut list: Vec<Src> = used.iter().map(|k| Src::Old(*k)).collect();
    f(&mut list);
    list.resize(SLOTS, Src::Empty);
    arrange(params, values, &list, set);
}

pub fn router(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Set) {
    let g = |id| v(params, values, id);
    let choices = pedal_choices();
    let kinds: Vec<usize> = (0..SLOTS).map(|k| g(BASE + STRIDE * k as u32).round() as usize).collect();
    let used: Vec<usize> = (0..SLOTS).filter(|k| kinds[*k] != 0).collect();
    let names: Vec<String> = used.iter().map(|k| choices[kinds[*k]].to_string()).collect();
    let any_b = used.iter().any(|k| g(BASE + STRIDE * *k as u32 + 1) >= 0.5);
    let two = two_buses(&names, any_b);
    let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::hover());
    let hover = ui.ctx().pointer_hover_pos().is_some_and(|p| band.expand2(vec2(0.0, 30.0)).contains(p));
    if used.is_empty() || !(two || hover) {
        if !used.is_empty() {
            ui.painter().text(band.center(), Align2::CENTER_CENTER, "Router", look::regular(10.0), look::text_dim().gamma_multiply(0.5));
        }
        return;
    }
    let p = ui.painter().clone();
    let n = used.len();
    let w = band.width() / n as f32;
    let (ya, yb) = (band.min.y + 14.0, band.max.y - 14.0);
    let line_col = Color32::from_gray(0x8a);
    // a stereo bus: two parallel gray lines
    let bus = |y: f32, x0: f32, x1: f32| {
        p.line_segment([pos2(x0, y - 1.5), pos2(x1, y - 1.5)], Stroke::new(1.0, line_col));
        p.line_segment([pos2(x0, y + 1.5), pos2(x1, y + 1.5)], Stroke::new(1.0, line_col));
    };
    let has_splitter = names.iter().any(|x| x == "Splitter");
    let mixer = names.iter().position(|x| x == "Mixer");
    let sp = g(SPLIT_POINT).round() as usize;
    let split_at = if let Some(s) = names.iter().position(|x| x == "Splitter") { s } else { sp.min(n) };
    let end_b = mixer.unwrap_or(n);
    bus(ya, band.min.x, band.max.x);
    if two {
        let xs = band.min.x + w * split_at as f32;
        let xe = band.min.x + w * end_b as f32 + w * 0.5;
        bus(yb, xs, xe.min(band.max.x));
        // the cables between the buses
        p.line_segment([pos2(xs, ya), pos2(xs, yb)], Stroke::new(1.5, line_col));
        if mixer.is_some() {
            p.line_segment([pos2(xe, yb), pos2(xe, ya)], Stroke::new(1.5, line_col));
        }
        // the split marker (double-click: becomes a Splitter)
        if !has_splitter {
            let mr = Rect::from_center_size(pos2(xs, (ya + yb) / 2.0), vec2(12.0, 12.0));
            p.circle_filled(mr.center(), 5.0, look::accent());
            if ui.interact(mr, ui.scope_id().with("pb-split-marker"), Sense::click()).on_hover_text("Split point (double-click: replace with a Splitter)").double_clicked() && splitter_ok(&names, split_at) {
                let kind = choices.iter().position(|c| *c == "Splitter").unwrap_or(0);
                reorder(params, values, &used, |l| l.insert(split_at, Src::New(kind)), set);
            }
        }
    }
    let mut drag_to: Option<(usize, usize)> = None;
    for (i, k) in used.iter().enumerate() {
        let b = g(BASE + STRIDE * *k as u32 + 1) >= 0.5 && two;
        let name = &names[i];
        let x0 = band.min.x + w * i as f32;
        let r = Rect::from_min_size(pos2(x0 + 2.0, if b { yb - 8.0 } else { ya - 8.0 }), vec2(w - 4.0, 16.0));
        p.rect_filled(r, 3.0, look::group_bg());
        p.text(r.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, name, look::regular(10.0), look::text());
        // the stereo icon: two linked circles
        let ic = r.right_center() - vec2(10.0, 0.0);
        p.circle_stroke(ic - vec2(2.5, 0.0), 3.0, Stroke::new(1.0, look::text_dim()));
        p.circle_stroke(ic + vec2(2.5, 0.0), 3.0, Stroke::new(1.0, look::text_dim()));
        let utility = name == "Splitter" || name == "Mixer";
        let resp = ui.interact(r, ui.scope_id().with(("pb-route", *k)), if utility { Sense::click_and_drag() } else { Sense::click() });
        if utility {
            let resp = resp.on_hover_text(if name == "Splitter" { "Drag to move the split; double-click: replace with a split point" } else { "Drag to move where Bus A and Bus B are mixed" });
            if resp.double_clicked() && name == "Splitter" {
                // the Splitter becomes a split point at its place
                set.push((SPLIT_POINT, i as f32));
                reorder(params, values, &used, |l| {
                    l.remove(i);
                }, set);
            }
            if resp.drag_stopped()
                && let Some(pt) = resp.interact_pointer_pos()
            {
                let to = (((pt.x - band.min.x) / w).floor().max(0.0) as usize).min(n - 1);
                if to != i {
                    drag_to = Some((i, to));
                }
            }
            if resp.dragged()
                && let Some(pt) = resp.interact_pointer_pos()
            {
                p.rect_stroke(Rect::from_center_size(pos2(pt.x, r.center().y), r.size()), 3.0, Stroke::new(1.0, look::accent()), egui::StrokeKind::Inside);
            }
        } else if resp.on_hover_text("Click: move between Bus A and Bus B").clicked() && two {
            set.push((BASE + STRIDE * *k as u32 + 1, if b { 0.0 } else { 1.0 }));
        }
        // with two buses: a dot at each pedal's output; clicking sets the split point there
        if two && !has_splitter && !utility {
            let dot = pos2(x0 + w - 2.0, ya);
            let dr = Rect::from_center_size(dot, vec2(10.0, 10.0));
            let ok = split_point_ok(&names, i + 1);
            p.circle_filled(dot, 3.0, if ok { look::text() } else { look::text_dim().gamma_multiply(0.4) });
            if ok && ui.interact(dr, ui.scope_id().with(("pb-dot", i)), Sense::click()).on_hover_text("Split here").clicked() {
                set.push((SPLIT_POINT, (i + 1) as f32));
            }
        }
    }
    if let Some((from, to)) = drag_to {
        let mut l: Vec<String> = names.clone();
        let item = l.remove(from);
        l.insert(to, item.clone());
        let ok = if item == "Splitter" { splitter_ok(&l, to) } else { mixer_ok(&l, to, (!has_splitter).then_some(sp)) };
        if ok {
            reorder(params, values, &used, |s| {
                let it = s.remove(from);
                s.insert(to, it);
            }, set);
        }
    }
    p.text(pos2(band.min.x, ya), Align2::RIGHT_CENTER, "A ", look::regular(9.0), look::text_dim());
    if two {
        p.text(pos2(band.min.x, yb), Align2::RIGHT_CENTER, "B ", look::regular(9.0), look::text_dim());
    }
}

/// When a pedal is added from the Browser: a Splitter or a Mixer only where the rules allow
/// (a Mixer with no Splitter makes the split point the leftmost position); deleting the last of
/// the Splitter and Mixer takes Bus B away. Returns false when the place isn't allowed.
pub fn place_utility(params: &[Param], values: &[f32], name: &str, at: usize, set: &mut Set) -> bool {
    let g = |id| v(params, values, id);
    let choices = pedal_choices();
    let kinds: Vec<usize> = (0..SLOTS).map(|k| g(BASE + STRIDE * k as u32).round() as usize).collect();
    let used: Vec<usize> = (0..SLOTS).filter(|k| kinds[*k] != 0).collect();
    let names: Vec<String> = used.iter().map(|k| choices[kinds[*k]].to_string()).collect();
    let has_splitter = names.iter().any(|x| x == "Splitter");
    let ok = match name {
        "Splitter" => splitter_ok(&names, at),
        _ => mixer_ok(&names, at, (!has_splitter).then_some(0)) || (at == names.len() && at > 0 && names.last().is_some_and(|x| x != "Mixer" && x != "Splitter")),
    };
    if !ok {
        return false;
    }
    let kind = choices.iter().position(|c| *c == name).unwrap_or(0);
    if name == "Mixer" && !has_splitter {
        set.push((SPLIT_POINT, 0.0));
    }
    reorder(params, values, &used, |l| l.insert(at.min(l.len()), Src::New(kind)), set);
    true
}

/// After a slot is emptied: with no Splitter and no Mixer left, everything goes back to Bus A.
pub fn after_remove(params: &[Param], values: &[f32], removed: usize, set: &mut Set) {
    let g = |id| v(params, values, id);
    let choices = pedal_choices();
    let left: Vec<usize> = (0..SLOTS).filter(|k| *k != removed && g(BASE + STRIDE * *k as u32).round() as usize != 0).collect();
    let utility = left.iter().any(|k| matches!(choices[g(BASE + STRIDE * *k as u32).round() as usize], "Splitter" | "Mixer"));
    if !utility {
        for k in left {
            set.push((BASE + STRIDE * k as u32 + 1, 0.0));
        }
    }
}

/// Import mode's state (per window).
#[derive(Clone, Default)]
struct Import {
    on: bool,
    path: Option<std::path::PathBuf>,
    /// A replacement in progress: the Pedal area slot, its settings before, the Browser row.
    pending: Option<(usize, Vec<(u32, f32)>, usize)>,
}

/// The Pedal Browser's Import Mode button and, when on, the imported setting's pedals in place
/// of the browser: the View menu becomes Select Setting (the first time it opens by itself),
/// the setting's name shows at the bottom, clicking an imported pedal replaces the selected
/// Pedal area pedal with it and its settings (blinking blue until made permanent by clicking
/// the background or the button; clicking it again reverts). True when import mode drew the
/// browser.
pub fn import_mode(ui: &mut egui::Ui, params: &[Param], values: &[f32], sel: Option<usize>, set: &mut Set) -> bool {
    let id = ui.scope_id().with("pb-import");
    let mut st: Import = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let label = if st.on { "Import Mode ●" } else { "Import Mode" };
    if ui.add(egui::Button::selectable(st.on, label)).on_hover_text("Load single stompboxes from a saved Pedalboard setting").clicked() {
        st.on = !st.on;
        st.pending = None;
    }
    if !st.on {
        ui.data_mut(|d| d.insert_temp(id, st));
        return false;
    }
    let list = crate::pluginwin::saved_settings("Pedalboard");
    let pick = ui.button("Select Setting…");
    let open_now = pick.clicked() || st.path.is_none();
    let popup_id = ui.scope_id().with("pb-import-pick");
    if open_now {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }
    egui::Popup::menu(&pick).id(popup_id).show(|ui| {
        if list.is_empty() {
            ui.label("No saved Pedalboard settings (Save Setting As… first)");
        }
        for (n, p) in &list {
            if ui.button(n).clicked() {
                st.path = Some(p.clone());
                ui.close();
            }
        }
    });
    let choices = pedal_choices();
    let vals = st.path.as_deref().and_then(crate::pluginwin::read_setting).unwrap_or_default();
    let get = |id: u32| vals.iter().find(|x| x.0 == id).map(|x| x.1);
    let blink = (ui.input(|i| i.time) * 3.0).fract() < 0.5;
    let mut clicked_row = false;
    egui::ScrollArea::vertical().id_salt("pb-import-list").max_height(200.0).show(ui, |ui| {
        for k in 0..SLOTS {
            let kind = get(BASE + STRIDE * k as u32).unwrap_or(0.0).round() as usize;
            if kind == 0 || kind >= choices.len() {
                continue;
            }
            let pending_here = st.pending.as_ref().is_some_and(|p| p.2 == k);
            let r = ui.add(egui::Button::selectable(pending_here && blink, choices[kind]));
            if pending_here && blink {
                ui.painter().rect_stroke(r.rect, 3.0, Stroke::new(1.5, Color32::from_rgb(0x3a, 0x9c, 0xf6)), egui::StrokeKind::Outside);
            }
            if r.clicked() {
                clicked_row = true;
                if let Some((slot, prev, row)) = st.pending.take() {
                    // the same pedal again: back to how it was
                    if row == k {
                        for (i, v) in prev {
                            set.push((i, v));
                        }
                        continue;
                    }
                    st.pending = Some((slot, prev, row));
                }
                let Some(s) = sel else { continue };
                let prev: Vec<(u32, f32)> = params.iter().zip(values).filter(|(p, _)| p.id >= BASE + STRIDE * s as u32 && p.id < BASE + STRIDE * (s as u32 + 1)).map(|(p, v)| (p.id, *v)).collect();
                set.push((BASE + STRIDE * s as u32, kind as f32));
                for (i, v) in vals.iter().filter(|x| x.0 > BASE + STRIDE * k as u32 && x.0 < BASE + STRIDE * (k as u32 + 1)) {
                    set.push((i - STRIDE * k as u32 + STRIDE * s as u32, *v));
                }
                let prev = st.pending.take().map_or(prev, |p| p.1);
                st.pending = Some((s, prev, k));
            }
        }
    });
    if st.pending.is_some() {
        ui.label(egui::RichText::new("Click the selected item again to revert").size(11.0).color(look::text_dim()));
        ui.ctx().request_repaint();
    }
    // the background: makes a replacement permanent
    let bg = ui.interact(ui.min_rect(), ui.scope_id().with("pb-import-bg"), Sense::click());
    if bg.clicked() && !clicked_row {
        st.pending = None;
    }
    if let Some(p) = &st.path {
        ui.label(egui::RichText::new(p.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()).size(11.0).strong());
    }
    ui.data_mut(|d| d.insert_temp(id, st));
    true
}
