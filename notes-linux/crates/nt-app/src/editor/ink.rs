//! Ink: the strokes of a drawing (PencilKit's model: points with time, width, force), smoothing
//! of what the pointer gives, the outline of a stroke (a variable-width ribbon around a smoothed
//! centre line, round ends), drawn by egui on screen and by tiny-skia for the PNG Notes keeps as a
//! drawing's fallback picture and for Markup flattened into a picture. Ideas from ChordPro
//! Presenter's ink (lib/ink.dart, widgets/ink_layer.dart): midpoint smoothing, hit testing,
//! moving strokes.

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};

// The model is nt-core's (it reads and writes Apple's drawings and ours); the editor's
// geometry on it lives in the traits below.
pub use nt_core::model::{Drawing, InkKind, InkPoint, Shape, ShapeKind, Stroke};

pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// A new drawing's canvas (Notes' "Insert Drawing": 768 × 358, as the research's 33 Drawing).
pub const NEW_W: f32 = 768.0;
pub const NEW_H: f32 = 358.0;

pub trait DrawingExt {
    fn new() -> Self;
}

impl DrawingExt for Drawing {
    fn new() -> Drawing {
        Drawing { width: NEW_W, height: NEW_H, strokes: Vec::new(), ..Default::default() }
    }
}

pub trait PointExt {
    fn new(x: f32, y: f32, t: f32, size: f32) -> Self;
    fn pos(&self) -> Pos2;
}

impl PointExt for InkPoint {
    fn new(x: f32, y: f32, t: f32, size: f32) -> InkPoint {
        InkPoint { x, y, t, size, force: 1.0, azimuth: 0.0, altitude: std::f32::consts::FRAC_PI_2, opacity: 1.0 }
    }

    fn pos(&self) -> Pos2 {
        pos2(self.x, self.y)
    }
}

pub trait StrokeExt {
    fn placed(&self) -> Vec<InkPoint>;
    fn bounds(&self) -> Rect;
    fn hits(&self, p: Pos2, slack: f32) -> bool;
    fn moved(&mut self, by: Vec2);
    fn color32(&self) -> Color32;
}

impl StrokeExt for Stroke {
    /// The stroke's points with its transform applied.
    fn placed(&self) -> Vec<InkPoint> {
        let [a, b, c, d, tx, ty] = self.transform;
        if self.transform == IDENTITY {
            return self.points.clone();
        }
        let k = ((a * d - b * c).abs()).sqrt().max(0.01);
        self.points.iter().map(|p| InkPoint { x: a * p.x + c * p.y + tx, y: b * p.x + d * p.y + ty, size: p.size * k, ..*p }).collect()
    }

    fn bounds(&self) -> Rect {
        let mut r = Rect::NOTHING;
        for p in &self.placed() {
            r = r.union(Rect::from_center_size(p.pos(), Vec2::splat(p.size.max(1.0))));
        }
        r
    }

    /// Whether `p` is on the line (within `slack`).
    fn hits(&self, p: Pos2, slack: f32) -> bool {
        let pts = self.placed();
        if !self.bounds().expand(slack).contains(p) {
            return false;
        }
        if pts.len() == 1 {
            return (pts[0].pos() - p).length() <= pts[0].size / 2.0 + slack;
        }
        pts.windows(2).any(|w| dist_to_segment(p, w[0].pos(), w[1].pos()) <= w[0].size.max(w[1].size) / 2.0 + slack)
    }

    fn moved(&mut self, by: Vec2) {
        self.transform[4] += by.x;
        self.transform[5] += by.y;
    }

