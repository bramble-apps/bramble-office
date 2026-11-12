//! Phat FX's window: the XY pad (drag the blue point; X and Y on / off, one or two targets each
//! from the pop-ups, a depth each), the effects order strip (active effects lit, inactive dim,
//! left to right; drag a name sideways to change the order), the Input knob, and every
//! processor's and modulator's controls, with the modulated knobs marked by a blue ring for the
//! range and a dot for the value now. No picture of Logic's Phat FX is in the research: the
//! layout follows the guide.

use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_fx::phat_fx::*;

use crate::look;

type Set = Vec<(u32, f32)>;

const BLUE: Color32 = Color32::from_rgb(0x3a, 0x9c, 0xf6);

fn get(params: &[Param], values: &[f32], set: &Set, id: u32) -> f32 {
    set.iter().rev().find(|x| x.0 == id).map_or_else(|| params.iter().position(|p| p.id == id).map_or(0.0, |i| values[i]), |x| x.1)
}

fn rows(ui: &mut egui::Ui, params: &[Param], values: &[f32], ids: &[u32], salt: &str, set: &mut Set) {
    let mut p = Vec::new();
    let mut v = Vec::new();
    for id in ids {
        if let Some(i) = params.iter().position(|x| x.id == *id) {
            let mut q = params[i].clone();
            q.hidden = false;
            p.push(q);
            v.push(get(params, values, set, *id));
        }
    }
    set.extend(crate::pluginwin::control_rows(ui, &p, &v, &[], salt));
}

/// The parameter each modulation target (XY_TARGETS minus Off) moves.
const TARGET_IDS: [u32; 7] = [28, 29, 2, 3, 15, 39, 66];
/// Each processor's on switch.
const ON_IDS: [u32; 6] = [1, 13, 26, 37, 46, 55];

pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Set) {
    let g = |set: &Set, id| get(params, values, set, id);
    let w = ui.available_width();
    // the order strip
    let (or, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(or, 5.0, look::group_bg());
    let order: Vec<usize> = (0..6).map(|k| g(set, ORDER + k as u32).round() as usize % 6).collect();
    let cw = or.width() / 6.0;
    let drag_id = egui::Id::unique("phat-order-drag");
    for (place, proc_) in order.iter().enumerate() {
        let r = Rect::from_min_size(pos2(or.min.x + cw * place as f32 + 3.0, or.min.y + 3.0), vec2(cw - 6.0, or.height() - 6.0));
        let lit = g(set, ON_IDS[*proc_]) >= 0.5;
        p.rect_filled(r, 4.0, if lit { look::accent() } else { Color32::from_gray(0x34) });
        p.text(r.center(), Align2::CENTER_CENTER, PROCESSORS[*proc_], look::regular(11.0), if lit { Color32::WHITE } else { look::text_dim() });
        if place < 5 {
            p.text(pos2(r.max.x + 3.0, r.center().y), Align2::CENTER_CENTER, "›", look::regular(11.0), look::text_dim());
        }
        let resp = ui.interact(r, egui::Id::unique(("phat-order", place)), Sense::click_and_drag()).on_hover_text("Drag sideways to change the order; click to turn on or off");
        if resp.clicked() {
            set.push((ON_IDS[*proc_], if lit { 0.0 } else { 1.0 }));
        }
        if resp.drag_started() {
            ui.data_mut(|d| d.insert_temp(drag_id, place));
        }
        if resp.drag_stopped()
            && let Some(pt) = resp.interact_pointer_pos()
        {
            let to = (((pt.x - or.min.x) / cw).floor().max(0.0) as usize).min(5);
            if to != place {
                let mut o = order.clone();
                let it = o.remove(place);
                o.insert(to, it);
                for (k, x) in o.iter().enumerate() {
                    set.push((ORDER + k as u32, *x as f32));
                }
            }
        }
    }
    ui.add_space(6.0);
    // the XY pad and its controls
    let (pr, _) = ui.allocate_exact_size(vec2(w, 216.0), Sense::hover());
    let pad = Rect::from_min_size(pr.min, vec2(180.0, 180.0));
    p.rect_filled(pad, 6.0, look::lcd_bg());
    p.line_segment([pos2(pad.center().x, pad.min.y), pos2(pad.center().x, pad.max.y)], Stroke::new(0.5, look::line()));
    p.line_segment([pos2(pad.min.x, pad.center().y), pos2(pad.max.x, pad.center().y)], Stroke::new(0.5, look::line()));
    let (x, y) = (g(set, XY + 8), g(set, XY + 9));
    let dot = pos2(pad.min.x + pad.width() * x, pad.max.y - pad.height() * y);
    let resp = ui.interact(pad, egui::Id::unique("phat-xy"), Sense::click_and_drag());
    if let Some(pt) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        set.push((XY + 8, ((pt.x - pad.min.x) / pad.width()).clamp(0.0, 1.0)));
        set.push((XY + 9, ((pad.max.y - pt.y) / pad.height()).clamp(0.0, 1.0)));
    }
    p.circle_filled(dot, 7.0, BLUE);
    p.circle_stroke(dot, 9.0, Stroke::new(1.0, Color32::WHITE));
    let mut side = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(pad.max.x + 12.0, pr.min.y), pr.max)));
    rows(&mut side, params, values, &[XY + 4, XY, XY + 1, XY + 6, XY + 5, XY + 2, XY + 3, XY + 7], "phat-xy-rows", set);
    ui.add_space(6.0);
    // the modulated targets: a ring for the range, a dot for now
    let mut ranges = [0.0f32; 7];
    for (axis, on_id) in [(0u32, XY + 4), (1, XY + 5)] {
        if g(set, on_id) >= 0.5 {
            for t in [g(set, XY + 2 * axis).round() as usize, g(set, XY + 2 * axis + 1).round() as usize] {
                if t > 0 {
                    ranges[(t - 1).min(6)] += 0.5 * g(set, XY + 6 + axis).abs() / 100.0;
                }
            }
        }
    }
    let (rr, _) = ui.allocate_exact_size(vec2(w, 54.0), Sense::hover());
    let names = &XY_TARGETS[1..];
    let tw = rr.width() / names.len() as f32;
    for (k, n) in names.iter().enumerate() {
        let c = pos2(rr.min.x + tw * (k as f32 + 0.5), rr.min.y + 20.0);
        let val = g(set, TARGET_IDS[k]).clamp(0.0, 1.0);
        let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
        let sweep = std::f32::consts::PI * 5.0 / 3.0;
        p.circle_stroke(c, 14.0, Stroke::new(2.0, look::line()));
        if ranges[k] > 0.0 {
            let a0 = (val - ranges[k]).clamp(0.0, 1.0);
            let a1 = (val + ranges[k]).clamp(0.0, 1.0);
            let pts: Vec<egui::Pos2> = (0..=24).map(|i| {
                let a = start + sweep * (a0 + (a1 - a0) * i as f32 / 24.0);
                c + vec2(a.cos(), a.sin()) * 14.0
            }).collect();
            p.add(egui::Shape::line(pts, Stroke::new(3.0, BLUE)));
        }
        let a = start + sweep * val;
        p.circle_filled(c + vec2(a.cos(), a.sin()) * 14.0, 3.0, if ranges[k] > 0.0 { BLUE } else { look::text() });
        p.text(pos2(c.x, rr.max.y - 6.0), Align2::CENTER_CENTER, *n, look::regular(9.5), look::text_dim());
    }
    // every processor and modulator
    let ftype = g(set, 27).round() as usize;
    let filter_extra: Vec<u32> = match ftype {
        15..=17 => vec![F_FBACK, F_DAMP],
        18 => vec![F_GAIN, F_BW],
        27 => vec![F_AMOUNT],
        28 => vec![F_FREQ, F_MIX2],
        29 | 30 | 32 => vec![F_AMOUNT, F_MIX2],
        31 => vec![F_AMOUNT, F_MIX2, F_FREQ],
        _ => Vec::new(),
    };
    let groups: Vec<(&str, Vec<u32>)> = vec![
        ("Bandpass", vec![1, 12, 2, 3, 4, 5, 11]),
        ("Distortion", vec![13, 14, 15, 16, 17, 18, 19]),
        ("Filter", [vec![26, 27, 28, 29, 30, 31], filter_extra].concat()),
        ("Mod FX", vec![37, 45, 38, 39]),
        ("Bass Enhancer", vec![46, 54, 47, 48]),
        ("Compressor", vec![55, 64, 56, 57]),
        ("Master", vec![INPUT, 65, 66, 91]),
        ("Envelope", vec![98, 99, ENV_ATTACK, ENV_RELEASE, ENV_DEPTH]),
        ("LFO 1", vec![100, 101, 102, LFO_BASE, LFO_BASE + 1, LFO_BASE + 2, LFO_BASE + 3]),
        ("LFO 2", vec![103, 104, 105, LFO_BASE + 4, LFO_BASE + 5, LFO_BASE + 6, LFO_BASE + 7]),
    ];
    for (name, ids) in groups {
        egui::CollapsingHeader::new(name).id_salt(("phat-grp", name)).default_open(matches!(name, "Filter" | "Master")).show(ui, |ui| {
            rows(ui, params, values, &ids, name, set);
        });
    }
}
