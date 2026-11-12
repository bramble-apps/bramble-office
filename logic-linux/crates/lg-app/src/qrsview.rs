//! Quantec Room Simulator's window: the Quantec logo (click: the information panel), the QRS /
//! YardStick buttons, In and Out meters (one bar per channel in surround, clockwise), the Reverb
//! Time knob with a ring showing the range the Room Size allows, Room Size, Freeze / Add / Clear
//! (and Enhance on QRS, which dims the time controls), the reverb-time multiplier graph (time on
//! the right scale, multiplier 0.1×–10× on the left; drag the Low and High points up and down, on
//! YardStick sideways for the crossovers), the YardStick's Mode buttons, the Primary / Secondary
//! (/ Extended) pages, and in surround the Details view: the macros, Room Input Processing, the
//! sources, the seven offsets and the correlation patterns per channel.
//!
//! No picture of Logic's QRS window is in the research: the layout follows the guide's
//! description, in Logic's dark plug-in style.

use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_dsp::surround::Ch;
use lg_fx::verbs::qrs::*;

use crate::fxviews::Live;
use crate::look;

type Set = Vec<(u32, f32)>;

const PANEL: Color32 = Color32::from_rgb(0x24, 0x26, 0x2a);
const DISPLAY: Color32 = Color32::from_rgb(0x10, 0x12, 0x15);
const ACCENT: Color32 = Color32::from_rgb(0xe8, 0x9a, 0x2c);
const TEXT: Color32 = Color32::from_rgb(0xd4, 0xd6, 0xda);
const DIM: Color32 = Color32::from_rgb(0x8a, 0x8e, 0x94);

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

fn btn(ui: &mut egui::Ui, r: Rect, id: egui::Id, label: &str, on: bool, enabled: bool) -> bool {
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let fill = if on { ACCENT } else if resp.hovered() && enabled { Color32::from_gray(0x4a) } else { Color32::from_gray(0x38) };
    p.rect_filled(r, 4.0, fill);
    p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(11.5), if on { Color32::from_gray(0x14) } else if enabled { TEXT } else { DIM.gamma_multiply(0.6) });
    enabled && resp.clicked()
}

/// A vertical meter per channel (dB, -60..+6).
fn meters(p: &egui::Painter, r: Rect, title: &str, levels: &[(String, f32)]) {
    p.rect_filled(r, 3.0, DISPLAY);
    p.text(pos2(r.center().x, r.min.y - 8.0), Align2::CENTER_CENTER, title, look::regular(10.5), DIM);
    let n = levels.len().max(1);
    let area = Rect::from_min_max(r.min + vec2(3.0, 4.0), r.max - vec2(3.0, 14.0));
    let w = area.width() / n as f32;
    for (k, (lab, v)) in levels.iter().enumerate() {
        let x0 = area.min.x + w * k as f32 + 1.0;
        let f = ((v + 60.0) / 66.0).clamp(0.0, 1.0);
        let top = area.max.y - area.height() * f;
        let col = if *v > 0.0 { Color32::from_rgb(0xff, 0x3b, 0x30) } else if *v > -6.0 { Color32::from_rgb(0xf0, 0xc0, 0x20) } else { Color32::from_rgb(0x40, 0xc8, 0x5a) };
        if f > 0.0 {
            p.rect_filled(Rect::from_min_max(pos2(x0, top), pos2(x0 + w - 2.0, area.max.y)), 0.0, col);
        }
        p.text(pos2(x0 + w / 2.0 - 1.0, r.max.y - 6.0), Align2::CENTER_CENTER, lab, look::regular(8.5), DIM);
    }
}

