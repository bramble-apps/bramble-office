//! The slide canvas: shows the current slide, selection handles, and handles object editing
//! (move, resize, rotate with Ctrl-drag on a handle, rubber-band select, text editing).

use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind};
use kn_format::ObjId;
use kn_model::{DrawableKind, Geometry, Point, SlideScene};

use crate::app::{App, Zoom};
use crate::textures::CanvasKey;
use crate::theme;

#[derive(Clone, Debug)]
enum Drag {
    /// Moving the selection; `delta` in slide points.
    Move { start: Point, delta: (f32, f32) },
    /// Resizing one object by handle `handle` (0..8, clockwise from top-left).
    Resize { id: ObjId, handle: usize, orig: Geometry, cur: Geometry },
    Rotate { id: ObjId, orig: Geometry, start_angle: f32, cur: Geometry },
    Rubber { start: Pos2, cur: Pos2 },
    /// Dragging a shape's green control handle (arrow head, star radius, corner radius); `q`
    /// in the shape's frame.
    Control { id: ObjId, g: Geometry, q: Point },
    /// Dragging one end of a straight line (Keynote's line handles); `a`, `b` in slide points.
    LineEnd { id: ObjId, end: usize, a: Point, b: Point },
    /// A user guide being dragged out of a ruler (`index` None) or moved; `pos` in slide points.
    Guide { index: Option<usize>, vertical: bool, pos: f32 },
}

/// Point editing (Format > Shapes and Lines > Make Editable): the shape, its nodes as being
/// edited, the selected node and what is being dragged (node, in or out handle).
pub struct PointEdit {
    pub id: ObjId,
    pub nodes: kn_model::points::Editable,
    pub selected: Option<(usize, usize)>,
    drag: Option<(usize, usize, u8)>,
}

impl PointEdit {
    pub fn new(id: ObjId, nodes: kn_model::points::Editable) -> PointEdit {
        PointEdit { id, nodes, selected: None, drag: None }
    }
}

/// A plain two-point line's end points in slide coordinates (Keynote shows a handle on each end
/// instead of a selection box).
fn line_ends(d: &kn_model::Drawable) -> Option<(Point, Point)> {
    let DrawableKind::Shape(s) = &d.kind else { return None };
    if !s.is_line || s.path.els.len() != 2 {
        return None;
    }
    let pt = |e: &kn_model::PathEl| match e {
        kn_model::PathEl::Move(p) | kn_model::PathEl::Line(p) => Some(*p),
        _ => None,
    };
    let (a, b) = (pt(&s.path.els[0])?, pt(&s.path.els[1])?);
    let g = d.geom;
    Some((g.to_slide(a), g.to_slide(b)))
}

#[derive(Default)]
pub struct CanvasState {
    drag: Option<Drag>,
    /// Text being edited in place.
    pub editing: Option<crate::textedit::Editing>,
    /// Table cell being edited: (table, row, col, text, first frame).
    pub cell: Option<(ObjId, u32, u32, String, bool)>,
    /// Lines of the cell being edited as last drawn (its row grows as lines are typed).
    pub cell_lines: usize,
    /// The table cell edited last: (table, row, col) — Format > Table's rows and columns act
    /// on it while its table stays selected.
    pub table_cursor: Option<(ObjId, u32, u32)>,
    /// Alignment guides of the current move/resize.
    guides: Vec<crate::snap::Guide>,
    /// The drag that just ended had Alt held (duplicate instead of move).
    alt_drop: bool,
    /// Selected cell range (Format > Table > Select / Extend…).
    pub table_range: Option<crate::tableops::Range>,
    /// A table border being dragged: (table, rows?, index, size before, pointer start).
    pub border_drag: Option<(ObjId, bool, u32, f32, Pos2)>,
    /// A table tab's menu that is open: (table, rows?, index, where).
    pub tab_menu: Option<(ObjId, bool, u32, Pos2)>,
    /// The add-rows / add-columns stepper that is open: (table, rows?, where).
    pub add_stepper: Option<(ObjId, bool, Pos2)>,
    /// A cell / range being picked into a formula: (first row, first column, where it goes in
    /// the text).
    pub formula_pick: Option<(u32, u32, usize)>,
    /// Put the formula editor's caret at the end (after a pick).
    pub formula_caret_end: bool,
    /// The autofill dot being dragged: (table, down? else right).
    pub fill_drag: Option<(ObjId, bool)>,
    /// Where a run of Tab presses started: (table, row, column).
    pub tab_start: Option<(ObjId, u32, u32)>,
    /// A drag that began on a row / column tab: (table, rows?, first, last, moving the
    /// selected rows? else selecting from `first` to the tab under the pointer).
    pub tab_drag: Option<(ObjId, bool, u32, u32, bool)>,
    /// Cells being selected by dragging across a selected table: (table, first row, first column).
    pub cell_drag: Option<(ObjId, u32, u32)>,
    /// A web video playing inline while editing (Keynote: click its ▶), and one whose stream
    /// is still being looked up.
    pub web_player: Option<crate::media::Player>,
    pub web_waiting: Option<ObjId>,
    /// The picture shown whole while its mask is edited: (data id, texture).
    pub mask_tex: Option<(u64, egui::TextureHandle)>,
    /// The frame a tab menu / the add stepper opened on (that click isn't "elsewhere").
    pub popup_opened: u64,
}

impl CanvasState {
    pub fn cancel(&mut self) {
        self.drag = None;
        self.editing = None;
    }
    pub fn is_editing_text(&self) -> bool {
        self.editing.is_some()
    }
}

struct View {
    rect: Rect,
    scale: f32,
}

impl View {
    fn to_screen(&self, p: Point) -> Pos2 {
        pos2(self.rect.min.x + p.x * self.scale, self.rect.min.y + p.y * self.scale)
    }
    fn to_slide(&self, p: Pos2) -> Point {
        Point::new((p.x - self.rect.min.x) / self.scale, (p.y - self.rect.min.y) / self.scale)
    }
}

/// Corners + edge midpoints of a frame (local coords), clockwise from top-left.
fn handle_points(g: &Geometry) -> [Point; 8] {
    let (w, h) = (g.w, g.h);
    [
        Point::new(0.0, 0.0),
        Point::new(w / 2.0, 0.0),
        Point::new(w, 0.0),
        Point::new(w, h / 2.0),
        Point::new(w, h),
        Point::new(w / 2.0, h),
        Point::new(0.0, h),
        Point::new(0.0, h / 2.0),
    ]
}

fn resized(orig: &Geometry, handle: usize, local: Point, keep_aspect: bool) -> Geometry {
    // which edges each handle moves: (left, top, right, bottom)
    let (l, t, r, b) = match handle {
        0 => (true, true, false, false),
        1 => (false, true, false, false),
        2 => (false, true, true, false),
        3 => (false, false, true, false),
        4 => (false, false, true, true),
        5 => (false, false, false, true),
        6 => (true, false, false, true),
        _ => (true, false, false, false),
    };
    let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, orig.w, orig.h);
    if l {
        x0 = local.x.min(x1 - 1.0);
    }
    if r {
        x1 = local.x.max(x0 + 1.0);
    }
    if t {
        y0 = local.y.min(y1 - 1.0);
    }
    if b {
        y1 = local.y.max(y0 + 1.0);
    }
    if keep_aspect && orig.w > 0.0 && orig.h > 0.0 {
        let s = ((x1 - x0) / orig.w).max((y1 - y0) / orig.h);
        let (nw, nh) = (orig.w * s, orig.h * s);
        if l {
            x0 = x1 - nw
        } else {
            x1 = x0 + nw
        }
        if t {
            y0 = y1 - nh
        } else {
            y1 = y0 + nh
        }
    }
    // the new frame's centre in slide space, so rotation stays about the right point
    let c_local = Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let c = orig.to_slide(c_local);
    let (w, h) = (x1 - x0, y1 - y0);
    Geometry { x: c.x - w / 2.0, y: c.y - h / 2.0, w, h, auto_width: false, auto_height: false, ..*orig }
}

/// The picture whole (not masked), for Edit Mask, cached by data id.
fn mask_picture(app: &mut App, ctx: &egui::Context, data: u64) -> Option<egui::TextureHandle> {
    if let Some((k, t)) = &app.canvas.mask_tex {
        if *k == data {
            return Some(t.clone());
        }
    }
    let bytes = app.doc.as_ref()?.data(data)?;
    let img = image::load_from_memory(&bytes).ok()?.thumbnail(1600, 1600).to_rgba8();
    let tex = ctx.load_texture(format!("mask-picture-{data}"), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR);
    app.canvas.mask_tex = Some((data, tex.clone()));
    Some(tex)
}

/// Edit Mask's bar under the mask: the picture's size (it grows and shrinks about the mask's
/// centre, the mask staying put) and Done.
fn mask_bar(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    let Some(id) = app.mask_edit else { return };
    let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { return };
    let Some(m) = mask_box(d) else { return };
    let g = d.geom;
    let at = view.to_screen(Point::new(m.x + m.w / 2.0, m.y + m.h)) + vec2(0.0, 16.0);
    let mut pct = (g.w / m.w.max(0.01) * 100.0).clamp(100.0, 500.0);
    let mut done = false;
    let mut release = false;
    egui::Area::new(ui.scope_id().with(("mask-bar", id))).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_TOP).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).corner_radius(10).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().slider_width = 160.0;
                let r = ui.add(egui::Slider::new(&mut pct, 100.0..=500.0).show_value(false)).on_hover_text("Picture size");
                release = r.drag_stopped() || (r.changed() && !r.dragged());
                if ui.add(egui::Button::new("Done").fill(theme::ACCENT())).clicked() {
                    done = true;
                }
            });
        });
    });
    if release {
        // the picture scales about the mask's centre; the mask keeps its place on the slide
        let k = (m.w * pct / 100.0) / g.w.max(0.01);
        let (cx, cy) = (m.x + m.w / 2.0, m.y + m.h / 2.0);
        let ng = Geometry { x: cx - (cx - g.x) * k, y: cy - (cy - g.y) * k, w: g.w * k, h: g.h * k, ..g };
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Edit Mask");
            doc.set_frame(&mut app.hist, id, ng);
            doc.set_mask_rect(&mut app.hist, id, m.x - ng.x, m.y - ng.y, m.w, m.h);
            doc.end(&mut app.hist);
        }
        app.bump(scene.slide);
    }
    if done {
        app.mask_edit = None;
    }
}

/// The box an object is selected and hit by: a masked picture is its mask (Keynote), except
/// while its mask is being edited (then the whole picture can be grabbed).
fn select_box(app: &mut App, d: &kn_model::Drawable) -> Geometry {
    if let (DrawableKind::Image(i), true) = (&d.kind, d.geom.angle.rem_euclid(360.0) == 0.0 && app.mask_edit != Some(d.id)) {
        if let Some((m, _)) = &i.mask {
            return Geometry { x: d.geom.x + m.x, y: d.geom.y + m.y, w: m.w, h: m.h, ..d.geom };
        }
    }
    app.renderer.effective_geometry(d)
}

/// A path scaled by (sx, sy) about its origin.
fn scale_path(p: &mut kn_model::Path, sx: f32, sy: f32) {
    let sc = |q: &mut Point| {
        q.x *= sx;
        q.y *= sy;
    };
    for el in &mut p.els {
        match el {
            kn_model::PathEl::Move(a) | kn_model::PathEl::Line(a) => sc(a),
            kn_model::PathEl::Quad(a, b) => {
                sc(a);
                sc(b);
            }
            kn_model::PathEl::Cubic(a, b, c) => {
                sc(a);
                sc(b);
                sc(c);
            }
            kn_model::PathEl::Close => {}
        }
    }
}

/// The mask of a picture as a box on the slide (unrotated pictures).
fn mask_box(d: &kn_model::Drawable) -> Option<Geometry> {
    let DrawableKind::Image(i) = &d.kind else { return None };
    if d.geom.angle.rem_euclid(360.0) != 0.0 {
        return None;
    }
    match i.mask.as_ref() {
        Some((m, _)) => Some(Geometry { x: d.geom.x + m.x, y: d.geom.y + m.y, w: m.w, h: m.h, ..d.geom }),
        // being masked for the first time: the whole picture
        None if MASK_EDIT.with(|c| c.get()) == Some(d.id) => Some(d.geom),
        None => None,
    }
}

thread_local!(static MASK_EDIT: std::cell::Cell<Option<ObjId>> = const { std::cell::Cell::new(None) });

/// Topmost editable drawable under a slide point.
fn hit(app: &mut App, scene: &SlideScene, p: Point, tol: f32) -> Option<ObjId> {
    for d in scene.drawables.iter().rev() {
        if d.from_template {
            continue;
        }
        let g = select_box(app, d);
        let l = g.to_local(p);
        let tw = if g.w < tol { tol } else { 0.0 };
        let th = if g.h < tol { tol } else { 0.0 };
        if l.x >= -tw && l.y >= -th && l.x <= g.w + tw && l.y <= g.h + th {
            // empty text placeholders are still selectable (Keynote shows their prompt)
            return Some(d.id);
        }
    }
    None
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    MASK_EDIT.with(|c| c.set(app.mask_edit));
    let Some(scene) = app.scene(app.cur) else { return };
    let avail = ui.available_rect_before_wrap();
    // the canvas runs up under the floating toolbar; the slide fits in the part below it
    let inset = app.tb.inset.min(avail.height() / 2.0);
    let fit = Rect::from_min_max(avail.min + vec2(0.0, inset), avail.max);
    let size = scene.size;
    let scale = match app.zoom {
        // Keynote leaves about 10 pt round the slide (R35)
        Zoom::Fit => ((fit.width() - 20.0) / size.w).min((fit.height() - 20.0) / size.h).max(0.02),
        Zoom::Percent(p) => p / 100.0,
    };
    // pinch on a touchpad (or Ctrl + scroll) zooms the slide, like Keynote: about the pointer
    let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| avail.contains(*p) && !kn_ui::pointer_over_window(ui.ctx()));
    let zoom = ui.input(|i| if pointer.is_some() { i.zoom_delta() } else { 1.0 });
    let mut new_offset: Option<egui::Vec2> = None;
    let scale = if (zoom - 1.0).abs() > 1e-3 {
        let s = (scale * zoom).clamp(0.1, 8.0);
        app.zoom = Zoom::Percent((s * 100.0).round().max(10.0));
        if let Some(p) = pointer {
            // the slide point under the pointer stays under it
            let old_content = vec2((size.w * scale).max(fit.width()), (size.h * scale).max(fit.height()));
            let new_content = vec2((size.w * s).max(fit.width()), (size.h * s).max(fit.height()));
            let old_slide_min = fit.min - app.canvas_scroll + (old_content - vec2(size.w, size.h) * scale) / 2.0;
            let slide_pt = (p - old_slide_min) / scale;
            let new_slide_min_in_content = (new_content - vec2(size.w, size.h) * s) / 2.0;
            let off = new_slide_min_in_content + slide_pt * s - (p - fit.min);
            let max = (new_content + vec2(0.0, inset) - avail.size()).max(egui::Vec2::ZERO);
            new_offset = Some(off.max(egui::Vec2::ZERO).min(max));
        }
        s
    } else {
        scale
    };
    // KN_CANVAS_SCROLL="x,y" (automated checks): scroll the canvas there once
    // (kept up for the first frames: the zoom may not have taken effect on the first one)
    thread_local!(static SCROLLED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
    if let Some(v) = std::env::var("KN_CANVAS_SCROLL").ok().filter(|_| SCROLLED.with(|c| c.get()) < 60 && !matches!(app.zoom, Zoom::Fit)) {
        if let Some((x, y)) = v.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))) {
            new_offset = Some(vec2(x, y));
            SCROLLED.with(|c| c.set(c.get() + 1));
        }
    }
    // Zoom to Selection / Fit Content: that area fills the canvas (with a margin), centred
    let scale = match app.canvas_focus.take() {
        Some([x0, y0, x1, y1]) => {
            let (bw, bh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
            let s = ((fit.width() * 0.9) / bw).min((fit.height() * 0.9) / bh).clamp(0.1, 8.0);
            app.zoom = Zoom::Percent((s * 100.0).round().max(10.0));
            let s = (s * 100.0).round().max(10.0) / 100.0;
            let slide = vec2(size.w * s, size.h * s);
            let content = vec2(slide.x.max(fit.width()), slide.y.max(fit.height()));
            let centre = (content - slide) / 2.0 + vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0) * s;
            new_offset = Some((centre - fit.size() / 2.0).max(egui::Vec2::ZERO));
            s
        }
        None => scale,
    };
    let slide_size = vec2(size.w * scale, size.h * scale);
    let content = vec2(slide_size.x.max(fit.width()), slide_size.y.max(fit.height()) + inset);
    let mut area_ui = egui::ScrollArea::both().auto_shrink(false);
    if let Some(o) = new_offset {
        area_ui = area_ui.scroll_offset(o);
    }
    let mut slide_rect = Rect::NOTHING;
    let out = area_ui.show(ui, |ui| {
        let (area, resp) = ui.allocate_exact_size(content, Sense::click_and_drag());
        let below = Rect::from_min_max(area.min + vec2(0.0, inset), area.max);
        let rect = Rect::from_center_size(below.center(), slide_size);
        slide_rect = rect;
        let view = View { rect, scale };
        canvas_inner(app, ui, &scene, &view, &resp);
    });
    app.canvas_scroll = out.state.clamped_offset();
    // under the floating toolbar the slide shows blurred, sharpening again just below it
    let edge = Rect::from_min_max(avail.min, egui::pos2(avail.max.x, avail.min.y + inset + 18.0));
    crate::glass::set_backdrop(ui.ctx(), None);
    let reach = Rect::from_min_max(slide_rect.min - slide_rect.size(), slide_rect.max + slide_rect.size());
    // translucent panes show the slide where it runs on under them (blurred)
    let mut pane_blur = None;
    if crate::glass::translucent_panes() {
        if let Some(doc) = app.doc.as_ref() {
            let rev = app.slide_rev(scene.slide);
            let tex = app.tex.blurred(ui.ctx(), &mut app.renderer, doc, &scene, scene.slide, rev);
            pane_blur = Some((tex.id(), reach));
        }
    }
    crate::glass::set_slide_blur(ui.ctx(), pane_blur);
    let look = crate::glass::toolbar_background();
    if inset > 0.0 && look == crate::glass::ToolbarBackground::Solid {
        // nothing shows through: the band is always there, scrolled or not
        let solid = Rect::from_min_max(edge.min, egui::pos2(edge.max.x, avail.min.y + inset));
        ui.painter_at(solid).rect_filled(solid, 0.0, crate::theme::PANEL_BG());
    } else if inset > 0.0 && reach.intersects(edge) {
        if let Some(doc) = app.doc.as_ref() {
            let rev = app.slide_rev(scene.slide);
            let tex = app.tex.blurred(ui.ctx(), &mut app.renderer, doc, &scene, scene.slide, rev);
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            let solid = Rect::from_min_max(edge.min, egui::pos2(edge.max.x, avail.min.y + inset));
            // the texture covers the slide and a slide's size round it: everything under the
            // toolbar blurs, also what hangs off the slide (no sharp line at the slide's edge)
            let full = Rect::from_min_max(slide_rect.min - slide_rect.size(), slide_rect.max + slide_rect.size());
            let p = ui.painter_at(solid);
            p.rect_filled(solid, 0.0, crate::theme::CANVAS_BG());
            p.image(tex.id(), full, uv, Color32::WHITE);
            // (the toolbar's liquid glass refracts this)
            crate::glass::set_backdrop(ui.ctx(), Some(crate::glass::Backdrop { tex: tex.id(), slide: full, clip: solid }));
            if look == crate::glass::ToolbarBackground::Tinted {
                // as Pages: the document under the bar blurred in screen space (sigma ~5 pt,
                // worked at half a point per pixel: 3 box passes of 2 px), a tint over it and a
                // hard edge, no fade
                let reach = solid.expand(16.0);
                let region = Rect::from_min_max(((reach.min - slide_rect.min) / scale).to_pos2(), ((reach.max - slide_rect.min) / scale).to_pos2());
                let px = [(reach.width() / 2.0).ceil() as u32, (reach.height() / 2.0).ceil() as u32];
                let band = app.tex.band_blur(ui.ctx(), &mut app.renderer, doc, &scene, scene.slide, rev, region, px, 2);
                p.rect_filled(solid, 0.0, crate::theme::CANVAS_BG());
                p.image(band.id(), reach, uv, Color32::WHITE);
                crate::glass::set_backdrop(ui.ctx(), Some(crate::glass::Backdrop { tex: band.id(), slide: reach, clip: solid }));
                p.rect_filled(solid, 0.0, crate::glass::ToolbarBackground::tint());
                return;
            }
            let bands = 9;
            for k in 0..bands {
                let y0 = solid.max.y + (edge.max.y - solid.max.y) * k as f32 / bands as f32;
                let y1 = solid.max.y + (edge.max.y - solid.max.y) * (k + 1) as f32 / bands as f32;
                let a = 1.0 - (k as f32 + 0.5) / bands as f32;
                let band = Rect::from_min_max(egui::pos2(edge.min.x, y0), egui::pos2(edge.max.x, y1));
                ui.painter_at(band).image(tex.id(), full, uv, Color32::from_white_alpha((a * 255.0) as u8));
            }
        }
    }
}

