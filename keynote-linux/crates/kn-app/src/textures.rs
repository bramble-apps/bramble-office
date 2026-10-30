//! GPU textures of rendered slides: navigator thumbnails, the editing canvas, drag layers.
//!
//! Rendering happens on the UI thread but is (a) cached per slide revision, (b) time-budgeted for
//! thumbnails, and (c) panic-guarded so one bad slide can never take the app down.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use egui::{ColorImage, TextureHandle, TextureOptions};
use kn_format::ObjId;
use kn_model::{Document, SlideScene};
use kn_render::{tiny_skia as sk, Filter, Renderer};

pub fn color_image(pm: &sk::Pixmap) -> ColorImage {
    ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data())
}

/// Run `f`, turning a panic into `None` (and a log line) instead of a crash.
pub fn guarded<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Some(v),
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            eprintln!("render error ({what}): {msg}");
            None
        }
    }
}

fn error_pixmap(w: u32, h: u32) -> sk::Pixmap {
    let mut pm = sk::Pixmap::new(w.max(1), h.max(1)).unwrap();
    pm.fill(sk::Color::from_rgba8(80, 30, 30, 255));
    pm
}

#[derive(Clone, PartialEq)]
pub struct CanvasKey {
    pub slide: ObjId,
    pub rev: u64,
    pub width: u32,
    pub exclude: Vec<ObjId>,
}

#[derive(Default)]
pub struct Textures {
    thumbs: HashMap<ObjId, (TextureHandle, u64)>,
    /// Thumbnails drawn while some of their pictures were still being decoded.
    incomplete: std::collections::HashSet<ObjId>,
    canvas_incomplete: bool,
    canvas: Option<(CanvasKey, TextureHandle)>,
    /// Slides shown recently (going back and forth doesn't render them again).
    recent: Vec<(CanvasKey, TextureHandle)>,
    layer: Option<(CanvasKey, TextureHandle, egui::Rect)>,
    offslide: Option<(CanvasKey, TextureHandle, egui::Rect)>,
    /// A small blurred picture of the slide for the toolbar's edge: (slide, rev).
    blurred: Option<((ObjId, u64), TextureHandle)>,
    /// A screen-space blur of the strip under the toolbar (Pages' tinted look):
    /// (slide, rev, region in slide points x1000, pixel size), the picture.
    band_blur: Option<((ObjId, u64, [i64; 4], [u32; 2]), TextureHandle)>,
    /// While the slide's on-screen size keeps changing (panel resize, pinch zoom): the width
    /// asked for last and when; the old picture is shown scaled until it settles.
    resizing: Option<(u32, Instant)>,
    /// Zooming never draws on the UI thread: new sizes of the slide, and sharp pictures of the
    /// part on screen when zoomed in far, come from this thread (the old picture shows scaled
    /// meanwhile).
    zoom: Option<ZoomWorker>,
    /// The document as the zoom thread sees it, for (slide, revision).
    snap: Option<((ObjId, u64), std::sync::Arc<Document>)>,
    /// The whole-slide size asked of the zoom thread.
    full_asked: Option<CanvasKey>,
    /// The sharp picture of part of the slide (zoomed in), its area in slide points; what was
    /// asked for last, and since when the view has been where it is now.
    detail: Option<(CanvasKey, egui::Rect, TextureHandle)>,
    detail_asked: Option<(CanvasKey, egui::Rect)>,
    detail_since: Option<(CanvasKey, egui::Rect, Instant)>,
}

/// Widest whole-slide picture (pixels); zoomed in further, the part on screen is drawn sharp
/// on top of it.
pub const FULL_MAX: u32 = 2600;

/// What the zoom thread draws: the area (slide points) of a slide at `scale` pixels per point.
struct ZoomJob {
    doc: std::sync::Arc<Document>,
    scene: SlideScene,
    key: CanvasKey,
    area: egui::Rect,
    scale: f32,
    detail: bool,
}

type ZoomOut = (CanvasKey, egui::Rect, bool, ColorImage);

struct ZoomWorker {
    req: std::sync::Arc<(std::sync::Mutex<Option<ZoomJob>>, std::sync::Condvar)>,
    out: std::sync::Arc<std::sync::Mutex<Vec<ZoomOut>>>,
}

