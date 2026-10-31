//! The page canvas: the document's pages stacked down the window (or side by side in Two
//! Pages), drawn by the suite's renderer into horizontal strips cached per zoom, so typing
//! redraws only the strips whose lines changed and scrolling stays at the display's rate.
//! On top: the text selection and caret, find matches, misspellings, object selection
//! handles and alignment guides. Mouse: place the caret, select text (drag, double / triple
//! click, Shift-click), select, move and resize objects, edit text boxes, zoom with Ctrl+wheel
//! or a pinch.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use kn_render::Renderer;
use kn_ui::theme;
use pg_core::flow::Layout;
use pg_core::kn_format::ObjId;
use pg_core::kn_model::{Drawable, DrawableKind, Geometry};
use tiny_skia as sk;

use crate::app::{App, Cursor, Owner, Sel};

/// Strip height in page points.
const STRIP: f32 = 96.0;
/// Room above the first page and below the last (page points).
const PAD: f32 = 12.0;

/// How the zoom follows the window (View > Zoom: Fit Width / Fit Page / One Page / Two Pages).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    None,
    Width,
    Page,
}

/// Page renderer and its cached strips.
pub struct Tiles {
    pub renderer: Renderer,
    /// (page, strip) -> (content key, scale, texture)
    strips: HashMap<(usize, i32), (u64, f32, egui::TextureHandle)>,
    /// Objects whose content changed get new keys: a hash per object, by version.
    obj_keys: HashMap<ObjId, u64>,
    obj_keys_for: u64,
}

impl Default for Tiles {
    fn default() -> Self {
        Tiles { renderer: Renderer::new(), strips: HashMap::new(), obj_keys: HashMap::new(), obj_keys_for: u64::MAX }
    }
}

impl Tiles {
    pub fn font_installed(&self, name: &str) -> bool {
        self.renderer.fonts.is_installed(name)
    }
    pub fn ps_name_for_family(&self, family: &str) -> Option<String> {
        self.renderer.fonts.ps_name_for_family(family)
    }
    pub fn clear_text_caches(&mut self) {
        self.renderer.clear_caches();
        self.strips.clear();
    }
    pub fn font_families(&self) -> Vec<String> {
        self.renderer.fonts.families().to_vec()
    }
    /// The frame a drawable really occupies (auto-sized text boxes follow their text).
    pub fn effective(&mut self, d: &Drawable) -> Geometry {
        self.renderer.effective_geometry(d)
    }
}

#[derive(Clone, Debug)]
enum Drag {
    /// Selecting text from where the press was (the owner and storage of the press).
    Text { owner: Owner, storage: ObjId, anchor: usize, word: Option<(usize, usize)> },
    /// Moving objects: their page, the press point (page points) and their frames at the start.
    Move { page: usize, start: Pos2, frames: Vec<(ObjId, Geometry)>, moved: bool },
    /// Resizing an object by handle `handle` (0..8, Values' order).
    Resize { id: ObjId, handle: usize, start: Pos2, g0: Geometry },
    /// Rotating an object (Ctrl-drag a handle).
    Rotate { id: ObjId, g0: Geometry, a0: f32 },
    /// Marquee selection on a page.
    Marquee { page: usize, start: Pos2 },
    /// A shape's green control handle (arrow head, star points, corner radius…); `q` in the
    /// shape's frame (Keydeck's).
    Control { id: ObjId, g: Geometry, q: pg_core::kn_model::Point },
    /// One end of a straight line; `a`, `b` in page points.
    LineEnd { id: ObjId, end: usize, a: pg_core::kn_model::Point, b: pg_core::kn_model::Point },
}

/// A plain two-point line's ends in page points (selected lines show a handle on each end
/// instead of a box).
fn line_ends(d: &Drawable) -> Option<(pg_core::kn_model::Point, pg_core::kn_model::Point)> {
    let DrawableKind::Shape(s) = &d.kind else { return None };
    if !s.is_line || s.path.els.len() != 2 {
        return None;
    }
    let pt = |e: &pg_core::kn_model::PathEl| match e {
        pg_core::kn_model::PathEl::Move(p) | pg_core::kn_model::PathEl::Line(p) => Some(*p),
        _ => None,
    };
    let (a, b) = (pt(&s.path.els[0])?, pt(&s.path.els[1])?);
    Some((d.geom.to_slide(a), d.geom.to_slide(b)))
}

/// A selected shape's control handle in page points (when its preset has one).
fn control_point(app: &App, d: &Drawable) -> Option<pg_core::kn_model::Point> {
    if d.locked || !matches!(&d.kind, DrawableKind::Shape(s) if !s.is_line) {
        return None;
    }
    app.doc()?.shape_control(d.id, d.geom.w, d.geom.h).map(|c| d.geom.to_slide(c.pos))
}

pub struct Canvas {
    pub zoom: f32,
    /// Document point at the canvas' top-left.
    pub scroll: Vec2,
    pub two_up: bool,
    pub fit: Fit,
    pub tiles: Tiles,
    /// The text box / shape whose text the caret is in.
    pub editing: Option<ObjId>,
    pub obj: Option<ObjId>,
    pub objs: Vec<ObjId>,
    drag: Option<Drag>,
    /// The moving objects' live offset and guides while dragging (page points).
    live: Option<(Vec2, Vec<kn_ui::snap::Guide>)>,
    live_frame: Option<(ObjId, Geometry)>,
    marquee: Option<Rect>,
    pub(crate) size: Vec2,
    rect: Rect,
    /// A zoom step on its way: from, to, the doc point to keep under `anchor` (screen), seconds, start.
    zoom_anim: Option<(f32, f32, Pos2, Pos2, f32, std::time::Instant)>,
    pub page_in_view: usize,
    /// Scroll to this page / rect next frame.
    reveal: Option<(usize, Option<Rect>)>,
    /// Last click: time, position, count (double / triple click).
    clicks: (f64, Pos2, u32),
    /// Caret blink start.
    pub blink: f64,
    /// Keep the caret in view after the next layout.
    pub follow_caret: bool,
    /// Spelling: paragraph text -> misspelled byte ranges.
    spell_cache: HashMap<u64, Vec<(usize, usize)>>,
    /// Spelling dots of each paragraph for one layout: (layout, para -> (word, page, rect)).
    spell_rects: ((u64, u64), HashMap<usize, Vec<((usize, usize), usize, Rect)>>),
    /// Trackpad glide (screen points / second) after the fingers lift, and when they last moved
    /// / when the system last sent its own momentum events (then it glides by itself).
    kinetic: Vec2,
    last_finger: f64,
    os_momentum: f64,
    /// The fingers lifted (the platform said so) since they last moved.
    lifted: bool,
    /// Wheel notches still to ease in (screen points).
    wheel_pending: Vec2,
    /// The scrollbar thumb being dragged: (pointer y, scroll y) at the start.
    sb_drag: Option<(f32, f32)>,
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas {
            zoom: 1.25,
            scroll: Vec2::ZERO,
            two_up: false,
            fit: Fit::None,
            tiles: Tiles::default(),
            editing: None,
            obj: None,
            objs: Vec::new(),
            drag: None,
            live: None,
            live_frame: None,
            marquee: None,
            size: vec2(900.0, 700.0),
            rect: Rect::NOTHING,
            zoom_anim: None,
            page_in_view: 0,
            reveal: None,
            clicks: (-1.0, Pos2::ZERO, 0),
            blink: 0.0,
            follow_caret: false,
            spell_cache: HashMap::new(),
            spell_rects: ((u64::MAX, 0), HashMap::new()),
            kinetic: Vec2::ZERO,
            last_finger: -1.0,
            os_momentum: -1.0,
            lifted: false,
            wheel_pending: Vec2::ZERO,
            sb_drag: None,
        }
    }
}

impl Canvas {
    pub fn reset(&mut self) {
        self.scroll = Vec2::ZERO;
        self.drag = None;
        self.obj = None;
        self.objs.clear();
        self.editing = None;
        self.zoom_anim = None;
        self.tiles.strips.clear();
        self.tiles.obj_keys.clear();
        self.tiles.obj_keys_for = u64::MAX;
    }

    /// What the layout depends on in the view (Screen View reflows to the window width).
    pub fn layout_key(&self, screen_view: bool) -> u64 {
        if screen_view {
            (self.size.x / self.zoom) as u64 + 1
        } else {
            0
        }
    }

    pub fn zoom_goal(&self) -> f32 {
        self.zoom_anim.map(|a| a.1).unwrap_or(self.zoom)
    }

    /// Zoom to `z` around the view's top centre, eased over ~285 ms.
    pub fn animate_zoom(&mut self, z: f32) {
        let z = z.clamp(0.1, 4.0);
        self.fit = Fit::None;
        let anchor = pos2(self.rect.center().x, self.rect.min.y);
        let doc = self.scroll.to_pos2() + (anchor - self.rect.min) / self.zoom;
        self.zoom_anim = Some((self.zoom, z, doc, anchor, 0.285, std::time::Instant::now()));
    }

    /// The canvas on screen.
    pub fn view_rect(&self) -> Rect {
        self.rect
    }
    /// An object is being moved, resized or rotated (or text / a marquee dragged).
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    /// The objects being moved and how far they have moved so far (text wraps around them live).
    pub fn moving(&self) -> Option<(Vec<ObjId>, Vec2)> {
        match (&self.drag, &self.live) {
            (Some(Drag::Move { frames, moved: true, .. }), Some((d, _))) => Some((frames.iter().map(|f| f.0).collect(), *d)),
            _ => None,
        }
    }
    pub fn reveal_page(&mut self, p: usize) {
        self.reveal = Some((p, None));
    }

    /// The zoom that fits a page's width / a whole page / two pages into the view.
    pub fn fit_zoom(&self, setup: &pg_core::PageSetup, fit: Fit) -> f32 {
        let cols = if self.two_up { 2.0 } else { 1.0 };
        let w = (self.size.x - 40.0).max(100.0) / (setup.width * cols);
        let h = (self.size.y - 24.0).max(100.0) / setup.height;
        match fit {
            Fit::Width => w,
            Fit::Page => w.min(h),
            Fit::None => self.zoom,
        }
        .clamp(0.1, 4.0)
    }
}

/// Page `i`'s rectangle in document points.
pub fn page_rect(c: &Canvas, l: &Layout, i: usize) -> Rect {
    let (pw, ph) = (l.setup.width, l.setup.height);
    let view_w = c.size.x / c.zoom;
    if c.two_up {
        // facing pages start on the right
        let off = usize::from(l.setup.facing_pages);
        let k = i + off;
        let (row, col) = (k / 2, k % 2);
        let doc_w = (2.0 * pw + 2.0 * PAD).max(view_w);
        let x = doc_w / 2.0 - pw + col as f32 * pw;
        Rect::from_min_size(pos2(x, PAD + row as f32 * (ph + PAD)), vec2(pw, ph))
    } else if l.pages.iter().any(|p| p.setup.is_some()) {
        // a Word document with pages of their own sizes (a landscape section): each page at
        // its size, centred, below the ones before it
        let widest = (0..l.pages.len()).map(|j| l.page_setup(j).width).fold(pw, f32::max);
        let doc_w = (widest + 2.0 * PAD).max(view_w);
        let y: f32 = (0..i).map(|j| l.page_setup(j).height).sum();
        let s = l.page_setup(i);
        Rect::from_min_size(pos2((doc_w - s.width) / 2.0, PAD + y), vec2(s.width, s.height))
    } else {
        let doc_w = (pw + 2.0 * PAD).max(view_w);
        Rect::from_min_size(pos2((doc_w - pw) / 2.0, PAD + i as f32 * ph), vec2(pw, ph))
    }
}

fn doc_size(c: &Canvas, l: &Layout) -> Vec2 {
    let n = l.page_count();
    let last = page_rect(c, l, n.saturating_sub(1));
    let first = page_rect(c, l, 0);
    vec2(last.max.x.max(first.max.x) + PAD, last.max.y + PAD)
}

impl Canvas {
    pub fn to_screen(&self, doc: Pos2) -> Pos2 {
        self.rect.min + (doc - self.scroll.to_pos2()) * self.zoom
    }
    pub fn to_doc(&self, screen: Pos2) -> Pos2 {
        self.scroll.to_pos2() + (screen - self.rect.min) / self.zoom
    }
    /// Page point of page `i` -> screen.
    pub fn page_to_screen(&self, l: &Layout, i: usize, p: Pos2) -> Pos2 {
        self.to_screen(page_rect(self, l, i).min + p.to_vec2())
    }
    /// The page under a screen point and the page point there.
    pub fn hit_page(&self, l: &Layout, screen: Pos2) -> Option<(usize, Pos2)> {
        let d = self.to_doc(screen);
        (0..l.page_count()).find_map(|i| {
            let r = page_rect(self, l, i);
            r.expand(if self.two_up { 0.0 } else { PAD / 2.0 }).contains(d).then(|| (i, (d - r.min).to_pos2()))
        })
    }
    /// The page nearest a screen point (for drags that leave the pages).
    pub fn near_page(&self, l: &Layout, screen: Pos2) -> (usize, Pos2) {
        let d = self.to_doc(screen);
        let mut best = (0, Pos2::ZERO, f32::INFINITY);
        for i in 0..l.page_count() {
            let r = page_rect(self, l, i);
            let dist = r.distance_to_pos(d);
            if dist < best.2 {
                best = (i, (d - r.min).to_pos2(), dist);
            }
        }
        (best.0, best.1)
    }
}

