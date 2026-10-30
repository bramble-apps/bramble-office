//! The sheet canvas is drawn by Keydeck's renderer (tiny-skia) into 512 px tiles, cached per
//! zoom. Only tiles in view are rendered, a few per frame, so scrolling a big sheet stays at the
//! display's refresh rate; after an edit the old tiles stay on screen until their replacements
//! are ready (no white flash).

use std::collections::HashMap;
use std::time::Instant;

use kn_render::Renderer;
use nm_core::kn_format::ObjId;
use nm_core::kn_model::{CellRange, Document, Drawable, DrawableKind, Geometry, TableData};
use tiny_skia as sk;

pub const TILE: u32 = 512;

/// Device pixels per sheet point of the overview (sharp at 25 % on a 2x screen).
const OVERVIEW_SCALE: f32 = 0.5;

struct Tile {
    tex: egui::TextureHandle,
    version: u64,
}

pub struct Tiles {
    renderer: Renderer,
    /// Frozen header rows of each table, drawn once per (version, scale): texture and its size
    /// in sheet points.
    headers: HashMap<ObjId, (u64, f32, egui::TextureHandle, egui::Vec2)>,
    /// Frozen header columns for a block of rows: (version, scale, rows) -> texture, size.
    header_cols: HashMap<ObjId, (u64, f32, (u32, u32), egui::TextureHandle, egui::Vec2)>,
    /// Device pixels per sheet point the tiles were made for.
    scale: f32,
    tiles: HashMap<(i32, i32), Tile>,
    /// The tiles of earlier zooms, drawn stretched under the new ones until those are all
    /// there (no blank areas while pinching or after a zoom step): (scale, tiles), oldest
    /// first. Several are kept: zooming again before the new tiles are all in must not drop
    /// what only an older set covered (that flickered).
    old: Vec<(f32, HashMap<(i32, i32), Tile>)>,
    /// The whole sheet at a low resolution ([`OVERVIEW_SCALE`]), drawn under everything: no
    /// part of the sheet is ever blank while zooming, only soft until its sharp tiles come.
    /// Built and refreshed in idle moments (a stale one still stands in meanwhile).
    overview: HashMap<(i32, i32), Tile>,
    /// The sheet these tiles are of, and the other sheets' tiles (scale, tiles, overview),
    /// kept so switching back is instant (Numbers swaps sheets in one frame, Mac N21).
    sheet: Option<ObjId>,
    stash: HashMap<ObjId, (f32, HashMap<(i32, i32), Tile>, HashMap<(i32, i32), Tile>)>,
    /// The sheet's content (sheet points): outside it there's nothing to draw.
    content: egui::Rect,
    /// When the zoom last changed: while it keeps changing (a pinch) only the stretched
    /// tiles are drawn, so every frame stays fast; sharp tiles come once it settles.
    zoomed_at: Option<Instant>,
    /// An Excel workbook's cell extras of each table (rotated text, diagonal lines): Numbers
    /// can't hold them, Values keeps and draws them.
    pub extras: HashMap<ObjId, HashMap<(u32, u32), nm_core::xlstyle::Extra>>,
}

impl Default for Tiles {
    fn default() -> Self {
        Tiles { renderer: { let mut r = Renderer::new(); r.review_marks = true; r }, headers: HashMap::new(), header_cols: HashMap::new(), scale: 0.0, tiles: HashMap::new(), old: Vec::new(), overview: HashMap::new(), sheet: None, stash: HashMap::new(), content: egui::Rect::NOTHING, zoomed_at: None, extras: HashMap::new() }
    }
}

thread_local! {
    /// NM_PERF: tiles found with nothing to show under them (a blank patch on screen).
    pub static BLANKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// NM_PERF: tiles left out of a frame with nothing under them (any time, not only zooms).
    pub static MISSING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };

    /// The frames auto-sized text boxes really occupy (their stored size is 0 or stale: Numbers
    /// sizes them to their text), set by [`Tiles::resolve_frames`] whenever the sheet is re-read.
    static AUTO_FRAMES: std::cell::RefCell<HashMap<ObjId, Geometry>> = Default::default();
}

/// The frame a drawable occupies on the sheet (tables at their row/column sizes, text boxes
/// at their text's size).
pub fn frame(d: &Drawable) -> Geometry {
    match &d.kind {
        DrawableKind::Table(t) => t.frame(&d.geom),
        _ => AUTO_FRAMES.with(|m| m.borrow().get(&d.id).copied()).unwrap_or(d.geom),
    }
}