impl ZoomWorker {
    fn new(ctx: &egui::Context) -> ZoomWorker {
        let req: std::sync::Arc<(std::sync::Mutex<Option<ZoomJob>>, std::sync::Condvar)> = Default::default();
        let out: std::sync::Arc<std::sync::Mutex<Vec<ZoomOut>>> = Default::default();
        let (r2, o2, ctx) = (req.clone(), out.clone(), ctx.clone());
        std::thread::spawn(move || {
            // its own renderer: pictures it decoded stay decoded for the next zoom (the editing
            // canvas: review highlights show)
            let mut r = Renderer::new();
            r.review_marks = true;
            loop {
                let job = {
                    let (m, cv) = &*r2;
                    let Ok(mut g) = m.lock() else { return };
                    while g.is_none() {
                        g = match cv.wait(g) {
                            Ok(g) => g,
                            Err(_) => return,
                        };
                    }
                    g.take().unwrap()
                };
                let (w, h) = ((job.area.width() * job.scale).round().max(1.0) as u32, (job.area.height() * job.scale).round().max(1.0) as u32);
                let img = guarded("zoom", || {
                    let mut pm = sk::Pixmap::new(w, h)?;
                    let t = sk::Transform::from_scale(job.scale, job.scale).post_translate(-job.area.min.x * job.scale, -job.area.min.y * job.scale);
                    r.draw_scene(&job.doc, &job.scene, &mut pm, t, Filter::Except(&job.key.exclude), job.scene.motion.is_none());
                    Some(color_image(&pm))
                })
                .flatten();
                if let (Some(img), Ok(mut o)) = (img, o2.lock()) {
                    o.push((job.key, job.area, job.detail, img));
                }
                ctx.request_repaint();
            }
        });
        ZoomWorker { req, out }
    }

    /// Ask for a picture (replacing one asked for but not yet started).
    fn ask(&self, job: ZoomJob) {
        let (m, cv) = &*self.req;
        if let Ok(mut g) = m.lock() {
            *g = Some(job);
            cv.notify_one();
        }
    }
}

pub const THUMB_W: u32 = 320;

impl Textures {
    pub fn clear(&mut self) {
        *self = Textures::default();
    }

    /// Mark everything stale but keep showing the old textures until re-rendered
    /// (used when background-decoded images arrive).
    pub fn invalidate(&mut self) {
        // only pictures drawn with images missing (complete ones, e.g. from the thumbnail
        // worker, stay: redrawing them evicted other images and kept the cycle going)
        for (slide, (_, rev)) in self.thumbs.iter_mut() {
            if self.incomplete.contains(slide) {
                *rev = u64::MAX;
            }
        }
        self.incomplete.clear();
        if self.canvas_incomplete {
            self.canvas = None;
        }
        self.layer = None;
        self.offslide = None;
    }

    /// Thumbnail texture for a slide if it's up to date; renders it if `budget` allows.
    /// A finished, current thumbnail (no rendering).
    pub fn cached_thumb(&self, slide: ObjId, rev: u64) -> Option<TextureHandle> {
        self.thumbs.get(&slide).filter(|(_, r)| *r == rev).map(|(t, _)| t.clone())
    }

    /// Any thumbnail of the slide, even an outdated one.
    pub fn any_thumb(&self, slide: ObjId) -> Option<TextureHandle> {
        self.thumbs.get(&slide).map(|(t, _)| t.clone())
    }

    /// A thumbnail rendered elsewhere (the background thumbnail worker).
    pub fn put_thumb(&mut self, ctx: &egui::Context, slide: ObjId, rev: u64, img: ColorImage) {
        let tex = ctx.load_texture(format!("thumb-{slide}"), img, TextureOptions::LINEAR);
        self.thumbs.insert(slide, (tex, rev));
        self.incomplete.remove(&slide);
    }

    pub fn thumb(
        &mut self,
        ctx: &egui::Context,
        r: &mut Renderer,
        doc: &Document,
        scene: impl FnOnce() -> Option<SlideScene>,
        slide: ObjId,
        rev: u64,
        budget: &mut Budget,
    ) -> Option<TextureHandle> {
        if let Some((t, rv)) = self.thumbs.get(&slide) {
            if *rv == rev {
                return Some(t.clone());
            }
            if !budget.left() {
                return Some(t.clone()); // stale is better than empty
            }
        }
        if !budget.left() {
            return None;
        }
        let start = Instant::now();
        r.take_missing();
        let pm = scene()
            .and_then(|sc| guarded("thumbnail", || r.render_scene(doc, &sc, THUMB_W)).flatten())
            .unwrap_or_else(|| error_pixmap(THUMB_W, THUMB_W * 9 / 16));
        if r.take_missing() > 0 {
            self.incomplete.insert(slide);
        } else {
            self.incomplete.remove(&slide);
        }
        budget.spend(start.elapsed());
        let tex = ctx.load_texture(format!("thumb-{slide}"), color_image(&pm), TextureOptions::LINEAR);
        self.thumbs.insert(slide, (tex.clone(), rev));
        Some(tex)
    }