/// A drawable's frame on its page (page points), its real size for auto-sized text boxes.
pub fn frame(app: &mut App, d: &Drawable) -> Rect {
    // a masked picture: what shows (its mask)
    if app.canvas.live_frame.is_none_or(|(id, _)| id != d.id) {
        if let Some(m) = crate::mask::mask_box(app, d.id) {
            return Rect::from_min_size(pos2(m.x, m.y), vec2(m.w.max(1.0), m.h.max(1.0)));
        }
    }
    let g = app.canvas.tiles.effective(d);
    let (x, y, w, h) = (g.x, g.y, g.w.max(1.0), g.h.max(1.0));
    if g.angle.abs() > 0.1 {
        // the rotated frame's bounding box
        let (s, c) = g.angle.to_radians().sin_cos();
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let bw = (w * c).abs() + (h * s).abs();
        let bh = (w * s).abs() + (h * c).abs();
        return Rect::from_center_size(pos2(cx, cy), vec2(bw, bh));
    }
    Rect::from_min_size(pos2(x, y), vec2(w, h))
}

fn handle_points(r: Rect) -> [Pos2; 8] {
    [r.left_top(), r.center_top(), r.right_top(), r.left_center(), r.right_center(), r.left_bottom(), r.center_bottom(), r.right_bottom()]
}

thread_local! {
    /// Page bases (background + wallpaper) drawn at the canvas scale: (key, pixels), the last few.
    static BASES: std::cell::RefCell<Vec<(u64, std::sync::Arc<sk::Pixmap>)>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Pages drawn small for the toolbar band: page -> (key, pixels).
    static LOWRES: std::cell::RefCell<HashMap<usize, (u64, std::sync::Arc<sk::Pixmap>)>> = Default::default();
    /// The blurred view under the toolbar: (key, texture).
    static BAND: std::cell::RefCell<Option<(u64, egui::TextureHandle)>> = const { std::cell::RefCell::new(None) };
}

/// Under the floating toolbar (Settings > Toolbar): the pages that scroll up under it, blurred,
/// under Pages' tint (Tinted) or without it (Feathered); Solid leaves the band as it is.
fn paint_toolbar_band(app: &mut App, ui: &egui::Ui) {
    use kn_ui::glass::ToolbarBackground;
    let look = kn_ui::glass::toolbar_background();
    let Some(band) = app.toolbar_band else { return };
    if look == ToolbarBackground::Solid || app.layout.is_none() {
        return;
    }
    let rect = app.canvas.rect;
    // (feathered: the blur runs on below the band, fading out over the page)
    let reach = if look == ToolbarBackground::Feathered { kn_ui::glass::FEATHER_REACH.min(rect.height()) } else { 0.0 };
    let b = Rect::from_min_max(pos2(band.min.x.max(rect.min.x), band.min.y), pos2(band.max.x.min(rect.max.x), band.max.y + reach));
    if b.width() < 2.0 || b.height() < 2.0 {
        return;
    }
    let Some(l) = app.layout.take() else { return };
    let region = Rect::from_min_max(app.canvas.to_doc(b.min), app.canvas.to_doc(b.max));
    let ppp = ui.ctx().pixels_per_point();
    // (a blurred view needs few pixels: an eighth of the screen's in each direction; the
    // feathered one thins out to sharp, so it gets half)
    let feathered = look == ToolbarBackground::Feathered;
    let k = if feathered { 0.5 } else { 0.125 };
    let (w, h) = ((b.width() * ppp * k).ceil().max(1.0) as u32, (b.height() * ppp * k).ceil().max(1.0) as u32);
    let s = app.canvas.zoom * ppp * k;
    let key = {
        let mut hs = std::collections::hash_map::DefaultHasher::new();
        (app.version, app.layout_for, (region.min.x * 4.0) as i64, (region.min.y * 4.0) as i64, w, h, (s * 1000.0) as i64, kn_ui::theme::is_light()).hash(&mut hs);
        hs.finish()
    };
    let have = BAND.with(|c| c.borrow().as_ref().filter(|(kk, _)| *kk == key).map(|(_, t)| t.clone()));
    let tex = match have {
        Some(t) => Some(t),
        None => {
            let mut pm = sk::Pixmap::new(w, h);
            if let (Some(pm), Some(pages)) = (pm.as_mut(), app.wb.as_ref()) {
                let bg = crate::look::canvas_bg();
                pm.fill(sk::Color::from_rgba8(bg.r(), bg.g(), bg.b(), 255));
                // each page rendered once at this small scale, then copied in as it scrolls
                let page_key = {
                    let mut hs = std::collections::hash_map::DefaultHasher::new();
                    (app.version, app.layout_for, (s * 1000.0) as i64).hash(&mut hs);
                    hs.finish()
                };
                for pi in 0..l.page_count() {
                    let pr = page_rect(&app.canvas, &l, pi);
                    if !pr.intersects(region) {
                        continue;
                    }
                    let have = LOWRES.with(|c| c.borrow().get(&pi).filter(|(kk, _)| *kk == page_key).map(|(_, p)| p.clone()));
                    let lp = match have {
                        Some(p) => Some(p),
                        None => {
                            let drawables: Vec<Drawable> = page_layout_drawables(app, &l, pi).into_iter().chain(app.drawables_on(pi).filter(|d| !app.layout_drawables.iter().any(|x| x.id == d.id)).cloned()).collect();
                            let ps = l.page_setup(pi);
                            let mut p = sk::Pixmap::new((ps.width * s).ceil().max(1.0) as u32, (ps.height * s).ceil().max(1.0) as u32);
                            if let Some(p) = p.as_mut() {
                                pg_core::render::draw_page_with(pages, &l, pi, &mut app.canvas.tiles.renderer, p, sk::Transform::from_scale(s, s), Default::default(), &drawables);
                            }
                            let p = p.map(std::sync::Arc::new);
                            if let Some(p) = &p {
                                LOWRES.with(|c| c.borrow_mut().insert(pi, (page_key, p.clone())));
                            }
                            p
                        }
                    };
                    if let Some(lp) = lp {
                        // only the rows in view (opaque page pixels: a straight copy)
                        let (x, y) = (((pr.min.x - region.min.x) * s).round() as i32, ((pr.min.y - region.min.y) * s).round() as i32);
                        let (dw, dh, sw, sh) = (pm.width() as i32, pm.height() as i32, lp.width() as i32, lp.height() as i32);
                        let (x0, x1) = (x.max(0), (x + sw).min(dw));
                        if x1 > x0 {
                            let src = lp.data();
                            let dst = pm.data_mut();
                            for row in y.max(0)..(y + sh).min(dh) {
                                let sr = row - y;
                                let s0 = ((sr * sw + (x0 - x)) * 4) as usize;
                                let d0 = ((row * dw + x0) * 4) as usize;
                                let n = ((x1 - x0) * 4) as usize;
                                dst[d0..d0 + n].copy_from_slice(&src[s0..s0 + n]);
                            }
                        }
                    }
                }
                if !feathered {
                    box_blur(pm, (6.0 * ppp * k).ceil().max(1.0) as usize);
                }
            }
            pm.map(|pm| {
                let mut img = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data());
                if feathered {
                    // full under the buttons, thinning row by row to sharp below the toolbar
                    let (full, sharp) = kn_ui::glass::feather_rows(b, band);
                    img = kn_ui::glass::progressive_blur(&img, (5.0 * ppp * k).ceil().max(1.0) as usize, 3, full, sharp);
                }
                // one texture, updated as the view scrolls
                BAND.with(|c| {
                    let mut c = c.borrow_mut();
                    match c.as_mut() {
                        Some((kk, t)) => {
                            t.set(img, egui::TextureOptions::LINEAR);
                            *kk = key;
                            t.clone()
                        }
                        None => {
                            let t = ui.ctx().load_texture("toolbar-band", img, egui::TextureOptions::LINEAR);
                            *c = Some((key, t.clone()));
                            t
                        }
                    }
                })
            })
        }
    };
    app.layout = Some(l);
    // painted by the window over the band (the canvas's own painter stops at the canvas)
    app.toolbar_band_paint = tex.map(|t| (t, b, look == ToolbarBackground::Tinted));
}

/// A box blur (three passes each way, close to a gaussian) on premultiplied pixels, with running
/// sums (the cost doesn't grow with the radius).
fn box_blur(pm: &mut sk::Pixmap, r: usize) {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    if w == 0 || h == 0 {
        return;
    }
    let data = pm.data_mut();
    let mut tmp = vec![0u8; data.len()];
    // one line: `n` pixels `step` apart starting at `start`
    let line = |src: &[u8], dst: &mut [u8], start: usize, step: usize, n: usize| {
        let at = |i: usize| (start + i.min(n - 1) * step) * 4;
        let win = (2 * r + 1) as u32;
        // (the left edge repeats the first pixel r+1 times)
        let mut acc = [0u32; 4];
        for c in 0..4 {
            acc[c] = src[at(0) + c] as u32 * (r as u32 + 1);
            for j in 1..=r {
                acc[c] += src[at(j) + c] as u32;
            }
        }
        for i in 0..n {
            for c in 0..4 {
                dst[(start + i * step) * 4 + c] = (acc[c] / win) as u8;
            }
            let (out, inn) = (i.saturating_sub(r), i + r + 1);
            for c in 0..4 {
                acc[c] = acc[c] + src[at(inn) + c] as u32 - src[at(out) + c] as u32;
            }
        }
    };
    for _ in 0..3 {
        for y in 0..h {
            line(data, &mut tmp, y * w, 1, w);
        }
        for x in 0..w {
            line(&tmp, data, x, w, h);
        }
    }
}

/// The section layout objects (wallpaper) page `pi` shows, behind everything.
pub fn page_layout_drawables(app: &App, l: &Layout, pi: usize) -> Vec<Drawable> {
    let Some(page) = l.pages.get(pi) else { return Vec::new() };
    page.objects.iter().filter(|o| o.layout).filter_map(|o| app.layout_drawables.iter().find(|d| d.id == o.id).cloned()).collect()
}

/// The objects of page `p`, topmost first.
fn objects_on(app: &App, p: usize) -> Vec<Drawable> {
    let mut v: Vec<Drawable> = app.drawables_on(p).cloned().collect();
    v.reverse();
    v
}

/// A content key of strip `s` of page `pi` (what's drawn in it).
fn strip_key(app: &App, l: &Layout, pi: usize, s: i32, skip: &[ObjId]) -> u64 {
    let page = &l.pages[pi];
    let (y0, y1) = (s as f32 * STRIP, (s + 1) as f32 * STRIP);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (l.setup.width.to_bits(), l.setup.height.to_bits()).hash(&mut h);
    format!("{:?}", page.background).hash(&mut h);
    for line in &page.lines {
        let (a, b) = line.band();
        if b + 8.0 < y0 || a - 8.0 > y1 {
            continue;
        }
        (std::sync::Arc::as_ptr(&line.piece) as usize, line.line, line.dx.to_bits(), line.dy.to_bits()).hash(&mut h);
        // the paragraph's background and border (drawn with its lines)
        if let Some(ex) = l.body.as_ref().and_then(|b| b.extra.get(line.para)) {
            format!("{:?}{:?}", ex.fill, ex.border).hash(&mut h);
        }
    }
    // drop caps' shapes (their colour and corners aren't in the lines)
    for c in &page.caps {
        if c.rect.1 + c.rect.3 + 8.0 < y0 || c.rect.1 - 8.0 > y1 {
            continue;
        }
        format!("{:?}{:?}{}", c.rect, c.fill, c.radius).hash(&mut h);
    }
    for hf in page.headers.iter().chain(&page.footers) {
        (std::sync::Arc::as_ptr(&hf.piece) as usize, hf.dy.to_bits()).hash(&mut h);
    }
    for f in &page.footnotes {
        (std::sync::Arc::as_ptr(&f.piece) as usize, f.dy.to_bits()).hash(&mut h);
    }
    for b in &page.toc {
        for e in &b.entries {
            (&e.text, e.page, e.y.to_bits()).hash(&mut h);
        }
    }
    for o in &page.objects {
        if skip.contains(&o.id) || o.y + o.h + 40.0 < y0 || o.y - 40.0 > y1 {
            continue;
        }
        (o.id, o.x.to_bits(), o.y.to_bits(), app.canvas.tiles.obj_keys.get(&o.id).copied().unwrap_or(0)).hash(&mut h);
    }
    app.invisibles.hash(&mut h);
    h.finish()
}

