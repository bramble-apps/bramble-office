//! The slideshow for a front end that draws it (ffi.rs kd_play_*): the desktop's own show
//! (play.rs: builds, transitions, dynamic backgrounds, links, self-playing timing) runs here
//! headless, and what it paints comes out as textured triangles and the textures they use,
//! for the front end to draw as they are (Flutter: Canvas.drawVertices with an image shader).
//! Only the show's slide drawing goes out: its HUD, video controls and presenter display are
//! off (Play::external), the front end draws its own.
//!
//! A frame is little-endian binary, every section 4-byte aligned:
//!   u32 textures set; each: u64 id, u32 w, u32 h, w*h*4 bytes RGBA premultiplied (whole image)
//!   u32 textures freed; each: u64 id
//!   u32 meshes; each: u64 texture, f32 clip x0 y0 x1 y1, u32 vertices, u32 indices,
//!     vertices × (f32 x, f32 y, f32 u, f32 v (texture pixels), u32 ARGB unpremultiplied),
//!     indices × u16 (padded to 4 bytes)

use std::collections::HashMap;

use egui::{ColorImage, Event, TextureId};
use serde_json::Value;

use crate::app::App;

/// The textures as the show last set them (patches applied), by id.
#[derive(Default)]
pub struct Mirror {
    images: HashMap<u64, ColorImage>,
    started: Option<std::time::Instant>,
    /// When the show asked to be drawn again (None: only on input).
    pub repaint: Option<std::time::Duration>,
}

fn tex_key(id: TextureId) -> u64 {
    match id {
        TextureId::Managed(n) => n & !(1 << 63),
        TextureId::User(n) => n | (1 << 63),
    }
}

