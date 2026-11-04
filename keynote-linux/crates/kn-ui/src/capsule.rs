//! The suite's toolbar buttons (Bramble Notes' toolbar, measured on Apple Notes for macOS 26):
//! a button holding one symbol is a circle 36.5 pt across with its symbol centred; several
//! side by side share a capsule 36.5 pt tall with 4.5 pt at its ends. On the white page (light
//! mode, Glass on or off) the capsules get an angle-dependent rim, a white line inside the top
//! and bottom, a light-fill gradient, a faint shade inside the round ends and a shadow only
//! below, so they read on white; dark mode keeps each app's own capsules (`dark`).
//!
//! A lone circle's hover and pressed highlight fills the whole circle just inside its rim
//! (`solo_highlight`); an item inside a group keeps its inset rounded cell.

use egui::{pos2, vec2, Color32, Pos2, Rect, Stroke};

use crate::{glass, theme};

/// Apple's toolbar button height (and a lone button's diameter).
pub const HEIGHT: f32 = 36.5;
/// Room inside a capsule's ends.
pub const PAD: f32 = 4.5;
/// What a button on its own takes inside its capsule's ends: with them, a circle `HEIGHT` across.
pub const SOLO_W: f32 = HEIGHT - 2.0 * PAD;

/// How much darker than Apple's (as measured) the light rim is drawn: on the Linux laptop panels
/// the suite runs on, Apple's own values read too faint against white (the user's call).
pub const RIM_BOOST: f32 = 1.6;

/// The capsules behind the toolbar's buttons: liquid glass when on (light: with the under-shadow
/// and rim, as on white it all but vanishes), the classic look's flat strip, `dark` (the app's
/// own solid dark capsule) in dark mode, and the light capsules on white.
pub fn shapes(ctx: &egui::Context, caps: &[Rect], dark: &dyn Fn(Rect) -> Vec<egui::Shape>) -> Vec<egui::Shape> {
    let light = theme::is_light();
    let px = 1.0 / ctx.pixels_per_point();
    if glass::enabled() {
        if !light {
            return caps.iter().flat_map(|c| glass::shapes(ctx, *c, c.height() / 2.0, true)).collect();
        }
        let mut out = Vec::new();
        for c in caps {
            let c = snap_px(*c, px);
            let r = c.height() / 2.0;
            out.push(under_shadow(c, r));
            out.extend(glass::shapes(ctx, c, r, true));
            out.push(egui::Shape::Mesh(edges(c, px).into()));
        }
        out
    } else if glass::classic() {
        caps.iter().map(|c| egui::Shape::rect_filled(*c, c.height() / 2.0, theme::gray(0x36))).collect()
    } else if !light {
        caps.iter().flat_map(|c| dark(*c)).collect()
    } else {
        // on the white page (Apple's, measured on ui-list-view-light at 2x): an almost white fill
        // a touch darker in its upper half and near its ends, a white line inside the top and
        // bottom, a one-pixel rim far darker on the round ends than along the top (glass lit
        // from above), and a faint shadow only under it
        let mut out = Vec::new();
        for c in caps {
            let c = snap_px(*c, px);
            let r = c.height() / 2.0;
            out.push(under_shadow(c, r));
            out.push(egui::Shape::Mesh(gradient_fill(c, px).into()));
            out.push(egui::Shape::Mesh(edges(c, px).into()));
        }
        out
    }
}

/// Numbers' dark capsules (Mac N2), Letters', Values' and Notes' in dark mode: solid `fill`, a
/// light rim brighter at the top and bottom.
pub fn dark_rimmed(c: Rect, fill: Color32) -> Vec<egui::Shape> {
    let r = c.height() / 2.0;
    let mut out = vec![egui::Shape::rect_filled(c, r, fill), egui::Shape::rect_stroke(c, r, Stroke::new(1.0, Color32::from_white_alpha(34)), egui::StrokeKind::Inside)];
    for y in [c.min.y + 0.5, c.max.y - 0.5] {
        out.push(egui::Shape::line_segment([pos2(c.min.x + r, y), pos2(c.max.x - r, y)], Stroke::new(1.0, Color32::from_white_alpha(22))));
    }
    out
}

/// The circle a lone button sits in, `HEIGHT` across, centred on `at`.
pub fn solo_rect(at: Pos2) -> Rect {
    Rect::from_center_size(at, vec2(HEIGHT, HEIGHT))
}

/// A lone circle button's hover or pressed highlight (Mac): the whole circle, edge to edge, one
/// pixel inside its rim.
pub fn solo_highlight(painter: &egui::Painter, at: Pos2, fill: Color32) {
    let px = 1.0 / painter.ctx().pixels_per_point();
    painter.circle_filled(at, HEIGHT / 2.0 - px, fill);
}