    /// The canvas texture: slide (minus `key.exclude`) at `key.width` pixels.
    pub fn canvas(&mut self, ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, mut key: CanvasKey) -> TextureHandle {
        const SETTLE: Duration = Duration::from_millis(90);
        key.width = key.width.min(FULL_MAX);
        self.poll(ctx);
        if let Some((k, t)) = &self.canvas {
            if *k == key {
                self.resizing = None;
                return t.clone();
            }
            // only the size differs: keep the old picture (scaled) and have the zoom thread draw
            // the new size once the size stops changing
            if k.slide == key.slide && k.rev == key.rev && k.exclude == key.exclude {
                let t = t.clone();
                let settled = matches!(self.resizing, Some((w, at)) if w == key.width && at.elapsed() >= SETTLE);
                if self.resizing.map(|(w, _)| w) != Some(key.width) {
                    self.resizing = Some((key.width, Instant::now()));
                }
                if settled && self.full_asked.as_ref() != Some(&key) {
                    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(scene.size.w, scene.size.h));
                    let scale = key.width as f32 / scene.size.w;
                    self.ask_zoom(ctx, doc, scene, key.clone(), area, scale, false);
                    self.full_asked = Some(key);
                } else if !settled {
                    ctx.request_repaint_after(SETTLE);
                }
                return t;
            }
        }
        self.resizing = None;
        if let Some(i) = self.recent.iter().position(|(k, _)| *k == key) {
            let (k, t) = self.recent.remove(i);
            if let Some(old) = self.canvas.replace((k, t.clone())) {
                self.keep(old);
            }
            self.canvas_incomplete = false;
            return t;
        }
        let w = key.width.max(16);
        r.take_missing();
        // review highlights show on the editing canvas only (this renderer also draws
        // thumbnails and exports)
        r.review_marks = true;
        let pm = guarded("canvas", || {
            let scale = w as f32 / scene.size.w;
            let h = (scene.size.h * scale).round().max(1.0) as u32;
            let mut pm = sk::Pixmap::new(w, h)?;
            // a dynamic background is drawn live underneath
            r.draw_scene(doc, scene, &mut pm, sk::Transform::from_scale(scale, scale), Filter::Except(&key.exclude), scene.motion.is_none());
            Some(pm)
        })
        .flatten()
        .unwrap_or_else(|| error_pixmap(w, w * 9 / 16));
        r.review_marks = false;
        let tex = ctx.load_texture("canvas", color_image(&pm), TextureOptions::LINEAR);
        if let Some(old) = self.canvas.replace((key, tex.clone())) {
            self.keep(old);
        }
        self.canvas_incomplete = r.take_missing() > 0;
        tex
    }

    /// Take what the zoom thread finished.
    fn poll(&mut self, ctx: &egui::Context) {
        let done: Vec<ZoomOut> = self.zoom.as_ref().and_then(|z| z.out.lock().ok().map(|mut o| std::mem::take(&mut *o))).unwrap_or_default();
        for (key, area, detail, img) in done {
            if detail {
                if self.detail_asked.as_ref().is_some_and(|(k, a)| *k == key && *a == area) {
                    let tex = ctx.load_texture("canvas-detail", img, TextureOptions::LINEAR);
                    self.detail = Some((key, area, tex));
                }
            } else if self.full_asked.as_ref() == Some(&key) && self.canvas.as_ref().is_some_and(|(k, _)| k.slide == key.slide && k.rev == key.rev && k.exclude == key.exclude) {
                let tex = ctx.load_texture("canvas", img, TextureOptions::LINEAR);
                if let Some(old) = self.canvas.replace((key, tex)) {
                    self.keep(old);
                }
                self.full_asked = None;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ask_zoom(&mut self, ctx: &egui::Context, doc: &Document, scene: &SlideScene, key: CanvasKey, area: egui::Rect, scale: f32, detail: bool) {
        let at = (key.slide, key.rev);
        if self.snap.as_ref().map(|s| s.0) != Some(at) {
            self.snap = Some((at, std::sync::Arc::new(Document::from_file(doc.file.clone()))));
        }
        let Some((_, d)) = &self.snap else { return };
        let job = ZoomJob { doc: d.clone(), scene: scene.clone(), key, area, scale, detail };
        self.zoom.get_or_insert_with(|| ZoomWorker::new(ctx)).ask(job);
    }

    /// Zoomed in past [`FULL_MAX`]: a sharp picture of the part of the slide in `visible`
    /// (slide points) and around it, drawn by the zoom thread once the view rests; the last one
    /// (even at another size) meanwhile. `key.width` is the slide's full width on screen.
    pub fn detail(&mut self, ctx: &egui::Context, doc: &Document, scene: &SlideScene, key: CanvasKey, visible: egui::Rect) -> Option<(TextureHandle, egui::Rect)> {
        const SETTLE: Duration = Duration::from_millis(110);
        self.poll(ctx);
        let same = |k: &CanvasKey| k.slide == key.slide && k.rev == key.rev && k.exclude == key.exclude;
        if key.width <= FULL_MAX || visible.width() <= 0.0 || visible.height() <= 0.0 {
            self.detail = None;
            self.detail_since = None;
            return None;
        }
        let current = self.detail.as_ref().filter(|(k, _, _)| same(k)).map(|(_, a, t)| (t.clone(), *a));
        let fresh = self.detail.as_ref().is_some_and(|(k, a, _)| *k == key && a.contains_rect(visible));
        if !fresh {
            // the view has to rest a moment before a new picture is asked for
            let resting = match &self.detail_since {
                Some((k, v, at)) if *k == key && (v.min - visible.min).length() < 1.0 && (v.size() - visible.size()).length() < 1.0 => at.elapsed() >= SETTLE,
                _ => {
                    self.detail_since = Some((key.clone(), visible, Instant::now()));
                    false
                }
            };
            let asked = self.detail_asked.as_ref().is_some_and(|(k, a)| *k == key && a.contains_rect(visible));
            if resting && !asked {
                let slide = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(scene.size.w, scene.size.h));
                let area = visible.expand2(visible.size() * 0.3).intersect(slide);
                let scale = key.width as f32 / scene.size.w;
                // (at most about 16 megapixels)
                let scale = scale.min((16.0e6 / (area.width() * area.height()).max(1.0)).sqrt());
                self.detail_asked = Some((key.clone(), area));
                self.ask_zoom(ctx, doc, scene, key, area, scale, true);
            } else if !resting {
                ctx.request_repaint_after(SETTLE);
            }
        }
        current
    }

    /// The slide, small and blurred, with a slide's size of room round it (what hangs off it,
    /// faded): what shows through under the floating toolbar. The texture covers the slide's
    /// rectangle grown by its own size on every side.
    pub fn blurred(&mut self, ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, slide: ObjId, rev: u64) -> TextureHandle {
        if let Some((k, t)) = &self.blurred {
            if *k == (slide, rev) {
                return t.clone();
            }
        }
        const W: u32 = 320;
        let scale = W as f32 / scene.size.w;
        let h = (scene.size.h * scale).round().max(1.0) as u32;
        // small enough to decode its pictures at once (they'd come at the canvas's size)
        let was_async = r.async_images();
        r.set_async_images(false);
        // a slide's size of room on every side: what hangs off the slide passes under the
        // toolbar too, faded as the canvas shows it (25%), and must blur with the rest
        let img = guarded("blur", || {
            let (fw, fh) = (W * 3, h * 3);
            let mut pm = sk::Pixmap::new(fw, fh)?;
            let mut off = sk::Pixmap::new(fw, fh)?;
            let t = sk::Transform::from_scale(scale, scale).post_translate(W as f32, h as f32);
            for d in &scene.drawables {
                r.draw_drawable(doc, d, &mut off, t);
            }
            let faded = sk::PixmapPaint { opacity: 0.25, ..Default::default() };
            pm.draw_pixmap(0, 0, off.as_ref(), &faded, sk::Transform::identity(), None);
            let mut sl = sk::Pixmap::new(W, h)?;
            r.draw_scene(doc, scene, &mut sl, sk::Transform::from_scale(scale, scale), Filter::Except(&[]), true);
            pm.draw_pixmap(W as i32, h as i32, sl.as_ref(), &sk::PixmapPaint::default(), sk::Transform::identity(), None);
            Some(crate::glass::box_blur(&color_image(&pm), 3, 3))
        })
        .flatten()
        .unwrap_or_else(|| ColorImage::new([1, 1], vec![egui::Color32::TRANSPARENT]));
        r.set_async_images(was_async);
        let tex = ctx.load_texture("canvas-blur", img, TextureOptions::LINEAR);
        self.blurred = Some(((slide, rev), tex.clone()));
        tex
    }

    /// The part `region` (slide points; may reach off the slide) blurred at `px` pixels, with a
    /// 3-pass box blur of `radius` pixels: the slide as shown, what hangs off it faded (25 %),
    /// transparent elsewhere. Made again only when the region, size or slide changes.
    #[allow(clippy::too_many_arguments)]
    pub fn band_blur(&mut self, ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, slide: ObjId, rev: u64, region: egui::Rect, px: [u32; 2], radius: usize) -> TextureHandle {
        let q = |v: f32| (v * 1000.0).round() as i64;
        let key = (slide, rev, [q(region.min.x), q(region.min.y), q(region.max.x), q(region.max.y)], px);
        if let Some((k, t)) = &self.band_blur {
            if *k == key {
                return t.clone();
            }
        }
        let [fw, fh] = [px[0].max(1), px[1].max(1)];
        let s = fw as f32 / region.width().max(1e-3);
        let was_async = r.async_images();
        r.set_async_images(false);
        let img = guarded("band blur", || {
            let mut pm = sk::Pixmap::new(fw, fh)?;
            let mut off = sk::Pixmap::new(fw, fh)?;
            let t = sk::Transform::from_scale(s, s).post_translate(-region.min.x * s, -region.min.y * s);
            for d in &scene.drawables {
                r.draw_drawable(doc, d, &mut off, t);
            }
            pm.draw_pixmap(0, 0, off.as_ref(), &sk::PixmapPaint { opacity: 0.25, ..Default::default() }, sk::Transform::identity(), None);
            // the slide itself, only the part inside the region (drawn on its own pixmap, as its
            // background fills the whole of one)
            let x0 = ((0.0 - region.min.x) * s).round().max(0.0) as i32;
            let y0 = ((0.0 - region.min.y) * s).round().max(0.0) as i32;
            let x1 = (((scene.size.w - region.min.x) * s).round() as i32).min(fw as i32);
            let y1 = (((scene.size.h - region.min.y) * s).round() as i32).min(fh as i32);
            if x1 > x0 && y1 > y0 {
                let mut sl = sk::Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32)?;
                let st = sk::Transform::from_scale(s, s).post_translate(-region.min.x * s - x0 as f32, -region.min.y * s - y0 as f32);
                r.draw_scene(doc, scene, &mut sl, st, Filter::Except(&[]), true);
                pm.draw_pixmap(x0, y0, sl.as_ref(), &sk::PixmapPaint::default(), sk::Transform::identity(), None);
            }
            Some(crate::glass::box_blur(&color_image(&pm), radius, 3))
        })
        .flatten()
        .unwrap_or_else(|| ColorImage::new([1, 1], vec![egui::Color32::TRANSPARENT]));
        r.set_async_images(was_async);
        let tex = ctx.load_texture("band-blur", img, TextureOptions::LINEAR);
        self.band_blur = Some((key, tex.clone()));
        tex
    }

    /// Remember a slide picture for a quick return (not while objects are being dragged).
    fn keep(&mut self, entry: (CanvasKey, TextureHandle)) {
        // (one drawn with pictures missing is not worth keeping)
        if !entry.0.exclude.is_empty() || self.canvas_incomplete {
            return;
        }
        // a newer revision or size of the same slide replaces older ones
        self.recent.retain(|(k, _)| k.slide != entry.0.slide);
        self.recent.push(entry);
        // full-size slide pictures are big: a handful is enough
        if self.recent.len() > 5 {
            let _ = self.recent.remove(0);
        }
    }

    /// Transparent layer with only the given objects (used while dragging them). Only the area
    /// around those objects is rendered; the second value is that area as a fraction of the slide.
    pub fn layer(&mut self, ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, mut key: CanvasKey) -> (TextureHandle, egui::Rect) {
        key.width = key.width.min(FULL_MAX);
        if let Some((k, t, uv)) = &self.layer {
            // (a new size alone keeps the old picture, scaled, while zooming)
            if *k == key || (k.slide == key.slide && k.rev == key.rev && k.exclude == key.exclude && self.resizing.is_some()) {
                return (t.clone(), *uv);
            }
        }
        let (tex, uv) = self.render_layer(ctx, r, doc, scene, &key);
        self.layer = Some((key, tex.clone(), uv));
        (tex, uv)
    }

    /// The same for the unselected objects that hang off the slide (shown faded around it).
    pub fn offslide_layer(&mut self, ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, mut key: CanvasKey) -> (TextureHandle, egui::Rect) {
        key.width = key.width.min(FULL_MAX);
        if let Some((k, t, uv)) = &self.offslide {
            if *k == key || (k.slide == key.slide && k.rev == key.rev && k.exclude == key.exclude && self.resizing.is_some()) {
                return (t.clone(), *uv);
            }
        }
        let (tex, uv) = self.render_layer(ctx, r, doc, scene, &key);
        self.offslide = Some((key, tex.clone(), uv));
        (tex, uv)
    }

    fn render_layer(&mut self, ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, key: &CanvasKey) -> (TextureHandle, egui::Rect) {
        let w = key.width.max(16);
        let scale = w as f32 / scene.size.w;
        // the objects' bounds plus room for shadows, strokes and line ends, kept on the slide
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for d in scene.drawables.iter().filter(|d| key.exclude.contains(&d.id)) {
            let (a, b) = r.effective_geometry(d).bounds();
            x0 = x0.min(a.x);
            y0 = y0.min(a.y);
            x1 = x1.max(b.x);
            y1 = y1.max(b.y);
        }
        // (not cut at the slide's edges: what hangs off the slide shows while it moves; kept
        // within a slide's size around it)
        const MARGIN: f32 = 120.0;
        let (sw, sh) = (scene.size.w, scene.size.h);
        let (x0, y0) = ((x0 - MARGIN).max(-sw), (y0 - MARGIN).max(-sh));
        let (x1, y1) = ((x1 + MARGIN).min(2.0 * sw), (y1 + MARGIN).min(2.0 * sh));
        let (x0, y0, x1, y1) = if x0.is_finite() && x1 > x0 && y1 > y0 { (x0, y0, x1, y1) } else { (0.0, 0.0, 1.0, 1.0) };
        // keep the picture a sensible size
        let scale = scale.min(4096.0 / (x1 - x0).max(1.0)).min(4096.0 / (y1 - y0).max(1.0));
        let (pw, ph) = (((x1 - x0) * scale).ceil().max(1.0) as u32, ((y1 - y0) * scale).ceil().max(1.0) as u32);
        let uv = egui::Rect::from_min_max(
            egui::pos2(x0 / scene.size.w, y0 / scene.size.h),
            egui::pos2((x0 + pw as f32 / scale) / scene.size.w, (y0 + ph as f32 / scale) / scene.size.h),
        );
        let pm = guarded("layer", || {
            let mut pm = sk::Pixmap::new(pw, ph)?;
            let base = sk::Transform::from_scale(scale, scale).post_translate(-x0 * scale, -y0 * scale);
            for d in scene.drawables.iter().filter(|d| key.exclude.contains(&d.id)) {
                r.draw_drawable(doc, d, &mut pm, base);
            }
            Some(pm)
        })
        .flatten()
        .unwrap_or_else(|| error_pixmap(pw, ph));
        let tex = ctx.load_texture("layer", color_image(&pm), TextureOptions::LINEAR);
        (tex, uv)
    }
}