/// Front end input as egui events: [{"t": "tap", "x", "y"}, {"t": "key", "k": "ArrowRight"},
/// {"t": "move", "x", "y"}] (points on the show's screen).
fn events(v: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    for e in v.as_array().into_iter().flatten() {
        let pos = egui::pos2(e["x"].as_f64().unwrap_or(0.0) as f32, e["y"].as_f64().unwrap_or(0.0) as f32);
        match e["t"].as_str().unwrap_or_default() {
            "tap" | "tap2" => {
                let button = if e["t"] == "tap2" { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
                out.push(Event::PointerMoved(pos));
                for pressed in [true, false] {
                    out.push(Event::PointerButton { pos, button, pressed, modifiers: Default::default() });
                }
            }
            "move" => out.push(Event::PointerMoved(pos)),
            "key" => {
                if let Some(key) = egui::Key::from_name(e["k"].as_str().unwrap_or_default()) {
                    for pressed in [true, false] {
                        out.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// One frame of the show on a `w` × `h` point screen at `ppp` pixels per point, after `input`.
pub fn frame(ctx: &egui::Context, app: &mut App, m: &mut Mirror, w: f32, h: f32, ppp: f32, input: &Value) -> Vec<u8> {
    let t0 = *m.started.get_or_insert_with(std::time::Instant::now);
    let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))), events: events(input), time: Some(t0.elapsed().as_secs_f64()), focused: true, ..Default::default() };
    raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
    let full = ctx.run_ui(raw, |ui| crate::play::ui(app, ui));
    m.repaint = full.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).filter(|d| *d < std::time::Duration::from_secs(3600));
    let prims = ctx.tessellate(full.shapes, full.pixels_per_point);

    let mut out: Vec<u8> = Vec::new();
    let u32_ = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    let f32_ = |o: &mut Vec<u8>, v: f32| o.extend_from_slice(&v.to_le_bytes());
    // textures: patches applied to the mirror, the whole image sent
    let mut set: Vec<u64> = Vec::new();
    for (id, delta) in full.textures_delta.set.iter().flat_map(|(id, ds)| ds.iter().map(move |d| (id, d))) {
        let egui::ImageData::Color(img) = &delta.image;
        let k = tex_key(*id);
        match (delta.pos, m.images.get_mut(&k)) {
            (Some([x, y]), Some(base)) => {
                let (bw, iw) = (base.size[0], img.size[0]);
                for row in 0..img.size[1] {
                    let d = (y + row) * bw + x;
                    if d + iw <= base.pixels.len() {
                        base.pixels[d..d + iw].copy_from_slice(&img.pixels[row * iw..row * iw + iw]);
                    }
                }
            }
            _ => {
                m.images.insert(k, (*img).as_ref().clone());
            }
        }
        if !set.contains(&k) {
            set.push(k);
        }
    }
    u32_(&mut out, set.len() as u32);
    for k in &set {
        let img = &m.images[k];
        out.extend_from_slice(&k.to_le_bytes());
        u32_(&mut out, img.size[0] as u32);
        u32_(&mut out, img.size[1] as u32);
        out.reserve(img.pixels.len() * 4);
        for p in &img.pixels {
            out.extend_from_slice(&p.to_array());
        }
    }
    u32_(&mut out, full.textures_delta.free.len() as u32);
    for id in &full.textures_delta.free {
        let k = tex_key(*id);
        m.images.remove(&k);
        out.extend_from_slice(&k.to_le_bytes());
    }
    // meshes (at most 65 536 vertices each)
    let meshes: Vec<(egui::Rect, egui::epaint::Mesh16)> = prims
        .into_iter()
        .filter_map(|p| match p.primitive {
            egui::epaint::Primitive::Mesh(mesh) => Some((p.clip_rect, mesh)),
            egui::epaint::Primitive::Callback(_) => None,
        })
        .flat_map(|(clip, mesh)| mesh.split_to_u16().into_iter().map(move |m| (clip, m)))
        .collect();
    u32_(&mut out, meshes.len() as u32);
    for (clip, mesh) in &meshes {
        let k = tex_key(mesh.texture_id);
        let size = m.images.get(&k).map(|i| (i.size[0] as f32, i.size[1] as f32)).unwrap_or((1.0, 1.0));
        out.extend_from_slice(&k.to_le_bytes());
        for v in [clip.min.x, clip.min.y, clip.max.x, clip.max.y] {
            f32_(&mut out, v);
        }
        u32_(&mut out, mesh.vertices.len() as u32);
        u32_(&mut out, mesh.indices.len() as u32);
        for v in &mesh.vertices {
            f32_(&mut out, v.pos.x);
            f32_(&mut out, v.pos.y);
            f32_(&mut out, v.uv.x * size.0);
            f32_(&mut out, v.uv.y * size.1);
            let [r, g, b, a] = v.color.to_srgba_unmultiplied();
            u32_(&mut out, (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32);
        }
        for i in &mesh.indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
        if mesh.indices.len() % 2 == 1 {
            out.extend_from_slice(&[0, 0]);
        }
    }
    out
}

/// The show now, for the front end's own chrome: null when not playing, else {slide, step,
/// count, rehearse, timer (seconds), animating, notes, next (slide index or null), size,
/// repaint (ms, or null)}.
pub fn status(app: &mut App, m: &Mirror) -> Value {
    let Some(p) = app.play.as_ref() else { return Value::Null };
    let (rehearse, timer, animating) = p.info();
    let (i, step) = (p.index, p.step);
    let playing: Vec<(kn_format::ObjId, crate::media::Spec)> = p.movies().to_vec();
    let narrating = p.narrating();
    let count = app.slides.len();
    let next = (i + 1..count).find(|j| !app.slides[*j].skipped);
    let sc = app.scene(i);
    let notes = sc.as_ref().and_then(|sc| app.doc.as_ref().and_then(|d| d.notes_storage(sc.slide).map(|s| d.storage_text(s)))).unwrap_or_default();
    let steps = sc.as_ref().map(|s| s.build_order.len()).unwrap_or(0);
    // movies to play (the front end's player) where they are on the slide
    let movies: Vec<Value> = playing
        .iter()
        .filter_map(|(id, spec)| {
            let d = sc.as_ref()?.drawables.iter().find(|d| d.id == *id)?;
            let g = d.geom;
            Some(serde_json::json!({
                "id": id, "path": spec.path.to_string_lossy(), "start": spec.start, "end": spec.end, "volume": spec.volume, "loop": spec.looping,
                "video": spec.video.is_some(), "rect": [g.x, g.y, g.w, g.h], "angle": g.angle,
            }))
        })
        .collect();
    serde_json::json!({
        "slide": i, "step": step, "steps": steps, "count": count, "rehearse": rehearse, "timer": timer, "animating": animating,
        "notes": notes, "next": next, "size": sc.as_ref().map(|s| [s.size.w, s.size.h]),
        "repaint": m.repaint.map(|d| d.as_millis() as u64),
        "movies": movies,
        "narrating": narrating,
        "pd": { "current": app.settings.pd_current, "next": app.settings.pd_next, "notes": app.settings.pd_notes, "clock": app.settings.pd_clock, "timer": app.settings.pd_timer },
    })
}
