//! Logic's Compressor editor: the seven circuit buttons, the input and output level meters with
//! their peak fields (click to reset), the gain-reduction display with its Meter / Graph buttons
//! (a VU needle, or the reduction over time), Threshold / Ratio / Make Up / Knee / Attack /
//! Release knobs, the Auto Gain buttons (Off, 0 dB, -12 dB) and Auto (release), and on the right
//! the Side Chain / Output buttons: Output shows the Limiter (on, LED, threshold) and Distortion
//! (Off / Soft / Hard / Clip); Side Chain shows detection (Max / Sum, Peak / RMS), the side-chain
//! filter (on, LP / BP / HP / ParEQ / HS, frequency, Q, gain) and Listen. Mix and Output Gain
//! stay below either.
//!
//! Positions were measured on Logic's own window (screenshots/ui-plugin-window-compressor.png,
//! Platinum Digital, 100 %): the numbers in `at` calls are that picture's pixels as displayed at
//! 2000 wide, turned into points by `at`. The circuit types' panel colours other than Platinum
//! Digital's blue, and the Side Chain page's layout, aren't in the research: they are Studio's
//! own, in the same style.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_fx::compressor as c;

use crate::fxviews::Live;
use crate::look;

/// The body below Logic's header at 100 % (points).
const BODY_W: f32 = 1028.0;
const BODY_H: f32 = 668.0;

/// The default window size (Logic's window: 1028 × 813 points with title, header and footer; ours adds egui's title bar).
pub const SIZE: egui::Vec2 = egui::vec2(1014.0, 846.0);

fn val(params: &[Param], values: &[f32], id: u32) -> f32 {
    params.iter().position(|p| p.id == id).map_or(0.0, |i| values[i])
}

fn prm(params: &[Param], id: u32) -> Param {
    params.iter().find(|p| p.id == id).cloned().unwrap_or(Param::new(id, "", 0.0, 1.0, 0.0, lg_dsp::Unit::None))
}

/// How a knob's travel maps to its value.
#[derive(Clone, Copy)]
enum Taper {
    Lin(f32, f32),
    Log(f32, f32),
    /// The marks equally spaced around the dial (Logic's attack, release and make-up scales).
    Marks,
}

struct Dial {
    taper: Taper,
    marks: &'static [(f32, &'static str)],
    /// Text under the knob ("dB", ":1", "ms").
    unit: &'static str,
}

impl Dial {
    fn frac(&self, v: f32) -> f32 {
        match self.taper {
            Taper::Lin(a, b) => ((v - a) / (b - a)).clamp(0.0, 1.0),
            Taper::Log(a, b) => ((v.max(a) / a).ln() / (b / a).ln()).clamp(0.0, 1.0),
            Taper::Marks => {
                let m = self.marks;
                let n = (m.len() - 1) as f32;
                if v <= m[0].0 {
                    return 0.0;
                }
                for k in 0..m.len() - 1 {
                    if v <= m[k + 1].0 {
                        return (k as f32 + (v - m[k].0) / (m[k + 1].0 - m[k].0)) / n;
                    }
                }
                1.0
            }
        }
    }
    fn value(&self, f: f32) -> f32 {
        let f = f.clamp(0.0, 1.0);
        match self.taper {
            Taper::Lin(a, b) => a + (b - a) * f,
            Taper::Log(a, b) => a * (b / a).powf(f),
            Taper::Marks => {
                let m = self.marks;
                let n = (m.len() - 1) as f32;
                let k = ((f * n).floor() as usize).min(m.len() - 2);
                let t = f * n - k as f32;
                m[k].0 + (m[k + 1].0 - m[k].0) * t
            }
        }
    }
}