/// The animated texture of a dynamic background. Frames are rendered on a background thread
/// (the newest request wins), so the show never waits for one; the texture shows the newest
/// finished frame. Only the very first frame of a background is rendered here, small.
#[derive(Default)]
pub struct MotionTex {
    key: Option<(kn_model::motionbg::MotionBg, u32, u32, u32)>,
    tex: Option<TextureHandle>,
    worker: Option<MotionWorker>,
    /// First frames of backgrounds coming up next (rendered ahead by [`MotionTex::prime`]).
    primed: Vec<(kn_model::motionbg::MotionBg, u32, u32, ColorImage)>,
    /// The background asked for ahead (asked once).
    priming: Option<kn_model::motionbg::MotionBg>,
    /// The frame now shown (kept to freeze it under an outgoing slide).
    last: Option<std::sync::Arc<ColorImage>>,
}

type MotionReq = (kn_model::motionbg::MotionBg, f32, u32, u32);

struct MotionWorker {
    req: std::sync::Arc<(std::sync::Mutex<Option<MotionReq>>, std::sync::Condvar)>,
    out: std::sync::Arc<std::sync::Mutex<Option<(MotionReq, Vec<u8>)>>>,
}

impl MotionWorker {
    fn new(ctx: &egui::Context) -> MotionWorker {
        let req: std::sync::Arc<(std::sync::Mutex<Option<MotionReq>>, std::sync::Condvar)> = Default::default();
        let out: std::sync::Arc<std::sync::Mutex<Option<(MotionReq, Vec<u8>)>>> = Default::default();
        let (r2, o2, ctx) = (req.clone(), out.clone(), ctx.clone());
        std::thread::spawn(move || loop {
            let job = {
                let (m, cv) = &*r2;
                let mut g = m.lock().unwrap();
                while g.is_none() {
                    g = cv.wait(g).unwrap();
                }
                g.take().unwrap()
            };
            let px = kn_render::motion::render(&job.0, job.1, job.2, job.3);
            *o2.lock().unwrap() = Some((job, px));
            ctx.request_repaint();
        });
        MotionWorker { req, out }
    }
}