fn canvas_inner(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View, resp: &egui::Response) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let width_px = (view.rect.width() * ppp).round() as u32;
    let slide = scene.slide;
    let rev = app.slide_rev(slide);
    let painter = ui.painter_at(resp.rect);

    // a soft shadow all round the slide, as Keynote's (a hard offset one read as a grey stroke
    // on the sides and bottom)
    painter.add(egui::Shadow { offset: [0, 1], blur: 10, spread: 0, color: Color32::from_black_alpha(55) }.as_shape(view.rect, 0.0));

    let dragging_sel = matches!(app.canvas.drag, Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Rotate { .. } | Drag::Control { .. } | Drag::LineEnd { .. }));
    let editing_id = app.canvas.editing.as_ref().map(|e| e.id);
    let mut exclude: Vec<ObjId> = if dragging_sel { app.sel.clone() } else { Vec::new() };
    if let Some(id) = editing_id {
        exclude.push(id);
    }
    let doc = app.doc.as_ref().unwrap();
    let base = app.tex.canvas(&ctx, &mut app.renderer, doc, scene, CanvasKey { slide, rev, width: width_px, exclude: exclude.clone() });
    // zoomed in far: the part on screen sharp, on top of the whole slide
    let visible = resp.rect.intersect(view.rect);
    let visible = Rect::from_min_max(pos2((visible.min.x - view.rect.min.x) / view.scale, (visible.min.y - view.rect.min.y) / view.scale), pos2((visible.max.x - view.rect.min.x) / view.scale, (visible.max.y - view.rect.min.y) / view.scale));
    let detail = app.tex.detail(&ctx, doc, scene, CanvasKey { slide, rev, width: width_px, exclude: exclude.clone() }, visible);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    // what of the selection hangs off the slide shows faded around it (Keynote), also while it
    // moves; drawn first, so the slide covers the part on it
    let moving = match &app.canvas.drag {
        Some(Drag::Move { delta, .. }) => Some(vec2(delta.0, delta.1) * view.scale),
        _ => None,
    };
    let off_slide = !app.sel.is_empty()
        && app_scene_bounds(&mut app.renderer, scene, &app.sel).is_some_and(|(lo, hi)| {
            let d = moving.map(|m| m / view.scale).unwrap_or_default();
            lo.x + d.x < 0.0 || lo.y + d.y < 0.0 || hi.x + d.x > scene.size.w || hi.y + d.y > scene.size.h
        });
    // (and of every other object too, as Keynote shows them)
    {
        let sw = scene.size;
        let hanging: Vec<ObjId> = scene
            .drawables
            .iter()
            .filter(|d| !app.sel.contains(&d.id) && !d.from_template)
            .filter(|d| {
                let (lo, hi) = app.renderer.effective_geometry(d).bounds();
                lo.x < 0.0 || lo.y < 0.0 || hi.x > sw.w || hi.y > sw.h
            })
            .map(|d| d.id)
            .collect();
        if !hanging.is_empty() {
            let layer = app.tex.offslide_layer(&ctx, &mut app.renderer, doc, scene, CanvasKey { slide, rev, width: width_px, exclude: hanging });
            paint_layer_faded(&painter, view.rect, &layer);
        }
    }
    if off_slide && app.mask_edit.is_none() {
        let layer = app.tex.layer(&ctx, &mut app.renderer, doc, scene, CanvasKey { slide, rev, width: width_px, exclude: app.sel.clone() });
        paint_layer_faded(&painter, view.rect.translate(moving.unwrap_or_default()), &layer);
    }
    // a dynamic background, live (Preview Motion) or still, under the slide's objects
    if let Some(mb) = &scene.motion {
        let w = ((view.rect.width() * ppp) / 2.5).clamp(320.0, 1024.0) as u32;
        let h = (w as f32 * scene.size.h / scene.size.w).round().max(2.0) as u32;
        let t = if app.preview_motion { app.motion_clock.elapsed().as_secs_f32() } else { 0.0 };
        let tex = app.motion_tex.get(&ctx, mb, t, w, h);
        painter.image(tex.id(), view.rect, uv, Color32::WHITE);
        if app.preview_motion {
            ctx.request_repaint();
        }
    }
    painter.image(base.id(), view.rect, uv, Color32::WHITE);
    if let Some((tex, area)) = &detail {
        let r = Rect::from_min_max(view.rect.min + area.min.to_vec2() * view.scale, view.rect.min + area.max.to_vec2() * view.scale);
        painter.image(tex.id(), r, uv, Color32::WHITE);
    }

    // the object being edited: rendered from a scene rebuilt after each keystroke
    let mut edit_info: Option<(kn_render::text::TextLayout, Geometry)> = None;
    if let Some(id) = editing_id {
        if app.canvas.editing.as_ref().is_some_and(|e| e.scene.is_none()) {
            let fresh = app.doc.as_ref().and_then(|d| d.scene(&app.slides, app.cur));
            if let Some(e) = app.canvas.editing.as_mut() {
                e.scene = fresh;
            }
        }
        let version = app.canvas.editing.as_ref().map(|e| e.version).unwrap_or(0);
        if let Some(esc) = app.canvas.editing.as_ref().and_then(|e| e.scene.clone()) {
            let doc = app.doc.as_ref().unwrap();
            let layer = app.tex.layer(&ctx, &mut app.renderer, doc, &esc, CanvasKey { slide, rev: (1 << 62) | version, width: width_px, exclude: vec![id] });
            paint_layer(&painter, view.rect, &layer);
            if let Some(d) = esc.drawables.iter().find(|d| d.id == id) {
                edit_info = crate::textedit::stored_layout(app.renderer.text_layout(d), d);
            }
        }
    }
    let doc = app.doc.as_ref().unwrap();

    // live preview of the dragged objects
    if dragging_sel {
        match app.canvas.drag.clone() {
            Some(Drag::Move { delta, .. }) => {
                // on the slide at full strength (the faded copy shows what is off it)
                let layer = app.tex.layer(&ctx, &mut app.renderer, doc, scene, CanvasKey { slide, rev, width: width_px, exclude: app.sel.clone() });
                paint_layer(&painter.with_clip_rect(painter.clip_rect().intersect(view.rect)), view.rect.translate(vec2(delta.0, delta.1) * view.scale), &layer);
            }
            Some(Drag::Control { id, g, q }) => {
                let mut sc = scene.clone();
                if let Some(d) = sc.drawables.iter_mut().find(|d| d.id == id) {
                    if let (DrawableKind::Shape(sh), Some(p)) = (&mut d.kind, doc.shape_path_with_control(id, g.w, g.h, q)) {
                        sh.path = p;
                    }
                }
                let vrev = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (q.x.to_bits(), q.y.to_bits(), 9u8).hash(&mut h);
                    h.finish()
                };
                let layer = app.tex.layer(&ctx, &mut app.renderer, doc, &sc, CanvasKey { slide, rev: vrev, width: width_px, exclude: app.sel.clone() });
                paint_layer(&painter, view.rect, &layer);
            }
            Some(Drag::LineEnd { id, a, b, .. }) => {
                let mut sc = scene.clone();
                if let Some(d) = sc.drawables.iter_mut().find(|d| d.id == id) {
                    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
                    d.geom = Geometry { x: x0, y: y0, w: (a.x - b.x).abs(), h: (a.y - b.y).abs(), angle: 0.0, hflip: false, vflip: false, ..d.geom };
                    if let DrawableKind::Shape(sh) = &mut d.kind {
                        let mut p = kn_model::Path::default();
                        p.move_to(a.x - x0, a.y - y0);
                        p.line_to(b.x - x0, b.y - y0);
                        sh.path = p;
                    }
                }
                let vrev = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits(), 7u8).hash(&mut h);
                    h.finish()
                };
                let layer = app.tex.layer(&ctx, &mut app.renderer, doc, &sc, CanvasKey { slide, rev: vrev, width: width_px, exclude: app.sel.clone() });
                paint_layer(&painter, view.rect, &layer);
            }
            Some(Drag::Resize { id, cur, .. }) | Some(Drag::Rotate { id, cur, .. }) => {
                let mut sc = scene.clone();
                let editing_mask = app.mask_edit == Some(id);
                if let Some(d) = sc.drawables.iter_mut().find(|d| d.id == id) {
                    // a masked picture previews the way the resize will end: in Edit Mask only the
                    // mask changes; otherwise the picture and its mask scale together
                    let masked = mask_box(d).filter(|_| matches!(app.canvas.drag, Some(Drag::Resize { .. })));
                    if let (Some(m), DrawableKind::Image(i)) = (masked, &mut d.kind) {
                        let g = d.geom;
                        if let Some((mg, mp)) = i.mask.as_mut() {
                            let (sx, sy) = (cur.w / m.w.max(0.01), cur.h / m.h.max(0.01));
                            scale_path(mp, sx, sy);
                            if editing_mask {
                                *mg = Geometry { x: cur.x - g.x, y: cur.y - g.y, w: cur.w, h: cur.h, ..*mg };
                            } else {
                                let (mx, my) = (m.x - g.x, m.y - g.y);
                                d.geom = Geometry { x: cur.x - mx * sx, y: cur.y - my * sy, w: g.w * sx, h: g.h * sy, ..g };
                                *mg = Geometry { x: mx * sx, y: my * sy, w: cur.w, h: cur.h, ..*mg };
                            }
                        }
                    } else {
                        d.geom = cur;
                    }
                    // tables draw at their row/column sizes: scale those to the new size
                    if let DrawableKind::Table(t) = &mut d.kind {
                        let (tw, th) = (t.width(), t.height());
                        if tw > 0.0 {
                            t.col_widths.iter_mut().for_each(|s| *s *= cur.w / tw);
                        }
                        if th > 0.0 {
                            t.row_heights.iter_mut().for_each(|s| *s *= cur.h / th);
                        }
                    }
                    // the outline at the new size (shape paths are stored for the old one)
                    if let DrawableKind::Shape(sh) = &mut d.kind {
                        if let Some(p) = doc.shape_path_at(id, cur.w, cur.h) {
                            sh.path = p;
                        }
                    }
                }
                let vrev = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (cur.x.to_bits(), cur.y.to_bits(), cur.w.to_bits(), cur.h.to_bits(), cur.angle.to_bits()).hash(&mut h);
                    h.finish()
                };
                let layer = app.tex.layer(&ctx, &mut app.renderer, doc, &sc, CanvasKey { slide, rev: vrev, width: width_px, exclude: app.sel.clone() });
                paint_layer(&painter, view.rect, &layer);
            }
            _ => {}
        }
    }

    // tagged placeholders (Define as Text Placeholder): empty ones show the layout's prompt
    if let Some(doc) = app.doc.as_ref() {
        let sm = doc.file.obj(scene.slide);
        let tags: Vec<(String, ObjId)> = sm.map(|m| m.msgs("sage_tag_to_info_map").filter_map(|e| Some((e.str("tag")?.to_string(), e.ref_id("info")?))).collect()).unwrap_or_default();
        if !tags.is_empty() {
            let layout = sm.and_then(|m| m.ref_id("template_slide")).and_then(|l| doc.file.obj(l));
            for (tag, obj) in tags {
                let Some(d) = scene.drawables.iter().find(|d| d.id == obj) else { continue };
                let DrawableKind::Shape(s) = &d.kind else { continue };
                if !s.text.as_ref().is_none_or(|t| t.text.trim().is_empty()) || editing_id == Some(obj) {
                    continue;
                }
                // the layout's display name for the same tag
                let prompt = layout
                    .and_then(|l| {
                        let info = l.msgs("sage_tag_to_info_map").find(|e| e.str("tag") == Some(tag.as_str()))?.ref_id("info")?;
                        l.msg("instructional_text_map")?.msgs("instructional_text_for_infos").find(|e| e.ref_id("info") == Some(info))?.str("instructional_text").map(str::to_string)
                    })
                    .unwrap_or(tag.clone());
                let g = d.geom;
                let r = Rect::from_min_size(view.to_screen(Point::new(g.x, g.y)), vec2(g.w.max(40.0), g.h.max(24.0)) * view.scale);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(190)), StrokeKind::Inside);
                crate::theme::centered(&painter, r.center(), prompt, egui::FontId::proportional((24.0 * view.scale).clamp(9.0, 40.0)), Color32::from_gray(160));
            }
        }
    }
    // empty title/body placeholders show Keynote's grey prompt and outline (editing only)
    for d in &scene.drawables {
        if let (Some(kind), DrawableKind::Shape(s)) = (d.placeholder, &d.kind) {
            if s.text.as_ref().is_some_and(|t| t.text.is_empty()) && !d.from_template && editing_id != Some(d.id) {
                let g = d.geom;
                let r = Rect::from_min_size(view.to_screen(Point::new(g.x, g.y)), vec2(g.w, g.h) * view.scale);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(190)), StrokeKind::Inside);
                let prompt = match kind {
                    kn_model::PlaceholderKind::Title => "Double-click to add title",
                    kn_model::PlaceholderKind::Body => "Double-click to add text",
                    _ => continue,
                };
                let size = s.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.style.size).unwrap_or(48.0);
                crate::theme::centered(&painter, r.center(), prompt, egui::FontId::proportional((size * 0.6 * view.scale).clamp(9.0, 64.0)), Color32::from_gray(160));
            }
        }
    }

    if app.point_edit.as_ref().is_some_and(|p| app.sel == [p.id]) {
        point_mode(app, ui, scene, view, resp);
        return;
    } else if app.point_edit.is_some() {
        app.point_edit = None;
    }
    // Draw With Pen: strokes become freehand lines; Esc or Done leaves pen mode
    if app.pen.is_some() {
        pen_mode(app, ui, scene, view, resp);
        return;
    }

    // ---------------------------------------------------------------- interaction
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    let shift = ui.input(|i| i.modifiers.shift);
    let ctrl = ui.input(|i| i.modifiers.command);
    let handle_r = 5.0;

    // which handle is under the pointer (single selection only)
    let single = (app.sel.len() == 1).then(|| app.sel[0]);
    let sel_geom = single.and_then(|id| scene.drawables.iter().find(|d| d.id == id)).map(|d| {
        // editing a mask: the handles are the mask's
        if app.mask_edit == Some(d.id) {
            if let Some(m) = mask_box(d) {
                return m;
            }
        }
        select_box(app, d)
    });
    let sel_line = single.and_then(|id| scene.drawables.iter().find(|d| d.id == id)).and_then(line_ends);
    // the green control handle (slide coordinates) of a selected arrow / star / rounded rectangle
    let sel_control = single
        .and_then(|id| scene.drawables.iter().find(|d| d.id == id))
        .filter(|d| !d.locked && matches!(&d.kind, DrawableKind::Shape(s) if !s.is_line))
        .and_then(|d| app.doc.as_ref()?.shape_control(d.id, d.geom.w, d.geom.h).map(|c| (d.id, d.geom, c.pos)));
    let handle_under = |p: Pos2| -> Option<usize> {
        if let Some((a, b)) = sel_line {
            return [a, b].iter().position(|q| view.to_screen(*q).distance(p) <= handle_r + 6.0).map(|i| 100 + i);
        }
        if let Some((_, g, c)) = sel_control {
            if view.to_screen(g.to_slide(c)).distance(p) <= handle_r + 5.0 {
                return Some(200);
            }
        }
        let g = sel_geom?;
        handle_points(&g).iter().position(|hp| view.to_screen(g.to_slide(*hp)).distance(p) <= handle_r + 6.0)
    };

    // clicks and drags inside the text being edited move the caret / select
    let mut handled = false;
    // typing a formula: pressing / dragging over the table puts that cell / range into it
    if let (Some((tid, er, ec, _, _)), Some(p)) = (app.canvas.cell.clone(), pointer) {
        let formula = app.canvas.cell.as_ref().is_some_and(|c| c.3.trim_start().starts_with('='));
        let under = table_cell_at(app, scene, tid, view.to_slide(p)).map(|(r, c, _)| (r, c));
        if formula && (resp.drag_started() || resp.dragged() || resp.clicked()) {
            if let Some((r, c)) = under.filter(|rc| *rc != (er, ec)) {
                let start = match app.canvas.formula_pick {
                    Some((a, b, at)) if !resp.drag_started() && !resp.clicked() => (a, b, at),
                    _ => (r, c, app.canvas.cell.as_ref().map(|x| x.3.len()).unwrap_or(0)),
                };
                if resp.drag_started() || resp.clicked() {
                    // the press itself picks the first cell of the range
                    let origin = ui.input(|i| i.pointer.press_origin()).and_then(|o| table_cell_at(app, scene, tid, view.to_slide(o)).map(|(r, c, _)| (r, c)));
                    let (a, b) = origin.unwrap_or((r, c));
                    app.canvas.formula_pick = Some((a, b, start.2));
                }
                let (a, b, at) = app.canvas.formula_pick.unwrap_or(start);
                let name = |r: u32, c: u32| format!("{}{}", column_letters(c), r + 1);
                let reference = if (a, b) == (r, c) { name(r, c) } else { format!("{}:{}", name(a.min(r), b.min(c)), name(a.max(r), b.max(c))) };
                if let Some(cell) = app.canvas.cell.as_mut() {
                    cell.3.truncate(at.min(cell.3.len()));
                    cell.3.push_str(&reference);
                }
                app.canvas.formula_caret_end = true;
                if resp.clicked() {
                    app.canvas.formula_pick = None;
                }
                handled = true;
            }
        }
        if !resp.dragged() && !resp.drag_started() && app.canvas.formula_pick.is_some() && ui.input(|i| i.pointer.any_released()) {
            app.canvas.formula_pick = None;
        }
    }
    if let (Some((tl, g)), Some(p)) = (edit_info.as_ref(), pointer) {
        let sp = view.to_slide(p);
        if crate::textedit::contains(g, sp) && (resp.clicked() || resp.drag_started() || resp.dragged() || resp.double_clicked()) {
            let extend = shift || (resp.dragged() && !resp.drag_started());
            crate::textedit::point_to(app, tl, g, sp, extend);
            handled = true;
            ctx.request_repaint();
        } else if resp.clicked() || resp.drag_started() {
            crate::textedit::commit(app);
        }
    }
    if handled {
    } else if resp.double_clicked() {
        if let Some(p) = pointer {
            let sp = view.to_slide(p);
            if let Some(id) = hit(app, scene, sp, 8.0 / view.scale) {
                let has_text = scene
                    .drawables
                    .iter()
                    .find(|d| d.id == id)
                    .is_some_and(|d| matches!(&d.kind, DrawableKind::Shape(s) if s.text.is_some()));
                if has_text && crate::textedit::begin(app, scene, id, Some(sp)) {
                    app.sel = vec![id];
                }
                if scene.drawables.iter().any(|x| x.id == id && matches!(x.kind, DrawableKind::Chart(_))) {
                    app.sel = vec![id];
                    crate::chartedit::open(app, id);
                }
                if let Some((r, c, text)) = table_cell_at(app, scene, id, sp) {
                    app.sel = vec![id];
                    // formula cells are edited as their formula
                    let text = app.doc.as_ref().and_then(|d| d.formula_text(id, r, c)).unwrap_or(text);
                    app.canvas.cell = Some((id, r, c, text, true));
                }
            }
        }
    }
    if resp.clicked() || resp.drag_started() {
        app.nav_focus = false;
    }
    if handled || resp.double_clicked() {
        // (handled above)
    } else if resp.drag_started() {
        // where the press began: by the time a drag is recognised the pointer has moved a few
        // pixels and may already be off a small resize handle
        let origin = ui.input(|i| i.pointer.press_origin());
        if let Some(p) = origin.or(resp.interact_pointer_pos()) {
            let sp = view.to_slide(p);
            if let (Some(200), Some((id, g, c))) = (handle_under(p), sel_control) {
                app.canvas.drag = Some(Drag::Control { id, g, q: c });
            } else if let (Some(h), Some(id), Some((a, b))) = (handle_under(p).filter(|h| (100..200).contains(h)), single, sel_line) {
                app.canvas.drag = Some(Drag::LineEnd { id, end: h - 100, a, b });
            } else if let (Some(h), Some(id), Some(g)) = (handle_under(p), single, sel_geom) {
                app.canvas.drag = Some(if ctrl {
                    let c = view.to_screen(g.center());
                    Drag::Rotate { id, orig: g, start_angle: (p.y - c.y).atan2(p.x - c.x), cur: g }
                } else {
                    Drag::Resize { id, handle: h, orig: g, cur: g }
                });
            } else if let Some(g) = guide_grab(app, view, p, sp, scene) {
                app.canvas.drag = Some(g);
            } else if let Some((id, r, c)) = single.filter(|_| !shift && !ctrl).and_then(|id| table_cell_at(app, scene, id, sp).map(|(r, c, _)| (id, r, c))) {
                // dragging across the cells of a selected table selects them (Keynote); the
                // table itself moves by its round handle
                app.canvas.cell_drag = Some((id, r, c));
                app.canvas.table_cursor = Some((id, r, c));
                app.canvas.table_range = None;
            } else {
                match hit(app, scene, sp, 8.0 / view.scale) {
                    Some(id) => {
                        if !app.sel.contains(&id) {
                            // Shift or Ctrl (Keynote's ⌘) adds to the selection
                            if shift || ctrl {
                                app.sel.push(id)
                            } else {
                                app.sel = vec![id]
                            }
                        }
                        let locked = scene.drawables.iter().filter(|d| app.sel.contains(&d.id)).any(|d| d.locked);
                        if !locked {
                            app.canvas.drag = Some(Drag::Move { start: sp, delta: (0.0, 0.0) });
                        }
                    }
                    None => {
                        if !shift && !ctrl {
                            app.sel.clear();
                        }
                        app.canvas.drag = Some(Drag::Rubber { start: p, cur: p });
                    }
                }
            }
        }
    } else if resp.clicked() {
        if let Some(p) = pointer {
            let sp = view.to_slide(p);
            let hit_id = hit(app, scene, sp, 8.0 / view.scale);
            // a click in an already selected table selects a cell; Shift-click a cell range
            let cell = hit_id.filter(|id| app.sel == [*id]).and_then(|id| table_cell_at(app, scene, id, sp).map(|(r, c, _)| (id, r, c)));
            match hit_id {
                Some(id) if cell.is_some() => {
                    let (_, r, c) = cell.unwrap();
                    match app.canvas.table_cursor.filter(|(t, ..)| shift && *t == id) {
                        Some((_, r0, c0)) => app.canvas.table_range = Some((id, r0.min(r), c0.min(c), r0.max(r), c0.max(c))),
                        None => {
                            app.canvas.table_cursor = Some((id, r, c));
                            app.canvas.table_range = None;
                        }
                    }
                }
                Some(id) if shift || ctrl => {
                    if let Some(i) = app.sel.iter().position(|x| *x == id) {
                        app.sel.remove(i);
                    } else {
                        app.sel.push(id);
                    }
                }
                Some(id) => app.sel = vec![id],
                None => app.sel.clear(),
            }
        }
    }

    if let (Some((id, r0, c0)), true) = (app.canvas.cell_drag, resp.dragged()) {
        if let Some((r, c, _)) = pointer.and_then(|p| table_cell_at(app, scene, id, view.to_slide(p))) {
            app.canvas.table_range = ((r, c) != (r0, c0)).then_some((id, r0.min(r), c0.min(c), r0.max(r), c0.max(c)));
        }
        ctx.request_repaint();
    }
    if resp.drag_stopped() {
        app.canvas.cell_drag = None;
    }
    if resp.dragged() && !handled && app.canvas.cell_drag.is_none() {
        set_snap_guides(app, scene);
        if let Some(p) = resp.interact_pointer_pos() {
            let sp = view.to_slide(p);
            match &mut app.canvas.drag {
                Some(Drag::Move { start, delta }) => {
                    let mut d = (sp.x - start.x, sp.y - start.y);
                    if shift {
                        // Keynote: along the nearest of 0°, 45° and 90° (and their opposites)
                        let len = (d.0 * d.0 + d.1 * d.1).sqrt();
                        let a = (d.1.atan2(d.0) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
                        let along = d.0 * a.cos() + d.1 * a.sin();
                        d = if len > 0.0 { (along * a.cos(), along * a.sin()) } else { d };
                    }
                    let opts = snap_opts(&app.settings, view, ctrl);
                    // each selected object snaps by itself (Keynote, R36): the smallest correction wins
                    let others = other_boxes(&mut app.renderer, scene, &app.sel);
                    let boxes: Vec<crate::snap::Bx> =
                        scene.drawables.iter().filter(|x| app.sel.contains(&x.id)).map(|x| { let (lo, hi) = app.renderer.effective_geometry(x).bounds(); crate::snap::Bx::new(lo, hi) }).collect();
                    let mut best: Option<((f32, f32), Vec<crate::snap::Guide>, f32)> = None;
                    for b in boxes {
                        let (d2, g) = crate::snap::snap_move(b, &others, slide_box(scene), d, &opts);
                        let fix = (d2.0 - d.0).abs() + (d2.1 - d.1).abs();
                        if !g.is_empty() && best.as_ref().is_none_or(|x| fix < x.2) {
                            best = Some((d2, g, fix));
                        }
                    }
                    match best {
                        Some((d2, g, _)) => {
                            *delta = d2;
                            app.canvas.guides = g;
                        }
                        None => {
                            *delta = d;
                            app.canvas.guides.clear();
                        }
                    }
                }
                Some(Drag::Resize { id, handle, orig, cur }) => {
                    let local = orig.to_local(sp);
                    let corner = *handle % 2 == 0;
                    *cur = resized(orig, *handle, local, shift && corner);
                    app.canvas.guides.clear();
                    // unrotated, free resizes snap their moving edges (Keynote's guides)
                    if cur.angle.rem_euclid(360.0) == 0.0 && !(shift && corner) {
                        let opts = snap_opts(&app.settings, view, ctrl);
                        let others = other_boxes(&mut app.renderer, scene, &[*id]);
                        let b = crate::snap::Bx { x0: cur.x, y0: cur.y, x1: cur.x + cur.w, y1: cur.y + cur.h };
                        let (b, g) = crate::snap::snap_resize(b, handle_edges(*handle), &others, slide_box(scene), &opts);
                        (cur.x, cur.y, cur.w, cur.h) = (b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0);
                        app.canvas.guides = g;
                    }
                }
                Some(Drag::Rotate { orig, start_angle, cur, .. }) => {
                    let c = view.to_screen(orig.center());
                    let a = (p.y - c.y).atan2(p.x - c.x);
                    let mut deg = orig.angle - (a - *start_angle).to_degrees();
                    if shift {
                        deg = (deg / 45.0).round() * 45.0;
                    }
                    cur.angle = deg.rem_euclid(360.0);
                }
                Some(Drag::Rubber { cur, .. }) => *cur = p,
                Some(Drag::Guide { vertical, pos, .. }) => *pos = if *vertical { sp.x } else { sp.y },
                Some(Drag::Control { g, q, .. }) => *q = g.to_local(sp),
                Some(Drag::LineEnd { end, a, b, .. }) => {
                    let fixed = if *end == 0 { *b } else { *a };
                    let mut q = sp;
                    if shift {
                        // Shift: snap to 45° steps, like Keynote
                        let (dx, dy) = (q.x - fixed.x, q.y - fixed.y);
                        let len = (dx * dx + dy * dy).sqrt();
                        let ang = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
                        q = Point::new(fixed.x + len * ang.cos(), fixed.y + len * ang.sin());
                    }
                    if *end == 0 {
                        *a = q;
                    } else {
                        *b = q;
                    }
                }
                None => {}
            }
            ctx.request_repaint();
        }
    }

    // the frame a drag ends still shows the old scene: draw the overlays at the drag's final
    // place so the selection doesn't flash back for a frame
    let mut finished: Option<Drag> = None;
    if resp.drag_stopped() {
        app.canvas.guides.clear();
        if let Some(drag) = app.canvas.drag.take() {
            finished = Some(drag.clone());
            // Alt (Keynote: Option) while letting go of a moved selection: a copy is moved
            app.canvas.alt_drop = ui.input(|i| i.modifiers.alt);
            finish_drag(app, scene, drag, view);
            ctx.request_repaint();
        }
    }

    // right-click: select what's under the pointer, then Keynote's context menu
    if resp.secondary_clicked() {
        crate::textedit::commit(app);
        app.nav_focus = false;
        if let Some(p) = pointer {
            match hit(app, scene, view.to_slide(p), 8.0 / view.scale) {
                Some(id) if !app.sel.contains(&id) => app.sel = vec![id],
                Some(_) => {}
                None => app.sel.clear(),
            }
            // right-click in a table picks that cell (unless it is inside the selected range)
            if let [id] = app.sel[..] {
                if let Some((r, c, _)) = table_cell_at(app, scene, id, view.to_slide(p)) {
                    let in_range = app.canvas.table_range.is_some_and(|(t, r0, c0, r1, c1)| t == id && (r0..=r1).contains(&r) && (c0..=c1).contains(&c));
                    if !in_range {
                        app.canvas.table_cursor = Some((id, r, c));
                        app.canvas.table_range = None;
                    }
                }
            }
        }
    }
    resp.context_menu(|ui| {
        ui.set_min_width(240.0);
        let item = |app: &mut App, ui: &mut egui::Ui, l: &str| crate::menus::context_item(app, ui, l, l);
        if app.sel.is_empty() {
            item(app, ui, "Paste");
            item(app, ui, "Select All");
            ui.separator();
            crate::menus::context_item(app, ui, "New Slide", "New Slide");
            item(app, ui, "Reapply Layout to Slide");
            return;
        }
        for l in ["Cut", "Copy", "Paste", "Delete"] {
            item(app, ui, l);
        }
        // a table cell: rows, columns and sorting (Keynote's cell menu)
        if table_cursor(app).is_some() {
            ui.separator();
            for (label, cmd) in [
                ("Add Row Above", "Table > Add Row Above"),
                ("Add Row Below", "Table > Add Row Below"),
                ("Add Column Before", "Table > Add Column Before"),
                ("Add Column After", "Table > Add Column After"),
            ] {
                crate::menus::context_item(app, ui, label, cmd);
            }
            ui.separator();
            crate::menus::context_item(app, ui, "Delete Row", "Table > Delete Row");
            crate::menus::context_item(app, ui, "Delete Column", "Table > Delete Column");
            ui.separator();
            item(app, ui, "Sort Ascending");
            item(app, ui, "Sort Descending");
            ui.separator();
            item(app, ui, "Merge Cells");
            item(app, ui, "Unmerge Cells");
        }
        crate::menus::context_item(app, ui, "Duplicate", "Duplicate Selection");
        ui.separator();
        for l in ["Bring to Front", "Bring Forward", "Send Backward", "Send to Back"] {
            item(app, ui, l);
        }
        ui.separator();
        let locked = app.sel.iter().any(|id| scene.drawables.iter().any(|d| d.id == *id && d.locked));
        item(app, ui, if locked { "Unlock" } else { "Lock" });
        if app.sel.len() >= 2 {
            item(app, ui, "Group");
        } else {
            item(app, ui, "Ungroup");
        }
        ui.separator();
        crate::menus::context_item(app, ui, "Comment", "Comment");
        crate::menus::context_item(app, ui, "Add Link…", "Add Link");
        ui.separator();
        item(app, ui, "Copy Style");
        item(app, ui, "Paste Style");
        item(app, ui, "Copy Animation");
        item(app, ui, "Paste Animation");
    });

    // keyboard nudge / escape (a table with a cell selected moves between its cells instead)
    let cell_keys = table_keys(app, ui, &ctx, scene);
    if !cell_keys && !app.sel.is_empty() && app.canvas.editing.is_none() && !ctx.egui_wants_keyboard_input() {
        let step = if shift { 10.0 } else { 1.0 };
        let (mut dx, mut dy) = (0.0, 0.0);
        ui.input(|i| {
            if i.key_pressed(egui::Key::ArrowLeft) {
                dx -= step
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                dx += step
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                dy -= step
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                dy += step
            }
        });
        if dx != 0.0 || dy != 0.0 {
            let ids = app.sel.clone();
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Move");
                for id in ids {
                    doc.move_by(&mut app.hist, id, dx, dy);
                }
                doc.end(&mut app.hist);
            }
            app.bump(slide);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.sel.clear();
        }
    }

    // ---------------------------------------------------------------- overlays
    let painter = ui.painter_at(resp.rect);
    let shown_drag = app.canvas.drag.clone().or(finished);
    let sel_ids = app.sel.clone();
    for d in scene.drawables.iter().filter(|d| sel_ids.contains(&d.id)) {
        let mut g = if app.mask_edit == Some(d.id) { mask_box(d).unwrap_or(d.geom) } else { select_box(app, d) };
        match &shown_drag {
            Some(Drag::Move { delta, .. }) => {
                g.x += delta.0;
                g.y += delta.1;
            }
            Some(Drag::Resize { id, cur, .. } | Drag::Rotate { id, cur, .. }) if *id == d.id => g = *cur,
            _ => {}
        }
        let handle_fill = if d.locked { Color32::from_gray(120) } else { theme::HANDLE() };
        if let Some((mut a, mut b)) = line_ends(d) {
            match &shown_drag {
                Some(Drag::LineEnd { id, a: da, b: db, .. }) if *id == d.id => {
                    a = *da;
                    b = *db;
                }
                Some(Drag::Move { delta, .. }) => {
                    a = Point::new(a.x + delta.0, a.y + delta.1);
                    b = Point::new(b.x + delta.0, b.y + delta.1);
                }
                _ => {}
            }
            // Keynote: a round handle on each end, no box
            for q in [a, b] {
                let c = view.to_screen(q);
                painter.circle_filled(c, handle_r + 1.0, handle_fill);
                painter.circle_stroke(c, handle_r + 1.0, Stroke::new(1.0, theme::ACCENT()));
            }
            continue;
        }
        if app.mask_edit == Some(d.id) {
            // Edit Mask: the whole picture, faded where the mask hides it
            if let (DrawableKind::Image(i), true) = (&d.kind, d.geom.angle.rem_euclid(360.0) == 0.0) {
                if let Some(tex) = i.data_id.and_then(|data| mask_picture(app, ui.ctx(), data)) {
                    let r = Rect::from_two_pos(view.to_screen(Point::new(d.geom.x, d.geom.y)), view.to_screen(Point::new(d.geom.x + d.geom.w, d.geom.y + d.geom.h)));
                    let uv = Rect::from_min_max(pos2(if d.geom.hflip { 1.0 } else { 0.0 }, if d.geom.vflip { 1.0 } else { 0.0 }), pos2(if d.geom.hflip { 0.0 } else { 1.0 }, if d.geom.vflip { 0.0 } else { 1.0 }));
                    // only around the mask (inside it the picture already shows at full strength)
                    let shown = match &app.canvas.drag {
                        Some(Drag::Resize { id, cur, .. }) if *id == d.id => Some(*cur),
                        _ => mask_box(d),
                    };
                    let hole = shown.map(|m| Rect::from_two_pos(view.to_screen(Point::new(m.x, m.y)), view.to_screen(Point::new(m.x + m.w, m.y + m.h))));
                    let strips = match hole {
                        Some(h) => vec![
                            Rect::from_min_max(r.min, pos2(r.max.x, h.min.y)),
                            Rect::from_min_max(pos2(r.min.x, h.max.y), r.max),
                            Rect::from_min_max(pos2(r.min.x, h.min.y), pos2(h.min.x, h.max.y)),
                            Rect::from_min_max(pos2(h.max.x, h.min.y), pos2(r.max.x, h.max.y)),
                        ],
                        None => vec![r],
                    };
                    for strip in strips.into_iter().filter(|x| x.width() > 0.0 && x.height() > 0.0) {
                        painter.with_clip_rect(painter.clip_rect().intersect(strip)).image(tex.id(), r, uv, Color32::from_white_alpha(110));
                    }
                }
            }
            let fg = d.geom;
            let fc = [Point::new(0.0, 0.0), Point::new(fg.w, 0.0), Point::new(fg.w, fg.h), Point::new(0.0, fg.h)].map(|p| view.to_screen(fg.to_slide(p)));
            for i in 0..4 {
                painter.extend(egui::Shape::dashed_line(&[fc[i], fc[(i + 1) % 4]], Stroke::new(1.0, theme::ACCENT()), 5.0, 4.0));
            }
        }
        // (a table with cells selected shows the cell selection instead, as in Keynote)
        if app.sel == [d.id] && app.canvas.table_cursor.is_some_and(|c| c.0 == d.id) && matches!(d.kind, DrawableKind::Table(_)) {
            continue;
        }
        let corners = [Point::new(0.0, 0.0), Point::new(g.w, 0.0), Point::new(g.w, g.h), Point::new(0.0, g.h)]
            .map(|p| view.to_screen(g.to_slide(p)));
        let outline = Stroke::new(1.0, theme::ACCENT());
        for i in 0..4 {
            painter.line_segment([corners[i], corners[(i + 1) % 4]], outline);
        }
        if app.sel.len() == 1 {
            for hp in handle_points(&g) {
                let c = view.to_screen(g.to_slide(hp));
                painter.rect_filled(Rect::from_center_size(c, vec2(handle_r * 2.0, handle_r * 2.0)), 1.0, handle_fill);
                painter.rect_stroke(Rect::from_center_size(c, vec2(handle_r * 2.0, handle_r * 2.0)), 1.0, Stroke::new(1.0, theme::ACCENT()), StrokeKind::Inside);
            }
        }
        // Keynote's green control handle
        if app.sel.len() == 1 && !d.locked {
            let pos = match &shown_drag {
                Some(Drag::Control { id, q, .. }) if *id == d.id => Some(*q),
                Some(Drag::Resize { .. } | Drag::Rotate { .. } | Drag::Move { .. }) => None,
                _ => app.doc.as_ref().and_then(|doc| doc.shape_control(d.id, d.geom.w, d.geom.h)).map(|c| c.pos),
            };
            if let Some(q) = pos.filter(|_| matches!(&d.kind, DrawableKind::Shape(s) if !s.is_line)) {
                let c = view.to_screen(d.geom.to_slide(q));
                painter.circle_filled(c, handle_r, Color32::from_rgb(0x5c, 0xd1, 0x4a));
                painter.circle_stroke(c, handle_r, Stroke::new(1.0, Color32::WHITE));
            }
        }
        if let Some(Drag::Rotate { cur, .. }) = &shown_drag {
            let c = view.to_screen(g.center());
            painter.text(c + vec2(0.0, -20.0), egui::Align2::CENTER_BOTTOM, format!("{:.0}°", cur.angle), egui::FontId::proportional(13.0), Color32::WHITE);
        }
    }
    // Keynote's size/position bubble while dragging
    if app.settings.show_size_while_moving {
        let info = match &shown_drag {
            Some(Drag::Move { delta, .. }) => scene
                .drawables
                .iter()
                .find(|d| app.sel.first() == Some(&d.id))
                .map(|d| d.geom)
                .map(|g| format!("x: {:.0} pt   y: {:.0} pt", g.x + delta.0, g.y + delta.1)),
            Some(Drag::Resize { cur, .. }) => Some(format!("w: {:.0} pt   h: {:.0} pt", cur.w, cur.h)),
            _ => None,
        };
        if let (Some(text), Some(p)) = (info, ui.ctx().pointer_latest_pos()) {
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::WHITE);
            let r = Rect::from_min_size(p + vec2(14.0, 14.0), galley.size() + vec2(12.0, 6.0));
            painter.rect_filled(r, 4.0, Color32::from_black_alpha(200));
            painter.galley(r.min + vec2(6.0, 3.0), galley, Color32::WHITE);
        }
    }
    if let Some(Drag::Rubber { start, cur }) = &shown_drag {
        let r = Rect::from_two_pos(*start, *cur);
        painter.rect_filled(r, 0.0, theme::ACCENT().gamma_multiply(0.15));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::ACCENT()), StrokeKind::Inside);
    }
    if app.canvas.drag.is_some() {
        draw_guides(&painter, view, &app.canvas.guides);
    }

    draw_user_guides(app, &painter, view, scene, shown_drag.as_ref());
    // review highlights (Insert > Highlight), in the author's colour, while editing
    for d in &scene.drawables {
        let DrawableKind::Shape(sh) = &d.kind else { continue };
        let Some(tb) = sh.text.as_ref().filter(|t| !t.highlights.is_empty()) else { continue };
        let ranges = tb.highlights.clone();
        if let Some((tl, g)) = app.renderer.text_layout(d) {
            for (a, b, c) in ranges {
                let col = Color32::from_rgba_unmultiplied((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, 90);
                for gl in tl.glyphs.iter().filter(|gl| gl.start >= a && gl.end <= b && gl.end > gl.start) {
                    let top = gl.y - gl.size * 0.9;
                    let quad = [Point::new(gl.x, top), Point::new(gl.x + gl.w, top), Point::new(gl.x + gl.w, gl.y + gl.size * 0.25), Point::new(gl.x, gl.y + gl.size * 0.25)].map(|p| view.to_screen(g.to_slide(p)));
                    painter.add(egui::Shape::convex_polygon(quad.to_vec(), col, Stroke::NONE));
                }
            }
        }
    }
    if app.show_rulers {
        draw_rulers(&painter, view, scene.size, resp.rect, app.settings.ruler_units, app.settings.ruler_origin_center);
    }
    if app.show_comments {
        comment_markers(app, ui, scene, view);
    }

    mask_bar(app, ui, scene, view);
    table_chrome(app, ui, scene, view);
    table_borders(app, ui, scene, view);
    cell_editor(app, ui, scene, view);
    media_controls(app, ui, scene, view);
    live_video_preview(app, ui, scene, view);
    cell_selection(app, ui, scene, view);

    // in-place text editing: keyboard, caret and selection
    if app.canvas.editing.is_some() {
        crate::textedit::handle_keys(app, &ctx, edit_info.as_ref().map(|(tl, _)| tl));
        if let Some((tl, g)) = &edit_info {
            crate::textedit::draw_spelling(app, &painter, tl, g, |p| view.to_screen(p));
            crate::textedit::draw_overlay(app, &painter, tl, g, |p| view.to_screen(p));
            if resp.secondary_clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    crate::textedit::open_spell_menu(app, tl, g, view.to_slide(p), p);
                }
            }
        }
        crate::textedit::spell_menu_ui(app, &ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        ctx.set_cursor_icon(egui::CursorIcon::Text);
    }

    // cursor feedback
    if let Some(p) = resp.hover_pos() {
        if handle_under(p).is_some() {
            ctx.set_cursor_icon(if ctrl { egui::CursorIcon::Alias } else { egui::CursorIcon::Crosshair });
        }
    }
}