/// Draw the canvas; returns true when everything in view is drawn (automated checks wait for it).
pub fn canvas(app: &mut App, ui: &mut egui::Ui) -> bool {
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect();
    app.canvas.rect = rect;
    kn_ui::touch::pan_area(rect);
    app.canvas.size = rect.size();
    let resp = ui.interact(rect, ui.scope_id().with("page-canvas"), Sense::click_and_drag());
    if app.layout.is_none() {
        return true;
    }
    // zoom easing
    if let Some((z0, z1, doc, anchor, dur, t0)) = app.canvas.zoom_anim {
        let t = (t0.elapsed().as_secs_f32() / dur).min(1.0);
        let e = if t < 0.5 { 2.0 * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 };
        app.canvas.zoom = z0 + (z1 - z0) * e;
        app.canvas.scroll = (doc - (anchor - rect.min) / app.canvas.zoom).to_vec2();
        if t >= 1.0 {
            app.canvas.zoom_anim = None;
        } else {
            ctx.request_repaint();
        }
    }
    // fit modes follow the window
    if app.canvas.fit != Fit::None && app.canvas.zoom_anim.is_none() {
        let setup = app.layout.as_ref().unwrap().setup.clone();
        app.canvas.zoom = app.canvas.fit_zoom(&setup, app.canvas.fit);
    }
    // scrolling. Trackpad moves are applied as they come (egui smooths the bigger ones over a
    // tenth of a second, which lags when the direction flips); wheel notches ease in over a few
    // frames and a turn the other way drops what's left of them; after the fingers lift, a GTK
    // glide that stops as soon as they touch again or go the other way.
    {
        // (not under a window such as the Content Hub, which scrolls itself)
        let hovered = (resp.hovered() || resp.contains_pointer()) && !kn_ui::pointer_over_window(&ctx);
        let (events, now, dt, pressed, zoom_delta, ptr, ctrl) = ctx.input(|i| {
            let ev: Vec<(egui::MouseWheelUnit, Vec2, egui::TouchPhase, egui::MouseWheelSource, egui::Modifiers)> = i
                .raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, phase, source, modifiers } => Some((*unit, *delta, *phase, *source, *modifiers)),
                    _ => None,
                })
                .collect();
            let pressed = i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
            (ev, i.time, i.stable_dt.clamp(0.001, 0.05), pressed, i.zoom_delta(), i.pointer.hover_pos(), i.modifiers.command)
        });
        let (mut fingers, mut lines) = (Vec2::ZERO, Vec2::ZERO);
        let (mut touched, mut lifted, mut momentum) = (false, false, false);
        for (unit, delta, phase, source, mods) in events {
            if mods.command {
                continue;
            }
            match phase {
                egui::TouchPhase::Start => touched = true,
                egui::TouchPhase::End | egui::TouchPhase::Cancel => lifted = true,
                egui::TouchPhase::Move => {}
            }
            let d = if mods.shift { vec2(delta.x + delta.y, 0.0) } else { delta };
            match unit {
                egui::MouseWheelUnit::Point if source == egui::MouseWheelSource::Momentum => {
                    momentum = true;
                    fingers += d;
                }
                egui::MouseWheelUnit::Point => fingers += d,
                egui::MouseWheelUnit::Line => lines += d * 100.0,
                egui::MouseWheelUnit::Page => lines += d * rect.height(),
            }
        }
        // under a window (the Content Hub, a popover) the wheel isn't ours: nor is the glide
        if !hovered {
            (fingers, lines, momentum) = (Vec2::ZERO, Vec2::ZERO, false);
        }
        if hovered && !ctrl {
            if fingers != Vec2::ZERO {
                app.canvas.scroll -= fingers / app.canvas.zoom;
            }
            if lines != Vec2::ZERO {
                if lines.dot(app.canvas.wheel_pending) < 0.0 {
                    app.canvas.wheel_pending = Vec2::ZERO;
                }
                app.canvas.wheel_pending += lines;
            }
        }
        let pending = app.canvas.wheel_pending;
        if pending.length() > 0.5 {
            let step = pending * (1.0 - (-22.0 * dt).exp());
            app.canvas.scroll -= step / app.canvas.zoom;
            app.canvas.wheel_pending -= step;
            ctx.request_repaint();
        } else if pending != Vec2::ZERO {
            app.canvas.scroll -= pending / app.canvas.zoom;
            app.canvas.wheel_pending = Vec2::ZERO;
        }
        // the glide
        if momentum {
            app.canvas.os_momentum = now;
        }
        if touched || pressed || now - app.canvas.os_momentum < 0.5 || ctrl {
            app.canvas.kinetic = Vec2::ZERO;
        }
        if fingers != Vec2::ZERO && !momentum {
            let v = fingers / dt;
            let fresh = now - app.canvas.last_finger > 0.1 || v.dot(app.canvas.kinetic) < 0.0;
            app.canvas.kinetic = if fresh { v } else { app.canvas.kinetic * 0.6 + v * 0.4 };
            app.canvas.last_finger = now;
            app.canvas.lifted = false;
        } else {
            if lifted {
                app.canvas.lifted = true;
            }
            // (where the platform says when the fingers lift, glide from then; else after a pause)
            let go = app.canvas.lifted || now - app.canvas.last_finger > 0.06;
            if go && app.canvas.kinetic.length() > 30.0 {
                app.canvas.scroll -= app.canvas.kinetic * dt / app.canvas.zoom;
                app.canvas.kinetic *= (-2.6 * dt).exp();
                ctx.request_repaint();
            } else if go {
                app.canvas.kinetic = Vec2::ZERO;
            }
        }
        // Ctrl+wheel and pinch zoom
        if hovered && (zoom_delta - 1.0).abs() > 1e-4 {
            let anchor = ptr.unwrap_or(rect.center());
            let doc = app.canvas.to_doc(anchor);
            app.canvas.fit = Fit::None;
            app.canvas.zoom_anim = None;
            app.canvas.zoom = (app.canvas.zoom * zoom_delta).clamp(0.1, 4.0);
            app.canvas.scroll = (doc - (anchor - rect.min) / app.canvas.zoom).to_vec2();
        }
    }
    // keep the scroll inside the document
    {
        let l = app.layout.as_ref().unwrap();
        let size = doc_size(&app.canvas, l);
        let view = rect.size() / app.canvas.zoom;
        let max = (size - view).max(Vec2::ZERO);
        // narrower than the view: centred (page_rect centres on the view width)
        app.canvas.scroll = app.canvas.scroll.clamp(Vec2::ZERO, max);
        if let Some((p, r)) = app.canvas.reveal.take() {
            let pr = page_rect(&app.canvas, l, p.min(l.page_count().saturating_sub(1)));
            match r {
                Some(r) => {
                    let r = r.translate(pr.min.to_vec2());
                    let vis = Rect::from_min_size(app.canvas.scroll.to_pos2(), view);
                    let m = 20.0 / app.canvas.zoom;
                    let mut s = app.canvas.scroll;
                    // out of view: brought in a quarter of the way down (or up), as Pages does
                    if r.min.y < vis.min.y + m {
                        s.y = r.min.y - view.y * 0.25;
                    } else if r.max.y > vis.max.y - m {
                        s.y = r.max.y - view.y * 0.75;
                    }
                    if r.min.x < vis.min.x || r.max.x > vis.max.x {
                        s.x = r.center().x - view.x / 2.0;
                    }
                    app.canvas.scroll = s.clamp(Vec2::ZERO, max);
                }
                None => app.canvas.scroll.y = (pr.min.y - PAD).clamp(0.0, max.y),
            }
        }
    }
    // keep the caret in view after typing and moves
    if app.canvas.follow_caret {
        app.canvas.follow_caret = false;
        if let Some((c, l)) = app.cursor.filter(|_| app.layout.is_some()).map(|c| (c, app.layout.take().unwrap())) {
            if let Some((pi, x, top, h)) = caret_rect(app, &l, &c) {
                app.canvas.reveal = Some((pi, Some(Rect::from_min_size(pos2(x - 2.0, top), vec2(4.0, h)))));
            }
            app.layout = Some(l);
        }
    }
    // object keys (contents) once per rebuild of the drawables (the version they were built
    // for: an edit after a rebuild in the same frame, such as an inserted chart then given its
    // type, must not leave the old drawable's key under the new version)
    let built_for = app.layout_for.0;
    if app.canvas.tiles.obj_keys_for != built_for {
        app.canvas.tiles.obj_keys = app
            .drawables
            .iter()
            .chain(&app.layout_drawables)
            .map(|d| {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                format!("{:?}", d).hash(&mut h);
                (d.id, h.finish())
            })
            .collect();
        app.canvas.tiles.obj_keys_for = built_for;
    }
    let painter = ui.painter_at(rect);
    let settled = paint_pages(app, ui, &painter, rect);
    paint_toolbar_band(app, ui);
    paint_overlay(app, ui, &painter);
    // a shape's points being edited take the pointer near them (Make Editable)
    let mut points_took = false;
    if let Some((id, l)) = app.points.id.filter(|_| app.layout.is_some()).map(|id| (id, app.layout.take().unwrap())) {
        let pi = app.obj_page.get(&id).copied().unwrap_or(0);
        let origin = page_rect(&app.canvas, &l, pi).min;
        let (scroll, zoom, rmin) = (app.canvas.scroll, app.canvas.zoom, app.canvas.rect.min);
        app.layout = Some(l);
        let to_screen = move |q: Pos2| rmin + (origin + q.to_vec2() - scroll.to_pos2()) * zoom;
        points_took = crate::points::canvas(app, ui, &resp, &to_screen, zoom);
    }
    // a selected table's tabs, buttons, borders and cell editor take the pointer over them
    let bar_took = scrollbar(app, ui) || crate::chartedit::canvas_button(app, ui) || crate::media::canvas_controls(app, ui) || crate::mediainsp::gallery_controls(app, ui);
    let table_took = !points_took && !bar_took && crate::tables::ui(app, ui);
    // the Insert Page Number button of the header / footer field with the caret
    let hf_took = !points_took && !table_took && !bar_took && crate::hfnumber::ui(app, ui);
    // guides dragged from the ruler or grabbed on the page
    let guide_took = !points_took && !table_took && !bar_took && !hf_took && crate::guides::ui(app, ui, &resp);
    if !points_took && !table_took && !bar_took && !hf_took && !guide_took {
        interact(app, ui, &resp);
    }
    crate::text::handle_input(app, &ctx, !ctx.egui_wants_keyboard_input());
    // the page under the view's middle
    if let Some(l) = &app.layout {
        let mid = app.canvas.to_doc(rect.center());
        app.canvas.page_in_view = (0..l.page_count()).find(|i| page_rect(&app.canvas, l, *i).expand(PAD).y_range().contains(mid.y)).unwrap_or(app.canvas.page_in_view);
    }
    if app.word_count {
        crate::notes::word_count_pill(app, ui, rect);
    }
    if let Some((m, _)) = &app.message {
        let r = Rect::from_center_size(pos2(rect.center().x, rect.max.y - 60.0), vec2((m.len() as f32 * 7.5 + 40.0).min(rect.width() - 40.0), 34.0));
        painter.rect_filled(r, 17.0, theme::gray(0x30).gamma_multiply(0.95));
        painter.text(r.center(), egui::Align2::CENTER_CENTER, m, egui::FontId::proportional(13.0), theme::TEXT());
    }
    if app.screen_view {
        let r = Rect::from_min_size(pos2(rect.max.x - 190.0, rect.min.y + 10.0), vec2(176.0, 26.0));
        painter.rect_filled(r, 13.0, theme::gray(0x30).gamma_multiply(0.92));
        painter.circle_filled(pos2(r.min.x + 14.0, r.center().y), 4.0, accent_orange());
        painter.text(pos2(r.min.x + 26.0, r.center().y), egui::Align2::LEFT_CENTER, "Screen View is on.", egui::FontId::proportional(12.5), theme::TEXT());
    }
    settled
}

fn accent_orange() -> Color32 {
    theme::ACCENT()
}

