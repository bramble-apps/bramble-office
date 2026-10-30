//! Slide transitions, drawn on the GPU as textured meshes between two slide textures.
//! Effect ids are Keynote's stored ids (see data/transitions.txt).

use std::f32::consts::{PI, TAU};

use egui::{pos2, Color32, Mesh, Painter, Pos2, Rect, TextureId};

thread_local! {
    /// Slide textures with a picture drawn under them wherever they are drawn (a dynamic
    /// background under a slide drawn without one).
    static UNDER: std::cell::RefCell<Vec<(TextureId, TextureId)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Run `f` with `under` (slide texture, background texture) pairs in effect.
pub fn with_underlays<R>(under: &[(TextureId, TextureId)], f: impl FnOnce() -> R) -> R {
    UNDER.with(|u| *u.borrow_mut() = under.to_vec());
    let r = f();
    UNDER.with(|u| u.borrow_mut().clear());
    r
}

/// Add a textured mesh, first with the texture's underlay (same geometry), if it has one.
fn add(p: &Painter, m: Mesh) {
    if let Some(bg) = UNDER.with(|u| u.borrow().iter().find(|(t, _)| *t == m.texture_id).map(|(_, b)| *b)) {
        let mut under = m.clone();
        under.texture_id = bg;
        p.add(under);
    }
    p.add(m);
}

fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn quad(p: &Painter, tex: TextureId, dst: Rect, uv: Rect, alpha: f32) {
    let mut m = Mesh::with_texture(tex);
    m.add_rect_with_uv(dst, uv, Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8));
    add(p, m);
}

fn full(p: &Painter, tex: TextureId, r: Rect, alpha: f32) {
    quad(p, tex, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), alpha);
}

/// Texture clipped to a convex polygon given in unit (0..1) slide coordinates.
fn polygon(p: &Painter, tex: TextureId, r: Rect, pts: &[Pos2], alpha: f32) {
    if pts.len() < 3 {
        return;
    }
    let mut m = Mesh::with_texture(tex);
    let c = Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8);
    for q in pts {
        m.vertices.push(egui::epaint::Vertex { pos: pos2(r.min.x + q.x * r.width(), r.min.y + q.y * r.height()), uv: *q, color: c });
    }
    for i in 1..pts.len() as u32 - 1 {
        m.add_triangle(0, i, i + 1);
    }
    add(p, m);
}

/// A slide-sized plane transformed in 3D: rotation (radians) about the Y axis through `pivot_x`
/// (unit coords), plus a Z offset; projected with a camera at distance `cam` slide-widths.
#[allow(clippy::too_many_arguments)]
fn plane3d(p: &Painter, tex: TextureId, r: Rect, rot_y: f32, rot_x: f32, pivot: (f32, f32, f32), z: f32, alpha: f32, cam: f32) {
    const N: usize = 12;
    let w = r.width();
    let h = r.height();
    let cam = cam * w;
    let (sy, cy) = rot_y.sin_cos();
    let (sx, cx) = rot_x.sin_cos();
    let project = |u: f32, v: f32| -> Option<Pos2> {
        // local point relative to pivot (in pixels)
        let mut x = (u - pivot.0) * w;
        let mut y = (v - pivot.1) * h;
        let mut zz = -pivot.2 * w;
        // rotate about X, then Y
        let (y1, z1) = (y * cx - zz * sx, y * sx + zz * cx);
        y = y1;
        zz = z1;
        let (x1, z2) = (x * cy + zz * sy, -x * sy + zz * cy);
        x = x1;
        zz = z2;
        let px = x + (pivot.0 - 0.5) * w;
        let py = y + (pivot.1 - 0.5) * h;
        let pz = zz + pivot.2 * w + z * w;
        let d = cam / (cam + pz);
        (d > 0.0).then(|| pos2(r.center().x + px * d, r.center().y + py * d))
    };
    let mut m = Mesh::with_texture(tex);
    let c = Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8);
    for j in 0..=N {
        for i in 0..=N {
            let (u, v) = (i as f32 / N as f32, j as f32 / N as f32);
            let Some(pos) = project(u, v) else { return };
            m.vertices.push(egui::epaint::Vertex { pos, uv: pos2(u, v), color: c });
        }
    }
    for j in 0..N as u32 {
        for i in 0..N as u32 {
            let a = j * (N as u32 + 1) + i;
            let b = a + 1;
            let c2 = a + N as u32 + 1;
            let d = c2 + 1;
            m.add_triangle(a, b, d);
            m.add_triangle(a, d, c2);
        }
    }
    add(p, m);
}

/// Effect name without its "apple:" / "com.apple.iWork.Keynote." prefix.
pub fn short_name(effect: &str) -> &str {
    effect.rsplit([':', '.']).next().unwrap_or(effect)
}

/// Transitions that animate the slides' objects individually (need [`ObjSet`]s).
pub fn needs_objects(effect: &str) -> bool {
    matches!(
        short_name(effect),
        "magic-move-implied-motion-path" | "magic-move" | "ca-text-shimmer" | "ca-text-sparkle" | "ca-zoom" | "ca-push" | "ca-pop" | "ca-revolve" | "ca-cube" | "ca-dissolve-and-flip" | "ca-swing"
    )
}

