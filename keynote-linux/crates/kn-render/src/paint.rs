//! tiny-skia helpers: paths, paints, transforms, blur.

use kn_model::{Color, Fill, Geometry, GradientKind, LineCap, LineJoin, Path, PathEl, Stroke};
use tiny_skia as sk;

pub fn sk_color(c: Color) -> sk::Color {
    sk::Color::from_rgba(c.r.clamp(0.0, 1.0), c.g.clamp(0.0, 1.0), c.b.clamp(0.0, 1.0), c.a.clamp(0.0, 1.0))
        .unwrap_or(sk::Color::BLACK)
}

pub fn sk_path(p: &Path) -> Option<sk::Path> {
    let mut b = sk::PathBuilder::new();
    let mut open = false;
    for e in &p.els {
        match *e {
            PathEl::Move(q) => {
                b.move_to(q.x, q.y);
                open = true;
            }
            PathEl::Line(q) => {
                if !open {
                    b.move_to(q.x, q.y);
                    open = true;
                } else {
                    b.line_to(q.x, q.y);
                }
            }
            PathEl::Quad(a, q) => b.quad_to(a.x, a.y, q.x, q.y),
            PathEl::Cubic(a, c, q) => b.cubic_to(a.x, a.y, c.x, c.y, q.x, q.y),
            PathEl::Close => b.close(),
        }
    }
    b.finish()
}

pub fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<sk::Path> {
    sk::Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)).map(sk::PathBuilder::from_rect)
}

/// Transform mapping a frame's local coordinates (0..w, 0..h) into its parent's space.
/// `flips` applies the geometry's horizontal/vertical flip (not wanted for text).
pub fn frame_transform(g: &Geometry, flips: bool) -> sk::Transform {
    let (cx, cy) = (g.x + g.w / 2.0, g.y + g.h / 2.0);
    let mut t = sk::Transform::from_translate(cx, cy).pre_rotate(-g.angle);
    if flips {
        t = t.pre_scale(if g.hflip { -1.0 } else { 1.0 }, if g.vflip { -1.0 } else { 1.0 });
    }
    t.pre_translate(-g.w / 2.0, -g.h / 2.0)
}

pub fn solid(c: Color) -> sk::Paint<'static> {
    let mut p = sk::Paint::default();
    p.set_color(sk_color(c));
    p.anti_alias = true;
    p
}

/// A gradient's stops with each segment's inflection (where the colour is halfway, 0.5 = even)
/// applied: the segment follows a curve through that point, sampled as extra stops.
pub fn gradient_stops(stops: &[(f32, Color)], inflections: &[f32]) -> Vec<(f32, Color)> {
    let mut out = Vec::new();
    for (i, s) in stops.iter().enumerate() {
        out.push(*s);
        let (Some(next), Some(&m)) = (stops.get(i + 1), inflections.get(i)) else { continue };
        let m = m.clamp(0.01, 0.99);
        if (m - 0.5).abs() < 1e-3 || next.0 <= s.0 {
            continue;
        }
        // u = t^k with k chosen so u(m) = 0.5
        let k = 0.5f32.ln() / m.ln();
        const N: usize = 12;
        for j in 1..N {
            let t = j as f32 / N as f32;
            let u = t.powf(k);
            let (a, b) = (s.1, next.1);
            let c = Color { r: a.r + (b.r - a.r) * u, g: a.g + (b.g - a.g) * u, b: a.b + (b.b - a.b) * u, a: a.a + (b.a - a.a) * u };
            out.push((s.0 + (next.0 - s.0) * t, c));
        }
    }
    out
}

/// Paint for a fill over a frame of size w x h (local coordinates). Image fills are handled by the caller.
pub fn fill_paint(fill: &Fill, w: f32, h: f32) -> Option<sk::Paint<'static>> {
    match fill {
        Fill::None | Fill::Image { .. } => None,
        Fill::Color(c) => (c.a > 0.0).then(|| solid(*c)),
        Fill::Gradient { kind, stops, opacity, inflections, .. } => {
            let st: Vec<sk::GradientStop> = gradient_stops(stops, inflections).into_iter().map(|(f, c)| sk::GradientStop::new(f, sk_color(c.with_alpha(*opacity)))).collect();
            if st.is_empty() {
                return None;
            }
            let shader = match kind {
                GradientKind::Linear { angle } => {
                    let a = angle.to_radians();
                    let (dx, dy) = (a.cos(), -a.sin());
                    let len = (w * dx).abs() + (h * dy).abs();
                    let (cx, cy) = (w / 2.0, h / 2.0);
                    sk::LinearGradient::new(
                        sk::Point::from_xy(cx - dx * len / 2.0, cy - dy * len / 2.0),
                        sk::Point::from_xy(cx + dx * len / 2.0, cy + dy * len / 2.0),
                        st,
                        sk::SpreadMode::Pad,
                        sk::Transform::identity(),
                    )?
                }
                GradientKind::Radial => {
                    let c = sk::Point::from_xy(w / 2.0, h / 2.0);
                    sk::RadialGradient::new(c, 0.0, c, (w.max(h) / 2.0).max(0.1), st, sk::SpreadMode::Pad, sk::Transform::identity())?
                }
            };
            let mut p = sk::Paint::default();
            p.shader = shader;
            p.anti_alias = true;
            Some(p)
        }
    }
}