/// Draw every visible page from its strips (rendering missing ones within a time budget).
fn paint_pages(app: &mut App, ui: &egui::Ui, painter: &egui::Painter, rect: Rect) -> bool {
    let Some(l) = app.layout.take() else { return true };
    let ppp = ui.ctx().pixels_per_point();
    let zoom = app.canvas.zoom;
    let scale = zoom * ppp;
    let zooming = app.canvas.zoom_anim.is_some() || ui.input(|i| (i.zoom_delta() - 1.0).abs() > 1e-4);
    let t0 = std::time::Instant::now();
    let mut done = true;
    let skip: Vec<ObjId> = match &app.canvas.drag {
        Some(Drag::Move { frames, moved: true, .. }) => frames.iter().map(|f| f.0).collect(),
        Some(Drag::Resize { id, .. }) | Some(Drag::Rotate { id, .. }) => vec![*id],
        _ => Vec::new(),
    };
    let visible = Rect::from_min_size(app.canvas.scroll.to_pos2(), rect.size() / zoom);
    for pi in 0..l.page_count() {
        let pr = page_rect(&app.canvas, &l, pi);
        if !pr.intersects(visible) {
            continue;
        }
        let sr = Rect::from_min_max(app.canvas.to_screen(pr.min), app.canvas.to_screen(pr.max));
        // a soft shadow all round the page (Pages'), which also parts the pages
        painter.add(egui::Shadow { offset: [0, 1], blur: 8, spread: 0, color: Color32::from_black_alpha(45) }.as_shape(sr, 0.0));
        painter.rect_filled(sr, 0.0, Color32::WHITE);
        let page_h = l.page_setup(pi).height;
        let n_strips = (page_h / STRIP).ceil() as i32;
        for s in 0..n_strips {
            let y0 = s as f32 * STRIP;
            let strip_doc = Rect::from_min_size(pos2(pr.min.x, pr.min.y + y0), vec2(pr.width(), STRIP.min(page_h - y0)));
            if !strip_doc.intersects(visible) {
                continue;
            }
            let key = strip_key(app, &l, pi, s, &skip);
            let have = app.canvas.tiles.strips.get(&(pi, s));
            let fresh = have.is_some_and(|(k, sc, _)| *k == key && (*sc - scale).abs() < 1e-3);
            let budget = t0.elapsed().as_secs_f32() < 0.012 || have.is_none_or(|(k, _, _)| *k != key);
            if !fresh && !zooming && budget {
                let drawables: Vec<Drawable> = page_layout_drawables(app, &l, pi)
                    .into_iter()
                    .filter(|d| !skip.contains(&d.id))
                    .chain(app.drawables_on(pi).filter(|d| !skip.contains(&d.id) && !app.layout_drawables.iter().any(|x| x.id == d.id)).cloned())
                    .collect();
                if let Some(pages) = app.wb.as_ref() {
                    // pages with wallpaper share one drawn base per template, background and scale
                    let page = &l.pages[pi];
                    let base_key = page.objects.iter().any(|o| o.layout).then(|| {
                        let mut hs = std::collections::hash_map::DefaultHasher::new();
                        for o in page.objects.iter().filter(|o| o.layout) {
                            (o.id, app.canvas.tiles.obj_keys.get(&o.id).copied().unwrap_or(0)).hash(&mut hs);
                        }
                        (format!("{:?}", page.background), (scale * 1000.0) as i64, l.setup.width.to_bits(), l.setup.height.to_bits()).hash(&mut hs);
                        hs.finish()
                    });
                    if let Some(tex) = render_strip(&mut app.canvas.tiles.renderer, ui, pages, &l, pi, s, scale, &drawables, app.invisibles, base_key) {
                        app.canvas.tiles.strips.insert((pi, s), (key, scale, tex));
                    }
                }
            } else if !fresh {
                done = false;
            }
            if let Some((_, _, tex)) = app.canvas.tiles.strips.get(&(pi, s)) {
                // the last strip stops at the page's edge (what reaches past it isn't shown)
                let shown = (page_h - y0).clamp(0.0, STRIP);
                let dest = Rect::from_min_max(app.canvas.to_screen(strip_doc.min), app.canvas.to_screen(pos2(strip_doc.max.x, pr.min.y + y0 + shown)));
                painter.image(tex.id(), dest, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, shown / STRIP)), Color32::WHITE);
            }
        }
        painter.rect_stroke(sr, 0.0, Stroke::new(0.5, Color32::from_gray(0x4d)), egui::StrokeKind::Outside);
        // Show Layout: the text area and header / footer bands
        if app.show_layout {
            let s = &l.setup;
            let body = Rect::from_min_max(app.canvas.page_to_screen(&l, pi, pos2(s.left, s.top)), app.canvas.page_to_screen(&l, pi, pos2(s.width - s.right, s.height - s.bottom)));
            painter.rect_stroke(body, 0.0, Stroke::new(1.0, Color32::from_rgb(0x9c, 0xc2, 0xf0)), egui::StrokeKind::Middle);
        }
    }
    // forget strips of pages no longer there
    let n = l.page_count();
    app.canvas.tiles.strips.retain(|(p, _), _| *p < n);
    if !done {
        ui.ctx().request_repaint();
    }
    app.layout = Some(l);
    done
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn render_strip(renderer: &mut Renderer, ui: &egui::Ui, pages: &pg_core::Pages, l: &Layout, pi: usize, s: i32, scale: f32, drawables: &[Drawable], invisibles: bool, base_key: Option<u64>) -> Option<egui::TextureHandle> {
    let w = (l.page_setup(pi).width * scale).ceil() as u32;
    let h = (STRIP * scale).ceil() as u32;
    let mut pm = sk::Pixmap::new(w.max(1), h.max(1))?;
    let y0 = s as f32 * STRIP;
    let base = sk::Transform::from_scale(scale, scale).pre_translate(0.0, -y0);
    let mut opts = pg_core::render::RenderOpts { invisibles, clip: Some((y0, y0 + STRIP)), ..Default::default() };
    // a page with wallpaper: its base drawn once at this scale, its rows copied into the strip
    if let Some(key) = base_key {
        let have = BASES.with(|c| c.borrow().iter().find(|(k, _)| *k == key).map(|(_, p)| p.clone()));
        let layer = have.or_else(|| {
            let mut full = sk::Pixmap::new(w.max(1), (l.page_setup(pi).height * scale).ceil().max(1.0) as u32)?;
            let o = pg_core::render::RenderOpts { base_only: true, ..Default::default() };
            pg_core::render::draw_page_with(pages, l, pi, renderer, &mut full, sk::Transform::from_scale(scale, scale), o, drawables);
            let full = std::sync::Arc::new(full);
            BASES.with(|c| {
                let mut c = c.borrow_mut();
                c.push((key, full.clone()));
                if c.len() > 3 {
                    c.remove(0);
                }
            });
            Some(full)
        });
        if let Some(layer) = layer {
            let row0 = (y0 * scale).round() as usize;
            let stride = w as usize * 4;
            let src = layer.data();
            let dst = pm.data_mut();
            for row in 0..h as usize {
                let sr = row0 + row;
                if (sr + 1) * stride > src.len() {
                    break;
                }
                dst[row * stride..(row + 1) * stride].copy_from_slice(&src[sr * stride..(sr + 1) * stride]);
            }
            opts.skip_base = true;
        }
    }
    pg_core::render::draw_page_with(pages, l, pi, renderer, &mut pm, base, opts, drawables);
    let img = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data());
    Some(ui.ctx().load_texture(format!("strip-{pi}-{s}"), img, egui::TextureOptions::LINEAR))
}

/// Selection, caret, find matches, misspellings, object handles, guides, drag previews.
fn paint_overlay(app: &mut App, ui: &egui::Ui, painter: &egui::Painter) {
    let Some(l) = app.layout.take() else { return };
    let zoom = app.canvas.zoom;
    // header / footer fields (Pages): the band's three fields outlined thinly (#D9DCDE) while the
    // pointer is over it or the caret is in one of them
    {
        let editing = match app.cursor {
            Some(Cursor { owner: Owner::HeaderFooter { page }, storage, .. }) => Some((page, storage)),
            _ => None,
        };
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| app.canvas.rect.contains(*p)).and_then(|p| app.canvas.hit_page(&l, p));
        let s = &l.setup;
        let band = |pi: usize, footer: bool| -> Vec<Rect> { hf_fields(&l, pi).into_iter().filter(|f| f.2 == footer).map(|f| f.0).collect() };
        let mut show: Vec<(usize, bool)> = Vec::new();
        if let (Some((pi, pt)), true) = (hover, app.canvas.drag.is_none()) {
            if pt.y < s.top {
                show.push((pi, false));
            } else if pt.y > s.height - s.bottom {
                show.push((pi, true));
            }
        }
        if let Some((pi, st)) = editing {
            let footer = l.pages.get(pi).is_some_and(|pg| pg.footers.iter().any(|f| f.storage == st));
            if !show.contains(&(pi, footer)) {
                show.push((pi, footer));
            }
        }
        for (pi, footer) in show {
            let mut rects = band(pi, footer);
            rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
            let n = rects.len();
            for (k, r) in rects.into_iter().enumerate() {
                // a little room around the text: the band's outer edges sit outside the margins,
                // a small gap between neighbouring fields
                let mut sr = Rect::from_min_max(app.canvas.page_to_screen(&l, pi, r.min), app.canvas.page_to_screen(&l, pi, r.max)).expand2(vec2(0.0, 2.0));
                sr.min.x += if k == 0 { -5.0 } else { 2.0 };
                sr.max.x += if k + 1 == n { 5.0 } else { -2.0 };
                painter.rect_stroke(sr, 2.0, Stroke::new(1.0, Color32::from_rgb(0xd9, 0xdc, 0xde)), egui::StrokeKind::Middle);
            }
        }
    }
    let sel_col = theme::ACCENT().gamma_multiply(0.28);
    // find matches (body)
    if let (Some((_, matches, cur)), Some(body)) = (&app.find, &l.body) {
        for (k, (a, b)) in matches.iter().enumerate() {
            for (pi, r) in range_rects(&l, body.to_shown(*a), body.to_shown(*b)) {
                let sr = Rect::from_min_max(app.canvas.page_to_screen(&l, pi, r.min), app.canvas.page_to_screen(&l, pi, r.max));
                painter.rect_filled(sr, 2.0, if k == *cur { Color32::from_rgba_unmultiplied(0xff, 0xd6, 0x00, 170) } else { Color32::from_rgba_unmultiplied(0xff, 0xe8, 0x70, 110) });
            }
        }
    }
    // misspellings in the body (red dots), visible lines only
    if app.settings.spell_check {
        if app.canvas.spell_rects.0 != app.layout_for {
            app.canvas.spell_rects = (app.layout_for, HashMap::new());
        }
        if let Some(body) = &l.body {
            let visible = Rect::from_min_size(app.canvas.scroll.to_pos2(), app.canvas.size / zoom);
            for pi in 0..l.page_count() {
                let pr = page_rect(&app.canvas, &l, pi);
                if !pr.intersects(visible) {
                    continue;
                }
                let mut seen = std::collections::HashSet::new();
                // the part of this page in view (page points)
                let (vy0, vy1) = (visible.min.y - pr.min.y, visible.max.y - pr.min.y);
                for line in &l.pages[pi].lines {
                    let (t, b) = line.band();
                    if b < vy0 || t > vy1 || !seen.insert(line.para) {
                        continue;
                    }
                    let p = &body.tb.paragraphs[line.para];
                    let text = &body.tb.text[p.start..p.end];
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    text.hash(&mut h);
                    let key = h.finish();
                    let bad = app.canvas.spell_cache.entry(key).or_insert_with(|| kn_ui::spell::misspelled(text)).clone();
                    // the word being typed isn't marked yet
                    let caret = match app.cursor {
                        Some(Cursor { owner: Owner::Body, caret, .. }) => Some(body.to_shown(caret)),
                        _ => None,
                    };
                    // where each word is (kept until the layout changes)
                    let rects = app.canvas.spell_rects.1.entry(line.para).or_insert_with(|| {
                        bad.iter().flat_map(|(a, b)| {
                            let (a, b) = (p.start + a, p.start + b);
                            range_rects(&l, a, b).into_iter().map(move |(qi, r)| ((a, b), qi, r))
                        }).collect()
                    }).clone();
                    for ((a, b), qi, r) in rects {
                        if caret.is_some_and(|c| c >= a && c <= b) || qi != pi {
                            continue;
                        }
                        {
                            let y = app.canvas.page_to_screen(&l, qi, pos2(0.0, r.max.y - 1.5)).y;
                            let x0 = app.canvas.page_to_screen(&l, qi, pos2(r.min.x, 0.0)).x;
                            let x1 = app.canvas.page_to_screen(&l, qi, pos2(r.max.x, 0.0)).x;
                            let mut x = x0;
                            while x < x1 {
                                painter.circle_filled(pos2(x, y), 1.1 * zoom.clamp(0.8, 1.6), Color32::from_rgb(0xe0, 0x30, 0x30));
                                x += 3.5 * zoom.clamp(0.8, 1.6);
                            }
                        }
                    }
                }
            }
            if app.canvas.spell_cache.len() > 4000 {
                app.canvas.spell_cache.clear();
            }
        }
    }
    // text selection and caret
    if let Some(c) = app.cursor {
        let focused = ui.input(|i| i.viewport().focused.unwrap_or(true));
        if let Some(rects) = selection_rects(app, &l, &c) {
            let mut ends: Option<([Pos2; 2], [Pos2; 2])> = None;
            for (pi, r) in rects {
                let sr = Rect::from_min_max(app.canvas.page_to_screen(&l, pi, r.min), app.canvas.page_to_screen(&l, pi, r.max));
                painter.rect_filled(sr, 0.0, sel_col);
                let start = ends.map(|e| e.0).unwrap_or([sr.left_top(), sr.left_bottom()]);
                ends = Some((start, [sr.right_top(), sr.right_bottom()]));
            }
            // (with a finger: handles at both ends, to drag)
            if let Some((s, e)) = ends.filter(|_| !c.is_empty()) {
                kn_ui::touch::selection_handles(&painter, s, e, theme::ACCENT());
            }
        }
        if c.is_empty() && app.canvas.obj.is_none() {
            if let Some((pi, x, top, h)) = caret_rect(app, &l, &c) {
                let t = ui.input(|i| i.time) - app.canvas.blink;
                let on = !focused || (t % 1.06) < 0.62 || app.automated;
                if on {
                    let a = app.canvas.page_to_screen(&l, pi, pos2(x, top));
                    painter.rect_filled(Rect::from_min_size(a, vec2((1.6 * zoom).clamp(1.5, 3.0), h * zoom)), 0.0, theme::ACCENT());
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
            }
        }
    }
    // selected objects: frame and handles (moving ones at their live place)
    let (live, guides) = app.canvas.live.clone().map(|(v, g)| (v, g)).unwrap_or((Vec2::ZERO, Vec::new()));
    let moving: Vec<ObjId> = match &app.canvas.drag {
        Some(Drag::Move { frames, moved: true, .. }) => frames.iter().map(|f| f.0).collect(),
        _ => Vec::new(),
    };
    for id in app.selected_objs() {
        let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { continue };
        let Some(pi) = app.obj_page.get(&id).copied() else { continue };
        let mut r = frame(app, &d);
        if let Some((lid, g)) = app.canvas.live_frame.filter(|(lid, _)| *lid == id) {
            let _ = lid;
            r = Rect::from_min_size(pos2(g.x, g.y), vec2(g.w.max(1.0), g.h.max(1.0)));
        }
        if moving.contains(&id) {
            r = r.translate(live);
            // the object itself, drawn live
            draw_live_object(app, ui, painter, &l, pi, &d, live);
        }
        let sr = Rect::from_min_max(app.canvas.page_to_screen(&l, pi, r.min), app.canvas.page_to_screen(&l, pi, r.max));
        let locked = d.locked;
        if app.points.id == Some(id) {
            // Make Editable: only the points show
            continue;
        }
        // a table with cells selected shows the cell selection instead
        if app.sel.is_some_and(|s| s.table == id) {
            continue;
        }
        let single = app.selected_objs().len() == 1;
        let at = |q: pg_core::kn_model::Point| app.canvas.page_to_screen(&l, pi, pos2(q.x, q.y));
        let green = Color32::from_rgb(0x5c, 0xd1, 0x4a);
        if single {
            // a straight line: a handle on each end (its new place while one is dragged)
            if let Some((mut a, mut b)) = line_ends(&d) {
                if let Some(Drag::LineEnd { id: did, a: da, b: db, .. }) = &app.canvas.drag {
                    if *did == id {
                        (a, b) = (*da, *db);
                        painter.line_segment([at(a), at(b)], Stroke::new(1.5, theme::ACCENT()));
                    }
                }
                for q in [a, b] {
                    let hr = Rect::from_center_size(at(q), vec2(9.0, 9.0));
                    painter.rect_filled(hr, 2.0, Color32::WHITE);
                    painter.rect_stroke(hr, 2.0, Stroke::new(1.0, theme::ACCENT()), egui::StrokeKind::Middle);
                }
                continue;
            }
            // a shape's control handle being dragged: the shape's outline as it will be
            if let (Some(Drag::Control { id: did, g, q }), Some(doc)) = (&app.canvas.drag, app.doc()) {
                if *did == id {
                    if let Some(path) = doc.shape_path_with_control(id, g.w, g.h, *q) {
                        for poly in flatten(&path) {
                            let pts: Vec<Pos2> = poly.into_iter().map(|p| at(g.to_slide(p))).collect();
                            painter.add(egui::Shape::line(pts, Stroke::new(1.5, theme::ACCENT())));
                        }
                    }
                    let c = at(g.to_slide(*q));
                    painter.circle_filled(c, 5.0, green);
                    painter.circle_stroke(c, 5.0, Stroke::new(1.0, Color32::WHITE));
                    continue;
                }
            }
        }
        painter.rect_stroke(sr, 0.0, Stroke::new(1.0, theme::ACCENT()), egui::StrokeKind::Middle);
        // (a finger on the selection or its handles moves or resizes it)
        kn_ui::touch::grab(sr.expand(16.0));
        if single && app.canvas.drag.is_none() {
            if let Some(c) = control_point(app, &d) {
                let c = at(c);
                painter.circle_filled(c, 5.0, green);
                painter.circle_stroke(c, 5.0, Stroke::new(1.0, Color32::WHITE));
            }
        }
        // (the handles hide while the object is being moved, as in Pages)
        if app.canvas.editing != Some(id) && !moving.contains(&id) {
            for p in handle_points(sr) {
                let hr = Rect::from_center_size(p, vec2(8.0, 8.0));
                if locked {
                    painter.text(p, egui::Align2::CENTER_CENTER, "×", egui::FontId::proportional(11.0), Color32::from_gray(0x80));
                } else {
                    painter.rect_filled(hr, 1.0, Color32::WHITE);
                    painter.rect_stroke(hr, 1.0, Stroke::new(1.0, Color32::from_gray(0x80)), egui::StrokeKind::Middle);
                }
            }
        }
        // size and position while dragging (Settings > Rulers)
        if app.canvas.drag.is_some() && app.settings.show_size_while_moving && (moving.contains(&id) || app.canvas.live_frame.is_some()) {
            let units = |v: f32| crate::ruler::fmt_len(v, app.settings.ruler_units);
            let text = if app.canvas.live_frame.is_some() { format!("w: {}  h: {}", units(r.width()), units(r.height())) } else { format!("x: {}  y: {}", units(r.min.x), units(r.min.y)) };
            let at = sr.right_top() + vec2(12.0, -6.0);
            let g = painter.layout_no_wrap(text, egui::FontId::proportional(11.5), Color32::WHITE);
            let br = Rect::from_min_size(at, g.size() + vec2(12.0, 6.0));
            painter.rect_filled(br, 5.0, Color32::from_black_alpha(190));
            painter.galley(at + vec2(6.0, 3.0), g, Color32::WHITE);
        }
    }
    // guides (Keynote's yellow)
    if !guides.is_empty() {
        if let Some(pi) = moving.first().and_then(|id| app.obj_page.get(id)).copied() {
            let col = Color32::from_rgb(0xf5, 0xd0, 0x2a);
            for g in &guides {
                if let kn_ui::snap::Guide::Line(a, b) = g {
                    let (a, b) = (app.canvas.page_to_screen(&l, pi, pos2(a.x, a.y)), app.canvas.page_to_screen(&l, pi, pos2(b.x, b.y)));
                    painter.line_segment([a, b], Stroke::new(1.0, col));
                }
            }
        }
    }
    // Edit Mask: the whole picture outlined, the mask in the accent colour, the bar under it
    if let Some(id) = app.mask_edit {
        let pi = app.obj_page.get(&id).copied().unwrap_or(0);
        match (crate::mask::mask_box(app, id), app.drawables.iter().find(|d| d.id == id).map(|d| d.geom)) {
            (Some(m), Some(g)) => {
                let sr = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_max(app.canvas.page_to_screen(&l, pi, pos2(x, y)), app.canvas.page_to_screen(&l, pi, pos2(x + w, y + h)));
                let full = sr(g.x, g.y, g.w, g.h);
                painter.rect_stroke(full, 0.0, Stroke::new(1.0, Color32::from_gray(0x80)), egui::StrokeKind::Middle);
                let mr = sr(m.x, m.y, m.w, m.h);
                painter.rect_stroke(mr, 0.0, Stroke::new(2.0, theme::ACCENT()), egui::StrokeKind::Middle);
                crate::mask::mask_bar(app, ui.ctx(), mr.center_bottom() + vec2(0.0, 16.0));
            }
            _ => app.mask_edit = None,
        }
    }
    if let Some(m) = app.canvas.marquee {
        if let Some(Drag::Marquee { page, .. }) = app.canvas.drag {
            let sr = Rect::from_min_max(app.canvas.page_to_screen(&l, page, m.min), app.canvas.page_to_screen(&l, page, m.max));
            painter.rect_filled(sr, 0.0, theme::ACCENT().gamma_multiply(0.12));
            painter.rect_stroke(sr, 0.0, Stroke::new(1.0, theme::ACCENT()), egui::StrokeKind::Middle);
        }
    }
    app.layout = Some(l);
}