/// Draw a drag layer (texture + the part of the slide it covers) over `slide_rect`.
fn paint_layer(painter: &egui::Painter, slide_rect: Rect, layer: &(egui::TextureHandle, Rect)) {
    let (tex, uv) = layer;
    let at = |t: Pos2| slide_rect.min + vec2(t.x * slide_rect.width(), t.y * slide_rect.height());
    painter.image(tex.id(), Rect::from_min_max(at(uv.min), at(uv.max)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

/// The same, faded (what of the selection hangs off the slide, as Keynote shows it).
fn paint_layer_faded(painter: &egui::Painter, slide_rect: Rect, layer: &(egui::TextureHandle, Rect)) {
    let (tex, uv) = layer;
    let at = |t: Pos2| slide_rect.min + vec2(t.x * slide_rect.width(), t.y * slide_rect.height());
    painter.image(tex.id(), Rect::from_min_max(at(uv.min), at(uv.max)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha(64));
}

fn app_scene_bounds(r: &mut kn_render::Renderer, scene: &SlideScene, sel: &[ObjId]) -> Option<(Point, Point)> {
    let mut lo = Point::new(f32::INFINITY, f32::INFINITY);
    let mut hi = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for d in scene.drawables.iter().filter(|d| sel.contains(&d.id)) {
        let (a, b) = r.effective_geometry(d).bounds();
        lo.x = lo.x.min(a.x);
        lo.y = lo.y.min(a.y);
        hi.x = hi.x.max(b.x);
        hi.y = hi.y.max(b.y);
    }
    lo.x.is_finite().then_some((lo, hi))
}

fn point_mode(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View, resp: &egui::Response) {
    let Some(pe) = app.point_edit.as_mut() else { return };
    let Some(d) = scene.drawables.iter().find(|d| d.id == pe.id) else { return };
    let g = d.geom;
    let to = |p: Point| view.to_screen(g.to_slide(p));
    let painter = ui.painter_at(resp.rect);
    let acc = theme::ACCENT();
    // the outline as edited
    for sp in &pe.nodes.subpaths {
        let n = sp.nodes.len();
        let segs = if sp.closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let (a, b) = (sp.nodes[i], sp.nodes[(i + 1) % n]);
            let pts: Vec<Pos2> = (0..=20)
                .map(|k| {
                    let t = k as f32 / 20.0;
                    let u = 1.0 - t;
                    let x = u * u * u * a.pt.x + 3.0 * u * u * t * a.out.x + 3.0 * u * t * t * b.inc.x + t * t * t * b.pt.x;
                    let y = u * u * u * a.pt.y + 3.0 * u * u * t * a.out.y + 3.0 * u * t * t * b.inc.y + t * t * t * b.pt.y;
                    to(Point::new(x, y))
                })
                .collect();
            painter.add(egui::Shape::line(pts, Stroke::new(1.5, acc)));
        }
    }
    // nodes and the selected node's handles
    for (si, sp) in pe.nodes.subpaths.iter().enumerate() {
        for (i, n) in sp.nodes.iter().enumerate() {
            let c = to(n.pt);
            let on = pe.selected == Some((si, i));
            if on || n.kind != kn_model::points::NodeKind::Sharp {
                for h in [n.inc, n.out] {
                    if (h.x - n.pt.x).abs() + (h.y - n.pt.y).abs() > 0.01 {
                        painter.line_segment([c, to(h)], Stroke::new(1.0, acc));
                        painter.circle_filled(to(h), 3.5, if on { acc } else { Color32::WHITE });
                    }
                }
            }
            let r = Rect::from_center_size(c, vec2(8.0, 8.0));
            painter.rect_filled(r, 1.0, if on { acc } else { Color32::WHITE });
            painter.rect_stroke(r, 1.0, Stroke::new(1.0, acc), StrokeKind::Inside);
        }
    }
    let hit = |pe: &PointEdit, p: Pos2| -> Option<(usize, usize, u8)> {
        let mut best: Option<((usize, usize, u8), f32)> = None;
        for (si, sp) in pe.nodes.subpaths.iter().enumerate() {
            for (i, n) in sp.nodes.iter().enumerate() {
                for (part, q) in [(0u8, n.pt), (1, n.inc), (2, n.out)] {
                    if part > 0 && pe.selected != Some((si, i)) && n.kind == kn_model::points::NodeKind::Sharp {
                        continue;
                    }
                    let dd = to(q).distance(p);
                    if dd < 9.0 && best.is_none_or(|b| dd < b.1) {
                        best = Some(((si, i, part), dd));
                    }
                }
            }
        }
        best.map(|b| b.0)
    };
    if resp.drag_started() || resp.clicked() {
        let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
        if let Some(p) = origin {
            match hit(pe, p) {
                Some(h) => {
                    pe.selected = Some((h.0, h.1));
                    pe.drag = if resp.drag_started() { Some(h) } else { None };
                }
                None => {
                    // clicked away: point editing ends
                    app.point_edit = None;
                    return;
                }
            }
        }
    }
    if resp.dragged() {
        if let (Some((si, i, part)), Some(p)) = (pe.drag, resp.interact_pointer_pos()) {
            let q = g.to_local(view.to_slide(p));
            if let Some(n) = pe.nodes.subpaths.get_mut(si).and_then(|s| s.nodes.get_mut(i)) {
                match part {
                    0 => {
                        let (dx, dy) = (q.x - n.pt.x, q.y - n.pt.y);
                        n.pt = q;
                        n.inc = Point::new(n.inc.x + dx, n.inc.y + dy);
                        n.out = Point::new(n.out.x + dx, n.out.y + dy);
                    }
                    1 | 2 => {
                        if part == 1 { n.inc = q } else { n.out = q }
                        // a smooth node keeps its handles in line
                        if n.kind == kn_model::points::NodeKind::Smooth {
                            let (h, o) = if part == 1 { (n.inc, &mut n.out) } else { (n.out, &mut n.inc) };
                            let len = ((o.x - n.pt.x).powi(2) + (o.y - n.pt.y).powi(2)).sqrt();
                            let (dx, dy) = (n.pt.x - h.x, n.pt.y - h.y);
                            let l = (dx * dx + dy * dy).sqrt().max(1e-3);
                            *o = Point::new(n.pt.x + dx / l * len, n.pt.y + dy / l * len);
                        } else if n.kind == kn_model::points::NodeKind::Sharp {
                            n.kind = kn_model::points::NodeKind::Bezier;
                        }
                    }
                    _ => {}
                }
            }
            ui.ctx().request_repaint();
        }
    }
    if resp.drag_stopped() && pe.drag.take().is_some() {
        let (id, nodes) = (pe.id, pe.nodes.clone());
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Edit Points");
            doc.set_editable_nodes(&mut app.hist, id, &nodes);
            doc.end(&mut app.hist);
        }
        app.bump(scene.slide);
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.point_edit = None;
    }
}

fn pen_mode(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View, resp: &egui::Response) {
    let ctx = ui.ctx().clone();
    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    if resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let sp = view.to_slide(p);
            if let Some(pts) = app.pen.as_mut() {
                // a new sample every couple of screen points
                if pts.last().is_none_or(|q| ((q.x - sp.x).powi(2) + (q.y - sp.y).powi(2)).sqrt() * view.scale > 2.5) {
                    pts.push(sp);
                }
            }
        }
    }
    let painter = ui.painter_at(resp.rect);
    if let Some(pts) = &app.pen {
        let line: Vec<Pos2> = pts.iter().map(|p| view.to_screen(*p)).collect();
        if line.len() > 1 {
            painter.add(egui::Shape::line(line, Stroke::new(app.pen_width * view.scale, app.pen_color)));
        }
    }
    if resp.drag_stopped() {
        let pts = app.pen.replace(Vec::new()).unwrap_or_default();
        if pts.len() > 1 {
            let c = app.pen_color;
            let col = kn_model::Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: 1.0 };
            let width = app.pen_width;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Draw");
                let _ = doc.insert_freehand(&mut app.hist, scene.slide, &pts, col, width);
                doc.end(&mut app.hist);
            }
            app.bump(scene.slide);
        }
    }
    // the pen bar: colour, width, Done
    let mut done = ui.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Area::new(egui::Id::unique("pen-bar")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -20.0]).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                for c in [Color32::from_rgb(0x0a, 0x84, 0xff), Color32::from_rgb(0xff, 0x45, 0x3a), Color32::from_rgb(0x30, 0xd1, 0x58), Color32::from_rgb(0xff, 0xd6, 0x0a), Color32::BLACK, Color32::WHITE] {
                    let (r, re) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 9.0, c);
                    if app.pen_color == c {
                        ui.painter().circle_stroke(r.center(), 10.5, Stroke::new(2.0, theme::ACCENT()));
                    }
                    if re.clicked() {
                        app.pen_color = c;
                    }
                }
                ui.add(egui::Slider::new(&mut app.pen_width, 1.0..=24.0).suffix(" pt"));
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
        });
    });
    if done {
        app.pen = None;
    }
}

