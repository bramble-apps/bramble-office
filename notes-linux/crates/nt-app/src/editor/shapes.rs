//! Markup objects (macOS Markup's shapes, text boxes and signatures, nt-core's model::Shape) over
//! a drawing or a picture: their outlines, painting on screen and into the flattened picture, hit
//! testing, and the handles that resize, turn and reshape them. Coordinates are drawing points;
//! a shape turns about its frame's centre.

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};

use super::ink::{self, Shape, ShapeKind, Stroke, StrokeExt};

pub fn c32(c: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied((c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8, (c[3] * 255.0).round() as u8)
}

pub fn rgba(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
}

/// The style new objects get and the toolbar's menus change (Markup's Shape Style, Border
/// Color, Fill Color and Text Style).
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub stroke: [f32; 4],
    pub fill: [f32; 4],
    pub line_width: f32,
    pub dash: u8,
    pub shadow: bool,
    pub arrows: u8,
    pub font: String,
    pub font_size: f32,
    pub text_color: [f32; 4],
    pub align: u8,
}

/// Notes' default (NO3): shapes filled #C2C2C2 with no stroke (5 pt once one is chosen), text
/// black Helvetica 24.
impl Default for Style {
    fn default() -> Style {
        let grey = 194.0 / 255.0;
        Style { stroke: [0.0; 4], fill: [grey, grey, grey, 1.0], line_width: 5.0, dash: 0, shadow: false, arrows: 2, font: "Helvetica".into(), font_size: 24.0, text_color: [0.0, 0.0, 0.0, 1.0], align: 0 }
    }
}

/// The selection's blue (handles and frame, NO3: #3B86F7).
pub const SELECT_BLUE: Color32 = Color32::from_rgb(59, 134, 247);

/// A new object of `kind` centred on `c`, in `st`'s style.
pub fn new_shape(kind: ShapeKind, c: Pos2, st: &Style) -> Shape {
    // (a new rectangle measures 152 pt square in NO3)
    let (w, h) = match kind {
        ShapeKind::Bubble => (152.0, 112.0),
        ShapeKind::Line | ShapeKind::Arrow => (152.0, 0.0),
        ShapeKind::Text => (120.0, st.font_size * 1.3 + 8.0),
        _ => (152.0, 152.0),
    };
    let mut s = Shape {
        id: super::doc::new_uuid(),
        kind,
        frame: [c.x - w / 2.0, c.y - h / 2.0, w, h],
        stroke: st.stroke,
        fill: if kind == ShapeKind::Text { [0.0; 4] } else { st.fill },
        line_width: st.line_width,
        dash: st.dash,
        shadow: st.shadow,
        font: st.font.clone(),
        font_size: st.font_size,
        text_color: st.text_color,
        align: st.align,
        ..Default::default()
    };
    match kind {
        ShapeKind::Line | ShapeKind::Arrow => {
            // a line has nothing to fill: it gets a stroke
            if s.stroke[3] < 0.01 {
                s.stroke = [0.0, 0.0, 0.0, 1.0];
            }
            s.fill = [0.0; 4];
            s.points = vec![[c.x - w / 2.0, c.y], [c.x + w / 2.0, c.y]];
            s.arrows = if kind == ShapeKind::Arrow { st.arrows.max(2) } else { 0 };
            fit_line(&mut s);
        }
        ShapeKind::Bubble => {
            s.frame[3] = h - 30.0;
            s.points = vec![[c.x - w * 0.2, s.frame[1] + s.frame[3] + 30.0]];
        }
        ShapeKind::Star => s.sides = 5,
        ShapeKind::Polygon => s.sides = 6,
        ShapeKind::Loupe => {
            s.magnify = 2.0;
            s.stroke = [0.15, 0.15, 0.15, 1.0];
            s.line_width = 4.0;
            s.fill = [0.0; 4];
        }
        ShapeKind::Text => {
            s.text = "Text".into();
            s.stroke = [0.0; 4];
        }
        _ => {}
    }
    s
}

/// A signature (strokes in drawing points) as an object `width` wide centred on `c`.
pub fn new_signature(strokes: &[Stroke], c: Pos2, width: f32, color: [f32; 4]) -> Option<Shape> {
    let b = strokes.iter().fold(Rect::NOTHING, |r, s| r.union(s.bounds()));
    if !b.is_finite() || b.width() < 1.0 {
        return None;
    }
    let h = width * b.height().max(1.0) / b.width();
    let ink = strokes.iter().map(|s| {
        let mut s = s.clone();
        s.points = s.placed().iter().map(|p| ink::InkPoint { x: (p.x - b.min.x) / b.width(), y: (p.y - b.min.y) / b.height().max(1.0), size: p.size / b.width(), ..*p }).collect();
        s.transform = ink::IDENTITY;
        s.color = color;
        s
    }).collect();
    Some(Shape { id: super::doc::new_uuid(), kind: ShapeKind::Signature, frame: [c.x - width / 2.0, c.y - h / 2.0, width, h], ink, stroke: color, ..Default::default() })
}