/// A moving object drawn at its live place (rendered once into a texture).
fn draw_live_object(app: &mut App, ui: &egui::Ui, painter: &egui::Painter, l: &Layout, pi: usize, d: &Drawable, off: Vec2) {
    let r = frame(app, d).expand(20.0);
    let scale = app.canvas.zoom * ui.ctx().pixels_per_point();
    let key = egui::Id::unique(("live-obj", d.id, (scale * 100.0) as i32, app.version));
    let tex: Option<egui::TextureHandle> = ui.ctx().data(|m| m.get_temp(key));
    let tex = match tex {
        Some(t) => t,
        None => {
            let (w, h) = ((r.width() * scale).ceil() as u32, (r.height() * scale).ceil() as u32);
            let Some(mut pm) = sk::Pixmap::new(w.max(1), h.max(1)) else { return };
            let base = sk::Transform::from_scale(scale, scale).pre_translate(-r.min.x, -r.min.y);
            if let Some(w) = &app.wb {
                app.canvas.tiles.renderer.draw_drawable(&w.doc, d, &mut pm, base);
            }
            let img = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data());
            let t = ui.ctx().load_texture("live-object", img, egui::TextureOptions::LINEAR);
            ui.ctx().data_mut(|m| m.insert_temp(key, t.clone()));
            t
        }
    };
    let r = r.translate(off);
    let sr = Rect::from_min_max(app.canvas.page_to_screen(l, pi, r.min), app.canvas.page_to_screen(l, pi, r.max));
    painter.image(tex.id(), sr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha(235));
}

/// Rectangles (page, rect in page points) covering shown body range a..b.
pub fn range_rects(l: &Layout, a: usize, b: usize) -> Vec<(usize, Rect)> {
    let mut out = Vec::new();
    if a >= b {
        return out;
    }
    for (pi, page) in l.pages.iter().enumerate() {
        if page.end < a || page.start > b {
            continue;
        }
        for line in &page.lines {
            let (ls, le) = line.range();
            if le < a || ls > b || (ls == le && !(a <= ls && ls < b)) {
                continue;
            }
            let tl = &line.piece.tl;
            let mut x0 = f32::INFINITY;
            let mut x1 = f32::NEG_INFINITY;
            for &gi in &line.piece.line_glyphs[line.line] {
                let g = &tl.glyphs[gi];
                if g.label || g.end <= a || g.start >= b {
                    continue;
                }
                x0 = x0.min(g.x);
                x1 = x1.max(g.x + g.w);
            }
            let (top, bottom) = line.band();
            // the paragraph end selected: a little room for it
            if b > le && a <= le {
                let lx = tl.lines[line.line].x + tl.lines[line.line].width;
                if x0.is_infinite() {
                    x0 = lx;
                }
                x1 = x1.max(lx + 4.0);
            }
            if x0.is_finite() && x1 > x0 {
                out.push((pi, Rect::from_min_max(pos2(x0 + line.dx, top), pos2(x1 + line.dx, bottom))));
            }
        }
    }
    out
}

/// Rectangles of a cursor's selection (page, page-point rect).
fn selection_rects(app: &mut App, l: &Layout, c: &Cursor) -> Option<Vec<(usize, Rect)>> {
    if c.is_empty() {
        return None;
    }
    let (a, b) = c.range();
    match c.owner {
        Owner::Body => {
            let body = l.body.as_ref()?;
            Some(range_rects(l, body.to_shown(a), body.to_shown(b)))
        }
        Owner::HeaderFooter { page } | Owner::Footnote { page } => {
            let p = l.pages.get(page)?;
            let (pc, body, dx, dy) = p
                .headers
                .iter()
                .chain(&p.footers)
                .find(|h| h.storage == c.storage)
                .map(|h| (h.piece.clone(), h.body.clone(), h.dx, h.dy))
                .or_else(|| p.footnotes.iter().find(|f| f.storage == c.storage).map(|f| (f.piece.clone(), f.body.clone(), f.dx, f.dy)))?;
            let (sa, sb) = (body.to_shown(a), body.to_shown(b));
            let mut out = Vec::new();
            for li in 0..pc.tl.lines.len() {
                let mut x0 = f32::INFINITY;
                let mut x1 = f32::NEG_INFINITY;
                for &gi in &pc.line_glyphs[li] {
                    let g = &pc.tl.glyphs[gi];
                    if g.label || g.end <= sa || g.start >= sb {
                        continue;
                    }
                    x0 = x0.min(g.x);
                    x1 = x1.max(g.x + g.w);
                }
                if x0.is_finite() {
                    let (t, bt) = pc.bands[li];
                    out.push((page, Rect::from_min_max(pos2(x0 + dx, t + dy), pos2(x1 + dx, bt + dy))));
                }
            }
            Some(out)
        }
        Owner::Drawable(id) => {
            let d = app.drawables.iter().find(|d| d.id == id)?.clone();
            let pi = *app.obj_page.get(&id)?;
            let (tl, g) = app.canvas.tiles.renderer.text_layout(&d)?;
            let tb = crate::text::drawable_text(&d)?;
            let (sa, sb) = (crate::text::shown_of(tb, a), crate::text::shown_of(tb, b));
            let mut out = Vec::new();
            for (li, line) in tl.lines.iter().enumerate() {
                let gs: Vec<&kn_render::text::GlyphPos> = tl.glyphs.iter().filter(|g| g.line == li && !g.label && g.end > sa && g.start < sb).collect();
                if gs.is_empty() {
                    continue;
                }
                let x0 = gs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
                let x1 = gs.iter().map(|g| g.x + g.w).fold(f32::NEG_INFINITY, f32::max);
                out.push((pi, Rect::from_min_max(pos2(g.x + x0, g.y + line.top), pos2(g.x + x1, g.y + line.top + line.height))));
            }
            Some(out)
        }
    }
}