/// Alignment-guide options from Settings > Rulers; Ctrl (Keynote: Command) while dragging
/// turns snapping off.
fn snap_opts(s: &crate::settings::Settings, view: &View, off: bool) -> crate::snap::Opts {
    crate::snap::Opts {
        center: s.guides_center && !off,
        edges: s.guides_edges && !off,
        spacing: s.guides_spacing && !off,
        sizing: s.guides_sizing && !off,
        // Keynote snaps within about 6 screen points at any zoom (R36)
        tol: 6.0 / view.scale,
    }
}

fn slide_box(scene: &SlideScene) -> crate::snap::Bx {
    crate::snap::Bx { x0: 0.0, y0: 0.0, x1: scene.size.w, y1: scene.size.h }
}

fn set_snap_guides(app: &App, scene: &SlideScene) {
    let gs = match (app.show_guides, app.doc.as_ref()) {
        (true, Some(doc)) => doc.guides(scene.slide).iter().map(|g| (usize::from(!g.vertical), g.pos)).collect(),
        _ => Vec::new(),
    };
    crate::snap::set_user_guides(gs);
}

/// Pressing in a ruler starts a new guide; pressing on a guide (with no object under the
/// pointer) moves it.
fn guide_grab(app: &App, view: &View, p: Pos2, sp: Point, scene: &SlideScene) -> Option<Drag> {
    if app.show_rulers {
        if p.y < view.rect.min.y && p.y > view.rect.min.y - 26.0 && p.x >= view.rect.min.x {
            return Some(Drag::Guide { index: None, vertical: false, pos: sp.y });
        }
        if p.x < view.rect.min.x && p.x > view.rect.min.x - 26.0 && p.y >= view.rect.min.y {
            return Some(Drag::Guide { index: None, vertical: true, pos: sp.x });
        }
    }
    if !app.show_guides {
        return None;
    }
    let gs = app.doc.as_ref()?.guides(scene.slide);
    let tol = 4.0 / view.scale;
    let i = gs.iter().position(|g| if g.vertical { (sp.x - g.pos).abs() < tol } else { (sp.y - g.pos).abs() < tol })?;
    if scene.drawables.iter().any(|d| !d.from_template && {
        let (lo, hi) = d.geom.bounds();
        sp.x >= lo.x && sp.x <= hi.x && sp.y >= lo.y && sp.y <= hi.y
    }) {
        return None;
    }
    Some(Drag::Guide { index: Some(i), vertical: gs[i].vertical, pos: gs[i].pos })
}