impl Tiles {
    /// For printing: Excel cell extras drawn as on the sheet, review marks not.
    pub fn for_print(extras: HashMap<ObjId, HashMap<(u32, u32), nm_core::xlstyle::Extra>>) -> Tiles {
        let mut t = Tiles::default();
        t.renderer.review_marks = false;
        t.extras = extras;
        t
    }

    /// Draw a drawable as the sheet does (a table with its Excel extras).
    pub fn draw_any(&mut self, doc: &Document, d: &Drawable, pm: &mut sk::Pixmap, base: sk::Transform) {
        if matches!(d.kind, DrawableKind::Table(_)) {
            self.draw_table(doc, d, (0, 0), pm, base);
        } else {
            self.renderer.draw_drawable(doc, d, pm, base);
        }
    }

    /// The text layout of a drawable's text (frame-local), as the renderer draws it.
    pub fn text_layout(&mut self, d: &Drawable) -> Option<(kn_render::text::TextLayout, nm_core::kn_model::Geometry)> {
        self.renderer.text_layout(d)
    }

    /// The frame a drawable really occupies (auto-sized text resolved).
    pub fn effective(&mut self, d: &Drawable) -> Geometry {
        self.renderer.effective_geometry(d)
    }

    /// Rows that fit their content grow to their tallest cell's wrapped text, as Numbers lays
    /// them out (kn-render's shared fitting; `view` maps shown rows to model rows for
    /// categorized tables).
    pub fn fit_rows(&mut self, t: &mut TableData, view: Option<&[Option<u32>]>) {
        kn_render::table::fit_rows(&mut self.renderer.fonts, t, view);
    }

    /// The zoom is still changing (a pinch or wheel zoom in progress).
    fn pinching(&self) -> bool {
        self.zoomed_at.is_some_and(|t| t.elapsed().as_millis() < 120)
    }

    /// Table > Resize Column to Fit Content: the width column `c` needs for its widest cell,
    /// unwrapped, with the cell's padding (None: nothing in it).
    pub fn fit_width(&mut self, t: &TableData, c: u32) -> Option<f32> {
        let mut need: Option<f32> = None;
        for r in 0..t.rows {
            // a merge across columns spreads over them: left out
            if t.merge_at(r, c).is_some_and(|m| m.cols > 1 || m.row != r || m.col != c) {
                continue;
            }
            let i = (r * t.cols + c) as usize;
            let text = t.cells.get(i).map(String::as_str).unwrap_or("");
            if text.trim().is_empty() {
                continue;
            }
            let Some(style) = t.cell_styles.get(i) else { continue };
            let mut tb = kn_render::simple_text_box(text, style.text.clone(), style.align, style.valign);
            tb.padding = style.padding;
            let tl = kn_render::text::layout(&mut self.renderer.fonts, &tb, 100_000.0, 0.0, true, true);
            let w = tl.content_w + style.padding[0] + style.padding[2] + 2.0;
            need = Some(need.map_or(w, |n: f32| n.max(w)));
        }
        need
    }

    /// Whether font `name` (as a document names it) is installed here.
    pub fn font_installed(&self, name: &str) -> bool {
        self.renderer.fonts.is_installed(name)
    }

    /// The PostScript name a document should use for family `family`.
    pub fn ps_name_for_family(&self, family: &str) -> Option<String> {
        self.renderer.fonts.ps_name_for_family(family)
    }

    /// Forget laid out text (after fonts changed).
    pub fn clear_text_caches(&mut self) {
        self.renderer.clear_caches();
    }

    /// Installed font families.
    pub fn font_families(&self) -> Vec<String> {
        self.renderer.fonts.families().to_vec()
    }

    /// Work out the frames of the sheet's auto-sized text boxes (see [`frame`]).
    pub fn resolve_frames(&mut self, drawables: &[Drawable]) {
        let mut m = HashMap::new();
        for d in drawables {
            if let DrawableKind::Shape(s) = &d.kind
                && s.text.is_some()
            {
                let g = self.renderer.effective_geometry(d);
                if g != d.geom {
                    m.insert(d.id, g);
                }
            }
        }
        AUTO_FRAMES.with(|a| *a.borrow_mut() = m);
    }

    /// Show sheet `id`: the current sheet's tiles are put by, and `id`'s brought back if it was
    /// shown before (stale ones are redrawn in place, not blanked).
    pub fn show_sheet(&mut self, id: ObjId) {
        if self.sheet == Some(id) {
            return;
        }
        if let Some(cur) = self.sheet.replace(id) {
            let put = (self.scale, std::mem::take(&mut self.tiles), std::mem::take(&mut self.overview));
            self.stash.insert(cur, put);
        }
        self.old.clear();
        self.zoomed_at = None;
        match self.stash.remove(&id) {
            Some((scale, tiles, overview)) => {
                self.scale = scale;
                self.tiles = tiles;
                self.overview = overview;
            }
            None => {
                self.tiles.clear();
                self.overview.clear();
            }
        }
    }