/// The caret: (page, x, top, height) in page points.
pub fn caret_rect(app: &mut App, l: &Layout, c: &Cursor) -> Option<(usize, f32, f32, f32)> {
    match c.owner {
        Owner::Body => {
            let body = l.body.as_ref()?;
            l.caret(body.to_shown(c.caret))
        }
        Owner::HeaderFooter { page } | Owner::Footnote { page } => {
            let p = l.pages.get(page)?;
            let (pc, body, dx, dy) = p
                .headers
                .iter()
                .chain(&p.footers)
                .find(|h| h.storage == c.storage)
                .map(|h| (h.piece.clone(), h.body.clone(), h.dx, h.dy))
                .or_else(|| p.footnotes.iter().find(|f| f.storage == c.storage).map(|f| (f.piece.clone(), f.body.clone(), f.dx, f.dy)))?;
            let (x, t, h) = pg_core::flow::piece_caret(&pc, body.to_shown(c.caret));
            Some((page, x + dx, t + dy, h))
        }
        Owner::Drawable(id) => {
            let d = app.drawables.iter().find(|d| d.id == id)?.clone();
            let pi = *app.obj_page.get(&id)?;
            let tb = crate::text::drawable_text(&d)?.clone();
            let (tl, g) = app.canvas.tiles.renderer.text_layout(&d)?;
            let s = crate::text::shown_of(&tb, c.caret);
            let (x, top, h) = kn_render::text::caret(&tl, s).unwrap_or((tb.padding[0], tb.padding[1], 16.0));
            Some((pi, g.x + x, g.y + top, h))
        }
    }
}

/// What a press at page point `pt` of page `pi` lands in: an object, a header/footer field, the body.
enum Hit {
    Object(ObjId),
    HeaderFooter(ObjId),
    Footnote(ObjId),
    Body(usize),
    Page,
}

fn hit_test(app: &mut App, l: &Layout, pi: usize, pt: Pos2) -> Hit {
    let candidates: Vec<Drawable> = objects_on(app, pi).into_iter().filter(|d| app.layout_selectable || !app.layout_drawables.iter().any(|x| x.id == d.id)).collect();
    for d in candidates {
        let r = frame(app, &d);
        let thin = r.width() < 6.0 || r.height() < 6.0;
        if r.expand(if thin { 5.0 } else { 0.0 }).contains(pt) {
            return Hit::Object(d.id);
        }
    }
    // section layout objects, once Make Layout Objects Selectable is on: picked on this page
    if app.layout_selectable {
        for d in page_layout_drawables(app, l, pi).into_iter().rev() {
            let r = frame(app, &d);
            if r.contains(pt) {
                if !app.drawables.iter().any(|x| x.id == d.id) {
                    app.drawables.push(d.clone());
                    app.drawable_page.push(pi);
                }
                app.obj_page.insert(d.id, pi);
                return Hit::Object(d.id);
            }
        }
    }
    let page = &l.pages[pi];
    let s = &l.setup;
    // Pages: the header / footer fields take a click inside them; elsewhere the margins go to
    // the nearest line of the body
    let in_margin = pt.y < s.top || pt.y > s.height - s.bottom;
    if in_margin {
        if let Some(f) = page.footnotes.iter().find(|f| pt.y >= f.dy - 2.0 && pt.y <= f.dy + f.piece.tl.content_h + 2.0) {
            return Hit::Footnote(f.storage);
        }
        if let Some((_, st, _)) = hf_fields(l, pi).into_iter().find(|(r, _, _)| r.expand(2.0).contains(pt)) {
            return Hit::HeaderFooter(st);
        }
        if l.body.is_some() {
            if let Some(sh) = l.hit(pi, pt.x, pt.y.clamp(s.top + 1.0, s.height - s.bottom - 1.0)) {
                return Hit::Body(sh);
            }
        }
    }
    if let Some(f) = page.footnotes.iter().find(|f| pt.y >= f.dy - 2.0 && pt.y <= f.dy + f.piece.tl.content_h + 2.0) {
        return Hit::Footnote(f.storage);
    }
    if l.body.is_some() {
        if let Some(s) = l.hit(pi, pt.x, pt.y) {
            return Hit::Body(s);
        }
    }
    Hit::Page
}

/// The header and footer fields of page `pi` (page points, at least a line tall): (rect,
/// storage, footer). Pages shows them as boxes when the pointer is over the band.
pub fn hf_fields(l: &Layout, pi: usize) -> Vec<(Rect, ObjId, bool)> {
    let Some(page) = l.pages.get(pi) else { return Vec::new() };
    page.headers
        .iter()
        .chain(&page.footers)
        .map(|h| {
            let hh = h.frame.3.max(18.0);
            let y = if h.footer { h.frame.1 + h.frame.3 - hh } else { h.frame.1 };
            (Rect::from_min_size(pos2(h.frame.0, y), vec2(h.frame.2, hh)), h.storage, h.footer)
        })
        .collect()
}

/// The stored offset under page point `pt` in a header/footer/footnote piece.
fn piece_hit_stored(l: &Layout, pi: usize, storage: ObjId, pt: Pos2) -> Option<usize> {
    let p = l.pages.get(pi)?;
    let (pc, body, dx, dy) = p
        .headers
        .iter()
        .chain(&p.footers)
        .find(|h| h.storage == storage)
        .map(|h| (h.piece.clone(), h.body.clone(), h.dx, h.dy))
        .or_else(|| p.footnotes.iter().find(|f| f.storage == storage).map(|f| (f.piece.clone(), f.body.clone(), f.dx, f.dy)))?;
    Some(body.to_stored(pg_core::flow::piece_hit(&pc, pt.x - dx, pt.y - dy)))
}

/// The stored offset under page point `pt` in a text box's text.
fn drawable_hit_stored(app: &mut App, id: ObjId, pt: Pos2) -> Option<usize> {
    let d = app.drawables.iter().find(|d| d.id == id)?.clone();
    let tb = crate::text::drawable_text(&d)?.clone();
    let (tl, g) = app.canvas.tiles.renderer.text_layout(&d)?;
    let s = kn_render::text::hit_index(&tl, pt.x - g.x, pt.y - g.y);
    Some(tb.stored_offset(s))
}

/// Word boundaries around stored byte `at` of `text`.
pub fn word_at(text: &str, at: usize) -> (usize, usize) {
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '_';
    let mut a = at.min(text.len());
    while a > 0 {
        let c = text[..a].chars().next_back().unwrap();
        if !is_word(c) {
            break;
        }
        a -= c.len_utf8();
    }
    let mut b = at.min(text.len());
    while b < text.len() {
        let c = text[b..].chars().next().unwrap();
        if !is_word(c) {
            break;
        }
        b += c.len_utf8();
    }
    if a == b && b < text.len() {
        b += text[b..].chars().next().map(|c| c.len_utf8()).unwrap_or(0);
    }
    (a, b)
}

fn para_at(text: &str, at: usize) -> (usize, usize) {
    let at = at.min(text.len());
    let a = text[..at].rfind(|c: char| pg_core::body::is_para_end(c)).map(|i| i + 1).unwrap_or(0);
    let b = text[at..].find(|c: char| pg_core::body::is_para_end(c)).map(|i| at + i).unwrap_or(text.len());
    (a, b)
}

/// GNOME's overlay scrollbar at the right edge: a thin pill that widens when pointed at; drag
/// the thumb, or click the track to page. True while the pointer is its.
fn scrollbar(app: &mut App, ui: &egui::Ui) -> bool {
    let Some(l) = app.layout.as_ref() else { return false };
    let rect = app.canvas.rect;
    let size = doc_size(&app.canvas, l);
    let view = rect.height() / app.canvas.zoom;
    if size.y <= view + 1.0 {
        return false;
    }
    let top = app.toolbar_band.map(|b| b.max.y.max(rect.min.y)).unwrap_or(rect.min.y) + 4.0;
    let track = Rect::from_min_max(pos2(rect.max.x - 14.0, top), pos2(rect.max.x - 2.0, rect.max.y - 4.0));
    let max = size.y - view;
    let thumb_h = (track.height() * view / size.y).clamp(36.0, track.height());
    let thumb_y = track.min.y + (track.height() - thumb_h) * (app.canvas.scroll.y / max).clamp(0.0, 1.0);
    let resp = ui.interact(track, ui.scope_id().with("page-scrollbar"), Sense::click_and_drag());
    let hot = resp.hovered() || resp.dragged() || app.canvas.sb_drag.is_some();
    let w = if hot { 9.0 } else { 5.0 };
    let thumb = Rect::from_min_size(pos2(track.max.x - 2.0 - w, thumb_y), vec2(w, thumb_h));
    let p = ui.painter();
    if hot {
        p.rect_filled(Rect::from_min_max(pos2(track.max.x - 3.0 - w, track.min.y), pos2(track.max.x - 1.0, track.max.y)), 6.0, theme::gray(0x80).gamma_multiply(0.12));
    }
    let ink = if theme::is_light() { Color32::from_black_alpha(if hot { 150 } else { 110 }) } else { Color32::from_white_alpha(if hot { 170 } else { 120 }) };
    p.rect_filled(thumb, w / 2.0, ink);
    if resp.drag_started() {
        if let Some(q) = ui.input(|i| i.pointer.press_origin()) {
            if thumb.expand2(vec2(6.0, 0.0)).contains(q) {
                app.canvas.sb_drag = Some((q.y, app.canvas.scroll.y));
            } else {
                // a drag that starts on the track: the thumb jumps under the pointer first
                let s = ((q.y - track.min.y - thumb_h / 2.0) / (track.height() - thumb_h)).clamp(0.0, 1.0) * max;
                app.canvas.scroll.y = s;
                app.canvas.sb_drag = Some((q.y, s));
            }
        }
    }
    if let (Some((y0, s0)), Some(q)) = (app.canvas.sb_drag, ui.input(|i| i.pointer.latest_pos())) {
        let k = max / (track.height() - thumb_h).max(1.0);
        app.canvas.scroll.y = (s0 + (q.y - y0) * k).clamp(0.0, max);
        app.canvas.kinetic = Vec2::ZERO;
        ui.ctx().request_repaint();
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        app.canvas.sb_drag = None;
    }
    if resp.clicked() {
        if let Some(q) = resp.interact_pointer_pos() {
            let page = view * 0.9;
            if q.y < thumb.min.y {
                app.canvas.scroll.y = (app.canvas.scroll.y - page).max(0.0);
            } else if q.y > thumb.max.y {
                app.canvas.scroll.y = (app.canvas.scroll.y + page).min(max);
            }
        }
    }
    hot
}