    fn color32(&self) -> Color32 {
        let [r, g, b, a] = self.color;
        // (a stroke with its own per-point opacity, as Apple's pencil and marker: that, not ours)
        let own = self.points.iter().any(|p| p.opacity < 0.999);
        let alpha = if own { 1.0 } else { match self.ink {
            InkKind::Marker => 0.5,
            // (NO3 D3: Apple's pencil is a light grey line even in black)
            InkKind::Pencil => 0.4,
            InkKind::Crayon => 0.92,
            InkKind::Watercolor => 0.45,
            _ => 1.0,
        } } * a;
        Color32::from_rgba_unmultiplied((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, (alpha * 255.0) as u8)
    }
}

pub fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 == 0.0 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

// ---- turning pointer samples into stroke points

/// The five sizes Notes' tool picker offers for an ink (points; its smallest Monoline measures
/// 1.15 pt in the NO3 captures, the rest follow the picker's squiggles).
pub fn ink_widths(ink: InkKind) -> [f32; 5] {
    match ink {
        InkKind::Monoline => [1.15, 2.0, 3.5, 6.0, 10.0],
        InkKind::Pen => [1.3, 2.3, 4.0, 6.5, 10.0],
        InkKind::Marker => [6.0, 10.0, 15.0, 22.0, 30.0],
        InkKind::Pencil => [1.5, 2.5, 4.0, 6.0, 9.0],
        InkKind::Crayon => [4.0, 7.0, 11.0, 16.0, 22.0],
        InkKind::Fountain => [1.6, 3.0, 5.0, 8.0, 12.0],
        InkKind::Reed => [3.0, 5.0, 8.0, 12.0, 18.0],
        InkKind::Watercolor => [6.0, 10.0, 16.0, 24.0, 34.0],
    }
}

/// The calligraphic nibs: how wide a stroke is going in direction `d` (a share of its width).
fn nib(ink: InkKind, d: Vec2) -> f32 {
    let (angle, thin) = match ink {
        InkKind::Fountain => (std::f32::consts::FRAC_PI_4, 0.3),
        InkKind::Reed => (std::f32::consts::FRAC_PI_6, 0.18),
        _ => return 1.0,
    };
    if d.length() < 1e-4 {
        return 1.0;
    }
    let a = d.y.atan2(d.x) - angle;
    thin + (1.0 - thin) * a.sin().abs()
}

/// The stroke being drawn: raw samples come in, smoothed points go out.
#[derive(Clone, Debug)]
pub struct Builder {
    pub stroke: Stroke,
    last: Option<(Pos2, f64)>,
    start: f64,
    smoothed: Option<Pos2>,
}

impl Builder {
    pub fn new(ink: InkKind, color: [f32; 4], width: f32, at: Pos2, time: f64, force: Option<f32>) -> Builder {
        let mut b = Builder { stroke: Stroke { id: super::doc::new_uuid(), ink, color, width, points: Vec::new(), transform: IDENTITY, mask: None }, last: None, start: time, smoothed: None };
        b.add(at, time, force);
        b
    }

    /// A pointer sample (drawing points). The position is smoothed against the last one (more
    /// when it moves slowly, as a jittery hand does), the width follows the speed (and force
    /// when the device gives one): a pen thins when it moves fast.
    pub fn add(&mut self, at: Pos2, time: f64, force: Option<f32>) {
        let speed = match self.last {
            Some((p, t)) if time > t => (at - p).length() / (time - t) as f32,
            _ => 0.0,
        };
        if let Some((p, _)) = self.last {
            if (at - p).length() < 0.35 {
                return;
            }
        }
        let k = (0.35 + speed / 2000.0).clamp(0.35, 0.85);
        let sm = match self.smoothed {
            Some(s) => s + (at - s) * k,
            None => at,
        };
        self.smoothed = Some(sm);
        self.last = Some((at, time));
        let w = self.stroke.width;
        let f = force.unwrap_or(1.0).clamp(0.05, 1.5);
        let size = match self.stroke.ink {
            InkKind::Pen | InkKind::Fountain => w * (1.15 - (speed / 1600.0).min(0.45)) * (0.6 + 0.4 * f),
            InkKind::Pencil | InkKind::Crayon => w * (1.0 - (speed / 2500.0).min(0.3)) * (0.5 + 0.5 * f),
            InkKind::Marker | InkKind::Monoline | InkKind::Watercolor | InkKind::Reed => w,
        };
        // the width eases too, so it doesn't step
        let size = match self.stroke.points.last() {
            Some(p) => p.size + (size - p.size) * 0.4,
            None => size,
        };
        let mut pt = InkPoint::new(sm.x, sm.y, (time - self.start) as f32, size.max(0.3));
        pt.force = f;
        self.stroke.points.push(pt);
    }