/// A capsule filled top to bottom with Apple's light fill shape, a touch greyer so it reads on
/// white on Linux panels (#F8F8F8 at the top, #F5F5F5 a third of the way down, #F7F7F7 from the
/// middle), in one-pixel strips cut to its round ends.
pub fn gradient_fill(c: Rect, px: f32) -> egui::Mesh {
    let shade = |t: f32| {
        let (a, b, k) = if t < 0.3 { (248.0, 245.0, t / 0.3) } else if t < 0.5 { (245.0, 247.0, (t - 0.3) / 0.2) } else { (247.0, 247.0, 0.0) };
        theme::tint(Color32::from_gray((a + (b - a) * k) as u8))
    };
    let r = c.height() / 2.0;
    let half = |y: f32| {
        let d = (y - c.center().y).abs().min(r);
        c.width() / 2.0 - r + (r * r - d * d).sqrt()
    };
    let mut m = egui::Mesh::default();
    let n = (c.height() / px).ceil() as u32;
    for k in 0..=n {
        let y = (c.min.y + k as f32 * px).min(c.max.y);
        let (w, col) = (half(y), shade((y - c.min.y) / c.height()));
        m.colored_vertex(pos2(c.center().x - w, y), col);
        m.colored_vertex(pos2(c.center().x + w, y), col);
        if k > 0 {
            let a = 2 * (k - 1);
            m.add_triangle(a, a + 1, a + 2);
            m.add_triangle(a + 1, a + 3, a + 2);
        }
    }
    m
}

/// A capsule on whole device pixels, so its rim is one sharp pixel as Apple's (36.5 pt is 73 px).
pub fn snap_px(c: Rect, px: f32) -> Rect {
    let snap = |v: f32| (v / px).round() * px;
    Rect::from_min_max(pos2(snap(c.min.x), snap(c.min.y)), pos2(snap(c.max.x), snap(c.max.y)))
}

/// The light capsules' shadow, only under them: cast by the capsule 4 pt smaller all round, so
/// none shows beside it.
pub fn under_shadow(c: Rect, r: f32) -> egui::Shape {
    let shadow = egui::epaint::Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(15) };
    shadow.as_shape(c.shrink(4.0), r - 4.0).into()
}

/// A light capsule's edges, by the angle of the edge (Apple's, measured on ui-list-view-light):
/// the rim, one pixel just outside, black at 25% on the round ends falling to 7.5% along the top
/// and bottom (0.075 + 0.175 |nx|^3.5, times `RIM_BOOST`); a white line just inside the top and
/// bottom; and a faint shade some 15 pt deep inside the ends.
pub fn edges(c: Rect, px: f32) -> egui::Mesh {
    let r = c.height() / 2.0;
    // the outline, round end to round end, with each point's outward normal
    let mut pts: Vec<(Pos2, egui::Vec2)> = Vec::new();
    let steps = 24;
    for (cx, from) in [(c.max.x - r, -90.0_f32), (c.min.x + r, 90.0)] {
        for k in 0..=steps {
            let a = (from + 180.0 * k as f32 / steps as f32).to_radians();
            let n = vec2(a.cos(), a.sin());
            pts.push((pos2(cx, c.center().y) + n * r, n));
        }
    }
    let mut m = egui::Mesh::default();
    // a band: from `a` px along the normal (transparent) through `b` (the colour) to `e`
    // (transparent); its colour by the normal
    let mut band = |a: f32, b: f32, e: f32, col: &dyn Fn(egui::Vec2) -> Color32| {
        let base = m.vertices.len() as u32;
        for (q, n) in &pts {
            m.colored_vertex(*q + *n * a * px, Color32::TRANSPARENT);
            m.colored_vertex(*q + *n * b * px, col(*n));
            m.colored_vertex(*q + *n * e * px, Color32::TRANSPARENT);
        }
        let len = pts.len() as u32;
        for k in 0..len {
            let (i, j) = (base + 3 * k, base + 3 * ((k + 1) % len));
            for d in 0..2 {
                m.add_triangle(i + d, i + d + 1, j + d);
                m.add_triangle(i + d + 1, j + d + 1, j + d);
            }
        }
    };
    band(-30.0, -0.5, -0.5, &|n| theme::tint(Color32::from_black_alpha((5.0 * n.x.abs().powi(2)) as u8)));
    band(0.0, -1.0, -3.0, &|n| Color32::from_white_alpha((255.0 * n.y.abs().powi(3)) as u8));
    band(-0.5, 0.5, 1.5, &|n| Color32::from_black_alpha(((0.075 + 0.175 * n.x.abs().powf(3.5)) * RIM_BOOST * 255.0) as u8));
    m
}