fn interact(app: &mut App, ui: &egui::Ui, resp: &egui::Response) {
    let ctx = ui.ctx().clone();
    let Some(l) = app.layout.take() else { return };
    let (pressed, released, pos, mods, now, secondary) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.interact_pos(), i.modifiers, i.time, i.pointer.secondary_pressed()));
    let in_canvas = pos.is_some_and(|p| app.canvas.rect.contains(p)) && ctx.layer_id_at(pos.unwrap_or_default()).is_none_or(|l| l.order == egui::Order::Background);
    // cursor shape
    if let Some(p) = pos.filter(|_| in_canvas && app.canvas.drag.is_none()) {
        if let Some((pi, pt)) = app.canvas.hit_page(&l, p) {
            let on_handle = handle_under(app, &l, p).is_some();
            let icon = if on_handle {
                egui::CursorIcon::ResizeNwSe
            } else {
                match hit_test(app, &l, pi, pt) {
                    Hit::Object(id) if app.canvas.editing == Some(id) => egui::CursorIcon::Text,
                    Hit::Object(_) => egui::CursorIcon::Default,
                    Hit::Body(_) | Hit::HeaderFooter(_) | Hit::Footnote(_) => egui::CursorIcon::Text,
                    Hit::Page => {
                        if l.body.is_some() {
                            egui::CursorIcon::Text
                        } else {
                            egui::CursorIcon::Default
                        }
                    }
                }
            };
            ctx.set_cursor_icon(icon);
        }
    }
    // right click: the context menu for what's there
    if secondary && in_canvas {
        if let Some((pi, pt)) = pos.and_then(|p| app.canvas.hit_page(&l, p)) {
            match hit_test(app, &l, pi, pt) {
                Hit::Object(id) => {
                    if !app.selected_objs().contains(&id) {
                        select_object(app, id);
                    }
                }
                Hit::Body(s) => {
                    // keep a selection the click is inside; else move the caret there
                    let stored = l.body.as_ref().map(|b| b.to_stored(s)).unwrap_or(0);
                    let inside = app.cursor.is_some_and(|c| c.owner == Owner::Body && !c.is_empty() && (c.range().0..=c.range().1).contains(&stored));
                    if !inside {
                        place_caret(app, &l, Owner::Body, None, stored, false);
                    }
                }
                _ => {}
            }
        }
    }
    resp.context_menu(|ui| crate::menus::context_menu(app, ui));
    if pressed && in_canvas {
        let p = pos.unwrap();
        // a click on the page takes the keyboard back from any field (the text gets it)
        ctx.memory_mut(|m| {
            if let Some(f) = m.focused() {
                m.surrender_focus(f);
            }
        });
        app.canvas.blink = now;
        // double / triple clicks
        let (t, lp, n) = app.canvas.clicks;
        let count = if now - t < 0.45 && lp.distance(p) < 5.0 { n + 1 } else { 1 };
        app.canvas.clicks = (now, p, count);
        if let Some(h) = handle_under(app, &l, p) {
            let (id, handle) = h;
            let d = app.drawables.iter().find(|d| d.id == id).cloned();
            if let Some(d) = d.filter(|d| !d.locked) {
                let g0 = crate::mask::mask_box(app, id).unwrap_or_else(|| app.canvas.tiles.effective(&d));
                let pi = app.obj_page.get(&id).copied().unwrap_or(0);
                let pt = app.canvas.to_doc(p) - page_rect(&app.canvas, &l, pi).min;
                if handle == 200 {
                    let g = d.geom;
                    let q = g.to_local(pg_core::kn_model::Point { x: pt.x, y: pt.y });
                    app.canvas.drag = Some(Drag::Control { id, g, q });
                } else if handle >= 100 {
                    if let Some((a, b)) = line_ends(&d) {
                        app.canvas.drag = Some(Drag::LineEnd { id, end: handle - 100, a, b });
                    }
                } else if mods.command {
                    let c = pos2(g0.x + g0.w / 2.0, g0.y + g0.h / 2.0);
                    let a0 = (pt.y - c.y).atan2(pt.x - c.x).to_degrees();
                    app.canvas.drag = Some(Drag::Rotate { id, g0, a0 });
                } else {
                    app.canvas.drag = Some(Drag::Resize { id, handle, start: pt.to_pos2(), g0 });
                }
            }
        } else if let Some((pi, pt)) = app.canvas.hit_page(&l, p) {
            match hit_test(app, &l, pi, pt) {
                Hit::Object(id) => {
                    let d = app.drawables.iter().find(|d| d.id == id).cloned().unwrap();
                    let has_text = matches!(&d.kind, DrawableKind::Shape(s) if s.text.is_some() && !s.is_line);
                    if app.canvas.editing == Some(id) {
                        // inside the text being edited: the caret
                        if let Some(s) = drawable_hit_stored(app, id, pt) {
                            let st = crate::text::drawable_text(&d).map(|t| t.storage).unwrap_or(0);
                            place_caret(app, &l, Owner::Drawable(id), Some(st), s, mods.shift);
                            app.canvas.drag = Some(Drag::Text { owner: Owner::Drawable(id), storage: st, anchor: app.cursor.map(|c| c.anchor).unwrap_or(s), word: None });
                            if count == 2 {
                                select_word(app, st, Owner::Drawable(id), s);
                            }
                        }
                    } else if let (true, DrawableKind::Image(img)) = (count >= 2, &d.kind) && img.equation.is_some() {
                        // double-click an equation: edit its LaTeX
                        app.equation = img.equation.clone();
                        app.equation_target = Some(id);
                    } else if count >= 2 && has_text {
                        // double-click a text box: edit its text
                        crate::text::edit_drawable_text(app, id, Some(pt));
                    } else if count >= 2 && matches!(d.kind, DrawableKind::Chart(_)) {
                        // double-click a chart: its data
                        select_object(app, id);
                        crate::chartedit::open(app, id);
                    } else if count >= 2 && matches!(d.kind, DrawableKind::Table(_)) {
                        // double-click a cell: edit it
                        crate::tables::edit_cell_at(app, id, pt);
                    } else if mods.shift && app.sel.is_some_and(|s| s.table == id) {
                        // Shift-click in a table with cells selected: extend the cell range
                        crate::tables::extend_to(app, id, pt);
                    } else if matches!(d.kind, DrawableKind::Table(_)) && !mods.shift && !mods.command && app.selected_objs() == [id] {
                        // a selected table: the cell, and dragging selects across cells (the
                        // table itself moves by its round corner button)
                        crate::tables::press_cell(app, id, pt);
                    } else {
                        if mods.shift || mods.command {
                            toggle_object(app, id);
                        } else if !app.selected_objs().contains(&id) {
                            select_object(app, id);
                        }
                        let frames: Vec<(ObjId, Geometry)> = app.selected_objs().iter().filter_map(|i| app.drawables.iter().find(|d| d.id == *i)).filter(|d| !d.locked).map(|d| (d.id, d.geom)).collect();
                        if !frames.is_empty() {
                            app.canvas.drag = Some(Drag::Move { page: pi, start: pt, frames, moved: false });
                        }
                    }
                }
                Hit::HeaderFooter(st) | Hit::Footnote(st) => {
                    let owner = if l.pages[pi].footnotes.iter().any(|f| f.storage == st) { Owner::Footnote { page: pi } } else { Owner::HeaderFooter { page: pi } };
                    deselect_objects(app);
                    if let Some(s) = piece_hit_stored(&l, pi, st, pt) {
                        place_caret(app, &l, owner, Some(st), s, mods.shift);
                        // double-click: the word, triple-click: the paragraph (a drag then
                        // extends by words, as in the body)
                        match count {
                            2 => select_word(app, st, owner, s),
                            n if n >= 3 => {
                                let text = app.doc().map(|d| d.storage_text(st)).unwrap_or_default();
                                let (a, b) = para_at(&text, s);
                                app.cursor = Some(Cursor { storage: st, owner, anchor: a, caret: b, goal_x: None });
                            }
                            _ => {}
                        }
                        let word = if count >= 2 { app.cursor.map(|c| c.range()) } else { None };
                        app.canvas.drag = Some(Drag::Text { owner, storage: st, anchor: app.cursor.map(|c| c.anchor).unwrap_or(s), word });
                    }
                }
                Hit::Body(shown) => {
                    deselect_objects(app);
                    let body = l.body.as_ref().unwrap();
                    let stored = body.to_stored(shown);
                    let st = body.storage;
                    // a click on a link opens it (Ctrl-click while editing, Pages: click)
                    if mods.command && count == 1 {
                        if let Some(url) = body.spans.iter().find_map(|s| match &s.kind {
                            pg_core::body::SpanKind::Link(u) if s.start <= shown && shown < s.end => Some(u.clone()),
                            _ => None,
                        }) {
                            crate::menus::open_link(app, &url);
                            app.layout = Some(l);
                            return;
                        }
                    }
                    match count {
                        2 => select_word(app, st, Owner::Body, stored),
                        n if n >= 3 => {
                            let text = app.doc().map(|d| d.storage_text(st)).unwrap_or_default();
                            let (a, b) = para_at(&text, stored);
                            app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: a, caret: b, goal_x: None });
                        }
                        _ => place_caret(app, &l, Owner::Body, Some(st), stored, mods.shift),
                    }
                    let word = if count == 2 { app.cursor.map(|c| c.range()) } else { None };
                    app.canvas.drag = Some(Drag::Text { owner: Owner::Body, storage: st, anchor: app.cursor.map(|c| c.anchor).unwrap_or(stored), word });
                }
                Hit::Page => {
                    deselect_objects(app);
                    app.cursor = None;
                    app.sel = None;
                    if l.body.is_none() {
                        app.canvas.drag = Some(Drag::Marquee { page: pi, start: pt });
                        app.canvas.marquee = Some(Rect::from_min_size(pt, Vec2::ZERO));
                    }
                }
            }
        } else {
            // outside the pages: nothing selected
            deselect_objects(app);
            app.cursor = None;
        }
    }
    // dragging
    if let (Some(drag), Some(p)) = (app.canvas.drag.clone(), pos) {
        if ctx.input(|i| i.pointer.primary_down()) {
            match drag {
                Drag::Text { owner, storage, anchor, word } => {
                    let (pi, pt) = app.canvas.near_page(&l, p);
                    let s = match owner {
                        Owner::Body => l.hit(pi, pt.x, pt.y).and_then(|s| l.body.as_ref().map(|b| b.to_stored(s))),
                        Owner::HeaderFooter { page } | Owner::Footnote { page } => piece_hit_stored(&l, page, storage, pt),
                        Owner::Drawable(id) => drawable_hit_stored(app, id, pt),
                    };
                    if let Some(s) = s {
                        let (a, b) = match word {
                            Some((wa, wb)) => {
                                let text = app.doc().map(|d| d.storage_text(storage)).unwrap_or_default();
                                let (xa, xb) = word_at(&text, s);
                                if s < wa { (wb, xa) } else { (wa, xb.max(wb)) }
                            }
                            None => (anchor, s),
                        };
                        app.cursor = Some(Cursor { storage, owner, anchor: a, caret: b, goal_x: None });
                    }
                    // drag near the edges scrolls
                    let r = app.canvas.rect;
                    if p.y > r.max.y - 20.0 {
                        app.canvas.scroll.y += 12.0 / app.canvas.zoom;
                    } else if p.y < r.min.y + 20.0 {
                        app.canvas.scroll.y -= 12.0 / app.canvas.zoom;
                    }
                    ctx.request_repaint();
                }
                Drag::Move { page, start, frames, moved } => {
                    let pt = app.canvas.to_doc(p) - page_rect(&app.canvas, &l, page).min;
                    let mut d = pt - start.to_vec2();
                    if !moved && d.length() * app.canvas.zoom < 3.0 {
                        // not yet a drag
                    } else {
                        if mods.shift {
                            if d.x.abs() > d.y.abs() {
                                d.y = 0.0;
                            } else {
                                d.x = 0.0;
                            }
                        }
                        // snapping to the page and the other objects
                        let (snapped, guides) = snap_move(app, &l, page, &frames, d);
                        app.canvas.live = Some((snapped, guides));
                        app.canvas.drag = Some(Drag::Move { page, start, frames, moved: true });
                    }
                }
                Drag::Resize { id, handle, start, g0 } => {
                    let pi = app.obj_page.get(&id).copied().unwrap_or(0);
                    let pt = app.canvas.to_doc(p) - page_rect(&app.canvas, &l, pi).min;
                    let d = pt - start.to_vec2();
                    let mut g = g0;
                    let (mut x0, mut y0, mut x1, mut y1) = (g0.x, g0.y, g0.x + g0.w, g0.y + g0.h);
                    match handle {
                        0 => {
                            x0 += d.x;
                            y0 += d.y;
                        }
                        1 => y0 += d.y,
                        2 => {
                            x1 += d.x;
                            y0 += d.y;
                        }
                        3 => x0 += d.x,
                        4 => x1 += d.x,
                        5 => {
                            x0 += d.x;
                            y1 += d.y;
                        }
                        6 => y1 += d.y,
                        _ => {
                            x1 += d.x;
                            y1 += d.y;
                        }
                    }
                    let (w, h) = ((x1 - x0).max(4.0), (y1 - y0).max(if g0.h < 1.0 { 0.0 } else { 4.0 }));
                    g.x = x0.min(x1);
                    g.y = y0.min(y1);
                    g.w = w;
                    g.h = h;
                    // keep the proportions: Shift, pictures and corners of locked-ratio objects
                    let ratio = mods.shift || app.drawables.iter().find(|x| x.id == id).is_some_and(|x| matches!(x.kind, DrawableKind::Image(_)));
                    if ratio && g0.w > 0.0 && g0.h > 0.0 && matches!(handle, 0 | 2 | 5 | 7) {
                        let k = (w / g0.w).max(h / g0.h);
                        g.w = g0.w * k;
                        g.h = g0.h * k;
                        if matches!(handle, 0 | 5) {
                            g.x = x1 - g.w;
                        }
                        if matches!(handle, 0 | 2) {
                            g.y = y1 - g.h;
                        }
                    }
                    app.canvas.live_frame = Some((id, g));
                }
                Drag::Rotate { id, g0, a0 } => {
                    let pi = app.obj_page.get(&id).copied().unwrap_or(0);
                    let pt = app.canvas.to_doc(p) - page_rect(&app.canvas, &l, pi).min;
                    let c = pos2(g0.x + g0.w / 2.0, g0.y + g0.h / 2.0);
                    let a = (pt.y - c.y).atan2(pt.x - c.x).to_degrees();
                    let mut g = g0;
                    g.angle = (g0.angle - (a - a0)).rem_euclid(360.0);
                    if mods.shift {
                        g.angle = (g.angle / 45.0).round() * 45.0 % 360.0;
                    }
                    app.canvas.live_frame = Some((id, g));
                }
                Drag::Marquee { page, start } => {
                    let pt = app.canvas.to_doc(p) - page_rect(&app.canvas, &l, page).min;
                    app.canvas.marquee = Some(Rect::from_two_pos(start, pt.to_pos2()));
                }
                Drag::Control { id, g, .. } => {
                    let pi = app.obj_page.get(&id).copied().unwrap_or(0);
                    let pt = app.canvas.to_doc(p) - page_rect(&app.canvas, &l, pi).min;
                    let q = g.to_local(pg_core::kn_model::Point { x: pt.x, y: pt.y });
                    app.canvas.drag = Some(Drag::Control { id, g, q });
                }
                Drag::LineEnd { id, end, mut a, mut b } => {
                    let pi = app.obj_page.get(&id).copied().unwrap_or(0);
                    let pt = app.canvas.to_doc(p) - page_rect(&app.canvas, &l, pi).min;
                    let mut q = pg_core::kn_model::Point { x: pt.x, y: pt.y };
                    // Shift: every 45 degrees from the other end
                    let other = if end == 0 { b } else { a };
                    if mods.shift {
                        let (dx, dy) = (q.x - other.x, q.y - other.y);
                        let len = (dx * dx + dy * dy).sqrt();
                        let ang = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
                        q = pg_core::kn_model::Point { x: other.x + len * ang.cos(), y: other.y + len * ang.sin() };
                    }
                    if end == 0 {
                        a = q;
                    } else {
                        b = q;
                    }
                    app.canvas.drag = Some(Drag::LineEnd { id, end, a, b });
                }
            }
        }
    }
    if released {
        if let Some(drag) = app.canvas.drag.take() {
            match drag {
                Drag::Move { frames, moved: true, page, .. } => {
                    if let Some((d, _)) = app.canvas.live.take() {
                        let ids: Vec<ObjId> = frames.iter().map(|f| f.0).collect();
                        // dropped onto another page (Stay on Page objects move pages)
                        let target = pos.map(|p| app.canvas.near_page(&l, p).0).unwrap_or(page);
                        if ids.len() == 1 && app.mask_edit == Some(ids[0]) {
                            // editing its mask: the picture moves under the mask
                            crate::mask::move_under_mask(app, ids[0], d.x, d.y);
                        } else {
                            crate::insert::move_objects(app, &ids, d.x, d.y, page, target);
                        }
                    }
                }
                Drag::Resize { id, g0, .. } if crate::mask::mask_box(app, id).is_some() => {
                    // a masked picture: Edit Mask crops (the mask, within the picture); otherwise
                    // the picture and its mask scale together
                    if let (Some((_, g)), Some(img)) = (app.canvas.live_frame.take(), app.drawables.iter().find(|d| d.id == id).map(|d| d.geom)) {
                        if app.mask_edit == Some(id) {
                            let (x0, y0) = (g.x.max(img.x), g.y.max(img.y));
                            let (x1, y1) = ((g.x + g.w).min(img.x + img.w), (g.y + g.h).min(img.y + img.h));
                            if x1 - x0 >= 4.0 && y1 - y0 >= 4.0 {
                                app.edit("Edit Mask", |d, h| d.set_mask_rect(h, id, x0 - img.x, y0 - img.y, x1 - x0, y1 - y0));
                            }
                        } else {
                            let (kx, ky) = (g.w / g0.w.max(0.01), g.h / g0.h.max(0.01));
                            let ng = Geometry { x: g.x - (g0.x - img.x) * kx, y: g.y - (g0.y - img.y) * ky, w: img.w * kx, h: img.h * ky, ..img };
                            app.edit("Resize", |d, h| {
                                d.set_frame(h, id, ng);
                                d.set_mask_rect(h, id, (g0.x - img.x) * kx, (g0.y - img.y) * ky, g.w, g.h);
                            });
                        }
                    }
                }
                Drag::Resize { id, g0, .. } if app.table(id).is_some() => {
                    // a table: its rows and columns scale with it (it draws at their sizes)
                    if let Some((_, g)) = app.canvas.live_frame.take() {
                        let (kx, ky) = (g.w / g0.w.max(0.01), g.h / g0.h.max(0.01));
                        app.edit("Resize", |d, h| {
                            let Some(t) = d.file.obj(id).and_then(|m| pg_core::kn_model::table::table(&d.file, m)) else { return };
                            let rows: Vec<(u32, f32)> = t.row_heights.iter().enumerate().map(|(i, v)| (i as u32, (v * ky).max(8.0))).collect();
                            let cols: Vec<(u32, f32)> = t.col_widths.iter().enumerate().map(|(i, v)| (i as u32, (v * kx).max(12.0))).collect();
                            d.set_frame(h, id, g);
                            d.set_table_sizes(h, id, true, &rows);
                            d.set_table_sizes(h, id, false, &cols);
                        });
                    }
                }
                Drag::Resize { .. } | Drag::Rotate { .. } => {
                    if let Some((id, g)) = app.canvas.live_frame.take() {
                        app.edit_pages("Resize", |p, h| {
                            p.doc.set_frame(h, id, g);
                            p.update_connections(h, &[id]);
                        });
                    }
                }
                Drag::Control { id, g, q } => {
                    app.edit("Adjust Shape", |d, h| d.set_shape_control(h, id, g.w, g.h, q));
                }
                Drag::LineEnd { id, a, b, .. } => {
                    app.edit("Move Line Point", |d, h| d.set_line_points(h, id, a, b));
                }
                Drag::Marquee { page, .. } => {
                    if let Some(m) = app.canvas.marquee.take() {
                        let hits: Vec<ObjId> = objects_on(app, page).into_iter().filter(|d| m.intersects(Rect::from_min_size(pos2(d.geom.x, d.geom.y), vec2(d.geom.w.max(1.0), d.geom.h.max(1.0))))).map(|d| d.id).collect();
                        app.canvas.objs = hits.clone();
                        app.canvas.obj = hits.first().copied();
                    }
                }
                _ => {}
            }
        }
        app.canvas.live = None;
        app.canvas.live_frame = None;
        app.canvas.marquee = None;
    }
    // files dropped on the canvas: pictures go in where they're dropped
    let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
    for p in dropped {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        use kn_ui::media::{AUDIO_EXT, IMAGE_EXT, MOVIE_EXT};
        if IMAGE_EXT.iter().chain(MOVIE_EXT).chain(AUDIO_EXT).any(|e| *e == ext) {
            app.layout = Some(l);
            crate::media::insert_path(app, &p);
            return;
        } else if matches!(ext.as_str(), "pages" | "docx" | "rtf" | "txt" | "md") {
            app.layout = Some(l);
            crate::tabs::open_in_tab(app, &p);
            return;
        }
    }
    app.layout = Some(l);
}