    /// Forget everything drawn (another document).
    pub fn clear(&mut self) {
        self.tiles.clear();
        self.old.clear();
        self.overview.clear();
        self.stash.clear();
        self.sheet = None;
        self.headers.clear();
        self.header_cols.clear();
        self.renderer.clear_caches();
    }

    /// Draw the sheet: `origin` is the sheet point at the top-left of `rect`, `zoom` screen
    /// points per sheet point. Returns true while tiles are still missing or stale.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        origin: egui::Pos2,
        zoom: f32,
        doc: &Document,
        drawables: &[Drawable],
        version: u64,
    ) -> bool {
        let ppp = ui.ctx().pixels_per_point();
        let scale = zoom * ppp;
        if (scale - self.scale).abs() > 1e-4 {
            // keep the last zoom's tiles to stretch meanwhile (the fuller set, mid-pinch)
            let prev = std::mem::take(&mut self.tiles);
            if !prev.is_empty() {
                self.old.push((self.scale, prev));
                // at most 4 sets: the one covering least goes
                if self.old.len() > 4
                    && let Some(i) = (0..self.old.len() - 1).min_by_key(|i| self.old[*i].1.len())
                {
                    self.old.remove(i);
                }
            }
            self.scale = scale;
            self.zoomed_at = Some(Instant::now());
        }
        self.content = drawables.iter().map(|d| {
            let g = frame(d);
            egui::Rect::from_min_size(egui::pos2(g.x, g.y), egui::vec2(g.w, g.h))
        }).fold(egui::Rect::NOTHING, |a, b| a.union(b));
        // mid-pinch: no rendering, the stretched tiles stand in (sharp ones ~120 ms later)
        let pinching = !self.old.is_empty() && self.zoomed_at.is_some_and(|t| t.elapsed().as_millis() < 120);
        // tile edges on whole device pixels: no seams between neighbouring tiles
        let snap = |v: f32| (v * ppp).round() / ppp;
        let span = TILE as f32 / scale; // sheet points per tile
        let x0 = (origin.x / span).floor() as i32;
        let y0 = (origin.y / span).floor() as i32;
        let x1 = ((origin.x + rect.width() / zoom) / span).floor() as i32;
        let y1 = ((origin.y + rect.height() / zoom) / span).floor() as i32;
        let start = Instant::now();
        let first = self.old.is_empty() && self.overview.is_empty() && self.tiles.is_empty();
        let mut pending = false;
        let painter = ui.painter_at(rect);
        // the overview, then the earlier zooms' tiles, stretched, under everything
        let ov = std::iter::once((OVERVIEW_SCALE, &self.overview));
        for (os, old) in ov.chain(self.old.iter().map(|(s, t)| (*s, t))) {
            let ospan = TILE as f32 / os;
            for ((tx, ty), t) in old.iter() {
                let (ax, ay) = (*tx as f32 * ospan, *ty as f32 * ospan);
                let r = egui::Rect::from_min_max(
                    egui::pos2(snap(rect.min.x + (ax - origin.x) * zoom), snap(rect.min.y + (ay - origin.y) * zoom)),
                    egui::pos2(snap(rect.min.x + (ax + ospan - origin.x) * zoom), snap(rect.min.y + (ay + ospan - origin.y) * zoom)),
                );
                if r.intersects(rect) {
                    painter.image(t.tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                }
            }
        }
        // nearest tiles first, so the middle of the view fills in first
        let mut want: Vec<(i32, i32)> = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).collect();
        let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
        want.sort_by(|a, b| {
            let da = (a.0 as f32 - cx).abs() + (a.1 as f32 - cy).abs();
            let db = (b.0 as f32 - cx).abs() + (b.1 as f32 - cy).abs();
            da.total_cmp(&db)
        });
        for (tx, ty) in want {
            let fresh = self.tiles.get(&(tx, ty)).is_some_and(|t| t.version == version);
            // a tile with nothing to show yet may take up to 30 ms (no blank areas after a zoom);
            // stale ones are replaced within ~4 ms a frame, so frames stay under 8 ms (120 Hz)
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            // a tile with nothing at all under it may take longer, the rest 4 ms a frame
            // nothing under it at all (zooming out shows new ground): drawn even mid-pinch,
            // within a small budget, rather than left blank
            let bare = !self.tiles.contains_key(&(tx, ty)) && !self.covered(tx as f32 * span, ty as f32 * span, span);
            // a sheet shown for the first time comes in whole, in its first frame (no popping in)
            let budget_ok = if pinching { bare && ms < 6.0 } else { ms < 4.0 || bare && (ms < 30.0 || first) };
            // (counted during a zoom only: the first tiles of a sheet come in as they're drawn)
            if bare && !budget_ok && std::env::var_os("NM_PERF").is_some() {
                // (during a zoom, and at any time: a sheet coming in piece by piece)
                if !self.old.is_empty() {
                    BLANKS.with(|b| b.set(b.get() + 1));
                }
                MISSING.with(|b| b.set(b.get() + 1));
            }
            if !fresh {
                if budget_ok {
                    let tt = Instant::now();
                    let img = self.render_tile(doc, drawables, tx, ty, span, scale);
                    if std::env::var_os("NM_PERF").is_some() {
                        eprintln!("tile {tx},{ty}: {:.1} ms", tt.elapsed().as_secs_f32() * 1000.0);
                    }
                    let tex = ui.ctx().load_texture(format!("tile{tx},{ty}"), img, egui::TextureOptions::LINEAR);
                    self.tiles.insert((tx, ty), Tile { tex, version });
                } else {
                    pending = true;
                }
            }
            if let Some(t) = self.tiles.get(&(tx, ty)) {
                let r = egui::Rect::from_min_max(
                    egui::pos2(snap(rect.min.x + (tx as f32 * span - origin.x) * zoom), snap(rect.min.y + (ty as f32 * span - origin.y) * zoom)),
                    egui::pos2(snap(rect.min.x + ((tx + 1) as f32 * span - origin.x) * zoom), snap(rect.min.y + ((ty + 1) as f32 * span - origin.y) * zoom)),
                );
                painter.image(t.tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
        }
        // spare time: the ring just outside the view, so scrolling finds its tiles ready
        if !pending {
            for (tx, ty) in (y0 - 1..=y1 + 1).flat_map(|y| (x0 - 1..=x1 + 1).map(move |x| (x, y))) {
                if start.elapsed().as_secs_f32() * 1000.0 > 4.0 {
                    pending = true;
                    break;
                }
                if (x0..=x1).contains(&tx) && (y0..=y1).contains(&ty) || self.tiles.get(&(tx, ty)).is_some_and(|t| t.version == version) {
                    continue;
                }
                let img = self.render_tile(doc, drawables, tx, ty, span, scale);
                let tex = ui.ctx().load_texture(format!("tile{tx},{ty}"), img, egui::TextureOptions::LINEAR);
                self.tiles.insert((tx, ty), Tile { tex, version });
            }
        }
        // forget far-away tiles
        if self.tiles.len() > 400 {
            self.tiles.retain(|(x, y), _| *x >= x0 - 4 && *x <= x1 + 4 && *y >= y0 - 4 && *y <= y1 + 4);
        }
        // every tile in view is sharp again: the stretched ones can go
        if !pending {
            self.old.clear();
        }
        // idle (no zoom for a while, the view's tiles all in): one overview tile a frame
        // (NM_NO_OVERVIEW: checks without it)
        let idle = std::env::var_os("NM_NO_OVERVIEW").is_none() && !pending && self.zoomed_at.is_none_or(|t| t.elapsed().as_millis() > 300) && start.elapsed().as_secs_f32() * 1000.0 < 2.0;
        if idle {
            let ospan = TILE as f32 / OVERVIEW_SCALE;
            let (lo, hi) = (self.content.min, self.content.max);
            if lo.x <= hi.x {
                let (ox0, oy0, ox1, oy1) = ((lo.x / ospan).floor() as i32, (lo.y / ospan).floor() as i32, (hi.x / ospan).floor() as i32, (hi.y / ospan).floor() as i32);
                // (a huge sheet keeps to 64 overview tiles)
                let all: Vec<(i32, i32)> = (oy0..=oy1).flat_map(|y| (ox0..=ox1).map(move |x| (x, y))).take(64).collect();
                let next = all.iter().find(|k| !self.overview.contains_key(k)).or_else(|| all.iter().find(|k| self.overview.get(k).is_some_and(|t| t.version != version)));
                if let Some(&(tx, ty)) = next {
                    let img = self.render_tile(doc, drawables, tx, ty, ospan, OVERVIEW_SCALE);
                    let tex = ui.ctx().load_texture(format!("overview{tx},{ty}"), img, egui::TextureOptions::LINEAR);
                    self.overview.insert((tx, ty), Tile { tex, version });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
                }
                self.overview.retain(|k, _| all.contains(k));
            }
        }
        pending
    }

    /// Paint what the current tiles show of sheet rect `sheet` into screen rect `dest` (the
    /// ghost of cells being dragged).
    pub fn paint_region(&self, painter: &egui::Painter, sheet: egui::Rect, dest: egui::Rect, tint: egui::Color32) {
        if self.scale <= 0.0 || sheet.width() <= 0.0 || sheet.height() <= 0.0 {
            return;
        }
        let span = TILE as f32 / self.scale;
        let k = egui::vec2(dest.width() / sheet.width(), dest.height() / sheet.height());
        for ty in (sheet.min.y / span).floor() as i32..=(sheet.max.y / span).floor() as i32 {
            for tx in (sheet.min.x / span).floor() as i32..=(sheet.max.x / span).floor() as i32 {
                let Some(t) = self.tiles.get(&(tx, ty)) else { continue };
                let ts = egui::Rect::from_min_size(egui::pos2(tx as f32 * span, ty as f32 * span), egui::vec2(span, span));
                let part = ts.intersect(sheet);
                if part.width() <= 0.0 || part.height() <= 0.0 {
                    continue;
                }
                let uv = egui::Rect::from_min_max(((part.min - ts.min) / span).to_pos2(), ((part.max - ts.min) / span).to_pos2());
                let to = egui::Rect::from_min_max(dest.min + (part.min - sheet.min) * k, dest.min + (part.max - sheet.min) * k);
                painter.image(t.tex.id(), to, uv, tint);
            }
        }
    }

    /// Whether the stretched earlier tiles cover the sheet square at (x, y), `w` points wide
    /// (its corners and middle).
    fn covered(&self, x: f32, y: f32, w: f32) -> bool {
        let e = w * 0.02;
        [(x + e, y + e), (x + w - e, y + e), (x + e, y + w - e), (x + w - e, y + w - e), (x + w / 2.0, y + w / 2.0)].iter().all(|(px, py)| {
            // (past the content there is nothing to show: white either way)
            if !self.content.contains(egui::pos2(*px, *py)) {
                return true;
            }
            std::iter::once((OVERVIEW_SCALE, &self.overview)).chain(self.old.iter().map(|(s, t)| (*s, t))).any(|(os, t)| {
                let ospan = TILE as f32 / os;
                t.contains_key(&((px / ospan).floor() as i32, (py / ospan).floor() as i32))
            })
        })
    }

    /// The header rows of table drawable `d` as a texture (for keeping them in view while the
    /// table scrolls under them), with its size in sheet points.
    pub fn header_strip(&mut self, ui: &egui::Ui, doc: &Document, d: &Drawable, zoom: f32, version: u64) -> Option<(egui::TextureId, egui::Vec2)> {
        let DrawableKind::Table(t) = &d.kind else { return None };
        if t.header_rows == 0 {
            return None;
        }
        let scale = zoom * ui.ctx().pixels_per_point();
        if let Some((v, sc, tex, size)) = self.headers.get(&d.id) {
            // mid-pinch the last one is stretched (sizes are in sheet points)
            if *v == version && ((*sc - scale).abs() < 1e-4 || self.pinching()) {
                return Some((tex.id(), *size));
            }
        }
        let g = t.frame(&d.geom);
        let h: f32 = t.row_heights.iter().take(t.header_rows as usize).sum();
        let mut strip = d.clone();
        let mut c = t.clone();
        c.rows = t.header_rows;
        c.row_heights.truncate(t.header_rows as usize);
        c.cells.truncate((t.header_rows * t.cols) as usize);
        c.cell_styles.truncate(c.cell_styles.len().min((t.header_rows * t.cols) as usize));
        c.h_strokes.truncate(((t.header_rows + 1) * t.cols) as usize);
        c.v_strokes.truncate((t.header_rows * (t.cols + 1)) as usize);
        c.merges.retain(|m| m.row + m.rows <= t.header_rows);
        c.footer_rows = 0;
        strip.geom = Geometry { x: 0.0, y: 0.0, w: g.w, h, ..g };
        strip.kind = DrawableKind::Table(c);
        let (wpx, hpx) = (((g.w + 2.0) * scale).ceil().min(8192.0) as u32, ((h + 2.0) * scale).ceil().min(4096.0) as u32);
        let mut pm = sk::Pixmap::new(wpx.max(1), hpx.max(1))?;
        pm.fill(sk::Color::WHITE);
        self.draw_table(doc, &strip, (0, 0), &mut pm, sk::Transform::from_scale(scale, scale).pre_translate(1.0, 1.0));
        let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
        let tex = ui.ctx().load_texture(format!("header{}", d.id), img, egui::TextureOptions::LINEAR);
        let size = egui::vec2(pm.width() as f32 / scale, pm.height() as f32 / scale);
        let id = tex.id();
        self.headers.insert(d.id, (version, scale, tex, size));
        Some((id, size))
    }

    /// The header columns of table `d` over rows r0..=r1, as a texture with its size in sheet
    /// points (kept in view while the table scrolls sideways under them).
    #[allow(clippy::too_many_arguments)]
    pub fn header_col_strip(&mut self, ui: &egui::Ui, doc: &Document, d: &Drawable, zoom: f32, version: u64, r0: u32, r1: u32) -> Option<(egui::TextureId, egui::Vec2)> {
        let DrawableKind::Table(t) = &d.kind else { return None };
        if t.header_cols == 0 || r0 > r1 || r1 >= t.rows {
            return None;
        }
        let scale = zoom * ui.ctx().pixels_per_point();
        if let Some((v, sc, rows, tex, size)) = self.header_cols.get(&d.id) {
            if *v == version && ((*sc - scale).abs() < 1e-4 || self.pinching()) && *rows == (r0, r1) {
                return Some((tex.id(), *size));
            }
        }
        let hc = t.header_cols as usize;
        let cols = t.cols as usize;
        let w: f32 = t.col_widths.iter().take(hc).sum();
        let h: f32 = t.row_heights[r0 as usize..=r1 as usize].iter().sum();
        let mut c = t.clone();
        c.rows = r1 - r0 + 1;
        c.cols = hc as u32;
        c.header_rows = t.header_rows.saturating_sub(r0).min(c.rows);
        c.footer_rows = 0;
        c.row_heights = t.row_heights[r0 as usize..=r1 as usize].to_vec();
        c.col_widths.truncate(hc);
        c.cells = (r0..=r1).flat_map(|r| t.cells[r as usize * cols..r as usize * cols + hc].to_vec()).collect();
        c.cell_styles = if t.cell_styles.is_empty() { Vec::new() } else { (r0..=r1).flat_map(|r| t.cell_styles[r as usize * cols..r as usize * cols + hc].to_vec()).collect() };
        c.h_strokes = (r0..=r1 + 1).flat_map(|b| (0..hc).map(move |col| (b, col))).map(|(b, col)| t.h_strokes.get(b as usize * cols + col).cloned().flatten()).collect();
        c.v_strokes = (r0..=r1).flat_map(|r| (0..=hc).map(move |b| (r, b))).map(|(r, b)| t.v_strokes.get(r as usize * (cols + 1) + b).cloned().flatten()).collect();
        c.merges.retain(|m| m.col + m.cols <= hc as u32 && m.row >= r0 && m.row + m.rows <= r1 + 1);
        for m in &mut c.merges {
            m.row -= r0;
        }
        let mut strip = d.clone();
        strip.geom = Geometry { x: 0.0, y: 0.0, w, h, ..t.frame(&d.geom) };
        strip.kind = DrawableKind::Table(c);
        let (wpx, hpx) = (((w + 2.0) * scale).ceil().min(4096.0) as u32, ((h + 2.0) * scale).ceil().min(8192.0) as u32);
        let mut pm = sk::Pixmap::new(wpx.max(1), hpx.max(1))?;
        pm.fill(sk::Color::WHITE);
        self.draw_table(doc, &strip, (r0, 0), &mut pm, sk::Transform::from_scale(scale, scale).pre_translate(1.0, 1.0));
        let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
        let tex = ui.ctx().load_texture(format!("hcols{}", d.id), img, egui::TextureOptions::LINEAR);
        let size = egui::vec2(pm.width() as f32 / scale, pm.height() as f32 / scale);
        let id = tex.id();
        self.header_cols.insert(d.id, (version, scale, (r0, r1), tex, size));
        Some((id, size))
    }

    /// Draw a table (or a part of one: `off` is its first row and column in the whole table),
    /// with its rotated cells' text turned (Excel's textRotation: 45-90 reads upwards,
    /// 135-180 downwards, 255 letters stacked; small tilts are drawn level).
    fn draw_table(&mut self, doc: &Document, d: &Drawable, off: (u32, u32), pm: &mut sk::Pixmap, base: sk::Transform) {
        let DrawableKind::Table(t) = &d.kind else { return };
        let Some(ext) = self.extras.get(&d.id).filter(|r| !r.is_empty()) else {
            self.renderer.draw_drawable(doc, d, pm, base);
            return;
        };
        let mut c = t.clone();
        let mut turned: Vec<(u32, u32, i32, String)> = Vec::new();
        let mut diags: Vec<(u32, u32, nm_core::xlstyle::Diag)> = Vec::new();
        for (&(r, col), e) in ext {
            let (Some(lr), Some(lc)) = (r.checked_sub(off.0), col.checked_sub(off.1)) else { continue };
            if lr >= c.rows || lc >= c.cols {
                continue;
            }
            if let Some(dg) = &e.diag {
                diags.push((lr, lc, dg.clone()));
            }
            let a = e.rotation;
            if !(45..=90).contains(&a) && !(135..=180).contains(&a) && a != 255 {
                continue;
            }
            let i = (lr * c.cols + lc) as usize;
            let text = std::mem::take(&mut c.cells[i]);
            if !text.is_empty() {
                turned.push((lr, lc, a, text));
            }
        }
        let mut plain = d.clone();
        plain.kind = DrawableKind::Table(c.clone());
        self.renderer.draw_drawable(doc, &plain, pm, base);
        let g = c.frame(&d.geom);
        let edge = |sizes: &[f32], n: u32| -> f32 { sizes.iter().take(n as usize).sum() };
        // diagonal lines across their cells (whole merged ranges)
        for (r, col, dg) in diags {
            let (r1, c1) = match c.merge_at(r, col) {
                Some(m) if (m.row, m.col) != (r, col) => continue,
                Some(m) => (m.row + m.rows, m.col + m.cols),
                None => (r + 1, col + 1),
            };
            let (x0, y0) = (g.x + edge(&c.col_widths, col), g.y + edge(&c.row_heights, r));
            let (x1, y1) = (g.x + edge(&c.col_widths, c1), g.y + edge(&c.row_heights, r1));
            let col32 = dg.edge.color;
            let mut paint = sk::Paint::default();
            paint.set_color(sk::Color::from_rgba(col32.r, col32.g, col32.b, col32.a).unwrap_or(sk::Color::BLACK));
            paint.anti_alias = true;
            let stroke = sk::Stroke { width: dg.edge.width, ..Default::default() };
            for (on, a, b) in [(dg.up, (x0, y1), (x1, y0)), (dg.down, (x0, y0), (x1, y1))] {
                if !on {
                    continue;
                }
                let mut pb = sk::PathBuilder::new();
                pb.move_to(a.0, a.1);
                pb.line_to(b.0, b.1);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint, &stroke, base, None);
                }
            }
        }
        for (r, col, a, text) in turned {
            let (r1, c1) = match c.merge_at(r, col) {
                Some(m) => (m.row + m.rows, m.col + m.cols),
                None => (r + 1, col + 1),
            };
            let (x, y) = (g.x + edge(&c.col_widths, col), g.y + edge(&c.row_heights, r));
            let (w, h) = (edge(&c.col_widths, c1) - edge(&c.col_widths, col), edge(&c.row_heights, r1) - edge(&c.row_heights, r));
            let style = c.cell_styles.get((r * c.cols + col) as usize).cloned().unwrap_or_default();
            let (text, bw, bh, tr) = match a {
                255 => (text.chars().filter(|ch| *ch != '\n').map(String::from).collect::<Vec<_>>().join("\n"), w, h, base.pre_translate(x, y)),
                45..=90 => (text, h, w, base.pre_translate(x, y + h).pre_rotate(-90.0)),
                _ => (text, h, w, base.pre_translate(x + w, y).pre_rotate(90.0)),
            };
            let mut tb = kn_render::simple_text_box(&text, style.text.clone(), nm_core::kn_model::Align::Center, nm_core::kn_model::VAlign::Middle);
            tb.padding = style.padding;
            let tl = kn_render::text::layout(&mut self.renderer.fonts, &tb, bw, bh, false, false);
            self.renderer.draw_text(&tl, pm, tr);
        }
    }

    fn render_tile(&mut self, doc: &Document, drawables: &[Drawable], tx: i32, ty: i32, span: f32, scale: f32) -> egui::ColorImage {
        let dbg = std::env::var_os("NM_DEBUG").is_some();
        if dbg {
            eprintln!("render tile {tx},{ty} span {span} scale {scale}");
        }
        let mut pm = sk::Pixmap::new(TILE, TILE).expect("tile pixmap");
        pm.fill(sk::Color::WHITE);
        let (ox, oy) = (tx as f32 * span, ty as f32 * span);
        let base = sk::Transform::from_row(scale, 0.0, 0.0, scale, -ox * scale, -oy * scale);
        for d in drawables {
            let g = frame(d);
            // generous margin for strokes, shadows and text that spills out of its box
            let m = 40.0;
            if g.x - m > ox + span || g.y - m > oy + span || g.x + g.w + m < ox || g.y + g.h + m < oy {
                if g.angle == 0.0 {
                    continue;
                }
            }
            match &d.kind {
                // big tables: only the rows and columns this tile shows
                DrawableKind::Table(t) if g.angle == 0.0 && t.rows * t.cols > 64 => {
                    if let Some((c, off)) = crop_table(d, t, ox, oy, span) {
                        self.draw_table(doc, &c, off, &mut pm, base);
                    }
                }
                DrawableKind::Table(_) => self.draw_table(doc, d, (0, 0), &mut pm, base),
                _ => self.renderer.draw_drawable(doc, d, &mut pm, base),
            }
        }
        if dbg {
            eprintln!("rendered tile {tx},{ty}");
        }
        egui::ColorImage::from_rgba_premultiplied([TILE as usize, TILE as usize], pm.data())
    }
}

