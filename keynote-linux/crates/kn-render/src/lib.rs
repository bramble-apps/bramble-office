//! Slide rasterizer: draws [`kn_model::SlideScene`]s with tiny-skia. Text is shaped with
//! cosmic-text and drawn as glyph outlines, so it stays sharp under any zoom/rotation.

pub mod motion;
pub mod chart;
pub mod fonts;
pub mod paint;
pub mod table;
pub mod text;

use std::collections::HashMap;
use std::sync::Arc;

use kn_model::*;
use tiny_skia as sk;

use crate::fonts::{Fonts, OUTLINE_SIZE};
use crate::paint::*;
use crate::text::TextLayout;

pub use tiny_skia;

/// Which drawables to paint.
#[derive(Clone, Copy, Default)]
pub enum Filter<'a> {
    #[default]
    All,
    /// Everything except these (e.g. objects being dragged, or not yet built in).
    Except(&'a [kn_format::ObjId]),
    Only(kn_format::ObjId),
}

impl Filter<'_> {
    fn allows(&self, id: kn_format::ObjId) -> bool {
        match self {
            Filter::All => true,
            Filter::Except(ids) => !ids.contains(&id),
            Filter::Only(x) => *x == id,
        }
    }
}

enum ImageSlot {
    /// Decoded image and its last use.
    Ready(Arc<sk::Pixmap>, u64),
    Pending,
    Failed,
}

type DecodeJob = (u64, Vec<u8>);

/// Two worker threads decoding images; results are collected by `images_arrived`.
struct AsyncDecoder {
    tx: std::sync::mpsc::Sender<DecodeJob>,
    done: Arc<std::sync::Mutex<Vec<(u64, Option<Arc<sk::Pixmap>>)>>>,
}

impl AsyncDecoder {
    fn new() -> AsyncDecoder {
        let (tx, rx) = std::sync::mpsc::channel::<DecodeJob>();
        let rx = Arc::new(std::sync::Mutex::new(rx));
        let done = Arc::new(std::sync::Mutex::new(Vec::new()));
        for _ in 0..2 {
            let rx = rx.clone();
            let done = done.clone();
            std::thread::spawn(move || loop {
                let job = rx.lock().unwrap().recv();
                let Ok((id, bytes)) = job else { return };
                let px = std::panic::catch_unwind(|| decode_image(&bytes)).ok().flatten().map(Arc::new);
                done.lock().unwrap().push((id, px));
            });
        }
        AsyncDecoder { tx, done }
    }

    fn submit(&self, id: u64, bytes: Vec<u8>) {
        let _ = self.tx.send((id, bytes));
    }
}

pub struct Renderer {
    pub fonts: Fonts,
    images: HashMap<u64, ImageSlot>,
    /// Decode images on background threads (the app) instead of inline (exports, tests).
    async_images: Option<AsyncDecoder>,
    /// Pictures left out (still being decoded) since [`Renderer::take_missing`] was last called.
    missing: u32,
    /// Use counter for least-recently-used eviction.
    tick: u64,
    /// Decode inline even though a background decoder exists.
    async_paused: bool,
    /// Text layouts by (drawable id, content hash), reused between frames.
    layouts: HashMap<(kn_format::ObjId, u64), (TextLayout, Geometry)>,
    /// Halved copies of pictures drawn much smaller than their size: (data id, halvings).
    mips: HashMap<(u64, u32), Arc<sk::Pixmap>>,
    /// Draw review highlights (Insert > Highlight) behind text: an app's editing canvas turns
    /// this on; exports, slideshows and thumbnails leave it off.
    pub review_marks: bool,
}

/// Device pixels per source pixel for a pattern drawn with `pattern` under `t`.
fn draw_scale(t: sk::Transform, pattern: sk::Transform) -> f32 {
    let m = t.pre_concat(pattern);
    ((m.sx * m.sy - m.kx * m.ky).abs()).sqrt()
}

/// A copy at half the size (2x2 box average of premultiplied pixels).
fn halve(px: &sk::Pixmap) -> Option<sk::Pixmap> {
    let (w, h) = (px.width() as usize, px.height() as usize);
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = sk::Pixmap::new(nw as u32, nh as u32)?;
    let src = px.data();
    let dst = out.data_mut();
    for y in 0..nh {
        let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
        for x in 0..nw {
            let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
            for c in 0..4 {
                let s = u32::from(src[(y0 * w + x0) * 4 + c]) + u32::from(src[(y0 * w + x1) * 4 + c]) + u32::from(src[(y1 * w + x0) * 4 + c]) + u32::from(src[(y1 * w + x1) * 4 + c]);
                dst[(y * nw + x) * 4 + c] = ((s + 2) / 4) as u8;
            }
        }
    }
    Some(out)
}