/// The handle under a screen point of the selected object: (object, handle index).
fn handle_under(app: &mut App, l: &Layout, p: Pos2) -> Option<(ObjId, usize)> {
    if app.canvas.editing.is_some() || app.points.id.is_some() {
        return None;
    }
    let single = app.selected_objs().len() == 1;
    for id in app.selected_objs() {
        if app.sel.is_some_and(|s| s.table == id) {
            continue;
        }
        let d = app.drawables.iter().find(|d| d.id == id)?.clone();
        let pi = *app.obj_page.get(&id)?;
        if single {
            let at = |q: pg_core::kn_model::Point| app.canvas.page_to_screen(l, pi, pos2(q.x, q.y));
            if let Some(c) = control_point(app, &d) {
                if at(c).distance(p) < 8.0 {
                    return Some((id, 200));
                }
            }
            if let Some((a, b)) = line_ends(&d) {
                if !d.locked {
                    for (k, q) in [a, b].into_iter().enumerate() {
                        if at(q).distance(p) < 8.0 {
                            return Some((id, 100 + k));
                        }
                    }
                }
                continue;
            }
        }
        let r = frame(app, &d);
        let sr = Rect::from_min_max(app.canvas.page_to_screen(l, pi, r.min), app.canvas.page_to_screen(l, pi, r.max));
        for (k, h) in handle_points(sr).iter().enumerate() {
            if h.distance(p) < 7.0 {
                return Some((id, k));
            }
        }
    }
    None
}

fn snap_move(app: &App, l: &Layout, page: usize, frames: &[(ObjId, Geometry)], d: Vec2) -> (Vec2, Vec<kn_ui::snap::Guide>) {
    use kn_ui::snap::{Bx, Opts};
    use pg_core::kn_model::Point;
    let Some((x0, y0, x1, y1)) = frames.iter().fold(None, |acc: Option<(f32, f32, f32, f32)>, (_, g)| {
        let b = (g.x, g.y, g.x + g.w, g.y + g.h);
        Some(acc.map_or(b, |a| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))))
    }) else {
        return (d, Vec::new());
    };
    let moving: Vec<ObjId> = frames.iter().map(|f| f.0).collect();
    let others: Vec<Bx> = app
        .drawables_on(page)
        .filter(|o| !moving.contains(&o.id))
        .map(|o| Bx::new(Point { x: o.geom.x, y: o.geom.y }, Point { x: o.geom.x + o.geom.w, y: o.geom.y + o.geom.h }))
        .collect();
    let s = &l.setup;
    // the page's margins act as guides too (Pages snaps to them)
    let mut lines = vec![(0, s.left), (0, s.width - s.right), (1, s.top), (1, s.height - s.bottom)];
    lines.extend(crate::guides::snap_lines(app, page));
    kn_ui::snap::set_user_guides(lines);
    let slide = Bx::new(Point { x: 0.0, y: 0.0 }, Point { x: s.width, y: s.height });
    let o = Opts { center: app.settings.guides_center, edges: app.settings.guides_edges, spacing: app.settings.guides_spacing, sizing: app.settings.guides_sizing, tol: 5.0 / app.canvas.zoom };
    let ((dx, dy), guides) = kn_ui::snap::snap_move(Bx::new(Point { x: x0, y: y0 }, Point { x: x1, y: y1 }), &others, slide, (d.x, d.y), &o);
    (vec2(dx, dy), guides)
}

/// Start moving object `id` from screen point `q` (a table's corner button).
pub fn start_move(app: &mut App, id: ObjId, page: usize, q: Pos2) {
    let Some(l) = app.layout.as_ref() else { return };
    let pt = app.canvas.to_doc(q) - page_rect(&app.canvas, l, page).min;
    if let Some(d) = app.drawables.iter().find(|d| d.id == id) {
        app.canvas.drag = Some(Drag::Move { page, start: pt.to_pos2(), frames: vec![(id, d.geom)], moved: false });
    }
}

pub fn select_object(app: &mut App, id: ObjId) {
    crate::text::finish_typing(app);
    app.canvas.obj = Some(id);
    app.canvas.objs = vec![id];
    if app.canvas.editing != Some(id) {
        app.canvas.editing = None;
    }
    if !matches!(app.cursor, Some(Cursor { owner: Owner::Drawable(o), .. }) if o == id) {
        // the body caret is kept (hidden) so Escape-less clicks back go on from there
        if !matches!(app.cursor, Some(Cursor { owner: Owner::Body, .. })) {
            app.cursor = None;
        }
    }
    if app.sel.is_some_and(|s| s.table != id) {
        app.sel = None;
    }
    app.obj_tab = 0;
}

fn toggle_object(app: &mut App, id: ObjId) {
    let mut v = app.selected_objs();
    if let Some(i) = v.iter().position(|x| *x == id) {
        v.remove(i);
    } else {
        v.push(id);
    }
    app.canvas.obj = v.first().copied();
    app.canvas.objs = v;
}

pub fn deselect_objects(app: &mut App) {
    app.canvas.obj = None;
    app.canvas.objs.clear();
    app.canvas.editing = None;
    app.sel = None;
}

fn select_word(app: &mut App, storage: ObjId, owner: Owner, at: usize) {
    let text = app.doc().map(|d| d.storage_text(storage)).unwrap_or_default();
    let (a, b) = word_at(&text, at);
    app.cursor = Some(Cursor { storage, owner, anchor: a, caret: b, goal_x: None });
}

fn place_caret(app: &mut App, _l: &Layout, owner: Owner, storage: Option<ObjId>, at: usize, extend: bool) {
    let storage = storage.or_else(|| app.wb.as_ref().and_then(|w| w.body_storage())).unwrap_or(0);
    if app.cursor.is_some_and(|c| c.storage != storage) {
        crate::text::finish_typing(app);
    }
    let anchor = match app.cursor {
        Some(c) if extend && c.storage == storage => c.anchor,
        _ => at,
    };
    app.cursor = Some(Cursor { storage, owner, anchor, caret: at, goal_x: None });
    app.typing_props.clear();
    if !matches!(owner, Owner::Drawable(_)) {
        app.canvas.editing = None;
    }
}

#[allow(dead_code)]
fn unused(_s: Sel) {}

/// CS_LINES (automated checks): the first page's lines on screen.
pub fn dump_lines(app: &App) {
    let Some(l) = &app.layout else { return };
    let Some(p) = l.pages.get(app.canvas.page_in_view) else { return };
    for (i, line) in p.lines.iter().take(12).enumerate() {
        let (a, b) = line.band();
        let x0 = line.x();
        let tl = app.canvas.page_to_screen(l, p.index, pos2(x0, a));
        let br = app.canvas.page_to_screen(l, p.index, pos2(x0 + line.piece.tl.lines[line.line].width, b));
        eprintln!("line {i}: screen {:.0},{:.0} .. {:.0},{:.0} text {:?}", tl.x, tl.y, br.x, br.y, l.body.as_ref().map(|bd| bd.tb.text[line.range().0..line.range().1].to_string()));
    }
    for d in &app.drawables {
        if let Some(pi) = app.obj_page.get(&d.id) {
            let a = app.canvas.page_to_screen(l, *pi, pos2(d.geom.x, d.geom.y));
            let b = app.canvas.page_to_screen(l, *pi, pos2(d.geom.x + d.geom.w, d.geom.y + d.geom.h));
            eprintln!("object {} page {pi}: screen {:.0},{:.0} .. {:.0},{:.0}", d.id, a.x, a.y, b.x, b.y);
        }
    }
}

/// A path as polylines in its own coordinates (curves sampled).
fn flatten(path: &pg_core::kn_model::Path) -> Vec<Vec<pg_core::kn_model::Point>> {
    use pg_core::kn_model::{PathEl, Point};
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    let mut last = Point { x: 0.0, y: 0.0 };
    let mut start = last;
    for e in &path.els {
        match *e {
            PathEl::Move(p) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur = vec![p];
                last = p;
                start = p;
            }
            PathEl::Line(p) => {
                cur.push(p);
                last = p;
            }
            PathEl::Quad(c, p) => {
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    cur.push(Point { x: u * u * last.x + 2.0 * u * t * c.x + t * t * p.x, y: u * u * last.y + 2.0 * u * t * c.y + t * t * p.y });
                }
                last = p;
            }
            PathEl::Cubic(c1, c2, p) => {
                for i in 1..=10 {
                    let t = i as f32 / 10.0;
                    let u = 1.0 - t;
                    cur.push(Point {
                        x: u * u * u * last.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p.x,
                        y: u * u * u * last.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p.y,
                    });
                }
                last = p;
            }
            PathEl::Close => {
                cur.push(start);
                last = start;
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}