/// A line's frame is its ends' box.
pub fn fit_line(s: &mut Shape) {
    if let [a, b] = s.points[..] {
        s.frame = [a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()];
    }
}

pub fn is_line(s: &Shape) -> bool {
    matches!(s.kind, ShapeKind::Line | ShapeKind::Arrow) && s.points.len() == 2
}

pub fn frame(s: &Shape) -> Rect {
    Rect::from_min_size(pos2(s.frame[0], s.frame[1]), vec2(s.frame[2], s.frame[3]))
}

pub fn centre(s: &Shape) -> Pos2 {
    frame(s).center()
}

fn turn(p: Pos2, c: Pos2, a: f32) -> Pos2 {
    if a == 0.0 {
        return p;
    }
    let (sn, cs) = a.sin_cos();
    let d = p - c;
    c + vec2(d.x * cs - d.y * sn, d.x * sn + d.y * cs)
}

/// A point of the unturned frame, turned as the shape is.
pub fn place(s: &Shape, p: Pos2) -> Pos2 {
    turn(p, centre(s), s.rotation)
}

/// A drawing point in the shape's unturned frame.
pub fn local(s: &Shape, p: Pos2) -> Pos2 {
    turn(p, centre(s), -s.rotation)
}

/// The outline of a closed shape (unturned), clockwise.
fn outline_local(s: &Shape) -> Vec<Pos2> {
    let r = frame(s);
    let (c, rx, ry) = (r.center(), r.width() / 2.0, r.height() / 2.0);
    let ellipse = |n: usize, a0: f32, k: &dyn Fn(usize) -> f32| -> Vec<Pos2> {
        (0..n).map(|i| {
            let a = a0 + i as f32 / n as f32 * std::f32::consts::TAU;
            c + vec2(a.cos() * rx * k(i), a.sin() * ry * k(i))
        }).collect()
    };
    match s.kind {
        ShapeKind::Rect | ShapeKind::Text | ShapeKind::Signature => vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()],
        ShapeKind::RoundRect => round_rect(r, r.width().min(r.height()) * 0.18, None),
        ShapeKind::Bubble => {
            let tip = s.points.first().map_or(pos2(r.min.x + r.width() * 0.3, r.max.y + 34.0), |p| pos2(p[0], p[1]));
            round_rect(r, r.width().min(r.height()) * 0.3, Some(tip))
        }
        ShapeKind::Oval | ShapeKind::Loupe => ellipse(72, 0.0, &|_| 1.0),
        ShapeKind::Polygon => ellipse(s.sides.clamp(3, 24) as usize, -std::f32::consts::FRAC_PI_2, &|_| 1.0),
        ShapeKind::Star => {
            let n = s.sides.clamp(3, 24) as usize;
            let inner = star_inner(n);
            ellipse(n * 2, -std::f32::consts::FRAC_PI_2, &|i| if i % 2 == 0 { 1.0 } else { inner })
        }
        ShapeKind::Line | ShapeKind::Arrow => s.points.iter().map(|p| pos2(p[0], p[1])).collect(),
    }
}

/// A regular star's inner radius (of the outer): the five-pointed one's 0.382.
fn star_inner(n: usize) -> f32 {
    if n == 5 { 0.382 } else { 0.5 }
}

/// A rounded rectangle, clockwise from the top-left; with a speech bubble's tail to `tail`
/// (its base on the edge nearest the tip).
fn round_rect(r: Rect, rad: f32, tail: Option<Pos2>) -> Vec<Pos2> {
    let rad = rad.min(r.width() / 2.0).min(r.height() / 2.0);
    let mut out = Vec::new();
    let corner = |out: &mut Vec<Pos2>, c: Pos2, a0: f32| {
        for k in 0..=8 {
            let a = a0 + k as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
            out.push(c + vec2(a.cos(), a.sin()) * rad);
        }
    };
    use std::f32::consts::PI;
    corner(&mut out, pos2(r.min.x + rad, r.min.y + rad), PI);
    corner(&mut out, pos2(r.max.x - rad, r.min.y + rad), -PI / 2.0);
    corner(&mut out, pos2(r.max.x - rad, r.max.y - rad), 0.0);
    // the bottom edge, right to left: the tail goes out from it
    if let Some(t) = tail.filter(|t| t.y > r.max.y - 1.0) {
        let half = (r.width() * 0.09).max(7.0).min((r.width() - 2.0 * rad) / 2.0 - 1.0).max(1.0);
        let x = t.x.clamp(r.min.x + rad + half, r.max.x - rad - half);
        out.push(pos2(x + half, r.max.y));
        out.push(t);
        out.push(pos2(x - half, r.max.y));
    }
    corner(&mut out, pos2(r.min.x + rad, r.max.y - rad), PI / 2.0);
    out
}