/// The slide's user guides (and the layout's, when shown), plus the one being dragged.
fn draw_user_guides(app: &App, painter: &egui::Painter, view: &View, scene: &SlideScene, drag: Option<&Drag>) {
    let Some(doc) = app.doc.as_ref() else { return };
    let mut gs: Vec<(kn_model::slides::Guide, Color32)> = Vec::new();
    let col = Color32::from_rgb(0xee, 0xca, 0x00); // Keynote 15.4 (R36)
    if app.show_guides {
        let moving = match drag {
            Some(Drag::Guide { index: Some(i), .. }) => Some(*i),
            _ => None,
        };
        gs.extend(doc.guides(scene.slide).into_iter().enumerate().filter(|(i, _)| Some(*i) != moving).map(|(_, g)| (g, col)));
    }
    if app.show_layout_guides {
        gs.extend(doc.layout_guides(scene.slide).into_iter().map(|g| (g, col.gamma_multiply(0.6))));
    }
    if let Some(Drag::Guide { vertical, pos, .. }) = drag {
        gs.push((kn_model::slides::Guide { vertical: *vertical, pos: *pos }, col));
    }
    for (g, c) in gs {
        let (a, b) = if g.vertical { (Point::new(g.pos, 0.0), Point::new(g.pos, scene.size.h)) } else { (Point::new(0.0, g.pos), Point::new(scene.size.w, g.pos)) };
        painter.line_segment([view.to_screen(a), view.to_screen(b)], Stroke::new(1.0, c));
    }
    if let Some(Drag::Guide { vertical, pos, .. }) = drag {
        // Keynote's position label next to the guide
        let at = if *vertical { view.to_screen(Point::new(*pos, 0.0)) + vec2(6.0, 6.0) } else { view.to_screen(Point::new(0.0, *pos)) + vec2(6.0, 6.0) };
        let galley = painter.layout_no_wrap(format!("{:.0} pt", pos), egui::FontId::proportional(12.0), Color32::WHITE);
        let r = Rect::from_min_size(at, galley.size() + vec2(12.0, 6.0));
        painter.rect_filled(r, 4.0, Color32::from_black_alpha(200));
        painter.galley(r.min + vec2(6.0, 3.0), galley, Color32::WHITE);
    }
}

/// Bounds of the slide's objects other than `skip` (the snapping targets).
fn other_boxes(r: &mut kn_render::Renderer, scene: &SlideScene, skip: &[ObjId]) -> Vec<crate::snap::Bx> {
    scene
        .drawables
        .iter()
        .filter(|d| !skip.contains(&d.id) && !d.from_template)
        .map(|d| {
            let (lo, hi) = r.effective_geometry(d).bounds();
            crate::snap::Bx::new(lo, hi)
        })
        .collect()
}

/// Which edges a resize handle moves: (left, top, right, bottom).
fn handle_edges(handle: usize) -> (bool, bool, bool, bool) {
    match handle {
        0 => (true, true, false, false),
        1 => (false, true, false, false),
        2 => (false, true, true, false),
        3 => (false, false, true, false),
        4 => (false, false, true, true),
        5 => (false, false, false, true),
        6 => (true, false, false, true),
        _ => (true, false, false, false),
    }
}

/// Keynote's guides: yellow lines; equal gaps as bars with end ticks; same sizes as arrows
/// along the matching side.
fn draw_guides(painter: &egui::Painter, view: &View, guides: &[crate::snap::Guide]) {
    use crate::snap::Guide;
    let col = Color32::from_rgb(0xee, 0xca, 0x00); // Keynote 15.4 (R36)
    let st = Stroke::new(1.0, col);
    for g in guides {
        match g {
            Guide::Line(a, b) => {
                painter.line_segment([view.to_screen(*a), view.to_screen(*b)], st);
            }
            Guide::Gap { axis, from, to, at } => {
                let (a, b) = if *axis == 0 { (Point::new(*from, *at), Point::new(*to, *at)) } else { (Point::new(*at, *from), Point::new(*at, *to)) };
                let (a, b) = (view.to_screen(a), view.to_screen(b));
                painter.line_segment([a, b], st);
                let tick = if *axis == 0 { vec2(0.0, 5.0) } else { vec2(5.0, 0.0) };
                painter.line_segment([a - tick, a + tick], st);
                painter.line_segment([b - tick, b + tick], st);
            }
            Guide::Size { axis, bx } => {
                let (a, b) = if *axis == 0 {
                    (Point::new(bx.x0, bx.y0 - 8.0 / view.scale), Point::new(bx.x1, bx.y0 - 8.0 / view.scale))
                } else {
                    (Point::new(bx.x0 - 8.0 / view.scale, bx.y0), Point::new(bx.x0 - 8.0 / view.scale, bx.y1))
                };
                let (a, b) = (view.to_screen(a), view.to_screen(b));
                painter.line_segment([a, b], st);
                let dir = (b - a).normalized() * 5.0;
                let n = vec2(-dir.y, dir.x);
                painter.line_segment([a, a + dir + n], st);
                painter.line_segment([a, a + dir - n], st);
                painter.line_segment([b, b - dir + n], st);
                painter.line_segment([b, b - dir - n], st);
            }
        }
    }
}

fn finish_drag(app: &mut App, scene: &SlideScene, drag: Drag, view: &View) {
    let slide = scene.slide;
    match drag {
        Drag::Guide { index, vertical, pos } => {
            let Some(doc) = app.doc.as_mut() else { return };
            let mut gs = doc.guides(slide);
            let len = if vertical { scene.size.w } else { scene.size.h };
            // dragged off the slide: removed (Keynote)
            let keep = (0.0..=len).contains(&pos);
            match (index, keep) {
                (Some(i), true) if i < gs.len() => gs[i].pos = pos,
                (Some(i), false) if i < gs.len() => {
                    gs.remove(i);
                }
                (None, true) => gs.push(kn_model::slides::Guide { vertical, pos }),
                _ => return,
            }
            doc.begin(&mut app.hist, "Guide");
            doc.set_guides(&mut app.hist, slide, &gs);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Drag::Move { delta, .. } => {
            if delta.0.abs() < 0.01 && delta.1.abs() < 0.01 {
                return;
            }
            let ids = app.sel.clone();
            if std::mem::take(&mut app.canvas.alt_drop) {
                let Some(doc) = app.doc.as_mut() else { return };
                let Some(clip) = doc.copy_objects(&ids) else { return };
                doc.begin(&mut app.hist, "Duplicate");
                let pasted = doc.paste_objects(&mut app.hist, slide, &clip, 0.0);
                if let Ok(new_ids) = &pasted {
                    for id in new_ids {
                        doc.move_by(&mut app.hist, *id, delta.0, delta.1);
                    }
                }
                doc.end(&mut app.hist);
                if let Ok(new_ids) = pasted {
                    app.sel = new_ids;
                }
                app.bump(slide);
                return;
            }
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Move");
            if app.mask_edit.is_some() && ids.len() == 1 && app.mask_edit == ids.first().copied() {
                // Edit Mask: the picture slides under its fixed mask
                if doc.file.obj(ids[0]).and_then(|m| m.ref_id("mask")).is_none() {
                    if let Some(g) = scene.drawables.iter().find(|d| d.id == ids[0]).map(|d| d.geom) {
                        doc.set_mask_rect(&mut app.hist, ids[0], 0.0, 0.0, g.w, g.h);
                    }
                }
                doc.move_image_in_mask(&mut app.hist, ids[0], delta.0, delta.1);
                doc.end(&mut app.hist);
                app.bump(slide);
                return;
            }
            for id in &ids {
                doc.move_by(&mut app.hist, *id, delta.0, delta.1);
            }
            // connection lines follow the objects they join
            doc.update_connections(&mut app.hist, slide, &ids);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Drag::Resize { id, cur, .. } => {
            // a masked picture: in Edit Mask the handles move the mask's edges; otherwise the
            // picture and its mask scale together (the box being the mask)
            if let Some((g, m)) = scene.drawables.iter().find(|d| d.id == id).and_then(|d| Some((d.geom, mask_box(d)?))) {
                let Some(doc) = app.doc.as_mut() else { return };
                doc.begin(&mut app.hist, if app.mask_edit == Some(id) { "Edit Mask" } else { "Resize" });
                if app.mask_edit == Some(id) {
                    doc.set_mask_rect(&mut app.hist, id, cur.x - g.x, cur.y - g.y, cur.w, cur.h);
                } else {
                    let (kx, ky) = (cur.w / m.w.max(0.01), cur.h / m.h.max(0.01));
                    let (mx, my) = (m.x - g.x, m.y - g.y);
                    let ng = Geometry { x: cur.x - mx * kx, y: cur.y - my * ky, w: g.w * kx, h: g.h * ky, ..g };
                    doc.set_frame(&mut app.hist, id, ng);
                    doc.set_mask_rect(&mut app.hist, id, mx * kx, my * ky, cur.w, cur.h);
                }
                doc.end(&mut app.hist);
                app.bump(slide);
                return;
            }
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Resize");
            if scene.drawables.iter().any(|d| d.id == id && matches!(d.kind, DrawableKind::Table(_))) {
                doc.scale_table_to(&mut app.hist, id, cur.w, cur.h);
            }
            doc.set_frame(&mut app.hist, id, cur);
            doc.update_connections(&mut app.hist, slide, &[id]);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Drag::Rotate { id, cur, .. } => {
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Rotate");
            doc.set_frame(&mut app.hist, id, cur);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Drag::Control { id, g, q } => {
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Adjust Shape");
            doc.set_shape_control(&mut app.hist, id, g.w, g.h, q);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Drag::LineEnd { id, a, b, .. } => {
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Move Line Point");
            doc.set_line_points(&mut app.hist, id, a, b);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Drag::Rubber { start, cur } => {
            let r = Rect::from_two_pos(start, cur);
            let (a, b) = (view.to_slide(r.min), view.to_slide(r.max));
            for d in scene.drawables.iter().filter(|d| !d.from_template) {
                let (lo, hi) = app.renderer.effective_geometry(d).bounds();
                if lo.x < b.x && hi.x > a.x && lo.y < b.y && hi.y > a.y && !app.sel.contains(&d.id) {
                    app.sel.push(d.id);
                }
            }
        }
    }
}

/// Keynote-style rulers along the top and left of the slide (points, labels every 100 pt).
fn draw_rulers(painter: &egui::Painter, view: &View, size: kn_model::Size, area: Rect, units: u8, origin_center: bool) {
    let band = 18.0;
    let top = Rect::from_min_max(pos2(view.rect.min.x, (view.rect.min.y - band - 4.0).max(area.min.y)), pos2(view.rect.max.x, (view.rect.min.y - 4.0).max(area.min.y + band)));
    let left = Rect::from_min_max(pos2((view.rect.min.x - band - 4.0).max(area.min.x), view.rect.min.y), pos2((view.rect.min.x - 4.0).max(area.min.x + band), view.rect.max.y));
    let bg = theme::gray(0x2a);
    painter.rect_filled(top, 2.0, bg);
    painter.rect_filled(left, 2.0, bg);
    let tick = Stroke::new(1.0, theme::TEXT_DIM());
    for (axis, len) in [(0usize, size.w), (1usize, size.h)] {
        // Settings > Rulers: points, inches, centimetres or percent of the slide
        let (unit_pt, major, minor) = match units {
            1 => (72.0, 1.0, 0.125),
            2 => (72.0 / 2.54, 1.0, 0.25),
            3 => (len / 100.0, 10.0, 2.0),
            _ => (1.0, 100.0, 10.0),
        };
        // labels closer than ~40 px get thinned to every other/fifth major tick
        let mut every = 1.0;
        while major * every * unit_pt * view.scale < 40.0 {
            every *= if every == 1.0 { 2.0 } else { 2.5 };
        }
        let origin = if origin_center { len / 2.0 } else { 0.0 };
        let minor_pt = minor * unit_pt;
        let n0 = (-origin / minor_pt).floor() as i64;
        let n1 = ((len - origin) / minor_pt).ceil() as i64;
        let per_major = (major / minor).round() as i64;
        let per_label = per_major * every as i64;
        if minor_pt * view.scale < 3.0 && n1 - n0 > 4000 {
            continue;
        }
        for n in n0..=n1 {
            let v = origin + n as f32 * minor_pt;
            if v < -0.01 || v > len + 0.01 {
                continue;
            }
            let is_major = n % per_major == 0;
            if !is_major && minor_pt * view.scale < 4.0 {
                continue;
            }
            let l = if is_major { band * 0.6 } else { band * 0.25 };
            let label = (n % per_label == 0 && n != 0).then(|| {
                let val = n as f32 * minor;
                if val.fract().abs() < 0.001 { format!("{val:.0}") } else { format!("{val}") }
            });
            if axis == 0 {
                let x = view.rect.min.x + v * view.scale;
                painter.line_segment([pos2(x, top.max.y - l), pos2(x, top.max.y)], tick);
                if let Some(t) = label {
                    painter.text(pos2(x + 2.0, top.min.y + 1.0), egui::Align2::LEFT_TOP, t, egui::FontId::proportional(9.0), theme::TEXT_DIM());
                }
            } else {
                let y = view.rect.min.y + v * view.scale;
                painter.line_segment([pos2(left.max.x - l, y), pos2(left.max.x, y)], tick);
                if let Some(t) = label {
                    painter.text(pos2(left.min.x + 1.0, y + 2.0), egui::Align2::LEFT_TOP, t, egui::FontId::proportional(9.0), theme::TEXT_DIM());
                }
            }
        }
    }
}

/// Yellow sticky-note markers for objects with comments; click to open the comment.
fn comment_markers(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    let Some(doc) = app.doc.as_ref() else { return };
    let with: Vec<(ObjId, kn_model::comments::CommentInfo)> =
        scene.drawables.iter().filter(|d| !d.from_template).filter_map(|d| doc.comment(d.id).map(|c| (d.id, c))).collect();
    for (id, c) in with {
        let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { continue };
        let (lo, _) = app.renderer.effective_geometry(d).bounds();
        let at = view.to_screen(lo) + vec2(-10.0, -10.0);
        let r = Rect::from_min_size(at, vec2(22.0, 22.0));
        let fill = c.color.map(|k| Color32::from_rgb((k.r * 255.0) as u8, (k.g * 255.0) as u8, (k.b * 255.0) as u8)).unwrap_or(Color32::from_rgb(0xff, 0xd6, 0x0a));
        let resp = ui.interact(r, ui.scope_id().with(("comment", id)), Sense::click());
        ui.painter().rect_filled(r, 3.0, fill);
        ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(90)), StrokeKind::Inside);
        for k in 0..3 {
            let y = r.min.y + 7.0 + k as f32 * 4.0;
            ui.painter().line_segment([pos2(r.min.x + 5.0, y), pos2(r.max.x - 5.0, y)], Stroke::new(1.0, Color32::from_black_alpha(110)));
        }
        if resp.on_hover_text(format!("{}: {}", c.author, c.text)).clicked() {
            app.open_comment = Some((id, c.text.clone()));
        }
    }
    // the open comment
    let Some((id, mut buf)) = app.open_comment.take() else { return };
    let Some(info) = app.doc.as_ref().and_then(|d| d.comment(id)) else { return };
    let mut open = true;
    let mut delete = false;
    egui::Window::new("Comment").collapsible(false).resizable(false).default_width(260.0).open(&mut open).show(ui.ctx(), |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(if info.author.is_empty() { "Author" } else { info.author.as_str() }).strong());
            if let Some(d) = info.date {
                ui.label(egui::RichText::new(kn_model::comments::format_date(d)).color(theme::TEXT_DIM()));
            }
        });
        ui.add(egui::TextEdit::multiline(&mut buf).desired_rows(3).desired_width(f32::INFINITY));
        if ui.button("Delete").clicked() {
            delete = true;
        }
    });
    let slide = scene.slide;
    if delete {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Delete Comment");
            doc.delete_comment(&mut app.hist, id);
            doc.end(&mut app.hist);
        }
        app.bump(slide);
        return;
    }
    if open {
        app.open_comment = Some((id, buf));
    } else if buf != info.text {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Edit Comment");
            doc.set_comment_text(&mut app.hist, info.id, &buf);
            doc.end(&mut app.hist);
        }
        app.bump(slide);
    }
}

/// The table cell Format > Table's row / column commands act on: the one being edited, or the
/// last one edited while its table is still the only selection.
pub fn table_cursor(app: &App) -> Option<(ObjId, u32, u32)> {
    if let Some((id, r, c, ..)) = &app.canvas.cell {
        return Some((*id, *r, *c));
    }
    let (id, r, c) = app.canvas.table_cursor?;
    (app.sel == [id]).then_some((id, r, c))
}

/// Letters of a column (0 -> "A", 26 -> "AA").
fn column_letters(c: u32) -> String {
    let mut s = String::new();
    let mut n = c + 1;
    while n > 0 {
        s.insert(0, char::from(b'A' + ((n - 1) % 26) as u8));
        n = (n - 1) / 26;
    }
    s
}

