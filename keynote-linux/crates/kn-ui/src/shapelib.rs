//! Shape icons for the Content Hub (all three apps): an outline drawn once with tiny-skia
//! (concave outlines and holes come out right) and kept as a texture.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use egui::{vec2, Color32, TextureHandle, TextureOptions};
use kn_model::shapelib::{self, LibShape};
use kn_model::{Path, PathEl};

/// Icon size in pixels (drawn large, for sharp icons on 2x screens and big tiles).
const ICON_PX: u32 = 128;

/// An outline `w` × `h` as a filled icon texture; `key` names it in the cache.
pub fn texture(ctx: &egui::Context, key: &str, path: &Path, w: f32, h: f32, color: Color32) -> Option<TextureHandle> {
    static CACHE: OnceLock<Mutex<HashMap<(String, [u8; 4]), TextureHandle>>> = OnceLock::new();
    let ck = (key.to_string(), color.to_array());
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    if let Some(t) = cache.get(&ck) {
        return Some(t.clone());
    }
    let mut pm = tiny_skia::Pixmap::new(ICON_PX, ICON_PX)?;
    let pad = 4.0;
    let k = (ICON_PX as f32 - 2.0 * pad) / w.max(h).max(1.0);
    let (ox, oy) = ((ICON_PX as f32 - w * k) / 2.0, (ICON_PX as f32 - h * k) / 2.0);
    let mut pb = tiny_skia::PathBuilder::new();
    let at = |p: kn_model::Point| (ox + p.x * k, oy + p.y * k);
    for e in &path.els {
        match *e {
            PathEl::Move(a) => {
                let (x, y) = at(a);
                pb.move_to(x, y)
            }
            PathEl::Line(a) => {
                let (x, y) = at(a);
                pb.line_to(x, y)
            }
            PathEl::Quad(a, b) => {
                let ((x1, y1), (x, y)) = (at(a), at(b));
                pb.quad_to(x1, y1, x, y)
            }
            PathEl::Cubic(a, b, c) => {
                let ((x1, y1), (x2, y2), (x, y)) = (at(a), at(b), at(c));
                pb.cubic_to(x1, y1, x2, y2, x, y)
            }
            PathEl::Close => pb.close(),
        }
    }
    let path = pb.finish()?;
    let mut paint = tiny_skia::Paint::default();
    let [r, g, b, a] = color.to_array();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    let px: Vec<Color32> = pm.pixels().iter().map(|p| Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha())).collect();
    let img = egui::ColorImage { size: [ICON_PX as usize, ICON_PX as usize], pixels: px, source_size: vec2(ICON_PX as f32, ICON_PX as f32) };
    let t = ctx.load_texture(format!("shape-{key}"), img, TextureOptions::LINEAR);
    cache.insert(ck, t.clone());
    Some(t)
}

/// A library shape's icon.
pub fn icon(ctx: &egui::Context, s: &LibShape, color: Color32) -> Option<TextureHandle> {
    texture(ctx, &format!("lib-{}-{}", s.category, s.name), &s.outline(), s.w, s.h, color)
}

/// A library shape by name (the Content Hub's tab and chip icons).
pub fn named(name: &str) -> Option<&'static LibShape> {
    shapelib::shapes().iter().find(|s| s.name == name)
}