/// Draw the transition at progress `t` (0..1) from `from` to `to` inside `r`.
/// `dir` is Keynote's stored direction (11 left to right, 12 right to left, 13 top to bottom,
/// 14 bottom to top; 0 = the effect's default).
#[allow(clippy::too_many_arguments)]
pub fn draw(p: &Painter, r: Rect, from: TextureId, to: TextureId, effect: &str, t: f32, color: Color32, dir: u32) {
    let e = ease(t.clamp(0.0, 1.0));
    let w = r.width();
    let h = r.height();
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let name = short_name(effect);
    // direction the content travels
    let v = crate::builds::dir_vec(dir, egui::vec2(-1.0, 0.0));
    let span = egui::vec2(v.x * w, v.y * h);
    match name {
        "push" | "ca-push" | "ClotheslinePush" => {
            full(p, from, r.translate(span * e), 1.0);
            full(p, to, r.translate(-span * (1.0 - e)), 1.0);
        }
        "slide" => {
            // move in: the new slide slides over the old one
            full(p, from, r, 1.0);
            full(p, to, r.translate(-span * (1.0 - e)), 1.0);
        }
        "reveal" => {
            // the old slide gathers speed as it leaves (R43 movie: slow start, off at the end)
            let k = ((t - 0.1) / 0.85).clamp(0.0, 1.0).powf(1.9);
            full(p, to, r, 1.0);
            full(p, from, r.translate(span * k), 1.0);
        }
        "wipe" => {
            full(p, from, r, 1.0);
            // wipes default to left to right
            let v = crate::builds::dir_vec(dir, egui::vec2(1.0, 0.0));
            let x = e;
            let (dst, src) = if v.x < -0.5 {
                (Rect::from_min_max(pos2(r.max.x - w * x, r.min.y), r.max), Rect::from_min_max(pos2(1.0 - x, 0.0), pos2(1.0, 1.0)))
            } else if v.y > 0.5 {
                (Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + h * x)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, x)))
            } else if v.y < -0.5 {
                (Rect::from_min_max(pos2(r.min.x, r.max.y - h * x), r.max), Rect::from_min_max(pos2(0.0, 1.0 - x), pos2(1.0, 1.0)))
            } else {
                (Rect::from_min_max(r.min, pos2(r.min.x + w * x, r.max.y)), Rect::from_min_max(pos2(0.0, 0.0), pos2(x, 1.0)))
            };
            quad(p, to, dst, src, 1.0);
        }
        "wipe-iris" => {
            full(p, from, r, 1.0);
            let rad = e * 0.75;
            let aspect = w / h;
            let pts: Vec<Pos2> = (0..64)
                .map(|i| {
                    let a = TAU * i as f32 / 64.0;
                    pos2((0.5 + rad * a.cos()).clamp(0.0, 1.0), (0.5 + rad * aspect * a.sin()).clamp(0.0, 1.0))
                })
                .collect();
            polygon(p, to, r, &pts, 1.0);
        }
        "radial wipe" => {
            full(p, from, r, 1.0);
            let sweep = TAU * e;
            let mut pts = vec![pos2(0.5, 0.5)];
            let steps = (64.0 * e).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let a = -PI / 2.0 + sweep * i as f32 / steps as f32;
                // project the ray onto the unit square
                let (dx, dy) = (a.cos(), a.sin());
                let k = 0.5 / dx.abs().max(dy.abs()).max(1e-4);
                pts.push(pos2(0.5 + dx * k, 0.5 + dy * k));
            }
            // fan triangles from the centre (polygon() fans from point 0)
            polygon(p, to, r, &pts, 1.0);
        }
        "BLTFadeThruColor" => {
            p.rect_filled(r, 0.0, color);
            if t < 0.5 {
                full(p, from, r, 1.0 - ease(t * 2.0));
            } else {
                full(p, to, r, ease(t * 2.0 - 1.0));
            }
        }
        "scale" | "ca-zoom" => {
            full(p, from, Rect::from_center_size(r.center(), r.size() * (1.0 - 0.3 * e)), 1.0 - e);
            full(p, to, Rect::from_center_size(r.center(), r.size() * (1.3 - 0.3 * e)), e);
        }
        "3D-cube" | "ca-cube" => {
            // rotating cube: old face turns away to the left, new face comes from the right,
            // mostly turned by ~0.55 then a small overshoot settling (R43 movie)
            let mut k = ease(((t - 0.1) / 0.45).clamp(0.0, 1.0));
            if t > 0.5 && t < 0.85 {
                k += 0.06 * (PI * (t - 0.5) / 0.35).sin();
            }
            let a = k * PI / 2.0;
            p.rect_filled(r, 0.0, Color32::BLACK);
            // the nearer face last
            let faces = [(from, a), (to, a - PI / 2.0)];
            let order = if a < PI / 4.0 { [1, 0] } else { [0, 1] };
            // (the cube backs away a little while it turns)
            let back = 0.15 * (k.clamp(0.0, 1.0) * PI).sin();
            for i in order {
                plane3d(p, faces[i].0, r, faces[i].1, 0.0, (0.5, 0.5, 0.5), back, 1.0, 2.0);
            }
        }
        "revolve" | "ca-dissolve-and-flip" | "BUKFlop" | "FlipThrough" => {
            // edge-on at ~0.4, flat by ~0.8 (R43 movie)
            let k = ease(((t - 0.05) / 0.75).clamp(0.0, 1.0));
            p.rect_filled(r, 0.0, Color32::BLACK);
            if k < 0.5 {
                plane3d(p, from, r, -k * PI, 0.0, (0.5, 0.5, 0.0), 0.0, 1.0, 2.0);
            } else {
                plane3d(p, to, r, PI - k * PI, 0.0, (0.5, 0.5, 0.0), 0.0, 1.0, 2.0);
            }
        }
        "doorway" => {
            plane3d(p, to, r, 0.0, 0.0, (0.5, 0.5, 0.0), 0.4 * (1.0 - e), e.min(1.0), 2.0);
            let a = e * PI / 2.0 * 1.1;
            // two doors swing open
            let left = Rect::from_min_max(r.min, pos2(r.center().x, r.max.y));
            let right = Rect::from_min_max(pos2(r.center().x, r.min.y), r.max);
            let _ = (left, right);
            plane3d_part(p, from, r, (0.0, 0.5), a, 0.0, 1.0 - e * 0.5);
            plane3d_part(p, from, r, (0.5, 1.0), -a, 1.0, 1.0 - e * 0.5);
        }
        "fall" => {
            full(p, to, r, 1.0);
            plane3d(p, from, r, 0.0, -e * PI / 2.0, (0.5, 1.0, 0.0), 0.0, 1.0 - e * 0.3, 2.0);
        }
        "pivot" => {
            full(p, to, r, 1.0);
            let a = e * PI / 2.0;
            let mut m = Mesh::with_texture(from);
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            let (s, c) = a.sin_cos();
            for (u, v) in corners {
                let (x, y) = (u * w, v * h);
                let pos = pos2(r.min.x + x * c - y * s, r.min.y + x * s + y * c);
                m.vertices.push(egui::epaint::Vertex { pos, uv: pos2(u, v), color: Color32::WHITE });
            }
            m.add_triangle(0, 1, 2);
            m.add_triangle(0, 2, 3);
            add(p, m);
        }
        "bounce" => {
            // drop: new slide falls from above with a small bounce
            full(p, from, r, 1.0);
            let y = if t < 0.7 { (1.0 - (t / 0.7).powi(2)) * -h } else { -((t - 0.7) / 0.3 * PI).sin() * h * 0.05 };
            full(p, to, r.translate(egui::vec2(0.0, y)), 1.0);
        }
        "KLNSwap" => {
            // the slides trade places on a circle (R43 movie): the old one swings back to the
            // left, the new one comes from behind on the right to the front, each turned a little
            let k = ease(((t - 0.05) / 0.85).clamp(0.0, 1.0));
            p.rect_filled(r, 0.0, Color32::BLACK);
            let orbit = 0.75;
            let ph = k * PI;
            let old = (from, -orbit * ph.sin(), orbit * (1.0 - ph.cos()), -0.6 * ph.sin());
            let new = (to, orbit * ph.sin(), orbit * (1.0 + ph.cos()), 0.6 * ph.sin());
            // the farther card first
            let cards = if old.2 > new.2 { [old, new] } else { [new, old] };
            for (tex, x, z, rot) in cards {
                plane3d(p, tex, r.translate(egui::vec2(x * w * 0.6, 0.0)), rot, 0.0, (0.5, 0.5, 0.0), z, 1.0, 2.0);
            }
        }
        "pageflip" => {
            // the page folds over from its bottom-right corner (R43 movie: an even sweep from
            // ~0.37 to ~0.92), its back a pale mirror of the slide, the new slide beneath
            let k = ((t - 0.37) / 0.55).clamp(0.0, 1.0);
            full(p, to, r, 1.0);
            let n = egui::vec2(1.0, 0.4).normalized();
            let corners = [r.min, pos2(r.max.x, r.min.y), r.max, pos2(r.min.x, r.max.y)];
            let proj: Vec<f32> = corners.iter().map(|c| c.to_vec2().dot(n)).collect();
            let (lo, hi) = (proj.iter().cloned().fold(f32::INFINITY, f32::min), proj.iter().cloned().fold(f32::NEG_INFINITY, f32::max));
            let line = hi - k * (hi - lo) * 1.02;
            let uv_of = |q: Pos2| pos2((q.x - r.min.x) / w, (q.y - r.min.y) / h);
            let kept = clip_half(&corners, n, line, false);
            let flap = clip_half(&corners, n, line, true);
            let mut m = Mesh::with_texture(from);
            fan(&mut m, &kept, |q| (q, uv_of(q)), Color32::WHITE);
            add(p, m);
            if flap.len() >= 3 {
                let mirror = |q: Pos2| q - n * (2.0 * (q.to_vec2().dot(n) - line));
                let mut back = Mesh::default();
                fan(&mut back, &flap, |q| (mirror(q), egui::epaint::WHITE_UV), Color32::from_gray(238));
                add(p, back);
                let mut m = Mesh::with_texture(from);
                fan(&mut m, &flap, |q| (mirror(q), uv_of(q)), Color32::from_white_alpha(60));
                add(p, m);
            }
        }
        "BUKTwist" => {
            // a ribbon twist (R43 movie): thin vertical strips turn about the slide's horizontal
            // centre line, the left side leading, the new slide on their backs
            let k = ease(((t - 0.03) / 0.8).clamp(0.0, 1.0));
            p.rect_filled(r, 0.0, Color32::BLACK);
            const N: usize = 96;
            let cam = 2.0 * w;
            let turn = |u: f32| PI * (k * 1.6 - 0.6 * u).clamp(0.0, 1.0);
            for i in 0..N {
                let (u0, u1) = (i as f32 / N as f32, (i + 1) as f32 / N as f32);
                // each edge at its own angle so neighbouring strips meet; the strip's middle
                // says which side faces the viewer
                let back = turn((u0 + u1) * 0.5) >= PI / 2.0;
                let tex = if back { to } else { from };
                let at = |u: f32, v: f32| {
                    let ang = if back { turn(u) - PI } else { turn(u) };
                    let (s, c) = ang.sin_cos();
                    let y = (v - 0.5) * h;
                    let d = cam / (cam + y * s);
                    (pos2(r.center().x + (u - 0.5) * w * d, r.center().y + y * c * d), c)
                };
                let mut m = Mesh::with_texture(tex);
                for (u, v) in [(u0, 0.0), (u1, 0.0), (u1, 1.0), (u0, 1.0)] {
                    let (pos, c) = at(u, v);
                    m.vertices.push(egui::epaint::Vertex { pos, uv: pos2(u, v), color: Color32::from_gray((255.0 * (0.75 + 0.25 * c.abs())) as u8) });
                }
                m.add_triangle(0, 1, 2);
                m.add_triangle(0, 2, 3);
                add(p, m);
            }
        }
        "ca-swing" | "BLTRevolvingDoor" | "twirl" | "BLTSwoosh" => {
            // approximations: old slide turns away about its left edge, new one fades in
            full(p, to, r, e);
            plane3d(p, from, r, e * PI / 2.0, 0.0, (0.0, 0.5, 0.0), 0.0, 1.0 - e, 2.0);
        }
        "fade-and-move" => {
            full(p, from, r.translate(span * 0.1 * e), 1.0 - e);
            full(p, to, r.translate(-span * 0.1 * (1.0 - e)), e);
        }
        "BLTBlinds" => {
            full(p, from, r, 1.0);
            let n = 8;
            for i in 0..n {
                let x0 = i as f32 / n as f32;
                let x1 = x0 + e / n as f32;
                quad(p, to, Rect::from_min_max(pos2(r.min.x + x0 * w, r.min.y), pos2(r.min.x + x1 * w, r.max.y)), Rect::from_min_max(pos2(x0, 0.0), pos2(x1, 1.0)), 1.0);
            }
        }
        "apple-grid" | "BLTMosaicFlip" => {
            full(p, from, r, 1.0);
            let (nx, ny) = (8, 5);
            for j in 0..ny {
                for i in 0..nx {
                    // tiles appear in a diagonal sweep
                    let k = (i + j) as f32 / (nx + ny) as f32;
                    let a = ((t - k * 0.6) / 0.4).clamp(0.0, 1.0);
                    let (u0, v0) = (i as f32 / nx as f32, j as f32 / ny as f32);
                    let (u1, v1) = (u0 + 1.0 / nx as f32, v0 + 1.0 / ny as f32);
                    quad(p, to, Rect::from_min_max(pos2(r.min.x + u0 * w, r.min.y + v0 * h), pos2(r.min.x + u1 * w, r.min.y + v1 * h)), Rect::from_min_max(pos2(u0, v0), pos2(u1, v1)), a);
                }
            }
        }
        _ => {
            // dissolve (and fallback for effects not modelled yet)
            full(p, from, r, 1.0);
            full(p, to, r, e);
        }
    }
    let _ = uv;
}