/// A selected table's reference tabs and buttons (Keynote 15.4, R32): a light bar of column
/// letters above it and of row numbers at its left (tinted for the selected cells, solid blue
/// for a whole selected row / column; click to select one, its chevron for its menu), a round
/// button at the corner selecting the whole table, and "||" / "=" adding columns / rows (click
/// for a count stepper).
fn table_chrome(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    // big enough to hit easily
    const BAR: f32 = 20.0;
    const GAP: f32 = 6.0;
    const BTN: f32 = 24.0;
    let [id] = app.sel[..] else {
        app.canvas.tab_menu = None;
        app.canvas.add_stepper = None;
        return;
    };
    let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { return };
    let DrawableKind::Table(t) = &d.kind else { return };
    // (Keynote keeps the tabs while a cell is being edited)
    if d.geom.angle.rem_euclid(360.0) != 0.0 {
        return;
    }
    // the tabs ride along while the table is moved, and hide while it is resized
    let moved = match &app.canvas.drag {
        Some(Drag::Move { delta, .. }) => vec2(delta.0, delta.1) * view.scale,
        Some(Drag::Resize { .. } | Drag::Rotate { .. }) => return,
        _ => vec2(0.0, 0.0),
    };
    let f = t.frame(&d.geom);
    let at = |x: f32, y: f32| view.to_screen(f.to_slide(Point::new(x, y))) + moved;
    let (w, h): (f32, f32) = (t.col_widths.iter().sum(), t.row_heights.iter().sum());
    let (tl, br) = (at(0.0, 0.0), at(w, h));
    let col_bar = Rect::from_min_max(egui::pos2(tl.x, tl.y - GAP - BAR), egui::pos2(br.x, tl.y - GAP));
    let row_bar = Rect::from_min_max(egui::pos2(tl.x - GAP - BAR, tl.y), egui::pos2(tl.x - GAP, br.y));
    // what is selected: the cursor / range, and whether it is whole rows or columns
    let sel = crate::tableops::range(app).filter(|r| r.0 == id);
    let whole_rows = sel.is_some_and(|(_, _, c0, _, c1)| c0 == 0 && c1 + 1 >= t.cols && t.cols > 1);
    let whole_cols = sel.is_some_and(|(_, r0, _, r1, _)| r0 == 0 && r1 + 1 >= t.rows && t.rows > 1);
    let bar_fill = crate::theme::tint(Color32::from_gray(0xf4));
    let bar_line = crate::theme::tint(Color32::from_gray(0xc8));
    let tint = Color32::from_rgb(0xcf, 0xe0, 0xfb);
    let blue = theme::ACCENT();
    let p = ui.painter().clone();
    p.rect(col_bar, 4.0, bar_fill, Stroke::new(1.0, bar_line), StrokeKind::Inside);
    p.rect(row_bar, 4.0, bar_fill, Stroke::new(1.0, bar_line), StrokeKind::Inside);
    let font = egui::FontId::proportional(11.5);
    let mut menu_req: Option<(bool, u32, Pos2)> = None;
    let mut select: Option<(bool, u32, bool)> = None;
    let (mut col_tabs, mut row_tabs): (Vec<Rect>, Vec<Rect>) = (Vec::new(), Vec::new());
    // column tabs
    let mut x = 0.0;
    for (c, cw) in t.col_widths.iter().enumerate() {
        let c = c as u32;
        let r = Rect::from_min_max(egui::pos2(at(x, 0.0).x, col_bar.min.y), egui::pos2(at(x + cw, 0.0).x, col_bar.max.y));
        x += cw;
        let covered = sel.is_some_and(|(_, _, c0, _, c1)| (c0..=c1).contains(&c));
        let solid = covered && whole_cols;
        col_tabs.push(r);
        let resp = ui.interact(r, ui.scope_id().with(("col-tab", id, c)), Sense::click_and_drag());
        if resp.drag_started() {
            app.canvas.tab_drag = Some((id, false, c, c, false));
        }
        let fill = if solid { blue } else if covered { tint } else if resp.hovered() { Color32::WHITE } else { Color32::TRANSPARENT };
        if fill != Color32::TRANSPARENT {
            p.rect_filled(r.shrink(1.0), 3.0, fill);
        }
        if c > 0 {
            p.vline(r.min.x, r.y_range().shrink(3.0), Stroke::new(1.0, bar_line));
        }
        crate::theme::centered(&p, r.center(), column_letters(c), font.clone(), if solid { Color32::WHITE } else { crate::theme::tint(Color32::from_gray(0x70)) });
        let chevron = Rect::from_min_max(egui::pos2(r.max.x - 20.0, r.min.y), r.max);
        if resp.hovered() {
            crate::icons::draw(&p, Rect::from_center_size(chevron.center(), vec2(11.0, 11.0)), crate::icons::Icon::ChevronDown, crate::theme::tint(Color32::from_gray(0x60)));
        }
        let on_chevron = resp.interact_pointer_pos().is_some_and(|q| chevron.contains(q));
        if resp.secondary_clicked() || (resp.clicked() && on_chevron) {
            menu_req = Some((false, c, egui::pos2(r.min.x, r.max.y + 2.0)));
        } else if resp.clicked() {
            select = Some((false, c, ui.input(|i| i.modifiers.shift)));
        }
    }
    // row tabs
    let mut y = 0.0;
    for (rr, rh) in t.row_heights.iter().enumerate() {
        let ri = rr as u32;
        let r = Rect::from_min_max(egui::pos2(row_bar.min.x, at(0.0, y).y), egui::pos2(row_bar.max.x, at(0.0, y + rh).y));
        y += rh;
        let covered = sel.is_some_and(|(_, r0, _, r1, _)| (r0..=r1).contains(&ri));
        let solid = covered && whole_rows;
        row_tabs.push(r);
        let resp = ui.interact(r, ui.scope_id().with(("row-tab", id, ri)), Sense::click_and_drag());
        if resp.drag_started() {
            // a selected row's tab moves the selected rows; any other tab selects across
            app.canvas.tab_drag = Some(match sel {
                Some((_, r0, _, r1, _)) if solid => (id, true, r0, r1, true),
                _ => (id, true, ri, ri, false),
            });
        }
        let fill = if solid { blue } else if covered { tint } else if resp.hovered() { Color32::WHITE } else { Color32::TRANSPARENT };
        if fill != Color32::TRANSPARENT {
            p.rect_filled(r.shrink(1.0), 3.0, fill);
        }
        if ri > 0 {
            p.hline(r.x_range().shrink(3.0), r.min.y, Stroke::new(1.0, bar_line));
        }
        crate::theme::centered(&p, r.center(), format!("{}", ri + 1), font.clone(), if solid { Color32::WHITE } else { crate::theme::tint(Color32::from_gray(0x70)) });
        // the row's chevron sits just right of the bar (shown while the row tab is pointed at)
        let chev = Rect::from_center_size(egui::pos2(r.max.x + 10.0, r.center().y), vec2(16.0, r.height().min(22.0)));
        let cresp = ui.interact(chev, ui.scope_id().with(("row-chev", id, ri)), Sense::click());
        if resp.hovered() || cresp.hovered() {
            p.rect_filled(chev, 3.0, Color32::WHITE);
            crate::icons::draw(&p, Rect::from_center_size(chev.center(), vec2(11.0, 11.0)), crate::icons::Icon::ChevronDown, crate::theme::tint(Color32::from_gray(0x60)));
        }
        if resp.secondary_clicked() || cresp.clicked() {
            menu_req = Some((true, ri, egui::pos2(r.max.x + 2.0, r.min.y)));
        } else if resp.clicked() {
            select = Some((true, ri, ui.input(|i| i.modifiers.shift)));
        }
    }
    // the lines between tabs: drag one to resize the row / column before it (they sit over
    // the tabs, so they win where they overlap)
    for (rows, tabs) in [(false, &col_tabs), (true, &row_tabs)] {
        for (i, r) in tabs.iter().enumerate() {
            let zone = if rows { Rect::from_x_y_ranges(r.x_range(), (r.max.y - 4.0)..=(r.max.y + 4.0)) } else { Rect::from_x_y_ranges((r.max.x - 4.0)..=(r.max.x + 4.0), r.y_range()) };
            let resp = ui.interact(zone, ui.scope_id().with(("tab-line", id, rows, i)), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(if rows { egui::CursorIcon::ResizeRow } else { egui::CursorIcon::ResizeColumn });
            }
            if resp.drag_started() {
                let size = if rows { t.row_heights[i] } else { t.col_widths[i] };
                if let Some(q) = ui.input(|inp| inp.pointer.press_origin()) {
                    app.canvas.border_drag = Some((id, rows, i as u32, size, q));
                }
            }
        }
    }
    // a drag that began on a tab: select across tabs, or move the selected rows
    if let Some((tid, rows, lo, hi, moving)) = app.canvas.tab_drag.filter(|d| d.0 == id) {
        let tabs = if rows { &row_tabs } else { &col_tabs };
        let q = ui.input(|i| i.pointer.latest_pos());
        let along = |q: Pos2| if rows { q.y } else { q.x };
        if let (Some(q), false) = (q, tabs.is_empty()) {
            let v = along(q);
            let under = tabs.iter().position(|r| v < if rows { r.max.y } else { r.max.x }).unwrap_or(tabs.len() - 1) as u32;
            if moving {
                // the gap nearest the pointer, among the body rows
                let (first, end) = (t.header_rows, t.rows.saturating_sub(t.footer_rows));
                let edges: Vec<f32> = std::iter::once(if rows { tabs[0].min.y } else { tabs[0].min.x }).chain(tabs.iter().map(|r| if rows { r.max.y } else { r.max.x })).collect();
                let before = (first..=end).min_by(|a, b| (edges[*a as usize] - v).abs().total_cmp(&(edges[*b as usize] - v).abs())).unwrap_or(first);
                let y = edges[before as usize];
                p.hline(row_bar.min.x..=br.x, y, Stroke::new(3.0, blue));
                // the rows being carried, as a ghost of their tabs at the pointer
                let carried = Rect::from_min_max(egui::pos2(row_bar.min.x, tabs[lo as usize].min.y), egui::pos2(br.x, tabs[hi as usize].max.y));
                p.rect_filled(carried.translate(vec2(0.0, v - carried.center().y)), 2.0, blue.gamma_multiply(0.18));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if ui.input(|i| i.pointer.any_released()) {
                    app.canvas.tab_drag = None;
                    if !(lo..=hi + 1).contains(&before) {
                        let res = app.doc.as_mut().map(|doc| {
                            doc.begin(&mut app.hist, if hi > lo { "Move Rows" } else { "Move Row" });
                            let r = doc.move_table_rows(&mut app.hist, tid, lo, hi, before);
                            doc.end(&mut app.hist);
                            r
                        });
                        match res {
                            Some(Ok(at)) => {
                                app.canvas.table_cursor = Some((tid, at, 0));
                                app.canvas.table_range = Some((tid, at, 0, at + hi - lo, t.cols - 1));
                                app.bump(scene.slide);
                            }
                            Some(Err(e)) => app.notify(e.to_string()),
                            None => {}
                        }
                    }
                }
            } else {
                let (a, b) = (lo.min(under), lo.max(under));
                if rows {
                    app.canvas.table_cursor = Some((tid, lo, 0));
                    app.canvas.table_range = Some((tid, a, 0, b, t.cols - 1));
                } else {
                    app.canvas.table_cursor = Some((tid, 0, lo));
                    app.canvas.table_range = Some((tid, 0, a, t.rows - 1, b));
                }
                if ui.input(|i| i.pointer.any_released()) {
                    app.canvas.tab_drag = None;
                }
            }
            ui.ctx().request_repaint();
        }
        if !ui.input(|i| i.pointer.any_down()) {
            app.canvas.tab_drag = None;
        }
    }
    if let Some((rows, i, shift)) = select {
        let (anchor_r, anchor_c) = app.canvas.table_cursor.filter(|c| c.0 == id).map(|c| (c.1, c.2)).unwrap_or((i, i));
        let (a, b) = if shift { (if rows { anchor_r } else { anchor_c }.min(i), if rows { anchor_r } else { anchor_c }.max(i)) } else { (i, i) };
        if rows {
            app.canvas.table_cursor = Some((id, if shift { anchor_r } else { i }, 0));
            app.canvas.table_range = Some((id, a, 0, b, t.cols - 1));
        } else {
            app.canvas.table_cursor = Some((id, 0, if shift { anchor_c } else { i }));
            app.canvas.table_range = Some((id, 0, a, t.rows - 1, b));
        }
    }
    if let Some((rows, i, pos)) = menu_req {
        // the menu acts on that row / column
        if rows {
            app.canvas.table_cursor = Some((id, i, 0));
            app.canvas.table_range = Some((id, i, 0, i, t.cols - 1));
        } else {
            app.canvas.table_cursor = Some((id, 0, i));
            app.canvas.table_range = Some((id, 0, i, t.rows - 1, i));
        }
        app.canvas.tab_menu = Some((id, rows, i, pos));
        app.canvas.popup_opened = ui.ctx().cumulative_pass_nr();
    }
    // corner button: the whole table; "||" and "=": add columns / rows
    let round = |p: &egui::Painter, c: Pos2, hover: bool, glyph: &str| {
        p.circle(c, BTN / 2.0, if hover { blue } else { Color32::WHITE }, Stroke::new(1.0, if hover { blue } else { bar_line }));
        // drawn, not typed: font glyphs sit off the circle's centre
        let ink = Stroke::new(1.6, if hover { Color32::WHITE } else { crate::theme::tint(Color32::from_gray(0x70)) });
        match glyph {
            "○" => {
                p.circle_stroke(c, 4.8, ink);
            }
            "||" => {
                p.vline(c.x - 2.3, (c.y - 5.0)..=(c.y + 5.0), ink);
                p.vline(c.x + 2.3, (c.y - 5.0)..=(c.y + 5.0), ink);
            }
            _ => {
                p.hline((c.x - 5.0)..=(c.x + 5.0), c.y - 2.3, ink);
                p.hline((c.x - 5.0)..=(c.x + 5.0), c.y + 2.3, ink);
            }
        }
    };
    let corner = egui::pos2(row_bar.center().x, col_bar.center().y);
    // Keynote: click it to select the whole table, drag it to move the table
    let rc = ui.interact(Rect::from_center_size(corner, vec2(BTN, BTN)), ui.scope_id().with(("tbl-all", id)), Sense::click_and_drag());
    round(&p, corner, rc.hovered() || rc.dragged(), "○");
    if rc.hovered() || rc.dragged() {
        ui.ctx().set_cursor_icon(if rc.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    if rc.clicked() {
        app.canvas.table_cursor = None;
        app.canvas.table_range = None;
    }
    if rc.drag_started() && !d.locked {
        if let Some(q) = rc.interact_pointer_pos() {
            app.canvas.table_cursor = None;
            app.canvas.table_range = None;
            app.canvas.drag = Some(Drag::Move { start: view.to_slide(q), delta: (0.0, 0.0) });
        }
    }
    if rc.dragged() {
        if let (Some(Drag::Move { start, delta }), Some(q)) = (&mut app.canvas.drag, rc.interact_pointer_pos()) {
            let sp = view.to_slide(q);
            let mut dd = (sp.x - start.x, sp.y - start.y);
            if ui.input(|i| i.modifiers.shift) {
                if dd.0.abs() > dd.1.abs() { dd.1 = 0.0 } else { dd.0 = 0.0 }
            }
            *delta = dd;
        }
        ui.ctx().request_repaint();
    }
    if rc.drag_stopped() {
        if let Some(drag @ Drag::Move { .. }) = app.canvas.drag.take() {
            finish_drag(app, scene, drag, view);
            ui.ctx().request_repaint();
        }
        return;
    }
    for (rows, c, glyph) in [(false, egui::pos2(col_bar.max.x + GAP + BTN / 2.0, col_bar.center().y), "||"), (true, egui::pos2(row_bar.center().x, row_bar.max.y + GAP + BTN / 2.0), "=")] {
        // click: a stepper for the count; drag: add (or take away) rows / columns as the
        // handle goes past each one's size (Keynote)
        let resp = ui.interact(Rect::from_center_size(c, vec2(BTN, BTN)), ui.scope_id().with(("tbl-add", id, rows)), Sense::click_and_drag());
        round(&p, c, resp.hovered() || resp.dragged(), glyph);
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if rows { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
        }
        if resp.clicked() {
            app.canvas.add_stepper = if app.canvas.add_stepper.is_some_and(|s| s.0 == id && s.1 == rows) { None } else { Some((id, rows, c + vec2(BTN, 0.0))) };
            app.canvas.popup_opened = ui.ctx().cumulative_pass_nr();
        }
        let (cur, last) = if rows { (t.rows, t.row_heights.last().copied().unwrap_or(30.0)) } else { (t.cols, t.col_widths.last().copied().unwrap_or(80.0)) };
        let step = (last * view.scale).max(8.0);
        // dragging back only takes away empty rows / columns at the end (Keynote)
        let empty = |k: u32| {
            let cell = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).is_none_or(|x| x.trim().is_empty());
            if rows { (0..t.cols).all(|c| cell(k, c)) } else { (0..t.rows).all(|r| cell(r, k)) }
        };
        let floor = (1..cur).rev().take_while(|&k| empty(k)).last().unwrap_or(cur).max(1);
        // where the drag began (egui forgets the press origin once the button is let go)
        let origin_id = ui.scope_id().with(("tbl-add-origin", id, rows));
        if resp.drag_started() {
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                ui.data_mut(|d| d.insert_temp(origin_id, o));
            }
        }
        let origin: Option<Pos2> = ui.data(|d| d.get_temp(origin_id));
        let target = |q: Pos2| {
            let d = origin.map(|o| if rows { q.y - o.y } else { q.x - o.x }).unwrap_or(0.0);
            (cur as f32 + (d / step).round()).clamp(floor as f32, 255.0) as u32
        };
        if resp.dragged() {
            if let Some(q) = resp.interact_pointer_pos() {
                let n = target(q);
                // where the table's edge would be, and the new count
                let edge = if rows { br.y + (n as f32 - cur as f32) * step } else { br.x + (n as f32 - cur as f32) * step };
                let line = if rows { [egui::pos2(tl.x, edge), egui::pos2(br.x, edge)] } else { [egui::pos2(edge, tl.y), egui::pos2(edge, br.y)] };
                p.line_segment(line, Stroke::new(2.0, blue));
                let label = format!("{} {}", n, if rows { if n == 1 { "row" } else { "rows" } } else if n == 1 { "column" } else { "columns" });
                let g = p.layout_no_wrap(label, egui::FontId::proportional(12.0), Color32::WHITE);
                let tip = q + vec2(14.0, 14.0);
                p.rect_filled(Rect::from_min_size(tip, g.size() + vec2(12.0, 6.0)), 4.0, Color32::from_black_alpha(200));
                p.galley(tip + vec2(6.0, 3.0), g, Color32::WHITE);
            }
            ui.ctx().request_repaint();
        }
        if resp.drag_stopped() {
            if let Some(q) = resp.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos())) {
                let n = target(q);
                if n != cur {
                    set_table_count(app, id, rows, cur, n, scene.slide);
                }
            }
        }
    }
    tab_menu(app, ui, t.rows, t.cols);
    add_stepper(app, ui, t.rows, t.cols, scene.slide);
}

/// A row / column tab's menu (Keynote 15.4, R32: rows have no Sort items).
fn tab_menu(app: &mut App, ui: &mut egui::Ui, rows: u32, cols: u32) {
    let Some((id, is_row, i, pos)) = app.canvas.tab_menu else { return };
    let _ = (rows, cols);
    let mut close = false;
    let r = egui::Area::new(ui.scope_id().with(("tab-menu", id))).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_min_width(210.0);
            // menu rows: flat, full width (like the other right-click menus)
            ui.style_mut().visuals.button_frame = false;
            ui.style_mut().spacing.button_padding = vec2(8.0, 3.0);
            let mut item = |ui: &mut egui::Ui, label: &str, cmd: &str| {
                let on = crate::menus::enabled(app, cmd);
                let resp = ui.add_enabled(on, egui::Button::new(label).min_size(vec2(210.0, 0.0)));
                if resp.clicked() {
                    close = true;
                    let ctx = ui.ctx().clone();
                    crate::menus::run(app, &ctx, cmd);
                }
            };
            if is_row {
                item(ui, "Add Row Above", "Table > Add Row Above");
                item(ui, "Add Row Below", "Table > Add Row Below");
                ui.separator();
                item(ui, "Delete Row", "Table > Delete Row");
                ui.separator();
                item(ui, "Fit Height to Content", "Resize Row to Fit Content");
            } else {
                item(ui, "Sort Ascending", "Sort Ascending");
                item(ui, "Sort Descending", "Sort Descending");
                ui.separator();
                item(ui, "Add Column Before", "Table > Add Column Before");
                item(ui, "Add Column After", "Table > Add Column After");
                ui.separator();
                item(ui, "Delete Column", "Table > Delete Column");
                ui.separator();
                item(ui, "Fit Width to Content", "Resize Column to Fit Content");
            }
            ui.separator();
            for l in ["Cut", "Copy", "Paste"] {
                item(ui, l, l);
            }
            if ui.add(egui::Button::new("Clear All").min_size(vec2(210.0, 0.0))).clicked() {
                crate::tableops::clear_cells(app);
                close = true;
            }
            let _ = i;
        });
    });
    let just_opened = ui.ctx().cumulative_pass_nr() <= app.canvas.popup_opened + 1;
    if close || (r.response.clicked_elsewhere() && !just_opened) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.canvas.tab_menu = None;
    }
}