const D_THRESHOLD: Dial = Dial { taper: Taper::Lin(-50.0, 0.0), marks: &[(-50.0, "-50"), (-40.0, "-40"), (-30.0, "-30"), (-20.0, "-20"), (-10.0, "-10"), (0.0, "0")], unit: "dB" };
const D_RATIO: Dial = Dial { taper: Taper::Log(1.0, 30.0), marks: &[(1.0, "1"), (1.4, "1.4"), (2.0, "2"), (3.0, "3"), (5.0, "5"), (8.0, "8"), (12.0, "12"), (20.0, "20"), (30.0, "30")], unit: ":1" };
const D_MAKEUP: Dial = Dial { taper: Taper::Marks, marks: &[(-20.0, "-20"), (-15.0, "-15"), (-10.0, "-10"), (-5.0, "-5"), (0.0, "0"), (5.0, "5"), (10.0, "10"), (15.0, "15"), (20.0, "20"), (30.0, "30"), (40.0, "40"), (50.0, "50")], unit: "dB" };
const D_KNEE: Dial = Dial { taper: Taper::Lin(0.0, 1.0), marks: &[(0.0, "0"), (0.2, "0.2"), (0.4, "0.4"), (0.6, "0.6"), (0.8, "0.8"), (1.0, "1.0")], unit: "" };
const D_ATTACK: Dial = Dial { taper: Taper::Marks, marks: &[(0.0, "0"), (5.0, "5"), (10.0, "10"), (15.0, "15"), (20.0, "20"), (50.0, "50"), (80.0, "80"), (120.0, "120"), (160.0, "160"), (200.0, "200")], unit: "ms" };
const D_RELEASE: Dial = Dial { taper: Taper::Marks, marks: &[(5.0, "5"), (10.0, "10"), (20.0, "20"), (50.0, "50"), (100.0, "100"), (200.0, "200"), (500.0, "500"), (1000.0, "1k"), (2000.0, "2k"), (5000.0, "5k")], unit: "ms" };
const D_GAIN: Dial = Dial { taper: Taper::Lin(-30.0, 30.0), marks: &[(-30.0, "-30"), (-15.0, ""), (0.0, "0"), (15.0, ""), (30.0, "30")], unit: "dB" };
const D_LIMIT: Dial = Dial { taper: Taper::Lin(-10.0, 0.0), marks: &[(-10.0, "-10"), (-8.0, "-8"), (-6.0, "-6"), (-4.0, "-4"), (-2.0, "-2"), (0.0, "0")], unit: "dB" };
const D_MIX: Dial = Dial { taper: Taper::Lin(0.0, 100.0), marks: &[(0.0, "Input"), (25.0, ""), (50.0, "1:1"), (75.0, ""), (100.0, "Output")], unit: "" };
const D_FREQ: Dial = Dial { taper: Taper::Log(20.0, 20000.0), marks: &[(20.0, "20"), (200.0, "200"), (2000.0, "2k"), (20000.0, "20k")], unit: "Hz" };
const D_Q: Dial = Dial { taper: Taper::Log(0.1, 100.0), marks: &[(0.1, "0.1"), (1.0, "1"), (10.0, "10"), (100.0, "100")], unit: "" };
const D_SCGAIN: Dial = Dial { taper: Taper::Lin(-24.0, 24.0), marks: &[(-24.0, "-24"), (0.0, "0"), (24.0, "24")], unit: "dB" };

/// The dial's start (7 o'clock) and sweep, clockwise in screen angles.
const START: f32 = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
const SWEEP: f32 = std::f32::consts::PI * 5.0 / 3.0;

/// One circuit type's panel colours (top and bottom of the gradient).
fn skin(circuit: usize) -> (Color32, Color32) {
    let c = |r, g, b| Color32::from_rgb(r, g, b);
    match circuit {
        1 => (c(58, 58, 61), c(30, 30, 33)),
        2 => (c(40, 40, 42), c(16, 16, 18)),
        3 => (c(72, 74, 78), c(42, 43, 46)),
        4 => (c(66, 70, 76), c(36, 39, 44)),
        5 => (c(84, 82, 78), c(48, 47, 44)),
        6 => (c(104, 92, 74), c(62, 53, 41)),
        _ => (c(51, 67, 88), c(31, 41, 59)),
    }
}

struct Geo {
    o: Pos2,
    s: f32,
}

impl Geo {
    /// A point measured on Logic's screenshot (displayed pixels) in the body.
    fn at(&self, xd: f32, yd: f32) -> Pos2 {
        self.o + vec2(xd * 0.514, yd * 0.514 - 102.8) * self.s
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.at(x0, y0), self.at(x1, y1))
    }
    fn pt(&self, v: f32) -> f32 {
        v * self.s
    }
}

const LABEL: Color32 = Color32::from_rgb(0xc4, 0xca, 0xd2);
const SCALE: Color32 = Color32::from_rgb(0xae, 0xb5, 0xbd);
const BLUE: Color32 = Color32::from_rgb(0x5a, 0xb8, 0xf5);

