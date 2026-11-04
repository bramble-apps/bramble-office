//! Painting helpers: theme colours, gradient rounded shapes, icons, key labels and number lines.

use crate::fonts;
use calc_core::calc::Seg;
use calc_core::icons::{self, Prim};
use calc_core::layout::{KeyBox, Lab, Rect as CRect};
use calc_core::theme::{Family, KeyStyle, Press, Theme};
use egui::epaint::Mesh;
use egui::{pos2, vec2, Color32, FontId, Galley, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

pub fn col(c: u32) -> Color32 {
    Color32::from_rgba_unmultiplied((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub fn rect(r: CRect) -> Rect {
    Rect::from_min_size(pos2(r.x, r.y), vec2(r.w, r.h))
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

pub fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

/// The outline of a rounded rectangle, clockwise from the top of the top-right corner.
pub fn rrect_pts(r: Rect, rad: f32) -> Vec<Pos2> {
    let rad = rad.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    if rad < 0.5 {
        return vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    }
    let n = ((rad * 0.5).ceil() as usize).clamp(3, 20);
    let corners = [
        (r.right_top() + vec2(-rad, rad), -FRAC_PI_2),
        (r.right_bottom() + vec2(-rad, -rad), 0.0),
        (r.left_bottom() + vec2(rad, -rad), FRAC_PI_2),
        (r.left_top() + vec2(rad, rad), PI),
    ];
    let mut pts = Vec::with_capacity(4 * (n + 1));
    for (c, a0) in corners {
        for i in 0..=n {
            let a = a0 + FRAC_PI_2 * i as f32 / n as f32;
            let q = c + vec2(a.cos(), a.sin()) * rad;
            if pts.last().is_none_or(|l: &Pos2| l.distance(q) > 0.01) {
                pts.push(q);
            }
        }
    }
    if pts.len() > 2 && pts[0].distance(*pts.last().unwrap()) < 0.01 {
        pts.pop();
    }
    pts
}

/// Fills a convex outline with a vertical gradient from `top` (at y0) to `bot` (at y1), with a
/// one-pixel feathered edge.
pub fn fill_convex(p: &Painter, pts: &[Pos2], top: Color32, bot: Color32, y0: f32, y1: f32) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let feather = 1.0 / p.pixels_per_point();
    let cen = pts.iter().fold(Vec2::ZERO, |a, q| a + q.to_vec2()) / n as f32;
    let cen = cen.to_pos2();
    let span = (y1 - y0).abs().max(1e-3);
    let cf = |y: f32| mix(top, bot, (y - y0) / span);
    let mut m = Mesh::default();
    m.colored_vertex(cen, cf(cen.y));
    for i in 0..n {
        let (prev, cur, next) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let d = (next - prev).normalized();
        let mut nrm = vec2(d.y, -d.x);
        if nrm.dot(cur - cen) < 0.0 {
            nrm = -nrm;
        }
        let c = cf(cur.y);
        m.colored_vertex(cur - nrm * feather * 0.5, c);
        m.colored_vertex(cur + nrm * feather * 0.5, Color32::TRANSPARENT);
    }
    for i in 0..n as u32 {
        let a = 1 + 2 * i;
        let b = 1 + 2 * ((i + 1) % n as u32);
        m.add_triangle(0, a, b);
        m.add_triangle(a, a + 1, b + 1);
        m.add_triangle(a, b + 1, b);
    }
    p.add(Shape::mesh(m));
}

/// A rounded rectangle with a vertical gradient (solid when both colours match).
pub fn rrect(p: &Painter, r: Rect, rad: f32, top: Color32, bot: Color32) {
    if top.a() == 0 && bot.a() == 0 {
        return;
    }
    fill_convex(p, &rrect_pts(r, rad), top, bot, r.top(), r.bottom());
}

pub fn rrect_stroke(p: &Painter, r: Rect, rad: f32, w: f32, c: Color32) {
    if c.a() == 0 {
        return;
    }
    let r = r.shrink(w / 2.0);
    p.add(Shape::closed_line(rrect_pts(r, (rad - w / 2.0).max(0.0)), Stroke::new(w, c)));
}

/// Points round a rounded rectangle with their outward normals (after Bramble Office's
/// kn-ui glass `outline`): corner arcs plus points along the straight sides.
fn outline(r: Rect, radius: f32) -> Vec<(Pos2, Vec2)> {
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.5);
    let corners = [
        (pos2(r.min.x + rad, r.min.y + rad), PI),
        (pos2(r.max.x - rad, r.min.y + rad), 1.5 * PI),
        (pos2(r.max.x - rad, r.max.y - rad), 0.0),
        (pos2(r.min.x + rad, r.max.y - rad), FRAC_PI_2),
    ];
    let steps = ((rad * 0.5).ceil() as usize).clamp(6, 20);
    let mut out: Vec<(Pos2, Vec2)> = Vec::with_capacity(4 * (steps + 8));
    for (i, (c, a0)) in corners.iter().enumerate() {
        for k in 0..=steps {
            let a = a0 + k as f32 / steps as f32 * FRAC_PI_2;
            let n = vec2(a.cos(), a.sin());
            out.push((*c + n * rad, n));
        }
        let (next, na) = corners[(i + 1) % 4];
        let end = next + vec2(na.cos(), na.sin()) * rad;
        let (start, n) = *out.last().unwrap();
        let parts = ((end - start).length() / 8.0).floor() as usize;
        for j in 1..parts {
            out.push((start + (end - start) * (j as f32 / parts as f32), n));
        }
    }
    out
}

/// A Liquid Glass button (iOS 26 keys, toolbar circles, capsules), Bramble Office's glass
/// without the see-through: a faint tight contact shadow, the opaque `top`→`bottom` fill, a
/// white rim lit at 45° on a thin feathered band just inside the edge, and a hairline white
/// edge. The fill's alpha fades the whole button.
pub fn glass_shape(p: &Painter, r: Rect, radius: f32, top: Color32, bottom: Color32) {
    let a = top.a().max(bottom.a()) as f32 / 255.0;
    if a <= 0.0 || r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    let radius = radius.min(r.width() / 2.0).min(r.height() / 2.0);
    let shadow = egui::epaint::Shadow { offset: [0, 1], blur: 8, spread: 0, color: Color32::from_black_alpha((40.0 * a) as u8) };
    p.add(shadow.as_shape(r, radius.min(255.0)));
    rrect(p, r, radius, top, bottom);
    // rim light at 45° (both lit corners), up to 42% white; the band widens a little on big
    // keys so it still reads at 80 px
    let s = (radius / 20.0).clamp(1.0, 2.5);
    let dir = vec2(std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    let pts = outline(r, radius);
    let n = pts.len() as u32;
    let mut m = Mesh::default();
    m.reserve_vertices(3 * pts.len());
    m.reserve_triangles(4 * pts.len());
    for (q, nrm) in &pts {
        let k = nrm.dot(dir).abs();
        m.colored_vertex(*q + *nrm * 0.4 * s, Color32::TRANSPARENT);
        m.colored_vertex(*q - *nrm * 0.2 * s, Color32::from_white_alpha((k * 0.42 * a * 255.0) as u8));
        m.colored_vertex(*q - *nrm * 1.0 * s, Color32::TRANSPARENT);
    }
    for k in 0..n {
        let (i, j) = (3 * k, 3 * ((k + 1) % n));
        for d in 0..2 {
            m.add_triangle(i + d, i + d + 1, j + d);
            m.add_triangle(i + d + 1, j + d + 1, j + d);
        }
    }
    p.add(Shape::mesh(m));
    // hairline edge
    p.add(Shape::rect_stroke(r, radius.min(255.0), Stroke::new(0.5, Color32::from_white_alpha((14.0 * a) as u8)), egui::StrokeKind::Inside));
}

/// An icon from calc-core, fitted into `r`.
pub fn icon(p: &Painter, name: &str, r: Rect, c: Color32) {
    let Some(ic) = icons::icon(name) else { return };
    let s = r.width().min(r.height()) / 24.0;
    let o = r.center() - vec2(12.0, 12.0) * s;
    let tp = |q: [f32; 2]| o + vec2(q[0], q[1]) * s;
    let sw = ic.stroke * s;
    let st = Stroke::new(sw, c);
    let cap = |q: Pos2| {
        p.circle_filled(q, sw / 2.0, c);
    };
    for prim in &ic.prims {
        match prim {
            Prim::Line { a, b } => {
                p.line_segment([tp(*a), tp(*b)], st);
                cap(tp(*a));
                cap(tp(*b));
            }
            Prim::Poly { pts, closed, fill } => {
                let v: Vec<Pos2> = pts.iter().map(|q| tp(*q)).collect();
                if *fill {
                    p.add(Shape::convex_polygon(v, c, Stroke::NONE));
                } else {
                    for q in &v {
                        cap(*q);
                    }
                    p.add(if *closed { Shape::closed_line(v, st) } else { Shape::line(v, st) });
                }
            }
            Prim::Circle { c: cc, r: rr, fill } => {
                if *fill {
                    p.circle_filled(tp(*cc), rr * s, c);
                } else {
                    p.circle_stroke(tp(*cc), rr * s, st);
                }
            }
            Prim::Arc { c: cc, r: rr, a0, a1 } => {
                let n = (((a1 - a0).abs() * rr * s) / 2.0).ceil().clamp(4.0, 64.0) as usize;
                let v: Vec<Pos2> = (0..=n)
                    .map(|i| {
                        let a = a0 + (a1 - a0) * i as f32 / n as f32;
                        tp(*cc) + vec2(a.cos(), a.sin()) * *rr * s
                    })
                    .collect();
                cap(v[0]);
                cap(v[n]);
                p.add(Shape::line(v, st));
            }
            Prim::RRect { min, max, r: rr, fill } => {
                let b = Rect::from_min_max(tp(*min), tp(*max));
                if *fill {
                    rrect(p, b, rr * s, c, c);
                } else {
                    p.add(Shape::closed_line(rrect_pts(b, rr * s), st));
                }
            }
        }
    }
}

/// y of the first row's baseline inside a galley.
pub fn baseline(g: &Galley) -> f32 {
    g.rows.first().map(|r| r.pos.y + r.glyphs.first().map(|gl| gl.pos.y).unwrap_or(r.size.y * 0.8)).unwrap_or(0.0)
}

/// The size text is laid out at for `size`: a coarse grid (½ px up to 24, 1 px up to 64,
/// 2 px above), the rest made up by scaling the galley. egui rasterizes glyphs per exact size,
/// so animated sizes (key morphs, the glass press, menus scaling in, window resizes) would
/// otherwise rasterize new glyphs every frame and churn the font atlas.
pub fn quant(size: f32) -> f32 {
    let step = if size <= 24.0 { 0.5 } else if size <= 64.0 { 1.0 } else { 2.0 };
    ((size / step).round() * step).max(step)
}

/// A galley for `s` in `f` (laid out at the quantized size) and the scale to draw it at.
pub fn galley(p: &Painter, s: String, f: FontId) -> (Arc<Galley>, f32) {
    let q = quant(f.size);
    let k = f.size / q;
    (p.layout_no_wrap(s, FontId::new(q, f.family), Color32::WHITE), k)
}

/// Paints a galley with its top-left at `pos`, scaled by `k`.
pub fn paint_galley(p: &Painter, pos: Pos2, g: Arc<Galley>, k: f32, c: Color32) {
    let mut ts = egui::epaint::TextShape::new(pos, g, c);
    ts.override_text_color = Some(c);
    let mut sh = Shape::Text(ts);
    if (k - 1.0).abs() > 1e-4 {
        sh.transform(egui::emath::TSTransform::new(pos.to_vec2() * (1.0 - k), k));
    }
    p.add(sh);
}

/// Laid-out pieces of a line: (galley, baseline shift, scale).
pub struct Line {
    pub parts: Vec<(Arc<Galley>, f32, f32)>,
    pub width: f32,
}

impl Line {
    pub fn paint(&self, p: &Painter, left: f32, base: f32, c: Color32) {
        let mut x = left;
        for (g, dy, k) in &self.parts {
            let pos = pos2(x, base + dy - baseline(g) * k);
            paint_galley(p, pos, g.clone(), *k, c);
            x += g.size().x * k;
        }
    }
    pub fn paint_right(&self, p: &Painter, right: f32, base: f32, c: Color32) {
        self.paint(p, right - self.width, base, c);
    }
}

/// Display segments: `sup` raised at ~60% size.
pub fn segs(p: &Painter, segs: &[Seg], size: f32, font: &dyn Fn(f32) -> FontId) -> Line {
    let mut parts = Vec::new();
    let mut width = 0.0;
    for s in segs {
        let (sz, dy) = if s.sup { (size * 0.6, -size * 0.42) } else { (size, 0.0) };
        let (g, k) = galley(p, s.t.clone(), font(sz));
        width += g.size().x * k;
        parts.push((g, dy, k));
    }
    Line { parts, width }
}

/// Key label pieces: v=1 raised small, v=-1 lowered small.
pub fn labs(p: &Painter, labs: &[Lab], size: f32, font: &dyn Fn(f32) -> FontId) -> Line {
    let mut parts = Vec::new();
    let mut width = 0.0;
    for l in labs {
        let (sz, dy) = match l.v {
            1 => (size * 0.6, -size * 0.36),
            -1 => (size * 0.6, size * 0.16),
            _ => (size, 0.0),
        };
        let (g, k) = galley(p, l.t.clone(), font(sz));
        width += g.size().x * k;
        parts.push((g, dy, k));
    }
    Line { parts, width }
}

/// The size that makes `line` (laid out at `size`) fit `avail`.
pub fn fit(width: f32, size: f32, avail: f32) -> f32 {
    if width > avail && width > 0.0 {
        size * avail / width
    } else {
        size
    }
}

pub fn text(p: &Painter, pos: Pos2, anchor: egui::Align2, s: &str, f: FontId, c: Color32) -> Rect {
    let (g, k) = galley(p, s.to_string(), f);
    let r = anchor.anchor_size(pos, g.size() * k);
    paint_galley(p, r.min, g, k, c);
    r
}

pub fn text_width(p: &Painter, s: &str, f: FontId) -> f32 {
    let (g, k) = galley(p, s.to_string(), f);
    g.size().x * k
}

/// The theme's KeyStyle for a key kind.
pub fn style<'a>(t: &'a Theme, k: &KeyBox) -> &'a KeyStyle {
    if k.active {
        return &t.active;
    }
    match k.kind {
        "digit" => &t.digit,
        "func" => &t.func,
        "op" => &t.op,
        "eq" => &t.eq,
        "mem" => &t.mem,
        _ => &t.sci,
    }
}

/// How a key is drawn this frame.
pub struct Look {
    pub rect: Rect,
    pub font: f32,
    pub alpha: f32,
    /// 0 = fill, 1 = pressed colour
    pub press: f32,
    /// Liquid Glass swell (0..1)
    pub glow: f32,
    pub invert: bool,
}

pub fn key(p: &Painter, t: &Theme, k: &KeyBox, l: &Look) {
    let st = style(t, k);
    let r = l.rect;
    let rad = if r.width() > 0.0 { k.radius * r.width() / k.rect.w.max(1.0) } else { k.radius };
    let a = l.alpha;
    let (mut top, mut bot, mut txt) = (col(st.fill), col(st.fill2), col(st.text));
    if l.press > 0.0 {
        top = mix(top, col(st.pressed), l.press);
        bot = mix(bot, col(st.pressed), l.press);
    }
    if l.invert {
        std::mem::swap(&mut top, &mut txt);
        bot = top;
    }
    if st.shadow & 0xFF != 0 {
        rrect(p, r.translate(vec2(2.0, 2.0)), rad, fade(col(st.shadow), a), fade(col(st.shadow), a));
    }
    if l.glow > 0.0 {
        for i in 1..=4 {
            let g = r.expand(i as f32 * 2.5 * l.glow);
            rrect(p, g, rad + i as f32 * 2.5 * l.glow, fade(Color32::WHITE, 0.035 * l.glow * a), fade(Color32::WHITE, 0.035 * l.glow * a));
        }
    }
    let glass = t.press == Press::Glass;
    if glass {
        // held: the fill brightens (pressed colour, then a little white with the swell)
        let (top, bot) = (mix(top, Color32::WHITE, 0.16 * l.glow), mix(bot, Color32::WHITE, 0.08 * l.glow));
        glass_shape(p, r, rad, fade(top, a), fade(bot, a));
    } else {
        rrect(p, r, rad, fade(top, a), fade(bot, a));
    }
    if st.gloss {
        let g = r.shrink(1.0);
        let cut = g.top() + g.height() * 0.45;
        let pts: Vec<Pos2> = rrect_pts(g, (rad - 1.0).max(0.0)).into_iter().map(|q| pos2(q.x, q.y.min(cut))).collect();
        fill_convex(p, &pts, fade(Color32::WHITE, 0.35 * a), fade(Color32::WHITE, 0.06 * a), g.top(), cut);
    }
    if st.border & 0xFF != 0 && !glass {
        rrect_stroke(p, r, rad, 1.0, fade(col(st.border), a));
    }
    let txt = fade(txt, a);
    if let Some(ic) = k.icon {
        let s = l.font * 1.05;
        icon(p, ic, Rect::from_center_size(r.center(), vec2(s, s)), txt);
    } else {
        let face = t.font;
        let w = t.label_weight;
        let f = move |sz: f32| fonts::font(face, w, sz);
        // single operator glyphs sit on the math axis: centre them by their ink
        let op = k.label.len() == 1 && matches!(k.label[0].t.as_str(), "+" | "−" | "×" | "÷" | "=" | "-" | "*" | "/" | "±" | "%");
        if op && !t.family.classic() || matches!(k.kind, "op" | "eq") && k.label.len() == 1 {
            let size = if t.family == Family::Mac1984 { l.font } else if t.family.classic() { l.font * 1.2 } else if k.label[0].t == "%" { l.font * 1.05 } else { l.font * 1.4 };
            let (g, gk) = galley(p, k.label[0].t.clone(), f(size));
            let pos = r.center() - g.mesh_bounds.center().to_vec2() * gk;
            paint_galley(p, pos, g, gk, txt);
            return;
        }
        let size = l.font;
        let line = labs(p, &k.label, size, &f);
        let size2 = fit(line.width, size, r.width() * 0.86);
        let line = if size2 < size { labs(p, &k.label, size2, &f) } else { line };
        let base = r.center().y + size2 * 0.36;
        line.paint(p, r.center().x - line.width / 2.0, base, txt);
    }
}

/// The classic device panel: gradient, border, 1984 title bar and hard shadow, window buttons.
pub fn body(p: &Painter, t: &Theme, b: Rect) {
    let rad = match t.family {
        Family::Mac1984 => 0.0,
        Family::Aqua2001 => 9.0,
        Family::Iphone2007 => b.width() * 0.07,
        _ => 10.0,
    };
    if t.family == Family::Mac1984 {
        p.rect_filled(b.translate(vec2(4.0, 4.0)), 0.0, Color32::BLACK);
    } else {
        for i in 1..=6 {
            let s = b.expand(i as f32 * 2.0).translate(vec2(0.0, 3.0));
            rrect(p, s, rad + i as f32 * 2.0, Color32::from_black_alpha(14), Color32::from_black_alpha(14));
        }
    }
    rrect(p, b, rad, col(t.body), col(t.body2));
    rrect_stroke(p, b, rad, if t.family == Family::Mac1984 { 2.0 } else { 1.0 }, col(t.body_border));
}

/// What sits over the panel after the display: the 1984 title bar, window buttons.
/// `title_h`: the 1984 title bar's height (layout.title_h).
pub fn body_chrome(p: &Painter, t: &Theme, b: Rect, title_h: f32) {
    match t.family {
        Family::Mac1984 => {
            let h = if title_h > 0.0 { title_h } else { b.height() * 0.06 };
            let bar = Rect::from_min_size(b.min, vec2(b.width(), h));
            let mut y = bar.top() + h * 0.2;
            while y < bar.bottom() - h * 0.15 {
                p.line_segment([pos2(bar.left() + 3.0, y), pos2(bar.right() - 3.0, y)], Stroke::new(1.0, Color32::BLACK));
                y += (h / 7.0).max(2.0);
            }
            p.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(2.0, Color32::BLACK));
            let f = fonts::font("chicago", 700, h * 0.62);
            let tw = text_width(p, "Calculator", f.clone()) + h * 0.8;
            let title = Rect::from_center_size(bar.center(), vec2(tw, h - 2.0));
            p.rect_filled(title, 0.0, Color32::WHITE);
            text(p, title.center(), egui::Align2::CENTER_CENTER, "Calculator", f, Color32::BLACK);
            let close = Rect::from_center_size(pos2(bar.left() + h * 1.2, bar.center().y), vec2(h * 0.6, h * 0.6));
            p.rect_filled(close.expand(2.0), 0.0, Color32::WHITE);
            p.rect_stroke(close, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
        }
        Family::Aqua2001 => {
            // the window title in the panel's top margin
            let m = b.width() * 0.05;
            text(p, pos2(b.center().x, b.top() + m * 0.55), egui::Align2::CENTER_CENTER, "Calculator", fonts::ui(400, (m * 0.62).clamp(8.0, 13.0)), Color32::from_gray(40));
        }
        _ => {}
    }
}