/// The part of a convex polygon on one side of the line `q·n = at` (`beyond`: where q·n > at).
fn clip_half(pts: &[Pos2], n: egui::Vec2, at: f32, beyond: bool) -> Vec<Pos2> {
    let f = |q: Pos2| (q.to_vec2().dot(n) - at) * if beyond { 1.0 } else { -1.0 };
    let mut out = Vec::new();
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let (fa, fb) = (f(a), f(b));
        if fa >= 0.0 {
            out.push(a);
        }
        if (fa >= 0.0) != (fb >= 0.0) {
            out.push(a + (b - a) * (fa / (fa - fb)));
        }
    }
    out
}

/// A convex polygon as a triangle fan, each point placed and textured by `at` -> (pos, uv).
fn fan(m: &mut Mesh, pts: &[Pos2], at: impl Fn(Pos2) -> (Pos2, Pos2), color: Color32) {
    if pts.len() < 3 {
        return;
    }
    let base = m.vertices.len() as u32;
    for q in pts {
        let (pos, uv) = at(*q);
        m.vertices.push(egui::epaint::Vertex { pos, uv, color });
    }
    for i in 1..pts.len() as u32 - 1 {
        m.add_triangle(base, base + i, base + i + 1);
    }
}

/// Half-slide door panel rotated about its outer edge.
fn plane3d_part(p: &Painter, tex: TextureId, r: Rect, u_range: (f32, f32), rot_y: f32, pivot_u: f32, alpha: f32) {
    const N: usize = 8;
    let w = r.width();
    let h = r.height();
    let cam = 2.0 * w;
    let (s, c) = rot_y.sin_cos();
    let mut m = Mesh::with_texture(tex);
    let col = Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8);
    for j in 0..=N {
        for i in 0..=N {
            let u = u_range.0 + (u_range.1 - u_range.0) * i as f32 / N as f32;
            let v = j as f32 / N as f32;
            let x = (u - pivot_u) * w;
            let (xr, z) = (x * c, -x * s);
            let d = cam / (cam + z);
            let px = r.min.x + pivot_u * w + xr;
            let pos = pos2(r.center().x + (px - r.center().x) * d, r.center().y + (v - 0.5) * h * d);
            m.vertices.push(egui::epaint::Vertex { pos, uv: pos2(u, v), color: col });
        }
    }
    for j in 0..N as u32 {
        for i in 0..N as u32 {
            let a = j * (N as u32 + 1) + i;
            m.add_triangle(a, a + 1, a + N as u32 + 2);
            m.add_triangle(a, a + N as u32 + 2, a + N as u32 + 1);
        }
    }
    add(p, m);
}