/// A silver knob with its scale; returns the new value when dragged or reset.
#[allow(clippy::too_many_arguments)]
fn knob(ui: &mut egui::Ui, g: &Geo, c: Pos2, d: f32, id: egui::Id, v: f32, p: &Param, dial: &Dial, angles: Option<&[f32]>) -> Option<f32> {
    let r = g.pt(d) / 2.0;
    let resp = ui.interact(Rect::from_center_size(c, vec2(r * 2.0 + 8.0, r * 2.0 + 8.0)), id, Sense::click_and_drag());
    let mut out = None;
    let steps = angles.map(|a| a.len());
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let dy = resp.drag_delta().y;
        if let Some(n) = steps {
            // a switch knob: the accumulated drag picks the position
            let acc_id = id.with("acc");
            let mut acc: f32 = ui.data(|dd| dd.get_temp(acc_id)).unwrap_or(v);
            if resp.drag_started() {
                acc = v;
            }
            acc = (acc - dy / 30.0).clamp(0.0, n as f32 - 1.0);
            ui.data_mut(|dd| dd.insert_temp(acc_id, acc));
            if acc.round() != v {
                out = Some(acc.round());
            }
        } else {
            let f = dial.frac(v) - dy / if fine { 600.0 } else { 150.0 };
            let nv = dial.value(f).clamp(p.min, p.max);
            if nv != v {
                out = Some(nv);
            }
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if resp.double_clicked() || (resp.clicked() && ui.input(|i| i.modifiers.alt)) {
        out = Some(p.default);
    } else if resp.clicked()
        && let Some(n) = steps
    {
        out = Some(((v.round() as usize + 1) % n) as f32);
    }
    let now = out.unwrap_or(v);
    let shown = if p.choices.is_empty() { crate::pluginwin::fmt(p, now) } else { p.choices.get(now.round() as usize).copied().unwrap_or("").to_string() };
    let resp = resp.on_hover_text(format!("{}: {shown}", p.name));
    if resp.dragged() {
        egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), id.with("tip"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(&shown);
        });
    }
    let pt = ui.painter();
    // the scale: ticks at the marks and halfway between, labels outside
    let angle = |f: f32| START + SWEEP * f;
    let tick = |a: f32, long: bool| {
        let u = vec2(a.cos(), a.sin());
        pt.line_segment([c + u * (r + g.pt(4.0)), c + u * (r + g.pt(if long { 8.5 } else { 7.0 }))], Stroke::new(g.pt(1.2), SCALE));
    };
    let lab_r = r + g.pt(if d > 70.0 { 22.0 } else { 20.0 });
    if let Some(a) = angles {
        for (k, ang) in a.iter().enumerate() {
            let rad = ang.to_radians();
            tick(rad, true);
            let u = vec2(rad.cos(), rad.sin());
            pt.text(c + u * (lab_r + g.pt(4.0)), Align2::CENTER_CENTER, p.choices.get(k).copied().unwrap_or(""), look::regular(g.pt(11.5)), SCALE);
        }
    } else {
        let m = dial.marks;
        for (k, (mv, ml)) in m.iter().enumerate() {
            let f = dial.frac(*mv);
            tick(angle(f), true);
            if k + 1 < m.len() {
                tick(angle(0.5 * (f + dial.frac(m[k + 1].0))), false);
            }
            if !ml.is_empty() {
                let u = vec2(angle(f).cos(), angle(f).sin());
                let w = ml.len() as f32 * g.pt(3.2);
                pt.text(c + u * (lab_r + w * u.x.abs() * 0.5), Align2::CENTER_CENTER, *ml, look::regular(g.pt(11.5)), SCALE);
            }
        }
    }
    if !dial.unit.is_empty() {
        pt.text(c + vec2(0.0, r + g.pt(if d > 70.0 { 19.0 } else { 20.0 })), Align2::CENTER_CENTER, dial.unit, look::regular(g.pt(11.5)), SCALE);
    }
    // the body: a soft shadow, a dark rim and a brushed silver cap lit from the top
    pt.circle_filled(c + vec2(0.0, g.pt(3.0)), r + g.pt(1.5), Color32::from_black_alpha(110));
    pt.circle_filled(c, r, Color32::from_gray(0x50));
    for k in 0..10 {
        let t = k as f32 / 9.0;
        let gr = (0x86 as f32 + (0xd2 - 0x86) as f32 * t) as u8;
        pt.circle_filled(c - vec2(0.0, r * 0.18 * t), r * (0.97 - 0.42 * t), Color32::from_gray(gr));
    }
    // the brushing: faint rings
    for k in 1..6 {
        pt.circle_stroke(c, r * (0.18 * k as f32), Stroke::new(g.pt(0.5), Color32::from_white_alpha(14)));
    }
    if resp.hovered() || resp.dragged() {
        pt.circle_filled(c, r * 0.96, Color32::from_white_alpha(14));
    }
    let a = match angles {
        Some(a) => a.get(now.round() as usize).copied().unwrap_or(a[0]).to_radians(),
        None => angle(dial.frac(now)),
    };
    let u = vec2(a.cos(), a.sin());
    pt.line_segment([c + u * (r * 0.62), c + u * (r * 0.96)], Stroke::new(g.pt(2.2), Color32::from_gray(0x12)));
    out
}