/// A knob with a value text under it; `ring` draws the allowed range (fractions) around it.
#[allow(clippy::too_many_arguments)]
fn knob(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Set, c: egui::Pos2, d: f32, pid: u32, label: &str, log: bool, ring: Option<(f32, f32)>, enabled: bool) {
    let Some(p) = params.iter().find(|x| x.id == pid).cloned() else { return };
    let v = get(params, values, set, pid);
    // log knobs (time, room size): the knob works on the logarithm
    let (lo, hi) = if log { (p.min.ln(), p.max.ln()) } else { (p.min, p.max) };
    let mut kv = if log { v.max(p.min).ln() } else { v };
    let shown = |x: f32| crate::pluginwin::fmt(&p, if log { x.exp() } else { x });
    if let Some((a, b)) = ring {
        let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
        let sweep = std::f32::consts::PI * 5.0 / 3.0;
        let pts: Vec<egui::Pos2> = (0..=40).map(|k| {
            let ang = start + sweep * (a + (b - a) * k as f32 / 40.0);
            c + vec2(ang.cos(), ang.sin()) * (d / 2.0 + 9.0)
        }).collect();
        ui.painter().add(egui::Shape::line(pts, Stroke::new(3.0, ACCENT.gamma_multiply(0.8))));
    }
    if enabled {
        let def = if log { p.default.ln() } else { p.default };
        if crate::synthui::silver_knob(ui, c, d, egui::Id::unique(("qrs-knob", pid)), &mut kv, lo, hi, def, p.name, &shown) {
            set.push((pid, if log { kv.exp() } else { kv }));
        }
    } else {
        crate::synthui::silver_body(ui.painter(), c, d, false, None, true);
        ui.painter().circle_filled(c, d / 2.0, Color32::from_black_alpha(120));
    }
    ui.painter().text(c + vec2(0.0, d / 2.0 + 16.0), Align2::CENTER_CENTER, label, look::regular(11.0), if enabled { DIM } else { DIM.gamma_multiply(0.5) });
    ui.painter().text(c + vec2(0.0, d / 2.0 + 30.0), Align2::CENTER_CENTER, shown(kv), look::regular(11.0), if enabled { TEXT } else { DIM.gamma_multiply(0.5) });
}