// ---------------------------------------------------------------------------------- object transitions

use crate::builds::{self, Layer, Look, Map};
use egui::{vec2, TextureHandle, Vec2};
use kn_model::{Document, Drawable, DrawableKind, PlaceholderKind, SlideScene};
use kn_render::Renderer;

/// One slide object rendered into its own layer, with what Magic Move matches it by.
pub struct Obj {
    pub layer: Layer,
    pub key: String,
    pub placeholder: Option<PlaceholderKind>,
    pub alpha: f32,
    pub is_text: bool,
}

/// A slide split into its background (with layout objects) and per-object layers.
pub struct ObjSet {
    pub base: TextureHandle,
    pub objs: Vec<Obj>,
}

fn match_key(d: &Drawable) -> String {
    match &d.kind {
        // the same picture matches whatever its mask (Keynote morphs the crop)
        DrawableKind::Image(i) => format!("img:{:?}", i.data_id.or(i.thumb_id)),
        DrawableKind::Movie(m) => format!("mov:{:?}", m.movie_data.or(m.poster_data)),
        DrawableKind::Shape(s) => match &s.text {
            Some(t) if !t.text.trim().is_empty() => format!("txt:{}", t.text),
            _ => {
                let b = s.path.bounds().map(|(a, b)| ((b.x - a.x) / d.geom.w.max(1.0) * 100.0).round() as i32 * 1000 + ((b.y - a.y) / d.geom.h.max(1.0) * 100.0).round() as i32);
                format!("shape:{}:{:?}:{:?}:{:?}", s.is_line, b, s.fill, s.stroke.as_ref().map(|s| (s.color, s.width)))
            }
        },
        DrawableKind::Group(ch) => format!("group:{}:{}", ch.len(), ch.first().map(match_key).unwrap_or_default()),
        DrawableKind::Table(t) => format!("table:{}", t.cells.join("|")),
        DrawableKind::Chart(c) => format!("chart:{}:{:?}", c.chart_type, c.values),
        DrawableKind::Unsupported(n) => format!("other:{}:{n}", d.id),
    }
}