/// A grey push button (Logic's Compressor style): lit shows dark with blue text.
fn push(ui: &mut egui::Ui, g: &Geo, r: Rect, id: egui::Id, label: &str, lit: bool, size: f32) -> bool {
    let resp = ui.interact(r, id, Sense::click());
    let pt = ui.painter();
    pt.rect_filled(r.expand(g.pt(1.5)), g.pt(3.0), Color32::from_gray(0x16));
    let (top, bot) = if lit { (Color32::from_rgb(0x55, 0x59, 0x5f), Color32::from_rgb(0x4a, 0x4e, 0x54)) } else { (Color32::from_gray(0xa8), Color32::from_gray(0x96)) };
    gradient(pt, r, top, bot, g.pt(3.0));
    if resp.hovered() {
        pt.rect_filled(r, g.pt(3.0), Color32::from_white_alpha(16));
    }
    let col = if lit { BLUE } else { Color32::from_gray(0x3a) };
    let lines: Vec<&str> = label.split('\n').collect();
    let lh = g.pt(size * 1.25);
    for (k, l) in lines.iter().enumerate() {
        let y = r.center().y + (k as f32 - (lines.len() as f32 - 1.0) / 2.0) * lh;
        pt.text(pos2(r.center().x, y), Align2::CENTER_CENTER, *l, look::regular(g.pt(size)), col);
    }
    resp.clicked()
}

fn gradient(pt: &egui::Painter, r: Rect, top: Color32, bot: Color32, round: f32) {
    if round > 0.0 {
        pt.rect_filled(r, round, bot);
        pt.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x, r.center().y)), egui::CornerRadius { nw: round as u8, ne: round as u8, sw: 0, se: 0 }, top);
        return;
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(r.left_top(), top);
    mesh.colored_vertex(r.right_top(), top);
    mesh.colored_vertex(r.right_bottom(), bot);
    mesh.colored_vertex(r.left_bottom(), bot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    pt.add(mesh);
}

/// Logic's level meter scale: equal steps between these marks (dB).
const METER_MARKS: [f32; 11] = [3.0, 0.0, -3.0, -6.0, -9.0, -12.0, -18.0, -24.0, -30.0, -40.0, -60.0];

/// Where a level falls on the meter (0 at +3 dB, 1 at -60 dB).
pub fn meter_frac(db: f32) -> f32 {
    let m = METER_MARKS;
    if db >= m[0] {
        return 0.0;
    }
    for k in 0..m.len() - 1 {
        if db >= m[k + 1] {
            return (k as f32 + (m[k] - db) / (m[k] - m[k + 1])) / (m.len() - 1) as f32;
        }
    }
    1.0
}

fn db_text(v: f32) -> String {
    if v <= -119.0 { "-∞ dB".to_string() } else { format!("{v:.1} dB") }
}

/// A level meter (two bars), its scale and its peak field; true when the field was clicked.
#[allow(clippy::too_many_arguments)]
fn level_meter(ui: &mut egui::Ui, g: &Geo, x: f32, label_x: f32, tick_x: f32, l: f32, r: f32, peak: f32, id: egui::Id) -> bool {
    let pt = ui.painter().clone();
    // the peak field
    let field = g.rect(x - 55.0, 393.0, x + 55.0, 435.0);
    pt.rect_filled(field, g.pt(2.0), Color32::from_rgb(0x22, 0x2e, 0x3f));
    pt.text(field.center(), Align2::CENTER_CENTER, db_text(peak), look::regular(g.pt(13.0)), Color32::from_gray(0xd8));
    let clicked = ui.interact(field, id, Sense::click()).on_hover_text("Peak (click to reset)").clicked();
    let (y0, y1) = (466.0, 1166.0);
    for (k, v) in [l, r].into_iter().enumerate() {
        let bx = x - 23.0 + 30.0 * k as f32;
        let bar = g.rect(bx, y0 - 4.0, bx + 15.0, y1 + 4.0);
        pt.rect_filled(bar, 0.0, Color32::from_rgb(0x1c, 0x1c, 0x1b));
        if v > -60.0 {
            let top = y0 + (y1 - y0) * meter_frac(v);
            let fill = g.rect(bx + 1.5, top, bx + 13.5, y1 + 3.0);
            // green, yellow from -6 dB, red over 0 dB
            let segs: [(f32, f32, Color32); 3] = [(-200.0, -6.0, Color32::from_rgb(0x3d, 0xc4, 0x5a)), (-6.0, 0.0, Color32::from_rgb(0xe6, 0xc8, 0x2c)), (0.0, 200.0, Color32::from_rgb(0xe8, 0x3a, 0x30))];
            for (lo, hi, col) in segs {
                let ya = y0 + (y1 - y0) * meter_frac(hi.min(v));
                let yb = y0 + (y1 - y0) * meter_frac(lo);
                if v > lo {
                    let rr = Rect::from_min_max(pos2(fill.min.x, g.at(0.0, ya).y), pos2(fill.max.x, g.at(0.0, yb).y)).intersect(fill);
                    pt.rect_filled(rr, 0.0, col);
                }
            }
        }
    }
    for (k, m) in METER_MARKS.iter().enumerate() {
        let y = y0 + (y1 - y0) * k as f32 / (METER_MARKS.len() - 1) as f32;
        let t = if *m > 0.0 { format!("+{m:.0}") } else { format!("{m:.0}") };
        pt.text(g.at(label_x, y), Align2::RIGHT_CENTER, t, look::regular(g.pt(11.0)), SCALE);
        pt.line_segment([g.at(tick_x, y), g.at(tick_x + 10.0, y)], Stroke::new(g.pt(1.0), SCALE));
    }
    clicked
}