    /// The stroke as it shows while drawn: up to the pointer itself, so the ink is under it with
    /// no smoothing lag (Notes draws it so).
    pub fn shown(&self) -> Stroke {
        let mut s = self.stroke.clone();
        if let (Some((p, t)), Some(last)) = (self.last, s.points.last().copied()) {
            if (p - last.pos()).length() > 0.3 {
                let mut e = InkPoint::new(p.x, p.y, (t - self.start) as f32, last.size);
                e.force = last.force;
                s.points.push(e);
            }
        }
        s
    }

    /// The pointer lifted: the last sample itself ends the stroke.
    pub fn finish(mut self) -> Stroke {
        if let (Some((p, t)), Some(last)) = (self.last, self.stroke.points.last().copied()) {
            if (p - last.pos()).length() > 0.3 {
                let mut e = InkPoint::new(p.x, p.y, (t - self.start) as f32, last.size);
                e.force = last.force;
                self.stroke.points.push(e);
            }
        }
        self.stroke
    }
}

// ---- outline

/// A stroke's centre line: PencilKit's — its points are the control points of a uniform cubic
/// B-spline (the curve runs near them, not through them; NO3's Apple drawings only match so),
/// the ends repeated so it starts and ends on them; each piece cut into steps of about `step`
/// (the points' units), 1 to 6 a piece.
pub fn centre_line(pts: &[InkPoint], step: f32) -> Vec<(Pos2, f32)> {
    centre_line_op(pts, step).into_iter().map(|(p, w, _)| (p, w)).collect()
}

/// The centre line with each point's opacity too (PencilKit's per-point opacity).
pub fn centre_line_op(pts: &[InkPoint], step: f32) -> Vec<(Pos2, f32, f32)> {
    if pts.len() < 3 {
        return pts.iter().map(|p| (p.pos(), p.size, p.opacity)).collect();
    }
    let n = pts.len();
    // clamped: the first and last points twice more
    let at = |i: isize| pts[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::with_capacity(n * 2);
    for i in -2..(n as isize - 1) {
        let (p0, p1, p2, p3) = (at(i), at(i + 1), at(i + 2), at(i + 3));
        let steps = (((p2.pos() - p1.pos()).length() / step.max(1e-3)).ceil() as usize).clamp(1, 6);
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let w0 = (1.0 - t).powi(3) / 6.0;
            let w1 = (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0;
            let w2 = (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0;
            let w3 = t3 / 6.0;
            let f = |a: f32, b: f32, c: f32, d: f32| w0 * a + w1 * b + w2 * c + w3 * d;
            out.push((pos2(f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y)), f(p0.size, p1.size, p2.size, p3.size), f(p0.opacity, p1.opacity, p2.opacity, p3.opacity)));
        }
    }
    let l = pts[n - 1];
    out.push((l.pos(), l.size, l.opacity));
    out
}

/// A stroke's outline in screen space (via `to_screen`, its width times `scale`): the two edges
/// of its ribbon and its round ends. A pen's ends taper a little, as PencilKit's do.
struct Outline {
    left: Vec<Pos2>,
    right: Vec<Pos2>,
    /// each point's normal and half width (for soft edges)
    nrm: Vec<(Vec2, f32)>,
    /// each point's opacity
    op: Vec<f32>,
    ends: Vec<(Pos2, f32)>,
}

fn outline(s: &Stroke, to_screen: &dyn Fn(Pos2) -> Pos2, scale: f32) -> Outline {
    let pts = s.placed();
    // pieces about 2.5 screen points long: smooth curves, no more triangles than they need
    let full = centre_line_op(&pts, 2.5 / scale.max(0.01));
    let op: Vec<f32> = full.iter().map(|x| x.2.clamp(0.0, 1.0)).collect();
    let mut line: Vec<(Pos2, f32)> = full.into_iter().map(|(p, w, _)| (to_screen(p), w * scale)).collect();
    if matches!(s.ink, InkKind::Pen | InkKind::Fountain) && line.len() > 6 {
        let n = line.len();
        for k in 0..3 {
            let f = 0.55 + 0.15 * k as f32;
            line[k].1 *= f;
            line[n - 1 - k].1 *= f;
        }
    }
    let half = |w: f32| (w / 2.0).max(0.25);
    let n = line.len();
    if n <= 1 {
        return Outline { left: Vec::new(), right: Vec::new(), nrm: Vec::new(), ends: line.iter().map(|l| (l.0, half(l.1))).collect(), op };
    }
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    let mut nrms = Vec::with_capacity(n);
    for i in 0..n {
        let a = line[i.saturating_sub(1)].0;
        let b = line[(i + 1).min(n - 1)].0;
        let d = b - a;
        let nrm = if d.length() > 1e-4 { vec2(-d.y, d.x).normalized() } else { vec2(0.0, 1.0) };
        // a fountain pen's and a reed's nib: thin along it, broad across it
        let h = half(line[i].1 * nib(s.ink, d));
        left.push(line[i].0 + nrm * h);
        right.push(line[i].0 - nrm * h);
        nrms.push((nrm, h));
    }
    // flat ends for the marker, the reed, the crayon and watercolour (their overlaps would show)
    let ends = if !matches!(s.ink, InkKind::Marker | InkKind::Reed | InkKind::Crayon | InkKind::Watercolor) { vec![(line[0].0, half(line[0].1 * nib(s.ink, line[1].0 - line[0].0))), (line[n - 1].0, half(line[n - 1].1 * nib(s.ink, line[n - 1].0 - line[n - 2].0)))] } else { Vec::new() };
    Outline { left, right, nrm: nrms, ends, op }
}

/// Triangles covering a stroke (screen space via `to_screen`, width times `scale`).
pub fn triangles(s: &Stroke, to_screen: &dyn Fn(Pos2) -> Pos2, scale: f32) -> Vec<[Pos2; 3]> {
    let o = outline(s, to_screen, scale);
    let mut tris = Vec::with_capacity(o.left.len() * 2 + 48);
    for i in 0..o.left.len().saturating_sub(1) {
        tris.push([o.left[i], o.right[i], o.left[i + 1]]);
        tris.push([o.right[i], o.right[i + 1], o.left[i + 1]]);
    }
    for (c, r) in o.ends {
        let k = cap_segments(r);
        for i in 0..k {
            tris.push([c, cap_point(c, r, i, k), cap_point(c, r, i + 1, k)]);
        }
    }
    tris
}

fn cap_segments(r: f32) -> usize {
    ((r * 2.0) as usize).clamp(8, 24)
}

fn cap_point(c: Pos2, r: f32, i: usize, k: usize) -> Pos2 {
    let a = i as f32 / k as f32 * std::f32::consts::TAU;
    c + vec2(a.cos(), a.sin()) * r
}

/// A stroke's visible region (PencilKit's mask, what Apple's pixel eraser left), placed as the
/// stroke is (its transform applied).
pub fn mask_placed(s: &Stroke) -> Option<Vec<Pos2>> {
    let m = s.mask.as_ref().filter(|m| m.len() >= 3)?;
    let [a, b, c, d, tx, ty] = s.transform;
    Some(m.iter().map(|p| pos2(a * p[0] + c * p[1] + tx, b * p[0] + d * p[1] + ty)).collect())
}

/// A stroke as one indexed mesh: the ribbon's edges share their vertices, the ends are fans;
/// with a mask, only the triangles inside it (its points are already cut to it, so this trims
/// the ends of the cuts).
pub fn mesh(s: &Stroke, to_screen: &dyn Fn(Pos2) -> Pos2, scale: f32) -> egui::Mesh {
    let mut m = mesh_unmasked(s, to_screen, scale);
    if let Some(poly) = mask_placed(s) {
        let poly: Vec<Pos2> = poly.into_iter().map(to_screen).collect();
        let v = &m.vertices;
        let kept: Vec<u32> = m.indices.chunks(3).filter(|t| {
            let c = (v[t[0] as usize].pos.to_vec2() + v[t[1] as usize].pos.to_vec2() + v[t[2] as usize].pos.to_vec2()) / 3.0;
            inside(&poly, c.to_pos2())
        }).flatten().copied().collect();
        m.indices = kept;
    }
    m
}

fn mesh_unmasked(s: &Stroke, to_screen: &dyn Fn(Pos2) -> Pos2, scale: f32) -> egui::Mesh {
    let col = s.color32();
    let o = outline(s, to_screen, scale);
    let mut m = egui::Mesh::default();
    m.reserve_vertices(o.left.len() * 2 + o.ends.len() * 25);
    m.reserve_triangles(o.left.len() * 2 + o.ends.len() * 24);
    // a pencil's and a crayon's grain: the ink thins and thickens along the stroke
    let grain = |i: usize| -> Color32 {
        let k = match s.ink {
            InkKind::Pencil => 0.55,
            InkKind::Crayon => 0.7,
            _ => return col,
        };
        let h = ((i as u32).wrapping_mul(2654435761) >> 16) as f32 / 65535.0;
        col.gamma_multiply(k + (1.0 - k) * h)
    };
    if matches!(s.ink, InkKind::Watercolor) {
        // soft edges (watercolour): clear outer rows beyond each side
        for (i, (l, r)) in o.left.iter().zip(&o.right).enumerate() {
            let (n, h) = o.nrm[i];
            let k = if s.ink == InkKind::Marker { 0.3 } else { 0.6 };
            m.colored_vertex(*l + n * h * k, Color32::TRANSPARENT);
            m.colored_vertex(*l, col);
            m.colored_vertex(*r, col);
            m.colored_vertex(*r - n * h * k, Color32::TRANSPARENT);
        }
        for i in 0..o.left.len().saturating_sub(1) as u32 {
            let (a, b) = (4 * i, 4 * i + 4);
            for k in 0..3 {
                m.add_triangle(a + k, a + k + 1, b + k);
                m.add_triangle(a + k + 1, b + k + 1, b + k);
            }
        }
        return m;
    }
    let op = |i: usize| o.op.get(i).copied().unwrap_or(1.0);
    for (i, (l, r)) in o.left.iter().zip(&o.right).enumerate() {
        m.colored_vertex(*l, grain(2 * i).gamma_multiply(op(i)));
        m.colored_vertex(*r, grain(2 * i + 7).gamma_multiply(op(i)));
    }
    for i in 0..o.left.len().saturating_sub(1) as u32 {
        let a = 2 * i;
        m.add_triangle(a, a + 1, a + 2);
        m.add_triangle(a + 1, a + 3, a + 2);
    }
    let ends_op = [op(0), op(o.op.len().saturating_sub(1))];
    for (e, (c, r)) in o.ends.into_iter().enumerate() {
        let col = col.gamma_multiply(ends_op[e.min(1)]);
        let k = cap_segments(r);
        let base = m.vertices.len() as u32;
        m.colored_vertex(c, col);
        for i in 0..k {
            m.colored_vertex(cap_point(c, r, i, k), col);
        }
        for i in 0..k as u32 {
            m.add_triangle(base, base + 1 + i, base + 1 + (i + 1) % k as u32);
        }
    }
    m
}

/// Paints a stroke with egui.
pub fn paint_stroke(painter: &egui::Painter, s: &Stroke, to_screen: &dyn Fn(Pos2) -> Pos2, scale: f32) {
    painter.add(egui::Shape::mesh(mesh(s, to_screen, scale)));
}

/// Strokes into a picture (`scale` pixels a point); each stroke's triangles are one path filled
/// once, so its overlaps don't darken.
pub fn render_strokes(pm: &mut tiny_skia::Pixmap, strokes: &[Stroke], scale: f32) {
    for s in strokes {
        // (one paint a stroke: its points' average opacity)
        let avg = if s.points.is_empty() { 1.0 } else { s.points.iter().map(|p| p.opacity.clamp(0.0, 1.0)).sum::<f32>() / s.points.len() as f32 };
        let c = s.color32().gamma_multiply(avg);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(c.r(), c.g(), c.b(), c.a());
        paint.anti_alias = true;
        let mut pb = tiny_skia::PathBuilder::new();
        for t in triangles(s, &|p| pos2(p.x * scale, p.y * scale), scale) {
            pb.move_to(t[0].x, t[0].y);
            pb.line_to(t[1].x, t[1].y);
            pb.line_to(t[2].x, t[2].y);
            pb.close();
        }
        // clipped to its mask, exactly, when it has one
        let clip = mask_placed(s).and_then(|poly| {
            let mut mb = tiny_skia::PathBuilder::new();
            mb.move_to(poly[0].x * scale, poly[0].y * scale);
            for q in &poly[1..] {
                mb.line_to(q.x * scale, q.y * scale);
            }
            mb.close();
            let path = mb.finish()?;
            let mut mask = tiny_skia::Mask::new(pm.width(), pm.height())?;
            mask.fill_path(&path, tiny_skia::FillRule::Winding, true, tiny_skia::Transform::identity());
            Some(mask)
        });
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), clip.as_ref());
        }
    }
}