pub fn sk_stroke(s: &Stroke) -> sk::Stroke {
    let mut st = sk::Stroke {
        width: s.width,
        miter_limit: s.miter_limit,
        line_cap: match s.cap {
            LineCap::Butt => sk::LineCap::Butt,
            LineCap::Round => sk::LineCap::Round,
            LineCap::Square => sk::LineCap::Square,
        },
        line_join: match s.join {
            LineJoin::Miter => sk::LineJoin::Miter,
            LineJoin::Round => sk::LineJoin::Round,
            LineJoin::Bevel => sk::LineJoin::Bevel,
        },
        dash: None,
    };
    let dashes: Vec<f32> = s.dash.iter().map(|d| (d * s.width).max(0.1)).collect();
    if dashes.len() >= 2 && dashes.len() % 2 == 0 {
        st.dash = sk::StrokeDash::new(dashes, s.phase * s.width);
    }
    st
}

/// Separable box blur (3 passes ~ gaussian) of a premultiplied pixmap's alpha, then tint.
pub fn blurred_tint(src: &sk::Pixmap, radius: f32, color: Color) -> Option<sk::Pixmap> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let mut a: Vec<f32> = src.pixels().iter().map(|p| f32::from(p.alpha()) / 255.0).collect();
    let r = (radius / 1.7).round().max(0.0) as usize;
    if r > 0 {
        let mut tmp = vec![0f32; a.len()];
        for _ in 0..3 {
            box_pass(&a, &mut tmp, w, h, r, true);
            box_pass(&tmp, &mut a, w, h, r, false);
        }
    }
    let mut out = sk::Pixmap::new(w as u32, h as u32)?;
    let c = sk_color(color);
    for (px, al) in out.pixels_mut().iter_mut().zip(a) {
        let al = (al * c.alpha()).clamp(0.0, 1.0);
        let a8 = (al * 255.0) as u8;
        let ch = |v: f32| ((v * al * 255.0) as u8).min(a8);
        *px = sk::PremultipliedColorU8::from_rgba(ch(c.red()), ch(c.green()), ch(c.blue()), a8).unwrap_or(*px);
    }
    Some(out)
}

fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (n, m) = if horizontal { (h, w) } else { (w, h) };
    let idx = |i: usize, j: usize| if horizontal { i * w + j } else { j * w + i };
    let norm = 1.0 / (2 * r + 1) as f32;
    for i in 0..n {
        let mut acc = 0f32;
        for j in 0..=r.min(m - 1) {
            acc += src[idx(i, j)];
        }
        for j in 0..m {
            dst[idx(i, j)] = acc * norm;
            let add = j + r + 1;
            if add < m {
                acc += src[idx(i, add)];
            }
            if j >= r {
                acc -= src[idx(i, j - r)];
            }
        }
    }
}

/// Decode PNG/JPEG/TIFF/GIF/WebP/BMP into a premultiplied pixmap.
/// Largest dimension kept after decoding (enough for a 2880 px wide full-screen slide).
pub const MAX_IMAGE_SIDE: u32 = 2560;

pub fn decode_image(bytes: &[u8]) -> Option<sk::Pixmap> {
    let mut dynimg = image::load_from_memory(bytes).ok()?;
    if dynimg.width().max(dynimg.height()) > MAX_IMAGE_SIDE {
        dynimg = dynimg.resize(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE, image::imageops::FilterType::Triangle);
    }
    let img = dynimg.to_rgba8();
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        px[0] = ((u16::from(px[0]) * a + 127) / 255) as u8;
        px[1] = ((u16::from(px[1]) * a + 127) / 255) as u8;
        px[2] = ((u16::from(px[2]) * a + 127) / 255) as u8;
    }
    sk::Pixmap::from_vec(data, sk::IntSize::from_wh(w, h)?)
}