impl ObjSet {
    pub fn prepare(ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, st: &builds::State, width: u32) -> ObjSet {
        let shown = st.scene_for(scene);
        let scene = shown.as_ref().unwrap_or(scene);
        // a dynamic background is drawn live under the base
        let base = builds::state_texture(ctx, r, doc, scene, st, width, &|d: &Drawable| d.from_template, scene.motion.is_none(), "play-base");
        let k = width as f32 / scene.size.w;
        let mut objs = Vec::new();
        for d in scene.drawables.iter().filter(|d| !d.from_template && !st.hidden.contains(&d.id)) {
            let x = st.xform(d.id);
            if let Some(layer) = builds::render_layer(ctx, r, doc, d, &x, k, false) {
                let is_text = matches!(&d.kind, DrawableKind::Shape(s) if s.text.as_ref().is_some_and(|t| !t.text.trim().is_empty()));
                objs.push(Obj { layer, key: match_key(d), placeholder: d.placeholder, alpha: x.opacity.unwrap_or(d.opacity), is_text });
            }
        }
        ObjSet { base, objs }
    }
}

/// Pairs (from index, to index) of objects Magic Move morphs into each other.
pub fn matches(a: &ObjSet, b: &ObjSet) -> Vec<(usize, usize)> {
    let mut used = vec![false; a.objs.len()];
    let mut out = Vec::new();
    let mut taken = vec![false; b.objs.len()];
    for (j, o) in b.objs.iter().enumerate() {
        if let Some(i) = (0..a.objs.len()).find(|i| !used[*i] && a.objs[*i].key == o.key) {
            used[i] = true;
            taken[j] = true;
            out.push((i, j));
        }
    }
    // same kind of placeholder (title, body) with different text: morph and cross-fade
    for (j, o) in b.objs.iter().enumerate() {
        if taken[j] || !matches!(o.placeholder, Some(PlaceholderKind::Title | PlaceholderKind::Body)) {
            continue;
        }
        if let Some(i) = (0..a.objs.len()).find(|i| !used[*i] && a.objs[*i].placeholder == o.placeholder) {
            used[i] = true;
            out.push((i, j));
        }
    }
    out
}