/// The outline as drawn (turned), drawing points.
pub fn outline(s: &Shape) -> Vec<Pos2> {
    if is_line(s) {
        return outline_local(s);
    }
    outline_local(s).into_iter().map(|p| place(s, p)).collect()
}

/// The box round the shape as drawn.
pub fn bounds(s: &Shape) -> Rect {
    let mut r = Rect::from_points(&outline(s));
    if !is_line(s) {
        for p in [frame(s).left_top(), frame(s).right_top(), frame(s).right_bottom(), frame(s).left_bottom()] {
            r.extend_with(place(s, p));
        }
    }
    r.expand(s.line_width / 2.0)
}

/// Whether `p` touches the shape: on its border or line (within `slack`), or inside a filled or
/// text or signature shape.
pub fn hits(s: &Shape, p: Pos2, slack: f32) -> bool {
    if !bounds(s).expand(slack).contains(p) {
        return false;
    }
    let pts = outline(s);
    let near = |pts: &[Pos2], closed: bool| {
        let n = pts.len();
        (0..if closed { n } else { n.saturating_sub(1) }).any(|i| ink::dist_to_segment(p, pts[i], pts[(i + 1) % n]) <= s.line_width / 2.0 + slack)
    };
    if is_line(s) {
        return near(&pts, false);
    }
    let filled = s.fill[3] > 0.01 || matches!(s.kind, ShapeKind::Text | ShapeKind::Signature | ShapeKind::Loupe);
    near(&pts, true) || filled && ink::inside(&pts, p)
}

// ---- handles

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    Move,
    /// 0 top-left, 1 top-right, 2 bottom-right, 3 bottom-left
    Corner(u8),
    /// 0 top, 1 right, 2 bottom, 3 left
    Edge(u8),
    /// turning (Notes' Markup shows no handle for it; kept for the model's rotation)
    #[allow(dead_code)]
    Rotate,
    /// a line's end
    End(u8),
    /// a speech bubble's tail
    Tail,
    /// the green knob: a star's points, a polygon's sides, a loupe's magnification
    Knob,
}

/// The handles of a selected object (drawing points; `scale` is screen points a drawing point).
pub fn handles(s: &Shape, scale: f32) -> Vec<(Handle, Pos2)> {
    if is_line(s) {
        return vec![(Handle::End(0), pos2(s.points[0][0], s.points[0][1])), (Handle::End(1), pos2(s.points[1][0], s.points[1][1]))];
    }
    let r = frame(s);
    let mut v = Vec::new();
    // a text box is sized by its sides only (its height follows the text)
    if s.kind != ShapeKind::Text {
        v.extend([r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].into_iter().enumerate().map(|(i, p)| (Handle::Corner(i as u8), p)));
        v.extend([r.center_top(), r.right_center(), r.center_bottom(), r.left_center()].into_iter().enumerate().map(|(i, p)| (Handle::Edge(i as u8), p)));
    } else {
        v.extend([(Handle::Edge(1), r.right_center()), (Handle::Edge(3), r.left_center())]);
    }
    // (no turn handle in Notes' Markup)
    let _ = scale;
    match s.kind {
        ShapeKind::Bubble => {
            if let Some(t) = s.points.first() {
                v.push((Handle::Tail, pos2(t[0], t[1])));
            }
        }
        ShapeKind::Star | ShapeKind::Polygon => {
            let pts = outline_local(s);
            v.push((Handle::Knob, if s.kind == ShapeKind::Star { pts[1] } else { r.center() + (pts[1] - r.center()) * 0.6 }));
        }
        ShapeKind::Loupe => v.push((Handle::Knob, r.center() + vec2(r.width() / 2.0 * 0.707, -r.height() / 2.0 * 0.707))),
        _ => {}
    }
    v.into_iter().map(|(h, p)| (h, place(s, p))).collect()
}

/// The handle under `p` (drawing points; `reach` in drawing points), else Move when it touches.
pub fn handle_at(s: &Shape, p: Pos2, reach: f32, scale: f32) -> Option<Handle> {
    let best = handles(s, scale).into_iter().map(|(h, q)| (h, (q - p).length())).filter(|(_, d)| *d <= reach).min_by(|a, b| a.1.total_cmp(&b.1));
    match best {
        Some((h, _)) => Some(h),
        None if hits(s, p, reach * 0.6) || !is_line(s) && ink::inside(&outline(s), p) => Some(Handle::Move),
        None => None,
    }
}