/// The gain-reduction VU (Meter): a needle over Logic's scale.
fn vu(pt: &egui::Painter, g: &Geo, lcd: Rect, gr: f32) {
    // (measured: the needle's pivot sits under the display; the scale's arc is a circle)
    let pivot = g.at(857.0, 1050.0);
    let arc_c = g.at(933.0, 1587.0);
    let arc_r = g.pt(938.0 * 0.514);
    let marks: [(f32, f32); 6] = [(-50.0, -136.8), (-30.0, -121.0), (-20.0, -105.0), (-10.0, -89.3), (-5.0, -65.0), (0.0, -42.0)];
    let ang = |v: f32| -> f32 {
        let v = v.clamp(-50.0, 0.0);
        for w in marks.windows(2) {
            if v <= w[1].0 {
                return (w[0].1 + (w[1].1 - w[0].1) * (v - w[0].0) / (w[1].0 - w[0].0)).to_radians();
            }
        }
        marks[5].1.to_radians()
    };
    // where a ray from the pivot meets the arc
    let hit = |a: f32| -> Pos2 {
        let u = vec2(a.cos(), a.sin());
        let d = pivot - arc_c;
        let b = d.dot(u);
        let cc = d.length_sq() - arc_r * arc_r;
        let t = -b + (b * b - cc).max(0.0).sqrt();
        pivot + u * t
    };
    let p = pt.with_clip_rect(lcd);
    let ink = Color32::from_gray(0x2a);
    let pts: Vec<Pos2> = (0..=60).map(|k| hit(ang(-50.0 + 50.0 * k as f32 / 60.0))).collect();
    p.add(egui::Shape::line(pts, Stroke::new(g.pt(2.0), ink)));
    let mut ticks: Vec<(f32, f32)> = vec![(-50.0, 0.0), (-30.0, 22.0), (-20.0, 22.0), (-10.0, 60.0), (-5.0, 60.0), (0.0, 0.0)];
    ticks.extend((1..10).filter(|k| *k != 5).map(|k| (-(k as f32), 22.0)));
    for (v, len) in ticks {
        let a = ang(v);
        let h = hit(a);
        let u = vec2(a.cos(), a.sin());
        let len = if len == 0.0 { 12.0 } else { len };
        p.line_segment([h, h + u * g.pt(len * 0.514)], Stroke::new(g.pt(1.6), ink));
    }
    for (t, x, y) in [("-50", 455.0, 668.0), ("-30", 595.0, 610.0), ("-20", 725.0, 582.0), ("-10", 862.0, 568.0), ("-5", 1053.0, 590.0), ("0", 1262.0, 673.0)] {
        p.text(g.at(x, y), Align2::CENTER_CENTER, t, look::regular(g.pt(18.5)), ink);
    }
    let a = ang(-gr);
    let tip = hit(a) + vec2(a.cos(), a.sin()) * g.pt(10.0);
    p.line_segment([pivot, tip], Stroke::new(g.pt(1.8), Color32::from_gray(0x10)));
}

/// The Graph: the reduction over the last seconds (newest on the right).
fn graph(pt: &egui::Painter, g: &Geo, lcd: Rect, hist: &[f32]) {
    let inner = lcd.shrink2(vec2(g.pt(14.0), g.pt(14.0)));
    let area = Rect::from_min_max(pos2(inner.min.x + g.pt(26.0), inner.min.y + g.pt(36.0)), inner.max);
    let ink = Color32::from_gray(0x2a);
    let y_of = |db: f32| area.min.y + area.height() * (db.clamp(0.0, 30.0) / 30.0);
    for db in [0.0, 5.0, 10.0, 20.0, 30.0] {
        let y = y_of(db);
        pt.line_segment([pos2(area.min.x, y), pos2(area.max.x, y)], Stroke::new(g.pt(0.8), Color32::from_black_alpha(60)));
        pt.text(pos2(area.min.x - g.pt(5.0), y), Align2::RIGHT_CENTER, if db == 0.0 { "0".to_string() } else { format!("-{db:.0}") }, look::regular(g.pt(10.5)), ink);
    }
    if hist.len() > 1 {
        let n = hist.len();
        let x_of = |k: usize| area.min.x + area.width() * k as f32 / (n - 1) as f32;
        let mut mesh = egui::Mesh::default();
        let fill = Color32::from_rgba_unmultiplied(0x20, 0x40, 0x70, 110);
        for (k, v) in hist.iter().enumerate() {
            mesh.colored_vertex(pos2(x_of(k), area.min.y), fill);
            mesh.colored_vertex(pos2(x_of(k), y_of(*v)), fill);
            if k > 0 {
                let i = (2 * k) as u32;
                mesh.add_triangle(i - 2, i - 1, i + 1);
                mesh.add_triangle(i - 2, i + 1, i);
            }
        }
        pt.add(mesh);
        let line: Vec<Pos2> = hist.iter().enumerate().map(|(k, v)| pos2(x_of(k), y_of(*v))).collect();
        pt.add(egui::Shape::line(line, Stroke::new(g.pt(1.6), Color32::from_rgb(0x1a, 0x2c, 0x48))));
    }
}

pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, slot: (usize, usize), set: &mut Vec<(u32, f32)>) {
    let get = |id: u32, set: &Vec<(u32, f32)>| set.iter().rev().find(|x| x.0 == id).map_or(val(params, values, id), |x| x.1);
    let margin = ui.spacing().window_margin;
    let w = ui.available_width() + (margin.left + margin.right) as f32;
    let s = w / BODY_W;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), BODY_H * s), Sense::hover());
    let body = Rect::from_min_max(pos2(r.min.x - margin.left as f32, r.min.y), pos2(r.max.x + margin.right as f32, r.max.y));
    let g = Geo { o: body.min, s };
    let pt = ui.painter().with_clip_rect(body);
    let id = egui::Id::unique(("compressor-view", slot));
    let circuit = get(c::CIRCUIT, set).round() as usize;
    let (top, bot) = skin(circuit);
    gradient(&pt, body, top, bot, 0.0);
    // the dividers either side of the middle section
    for x in [255.0, 1475.0] {
        pt.line_segment([g.at(x, 395.0), g.at(x, 1445.0)], Stroke::new(g.pt(1.0), Color32::from_black_alpha(70)));
        pt.line_segment([g.at(x + 2.0, 395.0), g.at(x + 2.0, 1445.0)], Stroke::new(g.pt(1.0), Color32::from_white_alpha(14)));
    }
    let put = |set: &mut Vec<(u32, f32)>, id: u32, v: f32| set.push((id, v));

    // the circuit types
    pt.rect_filled(g.rect(293.0, 237.0, 1440.0, 343.0), g.pt(5.0), Color32::from_gray(0x14));
    let names = ["Platinum\nDigital", "Studio\nVCA", "Studio\nFET", "Classic\nVCA", "Vintage\nVCA", "Vintage\nFET", "Vintage\nOpto"];
    for (k, n) in names.iter().enumerate() {
        let x0 = 299.0 + 163.5 * k as f32;
        if push(ui, &g, g.rect(x0, 243.0, x0 + 157.0, 337.0), id.with(("circ", k)), n, k == circuit, 12.8) && k != circuit {
            put(set, c::CIRCUIT, k as f32);
        }
    }
    // Side Chain / Output
    let view = get(c::VIEW, set).round() as usize;
    pt.rect_filled(g.rect(1567.0, 250.0, 1898.0, 325.0), g.pt(5.0), Color32::from_gray(0x14));
    for (k, n) in ["Side Chain", "Output"].iter().enumerate() {
        let x0 = 1573.0 + 163.0 * k as f32;
        if push(ui, &g, g.rect(x0, 256.0, x0 + 157.0, 319.0), id.with(("view", k)), n, k == view, 13.0) && k != view {
            put(set, c::VIEW, k as f32);
        }
    }

    // input and output meters
    let lv = |n: &str| live.get(n).unwrap_or(-200.0);
    let reset = |set: &mut Vec<(u32, f32)>| {
        let n = get(c::PEAK_RESET, set) + 1.0;
        set.push((c::PEAK_RESET, n));
    };
    if level_meter(ui, &g, 130.0, 98.0, 156.0, lv("Input L"), lv("Input R"), lv("Input Peak"), id.with("in-peak")) {
        reset(set);
    }
    if level_meter(ui, &g, 1848.0, 1820.0, 1873.0, lv("Output L"), lv("Output R"), lv("Output Peak"), id.with("out-peak")) {
        reset(set);
    }

    // the gain-reduction display
    pt.rect_filled(g.rect(293.0, 393.0, 1438.0, 838.0), g.pt(3.0), Color32::from_gray(0x28));
    let lcd = g.rect(333.0, 432.0, 1400.0, 797.0);
    gradient(&pt, lcd, Color32::from_rgb(0x94, 0x94, 0x92), Color32::from_rgb(0x64, 0x64, 0x61), 0.0);
    let gr = live.get("Gain Reduction").unwrap_or(0.0);
    let hist_id = id.with("hist");
    let mut hist: Vec<f32> = ui.data(|d| d.get_temp(hist_id)).unwrap_or_default();
    hist.push(gr);
    if hist.len() > 240 {
        hist.remove(0);
    }
    ui.data_mut(|d| d.insert_temp(hist_id, hist.clone()));
    let display = get(c::DISPLAY, set).round() as usize;
    if display == 0 { vu(&pt, &g, lcd, gr) } else { graph(&pt, &g, lcd, &hist) }
    // Meter | Graph
    let seg = g.rect(633.0, 451.0, 1101.0, 503.0);
    pt.rect_filled(seg.expand(g.pt(1.5)), g.pt(2.0), Color32::from_gray(0x30));
    for (k, n) in ["Meter", "Graph"].iter().enumerate() {
        let half = Rect::from_min_max(pos2(seg.min.x + seg.width() * k as f32 / 2.0, seg.min.y), pos2(seg.min.x + seg.width() * (k + 1) as f32 / 2.0, seg.max.y));
        let on = k == display;
        pt.rect_filled(half, 0.0, if on { Color32::from_gray(0x46) } else { Color32::from_gray(0xbf) });
        pt.text(half.center(), Align2::CENTER_CENTER, *n, look::regular(g.pt(18.5)), if on { Color32::from_gray(0xc8) } else { Color32::from_gray(0x2c) });
        if ui.interact(half, id.with(("disp", k)), Sense::click()).clicked() && !on {
            put(set, c::DISPLAY, k as f32);
        }
    }

    // the knobs
    let kn = |ui: &mut egui::Ui, set: &mut Vec<(u32, f32)>, pid: u32, xd: f32, yd: f32, d: f32, dial: &Dial, title: Option<(&str, f32, f32)>| {
        let p = prm(params, pid);
        let v = get(pid, set);
        if let Some(nv) = knob(ui, &g, g.at(xd, yd), d, id.with(("knob", pid)), v, &p, dial, None) {
            set.push((pid, nv));
        }
        if let Some((t, tx, ty)) = title {
            ui.painter().text(g.at(tx, ty), Align2::CENTER_CENTER, t, look::regular(g.pt(14.5)), LABEL);
        }
    };
    kn(ui, set, c::THRESHOLD, 478.0, 1048.0, 82.0, &D_THRESHOLD, Some(("THRESHOLD", 476.0, 887.0)));
    let ratio = get(c::RATIO, set);
    let ratio_t = format!("{}:1", if ratio >= 10.0 { format!("{ratio:.0}") } else { format!("{ratio:.1}") });
    kn(ui, set, c::RATIO, 789.0, 1048.0, 82.0, &D_RATIO, Some((&ratio_t, 790.0, 887.0)));
    kn(ui, set, c::MAKEUP, 1097.0, 1048.0, 82.0, &D_MAKEUP, Some(("MAKE UP", 1105.0, 887.0)));
    kn(ui, set, c::KNEE, 475.0, 1357.0, 60.0, &D_KNEE, Some(("KNEE", 475.0, 1212.0)));
    kn(ui, set, c::ATTACK, 789.0, 1357.0, 60.0, &D_ATTACK, Some(("ATTACK", 789.0, 1212.0)));
    kn(ui, set, c::RELEASE, 1100.0, 1357.0, 60.0, &D_RELEASE, Some(("RELEASE", 1100.0, 1212.0)));
    kn(ui, set, c::IN_GAIN, 130.0, 1357.0, 60.0, &D_GAIN, Some(("INPUT GAIN", 131.0, 1212.0)));
    kn(ui, set, c::OUT_GAIN, 1848.0, 1357.0, 60.0, &D_GAIN, Some(("OUTPUT GAIN", 1848.0, 1212.0)));
    kn(ui, set, c::MIX, 1623.0, 1357.0, 60.0, &D_MIX, Some(("MIX", 1625.0, 1212.0)));

    // Auto Gain and Auto (release)
    ui.painter().text(g.at(1334.0, 890.0), Align2::CENTER_CENTER, "AUTO GAIN", look::regular(g.pt(14.5)), LABEL);
    let ag = get(c::AUTO_GAIN, set).round() as usize;
    for (k, (n, y)) in [("OFF", 933.0), ("0 dB", 1030.0), ("-12 dB", 1127.0)].iter().enumerate() {
        if push(ui, &g, g.rect(1285.0, *y, 1383.0, y + 67.0), id.with(("ag", k)), n, k == ag, 12.5) && k != ag {
            put(set, c::AUTO_GAIN, k as f32);
        }
    }
    let ar = get(c::AUTO_RELEASE, set) >= 0.5;
    if push(ui, &g, g.rect(1285.0, 1330.0, 1383.0, 1397.0), id.with("auto-rel"), "AUTO", ar, 12.5) {
        put(set, c::AUTO_RELEASE, if ar { 0.0 } else { 1.0 });
    }

    if view == 1 {
        // Output: the limiter and distortion
        let on = get(c::LIMIT_ON, set) >= 0.5;
        ui.painter().text(g.at(1623.0, 410.0), Align2::CENTER_CENTER, "LIMITER", look::regular(g.pt(14.5)), LABEL);
        let lit = on && live.get("Limiting").unwrap_or(0.0) > 0.0;
        let led = g.at(1703.0, 410.0);
        ui.painter().circle_filled(led, g.pt(6.5), Color32::from_gray(0x18));
        ui.painter().circle_filled(led, g.pt(5.2), if lit { Color32::from_rgb(0xff, 0x3a, 0x2a) } else { Color32::from_rgb(0x54, 0x17, 0x14) });
        if push(ui, &g, g.rect(1573.0, 455.0, 1672.0, 523.0), id.with("lim-on"), "ON", on, 12.5) {
            put(set, c::LIMIT_ON, if on { 0.0 } else { 1.0 });
        }
        kn(ui, set, c::LIMIT_THR, 1623.0, 685.0, 60.0, &D_LIMIT, Some(("THRESHOLD", 1622.0, 561.0)));
        ui.painter().text(g.at(1624.0, 922.0), Align2::CENTER_CENTER, "DISTORTION", look::regular(g.pt(14.5)), LABEL);
        let p = prm(params, c::DISTORTION);
        let v = get(c::DISTORTION, set);
        if let Some(nv) = knob(ui, &g, g.at(1623.0, 1062.0), 60.0, id.with("dist"), v, &p, &D_KNEE, Some(&[-151.0, -109.0, -68.0, -28.0])) {
            put(set, c::DISTORTION, nv);
        }
    } else {
        // Side Chain: detection, the filter, Listen
        let lp = ui.painter().clone();
        let lab = |t: &str, x: f32, y: f32| lp.text(g.at(x, y), Align2::CENTER_CENTER, t, look::regular(g.pt(14.5)), LABEL);
        lab("DETECTION", 1623.0, 410.0);
        let seg_row = |ui: &mut egui::Ui, set: &mut Vec<(u32, f32)>, pid: u32, labels: &[&str], y: f32, x0: f32, x1: f32, size: f32| {
            let cur = get(pid, set).round() as usize;
            let n = labels.len() as f32;
            let wd = (x1 - x0) / n;
            for (k, l) in labels.iter().enumerate() {
                let xa = x0 + wd * k as f32;
                if push(ui, &g, g.rect(xa + 3.0, y, xa + wd - 3.0, y + 58.0), id.with(("seg", pid, k)), l, k == cur, size) && k != cur {
                    set.push((pid, k as f32));
                }
            }
        };
        seg_row(ui, set, c::SC_DETECT, &["Max", "Sum"], 448.0, 1515.0, 1731.0, 12.5);
        seg_row(ui, set, c::PEAK_RMS, &["Peak", "RMS"], 520.0, 1515.0, 1731.0, 12.5);
        lab("FILTER", 1570.0, 625.0);
        let fon = get(c::SC_FILTER, set) >= 0.5;
        if push(ui, &g, g.rect(1640.0, 596.0, 1731.0, 654.0), id.with("sc-on"), "ON", fon, 12.5) {
            put(set, c::SC_FILTER, if fon { 0.0 } else { 1.0 });
        }
        seg_row(ui, set, c::SC_MODE, &["LP", "BP", "HP", "ParEQ", "HS"], 680.0, 1497.0, 1749.0, 9.5);
        kn(ui, set, c::SC_FREQ, 1560.0, 860.0, 46.0, &D_FREQ, Some(("FREQUENCY", 1560.0, 770.0)));
        kn(ui, set, c::SC_Q, 1690.0, 860.0, 46.0, &D_Q, Some(("Q", 1690.0, 770.0)));
        kn(ui, set, c::SC_GAIN, 1560.0, 1075.0, 46.0, &D_SCGAIN, Some(("GAIN", 1560.0, 985.0)));
        let li = get(c::LISTEN, set) >= 0.5;
        if push(ui, &g, g.rect(1641.0, 1042.0, 1739.0, 1109.0), id.with("listen"), "LISTEN", li, 12.0) {
            put(set, c::LISTEN, if li { 0.0 } else { 1.0 });
        }
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dials_round_trip() {
        for d in [&D_THRESHOLD, &D_RATIO, &D_MAKEUP, &D_ATTACK, &D_RELEASE, &D_MIX] {
            for f in [0.0, 0.13, 0.5, 0.77, 1.0] {
                assert!((d.frac(d.value(f)) - f).abs() < 1e-4);
            }
        }
        // Logic's scales: ratio 2 sits a fifth of the way round, make-up 0 dB at the fifth mark
        assert!((D_RATIO.frac(2.0) - 0.2038).abs() < 0.01);
        assert!((D_MAKEUP.frac(0.0) - 4.0 / 11.0).abs() < 1e-5);
        assert!((D_RELEASE.value(D_RELEASE.frac(1000.0)) - 1000.0).abs() < 0.01);
    }

    #[test]
    fn meter_scale_steps() {
        assert_eq!(meter_frac(3.0), 0.0);
        assert!((meter_frac(-12.0) - 0.5).abs() < 1e-6);
        assert_eq!(meter_frac(-90.0), 1.0);
    }
}