fn base_quad(p: &Painter, tex: TextureId, r: Rect, alpha: f32) {
    full(p, tex, r, alpha);
}

fn obj_look(alpha: f32) -> Look {
    Look::new(alpha)
}

/// Pseudo-random 0..1 from an integer.
fn hash01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Small four-pointed star.
fn sparkle(p: &Painter, c: Pos2, size: f32, alpha: f32) {
    if alpha <= 0.01 {
        return;
    }
    let col = Color32::from_rgba_unmultiplied(255, 252, 230, (alpha.clamp(0.0, 1.0) * 255.0) as u8);
    let st = egui::Stroke::new((size * 0.18).max(1.0), col);
    p.line_segment([c - vec2(size, 0.0), c + vec2(size, 0.0)], st);
    p.line_segment([c - vec2(0.0, size), c + vec2(0.0, size)], st);
    p.circle_filled(c, size * 0.3, col);
}

/// Object-wise transitions (Magic Move, Shimmer, Sparkle) between two prepared slides.
#[allow(clippy::too_many_arguments)]
pub fn draw_objects(p: &Painter, r: Rect, slide_w: f32, effect: &str, t: f32, a: &ObjSet, b: &ObjSet, pairs: &[(usize, usize)], dir: u32) {
    let map = Map::new(r, slide_w);
    let t = t.clamp(0.0, 1.0);
    let e = builds::smooth(t);
    let name = short_name(effect);
    if matches!(name, "ca-zoom" | "ca-push" | "ca-pop" | "ca-revolve" | "ca-cube" | "ca-dissolve-and-flip" | "ca-swing") {
        object_effect(p, r, &map, name, t, a, b, dir);
        return;
    }
    match name {
        "ca-text-shimmer" => {
            // old objects shimmer away (strips drifting apart and fading), new ones shimmer in
            base_quad(p, a.base.id(), r, 1.0);
            base_quad(p, b.base.id(), r, e);
            let out = builds::smooth(t / 0.6);
            let inn = builds::smooth((t - 0.4) / 0.6);
            for (n, o) in a.objs.iter().enumerate() {
                shimmer(p, &map, &o.layer, o.alpha * (1.0 - out), out, n as u32);
            }
            for (n, o) in b.objs.iter().enumerate() {
                shimmer(p, &map, &o.layer, o.alpha * inn, 1.0 - inn, 100 + n as u32);
            }
        }
        "ca-text-sparkle" => {
            base_quad(p, a.base.id(), r, 1.0);
            base_quad(p, b.base.id(), r, e);
            let out = builds::smooth(t / 0.45);
            for o in &a.objs {
                builds::draw_layer(p, &map, &o.layer, &obj_look(o.alpha * (1.0 - out)));
            }
            for (n, o) in b.objs.iter().enumerate() {
                // each object is revealed left to right with sparkles at the front
                let k = ((t - 0.2 - 0.1 * (n as f32 / b.objs.len().max(1) as f32)) / 0.7).clamp(0.0, 1.0);
                let mut lk = obj_look(o.alpha);
                lk.crop = builds::Crop::Wipe { dir: vec2(1.0, 0.0), p: k };
                builds::draw_layer(p, &map, &o.layer, &lk);
                if k > 0.0 && k < 1.0 {
                    let rr = o.layer.rect;
                    let x = rr.min.x + (rr.width() * 1.3) * k - rr.width() * 0.15;
                    let count = ((rr.height() / 12.0) as u32).clamp(4, 24);
                    for s in 0..count {
                        let seed = n as u32 * 977 + s * 31 + (t * 30.0) as u32;
                        let y = rr.min.y + rr.height() * hash01(s * 7 + n as u32 * 13);
                        let jx = (hash01(seed) - 0.5) * rr.height().min(80.0) * 0.6;
                        let local = vec2(x + jx - o.layer.center.x, y - o.layer.center.y);
                        let pos = map.pt(builds::place(o.layer.center, o.layer.angle, &obj_look(1.0), local));
                        let size = (3.0 + 7.0 * hash01(seed + 5)) * map.k * 1.5;
                        sparkle(p, pos, size, (1.0 - (k - 0.5).abs() * 2.0).max(0.3) * hash01(seed + 9));
                    }
                }
            }
        }
        _ => {
            // Magic Move: matched objects morph, the rest fade
            base_quad(p, a.base.id(), r, 1.0);
            base_quad(p, b.base.id(), r, e);
            let from_of: std::collections::HashMap<usize, usize> = pairs.iter().map(|(i, j)| (*j, *i)).collect();
            let matched_from: std::collections::HashSet<usize> = pairs.iter().map(|(i, _)| *i).collect();
            for (i, o) in a.objs.iter().enumerate() {
                if !matched_from.contains(&i) {
                    builds::draw_layer(p, &map, &o.layer, &obj_look(o.alpha * (1.0 - (e * 2.0).min(1.0))));
                }
            }
            for (j, o) in b.objs.iter().enumerate() {
                match from_of.get(&j) {
                    Some(&i) => {
                        let f = &a.objs[i];
                        // text pivots on its painted lines (frames may align the text differently)
                        let repivot = |l: &Layer| Layer { tex: l.tex.clone(), low: Vec::new(), rect: l.rect, center: l.rect.center(), angle: l.angle, size: l.size };
                        let (fp, tp);
                        let (fl, tl) = if f.is_text && o.is_text {
                            fp = repivot(&f.layer);
                            tp = repivot(&o.layer);
                            (&fp, &tp)
                        } else {
                            (&f.layer, &o.layer)
                        };
                        let center = fl.center + (tl.center - fl.center) * e;
                        let angle = fl.angle + (tl.angle - fl.angle) * e;
                        // morph the object frames (layers are cropped to their painted pixels)
                        let size = fl.size + (tl.size - fl.size) * e;
                        // same object; a differently shaped crop of the same picture cross-fades as it morphs
                        let aspect = |v: egui::Vec2| v.x / v.y.max(1.0);
                        let same = f.key == o.key && (o.is_text || (aspect(f.layer.size) / aspect(o.layer.size) - 1.0).abs() < 0.03);
                        // uniform text scale (from-layer relative): painted size ratio, 1 if the frames match
                        let text_end = if (fl.size - tl.size).length() < 1.0 { 1.0 } else { ((tl.rect.width() / fl.rect.width().max(1.0)) * (tl.rect.height() / fl.rect.height().max(1.0))).sqrt() };
                        let text_scale = 1.0 + (text_end - 1.0) * e;
                        let mk = |l: &Layer, alpha: f32| {
                            let mut lk = obj_look(alpha);
                            lk.offset = center - l.center;
                            lk.rot = angle - l.angle;
                            // identical objects stretch with their frame; different text moves and cross-fades
                            if same && o.is_text {
                                // text keeps its proportions: scale uniformly by how much bigger it is drawn
                                let k = text_scale / (if std::ptr::eq(l, fl) { 1.0 } else { text_end });
                                lk.sx = k;
                                lk.sy = k;
                            } else if same {
                                lk.sx = if l.size.x > 1.0 { size.x / l.size.x } else { 1.0 };
                                lk.sy = if l.size.y > 1.0 { size.y / l.size.y } else { 1.0 };
                            }
                            lk
                        };
                        let fa = f.alpha + (o.alpha - f.alpha) * e;
                        let from_alpha = if same { 1.0 - builds::smooth((t - 0.6) / 0.4) } else { 1.0 - e };
                        builds::draw_layer(p, &map, fl, &mk(fl, fa * from_alpha));
                        let to_alpha = if same { builds::smooth(t / 0.5) } else { e };
                        builds::draw_layer(p, &map, tl, &mk(tl, fa * to_alpha));
                    }
                    None => builds::draw_layer(p, &map, &o.layer, &obj_look(o.alpha * ((e - 0.5) * 2.0).clamp(0.0, 1.0))),
                }
            }
        }
    }
}