/// The drawing as a PNG (`scale` pixels a point) on `background` (None: transparent), with
/// strokes over a picture when `under` is given (Markup flattened).
pub fn render_png(d: &Drawing, scale: f32, under: Option<&image::RgbaImage>) -> Vec<u8> {
    let (w, h) = ((d.width * scale).round().max(1.0) as u32, (d.height * scale).round().max(1.0) as u32);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    if let Some(img) = under {
        let img = image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
        for (i, px) in img.pixels().enumerate() {
            let [r, g, b, a] = px.0;
            pm.pixels_mut()[i] = tiny_skia::PremultipliedColorU8::from_rgba((r as u16 * a as u16 / 255) as u8, (g as u16 * a as u16 / 255) as u8, (b as u16 * a as u16 / 255) as u8, a).unwrap();
        }
    }
    // (Apple's drawings: the ink over the shapes, NO3 33 Drawing)
    for sh in &d.shapes {
        super::shapes::render(&mut pm, sh, scale);
    }
    render_strokes(&mut pm, &d.strokes, scale);
    let mut out = Vec::new();
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let p = pm.pixel(x, y).unwrap().demultiply();
        image::Rgba([p.red(), p.green(), p.blue(), p.alpha()])
    });
    let _ = img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    out
}

// ---- erasing and selecting

