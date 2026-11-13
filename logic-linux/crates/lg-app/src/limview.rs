//! Logic's Limiter editor: the Input / Reduction / Output meters on the dark left panel, and on
//! the blue right panel the Gain, Release, Output Level and Lookahead knobs, the Mode pop-up
//! (Traditional / Precision) and the True Peak Detection button.
//!
//! Positions were measured on Logic's own window (screenshots/ui-plugin-window-limiter.png, at
//! 75 %): the numbers are that picture's pixels, turned into points by `ngview::Geo`.

use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;

use crate::fxviews::Live;
use crate::look;
use crate::ngview::{GREEN, Geo, LABEL, SCALE, knob, prm, val, value_text};

const X0: f32 = 4.0;
const Y0: f32 = 206.0;
const BODY_W: f32 = 1450.0;
const BODY_H: f32 = 502.0;

/// Logic's window at 100 %: 967 × 527 points with its header and footer; ours adds egui's title bar.
pub const SIZE: egui::Vec2 = egui::vec2(955.0, 515.0);

/// The meters' scales: (value, picture y) from the top mark to the bottom one.
const IN_MARKS: [(f32, &str); 14] = [(20.0, "20"), (12.0, "12"), (9.0, "9"), (6.0, "6"), (3.0, "3"), (0.0, "0"), (-3.0, "-3"), (-6.0, "-6"), (-9.0, "-9"), (-12.0, "-12"), (-15.0, "-15"), (-20.0, "-20"), (-30.0, "-30"), (-60.0, "-60")];
const RED_MARKS: [(f32, &str); 14] = [(0.0, "0"), (1.5, "1.5"), (3.0, "3"), (6.0, "6"), (9.0, "9"), (12.0, "12"), (15.0, "15"), (20.0, "20"), (25.0, "25"), (30.0, "30"), (35.0, "35"), (40.0, "40"), (50.0, "50"), (60.0, "60")];
const OUT_MARKS: [(f32, &str); 14] = [(0.0, "0"), (-1.5, "-1.5"), (-3.0, "-3"), (-6.0, "-6"), (-9.0, "-9"), (-12.0, "-12"), (-15.0, "-15"), (-20.0, "-20"), (-25.0, "-25"), (-30.0, "-30"), (-35.0, "-35"), (-40.0, "-40"), (-50.0, "-50"), (-60.0, "-60")];
const TOP: f32 = 323.0;
const STEP: f32 = 27.0;

/// Where a value sits on a scale (picture y), between its marks.
fn mark_y(marks: &[(f32, &str)], v: f32, falling: bool) -> f32 {
    let n = marks.len();
    for k in 0..n - 1 {
        let (a, b) = (marks[k].0, marks[k + 1].0);
        let inside = if falling { v <= a && v >= b } else { v >= a && v <= b };
        if inside {
            return TOP + STEP * (k as f32 + (v - a) / (b - a));
        }
    }
    let first = if falling { v > marks[0].0 } else { v < marks[0].0 };
    if first { TOP } else { TOP + STEP * (n - 1) as f32 }
}

fn one(v: f32) -> String {
    format!("{v:.1}")
}

pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, slot: (usize, usize), set: &mut Vec<(u32, f32)>) {
    let get = |id: u32, set: &Vec<(u32, f32)>| set.iter().rev().find(|x| x.0 == id).map_or(val(params, values, id), |x| x.1);
    let margin = ui.spacing().window_margin;
    let w = ui.available_width() + (margin.left + margin.right) as f32;
    let s = w / BODY_W;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), BODY_H * s), Sense::hover());
    let body = Rect::from_min_max(pos2(r.min.x - margin.left as f32, r.min.y), pos2(r.max.x + margin.right as f32, r.max.y));
    let g = Geo { o: body.min, s, x0: X0, y0: Y0 };
    let pt = ui.painter().with_clip_rect(body);
    let id = egui::Id::unique(("limiter-view", slot));
    // the panels: dark meters on the left, Logic's blue on the right
    let split = g.at(598.0, Y0).x;
    let grad = |rect: Rect, top: Color32, bot: Color32| {
        let mut m = egui::Mesh::default();
        m.colored_vertex(rect.left_top(), top);
        m.colored_vertex(rect.right_top(), top);
        m.colored_vertex(rect.right_bottom(), bot);
        m.colored_vertex(rect.left_bottom(), bot);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        pt.add(m);
    };
    grad(Rect::from_min_max(body.min, pos2(split, body.max.y)), Color32::from_rgb(0x16, 0x1e, 0x2a), Color32::from_rgb(0x12, 0x19, 0x24));
    grad(Rect::from_min_max(pos2(split, body.min.y), body.max), Color32::from_rgb(0x2e, 0x41, 0x59), Color32::from_rgb(0x2a, 0x3a, 0x52));

    // the meters: titles, dB scales, grid lines and the bars
    let input = live.get("Input").unwrap_or(-120.0);
    let red = live.get("Reduction").unwrap_or(0.0).max(0.0);
    let output = live.get("Output").unwrap_or(-120.0);
    let cols: [(&str, f32, f32, f32, &[(f32, &str)], f32, bool); 3] = [
        ("INPUT", 160.0, 79.0, 98.0, &IN_MARKS, input, true),
        ("REDUCTION", 317.0, 271.0, 285.0, &RED_MARKS, red, false),
        ("OUTPUT", 483.0, 408.0, 421.0, &OUT_MARKS, output, true),
    ];
    // the Margin fields: the highest level (most reduction) since the last click on them
    let mid = egui::Id::unique(("limiter-margins", slot));
    let mut margins: [f32; 3] = ui.data(|d| d.get_temp(mid)).unwrap_or([-120.0, 0.0, -120.0]);
    for (k, (title, tx, lx, x0, marks, v, falling)) in cols.into_iter().enumerate() {
        let x1 = x0 + if title == "INPUT" { 124.0 } else { 128.0 };
        margins[k] = margins[k].max(v);
        let mr = g.rect(x0 + 14.0, 244.0, x1 - 14.0, 264.0);
        let resp = ui.interact(mr, id.with(("margin", k)), Sense::click()).on_hover_text("Margin: click to reset");
        if resp.clicked() {
            margins[k] = if falling { -120.0 } else { 0.0 };
        }
        // (blank until there's a level to hold, as Logic's)
        let idle = if falling { margins[k] <= -119.0 } else { margins[k] < 0.05 };
        let shown = if idle { String::new() } else { one(margins[k]) };
        let hot = falling && margins[k] > 0.0;
        pt.text(mr.center(), Align2::CENTER_CENTER, shown, look::regular(g.pt(16.0)), if hot { Color32::from_rgb(0xff, 0x5a, 0x4f) } else { Color32::from_rgb(0xd8, 0xde, 0xe6) });
        pt.text(g.at(tx, 231.0), Align2::CENTER_CENTER, title, look::regular(g.pt(21.0)), Color32::from_rgb(0x9a, 0xa5, 0xb4));
        pt.text(g.at(lx, 283.0), Align2::RIGHT_CENTER, "dB", look::regular(g.pt(16.0)), SCALE.gamma_multiply(0.7));
        for (k, (_, t)) in marks.iter().enumerate() {
            let y = TOP + STEP * k as f32;
            let zero = *t == "0";
            pt.text(g.at(lx, y), Align2::RIGHT_CENTER, *t, look::regular(g.pt(16.0)), if zero { SCALE } else { SCALE.gamma_multiply(0.7) });
            pt.line_segment([g.at(x0, y), g.at(x1, y)], Stroke::new(g.pt(1.0), Color32::from_white_alpha(if zero { 60 } else { 10 })));
        }
        // the column's centre line and outline, as Logic's
        pt.line_segment([g.at((x0 + x1) / 2.0, 270.0), g.at((x0 + x1) / 2.0, 676.0)], Stroke::new(g.pt(1.0), Color32::from_white_alpha(10)));
        pt.rect_stroke(g.rect(x0, 270.0, x1, 676.0), 0.0, Stroke::new(g.pt(1.0), Color32::from_white_alpha(12)), egui::StrokeKind::Inside);
        // the bar: levels rise from the bottom, reduction falls from the top
        let (ya, yb) = if falling { (mark_y(marks, v, true), 676.0) } else { (TOP, mark_y(marks, v, false)) };
        if yb - ya > 0.5 && (falling && v > -60.0 || !falling && v > 0.05) {
            let c = if falling { Color32::from_rgb(0x6c, 0xd0, 0x7a) } else { Color32::from_rgb(0xe8, 0xa0, 0x3c) };
            pt.rect_filled(g.rect(x0 + 34.0, ya, x1 - 34.0, yb), 0.0, c);
        }
    }

    ui.data_mut(|d| d.insert_temp(mid, margins));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));

    // the knobs: Gain, Release, Output Level, Lookahead
    let knobs: [(u32, &str, (f32, f32), f32, f32); 4] = [
        (0, "Gain", (723.0, 425.0), 62.0, 332.0),
        (3, "Release", (937.0, 412.0), 55.0, 335.0),
        (4, "Output Level", (1141.0, 412.0), 50.0, 332.0),
        (1, "Lookahead", (1342.0, 412.0), 55.0, 335.0),
    ];
    for (pid, name, c, rad, vy) in knobs {
        let p = prm(params, pid);
        let v = get(pid, set);
        pt.text(g.at(c.0, 299.0), Align2::CENTER_CENTER, name, look::regular(g.pt(22.0)), LABEL);
        value_text(&pt, &g, g.at(c.0, vy), Align2::CENTER_CENTER, &one(v), crate::ngview::unit(p.unit));
        let ends = (format!("{}", p.min.round()), format!("{}", p.max.round()));
        if let Some(nv) = knob(ui, &pt, &g, c, rad, id.with(pid), &p, v, (&ends.0, &ends.1)) {
            set.push((pid, nv));
        }
    }

    // Mode: Traditional / Precision
    let precision = get(2, set) >= 0.5;
    pt.text(g.at(723.0, 567.0), Align2::CENTER_CENTER, "Mode", look::regular(g.pt(22.0)), LABEL);
    let mr = Rect::from_center_size(g.at(723.0, 601.0), vec2(g.pt(200.0), g.pt(40.0)));
    pt.text(mr.center(), Align2::CENTER_CENTER, if precision { "Precision ⇕" } else { "Traditional ⇕" }, look::regular(g.pt(30.0)), GREEN);
    let mresp = ui.interact(mr, id.with("mode"), Sense::click()).on_hover_text("Mode");
    egui::Popup::menu(&mresp).show(|ui| {
        for (k, n) in ["Traditional", "Precision"].into_iter().enumerate() {
            if ui.add(egui::Button::selectable(precision == (k == 1), n)).clicked() {
                set.push((2, k as f32));
                ui.close();
            }
        }
    });
    // True Peak Detection
    let tp = get(6, set) >= 0.5;
    pt.text(g.at(938.0, 567.0), Align2::CENTER_CENTER, "True Peak Detection", look::regular(g.pt(22.0)), LABEL);
    let b = g.rect(908.0, 590.0, 962.0, 630.0);
    pt.rect_filled(b, g.pt(4.0), if tp { Color32::from_rgb(0x1f, 0x2d, 0x3d) } else { Color32::from_rgb(0x3a, 0x48, 0x5c) });
    pt.text(b.center(), Align2::CENTER_CENTER, if tp { "ON" } else { "OFF" }, look::regular(g.pt(20.0)), if tp { GREEN } else { LABEL });
    if ui.interact(b, id.with("tp"), Sense::click()).clicked() {
        set.push((6, if tp { 0.0 } else { 1.0 }));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn meter_marks_place_values_between_them() {
        assert_eq!(super::mark_y(&super::IN_MARKS, 20.0, true), super::TOP);
        assert_eq!(super::mark_y(&super::IN_MARKS, 0.0, true), super::TOP + 5.0 * super::STEP);
        assert_eq!(super::mark_y(&super::RED_MARKS, 2.25, false), super::TOP + 1.5 * super::STEP);
        assert_eq!(super::mark_y(&super::OUT_MARKS, -100.0, true), super::TOP + 13.0 * super::STEP);
    }
}