/// Progress of object `i` of `n` when they start one after another (Keynote staggers the
/// objects of Object transitions), each running over the same length of time.
fn staggered(t: f32, i: usize, n: usize) -> f32 {
    if n <= 1 {
        return t.clamp(0.0, 1.0);
    }
    let d = (0.4 / (n - 1) as f32).min(0.12);
    let len = 1.0 - d * (n - 1) as f32;
    ((t - d * i as f32) / len).clamp(0.0, 1.0)
}

fn overshoot(t: f32) -> f32 {
    let c = 1.70158;
    let u = t - 1.0;
    1.0 + (c + 1.0) * u * u * u + c * u * u
}

/// Keynote's Object transitions: the outgoing slide's objects leave one by one, then the
/// incoming slide's objects arrive one by one; the backgrounds cross-fade.
#[allow(clippy::too_many_arguments)]
fn object_effect(p: &Painter, r: Rect, map: &Map, name: &str, t: f32, a: &ObjSet, b: &ObjSet, dir: u32) {
    use std::f32::consts::FRAC_PI_2;
    let slide = vec2(r.width(), r.height()) / map.k;
    // the outgoing objects use the first 55 %, the incoming ones start at 45 %
    let (t_out, t_in) = match name {
        "ca-push" | "ca-cube" => (t, t),
        _ => ((t / 0.55).min(1.0), ((t - 0.45) / 0.55).max(0.0)),
    };
    base_quad(p, a.base.id(), r, 1.0);
    base_quad(p, b.base.id(), r, builds::smooth(t));
    let v = crate::builds::dir_vec(dir, vec2(-1.0, 0.0));
    for (leaving, set, tt) in [(true, a, t_out), (false, b, t_in)] {
        let n = set.objs.len();
        for (i, o) in set.objs.iter().enumerate() {
            let k = builds::smooth(staggered(tt, i, n));
            let l = &o.layer;
            let (w, h) = (l.size.x.max(1.0), l.size.y.max(1.0));
            let mut lk = obj_look(o.alpha);
            match name {
                "ca-zoom" => {
                    if leaving {
                        // towards the viewer, fading
                        let s = 1.0 + 1.2 * k;
                        (lk.sx, lk.sy) = (s, s);
                        lk.alpha *= 1.0 - k;
                    } else {
                        // from far away
                        let s = 0.25 + 0.75 * k;
                        (lk.sx, lk.sy) = (s, s);
                        lk.alpha *= k;
                    }
                }
                "ca-push" => {
                    // pushed across and off the slide
                    let dist = vec2(v.x * (slide.x + w), v.y * (slide.y + h));
                    lk.offset = if leaving { dist * k } else { -dist * (1.0 - k) };
                }
                "ca-pop" => {
                    if leaving {
                        let s = 1.0 - k;
                        (lk.sx, lk.sy) = (s, s);
                        lk.alpha *= 1.0 - k * k;
                    } else {
                        let s = overshoot(staggered(tt, i, n)).max(0.0);
                        (lk.sx, lk.sy) = (s, s);
                        lk.alpha *= (k * 3.0).min(1.0);
                    }
                }
                "ca-revolve" => {
                    // turns about its left edge like a door
                    let ang = if leaving { k * FRAC_PI_2 } else { (1.0 - k) * FRAC_PI_2 };
                    lk.sx = ang.cos();
                    lk.offset = vec2(-(1.0 - lk.sx) * w / 2.0, 0.0);
                    lk.alpha *= if leaving { 1.0 - k * 0.5 } else { 0.5 + k * 0.5 };
                }
                "ca-cube" => {
                    // each object is a face of its own cube turning in the push direction
                    let hor = v.x.abs() >= v.y.abs();
                    let f = if leaving { 1.0 - k } else { k };
                    let face = (f * FRAC_PI_2).sin();
                    let shift = if leaving { k } else { k - 1.0 };
                    if hor {
                        lk.sx = face;
                        lk.offset = vec2(v.x * w / 2.0 * shift, 0.0);
                    } else {
                        lk.sy = face;
                        lk.offset = vec2(0.0, v.y * h / 2.0 * shift);
                    }
                    lk.alpha *= 0.4 + 0.6 * f;
                }
                "ca-dissolve-and-flip" => {
                    // half a turn about the vertical axis while dissolving
                    let ang = if leaving { k * FRAC_PI_2 } else { (1.0 - k) * FRAC_PI_2 };
                    lk.sx = ang.cos().max(0.02);
                    lk.alpha *= if leaving { 1.0 - k } else { k };
                }
                _ => {
                    // ca-swing: hinged at the top, swinging down into place (out: up and away)
                    let ang = if leaving { k * FRAC_PI_2 } else { (1.0 - k) * FRAC_PI_2 };
                    lk.sy = ang.cos().max(0.02);
                    lk.offset = vec2(0.0, -(1.0 - lk.sy) * h / 2.0);
                    lk.alpha *= if leaving { 1.0 - k } else { k };
                }
            }
            if lk.alpha > 0.005 && lk.sx > 0.001 && lk.sy > 0.001 {
                builds::draw_layer(p, map, l, &lk);
            }
        }
    }
}