/// The multiplier graph: frequency across, multiplier (0.1×–10×) on the left scale, reverb time
/// on the right.
#[allow(clippy::too_many_arguments)]
fn graph(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Set, r: Rect, yard: bool, time: f32, enabled: bool) {
    let p = ui.painter().clone();
    p.rect_filled(r, 4.0, DISPLAY);
    let area = Rect::from_min_max(r.min + vec2(34.0, 10.0), r.max - vec2(40.0, 18.0));
    let x_of = |f: f32| area.min.x + area.width() * ((f / 20.0).ln() / 1000f32.ln()).clamp(0.0, 1.0);
    let f_of = |x: f32| 20.0 * 1000f32.powf(((x - area.min.x) / area.width()).clamp(0.0, 1.0));
    let y_of = |m: f32| area.max.y - area.height() * ((m.max(0.1).log10() + 1.0) / 2.0).clamp(0.0, 1.0);
    let m_of = |y: f32| 10f32.powf(((area.max.y - y) / area.height()).clamp(0.0, 1.0) * 2.0 - 1.0);
    let grid = Color32::from_gray(0x2c);
    for (m, t) in [(0.1, "0.1×"), (1.0, "1×"), (10.0, "10×")] {
        p.line_segment([pos2(area.min.x, y_of(m)), pos2(area.max.x, y_of(m))], Stroke::new(0.5, grid));
        p.text(pos2(area.min.x - 4.0, y_of(m)), Align2::RIGHT_CENTER, t, look::regular(9.5), DIM);
        p.text(pos2(area.max.x + 4.0, y_of(m)), Align2::LEFT_CENTER, format!("{:.1}s", time * m), look::regular(9.5), DIM);
    }
    for (f, t) in [(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        p.line_segment([pos2(x_of(f), area.min.y), pos2(x_of(f), area.max.y)], Stroke::new(0.5, grid));
        p.text(pos2(x_of(f), area.max.y + 9.0), Align2::CENTER_CENTER, t, look::regular(9.5), DIM);
    }
    let low = get(params, values, set, LOW);
    let high = get(params, values, set, HIGH);
    let (xl, xh) = if yard { (get(params, values, set, LOW_XO), get(params, values, set, HIGH_XO)) } else { (500.0, 1000.0) };
    // the curve: low multiplier below the low crossover, 1× between, high above
    let pts = vec![pos2(area.min.x, y_of(low)), pos2(x_of(xl), y_of(low)), pos2(x_of(xl * 1.3), y_of(1.0)), pos2(x_of(xh / 1.3), y_of(1.0)), pos2(x_of(xh), y_of(high)), pos2(area.max.x, y_of(high))];
    p.add(egui::Shape::line(pts, Stroke::new(2.0, if enabled { ACCENT } else { DIM })));
    for (k, (x, m, idm, idx)) in [(xl, low, LOW, LOW_XO), (xh, high, HIGH, HIGH_XO)].into_iter().enumerate() {
        let c = pos2(x_of(x), y_of(m));
        let resp = ui.interact(Rect::from_center_size(c, vec2(16.0, 16.0)), egui::Id::unique(("qrs-pt", k)), if enabled { Sense::drag() } else { Sense::hover() });
        p.circle_filled(c, 6.0, if resp.hovered() || resp.dragged() { Color32::WHITE } else { ACCENT });
        p.text(c + vec2(0.0, -12.0), Align2::CENTER_CENTER, if k == 0 { "Low" } else { "High" }, look::regular(9.5), TEXT);
        if resp.dragged() {
            let to = c + resp.drag_delta();
            set.push((idm, m_of(to.y).clamp(0.1, 10.0)));
            if yard {
                let f = f_of(to.x);
                set.push((idx, if k == 0 { f.clamp(50.0, 1000.0) } else { f.clamp(1000.0, 16000.0) }));
            }
        }
    }
}

pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Set) {
    let g = |set: &Set, id| get(params, values, set, id);
    let yard = g(set, MODEL) >= 0.5;
    let enhance = !yard && g(set, ENHANCE) >= 0.5;
    let n = live.get("Channels").unwrap_or(2.0) as usize;
    let surround = n > 2;
    let chans: Vec<Ch> = (0..n).map(|k| Ch::ALL[(live.get(&format!("Ch {k}")).unwrap_or(k as f32) as usize).min(16)]).collect();
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 300.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 6.0, PANEL);
    // the logo: opens the information panel
    let logo = Rect::from_min_size(r.min + vec2(12.0, 10.0), vec2(120.0, 24.0));
    p.text(logo.left_center(), Align2::LEFT_CENTER, "QUANTEC", look::bold(17.0), ACCENT);
    let info_on = g(set, INFO) >= 0.5;
    if ui.interact(logo, egui::Id::unique("qrs-logo"), Sense::click()).on_hover_text("About Quantec Room Simulator").clicked() {
        set.push((INFO, if info_on { 0.0 } else { 1.0 }));
    }
    // QRS | YardStick
    for (k, n) in ["QRS", "YardStick"].iter().enumerate() {
        let br = Rect::from_min_size(r.min + vec2(150.0 + 86.0 * k as f32, 10.0), vec2(82.0, 24.0));
        if btn(ui, br, egui::Id::unique(("qrs-model", k)), n, (k == 1) == yard, true) && (k == 1) != yard {
            set.push((MODEL, k as f32));
        }
    }
    // In / Out meters, one bar per channel (clockwise order in surround)
    let order = |v: &mut Vec<(String, f32)>| {
        if surround {
            let fmt = lg_dsp::surround::Format::of_channels(&chans).unwrap_or(lg_dsp::surround::Format::S51);
            let mut idx: Vec<usize> = (0..n).collect();
            idx.sort_by(|a, b| {
                let pa = lg_dsp::surround::position(fmt, chans[*a]).0.rem_euclid(360.0) + if chans[*a] == Ch::Lfe { 400.0 } else { 0.0 };
                let pb = lg_dsp::surround::position(fmt, chans[*b]).0.rem_euclid(360.0) + if chans[*b] == Ch::Lfe { 400.0 } else { 0.0 };
                pa.partial_cmp(&pb).unwrap()
            });
            *v = idx.iter().map(|k| v[*k].clone()).collect();
        }
    };
    let mw = (10.0 * n as f32).max(24.0);
    let mut ins: Vec<(String, f32)> = (0..n).map(|k| (chans[k].label().to_string(), live.get(&format!("In {k}")).unwrap_or(-120.0))).collect();
    let mut outs: Vec<(String, f32)> = (0..n).map(|k| (chans[k].label().to_string(), live.get(&format!("Out {k}")).unwrap_or(-120.0))).collect();
    order(&mut ins);
    order(&mut outs);
    meters(&p, Rect::from_min_size(r.min + vec2(12.0, 60.0), vec2(mw, 200.0)), "In", &ins);
    meters(&p, Rect::from_min_size(pos2(r.max.x - 12.0 - mw, r.min.y + 60.0), vec2(mw, 200.0)), "Out", &outs);
    // Reverb Time (with the room's range ring) and Room Size
    let lo_hi = (live.get("Time Min").unwrap_or(0.1), live.get("Time Max").unwrap_or(100.0));
    let tp = params.iter().find(|x| x.id == TIME).cloned();
    let ring = tp.map(|t| {
        let f = |v: f32| ((v.ln() - t.min.ln()) / (t.max.ln() - t.min.ln())).clamp(0.0, 1.0);
        (f(lo_hi.0), f(lo_hi.1))
    });
    let x0 = r.min.x + mw + 50.0;
    knob(ui, params, values, set, pos2(x0 + 40.0, r.min.y + 100.0), 64.0, TIME, "Reverb Time", true, ring, !enhance);
    knob(ui, params, values, set, pos2(x0 + 40.0, r.min.y + 215.0), 46.0, SIZE, "Room Size", true, None, true);
    // Freeze, Add, Clear, Enhance (QRS)
    let bx = x0 + 100.0;
    let frz = g(set, FREEZE) >= 0.5;
    let bw = 70.0;
    if btn(ui, Rect::from_min_size(pos2(bx, r.min.y + 50.0), vec2(bw, 22.0)), egui::Id::unique("qrs-freeze"), "Freeze", frz, true) {
        set.push((FREEZE, if frz { 0.0 } else { 1.0 }));
    }
    let add = g(set, ADD) >= 0.5;
    if btn(ui, Rect::from_min_size(pos2(bx, r.min.y + 76.0), vec2(bw, 22.0)), egui::Id::unique("qrs-add"), "Add", add, frz) {
        set.push((ADD, if add { 0.0 } else { 1.0 }));
    }
    if btn(ui, Rect::from_min_size(pos2(bx, r.min.y + 102.0), vec2(bw, 22.0)), egui::Id::unique("qrs-clear"), "Clear", false, true) {
        set.push((CLEAR, 1.0));
    }
    if !yard && btn(ui, Rect::from_min_size(pos2(bx, r.min.y + 134.0), vec2(bw, 22.0)), egui::Id::unique("qrs-enh"), "Enhance", enhance, true) {
        set.push((ENHANCE, if enhance { 0.0 } else { 1.0 }));
    }
    if yard {
        // Mode: Complex / Medium / Simple
        let m = g(set, MODE).round() as usize;
        for (k, n) in ["Complex", "Medium", "Simple"].iter().enumerate() {
            if btn(ui, Rect::from_min_size(pos2(bx, r.min.y + 134.0 + 26.0 * k as f32), vec2(bw, 22.0)), egui::Id::unique(("qrs-mode", k)), n, k == m, true) && k != m {
                set.push((MODE, k as f32));
            }
        }
    }
    let gr = Rect::from_min_max(pos2(bx + bw + 16.0, r.min.y + 50.0), pos2(r.max.x - mw - 30.0, r.max.y - 20.0));
    let time = g(set, TIME).clamp(lo_hi.0, lo_hi.1);
    graph(ui, params, values, set, gr, yard, time, !enhance);

    if g(set, INFO) >= 0.5 {
        let ir = Rect::from_center_size(r.center(), vec2(380.0, 120.0));
        p.rect_filled(ir, 8.0, Color32::from_gray(0x18));
        p.rect_stroke(ir, 8.0, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
        p.text(ir.min + vec2(16.0, 18.0), Align2::LEFT_CENTER, "Quantec Room Simulator", look::bold(14.0), TEXT);
        let t = "Models two classic room simulators: the QRS of the early\n1980s and its successor, the YardStick, with more control\nover early reflections, diffusion and modulation.\nClick the logo to close.";
        p.text(ir.min + vec2(16.0, 38.0), Align2::LEFT_TOP, t, look::regular(11.0), DIM);
    }

    // the pages
    ui.add_space(6.0);
    let pages: &[&str] = if yard { &["Primary", "Secondary", "Extended"] } else { &["Primary", "Secondary"] };
    let page = (g(set, PAGE).round() as usize).min(pages.len() - 1);
    let (pr, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
    for (k, n) in pages.iter().enumerate() {
        if btn(ui, Rect::from_min_size(pr.min + vec2(86.0 * k as f32, 2.0), vec2(82.0, 22.0)), egui::Id::unique(("qrs-page", k)), n, k == page, true) && k != page {
            set.push((PAGE, k as f32));
        }
    }
    let details = g(set, DETAILS) >= 0.5;
    if surround && btn(ui, Rect::from_min_size(pos2(pr.max.x - 90.0, pr.min.y + 2.0), vec2(86.0, 22.0)), egui::Id::unique("qrs-details"), "Details", details, true) {
        set.push((DETAILS, if details { 0.0 } else { 1.0 }));
    }
    let ids: Vec<u32> = match (yard, page) {
        (false, 0) => vec![REV_DELAY, DRY, REFL_LEVEL, REV_LEVEL, REFL_DELAY],
        (false, _) => vec![ADDA, DRIVE, DRY_SRC, REFL_SRC, REFL_PHASE, CORR, SUBSONIC],
        (true, 0) => vec![REV_DELAY, DRY, REFL_LEVEL, REV_LEVEL, DENSITY],
        // (in surround the Subsonic button sits on the Secondary page)
        (true, 1) => {
            let mut v = vec![REFL_DELAY, REFL_SPREAD, HIGH_CUT, BASS, BASS_XO, LOW_XO, HIGH_XO];
            if surround {
                v.push(SUBSONIC);
            }
            v
        }
        (true, _) => {
            let mut v = if surround { vec![CORR] } else { vec![DRY_SRC, REFL_SRC, PATTERN_L, PATTERN_R, CORR] };
            if !surround {
                v.push(SUBSONIC);
            }
            v
        }
    };
    rows(ui, params, values, &ids, "qrs-page", set);
    if surround && (yard && page == 2 || !yard && page == 1) {
        ui.label(egui::RichText::new("Dry Source, 1st Reflection Source and Correlation Pattern: per channel in Details").color(DIM).size(11.0));
    }
    if surround && details {
        details_view(ui, params, values, set, &chans);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
}

/// Surround details: macros, then a row per channel.
fn details_view(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Set, chans: &[Ch]) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Macros").color(TEXT).strong());
    rows(ui, params, values, &MACROS, "qrs-macros", set);
    ui.add_space(6.0);
    let patterns = params.iter().find(|p| p.id == PATTERN_L).map(|p| p.choices).unwrap_or(&[]);
    egui::ScrollArea::horizontal().id_salt("qrs-details-grid").show(ui, |ui| {
        egui::Grid::new("qrs-details").striped(true).spacing(vec2(8.0, 3.0)).show(ui, |ui| {
            for h in ["", "Room Input", "Pre Level", "Pre Delay", "Pre High Cut", "Dry Source", "1st Refl. Source", "1st Refl. Delay", "1st Refl. Spread", "Reverb Delay", "Reverb High Cut", "Dry Level", "1st Refl. Level", "Reverb Level", "Pattern"] {
                ui.label(egui::RichText::new(h).color(DIM).size(10.5));
            }
            ui.end_row();
            for c in chans {
                let s = Ch::ALL.iter().position(|x| x == c).unwrap_or(0);
                let b = lg_fx::verbs::qch(s);
                ui.label(egui::RichText::new(c.label()).color(TEXT));
                let combo = |ui: &mut egui::Ui, set: &mut Set, id: u32, choices: &[&str]| {
                    let cur = get(params, values, set, id).round() as usize;
                    egui::ComboBox::from_id_salt(("qrs-d", id)).width(52.0).selected_text(choices.get(cur).copied().unwrap_or("")).show_ui(ui, |ui| {
                        for (k, t) in choices.iter().enumerate() {
                            if ui.selectable_label(k == cur, *t).clicked() {
                                set.push((id, k as f32));
                            }
                        }
                    });
                };
                combo(ui, set, b + ROOM_INPUT, &["1", "2", "3", "4", "5", "6", "7", "8"]);
                let num = |ui: &mut egui::Ui, set: &mut Set, id: u32, mute: bool, none_hi: bool| {
                    let Some(p) = params.iter().find(|x| x.id == id) else { return };
                    let mut v = get(params, values, set, id);
                    let resp = ui.add(egui::DragValue::new(&mut v).range(p.min..=p.max).speed((p.max - p.min) / 300.0).custom_formatter(move |x, _| {
                        if mute && x <= -30.5 {
                            "Mute".into()
                        } else if none_hi && x > 20000.0 {
                            "None".into()
                        } else {
                            format!("{x:.1}")
                        }
                    }));
                    if resp.changed() {
                        set.push((id, v));
                    }
                };
                num(ui, set, b + PRE_LEVEL, true, false);
                num(ui, set, b + PRE_DELAY, false, false);
                num(ui, set, b + PRE_HIGH_CUT, false, true);
                let labels: Vec<&str> = chans.iter().map(|c| c.label()).collect();
                // (sources are picked among this format's channels)
                let pick_src = |ui: &mut egui::Ui, set: &mut Set, id: u32| {
                    let cur = Ch::ALL[(get(params, values, set, id).round() as usize).min(16)];
                    egui::ComboBox::from_id_salt(("qrs-src", id)).width(48.0).selected_text(cur.label()).show_ui(ui, |ui| {
                        for (k, l) in labels.iter().enumerate() {
                            if ui.selectable_label(chans[k] == cur, *l).clicked() {
                                set.push((id, Ch::ALL.iter().position(|x| *x == chans[k]).unwrap_or(0) as f32));
                            }
                        }
                    });
                };
                pick_src(ui, set, b + DRY_SOURCE);
                pick_src(ui, set, b + REFL_SOURCE);
                for (k, off) in OFFSETS.iter().enumerate() {
                    // (spread only for channel pairs, not C, S or LFE)
                    if k == 1 && matches!(c, Ch::C | Ch::S | Ch::Lfe) {
                        ui.label("—");
                        continue;
                    }
                    num(ui, set, b + off, k >= 4, false);
                }
                combo(ui, set, b + PATTERN, patterns);
                ui.end_row();
            }
        });
    });
}