impl MotionTex {
    /// Texture of `bg` at about time `t`, `w` x `h` pixels (scaled up smoothly when drawn).
    pub fn get(&mut self, ctx: &egui::Context, bg: &kn_model::motionbg::MotionBg, t: f32, w: u32, h: u32) -> TextureHandle {
        let frame = (t * 60.0) as u32;
        let worker = self.worker.get_or_insert_with(|| MotionWorker::new(ctx));
        // ask for this frame (replacing an older request still waiting)
        if !self.key.as_ref().is_some_and(|k| k.0.looks_same(bg) && (k.1, k.2, k.3) == (frame, w, h)) {
            let (m, cv) = &*worker.req;
            *m.lock().unwrap() = Some((bg.clone(), frame as f32 / 60.0, w, h));
            cv.notify_one();
        }
        // the newest finished frame of this background (or one rendered ahead for another)
        let done = worker.out.lock().unwrap().take();
        if let Some(((obg, ot, ow, oh), px)) = done {
            if obg.looks_same(bg) {
                self.upload(ctx, &px, ow, oh);
                self.key = Some((obg, (ot * 60.0) as u32, ow, oh));
            } else {
                self.priming = None;
                self.primed.retain(|p| !p.0.looks_same(&obg));
                self.primed.push((obg, ow, oh, egui::ColorImage::from_rgba_unmultiplied([ow as usize, oh as usize], &px)));
                self.primed.truncate(4);
            }
        }
        if let (Some(tex), Some(k)) = (&self.tex, &self.key) {
            if k.0.looks_same(bg) {
                return tex.clone();
            }
        }
        // a new background: its frame rendered ahead, else a full one now (a small stand-in
        // first showed as a still that then jumped)
        if let Some(i) = self.primed.iter().position(|p| p.0.looks_same(bg)) {
            let (pbg, pw, ph, img) = self.primed.remove(i);
            self.set_image(ctx, img);
            self.key = Some((pbg, frame, pw, ph));
            return self.tex.clone().unwrap();
        }
        let px = kn_render::motion::render(bg, frame as f32 / 60.0, w, h);
        self.upload(ctx, &px, w, h);
        self.key = Some((bg.clone(), frame, w, h));
        self.tex.clone().unwrap()
    }