fn text_key(tb: &TextBox, g: &Geometry) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{tb:?}").hash(&mut h);
    (g.w.to_bits(), g.h.to_bits(), g.x.to_bits(), g.y.to_bits(), g.auto_width, g.auto_height).hash(&mut h);
    h.finish()
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer { fonts: Fonts::new(), images: HashMap::new(), async_images: None, missing: 0, tick: 0, async_paused: false, layouts: HashMap::new(), mips: HashMap::new(), review_marks: false }
    }

    /// Forget cached images/layouts (e.g. after opening another document).
    pub fn clear_caches(&mut self) {
        self.mips.clear();
        self.images.clear();
        self.layouts.clear();
    }

    /// Decode images on two background threads from now on; `image()` returns None until an
    /// image is ready, and [`Renderer::images_arrived`] reports when to redraw.
    pub fn enable_async_images(&mut self) {
        if self.async_images.is_none() {
            self.async_images = Some(AsyncDecoder::new());
        }
    }

    /// Switch background decoding on/off (the slideshow needs every image immediately).
    /// Whether images are being decoded in the background now.
    pub fn async_images(&self) -> bool {
        self.async_images.is_some() && !self.async_paused
    }

    pub fn set_async_images(&mut self, on: bool) {
        if on {
            self.enable_async_images();
        }
        self.async_paused = !on;
    }

    /// How many pictures renders left out since the last call (being decoded in the background):
    /// 0 means what was drawn is complete.
    pub fn take_missing(&mut self) -> u32 {
        std::mem::take(&mut self.missing)
    }

    /// True (once) when background-decoded images became available since the last call.
    pub fn images_arrived(&mut self) -> bool {
        let Some(dec) = &self.async_images else { return false };
        let done: Vec<(u64, Option<Arc<sk::Pixmap>>)> = std::mem::take(&mut *dec.done.lock().unwrap());
        let any = !done.is_empty();
        for (id, px) in done {
            self.tick += 1;
            self.images.insert(id, match px {
                Some(p) => ImageSlot::Ready(p, self.tick),
                None => ImageSlot::Failed,
            });
        }
        if any {
            self.evict();
        }
        any
    }

    /// A picture to draw at `scale` device pixels per source pixel: the picture itself, or (much
    /// smaller on screen) a cached halved copy, so the cheap bilinear filter looks right and big
    /// photos don't cost their full size every time. Returns it and the size factor used.
    fn picture_for(&mut self, data_id: u64, px: Arc<sk::Pixmap>, scale: f32) -> (Arc<sk::Pixmap>, f32) {
        let mut halvings = 0u32;
        let mut s = scale;
        while s < 0.5 && halvings < 6 && px.width() >> (halvings + 1) >= 16 && px.height() >> (halvings + 1) >= 16 {
            s *= 2.0;
            halvings += 1;
        }
        if halvings == 0 {
            return (px, 1.0);
        }
        if let Some(m) = self.mips.get(&(data_id, halvings)) {
            return (m.clone(), (1u32 << halvings) as f32);
        }
        let mut cur = px;
        for h in 1..=halvings {
            if let Some(m) = self.mips.get(&(data_id, h)) {
                cur = m.clone();
                continue;
            }
            let Some(next) = halve(&cur) else { break };
            cur = Arc::new(next);
            self.mips.insert((data_id, h), cur.clone());
        }
        if self.mips.len() > 64 {
            self.mips.clear();
        }
        (cur, (1u32 << halvings) as f32)
    }

    pub fn image(&mut self, doc: &Document, data_id: u64) -> Option<Arc<sk::Pixmap>> {
        self.tick += 1;
        let tick = self.tick;
        match self.images.get_mut(&data_id) {
            Some(ImageSlot::Ready(p, used)) => {
                *used = tick;
                return Some(p.clone());
            }
            Some(ImageSlot::Failed) => return None,
            Some(ImageSlot::Pending) if !self.async_paused => {
                self.missing += 1;
                return None;
            }
            _ => {}
        }
        match self.async_images.as_ref().filter(|_| !self.async_paused) {
            Some(dec) => {
                if let Some(bytes) = doc.data(data_id) {
                    dec.submit(data_id, bytes.to_vec());
                    self.images.insert(data_id, ImageSlot::Pending);
                    self.missing += 1;
                } else {
                    self.images.insert(data_id, ImageSlot::Failed);
                }
                None
            }
            None => {
                let px = doc.data(data_id).and_then(|b| decode_image(&b)).map(Arc::new);
                self.images.insert(data_id, px.clone().map(|p| ImageSlot::Ready(p, tick)).unwrap_or(ImageSlot::Failed));
                self.evict();
                px
            }
        }
    }

    /// Keep decoded images under ~256 MB, dropping the least recently used.
    fn evict(&mut self) {
        const BUDGET: usize = 256 << 20;
        let size = |s: &ImageSlot| if let ImageSlot::Ready(p, _) = s { p.data().len() } else { 0 };
        let mut total: usize = self.images.values().map(size).sum();
        while total > BUDGET {
            let Some((&id, _)) = self
                .images
                .iter()
                .filter(|(_, s)| matches!(s, ImageSlot::Ready(..)))
                .min_by_key(|(_, s)| if let ImageSlot::Ready(_, u) = s { *u } else { u64::MAX })
            else {
                break;
            };
            total -= size(&self.images[&id]);
            self.images.remove(&id);
        }
    }

    /// Text layout of a shape's text plus its effective frame (auto-sized boxes resolved).
    pub fn text_layout(&mut self, d: &Drawable) -> Option<(TextLayout, Geometry)> {
        let DrawableKind::Shape(s) = &d.kind else { return None };
        let tb = s.text.as_ref()?;
        let key = (d.id, text_key(tb, &d.geom));
        if let Some(v) = self.layouts.get(&key) {
            return Some(v.clone());
        }
        let g = d.geom;
        // only text-bearing boxes auto-size; shapes keep their frame
        let auto_w = g.auto_width && (s.is_text_box || d.placeholder.is_some() || g.w <= 0.0);
        let auto_h = g.auto_height && (s.is_text_box || d.placeholder.is_some() || g.h <= 0.0);
        let tl = text::layout(&mut self.fonts, tb, g.w, g.h, auto_w, auto_h);
        let mut eg = g;
        if auto_w {
            let w = tl.content_w + tb.padding[0] + tb.padding[2];
            let align = tb.paragraphs.first().map(|p| p.align).unwrap_or(Align::Left);
            // an auto-sized box's stored size is stale: Keynote keeps its stored position as the
            // anchor its alignment grows from (R28: middle-aligned text centres on y)
            eg.x = match align {
                Align::Center => g.x - w / 2.0,
                Align::Right => g.x - w,
                _ => g.x,
            };
            eg.w = w;
        }
        if auto_h {
            let h = tl.content_h + tb.padding[1] + tb.padding[3];
            eg.y = match tb.valign {
                VAlign::Middle => g.y - h / 2.0,
                VAlign::Bottom => g.y - h,
                VAlign::Top => g.y,
            };
            eg.h = h;
        }
        // auto-width layouts were made without a width; redo alignment inside the final width
        let tl = if auto_w { text::layout(&mut self.fonts, tb, eg.w, eg.h, false, auto_h) } else { tl };
        if self.layouts.len() > 4000 {
            self.layouts.clear();
        }
        self.layouts.insert(key, (tl.clone(), eg));
        Some((tl, eg))
    }

    /// The frame a drawable actually occupies (auto-sized text boxes resolved, tables at their
    /// row/column size).
    pub fn effective_geometry(&mut self, d: &Drawable) -> Geometry {
        if let DrawableKind::Table(t) = &d.kind {
            return t.frame(&d.geom);
        }
        self.text_layout(d).map(|(_, g)| g).unwrap_or(d.geom)
    }

    /// Render a whole slide into a new pixmap `width_px` wide.
    pub fn render_scene(&mut self, doc: &Document, scene: &SlideScene, width_px: u32) -> Option<sk::Pixmap> {
        let scale = width_px as f32 / scene.size.w;
        let h = (scene.size.h * scale).round().max(1.0) as u32;
        let mut pm = sk::Pixmap::new(width_px.max(1), h)?;
        self.draw_scene(doc, scene, &mut pm, sk::Transform::from_scale(scale, scale), Filter::All, true);
        Some(pm)
    }

    pub fn draw_scene(
        &mut self,
        doc: &Document,
        scene: &SlideScene,
        pm: &mut sk::Pixmap,
        base: sk::Transform,
        filter: Filter,
        background: bool,
    ) {
        if background {
            self.draw_fill_rect(doc, pm, &scene.background, scene.size.w, scene.size.h, base);
        }
        for d in &scene.drawables {
            if filter.allows(d.id) {
                self.draw_drawable(doc, d, pm, base);
            }
        }
    }

    pub fn draw_fill_rect(&mut self, doc: &Document, pm: &mut sk::Pixmap, fill: &Fill, w: f32, h: f32, t: sk::Transform) {
        let Some(path) = rect_path(0.0, 0.0, w, h) else { return };
        self.draw_fill(doc, pm, fill, &path, w, h, t);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_fill(&mut self, doc: &Document, pm: &mut sk::Pixmap, fill: &Fill, path: &sk::Path, w: f32, h: f32, t: sk::Transform) {
        if let Fill::Image { data_id, technique, tint, reference, .. } = fill {
            let Some(img) = self.image(doc, *data_id) else {
                // the image isn't available: Keynote's stored stand-in colour
                if let (Some(c), Some(p)) = (reference, rect_path(0.0, 0.0, w, h)) {
                    pm.fill_path(&p, &solid(*c), sk::FillRule::Winding, t, None);
                }
                return;
            };
            let (iw, ih) = (img.width() as f32, img.height() as f32);
            let (sx, sy, tx, ty, spread) = match technique {
                ImageFillTechnique::Stretch => (w / iw, h / ih, 0.0, 0.0, sk::SpreadMode::Pad),
                ImageFillTechnique::Tile => (1.0, 1.0, 0.0, 0.0, sk::SpreadMode::Repeat),
                ImageFillTechnique::Natural => (1.0, 1.0, (w - iw) / 2.0, (h - ih) / 2.0, sk::SpreadMode::Pad),
                ImageFillTechnique::ScaleToFit => {
                    let s = (w / iw).min(h / ih);
                    (s, s, (w - iw * s) / 2.0, (h - ih * s) / 2.0, sk::SpreadMode::Pad)
                }
                ImageFillTechnique::ScaleToFill => {
                    let s = (w / iw).max(h / ih);
                    (s, s, (w - iw * s) / 2.0, (h - ih * s) / 2.0, sk::SpreadMode::Pad)
                }
            };
            let pattern = sk::Transform::from_row(sx, 0.0, 0.0, sy, tx, ty);
            let (img, f) = match technique {
                ImageFillTechnique::Tile => (img, 1.0),
                _ => self.picture_for(*data_id, img, draw_scale(t, pattern)),
            };
            let mut p = sk::Paint::default();
            p.anti_alias = true;
            p.shader = sk::Pattern::new(img.as_ref().as_ref(), spread, sk::FilterQuality::Bilinear, 1.0, pattern.pre_scale(f, f));
            pm.fill_path(path, &p, sk::FillRule::Winding, t, None);
            if let Some(c) = tint {
                pm.fill_path(path, &solid(*c), sk::FillRule::Winding, t, None);
            }
            return;
        }
        if let Some(p) = fill_paint(fill, w, h) {
            pm.fill_path(path, &p, sk::FillRule::Winding, t, None);
        }
    }

    /// Device-space bounds of a drawable (rotated frame + a margin for strokes/shadows).
    fn device_bounds(&self, g: &Geometry, base: sk::Transform, margin: f32) -> sk::IntRect {
        let (lo, hi) = g.bounds();
        let mut pts = [
            sk::Point::from_xy(lo.x - margin, lo.y - margin),
            sk::Point::from_xy(hi.x + margin, lo.y - margin),
            sk::Point::from_xy(lo.x - margin, hi.y + margin),
            sk::Point::from_xy(hi.x + margin, hi.y + margin),
        ];
        base.map_points(&mut pts);
        let minx = pts.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).floor();
        let miny = pts.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).floor();
        let maxx = pts.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).ceil();
        let maxy = pts.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).ceil();
        sk::IntRect::from_ltrb(minx as i32, miny as i32, (maxx as i32).max(minx as i32 + 1), (maxy as i32).max(miny as i32 + 1))
            .unwrap_or(sk::IntRect::from_xywh(0, 0, 1, 1).unwrap())
    }

    pub fn draw_drawable(&mut self, doc: &Document, d: &Drawable, pm: &mut sk::Pixmap, base: sk::Transform) {
        let needs_layer = d.opacity < 0.999 || d.shadow.is_some() || d.reflection.is_some_and(|r| r > 0.0);
        if !needs_layer {
            self.draw_content(doc, d, pm, base);
            return;
        }
        let scale = (base.sx * base.sx + base.ky * base.ky).sqrt();
        let g = self.effective_geometry(d);
        let blur = d.shadow.as_ref().map(|s| s.blur + s.offset).unwrap_or(0.0);
        let refl_extra = if d.reflection.is_some() { g.h.max(g.w) } else { 0.0 };
        let margin = 8.0 + blur * 2.0 + stroke_margin(d) + refl_extra;
        let r = self.device_bounds(&g, base, margin);
        // clip the layer to the target so huge zooms don't allocate huge layers
        let screen = sk::IntRect::from_xywh(0, 0, pm.width(), pm.height()).unwrap();
        let outset = sk::IntRect::from_ltrb(-64, -64, pm.width() as i32 + 64, pm.height() as i32 + 64).unwrap_or(screen);
        let Some(r) = r.intersect(&outset) else { return };
        let Some(mut layer) = sk::Pixmap::new(r.width(), r.height()) else { return };
        let lt = base.post_translate(-(r.x() as f32), -(r.y() as f32));
        self.draw_content(doc, d, &mut layer, lt);

        if let Some(s) = &d.shadow {
            if let Some(sh) = blurred_tint(&layer, s.blur * scale, s.color.with_alpha(s.opacity)) {
                let pp = sk::PixmapPaint { opacity: d.opacity, ..Default::default() };
                // the object's bottom edge in the layer
                let bottom = {
                    let (_, hi) = g.bounds();
                    let mut p = [sk::Point::from_xy(0.0, hi.y)];
                    base.map_points(&mut p);
                    p[0].y - r.y() as f32
                };
                match s.kind {
                    // contact: the shape flattened onto the ground at its foot
                    1 => {
                        // (the transform places the layer itself: squashed toward its foot)
                        let k = s.height.clamp(0.02, 1.0);
                        let t = sk::Transform::from_row(1.0, 0.0, 0.0, k, r.x() as f32, r.y() as f32 + bottom * (1.0 - k) + s.offset * scale);
                        pm.draw_pixmap(0, 0, sh.as_ref(), &pp, t, None);
                    }
                    // curved: a band under the bottom edge that bends in from the corners
                    2 => {
                        let w = sh.width() as f32;
                        let band = (s.offset * scale).max(2.0) + s.blur * scale;
                        // the mask covers the whole target; the band sits under the layer's foot
                        if let Some(mut mask) = sk::Mask::new(pm.width(), pm.height()) {
                            let (ox, oy) = (r.x() as f32, r.y() as f32);
                            let mut pb = sk::PathBuilder::new();
                            let dip = band * (0.5 + s.curve.clamp(0.0, 1.0));
                            pb.move_to(ox, oy + bottom - 2.0);
                            pb.quad_to(ox + w / 2.0, oy + bottom + dip, ox + w, oy + bottom - 2.0);
                            pb.line_to(ox + w, oy + bottom + dip + band);
                            pb.line_to(ox, oy + bottom + dip + band);
                            pb.close();
                            if let Some(path) = pb.finish() {
                                mask.fill_path(&path, sk::FillRule::Winding, true, sk::Transform::identity());
                                pm.draw_pixmap(r.x(), r.y() + (band * 0.6) as i32, sh.as_ref(), &pp, sk::Transform::identity(), Some(&mask));
                            }
                        }
                    }
                    _ => {
                        let a = s.angle.to_radians();
                        let (dx, dy) = (s.offset * a.cos() * scale, -s.offset * a.sin() * scale);
                        pm.draw_pixmap(r.x() + dx.round() as i32, r.y() + dy.round() as i32, sh.as_ref(), &pp, sk::Transform::identity(), None);
                    }
                }
            }
        }
        if let Some(op) = d.reflection.filter(|r| *r > 0.0) {
            // mirrored below the object's bottom edge, fading out
            let bottom = {
                let (_, hi) = g.bounds();
                let mut p = [sk::Point::from_xy(0.0, hi.y)];
                base.map_points(&mut p);
                p[0].y - r.y() as f32
            };
            if let Some(mut refl) = sk::Pixmap::new(layer.width(), layer.height()) {
                let pp = sk::PixmapPaint { opacity: op, ..Default::default() };
                refl.draw_pixmap(0, 0, layer.as_ref(), &pp, sk::Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, 2.0 * bottom), None);
                let h = refl.height() as f32;
                let w = layer.width() as usize;
                for (i, row) in refl.pixels_mut().chunks_mut(w).enumerate() {
                    let y = i as f32;
                    let f = if y < bottom { 0.0 } else { (1.0 - (y - bottom) / (h - bottom).max(1.0) * 2.0).clamp(0.0, 1.0) };
                    for px in row {
                        let c = *px;
                        let s = |v: u8| (f32::from(v) * f) as u8;
                        *px = sk::PremultipliedColorU8::from_rgba(s(c.red()), s(c.green()), s(c.blue()), s(c.alpha())).unwrap_or(c);
                    }
                }
                let pp = sk::PixmapPaint { opacity: d.opacity, ..Default::default() };
                pm.draw_pixmap(r.x(), r.y(), refl.as_ref(), &pp, sk::Transform::identity(), None);
            }
        }
        let pp = sk::PixmapPaint { opacity: d.opacity, ..Default::default() };
        pm.draw_pixmap(r.x(), r.y(), layer.as_ref(), &pp, sk::Transform::identity(), None);
    }

    /// Draw a drawable without opacity/shadow/reflection.
    pub fn draw_content(&mut self, doc: &Document, d: &Drawable, pm: &mut sk::Pixmap, base: sk::Transform) {
        match &d.kind {
            DrawableKind::Shape(s) => self.draw_shape(doc, d, s, pm, base),
            DrawableKind::Image(img) => self.draw_image(doc, d, img, pm, base),
            DrawableKind::Movie(m) => self.draw_movie(doc, d, m, pm, base),
            DrawableKind::Group(children) => {
                let t = base.pre_concat(frame_transform(&d.geom, true));
                for c in children {
                    self.draw_drawable(doc, c, pm, t);
                }
            }
            DrawableKind::Table(t) => table::draw_table(self, doc, d, t, pm, base),
            DrawableKind::Chart(c) => chart::draw_chart(self, d, c, pm, base),
            DrawableKind::Unsupported(_) => {
                let t = base.pre_concat(frame_transform(&d.geom, true));
                if let Some(p) = rect_path(0.0, 0.0, d.geom.w, d.geom.h) {
                    pm.fill_path(&p, &solid(Color::rgba(0.5, 0.5, 0.5, 0.15)), sk::FillRule::Winding, t, None);
                }
            }
        }
    }

    fn draw_shape(&mut self, doc: &Document, d: &Drawable, s: &ShapeData, pm: &mut sk::Pixmap, base: sk::Transform) {
        let layout = self.text_layout(d);
        let g = layout.as_ref().map(|(_, g)| *g).unwrap_or(d.geom);
        let t = base.pre_concat(frame_transform(&g, true));
        // auto-sized text boxes use their effective frame as outline
        let path_src = if g != d.geom && !s.is_line { Path::rect(g.w, g.h) } else { s.path.clone() };
        if let Some(path) = sk_path(&path_src) {
            if !s.is_line {
                self.draw_fill(doc, pm, &s.fill, &path, g.w, g.h, t);
            }
            if let Some(st) = &s.stroke {
                let mut drawn = path.clone();
                if s.is_line && (s.head.is_some() || s.tail.is_some()) {
                    // the line stops at each end's base; the end draws the rest up to the point
                    let a = s.tail.as_ref().map(|le| st.width.max(1.0) * line_end_reach(le)).unwrap_or(0.0);
                    let b = s.head.as_ref().map(|le| st.width.max(1.0) * line_end_reach(le)).unwrap_or(0.0);
                    if let Some(p) = shorten(&path_src, a, b).and_then(|p| sk_path(&p)) {
                        drawn = p;
                    }
                }
                if st.brush.is_some() {
                    // an artistic brush (Keynote paints it with a texture): a mark about half the
                    // stated width with a rough, lighter fringe (R: "Dry Brush" boxes in a real deck)
                    let mut core = st.clone();
                    core.width = st.width * 0.5;
                    let mut fringe = st.clone();
                    fringe.width = st.width * 0.72;
                    pm.stroke_path(&drawn, &solid(st.color.with_alpha(0.35)), &sk_stroke(&fringe), t, None);
                    pm.stroke_path(&drawn, &solid(st.color), &sk_stroke(&core), t, None);
                } else {
                    pm.stroke_path(&drawn, &solid(st.color), &sk_stroke(st), t, None);
                }
                if s.is_line {
                    for (end, at_start) in [(&s.head, false), (&s.tail, true)] {
                        if let Some(le) = end {
                            draw_line_end(pm, &path_src, le, st, at_start, t);
                        }
                    }
                }
            }
        }
        if let Some((tl, _)) = layout {
            let tt = base.pre_concat(frame_transform(&g, false));
            self.draw_text(&tl, pm, tt);
        }
    }

    /// Draw laid-out text in frame-local space `t`.
    pub fn draw_text(&mut self, tl: &TextLayout, pm: &mut sk::Pixmap, t: sk::Transform) {
        // one band per run of touching boxes of a colour (per-letter boxes leave seams)
        let mut bands: Vec<(f32, f32, f32, f32, Color)> = Vec::new();
        let review: &[(f32, f32, f32, f32, Color)] = if self.review_marks { &tl.review } else { &[] };
        for &(x, y, w, h, c) in tl.highlights.iter().chain(review) {
            match bands.last_mut() {
                Some(b) if b.4 == c && (b.1 - y).abs() < 0.5 && (b.3 - h).abs() < 0.5 && (x - (b.0 + b.2)).abs() < 1.0 => b.2 = x + w - b.0,
                _ => bands.push((x, y, w, h, c)),
            }
        }
        for (x, y, w, h, c) in bands {
            if let Some(p) = rect_path(x, y, w, h) {
                pm.fill_path(&p, &solid(c), sk::FillRule::Winding, t, None);
            }
        }
        // batch glyphs of one colour into one path
        let mut by_color: Vec<(Color, sk::PathBuilder)> = Vec::new();
        for g in &tl.glyphs {
            let Some(cmds) = self.fonts.outline(g.font_id, g.glyph_id, g.weight) else { continue };
            let s = g.size / OUTLINE_SIZE;
            let idx = match by_color.iter().position(|(c, _)| *c == g.color) {
                Some(i) => i,
                None => {
                    by_color.push((g.color, sk::PathBuilder::new()));
                    by_color.len() - 1
                }
            };
            let b = &mut by_color[idx].1;
            let tx = |v: swash::zeno::Vector| (g.x + v.x * s, g.y - v.y * s);
            for c in cmds {
                use swash::zeno::Command;
                match *c {
                    Command::MoveTo(p) => {
                        let (x, y) = tx(p);
                        b.move_to(x, y)
                    }
                    Command::LineTo(p) => {
                        let (x, y) = tx(p);
                        b.line_to(x, y)
                    }
                    Command::QuadTo(c1, p) => {
                        let (x1, y1) = tx(c1);
                        let (x, y) = tx(p);
                        b.quad_to(x1, y1, x, y)
                    }
                    Command::CurveTo(c1, c2, p) => {
                        let (x1, y1) = tx(c1);
                        let (x2, y2) = tx(c2);
                        let (x, y) = tx(p);
                        b.cubic_to(x1, y1, x2, y2, x, y)
                    }
                    Command::Close => b.close(),
                }
            }
        }
        for (c, b) in by_color {
            if let Some(p) = b.finish() {
                pm.fill_path(&p, &solid(c), sk::FillRule::Winding, t, None);
            }
        }
        for dc in &tl.decos {
            if let Some(p) = rect_path(dc.x0, dc.y - dc.thickness / 2.0, dc.x1 - dc.x0, dc.thickness) {
                pm.fill_path(&p, &solid(dc.color), sk::FillRule::Winding, t, None);
            }
        }
    }

    fn draw_image(&mut self, doc: &Document, d: &Drawable, img: &ImageData, pm: &mut sk::Pixmap, base: sk::Transform) {
        let g = d.geom;
        let t = base.pre_concat(frame_transform(&g, true));
        let mut clip = None;
        let mut outline = Path::rect(g.w, g.h);
        let mut outline_t = t;
        if let Some((mg, mpath)) = &img.mask {
            let mt = t.pre_concat(frame_transform(mg, true));
            if let (Some(p), Some(mut m)) = (sk_path(mpath), sk::Mask::new(pm.width(), pm.height())) {
                m.fill_path(&p, sk::FillRule::Winding, true, mt);
                clip = Some(m);
            }
            outline = mpath.clone();
            outline_t = mt;
        }
        let Some(rect) = rect_path(0.0, 0.0, g.w, g.h) else { return };
        let px = img.data_id.and_then(|id| self.image(doc, id).map(|p| (id, p))).or_else(|| img.thumb_id.and_then(|id| self.image(doc, id).map(|p| (id, p))));
        match px {
            Some((id, px)) => {
                let (pw, ph) = (px.width() as f32, px.height() as f32);
                let pattern = match img.gallery.get(img.gallery_shown) {
                    // a gallery image fills its frame, then its own zoom and pan apply (Keynote)
                    Some(it) => {
                        let s = (g.w / pw).max(g.h / ph) * it.scale;
                        sk::Transform::from_row(s, 0.0, 0.0, s, g.w / 2.0 + it.offset.0 - pw * s / 2.0, g.h / 2.0 + it.offset.1 - ph * s / 2.0)
                    }
                    None => sk::Transform::from_scale(g.w / pw, g.h / ph),
                };
                let (px, f) = self.picture_for(id, px, draw_scale(t, pattern));
                let mut p = sk::Paint::default();
                p.anti_alias = true;
                p.shader = sk::Pattern::new(px.as_ref().as_ref(), sk::SpreadMode::Pad, sk::FilterQuality::Bilinear, 1.0, pattern.pre_scale(f, f));
                pm.fill_path(&rect, &p, sk::FillRule::Winding, t, clip.as_ref());
            }
            None if img.equation.is_some() => {
                // equation images are PDF; show the source until a TeX renderer exists
                let src = img.equation.clone().unwrap_or_default();
                let style = CharStyle { size: (g.h * 0.5).clamp(8.0, 48.0), ..Default::default() };
                let tb = simple_text_box(&src, style, Align::Center, VAlign::Middle);
                let tl = text::layout(&mut self.fonts, &tb, g.w, g.h, false, false);
                self.draw_text(&tl, pm, base.pre_concat(frame_transform(&g, false)));
            }
            None => {
                pm.fill_path(&rect, &solid(Color::rgba(0.85, 0.85, 0.87, 1.0)), sk::FillRule::Winding, t, clip.as_ref());
            }
        }
        if let Some(st) = &img.stroke {
            if let Some(p) = sk_path(&outline) {
                pm.stroke_path(&p, &solid(st.color), &sk_stroke(st), outline_t, None);
            }
        }
    }

    fn draw_movie(&mut self, doc: &Document, d: &Drawable, m: &MovieData, pm: &mut sk::Pixmap, base: sk::Transform) {
        let g = d.geom;
        let t = base.pre_concat(frame_transform(&g, true));
        let Some(rect) = rect_path(0.0, 0.0, g.w, g.h) else { return };
        match m.poster_data.and_then(|id| self.image(doc, id).map(|p| (id, p))) {
            Some((id, px)) => {
                let pattern = sk::Transform::from_scale(g.w / px.width() as f32, g.h / px.height() as f32);
                let (px, f) = self.picture_for(id, px, draw_scale(t, pattern));
                let mut p = sk::Paint::default();
                p.shader = sk::Pattern::new(px.as_ref().as_ref(), sk::SpreadMode::Pad, sk::FilterQuality::Bilinear, 1.0, pattern.pre_scale(f, f));
                pm.fill_path(&rect, &p, sk::FillRule::Winding, t, None);
            }
            None if m.audio_only => {
                // speaker glyph like Keynote's audio object
                let s = g.w.min(g.h);
                let mut b = sk::PathBuilder::new();
                b.move_to(s * 0.15, s * 0.38);
                b.line_to(s * 0.35, s * 0.38);
                b.line_to(s * 0.6, s * 0.15);
                b.line_to(s * 0.6, s * 0.85);
                b.line_to(s * 0.35, s * 0.62);
                b.line_to(s * 0.15, s * 0.62);
                b.close();
                if let Some(p) = b.finish() {
                    pm.fill_path(&p, &solid(Color::rgba(0.3, 0.3, 0.32, 1.0)), sk::FillRule::Winding, t, None);
                }
            }
            // live video / movies without a poster frame: nothing in exports (the app draws a placeholder)
            None => {}
        }
    }
}