/// Drags handle `h` of `orig` from `from` to `to` (drawing points) into `s`; Shift keeps a
/// corner drag's proportions, a line at 45° steps, a turn at 15° steps.
pub fn drag(s: &mut Shape, orig: &Shape, h: Handle, from: Pos2, to: Pos2, shift: bool) {
    *s = orig.clone();
    let d = to - from;
    match h {
        Handle::Move => {
            s.frame[0] += d.x;
            s.frame[1] += d.y;
            for p in &mut s.points {
                p[0] += d.x;
                p[1] += d.y;
            }
        }
        Handle::End(i) => {
            let other = s.points[1 - i as usize];
            let mut q = to;
            if shift {
                let v = q - pos2(other[0], other[1]);
                let a = (v.y.atan2(v.x) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
                q = pos2(other[0], other[1]) + vec2(a.cos(), a.sin()) * v.length();
            }
            s.points[i as usize] = [q.x, q.y];
            fit_line(s);
        }
        Handle::Rotate => {
            let c = centre(orig);
            let v = to - c;
            let mut a = v.y.atan2(v.x) + std::f32::consts::FRAC_PI_2;
            if shift {
                let step = 15f32.to_radians();
                a = (a / step).round() * step;
            }
            s.rotation = a;
        }
        Handle::Tail => {
            let q = local(orig, to);
            s.points = vec![[q.x, q.y]];
        }
        Handle::Knob => match s.kind {
            ShapeKind::Loupe => s.magnify = (orig.magnify + d.x / 40.0 - d.y / 40.0).clamp(1.2, 8.0),
            _ => s.sides = ((orig.sides.max(3) as f32) + (d.x - d.y) / 14.0).round().clamp(3.0, 24.0) as u32,
        },
        Handle::Corner(_) | Handle::Edge(_) => {
            let r = frame(orig);
            let q = local(orig, to);
            let (mut x0, mut y0, mut x1, mut y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
            match h {
                Handle::Corner(0) => (x0, y0) = (q.x, q.y),
                Handle::Corner(1) => (x1, y0) = (q.x, q.y),
                Handle::Corner(2) => (x1, y1) = (q.x, q.y),
                Handle::Corner(_) => (x0, y1) = (q.x, q.y),
                Handle::Edge(0) => y0 = q.y,
                Handle::Edge(1) => x1 = q.x,
                Handle::Edge(2) => y1 = q.y,
                _ => x0 = q.x,
            }
            if shift && matches!(h, Handle::Corner(_)) && r.height() > 0.0 {
                // as wide as the drag says, as tall as the proportions say, from the fixed corner
                let k = r.width() / r.height();
                let w = (x1 - x0).abs().max((y1 - y0).abs() * k);
                let (fx, fy) = match h {
                    Handle::Corner(0) => (r.max.x, r.max.y),
                    Handle::Corner(1) => (r.min.x, r.max.y),
                    Handle::Corner(2) => (r.min.x, r.min.y),
                    _ => (r.max.x, r.min.y),
                };
                let sx = if (x0 + x1) / 2.0 < fx || x1 < fx { -1.0 } else { 1.0 };
                let sy = if (y0 + y1) / 2.0 < fy || y1 < fy { -1.0 } else { 1.0 };
                (x0, x1) = (fx, fx + sx * w);
                (y0, y1) = (fy, fy + sy * w / k);
            }
            let nr = Rect::from_two_pos(pos2(x0, y0), pos2(x1, y1));
            let nr = Rect::from_center_size(nr.center(), nr.size().max(vec2(4.0, 4.0)));
            // the fixed side stays put on the page: the new centre turned about the old one
            let wc = turn(nr.center(), r.center(), orig.rotation);
            s.frame = [wc.x - nr.width() / 2.0, wc.y - nr.height() / 2.0, nr.width(), nr.height()];
            // a bubble's tail keeps its place against the box
            if let Some(t) = orig.points.first().filter(|_| s.kind == ShapeKind::Bubble) {
                let fx = (t[0] - r.min.x) / r.width().max(1.0);
                let nb = frame(s);
                s.points = vec![[nb.min.x + fx * nb.width(), nb.max.y + (t[1] - r.max.y)]];
            }
        }
    }
}

// ---- painting on screen

/// Paints a shape; `under` is the picture being marked up (a loupe magnifies it), `galley` makes
/// a text box's text.
pub fn paint(painter: &egui::Painter, s: &Shape, to_screen: &dyn Fn(Pos2) -> Pos2, scale: f32, under: Option<(egui::TextureId, Vec2)>) {
    let ctx = painter.ctx();
    let lw = (s.line_width * scale).max(0.5);
    let border = c32(s.stroke);
    let fill = c32(s.fill);
    let pts: Vec<Pos2> = outline(s).into_iter().map(to_screen).collect();
    if s.shadow {
        let off = vec2(2.0, 3.0) * scale;
        let shade = Color32::from_black_alpha(70);
        let moved: Vec<Pos2> = pts.iter().map(|p| *p + off).collect();
        if is_line(s) {
            line_with_arrows(painter, s, &moved, lw, shade);
        } else {
            if s.fill[3] > 0.01 {
                fan(painter, &moved, shade);
            }
            border_line(painter, &moved, lw, s.dash, shade);
        }
    }
    match s.kind {
        ShapeKind::Line | ShapeKind::Arrow if is_line(s) => line_with_arrows(painter, s, &pts, lw, border),
        ShapeKind::Signature => {
            let r = frame(s);
            for st in &s.ink {
                let mut st = st.clone();
                st.points = st.points.iter().map(|p| {
                    let q = place(s, pos2(r.min.x + p.x * r.width(), r.min.y + p.y * r.height()));
                    ink::InkPoint { x: q.x, y: q.y, size: p.size * r.width(), ..*p }
                }).collect();
                ink::paint_stroke(painter, &st, to_screen, scale);
            }
        }
        ShapeKind::Loupe => {
            if let Some((tex, size)) = under {
                // the picture under it, magnified, in a disc of triangles
                let c = centre(s);
                let k = s.magnify.max(1.0);
                let mut m = egui::Mesh::with_texture(tex);
                let uv = |p: Pos2| {
                    let q = c + (p - c) / k;
                    pos2(q.x / size.x.max(1.0), q.y / size.y.max(1.0))
                };
                m.vertices.push(egui::epaint::Vertex { pos: to_screen(c), uv: uv(c), color: Color32::WHITE });
                for p in outline(s) {
                    m.vertices.push(egui::epaint::Vertex { pos: to_screen(p), uv: uv(p), color: Color32::WHITE });
                }
                let n = m.vertices.len() as u32 - 1;
                for i in 0..n {
                    m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
                }
                painter.add(egui::Shape::mesh(m));
            } else {
                fan(painter, &pts, Color32::from_white_alpha(40));
            }
            border_line(painter, &pts, lw, 0, border);
        }
        _ => {
            if s.fill[3] > 0.01 {
                fan(painter, &pts, fill);
            }
            if s.stroke[3] > 0.01 {
                border_line(painter, &pts, lw, s.dash, border);
            }
        }
    }
    if s.kind != ShapeKind::Text && !s.text.is_empty() && !is_line(s) {
        // a shape's own text, in its middle
        let galley = text_galley(ctx, s, scale);
        let r = frame(s);
        let x = match s.align {
            1 => (r.width() - 2.0 * TEXT_PAD) / 2.0,
            2 => r.width() - 2.0 * TEXT_PAD,
            _ => 0.0,
        };
        let at = to_screen(place(s, pos2(r.min.x + TEXT_PAD + x, r.center().y - galley.size().y / scale / 2.0)));
        painter.add(egui::epaint::TextShape::new(at, galley, c32(s.text_color)).with_angle(s.rotation));
    }
    if s.kind == ShapeKind::Text {
        let galley = text_galley(ctx, s, scale);
        let r = frame(s);
        let pad = TEXT_PAD;
        let x = match s.align {
            1 => (r.width() - 2.0 * pad) / 2.0,
            2 => r.width() - 2.0 * pad,
            _ => 0.0,
        };
        let at = to_screen(place(s, pos2(r.min.x + pad + x, r.min.y + pad)));
        painter.add(egui::epaint::TextShape::new(at, galley, c32(s.text_color)).with_angle(s.rotation));
    }
}

pub const TEXT_PAD: f32 = 4.0;

/// The font Markup's text uses (an installed family, else the interface font).
pub fn text_font(s: &Shape, size: f32) -> egui::FontId {
    egui::FontId::new(size, super::layout::markup_family(&s.font))
}

/// A text box's text, laid out at `scale` (wrapped to its box).
pub fn text_galley(ctx: &egui::Context, s: &Shape, scale: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(if s.text.is_empty() { " ".into() } else { s.text.clone() }, text_font(s, s.font_size * scale), c32(s.text_color), ((s.frame[2] - 2.0 * TEXT_PAD) * scale).max(4.0));
    job.halign = match s.align {
        1 => egui::Align::Center,
        2 => egui::Align::RIGHT,
        _ => egui::Align::LEFT,
    };
    if s.align == 3 {
        job.justify = true;
    }
    ctx.fonts_mut(|f| f.layout_job(job))
}

/// A text box's widest line, unwrapped (drawing points).
pub fn text_natural_width(ctx: &egui::Context, s: &Shape) -> f32 {
    let job = egui::text::LayoutJob::simple(s.text.clone(), text_font(s, s.font_size), Color32::BLACK, f32::INFINITY);
    ctx.fonts_mut(|f| f.layout_job(job)).size().x
}

/// A text box's height for its text (drawing points).
pub fn text_height(ctx: &egui::Context, s: &Shape) -> f32 {
    text_galley(ctx, s, 1.0).size().y + 2.0 * TEXT_PAD
}

/// A fill from the middle out (the shapes are all star-shaped about their centre).
fn fan(painter: &egui::Painter, pts: &[Pos2], col: Color32) {
    if pts.len() < 3 {
        return;
    }
    let c = pts.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / pts.len() as f32;
    let mut m = egui::Mesh::default();
    m.colored_vertex(c.to_pos2(), col);
    for p in pts {
        m.colored_vertex(*p, col);
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(egui::Shape::mesh(m));
}

fn border_line(painter: &egui::Painter, pts: &[Pos2], lw: f32, dash: u8, col: Color32) {
    let line = egui::Stroke::new(lw, col);
    match dash {
        1 => {
            let mut closed = pts.to_vec();
            closed.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(&closed, line, lw * 3.0, lw * 2.0));
        }
        2 => {
            let mut closed = pts.to_vec();
            closed.push(pts[0]);
            painter.extend(egui::Shape::dotted_line(&closed, col, lw * 2.0, lw / 2.0));
        }
        _ => {
            painter.add(egui::Shape::closed_line(pts.to_vec(), line));
        }
    }
}

/// An arrowhead's length for a line width (screen points).
fn head_len(lw: f32) -> f32 {
    (lw * 4.0).max(10.0)
}

fn line_with_arrows(painter: &egui::Painter, s: &Shape, pts: &[Pos2], lw: f32, col: Color32) {
    let (mut a, mut b) = (pts[0], pts[1]);
    let dir = (b - a).normalized();
    let hl = head_len(lw);
    let heads = [(s.arrows & 1 != 0, pts[0], -dir), (s.arrows & 2 != 0, pts[1], dir)];
    for (on, tip, d) in heads {
        if on && (pts[1] - pts[0]).length() > 1.0 {
            let n = vec2(-d.y, d.x);
            let base = tip - d * hl;
            painter.add(egui::Shape::convex_polygon(vec![tip, base + n * hl * 0.45, base - n * hl * 0.45], col, egui::Stroke::NONE));
        }
    }
    if s.arrows & 1 != 0 {
        a += dir * hl * 0.8;
    }
    if s.arrows & 2 != 0 {
        b -= dir * hl * 0.8;
    }
    let line = egui::Stroke::new(lw, col);
    match s.dash {
        1 => painter.extend(egui::Shape::dashed_line(&[a, b], line, lw * 3.0, lw * 2.0)),
        2 => painter.extend(egui::Shape::dotted_line(&[a, b], col, lw * 2.0, lw / 2.0)),
        _ => {
            painter.line_segment([a, b], line);
        }
    }
}

/// The selection's look (NO3): a 1 pt blue frame and round blue handles (8 pt, a white rim).
pub fn paint_selection(painter: &egui::Painter, s: &Shape, to_screen: &dyn Fn(Pos2) -> Pos2, scale: f32) {
    if !is_line(s) {
        let r = frame(s);
        let box_: Vec<Pos2> = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].into_iter().map(|p| to_screen(place(s, p))).collect();
        painter.add(egui::Shape::closed_line(box_, egui::Stroke::new(1.0, SELECT_BLUE)));
    }
    for (h, p) in handles(s, scale) {
        let q = to_screen(p);
        let col = if matches!(h, Handle::Knob | Handle::Tail) { Color32::from_rgb(52, 199, 89) } else { SELECT_BLUE };
        painter.circle(q, 4.0, col, egui::Stroke::new(1.0, Color32::WHITE));
    }
}

/// An object's opacity (its most opaque colour).
pub fn opacity(s: &Shape) -> f32 {
    [s.fill[3], s.stroke[3], if s.kind == ShapeKind::Text { s.text_color[3] } else { 0.0 }].into_iter().fold(0.0f32, f32::max).max(s.ink.first().map_or(0.0, |k| k.color[3]))
}

/// Sets an object's opacity: each of its colours that shows takes it.
pub fn set_opacity(s: &mut Shape, v: f32) {
    for c in [&mut s.fill, &mut s.stroke, &mut s.text_color] {
        if c[3] > 0.0 {
            c[3] = v;
        }
    }
    for k in &mut s.ink {
        k.color[3] = v;
    }
}

// ---- the flattened picture (tiny-skia)

fn sk_paint(c: [f32; 4]) -> tiny_skia::Paint<'static> {
    let mut p = tiny_skia::Paint::default();
    let c = c32(c);
    p.set_color_rgba8(c.r(), c.g(), c.b(), c.a());
    p.anti_alias = true;
    p
}