    /// Render `bg`'s frame at `t` ahead (on the worker), at the size last asked for, for a slide
    /// about to be shown.
    pub fn prime(&mut self, ctx: &egui::Context, bg: &kn_model::motionbg::MotionBg, t: f32) {
        let Some((cur, _, w, h)) = &self.key else { return };
        if cur.looks_same(bg) || self.primed.iter().any(|p| p.0.looks_same(bg)) || self.priming.as_ref().is_some_and(|p| p.looks_same(bg)) {
            return;
        }
        self.priming = Some(bg.clone());
        let worker = self.worker.get_or_insert_with(|| MotionWorker::new(ctx));
        let (m, cv) = &*worker.req;
        *m.lock().unwrap() = Some((bg.clone(), t, *w, *h));
        cv.notify_one();
    }

    fn upload(&mut self, ctx: &egui::Context, px: &[u8], w: u32, h: u32) {
        self.set_image(ctx, egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], px));
    }

    fn set_image(&mut self, ctx: &egui::Context, img: ColorImage) {
        let img = std::sync::Arc::new(img);
        match &mut self.tex {
            Some(t) => t.set(egui::ImageData::Color(img.clone()), TextureOptions::LINEAR),
            None => self.tex = Some(ctx.load_texture("motion-bg", egui::ImageData::Color(img.clone()), TextureOptions::LINEAR)),
        }
        self.last = Some(img);
    }

    /// A still copy of the frame now shown, if it is of `bg`.
    pub fn freeze(&self, ctx: &egui::Context, bg: &kn_model::motionbg::MotionBg) -> Option<TextureHandle> {
        let k = self.key.as_ref()?;
        let img = self.last.as_ref().filter(|_| k.0.looks_same(bg))?;
        Some(ctx.load_texture("motion-bg-still", egui::ImageData::Color(img.clone()), TextureOptions::LINEAR))
    }
}

