//! Match EQ's editor view, after the guide: Current / Reference Learn and Match with each
//! spectrum's Action menu (Clear, Copy / Paste Spectrum, Load Spectrum from a settings file,
//! Generate from an audio file); the display buttons (Current, Reference, EQ Curve, Analyzer
//! with Pre / Post) and the curves (the EQ curve yellow, dimmed while a spectrum shows too);
//! editing the EQ curve with the pointer (vertical = level at that frequency, horizontal = move
//! it, Shift after starting = Q, Option-click = remove the manual changes, a help tag with
//! frequency / gain / Q); the scales (the right one, and the left one while the Analyzer is off,
//! drag a gain offset; with the Analyzer on the left one moves the spectra's 0 dB line); the
//! Limit Boost / Cut lines and the Ease-In / Ease-Out handles, each pair also dragged together;
//! Channel (L&R / L / R) with Hide Others and Channel Link; Apply (Control-click = 100 %) and
//! Smoothing. Phase sits in the extended parameters under the footer.

use egui::{Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_fx::match_eq::*;

use crate::look;

const YELLOW: Color32 = Color32::from_rgb(232, 200, 72);
const GREEN: Color32 = Color32::from_rgb(96, 206, 120);
const PURPLE: Color32 = Color32::from_rgb(178, 128, 236);
const RIGHT_RED: Color32 = Color32::from_rgb(232, 112, 104);

fn val(params: &[Param], values: &[f32], id: u32) -> f32 {
    params.iter().position(|p| p.id == id).map(|i| values[i]).unwrap_or(0.0)
}

/// A bell of `db` at `f` with `q` over the 63 bands (a manual change).
pub fn bell(f: f32, db: f32, q: f32) -> Vec<f32> {
    let oct = (1.0 / (2.0 * q.max(0.1))).asinh() / std::f32::consts::LN_2;
    (0..BANDS).map(|k| {
        let d = (band_hz(k) / f).log2() / oct.max(0.05);
        db * (-d * d * 1.4).exp()
    }).collect()
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Curve { f: f32, q: f32 },
    Limit(u32),
    Limits,
    Ease(u32),
    Eases(u32, u32),
}

/// The view's spectrum clipboard (Copy / Paste Current or Reference Spectrum).
fn clip_id() -> egui::Id {
    egui::Id::unique("match-eq-spectrum-clipboard")
}

pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &crate::fxviews::Live, slot: (usize, usize), set: &mut Vec<(u32, f32)>, act: &mut Option<crate::fxviews::Action>) {
    let g = |id| val(params, values, id);
    let on = |id| g(id) >= 0.5;
    let id = egui::Id::unique(("match-eq-view", slot));
    // the Learn buttons, Match and the Action menus
    ui.horizontal(|ui| {
        for (pid, n, action_ref) in [(CURRENT_LEARN, "Current", false), (REFERENCE_LEARN, "Reference", true)] {
            if ui.add(egui::Button::selectable(on(pid), format!("{n} Learn"))).clicked() {
                set.push((pid, if on(pid) { 0.0 } else { 1.0 }));
            }
            ui.menu_button("⌄", |ui| {
                let base = if action_ref { REFERENCE } else { CURRENT };
                let has = if action_ref { HAS_REFERENCE } else { HAS_CURRENT };
                if ui.button(format!("Clear {n} Spectrum")).clicked() {
                    set.push((has, 0.0));
                    ui.close();
                }
                if ui.button(format!("Copy {n} Spectrum")).clicked() {
                    let spec: Vec<f32> = (0..BANDS).map(|k| g(base + k as u32)).collect();
                    ui.ctx().data_mut(|d| d.insert_temp(clip_id(), spec));
                    ui.close();
                }
                let clip: Option<Vec<f32>> = ui.ctx().data(|d| d.get_temp(clip_id()));
                if ui.add_enabled(clip.is_some(), egui::Button::new(format!("Paste {n} Spectrum"))).clicked() {
                    if let Some(c) = clip {
                        for (k, v) in c.iter().enumerate().take(BANDS) {
                            set.push((base + k as u32, *v));
                        }
                        set.push((has, 1.0));
                    }
                    ui.close();
                }
                ui.menu_button("Load Spectrum from Setting File", |ui| {
                    let saved = crate::pluginwin::saved_settings("Match EQ");
                    if saved.is_empty() {
                        ui.label(egui::RichText::new("No saved Match EQ settings").color(look::text_dim()));
                    }
                    for (sn, path) in saved {
                        if ui.button(&sn).clicked() {
                            if let Some(pairs) = crate::pluginwin::read_setting(&path) {
                                // the setting's own spectrum of the same kind
                                let mut got = false;
                                for (pid, v) in pairs {
                                    if (base..base + BANDS as u32).contains(&pid) {
                                        set.push((pid, v));
                                        got = true;
                                    }
                                }
                                if got {
                                    set.push((has, 1.0));
                                }
                            }
                            ui.close();
                        }
                    }
                });
                if ui.button(format!("Generate {n} Spectrum from Audio File…")).clicked() {
                    *act = Some(crate::fxviews::Action::LearnFromFile(action_ref));
                    ui.close();
                }
            });
            ui.add_space(8.0);
        }
        if ui.add(egui::Button::selectable(on(MATCH), "Match")).clicked() {
            set.push((MATCH, if on(MATCH) { 0.0 } else { 1.0 }));
        }
    });
    // the display buttons
    ui.horizontal(|ui| {
        for (pid, n) in [(SHOW_CURRENT, "Current"), (SHOW_REFERENCE, "Reference"), (SHOW_EQ, "EQ Curve"), (ANALYZER, "Analyzer")] {
            let enabled = pid != SHOW_EQ || on(MATCH);
            if ui.add_enabled(enabled, egui::Button::selectable(on(pid) && enabled, n)).clicked() {
                set.push((pid, if on(pid) { 0.0 } else { 1.0 }));
            }
        }
        let post = g(ANALYZER_POST) >= 0.5;
        if ui.add(egui::Button::selectable(false, if post { "Post" } else { "Pre" })).on_hover_text("Analyzer before or after the EQ curve").clicked() {
            set.push((ANALYZER_POST, if post { 0.0 } else { 1.0 }));
        }
    });
    // the display
    let w = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(vec2(w, 240.0), Sense::hover());
    let disp = Rect::from_min_max(pos2(outer.min.x + 34.0, outer.min.y + 6.0), pos2(outer.max.x - 34.0, outer.max.y - 18.0));
    let p = ui.painter().with_clip_rect(outer);
    p.rect_filled(outer, 6.0, Color32::from_rgb(22, 30, 42));
    let x_of = |f: f32| disp.min.x + (f.clamp(20.0, 20000.0) / 20.0).log10() / 3.0 * disp.width();
    let f_of = |x: f32| 20.0 * 10f32.powf(((x - disp.min.x) / disp.width()).clamp(0.0, 1.0) * 3.0);
    let eq_y = |db: f32| disp.center().y - db / 24.0 * disp.height() / 2.0;
    let ref_id = id.with("spec-ref");
    let spec_top: f32 = ui.ctx().data(|d| d.get_temp(ref_id)).unwrap_or(-10.0);
    let spec_y = |db: f32| disp.min.y + ((spec_top - db) / 100.0).clamp(0.0, 1.0) * disp.height();
    let grid = Color32::from_rgb(44, 56, 74);
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
        p.line_segment([pos2(x_of(f), disp.min.y), pos2(x_of(f), disp.max.y)], Stroke::new(0.7, grid));
        let t = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") };
        p.text(pos2(x_of(f), disp.max.y + 3.0), Align2::CENTER_TOP, t, look::regular(10.0), look::text_dim());
    }
    for d in [-18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0] {
        p.line_segment([pos2(disp.min.x, eq_y(d)), pos2(disp.max.x, eq_y(d))], Stroke::new(if d == 0.0 { 1.0 } else { 0.5 }, grid));
        p.text(pos2(disp.max.x + 16.0, eq_y(d)), Align2::CENTER_CENTER, format!("{d:+.0}"), look::regular(9.5), look::text_dim());
    }
    for i in 0..=5 {
        let db = spec_top - i as f32 * 20.0;
        p.text(pos2(outer.min.x + 16.0, spec_y(db)), Align2::CENTER_CENTER, format!("{db:.0}"), look::regular(9.5), if on(ANALYZER) { look::text_dim() } else { look::text_dim().gamma_multiply(0.5) });
    }
    let chan = g(CHANNEL) as usize;
    let hide = on(HIDE_OTHERS) && chan > 0;
    let spec_line = |c: &[f32], col: Color32| {
        if c.len() == BANDS {
            let pts: Vec<Pos2> = (0..BANDS).map(|k| pos2(x_of(band_hz(k)), spec_y(c[k]))).collect();
            p.add(egui::Shape::line(pts, Stroke::new(1.4, col)));
        }
    };
    let show_spec = (on(SHOW_CURRENT) && on(HAS_CURRENT)) || (on(SHOW_REFERENCE) && on(HAS_REFERENCE));
    if on(SHOW_CURRENT) && on(HAS_CURRENT) {
        spec_line(live.curve("current"), GREEN);
    }
    if on(SHOW_REFERENCE) && on(HAS_REFERENCE) {
        spec_line(live.curve("reference"), PURPLE);
    }
    if on(ANALYZER) {
        let a = live.curve("analyzer");
        if !a.is_empty() {
            let n = a.len();
            let pts: Vec<Pos2> = (0..n).map(|k| pos2(x_of(20.0 * 1000f32.powf(k as f32 / (n - 1) as f32)), spec_y(a[k]))).collect();
            p.add(egui::Shape::line(pts, Stroke::new(1.0, Color32::from_rgba_unmultiplied(170, 200, 235, 160))));
            ui.ctx().request_repaint();
        }
    }
    let eq_visible = on(SHOW_EQ) && on(MATCH);
    if eq_visible {
        // yellow; dimmed while a spectrum is shown with it. L&R: the left curve (and the right
        // one where they differ); L or R: that channel, the other dimmed unless Hide Others
        let curves = [live.curve("eq"), live.curve("eq_r")];
        for (ch, c) in curves.iter().enumerate() {
            if c.len() != BANDS {
                continue;
            }
            let mine = chan == 0 || chan == ch + 1;
            if !mine && hide {
                continue;
            }
            let mut col = if ch == 1 && chan != 2 && curves[0] != curves[1] { RIGHT_RED } else { YELLOW };
            if show_spec || !mine {
                col = col.gamma_multiply(0.5);
            }
            if ch == 1 && chan == 0 && curves[0] == curves[1] {
                continue;
            }
            let pts: Vec<Pos2> = (0..BANDS).map(|k| pos2(x_of(band_hz(k)), eq_y(c[k]))).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
        }
    }
    // the limit lines and the ease handles
    let handle = |pt: Pos2, col: Color32| {
        p.circle_filled(pt, 4.5, col);
    };
    if eq_visible {
        for (flag, maxid, sign) in [(LIMIT_BOOST, BOOST_MAX, 1.0), (LIMIT_CUT, CUT_MAX, -1.0)] {
            if on(flag) {
                let y = eq_y(sign * g(maxid));
                p.line_segment([pos2(disp.min.x, y), pos2(disp.max.x, y)], Stroke::new(1.0, look::text_dim()));
                handle(pos2(disp.max.x - 10.0, y), look::text());
            }
        }
    }
    for (flag, lo, hi) in [(EASE_IN, EASE_IN_LO, EASE_IN_HI), (EASE_OUT, EASE_OUT_LO, EASE_OUT_HI)] {
        if on(flag) {
            let (a, b) = (x_of(g(lo)), x_of(g(hi)));
            p.rect_filled(Rect::from_min_max(pos2(a, disp.max.y - 10.0), pos2(b, disp.max.y)), 2.0, Color32::from_white_alpha(18));
            for x in [a, b] {
                p.line_segment([pos2(x, disp.min.y), pos2(x, disp.max.y)], Stroke::new(0.8, look::text_dim()));
                handle(pos2(x, disp.max.y - 5.0), look::text());
            }
        }
    }
    // interaction
    let resp = ui.interact(disp, id.with("disp"), Sense::click_and_drag());
    let dstate = id.with("drag");
    let mut drag: Option<Drag> = ui.data(|d| d.get_temp(dstate));
    let pt = resp.hover_pos();
    let near = |a: f32, b: f32| (a - b).abs() < 6.0;
    // what a press here grabs
    let grab = |pt: Pos2| -> Option<Drag> {
        if eq_visible {
            for (flag, maxid, sign) in [(LIMIT_BOOST, BOOST_MAX, 1.0), (LIMIT_CUT, CUT_MAX, -1.0)] {
                if on(flag) && near(pt.y, eq_y(sign * g(maxid))) {
                    return Some(Drag::Limit(maxid));
                }
            }
            if on(LIMIT_BOOST) && on(LIMIT_CUT) && pt.y > eq_y(g(BOOST_MAX)) && pt.y < eq_y(-g(CUT_MAX)) && pt.x > disp.max.x - 24.0 {
                return Some(Drag::Limits);
            }
        }
        if pt.y > disp.max.y - 12.0 {
            for (flag, lo, hi) in [(EASE_IN, EASE_IN_LO, EASE_IN_HI), (EASE_OUT, EASE_OUT_LO, EASE_OUT_HI)] {
                if on(flag) {
                    if near(pt.x, x_of(g(lo))) {
                        return Some(Drag::Ease(lo));
                    }
                    if near(pt.x, x_of(g(hi))) {
                        return Some(Drag::Ease(hi));
                    }
                    if pt.x > x_of(g(lo)) && pt.x < x_of(g(hi)) {
                        return Some(Drag::Eases(lo, hi));
                    }
                }
            }
        }
        if eq_visible {
            let f = f_of(pt.x);
            let k = (0..BANDS).min_by(|a, b| (band_hz(*a) / f).ln().abs().total_cmp(&(band_hz(*b) / f).ln().abs())).unwrap_or(0);
            let _ = k;
            return Some(Drag::Curve { f, q: 1.0 });
        }
        None
    };
    if let Some(pt) = pt {
        let icon = match drag.or_else(|| grab(pt)) {
            Some(Drag::Limit(_)) | Some(Drag::Limits) => CursorIcon::ResizeVertical,
            Some(Drag::Ease(_)) | Some(Drag::Eases(..)) => CursorIcon::ResizeHorizontal,
            Some(Drag::Curve { .. }) => CursorIcon::Crosshair,
            None => CursorIcon::Default,
        };
        ui.ctx().set_cursor_icon(icon);
    }
    let mods = ui.input(|i| i.modifiers);
    if resp.clicked() && mods.alt && eq_visible {
        // Option-click: the manual changes go
        for k in 0..BANDS {
            set.push((USER_L + k as u32, 0.0));
            set.push((USER_R + k as u32, 0.0));
        }
    }
    if resp.drag_started()
        && let Some(pt) = resp.interact_pointer_pos()
    {
        drag = grab(pt);
        ui.data_mut(|d| d.insert_temp(id.with("start"), pt));
        let snapshot: Vec<f32> = (0..BANDS).flat_map(|k| [g(USER_L + k as u32), g(USER_R + k as u32)]).collect();
        ui.data_mut(|d| d.insert_temp(id.with("snap"), snapshot));
    }
    if resp.dragged()
        && let (Some(d), Some(pt)) = (drag, resp.interact_pointer_pos())
    {
        let start: Pos2 = ui.data(|d| d.get_temp(id.with("start"))).unwrap_or(pt);
        let dv = pt - start;
        let per_db = disp.height() / 48.0;
        match d {
            Drag::Limit(maxid) => {
                let sign = if maxid == BOOST_MAX { 1.0 } else { -1.0 };
                set.push((maxid, (sign * -(pt.y - disp.center().y) / per_db).clamp(0.0, 24.0)));
            }
            Drag::Limits => {
                let delta = -resp.drag_delta().y / per_db;
                set.push((BOOST_MAX, (g(BOOST_MAX) + delta).clamp(0.0, 24.0)));
                set.push((CUT_MAX, (g(CUT_MAX) - delta).clamp(0.0, 24.0)));
            }
            Drag::Ease(pid) => set.push((pid, f_of(pt.x))),
            Drag::Eases(lo, hi) => {
                let k = 2f32.powf(resp.drag_delta().x / (disp.width() / 1000f32.log2()));
                set.push((lo, g(lo) * k));
                set.push((hi, g(hi) * k));
            }
            Drag::Curve { f, q } => {
                // vertical = level, horizontal = where; Shift (after starting) = Q
                let snap: Vec<f32> = ui.data(|d| d.get_temp(id.with("snap"))).unwrap_or_default();
                let (gain, at, qq) = if mods.shift { (0.0, f, (q * 2f32.powf(dv.x / 50.0)).clamp(0.2, 20.0)) } else { (-dv.y / per_db, f * 2f32.powf(dv.x / (disp.width() / 1000f32.log2())), q) };
                let qq = if mods.shift { qq } else { ui.data(|d| d.get_temp::<f32>(id.with("q"))).unwrap_or(qq) };
                if mods.shift {
                    ui.data_mut(|d| d.insert_temp(id.with("q"), qq));
                }
                let gain = if mods.shift { ui.data(|d| d.get_temp::<f32>(id.with("gain"))).unwrap_or(0.0) } else { gain };
                ui.data_mut(|d| d.insert_temp(id.with("gain"), gain));
                let b = bell(at, gain, qq);
                // Channel and Channel Link: edits on one side reach the other by the link amount
                let link = g(CHANNEL_LINK) / 100.0;
                let (wl, wr) = match chan {
                    1 => (1.0, link),
                    2 => (link, 1.0),
                    _ => (1.0, 1.0),
                };
                for k in 0..BANDS {
                    let (l0, r0) = (snap.get(2 * k).copied().unwrap_or(0.0), snap.get(2 * k + 1).copied().unwrap_or(0.0));
                    set.push((USER_L + k as u32, (l0 + b[k] * wl).clamp(-30.0, 30.0)));
                    set.push((USER_R + k as u32, (r0 + b[k] * wr).clamp(-30.0, 30.0)));
                }
                // the help tag
                let tip = format!("{:.0} Hz  {:+.1} dB  Q {:.2}", at, gain, qq);
                let tp = ui.painter().clone();
                let gal = tp.layout_no_wrap(tip, look::regular(11.0), Color32::WHITE);
                let tr = Rect::from_min_size(pt + vec2(12.0, -26.0), gal.size() + vec2(10.0, 6.0));
                tp.rect_filled(tr, 4.0, Color32::from_black_alpha(200));
                tp.galley(tr.min + vec2(5.0, 3.0), gal, Color32::WHITE);
            }
        }
    }
    if resp.drag_stopped() {
        drag = None;
        ui.data_mut(|d| {
            d.remove::<f32>(id.with("q"));
            d.remove::<f32>(id.with("gain"));
        });
    }
    ui.data_mut(|d| match drag {
        Some(x) => {
            d.insert_temp(dstate, x);
        }
        None => d.remove::<Drag>(dstate),
    });
    // the scales: right = gain offset; left = offset too, or (Analyzer on) the spectra's 0 dB line
    let lr = Rect::from_min_max(outer.min, pos2(disp.min.x, outer.max.y));
    let rr = Rect::from_min_max(pos2(disp.max.x, outer.min.y), outer.max);
    let lresp = ui.interact(lr, id.with("lscale"), Sense::drag()).on_hover_cursor(CursorIcon::ResizeVertical);
    let rresp = ui.interact(rr, id.with("rscale"), Sense::drag()).on_hover_cursor(CursorIcon::ResizeVertical);
    let per_db = disp.height() / 48.0;
    if rresp.dragged() || (lresp.dragged() && !on(ANALYZER)) {
        let dy = rresp.drag_delta().y + if on(ANALYZER) { 0.0 } else { lresp.drag_delta().y };
        set.push((GAIN_OFFSET, (g(GAIN_OFFSET) - dy / per_db).clamp(-24.0, 24.0)));
    } else if lresp.dragged() {
        let top = (spec_top + lresp.drag_delta().y * 100.0 / disp.height()).clamp(-60.0, 40.0);
        ui.data_mut(|d| d.insert_temp(ref_id, top));
    }
    // the controls below
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Apply");
        let mut a = g(APPLY);
        let r = ui.add(egui::Slider::new(&mut a, -100.0..=200.0).suffix(" %"));
        if r.changed() {
            set.push((APPLY, a));
        }
        if r.secondary_clicked() {
            // (Control-click: 100 %)
            set.push((APPLY, 100.0));
        }
        ui.add_space(12.0);
        ui.label("Smoothing");
        let mut s = g(SMOOTHING);
        if ui.add(egui::Slider::new(&mut s, 0.0..=100.0).suffix(" %")).changed() {
            set.push((SMOOTHING, s));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Channel");
        for (i, n) in ["L&R", "L", "R"].iter().enumerate() {
            if ui.add(egui::Button::selectable(chan == i, *n)).clicked() {
                set.push((CHANNEL, i as f32));
            }
        }
        let mut h = on(HIDE_OTHERS);
        if ui.add_enabled(chan > 0, egui::Checkbox::new(&mut h, "Hide Others")).changed() {
            set.push((HIDE_OTHERS, if h { 1.0 } else { 0.0 }));
        }
        ui.add_space(12.0);
        ui.label("Channel Link");
        let mut l = g(CHANNEL_LINK);
        if ui.add(egui::Slider::new(&mut l, 0.0..=100.0).suffix(" %")).changed() {
            set.push((CHANNEL_LINK, l));
        }
    });
    ui.horizontal(|ui| {
        for (pid, n) in [(LIMIT_BOOST, "Limit Boost"), (LIMIT_CUT, "Limit Cut"), (EASE_IN, "Ease-In"), (EASE_OUT, "Ease-Out")] {
            let mut b = on(pid);
            if ui.checkbox(&mut b, n).changed() {
                set.push((pid, if b { 1.0 } else { 0.0 }));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bell_peaks_at_its_frequency() {
        let b = bell(1000.0, 6.0, 1.0);
        let k = (0..BANDS).max_by(|a, c| b[*a].total_cmp(&b[*c])).unwrap();
        assert!((band_hz(k) / 1000.0).log2().abs() < 0.2);
        assert!(b[k] > 5.0 && b[0].abs() < 0.1);
        let narrow = bell(1000.0, 6.0, 8.0);
        assert!(narrow.iter().filter(|v| **v > 1.0).count() < b.iter().filter(|v| **v > 1.0).count());
    }
}