/// The add-rows / add-columns count stepper.
fn add_stepper(app: &mut App, ui: &mut egui::Ui, rows: u32, cols: u32, slide: ObjId) {
    let Some((id, is_rows, pos)) = app.canvas.add_stepper else { return };
    let cur = if is_rows { rows } else { cols };
    let mut want = cur;
    let r = egui::Area::new(ui.scope_id().with(("add-stepper", id))).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(if is_rows { "Rows" } else { "Columns" });
                ui.add(egui::DragValue::new(&mut want).range(1..=if is_rows { 255 } else { 255 }));
                if ui.small_button("▲").clicked() {
                    want = cur + 1;
                }
                if ui.small_button("▼").clicked() && cur > 1 {
                    want = cur - 1;
                }
            });
        });
    });
    if want != cur {
        set_table_count(app, id, is_rows, cur, want, slide);
    }
    let just_opened = ui.ctx().cumulative_pass_nr() <= app.canvas.popup_opened + 1;
    if (r.response.clicked_elsewhere() && !just_opened) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.canvas.add_stepper = None;
    }
}

/// Give a table `want` rows / columns (from `cur`): new ones copy the last one's size, and
/// the last ones go first.
fn set_table_count(app: &mut App, id: ObjId, is_rows: bool, cur: u32, want: u32, slide: ObjId) {
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, if is_rows { "Rows" } else { "Columns" });
    let axis = if is_rows { kn_model::tablerows::Axis::Row } else { kn_model::tablerows::Axis::Column };
    let mut n = cur;
    let mut res = Ok(());
    while res.is_ok() && n < want {
        res = doc.table_insert(&mut app.hist, id, axis, n, n - 1);
        n += 1;
    }
    while res.is_ok() && n > want.max(1) {
        res = doc.table_delete(&mut app.hist, id, axis, n - 1);
        n -= 1;
    }
    doc.end(&mut app.hist);
    if let Err(e) = res {
        app.notify(e.to_string());
    }
    app.bump(slide);
}

/// The selected table's column and row borders: drag one to resize that column / row (the
/// table grows or shrinks with it), as in Keynote.
fn table_borders(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    if matches!(app.canvas.drag, Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Rotate { .. })) {
        return;
    }
    let [id] = app.sel[..] else {
        app.canvas.border_drag = None;
        return;
    };
    if app.canvas.cell.is_some() {
        return;
    }
    let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { return };
    let DrawableKind::Table(t) = &d.kind else { return };
    if d.geom.angle.rem_euclid(360.0) != 0.0 {
        return;
    }
    let f = t.frame(&d.geom);
    let at = |x: f32, y: f32| view.to_screen(f.to_slide(Point::new(x, y)));
    let (w, h): (f32, f32) = (t.col_widths.iter().sum(), t.row_heights.iter().sum());
    let mut lines: Vec<(bool, u32, Rect)> = Vec::new();
    let mut x = 0.0;
    // (row borders are added first below, so a column border wins where they cross)
    for (i, cw) in t.col_widths.iter().enumerate() {
        x += cw;
        let (a, b) = (at(x, 0.0), at(x, h));
        lines.push((false, i as u32, Rect::from_min_max(egui::pos2(a.x - 3.0, a.y), egui::pos2(b.x + 3.0, b.y))));
    }
    let mut y = 0.0;
    for (i, rh) in t.row_heights.iter().enumerate() {
        y += rh;
        let (a, b) = (at(0.0, y), at(w, y));
        lines.push((true, i as u32, Rect::from_min_max(egui::pos2(a.x, a.y - 3.0), egui::pos2(b.x, b.y + 3.0))));
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    // the table's own selection handles (corners and edge middles) win over the borders under
    // them: grabbing one resizes the whole table, not the last row / column
    if app.canvas.table_cursor.is_none() && app.canvas.table_range.is_none() {
        let (a, b) = (at(0.0, 0.0), at(w, h));
        let handles: Vec<Rect> = [a.x, (a.x + b.x) / 2.0, b.x]
            .iter()
            .flat_map(|&x| [a.y, (a.y + b.y) / 2.0, b.y].map(|y| Rect::from_center_size(egui::pos2(x, y), vec2(14.0, 14.0))))
            .collect();
        lines = lines
            .into_iter()
            .flat_map(|(rows, i, r)| {
                // cut the handle squares out of the line, along its length
                let mut pieces = vec![r];
                for hr in handles.iter().filter(|hr| hr.intersects(r)) {
                    pieces = pieces
                        .into_iter()
                        .flat_map(|p| {
                            if !hr.intersects(p) {
                                return vec![p];
                            }
                            if rows {
                                vec![Rect::from_x_y_ranges(p.min.x..=hr.min.x, p.y_range()), Rect::from_x_y_ranges(hr.max.x..=p.max.x, p.y_range())]
                            } else {
                                vec![Rect::from_x_y_ranges(p.x_range(), p.min.y..=hr.min.y), Rect::from_x_y_ranges(p.x_range(), hr.max.y..=p.max.y)]
                            }
                        })
                        .filter(|p| p.width() > 1.0 && p.height() > 1.0)
                        .collect();
                }
                pieces.into_iter().map(move |p| (rows, i, p))
            })
            .collect();
    }
    lines.sort_by_key(|l| !l.0);
    for (k, (rows, i, r)) in lines.into_iter().enumerate() {
        let resp = ui.interact(r, ui.scope_id().with(("table-border", id, rows, i, k)), egui::Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if rows { egui::CursorIcon::ResizeRow } else { egui::CursorIcon::ResizeColumn });
        }
        if resp.drag_started() {
            let size = if rows { t.row_heights[i as usize] } else { t.col_widths[i as usize] };
            if let Some(p) = pointer {
                app.canvas.border_drag = Some((id, rows, i, size, p));
            }
        }
    }
    let Some((bid, rows, i, size, start)) = app.canvas.border_drag else { return };
    if bid != id {
        app.canvas.border_drag = None;
        return;
    }
    let Some(p) = pointer.or(ui.input(|i| i.pointer.latest_pos())) else { return };
    let delta = if rows { p.y - start.y } else { p.x - start.x } / view.scale;
    let new = (size + delta).max(if rows { 12.0 } else { 20.0 });
    // the border where it would go, and the size
    let before: f32 = if rows { t.row_heights[..i as usize].iter().sum() } else { t.col_widths[..i as usize].iter().sum() };
    let painter = ui.painter();
    let (a, b) = if rows { (at(0.0, before + new), at(w, before + new)) } else { (at(before + new, 0.0), at(before + new, h)) };
    painter.line_segment([a, b], Stroke::new(2.0, theme::ACCENT()));
    let label = format!("{} {:.0} pt", if rows { "Height" } else { "Width" }, new);
    let tip = egui::pos2(p.x + 14.0, p.y + 14.0);
    let g = painter.layout_no_wrap(label, egui::FontId::proportional(12.0), Color32::WHITE);
    painter.rect_filled(Rect::from_min_size(tip, g.size() + vec2(12.0, 6.0)), 4.0, Color32::from_black_alpha(200));
    painter.galley(tip + vec2(6.0, 3.0), g, Color32::WHITE);
    ui.ctx().request_repaint();
    if ui.input(|i| i.pointer.any_released()) {
        app.canvas.border_drag = None;
        if (new - size).abs() > 0.5 {
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, if rows { "Resize Row" } else { "Resize Column" });
                doc.set_table_sizes(&mut app.hist, id, rows, &[(i, new)]);
                doc.end(&mut app.hist);
            }
            app.bump(scene.slide);
        }
    }
}

/// The selected cell or range (Keynote, R32): a thin blue outline with round handles at its
/// top-left and bottom-right corners, and a yellow dot at the middle of its bottom / right edge
/// when pointed at; dragging the dot fills the cells it covers (autofill, yellow outline).
fn cell_selection(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    if matches!(app.canvas.drag, Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Rotate { .. })) {
        return;
    }
    let Some((tid, r0, c0, r1, c1)) = crate::tableops::range(app) else {
        app.canvas.fill_drag = None;
        return;
    };
    if app.sel != [tid] || app.canvas.cell.is_some() {
        return;
    }
    let Some(d) = scene.drawables.iter().find(|d| d.id == tid) else { return };
    let DrawableKind::Table(t) = &d.kind else { return };
    let f = t.frame(&d.geom);
    let at = |x: f32, y: f32| view.to_screen(f.to_slide(Point::new(x, y)));
    let block = |r0: u32, c0: u32, r1: u32, c1: u32| {
        let (x0, y0, _, _) = cell_rect(&t.row_heights, &t.col_widths, r0, c0);
        let (x1, y1, w1, h1) = cell_rect(&t.row_heights, &t.col_widths, r1, c1);
        Rect::from_two_pos(at(x0, y0), at(x1 + w1, y1 + h1))
    };
    let r = block(r0, c0, r1, c1);
    let p = ui.painter().clone();
    let blue = theme::ACCENT();
    p.rect_stroke(r, 0.0, Stroke::new(1.5, blue), StrokeKind::Middle);
    for c in [r.left_top(), r.right_bottom()] {
        p.circle(c, 3.5, Color32::WHITE, Stroke::new(1.2, blue));
    }
    // autofill
    let yellow = Color32::from_rgb(0xf5, 0xc2, 0x11);
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let bottom = r.center_bottom();
    let right = r.right_center();
    let near = |q: Pos2, e: Pos2| (q - e).length() < 14.0;
    for (down, dot) in [(true, bottom), (false, right)] {
        let hit = Rect::from_center_size(dot, vec2(12.0, 12.0));
        let resp = ui.interact(hit, ui.scope_id().with(("autofill", tid, down)), Sense::drag());
        let show = app.canvas.fill_drag.is_some_and(|f| f.1 == down) || resp.hovered() || pointer.is_some_and(|q| near(q, dot));
        if show {
            p.circle(dot, 4.5, yellow, Stroke::new(1.0, Color32::WHITE));
            ui.ctx().set_cursor_icon(if down { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
        }
        if resp.drag_started() {
            app.canvas.fill_drag = Some((tid, down));
        }
    }
    let Some((fid, down)) = app.canvas.fill_drag else { return };
    if fid != tid {
        app.canvas.fill_drag = None;
        return;
    }
    let Some(q) = ui.input(|i| i.pointer.latest_pos()) else { return };
    // the cell under the pointer decides how far the fill goes
    let local = f.to_local(view.to_slide(q));
    let cell_at = |sizes: &[f32], v: f32| -> u32 {
        let mut acc = 0.0;
        for (i, s) in sizes.iter().enumerate() {
            acc += s;
            if v < acc {
                return i as u32;
            }
        }
        sizes.len().saturating_sub(1) as u32
    };
    let (to_r1, to_c1) = if down { (cell_at(&t.row_heights, local.y).max(r1), c1) } else { (r1, cell_at(&t.col_widths, local.x).max(c1)) };
    let target = block(r0, c0, to_r1, to_c1);
    p.rect_stroke(target, 0.0, Stroke::new(3.0, yellow), StrokeKind::Middle);
    ui.ctx().request_repaint();
    if ui.input(|i| i.pointer.any_released()) {
        app.canvas.fill_drag = None;
        if (to_r1, to_c1) != (r1, c1) {
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Autofill");
                let res = doc.autofill(&mut app.hist, tid, (r0, c0, r1, c1), (r0, c0, to_r1, to_c1));
                doc.end(&mut app.hist);
                if let Err(e) = res {
                    app.notify(e.to_string());
                }
            }
            app.canvas.table_range = Some((tid, r0, c0, to_r1, to_c1));
            app.bump(scene.slide);
        }
    }
}

/// The cell references in a formula being typed, in order: ((row, col), (row, col)) blocks.
fn formula_refs(text: &str) -> Vec<((u32, u32), (u32, u32))> {
    let parse = |w: &str| -> Option<(u32, u32)> {
        let w = w.replace('$', "");
        let split = w.find(|c: char| c.is_ascii_digit())?;
        let (l, d) = w.split_at(split);
        if l.is_empty() || l.len() > 3 || !l.chars().all(|c| c.is_ascii_alphabetic()) || !d.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let col = l.to_ascii_uppercase().bytes().fold(0u32, |a, c| a * 26 + u32::from(c - b'A' + 1)) - 1;
        Some((d.parse::<u32>().ok()?.checked_sub(1)?, col))
    };
    let mut out = Vec::new();
    let mut in_str = false;
    let mut word = String::new();
    let mut words: Vec<String> = Vec::new();
    for ch in text.chars() {
        if ch == '"' {
            in_str = !in_str;
        }
        if !in_str && (ch.is_ascii_alphanumeric() || ch == '$' || ch == ':') {
            word.push(ch);
        } else {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    for w in words {
        let (a, b) = w.split_once(':').unwrap_or((&w, &w));
        if let (Some(x), Some(y)) = (parse(a), parse(b)) {
            out.push(((x.0.min(y.0), x.1.min(y.1)), (x.0.max(y.0), x.1.max(y.1))));
        }
    }
    out
}

/// Tab / Shift-Tab from cell (r, c): right / left; past the last column, the next row at the
/// column where tabbing started (Keynote, R32).
fn tab_move(app: &mut App, id: ObjId, r: u32, c: u32, rows: u32, cols: u32, back: bool) -> (u32, u32) {
    let start = match app.canvas.tab_start {
        Some((t, row, col)) if t == id && row == r => col,
        _ => c,
    };
    let (nr, nc) = if back {
        if c > 0 { (r, c - 1) } else if r > 0 { (r - 1, cols - 1) } else { (r, c) }
    } else if c + 1 < cols {
        (r, c + 1)
    } else if r + 1 < rows {
        (r + 1, start.min(cols - 1))
    } else {
        (r, c)
    };
    app.canvas.tab_start = Some((id, nr, if nr == r { start } else { nc }));
    (nr, nc)
}

/// Keys with a table cell selected (not editing it), as in Keynote: arrows move between cells
/// (Shift extends the selection), Tab / Shift-Tab move right / left, Return edits the cell,
/// typing replaces what is in it, Escape goes back to the table. Returns whether the keys
/// went to the table.
fn table_keys(app: &mut App, ui: &egui::Ui, ctx: &egui::Context, scene: &SlideScene) -> bool {
    if app.canvas.cell.is_some() || app.canvas.editing.is_some() || ctx.egui_wants_keyboard_input() {
        return false;
    }
    let Some((id, r, c)) = table_cursor(app) else { return false };
    let Some(DrawableKind::Table(t)) = scene.drawables.iter().find(|d| d.id == id).map(|d| &d.kind) else { return false };
    let (rows, cols) = (t.rows.max(1), t.cols.max(1));
    let (shift, dr, dc, enter, esc) = ui.input(|i| {
        let (mut dr, mut dc) = (0i64, 0i64);
        if i.key_pressed(egui::Key::ArrowUp) {
            dr -= 1;
        }
        if i.key_pressed(egui::Key::ArrowDown) {
            dr += 1;
        }
        if i.key_pressed(egui::Key::ArrowLeft) {
            dc -= 1;
        }
        if i.key_pressed(egui::Key::ArrowRight) {
            dc += 1;
        }
        (i.modifiers.shift, dr, dc, i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape))
    });
    let tab = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
    let back_tab = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
    if tab || back_tab {
        let (nr, nc) = tab_move(app, id, r, c, rows, cols, back_tab);
        app.canvas.table_cursor = Some((id, nr, nc));
        app.canvas.table_range = None;
        return true;
    }
    let typed: String = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
    let clamp = |v: i64, n: u32| v.clamp(0, i64::from(n) - 1) as u32;
    if esc {
        app.canvas.table_cursor = None;
        app.canvas.table_range = None;
        return true;
    }
    if dr != 0 || dc != 0 {
        if shift && !tab && !back_tab {
            // extend from the cursor (the anchor) to the moved far corner
            let (_, a0, b0, a1, b1) = app.canvas.table_range.unwrap_or((id, r, c, r, c));
            let far_r = if a0 == r { a1 } else { a0 };
            let far_c = if b0 == c { b1 } else { b0 };
            let nr = clamp(i64::from(far_r) + dr, rows);
            let nc = clamp(i64::from(far_c) + dc, cols);
            app.canvas.table_range = Some((id, r.min(nr), c.min(nc), r.max(nr), c.max(nc)));
        } else {
            app.canvas.table_cursor = Some((id, clamp(i64::from(r) + dr, rows), clamp(i64::from(c) + dc, cols)));
            app.canvas.table_range = None;
        }
        return true;
    }
    if enter {
        let text = app.doc.as_ref().and_then(|d| d.formula_text(id, r, c)).unwrap_or_else(|| t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default());
        app.canvas.cell = Some((id, r, c, text, true));
        return true;
    }
    if !typed.is_empty() && !ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
        // typing replaces the cell's content (the keys are used up here, not typed twice)
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_))));
        app.canvas.table_range = None;
        app.canvas.cell = Some((id, r, c, typed, true));
        return true;
    }
    // arrows / Delete are the table's while a cell is selected
    true
}