/// Per-frame time budget for background-ish work done on the UI thread.
pub struct Budget {
    left: Duration,
}

impl Budget {
    pub fn new(ms: u64) -> Budget {
        Budget { left: Duration::from_millis(ms) }
    }
    pub fn left(&self) -> bool {
        !self.left.is_zero()
    }
    pub fn spend(&mut self, d: Duration) {
        self.left = self.left.saturating_sub(d);
    }
}

/// Renders slide thumbnails on background threads from a copy of the document, so the navigator
/// has them all (nearly) at once instead of a few per frame. Slides edited after the copy was
/// made are rendered on the UI thread as before.
pub struct ThumbWorker {
    jobs: std::sync::Arc<std::sync::Mutex<Vec<(ObjId, usize)>>>,
    results: std::sync::Arc<std::sync::Mutex<Vec<(ObjId, ColorImage)>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Slide revisions when the copy was made.
    pub revs: HashMap<ObjId, u64>,
}

impl ThumbWorker {
    pub fn start(ctx: &egui::Context, file: kn_format::KeyFile, slides: Vec<kn_model::SlideInfo>, revs: HashMap<ObjId, u64>) -> ThumbWorker {
        use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};
        let doc = Arc::new(Document::from_file(file));
        let slides = Arc::new(slides);
        let jobs: Arc<Mutex<Vec<(ObjId, usize)>>> = Arc::new(Mutex::new(slides.iter().enumerate().map(|(i, s)| (s.slide, i)).collect()));
        let results: Arc<Mutex<Vec<(ObjId, ColorImage)>>> = Default::default();
        let stop = Arc::new(AtomicBool::new(false));
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8) / 2;
        for _ in 0..threads.max(1) {
            let (doc, slides, jobs, results, stop, ctx) = (doc.clone(), slides.clone(), jobs.clone(), results.clone(), stop.clone(), ctx.clone());
            std::thread::spawn(move || {
                let t0 = Instant::now();
                let mut r = Renderer::new();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let job = jobs.lock().ok().and_then(|mut j| if j.is_empty() { None } else { Some(j.remove(0)) });
                    let Some((slide, i)) = job else {
                        if std::env::var_os("KN_DEBUG").is_some() {
                            eprintln!("thumbnail worker done in {:?}", t0.elapsed());
                        }
                        return;
                    };
                    let pm = guarded("thumbnail", || doc.scene(&slides, i).and_then(|sc| r.render_scene(&doc, &sc, THUMB_W))).flatten();
                    if let Some(pm) = pm {
                        if let Ok(mut out) = results.lock() {
                            out.push((slide, color_image(&pm)));
                        }
                        ctx.request_repaint();
                    }
                }
            });
        }
        ThumbWorker { jobs, results, stop, revs }
    }

    /// Whether the slide is still waiting to be rendered here.
    pub fn pending(&self, slide: ObjId) -> bool {
        self.jobs.lock().is_ok_and(|j| j.iter().any(|(s, _)| *s == slide))
    }

    /// Render these slides next (the ones on screen).
    pub fn prioritize(&self, slide: ObjId) {
        if let Ok(mut j) = self.jobs.lock() {
            if let Some(p) = j.iter().position(|(s, _)| *s == slide) {
                let e = j.remove(p);
                j.insert(0, e);
            }
        }
    }

    pub fn take_results(&self) -> Vec<(ObjId, ColorImage)> {
        self.results.lock().map(|mut r| std::mem::take(&mut *r)).unwrap_or_default()
    }
}

impl Drop for ThumbWorker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