/// The pixel eraser: the parts of strokes within `r` of `p` go; a stroke cut in the middle
/// becomes two. Where the eraser passes between two of a stroke's points, the line is cut there
/// too (points are added along it first, a third of the eraser apart).
pub fn erase_at(strokes: &mut Vec<Stroke>, p: Pos2, r: f32) -> bool {
    let mut changed = false;
    let mut out = Vec::with_capacity(strokes.len());
    for s in strokes.drain(..) {
        if !s.bounds().expand(r).contains(p) {
            out.push(s);
            continue;
        }
        let placed = s.placed();
        let reach = |q: &InkPoint| r + q.size / 2.0;
        // the points, with more along the segments the eraser touches
        let mut pts: Vec<InkPoint> = Vec::with_capacity(placed.len());
        for (i, q) in placed.iter().enumerate() {
            if let Some(prev) = i.checked_sub(1).map(|j| placed[j]) {
                let (a, b) = (prev.pos(), q.pos());
                if dist_to_segment(p, a, b) <= reach(q).max(reach(&prev)) {
                    let step = (r / 3.0).max(0.4);
                    let n = ((b - a).length() / step).ceil() as usize;
                    for k in 1..n {
                        let t = k as f32 / n as f32;
                        pts.push(InkPoint { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, t: prev.t + (q.t - prev.t) * t, size: prev.size + (q.size - prev.size) * t, force: prev.force + (q.force - prev.force) * t, ..*q });
                    }
                }
            }
            pts.push(*q);
        }
        let keep: Vec<bool> = pts.iter().map(|q| (q.pos() - p).length() > reach(q)).collect();
        if keep.iter().all(|k| *k) {
            out.push(s);
            continue;
        }
        changed = true;
        let mut run: Vec<InkPoint> = Vec::new();
        let mut flush = |run: &mut Vec<InkPoint>| {
            // (a lone point left between two cuts would show as a dot)
            if run.len() > 1 || placed.len() == 1 && run.len() == 1 {
                // (an Apple stroke's visible region goes with its pieces, placed as their points)
                let mask = mask_placed(&s).map(|m| m.iter().map(|p| [p.x, p.y]).collect());
                out.push(Stroke { id: super::doc::new_uuid(), points: std::mem::take(run), transform: IDENTITY, mask, ..s.clone() });
            }
            run.clear();
        };
        for (q, k) in pts.iter().zip(&keep) {
            if *k {
                run.push(*q);
            } else {
                flush(&mut run);
            }
        }
        flush(&mut run);
    }
    *strokes = out;
    changed
}