/// A one-paragraph text box with a single style (labels, table cells, chart text).
pub fn simple_text_box(text: &str, style: CharStyle, align: Align, valign: VAlign) -> TextBox {
    TextBox {
        storage: 0,
        highlights: Vec::new(),
        fields: Vec::new(),
        text: text.to_string(),
        paragraphs: vec![Paragraph {
            start: 0,
            end: text.len(),
            runs: vec![Run { start: 0, end: text.len(), style }],
            align,
            line_spacing: LineSpacing::Relative(1.0),
            space_before: 0.0,
            space_after: 0.0,
            left_indent: 0.0,
            right_indent: 0.0,
            first_line_indent: 0.0,
            level: 0,
            label: None,
            list_indent: 0.0,
            label_indent: 0.0,
            label_color: None,
            label_scale: 1.0,
            first_text_indent: None,
            tabs: Vec::new(),
            default_tab: 0.0,
            rtl: false,
        }],
        valign,
        padding: [4.0; 4],
        shrink_to_fit: false,
        columns: 1,
        column_gap: 0.0,
        autogrow: false,
    }
}

fn stroke_margin(d: &Drawable) -> f32 {
    match &d.kind {
        DrawableKind::Shape(s) => s.stroke.as_ref().map(|s| s.width * 4.0).unwrap_or(0.0),
        DrawableKind::Image(i) => i.stroke.as_ref().map(|s| s.width).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// End point and outward direction of an open path (start or end).
fn path_end(p: &Path, at_start: bool) -> Option<(Point, Point)> {
    let mut pts: Vec<Point> = Vec::new();
    for e in &p.els {
        match *e {
            PathEl::Move(q) | PathEl::Line(q) => pts.push(q),
            PathEl::Quad(a, q) => {
                pts.push(a);
                pts.push(q)
            }
            PathEl::Cubic(a, b, q) => {
                pts.push(a);
                pts.push(b);
                pts.push(q)
            }
            PathEl::Close => {}
        }
    }
    if pts.len() < 2 {
        return None;
    }
    let (tip, prev) = if at_start {
        (pts[0], pts[1..].iter().find(|q| **q != pts[0]).copied()?)
    } else {
        let last = *pts.last().unwrap();
        (last, pts.iter().rev().find(|q| **q != last).copied()?)
    };
    let (dx, dy) = (tip.x - prev.x, tip.y - prev.y);
    let l = (dx * dx + dy * dy).sqrt().max(1e-6);
    Some((tip, Point::new(dx / l, dy / l)))
}

/// Pull the ends of an open path in by the given distances (so arrowheads cover the tip).
fn shorten(p: &Path, start: f32, end: f32) -> Option<Path> {
    let mut out = p.clone();
    if start > 0.0 {
        let (tip, dir) = path_end(p, true)?;
        if let Some(PathEl::Move(q)) = out.els.first_mut() {
            if *q == tip {
                *q = Point::new(tip.x - dir.x * start, tip.y - dir.y * start);
            }
        }
    }
    if end > 0.0 {
        let (tip, dir) = path_end(p, false)?;
        let nt = Point::new(tip.x - dir.x * end, tip.y - dir.y * end);
        if let Some(last) = out.els.iter_mut().rev().find(|e| !matches!(e, PathEl::Close)) {
            match last {
                PathEl::Line(q) | PathEl::Move(q) | PathEl::Quad(_, q) | PathEl::Cubic(_, _, q) => *q = nt,
                PathEl::Close => {}
            }
        }
    }
    Some(out)
}

/// A line end's shape (Keynote's default arrow when none is stored) and its attach point.
fn line_end_shape(le: &LineEnd) -> (Path, Point) {
    if le.path.els.is_empty() {
        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.line_to(3.0, 6.0);
        p.line_to(6.0, 0.0);
        p.close();
        (p, Point::new(3.0, 0.0))
    } else {
        (le.path.clone(), le.end_point)
    }
}

/// How far the end's point reaches beyond its attach point, in line-end units (Keynote's arrows
/// are drawn base at y = 0, point towards +y: "simple arrow" 6, "open arrow" 5).
fn line_end_reach(le: &LineEnd) -> f32 {
    let (shape, ep) = line_end_shape(le);
    let max_y = shape
        .els
        .iter()
        .filter_map(|e| match e {
            PathEl::Move(q) | PathEl::Line(q) => Some(q.y),
            PathEl::Quad(_, q) | PathEl::Cubic(_, _, q) => Some(q.y),
            PathEl::Close => None,
        })
        .fold(ep.y, f32::max);
    (max_y - ep.y).max(0.0)
}

fn draw_line_end(pm: &mut sk::Pixmap, line: &Path, le: &LineEnd, st: &Stroke, at_start: bool, t: sk::Transform) {
    let Some((tip, dir)) = path_end(line, at_start) else { return };
    let (shape, ep) = line_end_shape(le);
    let reach = line_end_reach(le);
    // line-end space: +y points out of the line; the end's point goes on the line's end
    let phi = (-dir.x).atan2(dir.y).to_degrees();
    let w = st.width.max(1.0);
    let lt = t.pre_translate(tip.x, tip.y).pre_rotate(phi).pre_scale(w, w).pre_translate(-ep.x, -(ep.y + reach));
    let Some(p) = sk_path(&shape) else { return };
    let paint = solid(st.color);
    if le.filled {
        pm.fill_path(&p, &paint, sk::FillRule::Winding, lt, None);
    } else {
        let s = sk::Stroke { width: 1.0, line_join: sk::LineJoin::Round, line_cap: sk::LineCap::Round, ..Default::default() };
        pm.stroke_path(&p, &paint, &s, lt, None);
    }
}