/// The part of a table drawable inside the sheet square (ox, oy, span): its rows and columns
/// there plus a margin (text spilling over, strokes at the edges), widened to cover every
/// merged range it cuts through.
fn crop_table(d: &Drawable, t: &TableData, ox: f32, oy: f32, span: f32) -> Option<(Drawable, (u32, u32))> {
    let g = t.frame(&d.geom);
    let edges = |sizes: &[f32], n: u32| {
        let mut v = vec![0.0f32];
        for i in 0..n as usize {
            v.push(v[i] + sizes.get(i).copied().unwrap_or(0.0));
        }
        v
    };
    let xs = edges(&t.col_widths, t.cols);
    let ys = edges(&t.row_heights, t.rows);
    let first = |es: &[f32], v: f32| es.partition_point(|&e| e <= v).saturating_sub(1) as u32;
    let (x0, x1) = (ox - g.x, ox + span - g.x);
    let (y0, y1) = (oy - g.y, oy + span - g.y);
    if x1 < 0.0 || y1 < 0.0 || x0 > *xs.last()? || y0 > *ys.last()? {
        return None;
    }
    let mut c0 = first(&xs, x0.max(0.0)).saturating_sub(2);
    let mut c1 = (first(&xs, x1.max(0.0)) + 2).min(t.cols.saturating_sub(1));
    let mut r0 = first(&ys, y0.max(0.0)).saturating_sub(1);
    let mut r1 = (first(&ys, y1.max(0.0)) + 1).min(t.rows.saturating_sub(1));
    // a merge crossing the crop's edge brings in its whole range
    loop {
        let mut grew = false;
        for m in &t.merges {
            let (mr1, mc1) = (m.row + m.rows - 1, m.col + m.cols - 1);
            if m.row <= r1 && mr1 >= r0 && m.col <= c1 && mc1 >= c0 && (m.row < r0 || mr1 > r1 || m.col < c0 || mc1 > c1) {
                r0 = r0.min(m.row);
                c0 = c0.min(m.col);
                r1 = r1.max(mr1).min(t.rows - 1);
                c1 = c1.max(mc1).min(t.cols - 1);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    if (r0, c0, r1, c1) == (0, 0, t.rows - 1, t.cols - 1) {
        return Some((d.clone(), (0, 0)));
    }
    let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
    let mut c = TableData {
        rows,
        cols,
        header_rows: t.header_rows.saturating_sub(r0).min(rows),
        header_cols: t.header_cols.saturating_sub(c0).min(cols),
        footer_rows: 0,
        row_heights: t.row_heights[r0 as usize..=r1 as usize].to_vec(),
        col_widths: t.col_widths[c0 as usize..=c1 as usize].to_vec(),
        name: t.name.clone(),
        ..Default::default()
    };
    for r in r0..=r1 {
        let a = (r * t.cols + c0) as usize;
        let b = (r * t.cols + c1) as usize;
        c.cells.extend_from_slice(&t.cells[a..=b]);
        if !t.cell_styles.is_empty() {
            c.cell_styles.extend_from_slice(&t.cell_styles[a..=b]);
        }
    }
    c.merges = t
        .merges
        .iter()
        .filter(|m| m.row >= r0 && m.col >= c0 && m.row + m.rows - 1 <= r1 && m.col + m.cols - 1 <= c1)
        .map(|m| CellRange { row: m.row - r0, col: m.col - c0, ..*m })
        .collect();
    for b in r0..=r1 + 1 {
        for col in c0..=c1 {
            c.h_strokes.push(t.h_strokes.get((b * t.cols + col) as usize).cloned().flatten());
        }
    }
    for row in r0..=r1 {
        for b in c0..=c1 + 1 {
            c.v_strokes.push(t.v_strokes.get((row * (t.cols + 1) + b) as usize).cloned().flatten());
        }
    }
    let mut out = d.clone();
    out.geom = Geometry { x: g.x + xs[c0 as usize], y: g.y + ys[r0 as usize], w: xs[c1 as usize + 1] - xs[c0 as usize], h: ys[r1 as usize + 1] - ys[r0 as usize], ..g };
    out.kind = DrawableKind::Table(c);
    Some((out, (r0, c0)))
}