/// An object split into horizontal strips that drift sideways by `amount` (0 = still).
fn shimmer(p: &Painter, map: &Map, l: &Layer, alpha: f32, amount: f32, seed: u32) {
    if alpha <= 0.01 {
        return;
    }
    if amount <= 0.001 {
        builds::draw_layer(p, map, l, &obj_look(alpha));
        return;
    }
    let n = 14;
    let lo: Vec2 = l.rect.min - l.center;
    let hi: Vec2 = l.rect.max - l.center;
    let mut m = Mesh::with_texture(l.tex.id());
    for i in 0..n {
        let y0 = lo.y + (hi.y - lo.y) * i as f32 / n as f32;
        let y1 = lo.y + (hi.y - lo.y) * (i + 1) as f32 / n as f32;
        let dx = (hash01(seed * 31 + i) - 0.5) * 2.0 * amount * l.rect.width().max(60.0) * 0.35;
        let mut lk = obj_look(alpha * (1.0 - amount * hash01(seed * 7 + i) * 0.6));
        lk.offset = vec2(dx, 0.0);
        builds::poly(&mut m, map, l.rect, l.center, l.angle, &lk, &[(vec2(lo.x, y0), 1.0), (vec2(hi.x, y0), 1.0), (vec2(hi.x, y1), 1.0), (vec2(lo.x, y1), 1.0)]);
    }
    p.add(m);
}