fn sk_poly(pts: &[Pos2], k: f32, closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(pts.first()?.x * k, pts[0].y * k);
    for p in &pts[1..] {
        pb.line_to(p.x * k, p.y * k);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

fn sk_stroke(lw: f32, dash: u8) -> tiny_skia::Stroke {
    let mut st = tiny_skia::Stroke { width: lw, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
    match dash {
        1 => st.dash = tiny_skia::StrokeDash::new(vec![lw * 3.0, lw * 2.0], 0.0),
        2 => {
            st.line_cap = tiny_skia::LineCap::Round;
            st.dash = tiny_skia::StrokeDash::new(vec![0.01, lw * 2.0], 0.0);
        }
        _ => {}
    }
    st
}

/// Draws a shape into the picture (`k` pixels a drawing point).
pub fn render(pm: &mut tiny_skia::Pixmap, s: &Shape, k: f32) {
    let id = tiny_skia::Transform::identity();
    let pts = outline(s);
    let lw = s.line_width * k;
    let draw = |pm: &mut tiny_skia::Pixmap, pts: &[Pos2], off: Vec2, border: [f32; 4], fill: [f32; 4]| {
        let pts: Vec<Pos2> = pts.iter().map(|p| *p + off).collect();
        if is_line(s) {
            let (mut a, mut b) = (pts[0], pts[1]);
            let dir = (b - a).normalized();
            let hl = head_len(lw) / k;
            for (on, tip, d) in [(s.arrows & 1 != 0, a, -dir), (s.arrows & 2 != 0, b, dir)] {
                if on {
                    let n = vec2(-d.y, d.x);
                    let base = tip - d * hl;
                    if let Some(p) = sk_poly(&[tip, base + n * hl * 0.45, base - n * hl * 0.45], k, true) {
                        pm.fill_path(&p, &sk_paint(border), tiny_skia::FillRule::Winding, id, None);
                    }
                }
            }
            if s.arrows & 1 != 0 {
                a += dir * hl * 0.8;
            }
            if s.arrows & 2 != 0 {
                b -= dir * hl * 0.8;
            }
            if let Some(p) = sk_poly(&[a, b], k, false) {
                pm.stroke_path(&p, &sk_paint(border), &sk_stroke(lw, s.dash), id, None);
            }
            return;
        }
        if let Some(p) = sk_poly(&pts, k, true) {
            if fill[3] > 0.01 {
                pm.fill_path(&p, &sk_paint(fill), tiny_skia::FillRule::Winding, id, None);
            }
            if border[3] > 0.01 {
                pm.stroke_path(&p, &sk_paint(border), &sk_stroke(lw, s.dash), id, None);
            }
        }
    };
    if s.shadow {
        let shade = [0.0, 0.0, 0.0, 0.27];
        draw(pm, &pts, vec2(2.0, 3.0), if s.stroke[3] > 0.01 { shade } else { [0.0; 4] }, if s.fill[3] > 0.01 { shade } else { [0.0; 4] });
    }
    match s.kind {
        ShapeKind::Signature => {
            let r = frame(s);
            let strokes: Vec<Stroke> = s.ink.iter().map(|st| {
                let mut st = st.clone();
                st.points = st.points.iter().map(|p| {
                    let q = place(s, pos2(r.min.x + p.x * r.width(), r.min.y + p.y * r.height()));
                    ink::InkPoint { x: q.x, y: q.y, size: p.size * r.width(), ..*p }
                }).collect();
                st
            }).collect();
            ink::render_strokes(pm, &strokes, k);
        }
        ShapeKind::Loupe => {
            // what is under it so far, magnified, inside the circle
            let src = pm.clone();
            let c = centre(s);
            let m = s.magnify.max(1.0);
            let b = bounds(s);
            let (w, h) = (pm.width() as i32, pm.height() as i32);
            for y in ((b.min.y * k) as i32).max(0)..((b.max.y * k) as i32 + 1).min(h) {
                for x in ((b.min.x * k) as i32).max(0)..((b.max.x * k) as i32 + 1).min(w) {
                    let p = pos2((x as f32 + 0.5) / k, (y as f32 + 0.5) / k);
                    if !ink::inside(&pts, p) {
                        continue;
                    }
                    let q = c + (p - c) / m;
                    let (sx, sy) = ((q.x * k) as i32, (q.y * k) as i32);
                    if sx >= 0 && sy >= 0 && sx < w && sy < h {
                        if let Some(px) = src.pixel(sx as u32, sy as u32) {
                            pm.pixels_mut()[(y * w + x) as usize] = px;
                        }
                    }
                }
            }
            draw(pm, &pts, Vec2::ZERO, s.stroke, [0.0; 4]);
        }
        _ => draw(pm, &pts, Vec2::ZERO, s.stroke, s.fill),
    }
    if s.kind == ShapeKind::Text || !s.text.is_empty() && !is_line(s) {
        render_text(pm, s, k);
    }
}

thread_local! {
    /// The editor's egui context, for laying out and rasterising Markup text in the picture.
    pub static CTX: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

/// A text box's text into the picture: laid out by egui, its glyphs taken from egui's font
/// atlas (so the picture shows the text as the screen did), turned with the box.
fn render_text(pm: &mut tiny_skia::Pixmap, s: &Shape, k: f32) {
    let Some(ctx) = CTX.with_borrow(|c| c.clone()) else { return };
    let ppp = ctx.pixels_per_point();
    // laid out so a glyph is rasterised at `k` pixels a point
    let galley = text_galley(&ctx, s, k / ppp);
    let atlas = ctx.fonts(|f| f.image());
    let size = ctx.fonts(|f| f.font_image_size());
    let mut tess = egui::epaint::Tessellator::new(ppp, Default::default(), size, Vec::new());
    let mut mesh = egui::Mesh::default();
    // (centred and right-aligned rows start left of the anchor: shifted so all of it is in)
    let shift = vec2(-galley.rect.min.x, -galley.rect.min.y);
    tess.tessellate_text(&egui::epaint::TextShape::new(Pos2::ZERO + shift, galley.clone(), c32(s.text_color)), &mut mesh);
    let (tw, th) = ((galley.size().x * ppp).ceil() as u32 + 2, (galley.size().y * ppp).ceil() as u32 + 2);
    let Some(mut tp) = tiny_skia::Pixmap::new(tw.max(1), th.max(1)) else { return };
    // glyph quads: four vertices each, upright
    for q in mesh.vertices.chunks(4) {
        if q.len() < 4 {
            break;
        }
        let (p0, p1) = (q[0].pos, q[3].pos);
        let (u0, u1) = (q[0].uv, q[3].uv);
        let col = q[0].color;
        let (x0, y0, x1, y1) = ((p0.x * ppp).floor() as i32, (p0.y * ppp).floor() as i32, (p1.x * ppp).ceil() as i32, (p1.y * ppp).ceil() as i32);
        for y in y0.max(0)..y1.min(th as i32) {
            for x in x0.max(0)..x1.min(tw as i32) {
                let fx = ((x as f32 + 0.5) / ppp - p0.x) / (p1.x - p0.x).max(1e-3);
                let fy = ((y as f32 + 0.5) / ppp - p0.y) / (p1.y - p0.y).max(1e-3);
                let ax = ((u0.x + (u1.x - u0.x) * fx) * atlas.size[0] as f32) as usize;
                let ay = ((u0.y + (u1.y - u0.y) * fy) * atlas.size[1] as f32) as usize;
                let Some(a) = atlas.pixels.get(ay * atlas.size[0] + ax.min(atlas.size[0] - 1)) else { continue };
                let cov = a.a() as f32 / 255.0;
                if cov <= 0.0 {
                    continue;
                }
                let alpha = cov * col.a() as f32 / 255.0;
                let i = (y as u32 * tw + x as u32) as usize;
                let old = tp.pixels()[i];
                let mix = |o: u8, n: u8| ((n as f32 * alpha) + o as f32 * (1.0 - alpha)).round().min(255.0) as u8;
                let na = (alpha * 255.0 + old.alpha() as f32 * (1.0 - alpha)).round().min(255.0) as u8;
                if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba(mix(old.red(), col.r()).min(na), mix(old.green(), col.g()).min(na), mix(old.blue(), col.b()).min(na), na) {
                    tp.pixels_mut()[i] = px;
                }
            }
        }
    }
    // where the screen puts it: the anchor (as `paint`), then the rows' own offset
    let r = frame(s);
    let x = match s.align {
        1 => (r.width() - 2.0 * TEXT_PAD) / 2.0,
        2 => r.width() - 2.0 * TEXT_PAD,
        _ => 0.0,
    };
    // (a text box's text from its top, a shape's in its middle)
    let y = if s.kind == ShapeKind::Text { r.min.y + TEXT_PAD } else { r.center().y - galley.size().y * ppp / k / 2.0 };
    let at = place(s, pos2(r.min.x + TEXT_PAD + x, y));
    let t = tiny_skia::Transform::from_translate(at.x * k, at.y * k).pre_concat(tiny_skia::Transform::from_rotate(s.rotation.to_degrees())).pre_translate(galley.rect.min.x * ppp, galley.rect.min.y * ppp);
    pm.draw_pixmap(0, 0, tp.as_ref(), &tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() }, t, None);
}