/// Save what is typed in the cell editor and close it (before a command changes the table).
pub fn commit_cell(app: &mut App) {
    let grown = std::mem::take(&mut app.canvas.cell_lines) > 1;
    let Some((id, r, c, text, _)) = app.canvas.cell.take() else { return };
    if grown {
        // (the row was drawn grown while typing; drawn again from the document either way)
        if let Some(cur) = app.current_slide().map(|s| s.slide) {
            app.bump(cur);
        }
    }
    let Some(cur) = app.current_slide().map(|s| s.slide) else { return };
    let Some(scene) = app.scene(app.cur) else { return };
    let Some(DrawableKind::Table(t)) = scene.drawables.iter().find(|d| d.id == id).map(|d| &d.kind) else { return };
    let old = t.cells.get((r * t.cols) as usize + c as usize).cloned().unwrap_or_default();
    if text == old || app.doc.as_ref().and_then(|d| d.formula_text(id, r, c)).is_some_and(|f| f == text.trim()) {
        return;
    }
    let value = kn_model::tablecells::CellInput::parse(&text);
    let mut failed = None;
    if let Some(doc) = app.doc.as_mut() {
        doc.begin(&mut app.hist, "Edit Cell");
        // "=SUM(B2:B4)" and the like make a formula
        failed = if text.trim_start().starts_with('=') && text.trim().len() > 1 {
            doc.set_formula_text(&mut app.hist, id, r, c, &text).err()
        } else {
            doc.set_cell(&mut app.hist, id, r, c, &value).err()
        };
        doc.end(&mut app.hist);
    }
    if let Some(e) = failed {
        app.notify(format!("Couldn't change the cell: {e}"));
    }
    app.bump(cur);
}

/// View > Show Live Video Preview: the camera in the slide's live video objects.
fn live_video_preview(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    let lives: Vec<&kn_model::Drawable> = scene.drawables.iter().filter(|d| matches!(&d.kind, DrawableKind::Movie(m) if m.live)).collect();
    if !app.live_preview || lives.is_empty() {
        if !app.live_preview {
            app.live_cam = None;
        }
        return;
    }
    let ctx = ui.ctx().clone();
    if app.live_cam.as_ref().is_none_or(|p| p.finished()) {
        app.live_cam = Some(crate::media::Player::camera(&ctx, lives[0].id, 640, 360));
    }
    let Some(tex) = app.live_cam.as_mut().and_then(|p| p.texture(&ctx)).map(|t| t.id()) else { return };
    for d in lives {
        let corners = [(0.0, 0.0), (d.geom.w, 0.0), (0.0, d.geom.h), (d.geom.w, d.geom.h)].map(|(x, y)| view.to_screen(d.geom.to_slide(Point::new(x, y))));
        let r = Rect::from_points(&corners);
        // the camera picture fills the object, cropped to its shape
        let (cw, ch) = (640.0f32, 360.0f32);
        let k = (r.width() / cw).max(r.height() / ch);
        let (uw, uh) = ((r.width() / (cw * k)).min(1.0), (r.height() / (ch * k)).min(1.0));
        let uv = Rect::from_center_size(egui::pos2(0.5, 0.5), vec2(uw, uh));
        ui.painter().image(tex, r, uv, Color32::WHITE);
    }
    ctx.request_repaint();
}

/// A play badge on web videos (click: open the video's page), and ‹ n of m › arrows under a
/// selected image gallery to flip through its images.
fn media_controls(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    for d in &scene.drawables {
        let DrawableKind::Image(im) = &d.kind else { continue };
        if im.web_video.is_none() && im.gallery.len() < 2 {
            continue;
        }
        let corners = [(0.0, 0.0), (d.geom.w, 0.0), (0.0, d.geom.h), (d.geom.w, d.geom.h)].map(|(x, y)| view.to_screen(d.geom.to_slide(Point::new(x, y))));
        let r = Rect::from_points(&corners);
        if let Some(w) = &im.web_video {
            let start = |app: &mut App, ctx: &egui::Context| -> bool {
                let Some(stream) = crate::media::web_stream(&w.url) else { return false };
                let px = [((r.width() * ctx.pixels_per_point()) as u32 + 1) & !1, ((r.height() * ctx.pixels_per_point()) as u32 + 1) & !1];
                let spec = crate::media::Spec { path: std::path::PathBuf::from(stream), len: 0, start: 0.0, end: 0.0, volume: 1.0, looping: false, video: Some(px) };
                app.canvas.web_player = Some(crate::media::Player::start(ctx, d.id, 1, spec));
                true
            };
            // a click made while the stream was being looked up
            if app.canvas.web_waiting == Some(d.id) && start(app, ui.ctx()) {
                app.canvas.web_waiting = None;
            }
            let playing = app.canvas.web_player.as_ref().is_some_and(|p| p.drawable == d.id && !p.finished());
            if playing {
                // playing inline: the video over its poster; a click stops it
                if let Some(t) = app.canvas.web_player.as_mut().and_then(|p| p.texture(ui.ctx())).map(|t| t.id()) {
                    ui.painter().image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                let hit = ui.interact(r, ui.scope_id().with(("webvideo-playing", d.id)), egui::Sense::click());
                if hit.clicked() {
                    app.canvas.web_player = None;
                }
                ui.ctx().request_repaint();
            } else {
                // Keynote: a big white ▶ in a dark circle on the poster
                let rad = (r.width().min(r.height()) * 0.12).clamp(16.0, 48.0);
                let c = r.center();
                let p = ui.painter();
                p.circle_filled(c, rad, Color32::from_black_alpha(140));
                p.circle_stroke(c, rad, Stroke::new(2.0, Color32::WHITE));
                let s = rad * 0.45;
                p.add(egui::Shape::convex_polygon(vec![c + vec2(-s * 0.55, -s), c + vec2(s, 0.0), c + vec2(-s * 0.55, s)], Color32::WHITE, Stroke::NONE));
                let hit = ui.interact(Rect::from_center_size(c, vec2(rad, rad) * 2.0), ui.scope_id().with(("webvideo", d.id)), egui::Sense::click());
                let tip = if w.title.is_empty() { w.url.clone() } else { format!("{}\n{}", w.title, w.url) };
                if hit.on_hover_text(tip).clicked() && !start(app, ui.ctx()) {
                    app.canvas.web_waiting = Some(d.id);
                    app.notify("Loading the video…");
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                }
                if app.canvas.web_waiting == Some(d.id) {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                }
            }
        }
        if im.gallery.len() > 1 && app.sel == [d.id] {
            // Keynote: round arrows inside the frame (no wrapping round) and a dot per image
            // under the caption
            let n = im.gallery.len();
            let pos = im.gallery_shown.min(n - 1);
            let mut go: Option<usize> = None;
            let rad = 18.0;
            for (dir, show) in [(-1i32, pos > 0), (1, pos + 1 < n)] {
                if !show {
                    continue;
                }
                let c = egui::pos2(if dir < 0 { r.min.x + 16.0 + rad } else { r.max.x - 16.0 - rad }, r.center().y);
                let hit = Rect::from_center_size(c, vec2(rad * 2.0, rad * 2.0));
                let resp = ui.interact(hit, ui.scope_id().with(("gallery-arrow", d.id, dir)), egui::Sense::click());
                ui.painter().circle_filled(c, rad, Color32::from_black_alpha(if resp.hovered() { 150 } else { 110 }));
                let k = 5.0 * dir as f32;
                ui.painter().add(egui::Shape::line(vec![c + vec2(-k * 0.6, -8.0), c + vec2(k * 0.6, 0.0), c + vec2(-k * 0.6, 8.0)], Stroke::new(2.4, Color32::WHITE)));
                if resp.clicked() {
                    go = Some(if dir < 0 { pos - 1 } else { pos + 1 });
                }
            }
            let below = im.gallery_caption_id.and_then(|c| scene.drawables.iter().find(|x| x.id == c)).map(|c| view.to_screen(c.geom.to_slide(Point::new(0.0, c.geom.h))).y).unwrap_or(r.max.y);
            for k in 0..n {
                let c = egui::pos2(r.center().x + (k as f32 - (n - 1) as f32 / 2.0) * 16.0, below + 12.0);
                let dot = Rect::from_center_size(c, vec2(14.0, 14.0));
                let resp = ui.interact(dot, ui.scope_id().with(("gallery-dot", d.id, k)), egui::Sense::click());
                ui.painter().circle_filled(c, 4.0, if k == pos { theme::gray(0x70) } else { theme::gray(0xc8) });
                if resp.clicked() {
                    go = Some(k);
                }
            }
            if let Some(p) = go {
                app.gallery_pos.insert(d.id, p);
                app.bump(slide);
            }
        }
    }
}

/// Cell of a table under a slide point: (row, col, current text).
fn table_cell_at(app: &mut App, scene: &SlideScene, id: ObjId, p: Point) -> Option<(u32, u32, String)> {
    let d = scene.drawables.iter().find(|d| d.id == id)?;
    let DrawableKind::Table(t) = &d.kind else { return None };
    let l = t.frame(&d.geom).to_local(p);
    let row = cell_index(&t.row_heights, l.y)?;
    let col = cell_index(&t.col_widths, l.x)?;
    let text = t.cells.get((row * t.cols) as usize + col as usize).cloned().unwrap_or_default();
    let _ = app;
    Some((row, col, text))
}

fn cell_index(sizes: &[f32], v: f32) -> Option<u32> {
    let mut acc = 0.0;
    for (i, s) in sizes.iter().enumerate() {
        acc += s;
        if v < acc {
            return (v >= 0.0).then_some(i as u32);
        }
    }
    None
}

fn cell_rect(sizes_r: &[f32], sizes_c: &[f32], r: u32, c: u32) -> (f32, f32, f32, f32) {
    let y: f32 = sizes_r.iter().take(r as usize).sum();
    let x: f32 = sizes_c.iter().take(c as usize).sum();
    (x, y, sizes_c.get(c as usize).copied().unwrap_or(100.0), sizes_r.get(r as usize).copied().unwrap_or(40.0))
}

/// Inline editor over a table cell (double-click a cell). Enter = save + next row, Tab = next
/// column, Esc = cancel.
fn cell_editor(app: &mut App, ui: &mut egui::Ui, scene: &SlideScene, view: &View) {
    let Some((id, r, c, mut text, first)) = app.canvas.cell.take() else { return };
    let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { return };
    let DrawableKind::Table(t) = &d.kind else { return };
    app.canvas.table_cursor = Some((id, r, c));
    let (x, y, w, h) = cell_rect(&t.row_heights, &t.col_widths, r, c);
    let f = t.frame(&d.geom);
    let tl = view.to_screen(f.to_slide(Point::new(x, y)));
    let formula = text.trim_start().starts_with('=');
    let mut rect = Rect::from_min_size(tl, vec2(w, h) * view.scale);
    let mut cancel = false;
    let mut accept = false;
    if formula {
        // Keynote's formula editor: a bar over the cell (fx | the formula | ✕ ✓), and each
        // reference highlighted on the table in its own colour
        let colors = [Color32::from_rgb(0x35, 0x84, 0xe4), Color32::from_rgb(0xff, 0x78, 0x00), Color32::from_rgb(0x91, 0x41, 0xac), Color32::from_rgb(0xe0, 0x1b, 0x24), Color32::from_rgb(0x26, 0xa2, 0x69)];
        for (k, (a, b)) in formula_refs(&text).into_iter().enumerate() {
            let col = colors[k % colors.len()];
            let (x0, y0, _, _) = cell_rect(&t.row_heights, &t.col_widths, a.0, a.1);
            let (x1, y1, w1, h1) = cell_rect(&t.row_heights, &t.col_widths, b.0, b.1);
            let rr = Rect::from_two_pos(view.to_screen(f.to_slide(Point::new(x0, y0))), view.to_screen(f.to_slide(Point::new(x1 + w1, y1 + h1))));
            ui.painter().rect(rr, 2.0, col.gamma_multiply(0.15), Stroke::new(1.5, col), StrokeKind::Middle);
        }
        // one line tall whatever the cell's size, centred on the cell, at least as wide as it
        let bar_h = 28.0;
        let bar_w = rect.width().max(280.0);
        let bar = Rect::from_min_size(egui::pos2(rect.min.x - 1.0, rect.center().y - bar_h / 2.0), vec2(bar_w, bar_h));
        ui.painter().rect_filled(bar.translate(vec2(0.0, 2.0)).expand(1.0), 8.0, Color32::from_black_alpha(40));
        ui.painter().rect(bar, 7.0, Color32::WHITE, Stroke::new(1.0, crate::theme::tint(Color32::from_gray(0xb8))), StrokeKind::Outside);
        // ƒx in its own grey segment, as in Keynote
        let fx = Rect::from_min_size(bar.min, vec2(30.0, bar_h));
        ui.painter().rect_filled(fx, egui::CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 }, crate::theme::tint(Color32::from_gray(0xf2)));
        ui.painter().vline(fx.max.x, bar.y_range(), Stroke::new(1.0, crate::theme::tint(Color32::from_gray(0xd8))));
        crate::theme::centered(ui.painter(), fx.center(), "ƒx", egui::FontId::proportional(13.0), crate::theme::tint(Color32::from_gray(0x60)));
        let ok = Rect::from_center_size(bar.right_center() - vec2(15.0, 0.0), vec2(20.0, 20.0));
        let no = Rect::from_center_size(bar.right_center() - vec2(39.0, 0.0), vec2(20.0, 20.0));
        let okr = ui.interact(ok, ui.scope_id().with("formula-ok"), Sense::click());
        let nor = ui.interact(no, ui.scope_id().with("formula-cancel"), Sense::click());
        ui.painter().circle_filled(ok.center(), 9.0, Color32::from_rgb(0x2e, 0xc2, 0x7e));
        crate::theme::centered(ui.painter(), ok.center(), "✓", egui::FontId::proportional(12.0), Color32::WHITE);
        ui.painter().circle_filled(no.center(), 9.0, Color32::from_rgb(0xe0, 0x1b, 0x24));
        crate::theme::centered(ui.painter(), no.center(), "✕", egui::FontId::proportional(11.0), Color32::WHITE);
        accept = okr.clicked();
        cancel = nor.clicked();
        rect = Rect::from_min_max(egui::pos2(fx.max.x + 6.0, bar.center().y - 9.0), egui::pos2(no.min.x - 6.0, bar.center().y + 9.0));
    }
    // a plain cell is edited in place, in the cell's own size, colour and alignment, over its
    // own background (so the drawn text underneath doesn't show through)
    let style = t.cell_styles.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
    let c32 = |k: kn_model::Color| Color32::from_rgba_unmultiplied((k.r * 255.0) as u8, (k.g * 255.0) as u8, (k.b * 255.0) as u8, (k.a * 255.0) as u8);
    let font_px = if formula { 14.0 } else { (if style.text.size > 0.0 { style.text.size } else { 20.0 }) * view.scale };
    let cell_rect = rect;
    if !formula {
        let back = match &style.fill {
            kn_model::Fill::Color(k) if k.a > 0.99 => c32(*k),
            kn_model::Fill::Gradient { stops, .. } if !stops.is_empty() => c32(stops[0].1).to_opaque(),
            _ => Color32::WHITE,
        };
        ui.painter().rect_filled(cell_rect, 0.0, back);
        let [pl, pt, pr, pb] = style.padding.map(|v| v * view.scale);
        let inner = Rect::from_min_max(cell_rect.min + vec2(pl, pt), cell_rect.max - vec2(pr, pb));
        // as tall as its lines (Shift+Return starts a new one)
        let line_h = font_px * 1.25 * text.split('\n').count() as f32;
        let top = match style.valign {
            kn_model::VAlign::Top => inner.min.y,
            kn_model::VAlign::Middle => inner.center().y - line_h / 2.0,
            kn_model::VAlign::Bottom => inner.max.y - line_h,
        };
        rect = Rect::from_min_size(egui::pos2(inner.min.x, top), vec2(inner.width().max(10.0), line_h));
    }
    let halign = match style.align {
        _ if formula => egui::Align::Min,
        kn_model::Align::Center => egui::Align::Center,
        kn_model::Align::Right => egui::Align::Max,
        // Natural: numbers right, text left (Keynote's automatic alignment)
        kn_model::Align::Natural if text.trim().parse::<f64>().is_ok() => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let color = if formula || style.text.color.a <= 0.0 { Color32::BLACK } else { c32(style.text.color) };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let edit_id = egui::Id::unique(("cell-editor", id));
    // a plain cell takes several lines: Shift+Return starts a new one, Return alone enters the
    // cell (as Numbers and Keynote); a formula stays on one line
    let edit = if formula { egui::TextEdit::singleline(&mut text) } else { egui::TextEdit::multiline(&mut text).return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter)).lock_focus(false).desired_rows(1) };
    let resp = child.add_sized(rect.size(), edit.id(edit_id).font(egui::FontId::proportional(font_px)).text_color(color).align(egui::Align2([halign, egui::Align::Center])).margin(egui::Margin::ZERO).frame(egui::Frame::NONE));
    if !formula {
        ui.painter().rect_stroke(cell_rect, 0.0, Stroke::new(2.0, theme::ACCENT()), StrokeKind::Outside);
    }
    if first || (formula && app.canvas.formula_pick.is_some()) || std::mem::take(&mut app.canvas.formula_caret_end) {
        resp.request_focus();
        // (a new editor has no state yet: start one, or the first key typed can replace the text)
        let mut st = egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
        let n = text.chars().count();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
        st.store(ui.ctx(), edit_id);
    }
    // Return without Shift enters the cell (Shift+Return was a new line, taken by the editor)
    let enter = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift)));
    let (tab, esc) = ui.input(|i| (i.key_pressed(egui::Key::Tab), i.key_pressed(egui::Key::Escape)));
    // the row grows (and the slide is drawn again) as lines are added or taken away
    let lines = text.split('\n').count();
    let slide = app.current_slide().map(|s| s.slide);
    if esc || cancel {
        // drawn again from the cell's real text
        if std::mem::take(&mut app.canvas.cell_lines) > 1 {
            if let Some(s) = slide {
                app.bump(s);
            }
        }
        return;
    }
    if lines != app.canvas.cell_lines.max(1) {
        app.canvas.cell_lines = lines;
        if let Some(s) = slide {
            app.bump(s);
        }
    }
    // a formula stays open while cells are picked into it: only Return, Tab or ✓ enter it
    let enter = enter || accept;
    if formula && resp.lost_focus() && !enter && !tab {
        resp.request_focus();
        app.canvas.cell = Some((id, r, c, text, false));
        return;
    }
    if resp.lost_focus() || enter || tab {
        // saved the one way (typed formulas such as =B2+B3 become formulas)
        app.canvas.cell = Some((id, r, c, text, false));
        commit_cell(app);
        let t = match app.scene(app.cur).and_then(|sc| sc.drawables.into_iter().find(|d| d.id == id)).map(|d| d.kind) {
            Some(DrawableKind::Table(t)) => t,
            _ => return,
        };
        // move to the next cell like Keynote
        // Keynote (R32): Return keeps this cell selected; Tab moves right, and from the last
        // column to the next row at the column where tabbing started
        let _ = (&t, enter);
        app.canvas.table_cursor = Some((id, r, c));
        if tab {
            let back = ui.input(|i| i.modifiers.shift);
            let (nr, nc) = tab_move(app, id, r, c, t.rows, t.cols, back);
            app.canvas.table_cursor = Some((id, nr, nc));
        }
        app.canvas.table_range = None;
        return;
    }
    app.canvas.cell = Some((id, r, c, text, false));
}