/// Whether a point is inside a closed polygon (the lasso).
pub fn inside(poly: &[Pos2], p: Pos2) -> bool {
    let mut c = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

/// The strokes the lasso takes: those with most of their points inside it.
pub fn lasso(strokes: &[Stroke], poly: &[Pos2]) -> Vec<String> {
    strokes
        .iter()
        .filter(|s| {
            let pts = s.placed();
            let n = pts.iter().filter(|q| inside(poly, q.pos())).count();
            n * 2 > pts.len()
        })
        .map(|s| s.id.clone())
        .collect()
}

/// A point drawn near the ruler's edge goes onto it.
pub fn snap_to_ruler(p: Pos2, centre: Pos2, angle: f32, half_w: f32, reach: f32) -> Pos2 {
    let dir = vec2(angle.cos(), angle.sin());
    let nrm = vec2(-dir.y, dir.x);
    let rel = p - centre;
    let along = rel.dot(dir);
    let across = rel.dot(nrm);
    for edge in [-half_w, half_w] {
        if (across - edge).abs() < reach {
            return centre + dir * along + nrm * (edge + edge.signum() * 0.0);
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_smooths_and_thins_with_speed() {
        let mut b = Builder::new(InkKind::Pen, [0.0, 0.0, 0.0, 1.0], 3.0, pos2(0.0, 0.0), 0.0, None);
        // slow then fast
        for i in 1..=10 {
            b.add(pos2(i as f32 * 1.0, 0.0), i as f64 * 0.02, None);
        }
        for i in 1..=10 {
            b.add(pos2(10.0 + i as f32 * 40.0, 0.0), 0.2 + i as f64 * 0.01, None);
        }
        let s = b.finish();
        let slow = s.points[8].size;
        let fast = s.points[s.points.len() - 2].size;
        assert!(fast < slow, "{slow} {fast}");
        assert!(s.points.windows(2).all(|w| w[1].t >= w[0].t));
        assert_eq!(s.points.last().unwrap().x, 410.0, "ends where the pointer lifted");
    }

    #[test]
    fn pixel_eraser_splits_a_line() {
        let pts: Vec<InkPoint> = (0..=20).map(|i| InkPoint::new(i as f32 * 5.0, 0.0, 0.0, 2.0)).collect();
        let mut v = vec![Stroke { id: "a".into(), ink: InkKind::Pen, color: [0.0, 0.0, 0.0, 1.0], width: 2.0, points: pts, transform: IDENTITY, mask: None }];
        assert!(erase_at(&mut v, pos2(50.0, 0.0), 6.0));
        assert_eq!(v.len(), 2);
        assert!(v[0].points.iter().all(|p| p.x < 44.0) && v[1].points.iter().all(|p| p.x > 56.0));
        // the object eraser test: hits
        assert!(v[0].hits(pos2(20.0, 1.0), 1.0));
        assert!(!v[0].hits(pos2(20.0, 9.0), 1.0));
    }

    #[test]
    fn lasso_takes_strokes_inside() {
        let line = |y: f32| Stroke { id: format!("{y}"), ink: InkKind::Pen, color: [0.0; 4], width: 2.0, points: (0..5).map(|i| InkPoint::new(10.0 + i as f32 * 5.0, y, 0.0, 2.0)).collect(), transform: IDENTITY, mask: None };
        let v = vec![line(10.0), line(100.0)];
        let poly = [pos2(0.0, 0.0), pos2(60.0, 0.0), pos2(60.0, 30.0), pos2(0.0, 30.0)];
        assert_eq!(lasso(&v, &poly), vec!["10".to_string()]);
    }

    #[test]
    fn a_mask_clips_the_stroke() {
        // a line across, its mask the left half only (as Apple's pixel eraser leaves it)
        let mut s = Stroke { id: "m".into(), ink: InkKind::Monoline, color: [0.0, 0.0, 0.0, 1.0], width: 6.0, points: (0..=20).map(|i| InkPoint::new(10.0 + i as f32 * 4.0, 25.0, 0.0, 6.0)).collect(), transform: IDENTITY, mask: None };
        s.mask = Some(vec![[0.0, 0.0], [50.0, 0.0], [50.0, 50.0], [0.0, 50.0]]);
        let d = Drawing { width: 100.0, height: 50.0, strokes: vec![s.clone()], ..Default::default() };
        let img = image::load_from_memory(&render_png(&d, 1.0, None)).unwrap().to_rgba8();
        assert!(img.get_pixel(30, 25)[3] > 200, "inside the mask: drawn");
        assert_eq!(img.get_pixel(70, 25)[3], 0, "outside the mask: not drawn");
        // on screen: no triangle beyond it
        let m = mesh(&s, &|p| p, 1.0);
        assert!(!m.indices.is_empty());
        assert!(m.indices.iter().all(|&i| m.vertices[i as usize].pos.x <= 56.0), "nothing drawn right of the mask");
    }

    #[test]
    fn png_renders_the_ink() {
        let mut d = Drawing { width: 100.0, height: 50.0, strokes: Vec::new(), ..Default::default() };
        d.strokes.push(Stroke { id: "a".into(), ink: InkKind::Pen, color: [1.0, 0.0, 0.0, 1.0], width: 4.0, points: (0..10).map(|i| InkPoint::new(10.0 + i as f32 * 8.0, 25.0, 0.0, 4.0)).collect(), transform: IDENTITY, mask: None });
        let png = render_png(&d, 2.0, None);
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (200, 100));
        assert!(img.get_pixel(100, 50)[0] > 200 && img.get_pixel(100, 50)[3] > 200, "red in the middle of the line");
        assert_eq!(img.get_pixel(100, 10)[3], 0, "clear off the line");
    }
}
