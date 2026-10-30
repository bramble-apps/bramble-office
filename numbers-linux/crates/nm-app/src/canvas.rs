//! The sheet canvas: Keydeck's renderer draws the sheet (tiles.rs); on top of it go Numbers'
//! table chrome (reference bars with column letters and row numbers, the table name), the cell
//! selection and the in-cell editor. Keyboard and mouse follow Numbers 15.4.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use nm_core::kn_format::ObjId;
use nm_core::kn_model::tablecells::CellInput;
use nm_core::kn_model::tablerows::{Axis, HeaderKind};
use nm_core::kn_model::{CellRange, DrawableKind, Geometry, TableData};

use crate::app::{accent, accent_light, App, InspTab, Sel};
use crate::tiles::{self, Tiles};

/// Reference bars (screen points, not zoomed).
const BAR: f32 = 16.0;
const BAR_GAP: f32 = 8.0;

pub struct CellEdit {
    pub table: ObjId,
    pub row: u32,
    pub col: u32,
    pub text: String,
    pub focus: bool,
    /// Started by typing over the cell: arrow keys then commit and move (Numbers, N2 §7).
    pub typed: bool,
    /// A double-click started it here (screen point): the caret goes there (Numbers, N16).
    pub click: Option<Pos2>,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Cells(ObjId),
    Columns(ObjId),
    Rows(ObjId),
    /// Dragging the right edge of column `index` (or the bottom edge of a row) in a
    /// reference bar: its size when the drag started and where the pointer was.
    Resize { table: ObjId, rows: bool, index: u32, start: f32, from: f32 },
    /// Moving a chart, shape or text box: where the pointer started.
    Move { id: ObjId, from: Pos2 },
    /// Dragging the autofill handle of the selected block (r0, c0, r1, c1).
    Fill { table: ObjId, src: (u32, u32, u32, u32) },
    /// Dragging the yellow autofill dot on the selection's border: the cells `to` (the source
    /// block and the cells it extends over) fill when it's let go (Apple: "hold the pointer over
    /// the border of the selection until a yellow dot appears, then drag it").
    AutoFill { table: ObjId, src: (u32, u32, u32, u32), to: (u32, u32, u32, u32) },
    /// The table's "=" (rows, kind 0), "||" (columns, 1) and corner (both, 2) handles: drag to
    /// add or remove rows (one per 20 pt) and columns (one per 98 pt), Numbers' N16.
    Grow { table: ObjId, kind: u8, from: Pos2 },
    /// Dragging the selected cells by a cell inside them moves their content (Numbers, N16):
    /// the block and the cell the drag started on.
    MoveCells { table: ObjId, src: (u32, u32, u32, u32), from: (u32, u32) },
    /// Ctrl-dragging a handle rotates the object about its centre (Numbers, N16).
    Rotate { id: ObjId, from: Pos2 },
    /// Resizing the selected object by one of its eight handles (see [`HANDLE_EDGES`]).
    ObjSize { id: ObjId, handle: usize, from: Pos2 },
    /// A user guide pulled out of a ruler (`index` None) or moved; let go over the ruler, it
    /// goes (Numbers, as Keynote).
    Guide { index: Option<usize>, vertical: bool, pos: f32 },
}

/// The edges each selection handle moves (left, top, right, bottom), in the order the handles
/// are drawn: top-left, top, top-right, left, right, bottom-left, bottom, bottom-right.
const HANDLE_EDGES: [(bool, bool, bool, bool); 8] = [
    (true, true, false, false),
    (false, true, false, false),
    (false, true, true, false),
    (true, false, false, false),
    (false, false, true, false),
    (true, false, false, true),
    (false, false, false, true),
    (false, false, true, true),
];

/// The handle points of a selected object's screen rect, in [`HANDLE_EDGES`] order.
fn handle_points(r: Rect) -> [Pos2; 8] {
    [r.left_top(), r.center_top(), r.right_top(), r.left_center(), r.right_center(), r.left_bottom(), r.center_bottom(), r.right_bottom()]
}

pub struct Canvas {
    pub zoom: f32,
    /// Sheet point at the top-left corner of the canvas; None = place on the next frame.
    origin: Option<Pos2>,
    pub(crate) tiles: Tiles,
    /// Keydeck's in-place text editing state (the ported inspector reads it; Values edits
    /// text in its own field, so it stays empty).
    pub editing: Option<crate::textedit::Editing>,
    pub edit: Option<CellEdit>,
    drag: Option<Drag>,
    /// A selected chart, shape, text box or image (tables select cells instead).
    pub obj: Option<ObjId>,
    /// Shift-click: several objects selected, `obj` first (valid while it is `obj`'s; see
    /// App::selected_objs).
    pub multi: Vec<ObjId>,
    /// Editing the text of a text box / shape: (drawable, its storage, the text).
    /// Renaming a table in place: (table, its new name). Double-click the title (Numbers).
    pub title_edit: Option<(ObjId, String)>,
    /// Size of the canvas in the last frame (screen points), and a sheet rect to bring into view.
    size: Vec2,
    pub reveal: Option<Rect>,
    /// Tab pressed this frame (with Shift?): taken out of egui's input so it moves between
    /// cells instead of between widgets.
    pub tab: Option<bool>,
    /// Where Return goes after a run of Tabs: the column the run started in (Numbers, N2 §7).
    pub return_col: Option<u32>,
    /// The cell an arrow key landed on inside a merge: leaving it again goes on from that row /
    /// column (Numbers, Mac N16: Down into D23:F23 from E22, Down again -> E24).
    merge_entry: Option<(u32, u32)>,
    /// Clicking cells while typing a formula: the text before the inserted reference, the
    /// reference text, and the cell the click / drag started on.
    pick: Option<(String, String, (u32, u32))>,
    /// A zoom step on its way (menu, Ctrl+Plus/Minus, Zoom to Selection): from zoom, to zoom,
    /// the sheet point to end at the view's centre (None: the view's top-left stays put, as
    /// Numbers' zoom steps do, Mac N21), seconds it takes, started at.
    zoom_anim: Option<(f32, f32, Option<Pos2>, f32, std::time::Instant)>,
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas { title_edit: None, multi: Vec::new(), zoom_anim: None, merge_entry: None, editing: None, zoom: 1.25, origin: None, tiles: Tiles::default(), edit: None, drag: None, obj: None, pick: None, tab: None, return_col: None, size: Vec2::new(800.0, 600.0), reveal: None }
    }
}

impl Canvas {
    /// Zoom to Selection: the largest of Numbers' zoom steps that fits `r` (sheet points),
    /// centred in the view.
    pub fn zoom_to(&mut self, r: Rect) {
        let fit = (self.size.x / r.width().max(1.0)).min(self.size.y / r.height().max(1.0)) * 0.9;
        let z = crate::chrome::ZOOMS.iter().copied().filter(|z| *z <= fit).last().unwrap_or(0.25);
        // (Mac N21: ~317 ms)
        self.zoom_anim = Some((self.zoom, z, Some(r.center()), 0.317, std::time::Instant::now()));
    }

    /// The zoom a step on its way is going to (else the zoom): repeated Ctrl+Plus steps on
    /// from there, not from part-way.
    pub fn zoom_goal(&self) -> f32 {
        self.zoom_anim.map(|a| a.1).unwrap_or(self.zoom)
    }

    /// Zoom to `z`, the view's top-left staying put, eased over ~285 ms (Mac N21).
    pub fn animate_zoom(&mut self, z: f32) {
        let z = z.clamp(0.25, 4.0);
        // (a step during a step goes on from where the first one has got to)
        self.zoom_anim = Some((self.zoom, z, None, 0.285, std::time::Instant::now()));
    }

    /// The part of the sheet in view (sheet points).
    pub fn visible(&self) -> Option<Rect> {
        self.origin.map(|o| Rect::from_min_size(o, self.size / self.zoom))
    }

    /// The sheet point in the middle of the view.
    pub fn view_center(&self) -> Option<Pos2> {
        self.origin.map(|o| o + self.size / self.zoom / 2.0)
    }

    pub fn reset(&mut self) {
        self.origin = None;
        self.edit = None;
        self.obj = None;
        self.pick = None;
        // (a sheet's view is set straight away: no zoom easing carried over)
        self.zoom_anim = None;
        // the tiles stay: each sheet keeps its own (Tiles::show_sheet); another document
        // clears them (App::load)
    }
}

/// "A", "B", … "Z", "AA", …
pub fn col_name(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// Edges of the columns and rows of a table, in sheet points from its top-left.
struct Grid {
    g: Geometry,
    xs: Vec<f32>,
    ys: Vec<f32>,
}

impl Grid {
    fn new(g: Geometry, t: &TableData) -> Grid {
        let mut xs = vec![0.0];
        for c in 0..t.cols as usize {
            xs.push(xs[c] + t.col_widths.get(c).copied().unwrap_or(98.0));
        }
        let mut ys = vec![0.0];
        for r in 0..t.rows as usize {
            ys.push(ys[r] + t.row_heights.get(r).copied().unwrap_or(20.0));
        }
        Grid { g, xs, ys }
    }
    fn w(&self) -> f32 {
        *self.xs.last().unwrap()
    }
    fn h(&self) -> f32 {
        *self.ys.last().unwrap()
    }
    fn col_at(&self, x: f32) -> Option<u32> {
        let x = x - self.g.x;
        (x >= 0.0 && x < self.w()).then(|| self.xs.partition_point(|&e| e <= x).saturating_sub(1) as u32)
    }
    fn row_at(&self, y: f32) -> Option<u32> {
        let y = y - self.g.y;
        (y >= 0.0 && y < self.h()).then(|| self.ys.partition_point(|&e| e <= y).saturating_sub(1) as u32)
    }
    /// Sheet rect of a block of cells (inclusive).
    fn block(&self, r0: u32, c0: u32, r1: u32, c1: u32) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.g.x + self.xs[c0 as usize], self.g.y + self.ys[r0 as usize]),
            Pos2::new(self.g.x + self.xs[c1 as usize + 1], self.g.y + self.ys[r1 as usize + 1]),
        )
    }
}

struct View {
    rect: Rect,
    origin: Pos2,
    zoom: f32,
}

impl View {
    fn to_screen(&self, p: Pos2) -> Pos2 {
        self.rect.min + (p - self.origin) * self.zoom
    }
    fn rect(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }
    fn to_sheet(&self, p: Pos2) -> Pos2 {
        self.origin + (p - self.rect.min) / self.zoom
    }
}

/// How far (0..1, eased) zoom `z` is on the geometric way from `z0` to `z1`.
fn zoom_progress(z0: f32, z1: f32, z: f32) -> f32 {
    if (z1 / z0).ln().abs() < 1e-6 { 1.0 } else { ((z / z0).ln() / (z1 / z0).ln()).clamp(0.0, 1.0) }
}

/// Draw the canvas and handle its mouse input; false while tiles are still being drawn.
pub fn canvas(app: &mut App, ui: &mut egui::Ui) -> bool {
    // panels drawn before the canvas (inspector, menus, toolbar) may have edited the document
    // this frame: the drawables must match the version the tiles are stamped with
    app.refresh();
    if let Some(id) = app.sheets().get(app.sheet).map(|s| s.id) {
        app.canvas.tiles.show_sheet(id);
    }
    let rect = ui.max_rect();
    let resp = ui.interact(rect, ui.scope_id().with("sheet"), Sense::click_and_drag());
    // first frame of a sheet: where the document was saved showing it, else its content's
    // top-left with Numbers' margin (40 screen points)
    if app.canvas.origin.is_none() {
        match app.saved_view() {
            Some((z, o)) => {
                app.canvas.zoom = z;
                app.canvas.origin = Some(o);
            }
            // a sheet the document has no view for opens at Settings' zoom (Numbers: 125 %)
            None => app.canvas.zoom = app.settings.default_zoom,
        }
    }
    // a zoom step on its way: ease-in-out, zooming geometrically; the top-left stays put, or
    // (Zoom to Selection) the view's centre glides to the selection's
    if let (Some((z0, z1, to, secs, t0)), Some(o)) = (app.canvas.zoom_anim, app.canvas.origin) {
        let t = (t0.elapsed().as_secs_f32() / secs).min(1.0);
        let e = egui::emath::easing::cubic_in_out(t);
        let z_was = app.canvas.zoom;
        let z = z0 * (z1 / z0).powf(e);
        app.canvas.zoom = z;
        app.canvas.origin = Some(match to {
            None => o,
            Some(end) => {
                // this frame's share of the way from where the centre is now
                let half = rect.size() / 2.0;
                let c = o + half / z_was;
                let k = if t >= 1.0 { 1.0 } else { ((e - zoom_progress(z0, z1, z_was)) / (1.0 - zoom_progress(z0, z1, z_was)).max(1e-3)).clamp(0.0, 1.0) };
                c + (end - c) * k - half / z
            }
        });
        if t >= 1.0 {
            app.canvas.zoom_anim = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
    let zoom = app.canvas.zoom;
    let origin = *app.canvas.origin.get_or_insert_with(|| {
        let (mut x, mut y) = (0f32, 0f32);
        for d in &app.drawables {
            let g = tiles::frame(d);
            x = x.min(g.x);
            y = y.min(g.y);
        }
        Pos2::new(x - 40.0 / zoom, y - 40.0 / zoom)
    });
    let mut view = View { rect, origin, zoom };
    app.canvas.size = rect.size();
    // Find (and others) asked for a part of the sheet to be shown: scroll the least needed
    if let Some(r) = app.canvas.reveal.take() {
        let vis = Rect::from_min_size(view.origin, rect.size() / zoom);
        let margin = Vec2::new(40.0, 60.0) / zoom;
        let mut o = view.origin;
        if r.min.x < vis.min.x + margin.x || r.max.x > vis.max.x - margin.x {
            o.x = r.min.x - margin.x * 2.0;
        }
        if r.min.y < vis.min.y + margin.y || r.max.y > vis.max.y - margin.y {
            o.y = r.min.y - vis.height() / 3.0;
        }
        view.origin = o;
        app.canvas.origin = Some(o);
    }

    // scrolling and pinch / ctrl+wheel zoom (about the pointer); not under a window such as
    // the Content Hub, which scrolls itself
    if (resp.hovered() || resp.contains_pointer()) && !kn_ui::pointer_over_window(ui.ctx()) {
        let (scroll, zd, hover) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
        if zd != 1.0 {
            app.canvas.zoom_anim = None;
            let at = hover.unwrap_or(rect.center());
            let before = view.to_sheet(at);
            app.canvas.zoom = (app.canvas.zoom * zd).clamp(0.25, 4.0);
            view.zoom = app.canvas.zoom;
            view.origin = before - (at - rect.min) / view.zoom;
        } else if scroll != Vec2::ZERO {
            view.origin -= scroll / view.zoom;
        }
        app.canvas.origin = Some(view.origin);
    }
    // dragging past the canvas edge scrolls that way (Numbers auto-scrolls while an object or
    // a selection is dragged out; the sheet grows with the object)
    if resp.dragged() {
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
            let out = |v: f32, lo: f32, hi: f32| if v < lo { v - lo } else if v > hi { v - hi } else { 0.0 };
            let (dx, dy) = (out(p.x, rect.min.x + 4.0, rect.max.x - 4.0), out(p.y, rect.min.y + 4.0, rect.max.y - 4.0));
            if dx != 0.0 || dy != 0.0 {
                let speed = |d: f32| d.signum() * (4.0 + d.abs() * 0.5).min(40.0);
                view.origin += Vec2::new(if dx != 0.0 { speed(dx) } else { 0.0 }, if dy != 0.0 { speed(dy) } else { 0.0 }) / view.zoom;
                app.canvas.origin = Some(view.origin);
                ui.ctx().request_repaint();
            }
        }
    }
    // the sheet can't be scrolled past its top-left (the margin it opens with), nor further
    // right / down than its content's far edge half a view in
    {
        let (mut lo, mut hi) = (Pos2::new(0.0, 0.0), Pos2::new(0.0, 0.0));
        for d in &app.drawables {
            let g = tiles::frame(d);
            lo = Pos2::new(lo.x.min(g.x), lo.y.min(g.y));
            hi = Pos2::new(hi.x.max(g.x + g.w), hi.y.max(g.y + g.h));
        }
        let vw = rect.width() / view.zoom;
        let vh = rect.height() / view.zoom;
        let min = Pos2::new(lo.x - 40.0 / view.zoom, lo.y - 40.0 / view.zoom);
        // Numbers: the furthest object's edge plus ~350 pt (right) / ~311 pt (bottom), Mac N16
        let max = Pos2::new((hi.x + 350.0 / view.zoom - vw).max(min.x), (hi.y + 311.0 / view.zoom - vh).max(min.y));
        // while dragging out past the content the sheet grows with it: no pulling back
        let max = if resp.dragged() { Pos2::new(max.x.max(view.origin.x), max.y.max(view.origin.y)) } else { max };
        let o = Pos2::new(view.origin.x.clamp(min.x, max.x), view.origin.y.clamp(min.y, max.y));
        if o != view.origin {
            view.origin = o;
            app.canvas.origin = Some(o);
        }
    }

    if let Some(id) = app.sheets().get(app.sheet).map(|s| s.id) {
        app.session_views.insert(id, (view.zoom, view.origin));
    }
    if std::env::var_os("NM_DEBUG").is_some() {
        eprintln!("view origin {:?} zoom {} rect {:?} version {}", view.origin, view.zoom, rect, app.version);
    }
    let pending = match app.wb.as_ref() {
        Some(wb) => app.canvas.tiles.paint(ui, rect, view.origin, view.zoom, &wb.doc, &app.drawables, app.version),
        None => false,
    };
    if std::env::var_os("NM_DEBUG").is_some() {
        eprintln!("pending {pending}");
    }
    if pending {
        ui.ctx().request_repaint();
    }

    let painter = ui.painter_at(rect);
    // frozen header rows stay at the top while their table scrolls under them
    let mut pinned: Vec<(ObjId, Rect, f32)> = Vec::new();
    for d in &app.drawables {
        let DrawableKind::Table(t) = &d.kind else { continue };
        let frozen = app.doc().and_then(|doc| doc.file.obj(d.id)?.ref_id("tableModel")).and_then(|m| app.doc()?.file.obj(m)?.bool("header_rows_frozen")).unwrap_or(false);
        let g = t.frame(&d.geom);
        let hh: f32 = t.row_heights.iter().take(t.header_rows as usize).sum();
        let top = view.to_screen(Pos2::new(g.x, g.y));
        let bottom = view.to_screen(Pos2::new(g.x, g.y + g.h - hh)).y;
        if !frozen || t.header_rows == 0 || top.y >= rect.min.y || bottom <= rect.min.y + hh * view.zoom {
            continue;
        }
        let Some(wb) = app.wb.as_ref() else { continue };
        if let Some((tex, size)) = app.canvas.tiles.header_strip(ui, &wb.doc, d, view.zoom, app.version) {
            let r = Rect::from_min_size(Pos2::new(top.x - 1.0 * view.zoom, rect.min.y - 1.0 * view.zoom), size * view.zoom);
            painter.image(tex, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            painter.line_segment([Pos2::new(top.x, rect.min.y + hh * view.zoom + 1.0), Pos2::new(top.x + g.w * view.zoom, rect.min.y + hh * view.zoom + 1.0)], Stroke::new(1.0, Color32::from_black_alpha(40)));
            pinned.push((d.id, Rect::from_min_size(Pos2::new(top.x, rect.min.y), egui::vec2(g.w * view.zoom, hh * view.zoom)), g.y));
        }
    }
    // frozen header columns stay at the left while their table scrolls sideways under them
    for d in &app.drawables {
        let DrawableKind::Table(t) = &d.kind else { continue };
        let frozen = app.doc().and_then(|doc| doc.file.obj(d.id)?.ref_id("tableModel")).and_then(|m| app.doc()?.file.obj(m)?.bool("header_columns_frozen")).unwrap_or(false);
        let g = t.frame(&d.geom);
        let hw: f32 = t.col_widths.iter().take(t.header_cols as usize).sum();
        let left = view.to_screen(Pos2::new(g.x, g.y));
        let right = view.to_screen(Pos2::new(g.x + g.w, g.y)).x;
        if !frozen || t.header_cols == 0 || left.x >= rect.min.x || right <= rect.min.x + hw * view.zoom {
            continue;
        }
        // the rows in view, widened to blocks of 32 so scrolling reuses the strip
        let top = view.to_sheet(rect.min).y - g.y;
        let bottom = view.to_sheet(rect.max).y - g.y;
        let mut acc = 0.0;
        let (mut first, mut last) = (0u32, t.rows.saturating_sub(1));
        for (i, hgt) in t.row_heights.iter().enumerate() {
            if acc + hgt < top {
                first = i as u32 + 1;
            }
            if acc <= bottom {
                last = i as u32;
            }
            acc += hgt;
        }
        let mut r0 = (first / 32 * 32).min(t.rows.saturating_sub(1));
        let mut r1 = ((last / 32 + 1) * 32).min(t.rows.saturating_sub(1)).max(r0);
        // a merged header cell the block cuts through comes in whole (its text centred in all of it)
        loop {
            let grow = t.merges.iter().filter(|m| m.col < t.header_cols && m.row <= r1 && m.row + m.rows - 1 >= r0).fold((r0, r1), |(a, b), m| (a.min(m.row), b.max(m.row + m.rows - 1)));
            if grow == (r0, r1) {
                break;
            }
            (r0, r1) = (grow.0, grow.1.min(t.rows.saturating_sub(1)));
        }
        let y0: f32 = t.row_heights.iter().take(r0 as usize).sum();
        let Some(wb) = app.wb.as_ref() else { continue };
        if let Some((tex, size)) = app.canvas.tiles.header_col_strip(ui, &wb.doc, d, view.zoom, app.version, r0, r1) {
            let p = Pos2::new(rect.min.x - 1.0 * view.zoom, view.to_screen(Pos2::new(0.0, g.y + y0)).y - 1.0 * view.zoom);
            painter.image(tex, Rect::from_min_size(p, size * view.zoom), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let x = rect.min.x + hw * view.zoom + 1.0;
            painter.line_segment([Pos2::new(x, left.y.max(rect.min.y)), Pos2::new(x, view.to_screen(Pos2::new(0.0, g.y + g.h)).y)], Stroke::new(1.0, Color32::from_black_alpha(40)));
        }
    }
    let grids: Vec<(ObjId, Grid, bool, String)> = app
        .drawables
        .iter()
        .filter_map(|d| match &d.kind {
            DrawableKind::Table(t) => {
                let shown = app.doc().is_some_and(|doc| doc.table_name_shown(d.id));
                Some((d.id, Grid::new(d.geom, t), shown, t.name.clone()))
            }
            _ => None,
        })
        .collect();

    // table names above the tables (and where they are, for double-clicking to rename)
    let mut title_rects: Vec<(ObjId, Rect)> = Vec::new();
    for (id, grid, shown, name) in &grids {
        if *shown {
            let p = view.to_screen(Pos2::new(grid.g.x + grid.w() / 2.0, grid.g.y - 6.0));
            let font = egui::FontId::proportional(14.0 * view.zoom);
            let galley = ui.painter().layout_no_wrap(if name.is_empty() { " ".into() } else { name.clone() }, font.clone(), Color32::BLACK);
            let r = Rect::from_center_size(p - Vec2::new(0.0, galley.size().y / 2.0), galley.size() + Vec2::new(16.0, 4.0));
            title_rects.push((*id, r));
            if app.canvas.title_edit.as_ref().is_some_and(|t| t.0 == *id) {
                title_editor(app, ui, r, view.zoom);
            } else {
                painter.text(p, egui::Align2::CENTER_BOTTOM, name, font, Color32::BLACK);
            }
        }
    }

    // commented cells: Numbers' yellow corner marker; the comment shows on hover
    let hover = ui.input(|i| i.pointer.hover_pos());
    let mut hover_comment: Option<String> = None;
    for (id, grid, _, _) in &grids {
        let Some(cm) = app.comments.get(id) else { continue };
        for ((mr, c), (text, author)) in cm {
            let r = app.vrow(*id, *mr);
            if (r as usize) + 1 >= grid.ys.len() || (*c as usize) + 1 >= grid.xs.len() {
                continue;
            }
            let cell = view.rect(grid.block(r, *c, r, *c));
            let k = 7.0f32.min(cell.height() * 0.4);
            // your own comments in your Author Color (View > Author Color), others' in yellow
            let rgb = if !author.is_empty() && *author == app.settings.author { crate::settings::AUTHOR_COLORS[app.settings.author_color as usize % 10].1 } else { 0xf5c518 };
            painter.add(egui::Shape::convex_polygon(vec![cell.right_top(), cell.right_top() + Vec2::new(0.0, k), cell.right_top() - Vec2::new(k, 0.0)], Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8), Stroke::NONE));
            if hover.is_some_and(|p| cell.contains(p)) {
                hover_comment = Some(if author.is_empty() { text.clone() } else { format!("{text}\n— {author}") });
            }
        }
    }
    if let (Some(text), Some(p)) = (hover_comment, hover) {
        let galley = painter.layout(text, egui::FontId::proportional(12.0), Color32::from_gray(0x20), 260.0);
        let r = Rect::from_min_size(p + Vec2::new(14.0, 14.0), galley.size() + Vec2::splat(12.0));
        painter.rect(r, 4.0, Color32::from_rgb(0xff, 0xf6, 0xc8), Stroke::new(1.0, Color32::from_rgb(0xd8, 0xb4, 0x30)), egui::StrokeKind::Inside);
        painter.galley(r.min + Vec2::splat(6.0), galley, Color32::BLACK);
    }

    // selection and reference bars of the selected table
    let mut bar_hits: Option<(ObjId, Rect, Rect)> = None;
    let mut fill_handle: Option<(ObjId, Pos2, (u32, u32, u32, u32))> = None;
    // the selection's border, for the yellow autofill dot: (table, its screen rect, the block)
    let mut fill_border: Option<(ObjId, Rect, (u32, u32, u32, u32))> = None;
    let mut table_handle: Option<(ObjId, Pos2)> = None;
    let mut grow_handles: Option<(ObjId, [Pos2; 3])> = None;
    if let Some(sel) = app.sel {
        if let Some((_, grid, shown, _)) = grids.iter().find(|g| g.0 == sel.table) {
            let (r0, c0, r1, c1) = app.with_merges(sel.table, sel.block());
            let t = app.table(sel.table).map(|(_, t)| t);
            let rows = t.map(|t| t.rows).unwrap_or(0);
            let cols = t.map(|t| t.cols).unwrap_or(0);
            let top = view.rect(grid.block(0, 0, 0, 0)).min.y - if *shown { 26.0 * view.zoom } else { 0.0 };
            let left = view.to_screen(Pos2::new(grid.g.x, grid.g.y)).x;
            let colbar = Rect::from_min_max(Pos2::new(left, top - BAR_GAP - BAR), Pos2::new(view.to_screen(Pos2::new(grid.g.x + grid.w(), 0.0)).x, top - BAR_GAP));
            let rowbar = Rect::from_min_max(
                Pos2::new(left - BAR_GAP - BAR - 6.0, view.to_screen(Pos2::new(0.0, grid.g.y)).y),
                Pos2::new(left - BAR_GAP, view.to_screen(Pos2::new(0.0, grid.g.y + grid.h())).y),
            );
            let bg = Color32::from_gray(0xf7);
            let line = Stroke::new(1.0, Color32::from_gray(0xd4));
            painter.rect(colbar, 3.0, bg, line, egui::StrokeKind::Inside);
            painter.rect(rowbar, 3.0, bg, line, egui::StrokeKind::Inside);
            let whole_cols = r0 == 0 && r1 + 1 == rows;
            let whole_rows = c0 == 0 && c1 + 1 == cols;
            for c in 0..cols {
                let x0 = view.to_screen(Pos2::new(grid.g.x + grid.xs[c as usize], 0.0)).x;
                let x1 = view.to_screen(Pos2::new(grid.g.x + grid.xs[c as usize + 1], 0.0)).x;
                if x1 < rect.min.x || x0 > rect.max.x {
                    continue;
                }
                let r = Rect::from_min_max(Pos2::new(x0, colbar.min.y), Pos2::new(x1, colbar.max.y));
                if c >= c0 && c <= c1 {
                    painter.rect_filled(r.shrink(1.0), 0.0, if whole_cols { accent() } else { accent_light() });
                }
                if c > 0 {
                    painter.line_segment([Pos2::new(x0, colbar.min.y), Pos2::new(x0, colbar.max.y)], line);
                }
                let fg = if whole_cols && c >= c0 && c <= c1 { Color32::WHITE } else { Color32::from_gray(0x40) };
                if x1 - x0 > 10.0 {
                    painter.text(r.center(), egui::Align2::CENTER_CENTER, col_name(c), egui::FontId::proportional(11.0), fg);
                }
            }
            let model_rows: Vec<Option<u32>> = (0..rows).map(|r| app.mrow(sel.table, r)).collect();
            for r in 0..rows {
                let y0 = view.to_screen(Pos2::new(0.0, grid.g.y + grid.ys[r as usize])).y;
                let y1 = view.to_screen(Pos2::new(0.0, grid.g.y + grid.ys[r as usize + 1])).y;
                if y1 < rect.min.y || y0 > rect.max.y {
                    continue;
                }
                let rr = Rect::from_min_max(Pos2::new(rowbar.min.x, y0), Pos2::new(rowbar.max.x, y1));
                if r >= r0 && r <= r1 {
                    painter.rect_filled(rr.shrink(1.0), 0.0, if whole_rows { accent() } else { accent_light() });
                }
                if r > 0 {
                    painter.line_segment([Pos2::new(rowbar.min.x, y0), Pos2::new(rowbar.max.x, y0)], line);
                }
                let fg = if whole_rows && r >= r0 && r <= r1 { Color32::WHITE } else { Color32::from_gray(0x40) };
                if let (true, Some(m)) = (y1 - y0 > 8.0, model_rows[r as usize]) {
                    painter.text(rr.center(), egui::Align2::CENTER_CENTER, (m + 1).to_string(), egui::FontId::proportional(11.0), fg);
                }
            }
            bar_hits = Some((sel.table, colbar, rowbar));
            // the circle where the bars meet moves the whole table (Numbers' table handle)
            let corner = Pos2::new(rowbar.center().x, colbar.center().y);
            painter.circle(corner, 8.0, Color32::WHITE, Stroke::new(1.5, accent()));
            painter.circle_stroke(corner, 3.5, Stroke::new(1.5, accent()));
            table_handle = Some((sel.table, corner));
            // "=" under the row bar, "||" right of the column bar, and the corner circle
            {
                let tb = view.rect(grid.block(0, 0, rows.saturating_sub(1), cols.saturating_sub(1)));
                let hs = [Pos2::new(rowbar.center().x, tb.max.y + 14.0), Pos2::new(tb.max.x + 14.0, colbar.center().y), Pos2::new(tb.max.x + 14.0, tb.max.y + 14.0)];
                for (k, h) in hs.iter().enumerate() {
                    painter.circle(*h, 8.0, Color32::WHITE, Stroke::new(1.2, accent()));
                    let st = Stroke::new(1.2, accent());
                    match k {
                        0 => {
                            painter.hline(h.x - 3.5..=h.x + 3.5, h.y - 1.5, st);
                            painter.hline(h.x - 3.5..=h.x + 3.5, h.y + 1.5, st);
                        }
                        1 => {
                            painter.vline(h.x - 1.5, h.y - 3.5..=h.y + 3.5, st);
                            painter.vline(h.x + 1.5, h.y - 3.5..=h.y + 3.5, st);
                        }
                        _ => {
                            painter.line_segment([Pos2::new(h.x - 3.0, h.y + 3.0), Pos2::new(h.x + 3.0, h.y + 3.0)], st);
                            painter.line_segment([Pos2::new(h.x + 3.0, h.y - 3.0), Pos2::new(h.x + 3.0, h.y + 3.0)], st);
                        }
                    }
                }
                grow_handles = Some((sel.table, hs));
            }
            // the selected block: tint plus a green outline; the cursor cell gets its own outline
            if r1 < rows && c1 < cols {
                let br = view.rect(grid.block(r0, c0, r1, c1));
                // the cursor's cell (a whole merge when it's in one); a single merged cell
                // selected looks like any one cell
                let (cr, cc) = sel.cursor;
                let cur = app.with_merges(sel.table, (cr, cc, cr, cc));
                let single = cur == (r0, c0, r1, c1);
                if !single {
                    let a = accent();
                    painter.rect_filled(br, 0.0, Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 28));
                }
                // Numbers: a 1 pt ring just outside the gridlines
                painter.rect_stroke(br.expand(1.0), 0.0, Stroke::new(1.5, accent()), egui::StrokeKind::Middle);
                if !single {
                    painter.rect_stroke(view.rect(grid.block(cur.0, cur.1, cur.2, cur.3)), 0.0, Stroke::new(1.0, accent()), egui::StrokeKind::Inside);
                }
                // autofill handle (not while categories rearrange the rows)
                if !app.categorized(sel.table) {
                    painter.circle(br.right_bottom(), 4.0, Color32::WHITE, Stroke::new(1.5, accent()));
                    fill_handle = Some((sel.table, br.right_bottom(), (r0, c0, r1, c1)));
                    if app.canvas.edit.is_none() {
                        fill_border = Some((sel.table, br, (r0, c0, r1, c1)));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------ guides and rulers
    let sheet_id = app.sheets().get(app.sheet).map(|s| s.id);
    let user_guides: Vec<nm_core::kn_model::slides::Guide> = match (app.show_guides, sheet_id, app.wb.as_ref()) {
        (true, Some(s), Some(w)) => w.doc.guides(s),
        _ => Vec::new(),
    };
    // objects snap to them while moving
    kn_ui::snap::set_user_guides(user_guides.iter().map(|g| (usize::from(!g.vertical), g.pos)).collect());
    {
        let moving = match app.canvas.drag {
            Some(Drag::Guide { index: Some(i), .. }) => Some(i),
            _ => None,
        };
        let col = Color32::from_rgb(0xee, 0xca, 0x00);
        let mut lines: Vec<(bool, f32)> = user_guides.iter().enumerate().filter(|(i, _)| Some(*i) != moving).map(|(_, g)| (g.vertical, g.pos)).collect();
        if let Some(Drag::Guide { vertical, pos, .. }) = app.canvas.drag {
            lines.push((vertical, pos));
        }
        for (vertical, pos) in lines {
            if vertical {
                let x = view.to_screen(Pos2::new(pos, 0.0)).x;
                painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], Stroke::new(1.0, col));
            } else {
                let y = view.to_screen(Pos2::new(0.0, pos)).y;
                painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], Stroke::new(1.0, col));
            }
        }
        if let Some(Drag::Guide { vertical, pos, .. }) = app.canvas.drag {
            if let Some(p) = resp.interact_pointer_pos() {
                tooltip(&painter, p + Vec2::new(12.0, 12.0), &format!("{} {:.0} pt", if vertical { "x" } else { "y" }, pos));
            }
        }
    }
    if app.show_rulers {
        draw_rulers(&painter, &view, rect, app.settings.ruler_units);
    }

    // ------------------------------------------------------------------ media in place
    {
        let items: Vec<(ObjId, Rect)> = app
            .drawables
            .iter()
            .filter(|d| match &d.kind {
                nm_core::kn_model::DrawableKind::Movie(_) => true,
                nm_core::kn_model::DrawableKind::Image(im) => im.web_video.is_some() || im.gallery.len() > 1,
                _ => false,
            })
            .map(|d| {
                let g = tiles::frame(d);
                (d.id, view.rect(Rect::from_min_size(Pos2::new(g.x, g.y), Vec2::new(g.w, g.h))))
            })
            .filter(|(_, r)| r.intersects(rect))
            .collect();
        if !items.is_empty() {
            crate::media::controls(app, ui, &items);
        }
    }

    // ------------------------------------------------------------------ Draw with Pen
    if app.pen.is_some() {
        pen_mode(app, ui, &view, &resp);
        return !pending;
    }

    // ------------------------------------------------------------------ mouse
    let pointer = resp.interact_pointer_pos();
    let hit_cell = |p: Pos2| -> Option<(ObjId, u32, u32)> {
        let s = view.to_sheet(p);
        // a pinned header row: the header cell under it
        if let Some((id, r, top)) = pinned.iter().find(|(_, r, _)| r.contains(p)) {
            let g = &grids.iter().find(|g| g.0 == *id)?.1;
            let y = top + (p.y - r.min.y) / view.zoom;
            return Some((*id, g.row_at(y)?, g.col_at(s.x)?));
        }
        grids.iter().rev().find_map(|(id, g, _, _)| Some((*id, g.row_at(s.y)?, g.col_at(s.x)?)))
    };
    let shift = ui.input(|i| i.modifiers.shift);
    // a column / row edge in a reference bar under the pointer: (table, rows?, index, size, pointer coordinate)
    let edge_at = |p: Pos2| -> Option<(ObjId, bool, u32, f32, f32)> {
        let (id, cb, rb) = bar_hits?;
        let (_, g, _, _) = grids.iter().find(|g| g.0 == id)?;
        if cb.expand(2.0).contains(p) {
            (1..g.xs.len()).find(|&i| (view.to_screen(Pos2::new(g.g.x + g.xs[i], 0.0)).x - p.x).abs() <= 4.0).map(|i| (id, false, i as u32 - 1, g.xs[i] - g.xs[i - 1], p.x))
        } else if rb.expand(2.0).contains(p) {
            (1..g.ys.len()).find(|&i| (view.to_screen(Pos2::new(0.0, g.g.y + g.ys[i])).y - p.y).abs() <= 3.0).map(|i| (id, true, i as u32 - 1, g.ys[i] - g.ys[i - 1], p.y))
        } else {
            None
        }
    };
    if let Some(p) = resp.hover_pos() {
        match edge_at(p) {
            Some((_, false, ..)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn),
            Some((_, true, ..)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeRow),
            None => {}
        }
    }
    // the top-most thing under the pointer that isn't a table
    let frames: Vec<(ObjId, Geometry, bool)> = app.drawables.iter().map(|d| (d.id, tiles::frame(d), matches!(d.kind, DrawableKind::Table(_)))).collect();
    let hit_object = |p: Pos2| -> Option<ObjId> {
        let s = view.to_sheet(p);
        let (id, _, table) = frames.iter().rev().find(|(_, g, _)| s.x >= g.x && s.x <= g.x + g.w.max(8.0) && s.y >= g.y && s.y <= g.y + g.h.max(8.0))?;
        (!table).then_some(*id)
    };
    // the selected object's handles (not tables: they resize by their rows and columns)
    let obj_handles: Option<(ObjId, [Pos2; 8])> = app.canvas.obj.filter(|o| app.table(*o).is_none() && app.points.id != Some(*o)).and_then(|o| app.drawables.iter().find(|d| d.id == o)).map(|d| {
        let g = tiles::frame(d);
        (d.id, handle_points(view.rect(Rect::from_min_size(Pos2::new(g.x, g.y), Vec2::new(g.w.max(8.0), g.h.max(8.0))))))
    });
    if let (Some((_, hs)), Some(p)) = (obj_handles, pointer) {
        if let Some(k) = hs.iter().position(|h| h.distance(p) <= 7.0) {
            ui.ctx().set_cursor_icon(match k {
                0 | 7 => egui::CursorIcon::ResizeNwSe,
                2 | 5 => egui::CursorIcon::ResizeNeSw,
                1 | 6 => egui::CursorIcon::ResizeVertical,
                _ => egui::CursorIcon::ResizeHorizontal,
            });
        }
    }
    // a shape's points being edited take the pointer near them (before its resize handles)
    if crate::points::canvas(app, ui, &resp, &|q| view.to_screen(q), view.zoom) {
        return !pending;
    }
    // the yellow autofill dot: on the selection's border where the pointer is (not at its
    // corner circle, which extends the selection)
    let dot_at = |p: Pos2| -> Option<Pos2> {
        let (_, r, _) = fill_border?;
        if p.distance(r.right_bottom()) <= 9.0 || !r.expand(5.0).contains(p) || r.shrink(5.0).contains(p) {
            return None;
        }
        // the nearest point of the border
        let q = Pos2::new(p.x.clamp(r.min.x, r.max.x), p.y.clamp(r.min.y, r.max.y));
        let d = [(q.x - r.min.x).abs(), (r.max.x - q.x).abs(), (q.y - r.min.y).abs(), (r.max.y - q.y).abs()];
        let k = (0..4).min_by(|a, b| d[*a].total_cmp(&d[*b]))?;
        Some(match k {
            0 => Pos2::new(r.min.x, q.y),
            1 => Pos2::new(r.max.x, q.y),
            2 => Pos2::new(q.x, r.min.y),
            _ => Pos2::new(q.x, r.max.y),
        })
    };
    if app.canvas.drag.is_none() {
        if let Some(d) = resp.hover_pos().and_then(dot_at) {
            painter.circle(d, 5.5, Color32::from_rgb(0xf5, 0xc5, 0x18), Stroke::new(1.5, Color32::WHITE));
            // a two-headed arrow across the edge: up / down on the top and bottom, left / right on the sides
            let across_rows = fill_border.is_some_and(|(_, r, _)| d.y == r.min.y || d.y == r.max.y);
            ui.ctx().set_cursor_icon(if across_rows { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
        }
    }
    if resp.drag_started() {
        // where the button went down: the pointer has moved past the drag threshold by now
        if let Some(p) = ui.input(|i| i.pointer.press_origin()).or(pointer) {
            let on_handle = obj_handles.and_then(|(id, hs)| hs.iter().position(|h| h.distance(p) <= 7.0).map(|k| (id, k)));
            // a ruler: a new guide; a guide (away from the selection's handles): move it
            let sp = view.to_sheet(p);
            let ruler = RULER_BAND;
            let grab = if app.show_rulers && p.y < rect.min.y + ruler && p.x > rect.min.x + ruler {
                Some(Drag::Guide { index: None, vertical: false, pos: sp.y })
            } else if app.show_rulers && p.x < rect.min.x + ruler && p.y > rect.min.y + ruler {
                Some(Drag::Guide { index: None, vertical: true, pos: sp.x })
            } else if on_handle.is_none() {
                let tol = 4.0 / view.zoom;
                user_guides
                    .iter()
                    .position(|g| if g.vertical { (sp.x - g.pos).abs() < tol } else { (sp.y - g.pos).abs() < tol })
                    .map(|i| Drag::Guide { index: Some(i), vertical: user_guides[i].vertical, pos: user_guides[i].pos })
            } else {
                None
            };
            let on_grow = grow_handles.and_then(|(t, hs)| hs.iter().position(|h| h.distance(p) <= 9.0).map(|k| (t, k as u8)));
            if let Some(g) = grab {
                commit(app);
                app.canvas.drag = Some(g);
            } else if let Some((table, kind)) = on_grow {
                commit(app);
                app.canvas.drag = Some(Drag::Grow { table, kind, from: p });
            } else if let Some((id, handle)) = on_handle {
                commit(app);
                app.canvas.drag = Some(if ui.input(|i| i.modifiers.command) { Drag::Rotate { id, from: p } } else { Drag::ObjSize { id, handle, from: p } });
            } else if let Some((id, _)) = table_handle.filter(|(_, h)| h.distance(p) <= 11.0) {
                commit(app);
                app.canvas.drag = Some(Drag::Move { id, from: p });
            } else if let (Some(_), Some((table, _, src))) = (dot_at(p), fill_border) {
                commit(app);
                app.canvas.drag = Some(Drag::AutoFill { table, src, to: src });
            } else if let Some((table, _, src)) = fill_handle.filter(|(_, h, _)| h.distance(p) <= 7.0) {
                commit(app);
                app.canvas.drag = Some(Drag::Fill { table, src });
            } else if let Some((table, rows, index, start, from)) = edge_at(p) {
                commit(app);
                app.canvas.drag = Some(Drag::Resize { table, rows, index, start, from });
            } else if let Some(id) = hit_object(p).filter(|id| app.canvas.obj == Some(*id)) {
                app.canvas.drag = Some(Drag::Move { id, from: p });
            }
        }
    }
    // typing a formula: a click (or drag) on a cell of the same table puts its reference in
    let formula_target = app.canvas.edit.as_ref().filter(|e| e.text.trim_start().starts_with('=')).map(|e| (e.table, e.row, e.col));
    let mut picked = false;
    if let (Some((t, er, ec)), true) = (formula_target, resp.drag_started() || resp.clicked()) {
        if let Some((id, r, c)) = ui.input(|i| i.pointer.press_origin()).or(pointer).and_then(hit_cell) {
            if let (true, Some(mr)) = (id == t && (r, c) != (er, ec), app.mrow(id, r)) {
                let e = app.canvas.edit.as_mut().unwrap();
                // a new click right after a picked reference replaces it
                let base = match &app.canvas.pick {
                    Some((base, ins, _)) if e.text == format!("{base}{ins}") => base.clone(),
                    _ => e.text.clone(),
                };
                let ins = format!("{}{}", col_name(c), mr + 1);
                e.text = format!("{base}{ins}");
                e.focus = true;
                app.canvas.pick = Some((base, ins, (r, c)));
                picked = true;
            }
        }
    }
    let drag_cell = pointer.and_then(hit_cell).map(|(id, r, c)| (id, r, c, app.mrow(id, r).unwrap_or(r)));
    let from_model = app.canvas.pick.as_ref().zip(app.canvas.edit.as_ref()).map(|((_, _, from), e)| app.mrow(e.table, from.0).unwrap_or(from.0));
    if let (Some((base, _, from)), Some(e), true) = (app.canvas.pick.clone(), app.canvas.edit.as_mut(), resp.dragged() && !picked) {
        if let Some((id, _, c, tr)) = drag_cell {
            if id == e.table {
                let fr = from_model.unwrap_or(from.0);
                let (r0, c0, r1, c1) = (fr.min(tr), from.1.min(c), fr.max(tr), from.1.max(c));
                let ins = if (r0, c0) == (r1, c1) { format!("{}{}", col_name(c0), r0 + 1) } else { format!("{}{}:{}{}", col_name(c0), r0 + 1, col_name(c1), r1 + 1) };
                if e.text != format!("{base}{ins}") {
                    e.text = format!("{base}{ins}");
                    app.canvas.pick = Some((base, ins, from));
                }
            }
        }
    }
    let picking = picked || (app.canvas.pick.is_some() && app.canvas.edit.is_some() && (resp.dragged() || resp.drag_stopped()));
    // a click (no drag) on the corner circle selects the whole table (Numbers)
    if resp.clicked() && !resp.dragged() {
        if let Some((id, _)) = table_handle.filter(|(_, h)| ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| h.distance(p) <= 11.0)) {
            if let Some((_, t)) = app.table(id) {
                app.sel = Some(Sel { table: id, anchor: (0, 0), cursor: (t.rows.saturating_sub(1), t.cols.saturating_sub(1)) });
                app.insp_tab = InspTab::Table;
            }
            return !pending;
        }
    }
    let special_drag = picking || matches!(app.canvas.drag, Some(Drag::Resize { .. } | Drag::Move { .. } | Drag::Fill { .. } | Drag::AutoFill { .. } | Drag::ObjSize { .. } | Drag::Grow { .. } | Drag::MoveCells { .. } | Drag::Rotate { .. } | Drag::Guide { .. }));
    // double-clicking a table's title renames it there (Numbers)
    if resp.double_clicked()
        && let Some(p) = pointer
        && let Some((id, _)) = title_rects.iter().find(|(_, r)| r.contains(p))
    {
        commit(app);
        let name = app.table(*id).map(|(_, t)| t.name.clone()).unwrap_or_default();
        app.canvas.title_edit = Some((*id, name));
        return !pending;
    }
    // text being edited: a press inside moves the caret (a drag selects, Shift extends, a
    // double-click selects the word's line); outside, editing ends
    let mut text_hit = false;
    if let Some(id) = app.canvas.editing.as_ref().map(|e| e.id) {
        if let (Some((tl, g)), Some(p)) = (crate::textedit::layout_of(app, id), ui.input(|i| i.pointer.press_origin()).or(pointer)) {
            let sp = view.to_sheet(p);
            let pt = nm_core::kn_model::Point::new(sp.x, sp.y);
            if crate::textedit::contains(&g, pt) {
                if resp.secondary_clicked() {
                    crate::textedit::open_spell_menu(app, &tl, &g, pt, p);
                    text_hit = true;
                } else if resp.drag_started() || resp.clicked() || resp.double_clicked() {
                    let shift = ui.input(|i| i.modifiers.shift);
                    crate::textedit::point_to(app, &tl, &g, pt, shift);
                    text_hit = true;
                    // a click on linked text: its link popover (Numbers)
                    if resp.clicked() && !shift && crate::links::link_at_caret(app).is_some() {
                        crate::links::open(app, "Webpage");
                        if let Some(le) = app.link_edit.as_mut() {
                            le.at = Some(p + egui::vec2(-20.0, 16.0));
                        }
                    }
                } else if resp.dragged() {
                    if let Some(cur) = pointer {
                        let c = view.to_sheet(cur);
                        crate::textedit::point_to(app, &tl, &g, nm_core::kn_model::Point::new(c.x, c.y), true);
                    }
                    text_hit = true;
                }
            } else if resp.clicked() || resp.drag_started() {
                crate::textedit::commit(app);
            }
        }
    }
    if !special_drag && !text_hit && (resp.drag_started() || resp.clicked() || resp.double_clicked()) {
        if let Some(p) = ui.input(|i| i.pointer.press_origin()).or(pointer).or(resp.hover_pos()) {
            let bar = bar_hits.and_then(|(id, cb, rb)| {
                let (_, g, _, _) = grids.iter().find(|g| g.0 == id)?;
                let s = view.to_sheet(p);
                if cb.contains(p) {
                    g.col_at(s.x).map(|c| (id, Drag::Columns(id), c))
                } else if rb.contains(p) {
                    g.row_at(s.y).map(|r| (id, Drag::Rows(id), r))
                } else {
                    None
                }
            });
            if let Some((id, kind, i)) = bar {
                commit(app);
                let (rows, cols) = app.table(id).map(|(_, t)| (t.rows, t.cols)).unwrap_or((1, 1));
                let prev = app.sel.filter(|s| s.table == id && shift);
                app.sel = Some(match kind {
                    Drag::Columns(_) => Sel { table: id, anchor: (0, prev.map(|s| s.anchor.1).unwrap_or(i)), cursor: (rows - 1, i) },
                    _ => Sel { table: id, anchor: (prev.map(|s| s.anchor.0).unwrap_or(i), 0), cursor: (i, cols - 1) },
                });
                app.canvas.drag = Some(kind);
            } else if let Some(id) = hit_object(p).filter(|id| app.canvas.editing.as_ref().is_none_or(|t| t.id != *id)) {
                commit(app);
                app.sel = None;
                // Shift-click adds the object to the selection, or takes it out (Numbers)
                match app.canvas.obj {
                    Some(first) if shift && resp.clicked() && first != id => {
                        if app.canvas.multi.first() != Some(&first) {
                            app.canvas.multi = vec![first];
                        }
                        match app.canvas.multi.iter().position(|x| *x == id) {
                            Some(i) => {
                                app.canvas.multi.remove(i);
                            }
                            None => app.canvas.multi.push(id),
                        }
                    }
                    // pressing on an object already selected keeps the others (to drag them all)
                    Some(_) if app.selected_objs().contains(&id) => {}
                    _ => app.canvas.multi.clear(),
                }
                if !(shift && resp.clicked()) && !app.selected_objs().contains(&id) {
                    app.canvas.obj = Some(id);
                } else if app.canvas.obj.is_none() {
                    app.canvas.obj = Some(id);
                }
                // double-click: edit its text in place, the caret where it was clicked
                if resp.double_clicked() {
                    let sp = view.to_sheet(p);
                    if crate::textedit::begin(app, id, Some(nm_core::kn_model::Point::new(sp.x, sp.y))) {
                        app.canvas.drag = None;
                    }
                }
                app.insp_tab = InspTab::Arrange;
                if resp.drag_started() {
                    app.canvas.drag = Some(Drag::Move { id, from: p });
                }
            } else if let Some((id, r, c, src)) = hit_cell(p).filter(|_| resp.drag_started() && !shift && app.canvas.edit.is_none()).and_then(|(id, r, c)| {
                // a drag from inside the selection moves the selected cells
                let sel = app.sel.filter(|s| s.table == id)?;
                let b = app.with_merges(id, sel.block());
                (r >= b.0 && r <= b.2 && c >= b.1 && c <= b.3).then_some((id, r, c, b))
            }) {
                app.canvas.drag = Some(Drag::MoveCells { table: id, src, from: (r, c) });
            } else if let Some((id, r, c)) = hit_cell(p) {
                app.canvas.obj = None;
                // a click anywhere in a merged cell is a click on the merge
                let (r, c) = app.merge_origin(id, r, c);
                let editing_here = app.canvas.edit.as_ref().is_some_and(|e| e.table == id && e.row == r && e.col == c);
                if !editing_here {
                    commit(app);
                    app.canvas.return_col = None;
                    // a click on the cell that is already the selection starts editing it (Numbers)
                    let again = resp.clicked() && !shift && app.sel == Some(Sel::cell(id, r, c));
                    match app.sel {
                        Some(s) if shift && s.table == id => app.sel = Some(Sel { cursor: (r, c), ..s }),
                        _ => app.sel = Some(Sel::cell(id, r, c)),
                    }
                    if app.insp_tab == InspTab::Arrange {
                        app.insp_tab = InspTab::Cell;
                    }
                    app.canvas.drag = Some(Drag::Cells(id));
                    if resp.double_clicked() || again {
                        start_edit(app, None);
                        // a double-click puts the caret where it was (a click on the selected
                        // cell, at the end)
                        if resp.double_clicked() {
                            if let Some(e) = app.canvas.edit.as_mut() {
                                e.click = Some(p);
                            }
                        }
                    } else if resp.clicked() && !shift {
                        click_control(app, id, r, c, view.to_sheet(p).x - grids.iter().find(|g| g.0 == id).map(|g| g.1.g.x + g.1.xs[c as usize]).unwrap_or(0.0));
                    }
                }
            } else {
                commit(app);
                app.sel = None;
                app.canvas.obj = None;
            }
        }
    }
    // live feedback while resizing or moving; the edit happens on release
    match (app.canvas.drag, resp.interact_pointer_pos()) {
        (Some(Drag::Resize { table, rows, index, start, from }), Some(p)) => {
            if let Some((_, g, _, _)) = grids.iter().find(|g| g.0 == table) {
                let size = (start + ((if rows { p.y } else { p.x }) - from) / view.zoom).max(8.0);
                let guide = Stroke::new(1.5, accent());
                if rows {
                    let y = view.to_screen(Pos2::new(0.0, g.g.y + g.ys[index as usize] + size)).y;
                    painter.line_segment([Pos2::new(view.to_screen(Pos2::new(g.g.x, 0.0)).x - 40.0, y), Pos2::new(view.to_screen(Pos2::new(g.g.x + g.w(), 0.0)).x, y)], guide);
                    painter.text(Pos2::new(p.x + 12.0, y - 4.0), egui::Align2::LEFT_BOTTOM, format!("Height: {size:.0} pt"), egui::FontId::proportional(11.0), Color32::from_gray(0x30));
                } else {
                    let x = view.to_screen(Pos2::new(g.g.x + g.xs[index as usize] + size, 0.0)).x;
                    painter.line_segment([Pos2::new(x, view.to_screen(Pos2::new(0.0, g.g.y)).y - 40.0), Pos2::new(x, view.to_screen(Pos2::new(0.0, g.g.y + g.h())).y)], guide);
                    painter.text(Pos2::new(x + 6.0, p.y - 12.0), egui::Align2::LEFT_BOTTOM, format!("Width: {size:.0} pt"), egui::FontId::proportional(11.0), Color32::from_gray(0x30));
                }
                if resp.drag_stopped() {
                    let r = app.edit(if rows { "Row Height" } else { "Column Width" }, |d, h| d.set_table_sizes(h, table, rows, &[(index, size)]));
                    if r == Some(false) {
                        app.notify("Couldn't change the size.");
                    }
                }
            }
        }
        (Some(Drag::Guide { index, vertical, .. }), Some(p)) => {
            let s = view.to_sheet(p);
            app.canvas.drag = Some(Drag::Guide { index, vertical, pos: if vertical { s.x } else { s.y } });
            ui.ctx().set_cursor_icon(if vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
        }
        (Some(Drag::AutoFill { table, src, .. }), Some(p)) => {
            // the cells the pointer drags over, in one direction from the source block: down
            // or up (whole columns of it), right or left (whole rows of it)
            if let Some((_, g, _, _)) = grids.iter().find(|g| g.0 == table) {
                let s = view.to_sheet(p);
                let last_r = g.ys.len() as u32 - 2;
                let last_c = g.xs.len() as u32 - 2;
                let r = g.row_at(s.y).unwrap_or(if s.y < g.g.y { 0 } else { last_r });
                let c = g.col_at(s.x).unwrap_or(if s.x < g.g.x { 0 } else { last_c });
                let (r0, c0, r1, c1) = src;
                // the way it's furthest past the block wins
                let down = r.saturating_sub(r1) as i64;
                let up = r0.saturating_sub(r) as i64;
                let right = c.saturating_sub(c1) as i64;
                let left = c0.saturating_sub(c) as i64;
                let best = down.max(up).max(right).max(left);
                let to = if best == 0 {
                    src
                } else if best == down {
                    (r0, c0, r, c1)
                } else if best == up {
                    (r, c0, r1, c1)
                } else if best == right {
                    (r0, c0, r1, c)
                } else {
                    (r0, c, r1, c1)
                };
                app.canvas.drag = Some(Drag::AutoFill { table, src, to });
                let sideways = to.1 != src.1 || to.3 != src.3;
                ui.ctx().set_cursor_icon(if sideways { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
                let tr = view.rect(g.block(to.0, to.1, to.2, to.3));
                painter.rect_stroke(tr.expand(1.0), 0.0, Stroke::new(2.0, Color32::from_rgb(0xf5, 0xc5, 0x18)), egui::StrokeKind::Middle);
            }
        }
        (Some(Drag::Fill { table, src }), Some(p)) => {
            // the selection's corner circle extends the selection (Numbers 15.4, Mac N16;
            // autofill is Table > Autofill Cells, Ctrl+\)
            if let Some((_, g, _, _)) = grids.iter().find(|g| g.0 == table) {
                let s = view.to_sheet(p);
                let last_r = g.ys.len() as u32 - 2;
                let last_c = g.xs.len() as u32 - 2;
                let r = g.row_at(s.y).unwrap_or(if s.y < g.g.y { 0 } else { last_r });
                let c = g.col_at(s.x).unwrap_or(if s.x < g.g.x { 0 } else { last_c });
                app.sel = Some(Sel { table, anchor: (src.0, src.1), cursor: (r, c) });
            }
        }
        (Some(Drag::Move { id, from }), Some(p)) => {
            if let Some(d) = app.drawables.iter().find(|d| d.id == id) {
                let mut delta = (p - from) / view.zoom;
                let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                if shift {
                    // along the nearest of 0°, 45° and 90° (Keynote / Numbers)
                    let len = delta.length();
                    let a = (delta.y.atan2(delta.x) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
                    let along = delta.x * a.cos() + delta.y * a.sin();
                    if len > 0.0 {
                        delta = Vec2::new(along * a.cos(), along * a.sin());
                    }
                }
                let g = tiles::frame(d);
                // alignment guides: edges and centres of the other objects, equal spacing and
                // sizes (Keydeck's snapping); Ctrl moves freely
                let bx = |g: nm_core::kn_model::Geometry| kn_ui::snap::Bx { x0: g.x, y0: g.y, x1: g.x + g.w, y1: g.y + g.h };
                let others: Vec<kn_ui::snap::Bx> = app.drawables.iter().filter(|o| o.id != id).map(|o| bx(tiles::frame(o))).collect();
                let st = &app.settings;
                let opts = kn_ui::snap::Opts { center: !ctrl && st.guides_center, edges: !ctrl && st.guides_edges, spacing: !ctrl && st.guides_spacing, sizing: !ctrl && st.guides_sizing, tol: 6.0 / view.zoom };
                let page = kn_ui::snap::Bx { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
                let (d2, guides) = kn_ui::snap::snap_move(bx(g), &others, page, (delta.x, delta.y), &opts);
                delta = Vec2::new(d2.0, d2.1);
                draw_guides(&painter, &view, &guides);
                if app.settings.show_size_while_moving {
                    tooltip(&painter, p, &format!("x: {:.0} pt  y: {:.0} pt", g.x + delta.x, g.y + delta.y));
                }
                let r = view.rect(Rect::from_min_size(Pos2::new(g.x + delta.x, g.y + delta.y), Vec2::new(g.w.max(8.0), g.h.max(8.0))));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(0x80)), egui::StrokeKind::Middle);
                if resp.drag_stopped() && delta.length() > 0.0 {
                    // Option-drag leaves the original and moves a copy (Numbers, Mac N15)
                    if ui.input(|i| i.modifiers.alt) {
                        app.duplicate_moved(id, delta.x, delta.y);
                    } else if app.mask_edit == Some(id) {
                        // editing its mask: the picture moves under the mask
                        crate::mask::move_under_mask(app, id, delta.x, delta.y);
                    } else {
                        // every selected object moves with the one dragged
                        let mut ids = app.selected_objs();
                        if !ids.contains(&id) {
                            ids = vec![id];
                        }
                        let all: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
                        app.edit("Move", |doc, h| {
                            for i in &ids {
                                doc.move_by(h, *i, delta.x, delta.y);
                            }
                            // connection lines follow the objects they join
                            doc.update_connections_in(h, &all, &ids);
                        });
                    }
                }
            }
        }
        (Some(Drag::MoveCells { table, src, from }), Some(p)) => {
            if let Some((_, g, _, _)) = grids.iter().find(|g| g.0 == table) {
                let s = view.to_sheet(p);
                let (rows, cols) = (g.ys.len() as i64 - 1, g.xs.len() as i64 - 1);
                let r = g.row_at(s.y).map(i64::from).unwrap_or(from.0 as i64);
                let c = g.col_at(s.x).map(i64::from).unwrap_or(from.1 as i64);
                let (h, w) = (src.2 - src.0, src.3 - src.1);
                let r0 = (src.0 as i64 + r - from.0 as i64).clamp(0, rows - 1 - h as i64) as u32;
                let c0 = (src.1 as i64 + c - from.1 as i64).clamp(0, cols - 1 - w as i64) as u32;
                let to = (r0, c0, r0 + h, c0 + w);
                // a see-through ghost of the cells, with a soft shadow, follows the pointer; the
                // green outline snaps to where they'll go (Numbers, Mac N21)
                let from_block = g.block(src.0, src.1, src.2, src.3);
                if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                    let ghost = view.rect(from_block).translate(p - o);
                    painter.add(egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(60) }.as_shape(ghost, 2.0));
                    painter.rect_filled(ghost, 0.0, Color32::from_white_alpha(150));
                    app.canvas.tiles.paint_region(&painter, from_block, ghost, Color32::from_white_alpha(190));
                }
                painter.rect_stroke(view.rect(g.block(to.0, to.1, to.2, to.3)), 0.0, Stroke::new(2.0, accent()), egui::StrokeKind::Middle);
                if resp.drag_stopped() && to != src {
                    move_cells(app, table, src, (to.0, to.1));
                }
            }
        }
        (Some(Drag::Grow { table, kind, from }), Some(p)) => {
            let d = (p - from) / view.zoom;
            let dr = if kind != 1 { (d.y / 20.0).round() as i32 } else { 0 };
            let dc = if kind != 0 { (d.x / 98.0).round() as i32 } else { 0 };
            let sign = |n: i32| if n > 0 { format!("+{n}") } else { n.to_string() };
            let label = match kind {
                0 => format!("{} rows", sign(dr)),
                1 => format!("{} columns", sign(dc)),
                _ => format!("{} rows  {} columns", sign(dr), sign(dc)),
            };
            tooltip(&painter, p, &label);
            if resp.drag_stopped() && (dr != 0 || dc != 0) {
                grow_table(app, table, dr, dc);
            }
        }
        (Some(Drag::Rotate { id, from }), Some(p)) => {
            if let Some(d) = app.drawables.iter().find(|d| d.id == id) {
                let g = tiles::frame(d);
                let c = view.to_screen(Pos2::new(g.x + g.w / 2.0, g.y + g.h / 2.0));
                let a0 = (from - c).angle();
                let a1 = (p - c).angle();
                let mut deg = (d.geom.angle - (a1 - a0).to_degrees()).rem_euclid(360.0);
                // Shift: steps of 45°
                if ui.input(|i| i.modifiers.shift) {
                    deg = (deg / 45.0).round() * 45.0 % 360.0;
                }
                painter.line_segment([c, p], Stroke::new(1.0, accent()));
                tooltip(&painter, p, &format!("{deg:.1}°"));
                if resp.drag_stopped() {
                    let mut ng = d.geom;
                    ng.angle = deg;
                    app.edit("Rotate", |doc, h| doc.set_frame(h, id, ng));
                }
            }
        }
        (Some(Drag::ObjSize { id, handle, from }), Some(p)) => {
            if let Some(d) = app.drawables.iter().find(|d| d.id == id) {
                let g = tiles::frame(d);
                let delta = (p - from) / view.zoom;
                let (l, t, r, b) = HANDLE_EDGES[handle];
                let mut bx = kn_ui::snap::Bx { x0: g.x + if l { delta.x } else { 0.0 }, y0: g.y + if t { delta.y } else { 0.0 }, x1: g.x + g.w + if r { delta.x } else { 0.0 }, y1: g.y + g.h + if b { delta.y } else { 0.0 } };
                let (shift, ctrl, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.command, i.modifiers.alt));
                // Option resizes from the centre: the opposite edges move the other way
                if alt {
                    if l { bx.x1 = g.x + g.w - delta.x } else if r { bx.x0 = g.x - delta.x }
                    if t { bx.y1 = g.y + g.h - delta.y } else if b { bx.y0 = g.y - delta.y }
                }
                // Shift on a corner keeps the proportions (the larger change wins)
                if shift && (l || r) && (t || b) && g.w > 0.0 && g.h > 0.0 {
                    let k = ((bx.x1 - bx.x0) / g.w).abs().max(((bx.y1 - bx.y0) / g.h).abs());
                    let (w, h) = (g.w * k, g.h * k);
                    if l { bx.x0 = bx.x1 - w } else { bx.x1 = bx.x0 + w }
                    if t { bx.y0 = bx.y1 - h } else { bx.y1 = bx.y0 + h }
                }
                let others: Vec<kn_ui::snap::Bx> = app.drawables.iter().filter(|o| o.id != id).map(|o| { let f = tiles::frame(o); kn_ui::snap::Bx { x0: f.x, y0: f.y, x1: f.x + f.w, y1: f.y + f.h } }).collect();
                let st = &app.settings;
                let opts = kn_ui::snap::Opts { center: !ctrl && st.guides_center, edges: !ctrl && st.guides_edges, spacing: false, sizing: !ctrl && st.guides_sizing, tol: 6.0 / view.zoom };
                let page = kn_ui::snap::Bx { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
                let (bx, guides) = if shift { (bx, Vec::new()) } else { kn_ui::snap::snap_resize(bx, (l, t, r, b), &others, page, &opts) };
                draw_guides(&painter, &view, &guides);
                let (x0, x1) = (bx.x0.min(bx.x1), bx.x0.max(bx.x1));
                let (y0, y1) = (bx.y0.min(bx.y1), bx.y0.max(bx.y1));
                let rr = view.rect(Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1)));
                painter.rect_stroke(rr, 0.0, Stroke::new(1.0, accent()), egui::StrokeKind::Middle);
                // the size while resizing (Keynote / Numbers show it by the pointer)
                if app.settings.show_size_while_moving {
                    tooltip(&painter, p, &format!("w: {:.0} pt  h: {:.0} pt", x1 - x0, y1 - y0));
                }
                if resp.drag_stopped() && delta.length() > 0.0 {
                    let mut ng = d.geom;
                    ng.x = x0;
                    ng.y = y0;
                    ng.w = (x1 - x0).max(4.0);
                    ng.h = (y1 - y0).max(4.0);
                    // a size given by hand stops a text box fitting its text that way
                    if l || r {
                        ng.auto_width = false;
                    }
                    if t || b {
                        ng.auto_height = false;
                    }
                    let all: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
                    app.edit("Resize", |doc, h| {
                        doc.set_frame(h, id, ng);
                        doc.update_connections_in(h, &all, &[id]);
                    });
                }
            }
        }
        _ => {}
    }
    // Edit Mask: the whole picture outlined, the mask in the accent colour, and the bar below
    if let Some(id) = app.mask_edit {
        match (crate::mask::mask_box(app, id), app.drawables.iter().find(|d| d.id == id).map(|d| d.geom)) {
            (Some(m), Some(g)) => {
                let full = view.rect(Rect::from_min_size(Pos2::new(g.x, g.y), Vec2::new(g.w, g.h)));
                painter.rect_stroke(full, 0.0, Stroke::new(1.0, Color32::from_gray(0x80)), egui::StrokeKind::Middle);
                let mr = view.rect(Rect::from_min_size(Pos2::new(m.x, m.y), Vec2::new(m.w, m.h)));
                painter.rect_stroke(mr, 0.0, Stroke::new(2.0, accent()), egui::StrokeKind::Middle);
                crate::mask::mask_bar(app, ui.ctx(), mr.center_bottom() + Vec2::new(0.0, 16.0));
            }
            _ => app.mask_edit = None,
        }
    }
    // the other objects of a several-object selection: outlined
    for id in app.selected_objs().into_iter().skip(1) {
        if let Some(d) = app.drawables.iter().find(|d| d.id == id) {
            let g = tiles::frame(d);
            let r = view.rect(Rect::from_min_size(Pos2::new(g.x, g.y), Vec2::new(g.w.max(8.0), g.h.max(8.0))));
            painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent()), egui::StrokeKind::Middle);
        }
    }
    // selection handles of the selected object (not while its points are being edited)
    if let Some(d) = app.canvas.obj.filter(|o| app.points.id != Some(*o)).and_then(|id| app.drawables.iter().find(|d| d.id == id)) {
        let g = tiles::frame(d);
        let r = view.rect(Rect::from_min_size(Pos2::new(g.x, g.y), Vec2::new(g.w.max(8.0), g.h.max(8.0))));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, accent()), egui::StrokeKind::Middle);
        // Numbers: eight small square white handles
        for p in handle_points(r) {
            painter.rect(Rect::from_center_size(p, Vec2::splat(7.0)), 1.0, Color32::WHITE, Stroke::new(1.0, Color32::from_gray(0x80)), egui::StrokeKind::Middle);
        }
    }
    if resp.dragged() && !special_drag {
        if let (Some(p), Some(drag), Some(sel)) = (pointer, app.canvas.drag, app.sel) {
            let s = view.to_sheet(p);
            if let Some((_, g, _, _)) = grids.iter().find(|g| g.0 == sel.table) {
                let clamp_r = |y: f32| g.row_at(y).unwrap_or(if y < g.g.y { 0 } else { g.ys.len() as u32 - 2 });
                let clamp_c = |x: f32| g.col_at(x).unwrap_or(if x < g.g.x { 0 } else { g.xs.len() as u32 - 2 });
                let cursor = match drag {
                    Drag::Cells(_) => (clamp_r(s.y), clamp_c(s.x)),
                    Drag::Columns(_) => (sel.cursor.0, clamp_c(s.x)),
                    Drag::Rows(_) => (clamp_r(s.y), sel.cursor.1),
                    Drag::Resize { .. } | Drag::Move { .. } | Drag::Fill { .. } | Drag::AutoFill { .. } | Drag::ObjSize { .. } | Drag::Grow { .. } | Drag::MoveCells { .. } | Drag::Rotate { .. } | Drag::Guide { .. } => sel.cursor,
                };
                app.sel = Some(Sel { cursor, ..sel });
            }
        }
    }
    if resp.drag_stopped() {
        // a guide let go: kept where it is, or gone when dropped back on a ruler / off the sheet
        if let (Some(Drag::Guide { index, vertical, pos }), Some(s)) = (app.canvas.drag, sheet_id) {
            let p = resp.interact_pointer_pos().unwrap_or(rect.center());
            let away = (vertical && p.x < rect.min.x + RULER_BAND) || (!vertical && p.y < rect.min.y + RULER_BAND) || !rect.contains(p);
            let mut gs = user_guides.clone();
            match (index, away) {
                (Some(i), true) => {
                    gs.remove(i);
                }
                (Some(i), false) => gs[i].pos = pos,
                (None, false) => gs.push(nm_core::kn_model::slides::Guide { vertical, pos }),
                (None, true) => {}
            }
            if gs != user_guides {
                app.edit(if index.is_some() && away { "Remove Guide" } else { "Guide" }, |d, h| d.set_guides(h, s, &gs));
            }
        }
        // the autofill dot let go: the cells it covered fill from the block (series continue)
        if let Some(Drag::AutoFill { table, src, to }) = app.canvas.drag.take() {
            if to != src {
                if let Some(Err(e)) = app.edit("Autofill", |d, h| d.autofill(h, table, src, to)) {
                    app.notify(format!("Autofill: {e}"));
                }
                app.sel = Some(Sel { table, anchor: (to.0, to.1), cursor: (to.2, to.3) });
            }
        }
        app.canvas.drag = None;
    }
    resp.context_menu(|ui| table_menu(app, ui));

    // ------------------------------------------------------------------ text box editor
    // (Keydeck's in-place editor: the sheet draws the text, this draws the caret and selection)
    if let Some(id) = app.canvas.editing.as_ref().map(|e| e.id) {
        match crate::textedit::layout_of(app, id) {
            Some((tl, g)) => {
                let to_screen = |p: nm_core::kn_model::Point| view.to_screen(Pos2::new(p.x, p.y));
                crate::textedit::draw_spelling(app, &painter, &tl, &g, to_screen);
                crate::textedit::draw_overlay(app, &painter, &tl, &g, to_screen);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(530));
            }
            None => crate::textedit::commit(app),
        }
    }
    crate::textedit::spell_menu_ui(app, ui.ctx());

    // Edit Data References: the chart's cells outlined in its series' colours
    for (t, (r0, c0, r1, c1), col) in crate::datarefs::outlines(app) {
        if let Some((_, g, _, _)) = grids.iter().find(|g| g.0 == t) {
            if (r1 as usize) < g.ys.len() - 1 && (c1 as usize) < g.xs.len() - 1 {
                let b = g.block(r0, c0, r1, c1);
                let r = Rect::from_min_max(view.to_screen(b.min), view.to_screen(b.max));
                painter.rect_filled(r, 2.0, col.gamma_multiply(0.12));
                painter.rect_stroke(r, 2.0, Stroke::new(2.5, col), egui::StrokeKind::Middle);
            }
        }
    }
    crate::datarefs::bar(app, ui.ctx(), rect);

    // linked objects: the green link badge; a click opens the link popover
    let linked: Vec<(nm_core::kn_format::ObjId, Rect)> = app
        .drawables
        .iter()
        .filter(|d| d.hyperlink.is_some())
        .map(|d| (d.id, Rect::from_two_pos(view.to_screen(Pos2::new(d.geom.x, d.geom.y)), view.to_screen(Pos2::new(d.geom.x + d.geom.w, d.geom.y + d.geom.h)))))
        .filter(|(_, r)| rect.intersects(*r))
        .collect();
    for (id, r) in linked {
        if crate::links::badge(ui, r, id) {
            commit(app);
            app.sel = None;
            app.canvas.multi.clear();
            app.canvas.obj = Some(id);
            crate::links::open(app, "Webpage");
            if let Some(le) = app.link_edit.as_mut() {
                le.at = Some(Pos2::new(r.max.x - 160.0, r.max.y + 6.0));
            }
        }
    }

    // ------------------------------------------------------------------ in-cell editor
    if let Some(e) = &app.canvas.edit {
        if let Some((_, g, _, _)) = grids.iter().find(|g| g.0 == e.table) {
            // the cells a formula being typed refers to, each in its own colour
            if e.text.trim_start().starts_with('=') {
                for (i, (m0, c0, m1, c1)) in formula_refs(&e.text).into_iter().enumerate() {
                    let (a, b) = (app.vrow(e.table, m0), app.vrow(e.table, m1));
                    let (r0, r1) = (a.min(b), a.max(b));
                    if (r1 as usize) < g.ys.len() - 1 && (c1 as usize) < g.xs.len() - 1 {
                        let col = REF_COLORS[i % REF_COLORS.len()];
                        let r = view.rect(g.block(r0, c0, r1, c1));
                        painter.rect_filled(r, 0.0, col.gamma_multiply(0.15));
                        painter.rect_stroke(r, 0.0, Stroke::new(2.0, col), egui::StrokeKind::Middle);
                    }
                }
            }
            if (e.row as usize) < g.ys.len() - 1 && (e.col as usize) < g.xs.len() - 1 {
                // a merged cell is edited across the whole merge
                let (t, er, ec) = (e.table, e.row, e.col);
                let b = app.with_merges(t, (er, ec, er, ec));
                let r = view.rect(g.block(b.0, b.1, b.2, b.3)).expand(1.0);
                editor(app, ui, r);
            }
        }
    }
    !pending
}

/// The table title being renamed: a field over the title. Return or clicking away keeps the
/// name, Escape leaves it as it was.
fn title_editor(app: &mut App, ui: &mut egui::Ui, r: Rect, zoom: f32) {
    let Some((id, mut text)) = app.canvas.title_edit.clone() else { return };
    let area = Rect::from_center_size(r.center(), Vec2::new(r.width().max(140.0 * zoom), r.height() + 4.0));
    ui.painter().rect(area, 2.0, Color32::WHITE, Stroke::new(2.0, accent()), egui::StrokeKind::Outside);
    let te = egui::TextEdit::singleline(&mut text)
        .id(egui::Id::unique("table-title-editor"))
        .font(egui::FontId::proportional(14.0 * zoom))
        .text_color(Color32::BLACK)
        .align(egui::Align2::CENTER_CENTER)
        .frame(egui::Frame::NONE)
        .desired_width(area.width());
    let resp = ui.put(area, te);
    if !resp.has_focus() && !resp.lost_focus() {
        resp.request_focus();
    }
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if esc {
        app.canvas.title_edit = None;
        return;
    }
    if resp.lost_focus() {
        app.canvas.title_edit = None;
        let old = app.table(id).map(|(_, t)| t.name.clone()).unwrap_or_default();
        let new = text.trim().to_string();
        if !new.is_empty() && new != old {
            app.edit("Table Name", |d, h| nm_core::sheets::rename_table(d, h, id, &new));
        }
        return;
    }
    app.canvas.title_edit = Some((id, text));
}

fn editor(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let zoom = app.canvas.zoom;
    // the text at the cell's own size, as Numbers edits in place
    let (fsize, pad) = app
        .canvas
        .edit
        .as_ref()
        .and_then(|e| app.table(e.table).and_then(|(_, t)| t.cell_styles.get((e.row * t.cols + e.col) as usize).map(|s| (s.text.size, s.padding))))
        .filter(|s| s.0 > 0.0)
        .unwrap_or((13.0, [4.0; 4]));
    // Shift+Return (the suite's way) or Option+Return (Numbers'): a line break in the cell;
    // Return ends the edit (in a formula Shift+Return ends it too). Taken before the field sees them
    let formula = app.canvas.edit.as_ref().is_some_and(|e| e.text.trim_start().starts_with('='));
    let alt_enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::Enter) | (!formula && i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter)));
    let shift = ui.input(|i| i.modifiers.shift);
    let enter = !alt_enter && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) | i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let tab_key = app.canvas.tab.take();
    let (tab, shift) = (tab_key.is_some(), shift || tab_key == Some(true));
    let Some(e) = app.canvas.edit.as_mut() else { return };
    if alt_enter {
        // at the caret (else the end)
        let id = egui::Id::unique("cell-editor");
        let at = egui::TextEdit::load_state(ui.ctx(), id).and_then(|st| st.cursor.char_range()).map(|r| r.primary.index.0).unwrap_or(e.text.chars().count());
        let byte = e.text.char_indices().nth(at).map(|(b, _)| b).unwrap_or(e.text.len());
        e.text.insert(byte, '\n');
        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at + 1))));
            st.store(ui.ctx(), id);
        }
    }
    let w = r.width().max(80.0);
    let font = egui::FontId::proportional(fsize * zoom);
    let widest = e.text.lines().map(|l| ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), Color32::BLACK).size().x)).fold(0.0f32, f32::max);
    let lines = e.text.split('\n').count().max(1) as f32;
    // lines as the cell wraps them: when that changes, lay the table out again so the row grows
    // (or shrinks back) while typing, as Numbers does
    let wrapped: usize = e.text.split('\n').map(|l| {
        let lw = ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), Color32::BLACK).size().x);
        ((lw / (w - 8.0).max(1.0)).ceil() as usize).max(1)
    }).sum();
    let key = egui::Id::unique("cell-editor-lines");
    let before = ui.data(|d| d.get_temp::<(ObjId, u32, u32, usize)>(key));
    let here = (e.table, e.row, e.col, wrapped);
    if before != Some(here) {
        ui.data_mut(|d| d.insert_temp(key, here));
        if before.is_some_and(|b| (b.0, b.1, b.2) == (here.0, here.1, here.2)) {
            app.version += 1;
            ui.ctx().request_repaint();
        }
    }
    let Some(e) = app.canvas.edit.as_mut() else { return };
    let area = Rect::from_min_size(r.min, Vec2::new(w.max(widest + 16.0), r.height().max(lines * fsize * 1.15 * zoom + 6.0)));
    ui.painter().rect(area, 0.0, Color32::WHITE, Stroke::new(2.0, accent()), egui::StrokeKind::Outside);
    // Edit > Substitutions while typing (kn-ui's, as Keydeck): smart quotes and dashes, and
    // when a word ends its text replacement or spelling correction; not in formulas
    let st = &app.settings;
    if !e.text.starts_with('=') {
        let id = egui::Id::unique("cell-editor");
        let caret = egui::TextEdit::load_state(ui.ctx(), id).and_then(|s| s.cursor.char_range()).filter(|r| r.primary == r.secondary).map(|r| r.primary.index.0);
        if let Some(pos) = caret {
            let mut done = false;
            let text = &mut e.text;
            let mut new_pos = None;
            ui.input_mut(|i| {
                for ev in i.events.iter_mut() {
                    let egui::Event::Text(t) = ev else { continue };
                    if done {
                        break;
                    }
                    done = true;
                    let byte = text.char_indices().nth(pos).map(|(b, _)| b).unwrap_or(text.len());
                    let before = text[..byte].to_string();
                    let (back, ins) = kn_ui::typing::smart_text(before.chars().last(), t, true, st.smart_quotes, st.smart_dashes);
                    let ends = back == 0 && t.chars().all(|c| c.is_whitespace() || ".,;:!?)".contains(c));
                    let mut head = before.clone();
                    if ends {
                        if let Some((start, fixed)) = kn_ui::typing::word_fix(&before, st.text_replacement, st.autocorrect && st.spell_check) {
                            head = format!("{}{fixed}", &before[..start]);
                        }
                    } else if back > 0 {
                        for _ in 0..back {
                            head.pop();
                        }
                    }
                    if head != before || ins != *t {
                        *text = format!("{head}{}", &text[byte..]);
                        *t = ins;
                        new_pos = Some(head.chars().count());
                    }
                }
            });
            if let (Some(np), Some(mut s)) = (new_pos, egui::TextEdit::load_state(ui.ctx(), id)) {
                s.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(np))));
                s.store(ui.ctx(), id);
            }
        }
    }
    // misspelled words underlined in red (Check Spelling While Typing)
    let (check, grammar) = (st.spell_check && !e.text.starts_with('='), st.grammar);
    let font = egui::FontId::proportional(fsize * zoom);
    let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
        let text = buf.as_str();
        let mut marks: Vec<(usize, usize)> = if check { kn_ui::spell::misspelled(text) } else { Vec::new() };
        if check && grammar {
            marks.extend(kn_ui::spell::grammar(text));
        }
        marks.sort_unstable();
        let mut job = egui::text::LayoutJob::default();
        // lines as far apart as the cell's own (the row is sized by them)
        let plain = egui::TextFormat { font_id: font.clone(), color: Color32::BLACK, line_height: Some(fsize * 1.15 * zoom), ..Default::default() };
        let bad = egui::TextFormat { underline: Stroke::new(1.5 * zoom.max(0.8), Color32::from_rgb(0xe0, 0x1b, 0x24)), ..plain.clone() };
        let mut at = 0;
        for (a, b) in marks {
            if a < at {
                continue;
            }
            job.append(&text[at..a], 0.0, plain.clone());
            job.append(&text[a..b], 0.0, bad.clone());
            at = b;
        }
        job.append(&text[at..], 0.0, plain);
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let te = egui::TextEdit::multiline(&mut e.text)
        .layouter(&mut layouter)
        .desired_rows(lines as usize)
        .id(egui::Id::unique("cell-editor"))
        .font(egui::FontId::proportional(fsize * zoom))
        // (the cell is on the white sheet, whatever the interface's light / dark)
        .text_color(Color32::BLACK)
        // where the cell puts its text (its Text Inset); with no frame drawn the field's own
        // margin is not applied, so the inset is the frame's
        .frame(egui::Frame::NONE.inner_margin({
            let m = |v: f32| (v * zoom).round().clamp(0.0, 127.0) as i8;
            egui::Margin { left: m(pad[0]), top: m(pad[1]), right: m(pad[2]), bottom: m(pad[3]) }
        }))
        .lock_focus(true)
        .desired_width(area.width() - (pad[0] + pad[2]) * zoom);
    let resp = ui.put(area, te);
    if e.focus {
        // text changed from outside (a picked reference, typing over a cell): caret to the end
        resp.request_focus();
        e.focus = false;
        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
            // a double-click: the character boundary nearest the click, else the end
            let font = egui::FontId::proportional(fsize * zoom);
            let at = e.click.take().map(|p| {
                let x = p.x - (area.min.x + pad[0] * zoom);
                let chars: Vec<char> = e.text.chars().collect();
                let width = |n: usize| ui.fonts_mut(|f| f.layout_no_wrap(chars[..n].iter().collect(), font.clone(), Color32::BLACK).size().x);
                (0..=chars.len()).min_by(|a, b| (width(*a) - x).abs().total_cmp(&(width(*b) - x).abs())).unwrap_or(chars.len())
            });
            let end = egui::text::CCursor::new(at.unwrap_or(e.text.chars().count()));
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
            st.store(ui.ctx(), resp.id);
        }
    }
    // typing a function name in a formula: matching functions below the cell; Tab takes the first
    let token = if app.settings.formula_suggestions { function_token(&e.text).map(str::to_string) } else { None };
    let matches: Vec<(&str, &str)> = match &token {
        Some(t) if !t.is_empty() => {
            // shortest names first: "=SU" offers SUM before SUBSTITUTE
            let mut m: Vec<(&str, &str)> = nm_core::formula::functions().iter().filter(|(n, _)| n.starts_with(&t.to_ascii_uppercase())).copied().collect();
            m.sort_by_key(|(n, _)| (n.len(), *n));
            m.truncate(8);
            m
        }
        _ => Vec::new(),
    };
    let mut chosen = None;
    if !matches.is_empty() {
        egui::Area::new(egui::Id::unique("fn-suggest")).order(egui::Order::Foreground).fixed_pos(area.left_bottom() + Vec2::new(0.0, 4.0)).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for (i, (name, syntax)) in matches.iter().enumerate() {
                    let r = ui.add(egui::Button::new(RichText::new(format!("{name}    {syntax}")).size(12.0)).frame(i == 0));
                    if r.clicked() {
                        chosen = Some(*name);
                    }
                }
            });
        });
        if tab {
            chosen = Some(matches[0].0);
        }
    }
    if let (Some(name), Some(t)) = (chosen, token) {
        e.text.truncate(e.text.len() - t.len());
        e.text.push_str(name);
        e.text.push('(');
        e.focus = true;
        return;
    }
    // typing text: the column's matching values in a list under the cell (Numbers' auto-complete,
    // Mac N21: white with a shadow, the cell's width, shown at once). Down / Up pick one,
    // Return / Tab take the picked one, a click takes it; with none picked Return keeps the typing
    let (t, er, ec, typed_now) = (e.table, e.row, e.col, e.text.clone());
    let offers = if app.settings.autocomplete_cells && matches.is_empty() { column_matches(app, t, er, ec, &typed_now) } else { Vec::new() };
    let Some(e) = app.canvas.edit.as_mut() else { return };
    let pick_key = egui::Id::unique("cell-autocomplete");
    // (the pick, and the text it was for: typing more starts over)
    let mut pick: Option<usize> = ui.ctx().data(|d| d.get_temp::<(usize, String)>(pick_key)).filter(|(_, t)| *t == typed_now).map(|(i, _)| i).filter(|i| *i < offers.len());
    let mut take: Option<String> = None;
    if !offers.is_empty() {
        let n = offers.len();
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            pick = Some(pick.map_or(0, |p| (p + 1).min(n - 1)));
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            pick = match pick {
                Some(0) | None => None,
                Some(p) => Some(p - 1),
            };
        }
        let font = egui::FontId::proportional(fsize * zoom);
        let row_h = (18.0 * zoom).max(16.0);
        let list = Rect::from_min_size(area.left_bottom(), Vec2::new(r.width(), row_h * n as f32 + 4.0));
        egui::Area::new(egui::Id::unique("cell-autocomplete-list")).order(egui::Order::Foreground).fixed_pos(list.min).show(ui.ctx(), |ui| {
            let (rect, _) = ui.allocate_exact_size(list.size(), egui::Sense::hover());
            let p = ui.painter();
            p.add(egui::Shadow { offset: [0, 3], blur: 10, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(rect, 0.0));
            p.rect_filled(rect, 0.0, Color32::WHITE);
            for (i, v) in offers.iter().enumerate() {
                let row = Rect::from_min_size(rect.min + Vec2::new(0.0, 2.0 + row_h * i as f32), Vec2::new(rect.width(), row_h));
                let resp = ui.interact(row, egui::Id::unique(("cell-autocomplete-row", i)), egui::Sense::click());
                let on = pick == Some(i) || resp.hovered();
                if on {
                    p.rect_filled(row, 0.0, accent());
                }
                let galley = ui.painter().layout_no_wrap(v.clone(), font.clone(), if on { Color32::WHITE } else { Color32::BLACK });
                p.with_clip_rect(row).galley(egui::pos2(row.min.x + 6.0, row.center().y - galley.size().y / 2.0), galley, Color32::BLACK);
                if resp.clicked() {
                    take = Some(v.clone());
                }
            }
        });
        if (enter || tab) && let Some(i) = pick {
            take = Some(offers[i].clone());
        }
    }
    match pick {
        Some(i) => {
            ui.ctx().data_mut(|d| d.insert_temp(pick_key, (i, typed_now.clone())));
        }
        None => {
            ui.ctx().data_mut(|d| d.remove_temp::<(usize, String)>(pick_key));
        }
    }
    if let Some(v) = take {
        e.text = v;
        ui.ctx().data_mut(|d| d.remove_temp::<(usize, String)>(pick_key));
        // a click takes it and ends the edit there
        if !(enter || tab) {
            resp.surrender_focus();
            commit(app);
            return;
        }
    }
    if esc {
        resp.surrender_focus();
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        app.canvas.edit = None;
        // a row grown while typing goes back to the cell's real text
        app.version += 1;
        return;
    }
    // typing over a cell: arrows commit and move, as Numbers does
    let arrow = if e.typed {
        ui.input(|i| {
            [(egui::Key::ArrowDown, 1, 0), (egui::Key::ArrowUp, -1, 0), (egui::Key::ArrowRight, 0, 1), (egui::Key::ArrowLeft, 0, -1)].into_iter().find(|(k, ..)| i.key_pressed(*k)).map(|(_, r, c)| (r, c))
        })
    } else {
        None
    };
    // a key that ends the edit is used up here, and the field lets go of the keyboard
    let finishing = arrow.is_some() || tab || enter || esc;
    if finishing {
        resp.surrender_focus();
        ui.input_mut(|i| {
            for k in [egui::Key::Tab, egui::Key::Enter, egui::Key::Escape, egui::Key::ArrowDown, egui::Key::ArrowUp, egui::Key::ArrowLeft, egui::Key::ArrowRight] {
                i.consume_key(egui::Modifiers::NONE, k);
                i.consume_key(egui::Modifiers::SHIFT, k);
            }
        });
    }
    if let Some((dr, dc)) = arrow {
        commit(app);
        app.canvas.return_col = None;
        move_cursor(app, dr, dc, false);
        return;
    }
    if tab {
        commit(app);
        tab_move(app, shift);
    } else if enter {
        // Return (and Shift+Return: Numbers moves down for both) commits and goes down
        commit(app);
        return_move(app);
    }
}

/// The values of table `t`'s column `col` (body rows, other than `row`) that start with
/// `typed` (any case), text only, each once, alphabetically, other than `typed` itself; none
/// for a formula, an empty or a several-line entry.
fn column_matches(app: &App, t: ObjId, row: u32, col: u32, typed: &str) -> Vec<String> {
    let typed_l = typed.to_lowercase();
    if typed.trim().is_empty() || typed.starts_with('=') || typed.contains('\n') {
        return Vec::new();
    }
    let Some((_, td)) = app.table(t) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for r in td.header_rows..td.rows.saturating_sub(td.footer_rows) {
        if r == row {
            continue;
        }
        let Some(v) = td.cells.get((r * td.cols + col) as usize) else { continue };
        let v = v.trim();
        if v.is_empty() || v.contains('\n') || v.replace([',', ' '], "").parse::<f64>().is_ok() {
            continue;
        }
        if v.to_lowercase().starts_with(&typed_l) && !out.iter().any(|o| o.to_lowercase() == v.to_lowercase()) {
            out.push(v.to_string());
        }
    }
    // (what's typed already isn't worth offering)
    out.retain(|v| v != typed);
    out.sort_by_key(|v| v.to_lowercase());
    out.truncate(8);
    out
}

/// Start editing the cursor cell: with `typed` the cell's content is replaced (typing over
/// a selected cell), otherwise its formula or text is edited.
pub fn start_edit(app: &mut App, typed: Option<String>) {
    let typed_over = typed.is_some();
    // the first character typed over a cell gets its smart quote too (an opening one)
    let typed = typed.map(|t| kn_ui::typing::smart_text(None, &t, true, app.settings.smart_quotes, app.settings.smart_dashes).1);
    let Some(sel) = app.sel else { return };
    let (r, c) = sel.cursor;
    // a category's group row isn't a cell of the model
    let Some(mr) = app.mrow(sel.table, r) else { return };
    let formula = app.formula_of(sel.table, mr, c);
    let text = typed.unwrap_or_else(|| {
        formula
            .or_else(|| app.table(sel.table).and_then(|(_, t)| t.cells.get((r * t.cols + c) as usize).cloned()))
            .unwrap_or_default()
    });
    app.canvas.edit = Some(CellEdit { table: sel.table, row: r, col: c, focus: true, typed: typed_over, text, click: None });
}

/// Store the edited cell (Keydeck's commit_cell): "=…" makes a formula, else number or text.
pub fn commit(app: &mut App) {
    app.canvas.pick = None;
    // a text box being edited
    crate::textedit::commit(app);
    let Some(e) = app.canvas.edit.take() else { return };
    let old = app.table(e.table).and_then(|(_, t)| t.cells.get((e.row * t.cols + e.col) as usize).cloned()).unwrap_or_default();
    let Some(row) = app.mrow(e.table, e.row) else { return };
    let old_formula = app.formula_of(e.table, row, e.col);
    if (old_formula.is_none() && e.text == old) || old_formula.as_deref() == Some(e.text.trim()) {
        return;
    }
    let text = e.text.clone();
    let res = app.edit("Edit Cell", |d, h| {
        if text.trim_start().starts_with('=') && text.trim().len() > 1 {
            d.set_formula_text(h, e.table, row, e.col, &text)
        } else {
            nm_core::typed::set_typed(d, h, e.table, row, e.col, &text)
        }
    });
    if let Some(Err(err)) = res {
        app.notify(format!("Couldn't change the cell: {err}"));
    }
}

fn move_cursor(app: &mut App, dr: i32, dc: i32, extend: bool) {
    let entry = app.canvas.merge_entry.take();
    let Some(sel) = app.sel else { return };
    let Some((_, t)) = app.table(sel.table) else { return };
    let (rows, cols) = (t.rows as i32, t.cols as i32);
    // hidden (filtered) rows and columns have size 0 and are stepped over
    let step = |from: i32, d: i32, n: i32, size: &dyn Fn(i32) -> f32| -> u32 {
        let mut i = (from + d).clamp(0, n - 1);
        while d != 0 && size(i) <= 0.0 && i + d.signum() >= 0 && i + d.signum() < n {
            i += d.signum();
        }
        if size(i) <= 0.0 { from as u32 } else { i as u32 }
    };
    // from a merge, step out past its far edge
    let (mut fr, mut fc) = sel.cursor;
    if let Some(m) = t.merge_at(fr, fc) {
        if let Some((er, ec)) = entry.filter(|&(er, ec)| m.contains(er, ec)) {
            if dr != 0 {
                fc = ec;
            }
            if dc != 0 {
                fr = er;
            }
        }
        if dr > 0 {
            fr = m.row + m.rows - 1;
        }
        if dc > 0 {
            fc = m.col + m.cols - 1;
        }
        if dr < 0 {
            fr = m.row;
        }
        if dc < 0 {
            fc = m.col;
        }
    }
    let r = step(fr as i32, dr, rows, &|i| t.row_heights.get(i as usize).copied().unwrap_or(1.0));
    let c = step(fc as i32, dc, cols, &|i| t.col_widths.get(i as usize).copied().unwrap_or(1.0));
    // landing inside a merge is landing on the merge (unless extending a range); where it
    // landed is remembered for leaving it
    let landed = (r, c);
    let (r, c) = if extend { (r, c) } else { app.merge_origin(sel.table, r, c) };
    if !extend && landed != (r, c) {
        app.canvas.merge_entry = Some(landed);
    }
    app.sel = Some(if extend { Sel { cursor: (r, c), ..sel } } else { Sel::cell(sel.table, r, c) });
}

pub fn select_all(app: &mut App) {
    let Some(sel) = app.sel else { return };
    if let Some((_, t)) = app.table(sel.table) {
        app.sel = Some(Sel { table: sel.table, anchor: (0, 0), cursor: (t.rows - 1, t.cols - 1) });
    }
}

/// Delete: empty every cell of the selection.
pub fn clear_cells(app: &mut App) {
    let Some(sel) = app.sel else { return };
    let (r0, c0, r1, c1) = app.with_merges(sel.table, sel.block());
    let nonempty: Vec<(u32, u32)> = match app.table(sel.table) {
        Some((_, t)) => (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c))).filter(|&(r, c)| !t.cells[(r * t.cols + c) as usize].is_empty()).collect(),
        None => return,
    };
    if nonempty.is_empty() {
        return;
    }
    let nonempty: Vec<(u32, u32)> = nonempty.into_iter().filter_map(|(r, c)| Some((app.mrow(sel.table, r)?, c))).collect();
    let res = app.edit("Delete", |d, h| nonempty.iter().try_for_each(|&(r, c)| d.set_cell(h, sel.table, r, c, &CellInput::Empty)));
    if let Some(Err(e)) = res {
        app.notify(format!("Couldn't delete: {e}"));
    }
}

/// Copy the selection as tab-separated text (what Numbers puts on the clipboard as plain text).
pub fn copy(app: &mut App, ctx: &egui::Context, cut: bool) {
    // the cells with their formulas, formats and styles (Numbers), and as text for other apps
    let Some(text) = crate::clip::copy(app) else { return };
    ctx.copy_text(text);
    if cut {
        // Numbers empties the cut cells, their style too
        clear_cells(app);
        crate::clip::clear_style(app);
    }
}

/// Paste tab-separated text at the cursor (clipped to the table).
fn paste(app: &mut App, text: &str) {
    let Some(sel) = app.sel else { return };
    let Some((_, t)) = app.table(sel.table) else { return };
    let (rows, cols) = (t.rows, t.cols);
    let (r0, c0, _, _) = sel.block();
    let lines: Vec<&str> = text.trim_end_matches(['\n', '\r']).split('\n').collect();
    let mut cells = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        for (j, v) in line.trim_end_matches('\r').split('\t').enumerate() {
            let (r, c) = (r0 + i as u32, c0 + j as u32);
            if r < rows && c < cols {
                cells.push((r, c, v.to_string()));
            }
        }
    }
    if cells.is_empty() {
        return;
    }
    let (rmax, cmax) = cells.iter().fold((r0, c0), |a, x| (a.0.max(x.0), a.1.max(x.1)));
    let cells: Vec<(u32, u32, String)> = cells.into_iter().filter_map(|(r, c, v)| Some((app.mrow(sel.table, r)?, c, v))).collect();
    let res = app.edit("Paste", |d, h| {
        cells.iter().try_for_each(|(r, c, v)| {
            if v.trim_start().starts_with('=') && v.trim().len() > 1 {
                d.set_formula_text(h, sel.table, *r, *c, v)
            } else {
                nm_core::typed::set_typed(d, h, sel.table, *r, *c, v)
            }
        })
    });
    if let Some(Err(e)) = res {
        app.notify(format!("Couldn't paste: {e}"));
    }
    app.sel = Some(Sel { table: sel.table, anchor: (r0, c0), cursor: (rmax, cmax) });
}

/// Keyboard: shortcuts always; cell navigation and typing when no text field has focus.
pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
    let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
    // undo / redo end text editing first (its typing is one step)
    if app.canvas.editing.is_some() && ctx.input(|i| i.modifiers.command && (i.key_pressed(Key::Z) || i.key_pressed(Key::Y))) {
        crate::textedit::commit(app);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::Z))) || ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Y))) {
        app.canvas.edit = None;
        app.redo();
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Z))) {
        if app.canvas.edit.is_some() {
            app.canvas.edit = None;
        } else {
            app.undo();
        }
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::S))) {
        commit(app);
        app.save(true);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::S))) {
        commit(app);
        app.save(false);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::O))) {
        app.open_dialog();
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::F))) {
        commit(app);
        app.find = Some((String::new(), Vec::new(), 0));
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::N))) {
        app.new_document();
    }
    if app.canvas.edit.is_some() || app.stocks.open || ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    // text being edited in place: the keys go to it
    if app.canvas.editing.is_some() {
        let tl = app.canvas.editing.as_ref().map(|e| e.id).and_then(|id| crate::textedit::layout_of(app, id)).map(|x| x.0);
        crate::textedit::handle_keys(app, ctx, tl.as_ref());
        return;
    }
    // a selected chart / shape: Delete removes it, arrows nudge it
    if let Some(id) = app.canvas.obj {
        // copy / cut / paste of objects (their own clipboard, as in Keydeck)
        for ev in ctx.input(|i| i.events.clone()) {
            match ev {
                egui::Event::Copy => app.copy_objects(),
                egui::Event::Cut => {
                    app.copy_objects();
                    app.delete_objects();
                }
                egui::Event::Paste(_) if app.obj_clip_active => app.paste_here(),
                _ => {}
            }
        }
        if app.canvas.obj != Some(id) {
            return;
        }
        // Tab / Shift+Tab: the next / previous object on the sheet (Numbers, Mac N16)
        if let Some(back) = app.canvas.tab.take() {
            let ids: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
            if let Some(i) = ids.iter().position(|o| *o == id) {
                let n = ids.len();
                let next = ids[if back { (i + n - 1) % n } else { (i + 1) % n }];
                app.canvas.obj = Some(next);
                app.sel = None;
            }
            return;
        }
        // typing with a text box or shape selected replaces its text (Numbers, Mac N16)
        let typed: String = ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.as_str()) } else { None }).collect());
        if !typed.is_empty() && !ctx.input(|i| i.modifiers.command) {
            if crate::textedit::begin(app, id, None) {
                crate::textedit::insert_at_caret(app, &typed);
                return;
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            app.delete_objects();
            return;
        }
        let step = if ctx.input(|i| i.modifiers.shift) { 10.0 } else { 1.0 };
        for (k, dx, dy) in [(Key::ArrowLeft, -step, 0.0), (Key::ArrowRight, step, 0.0), (Key::ArrowUp, 0.0, -step), (Key::ArrowDown, 0.0, step)] {
            if ctx.input(|i| i.key_pressed(k)) {
                let all: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
                app.edit("Move", |d, h| {
                    d.move_by(h, id, dx, dy);
                    d.update_connections_in(h, &all, &[id]);
                });
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            app.canvas.obj = None;
        }
        return;
    }
    if app.sel.is_none() {
        // copied cells pasted on the empty canvas make a new table (Numbers, Mac N18)
        let ours = |t: &str| app.cell_clip.as_ref().is_some_and(|c| c.tsv.replace("\r\n", "\n") == t.replace("\r\n", "\n"));
        let paste_here = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(t) if ours(t))));
        if paste_here && !app.obj_clip_active {
            crate::clip::paste_new_table(app);
        }
        return;
    }
    // copy / cut / paste arrive as events
    let events = ctx.input(|i| i.events.clone());
    for ev in &events {
        match ev {
            egui::Event::Copy => copy(app, ctx, false),
            egui::Event::Cut => copy(app, ctx, true),
            egui::Event::Paste(_) if app.obj_clip_active => app.paste_here(),
            // our own copy: everything it holds; text from elsewhere: values
            egui::Event::Paste(t) if app.cell_clip.as_ref().is_some_and(|c| c.tsv.replace("\r\n", "\n") == t.replace("\r\n", "\n")) => {
                crate::clip::paste(app, crate::clip::PasteMode::All);
            }
            egui::Event::Paste(t) => paste(app, t),
            _ => {}
        }
    }
    if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::A))) {
        select_all(app);
    }
    let (shift, alt, cmd) = ctx.input(|i| (i.modifiers.shift, i.modifiers.alt, i.modifiers.command));
    let pressed = |k| ctx.input(|i| i.key_pressed(k));
    // Option+arrows add a row / column next to the cursor (Numbers); Cmd+arrows go to the edge
    if alt && !cmd {
        for (k, axis, after) in [(Key::ArrowDown, Axis::Row, true), (Key::ArrowUp, Axis::Row, false), (Key::ArrowRight, Axis::Column, true), (Key::ArrowLeft, Axis::Column, false)] {
            if pressed(k) {
                insert_next_to(app, axis, after);
            }
        }
        return;
    }
    let far = 1 << 20;
    let step = |n: i32| if cmd { n * far } else { n };
    for (k, dr, dc) in [(Key::ArrowDown, 1, 0), (Key::ArrowUp, -1, 0), (Key::ArrowRight, 0, 1), (Key::ArrowLeft, 0, -1)] {
        if pressed(k) {
            app.canvas.return_col = None;
            move_cursor(app, step(dr), step(dc), shift);
        }
    }
    if let Some(back) = app.canvas.tab.take() {
        tab_move(app, back);
    }
    if pressed(Key::Enter) {
        return_move(app);
    }
    if pressed(Key::F2) {
        start_edit(app, None);
    }
    if pressed(Key::Delete) || pressed(Key::Backspace) {
        clear_cells(app);
    }
    // typing over a selected cell replaces it
    let typed: String = events
        .iter()
        .filter_map(|e| match e {
            egui::Event::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    if !typed.is_empty() && !ctx.input(|i| i.modifiers.command) {
        let Some(sel) = app.sel else { return };
        app.sel = Some(Sel::cell(sel.table, sel.cursor.0, sel.cursor.1));
        start_edit(app, Some(typed));
    }
}

/// Table menu (menu bar and right-click): rows, columns, headers, merge, sort.
pub fn table_menu(app: &mut App, ui: &mut egui::Ui) {
    let Some(sel) = app.sel else {
        ui.label("Select cells in a table first.");
        return;
    };
    let t = sel.table;
    let (r0, c0, r1, c1) = sel.block();
    let categorized = app.categorized(t);
    let run = |app: &mut App, label: &str, f: &dyn Fn(&mut nm_core::kn_model::Document, &mut nm_core::kn_model::edit::History) -> Result<(), nm_core::kn_format::Error>| {
        commit(app);
        if let Some(Err(e)) = app.edit(label, |d, h| f(d, h)) {
            app.notify(format!("{label}: {e}"));
        }
    };
    if categorized {
        ui.label("Rows, merging and sorting wait until categories are off.");
        ui.separator();
    }
    if ui.add_enabled(!categorized, egui::Button::new("Add Row Above")).clicked() {
        run(app, "Add Row", &|d, h| d.table_insert(h, t, Axis::Row, r0, r0));
    }
    if ui.add_enabled(!categorized, egui::Button::new("Add Row Below")).clicked() {
        run(app, "Add Row", &|d, h| d.table_insert(h, t, Axis::Row, r1 + 1, r1));
    }
    if ui.button("Add Column Before").clicked() {
        run(app, "Add Column", &|d, h| d.table_insert(h, t, Axis::Column, c0, c0));
    }
    if ui.button("Add Column After").clicked() {
        run(app, "Add Column", &|d, h| d.table_insert(h, t, Axis::Column, c1 + 1, c1));
    }
    ui.separator();
    let label_r = if r1 > r0 { "Delete Selected Rows" } else { "Delete Row" };
    if ui.add_enabled(!categorized, egui::Button::new(label_r)).clicked() {
        run(app, "Delete Row", &|d, h| (r0..=r1).rev().try_for_each(|r| d.table_delete(h, t, Axis::Row, r)));
    }
    let label_c = if c1 > c0 { "Delete Selected Columns" } else { "Delete Column" };
    if ui.button(label_c).clicked() {
        run(app, "Delete Column", &|d, h| (c0..=c1).rev().try_for_each(|c| d.table_delete(h, t, Axis::Column, c)));
    }
    ui.separator();
    for (kind, name) in [(HeaderKind::Rows, "Header Rows"), (HeaderKind::Columns, "Header Columns"), (HeaderKind::Footer, "Footer Rows")] {
        let cur = app.doc().map(|d| d.table_header_count(t, kind)).unwrap_or(0);
        ui.menu_button(name, |ui| {
            for n in 0..=5 {
                if ui.radio(cur == n, n.to_string()).clicked() {
                    run(app, name, &|d, h| d.set_table_header_count(h, t, kind, n));
                }
            }
        });
    }
    ui.separator();
    if ui.add_enabled(!categorized && (r1 > r0 || c1 > c0), egui::Button::new("Merge Cells")).clicked() {
        run(app, "Merge Cells", &|d, h| d.merge_cells(h, t, CellRange { row: r0, col: c0, rows: r1 - r0 + 1, cols: c1 - c0 + 1 }));
    }
    if ui.add_enabled(!categorized, egui::Button::new("Unmerge Cells")).clicked() {
        commit(app);
        app.edit("Unmerge Cells", |d, h| d.unmerge_cells(h, t, r0, c0));
    }
    ui.separator();
    let col = sel.cursor.1;
    if ui.add_enabled(!categorized, egui::Button::new(format!("Sort Ascending by Column {}", col_name(col)))).clicked() {
        run(app, "Sort", &|d, h| d.sort_table_rows(h, t, col, true));
    }
    if ui.add_enabled(!categorized, egui::Button::new(format!("Sort Descending by Column {}", col_name(col)))).clicked() {
        run(app, "Sort", &|d, h| d.sort_table_rows(h, t, col, false));
    }
    ui.separator();
    if ui.button("Add Comment…").clicked() {
        commit(app);
        let (r, c) = sel.cursor;
        if let Some(mr) = app.mrow(t, r) {
            let text = app.comments.get(&t).and_then(|m| m.get(&(mr, c))).map(|x| x.0.clone()).unwrap_or_default();
            let author = std::env::var("USER").unwrap_or_else(|_| "Values".into());
            app.comment_dialog = Some((t, mr, c, text, author));
        }
    }
    ui.separator();
    if ui.button("Delete Table").clicked() {
        commit(app);
        if let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) {
            app.edit("Delete Table", |d, h| nm_core::sheets::detach_drawables(d, h, sheet, &[t]));
            app.sel = None;
        }
    }
    ui.separator();
    let shown = app.doc().is_some_and(|d| d.table_name_shown(t));
    if ui.checkbox(&mut shown.clone(), "Title").clicked() {
        commit(app);
        app.edit("Title", |d, h| d.set_table_name_shown(h, t, !shown));
    }
}

const REF_COLORS: [Color32; 6] = [
    Color32::from_rgb(0x2f, 0x7d, 0xf6),
    Color32::from_rgb(0xe6, 0x5c, 0x2e),
    Color32::from_rgb(0x9b, 0x4d, 0xd6),
    Color32::from_rgb(0x1f, 0xa8, 0x8f),
    Color32::from_rgb(0xd6, 0x3d, 0x8c),
    Color32::from_rgb(0xc9, 0x9a, 0x00),
];

/// Cell references in formula text ("B2", "$C$3", "A1:B4"), as inclusive (r0, c0, r1, c1).
pub fn formula_refs(text: &str) -> Vec<(u32, u32, u32, u32)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    // one cell at i: optional $, letters, optional $, digits; not inside a name like "LOG10"
    let cell = |mut i: usize| -> Option<(usize, u32, u32)> {
        if i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
            return None;
        }
        if b.get(i) == Some(&b'$') {
            i += 1;
        }
        let c0 = i;
        while i < b.len() && b[i].is_ascii_uppercase() {
            i += 1;
        }
        if i == c0 || i - c0 > 3 {
            return None;
        }
        let col = b[c0..i].iter().fold(0u32, |a, ch| a * 26 + u32::from(ch - b'A') + 1) - 1;
        if b.get(i) == Some(&b'$') {
            i += 1;
        }
        let d0 = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == d0 || b.get(i).is_some_and(|ch| ch.is_ascii_alphabetic() || *ch == b'(') {
            return None;
        }
        let row: u32 = std::str::from_utf8(&b[d0..i]).ok()?.parse().ok()?;
        (row > 0).then_some((i, row - 1, col))
    };
    let mut in_str = false;
    while i < b.len() {
        if b[i] == b'"' {
            in_str = !in_str;
            i += 1;
            continue;
        }
        if !in_str {
            if let Some((end, r, c)) = cell(i) {
                if b.get(end) == Some(&b':') {
                    if let Some((end2, r2, c2)) = cell(end + 1) {
                        out.push((r.min(r2), c.min(c2), r.max(r2), c.max(c2)));
                        i = end2;
                        continue;
                    }
                }
                out.push((r, c, r, c));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_in_formula_text() {
        assert_eq!(formula_refs("=B2+$C$3*2"), vec![(1, 1, 1, 1), (2, 2, 2, 2)]);
        assert_eq!(formula_refs("=SUM(A1:B4)"), vec![(0, 0, 3, 1)]);
        assert_eq!(formula_refs("=LOG10(A2)&\"B3\""), vec![(1, 0, 1, 0)]);
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(26), "AA");
        assert_eq!(function_token("=SUM(A1, VLO"), Some("VLO"));
        assert_eq!(function_token("=B2"), None);
        assert_eq!(function_token("hello"), None);
        assert_eq!(function_token("=B2×SU"), Some("SU"));
        assert_eq!(function_token("=B2÷"), None);
    }
}

/// A click on a checkbox toggles it; on a star rating sets the stars up to the one clicked
/// (`dx`: sheet points from the cell's left edge).
fn click_control(app: &mut App, t: ObjId, r: u32, c: u32, dx: f32) {
    use nm_core::controls::{self, Control};
    let Some(mr) = app.mrow(t, r) else { return };
    let Some((shown, size)) = app.table(t).map(|(_, td)| {
        let i = (r * td.cols + c) as usize;
        (td.cells.get(i).cloned().unwrap_or_default(), td.cell_styles.get(i).map(|s| s.text.size).unwrap_or(10.0))
    }) else {
        return;
    };
    let Some(ctl) = app.doc().and_then(|d| controls::control(d, t, mr, c, &shown)) else { return };
    let new = match ctl {
        Control::Checkbox(on) => Control::Checkbox(!on),
        // stars are about a font size wide, after the cell's 4 pt padding
        Control::Stars(n) => {
            let k = (((dx - 4.0) / (size * 1.05)).floor() as i64 + 1).clamp(0, 5) as u32;
            Control::Stars(if k == n { k.saturating_sub(1) } else { k })
        }
    };
    if let Some(Err(e)) = app.edit(if matches!(new, Control::Checkbox(_)) { "Checkbox" } else { "Star Rating" }, |d, h| controls::set(d, h, t, mr, c, new)) {
        app.notify(format!("Couldn't change the cell: {e}"));
    }
}

/// The function name being typed at the end of a formula ("=SUM(A1, VLO" -> "VLO").
fn function_token(text: &str) -> Option<&str> {
    if !text.trim_start().starts_with('=') {
        return None;
    }
    // after the last character that can't be part of a name (which may take several bytes: × ÷ ≥)
    let start = text.char_indices().rev().find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '.' || *c == '_')).map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
    let tok = &text[start..];
    // a cell reference ("B2") or a number isn't a function name
    if tok.is_empty() || tok.starts_with(|c: char| c.is_ascii_digit()) || formula_refs(&format!("={tok}")).len() == 1 && tok.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(tok)
}

/// Tab / Shift+Tab: next / previous cell in the row, remembering where the run started.
fn tab_move(app: &mut App, back: bool) {
    let Some(sel) = app.sel else { return };
    if app.canvas.return_col.is_none() {
        app.canvas.return_col = Some(sel.cursor.1);
    }
    move_cursor(app, 0, if back { -1 } else { 1 }, false);
}

/// Return: the next row, in the column a run of Tabs started from (else this column).
fn return_move(app: &mut App) {
    let Some(sel) = app.sel else { return };
    let col = app.canvas.return_col.take().unwrap_or(sel.cursor.1);
    let Some((_, t)) = app.table(sel.table) else { return };
    // from a merged cell, the row below the merge
    let below = t.merge_at(sel.cursor.0, sel.cursor.1).map(|m| m.row + m.rows - 1).unwrap_or(sel.cursor.0);
    let mut r = (below + 1).min(t.rows.saturating_sub(1));
    // step over hidden rows
    while r + 1 < t.rows && t.row_heights.get(r as usize).is_some_and(|h| *h <= 0.0) {
        r += 1;
    }
    let c = col.min(t.cols.saturating_sub(1));
    let (r, c) = app.merge_origin(sel.table, r, c);
    app.sel = Some(Sel::cell(sel.table, r, c));
}

/// Option+arrow: a row above / below or a column before / after the cursor.
fn insert_next_to(app: &mut App, axis: Axis, after: bool) {
    let Some(sel) = app.sel else { return };
    let t = sel.table;
    if axis == Axis::Row && app.categorized(t) {
        app.notify("Rows wait until categories are off.");
        return;
    }
    let (r, c) = sel.cursor;
    let Some(mr) = app.mrow(t, r) else { return };
    let (at, like) = match (axis, after) {
        (Axis::Row, true) => (mr + 1, mr),
        (Axis::Row, false) => (mr, mr),
        (Axis::Column, true) => (c + 1, c),
        (Axis::Column, false) => (c, c),
    };
    let label = if axis == Axis::Row { "Add Row" } else { "Add Column" };
    match app.edit(label, |d, h| d.table_insert(h, t, axis, at, like)) {
        Some(Err(e)) => app.notify(format!("{label}: {e}")),
        Some(Ok(())) => {
            // the cursor goes to the new row / column (Numbers)
            let cur = match (axis, after) {
                (Axis::Row, true) => (r + 1, c),
                (Axis::Row, false) => (r, c),
                (Axis::Column, true) => (r, c + 1),
                (Axis::Column, false) => (r, c),
            };
            app.sel = Some(Sel::cell(t, cur.0, cur.1));
        }
        None => {}
    }
}

/// Keydeck's text caret (see [`Canvas::editing`]).

/// Alignment guides while moving an object (Keydeck's drawing, Keynote's yellow).
fn draw_guides(painter: &egui::Painter, view: &View, guides: &[kn_ui::snap::Guide]) {
    use kn_ui::snap::Guide;
    use nm_core::kn_model::Point;
    // Numbers 15.4's guides: 1 pt #E8CB44 (Mac N16)
    let col = Color32::from_rgb(0xe8, 0xcb, 0x44);
    let st = Stroke::new(1.0, col);
    let at = |p: Point| view.to_screen(Pos2::new(p.x, p.y));
    for g in guides {
        match g {
            Guide::Line(a, b) => {
                painter.line_segment([at(*a), at(*b)], st);
            }
            Guide::Gap { axis, from, to, at: k } => {
                let (a, b) = if *axis == 0 { (Point::new(*from, *k), Point::new(*to, *k)) } else { (Point::new(*k, *from), Point::new(*k, *to)) };
                let (a, b) = (at(a), at(b));
                painter.line_segment([a, b], st);
                let tick = if *axis == 0 { Vec2::new(0.0, 5.0) } else { Vec2::new(5.0, 0.0) };
                painter.line_segment([a - tick, a + tick], st);
                painter.line_segment([b - tick, b + tick], st);
            }
            Guide::Size { axis, bx } => {
                let (a, b) = if *axis == 0 {
                    (Point::new(bx.x0, bx.y0 - 8.0 / view.zoom), Point::new(bx.x1, bx.y0 - 8.0 / view.zoom))
                } else {
                    (Point::new(bx.x0 - 8.0 / view.zoom, bx.y0), Point::new(bx.x0 - 8.0 / view.zoom, bx.y1))
                };
                let (a, b) = (at(a), at(b));
                painter.line_segment([a, b], st);
                let dir = (b - a).normalized() * 5.0;
                let n = Vec2::new(-dir.y, dir.x);
                painter.line_segment([a, a + dir + n], st);
                painter.line_segment([a, a + dir - n], st);
                painter.line_segment([b, b - dir + n], st);
                painter.line_segment([b, b - dir - n], st);
            }
        }
    }
}

/// Numbers' dark rounded label next to the pointer while moving or resizing.
fn tooltip(painter: &egui::Painter, at: Pos2, text: &str) {
    let font = egui::FontId::proportional(12.0);
    let g = painter.layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let r = Rect::from_min_size(at + Vec2::new(16.0, 16.0), g.size() + Vec2::new(14.0, 8.0));
    painter.rect_filled(r, 6.0, Color32::from_black_alpha(200));
    painter.galley(r.min + Vec2::new(7.0, 4.0), g, Color32::WHITE);
}

/// Add (or remove, while they're empty) rows at the bottom and columns at the right.
fn grow_table(app: &mut App, t: ObjId, dr: i32, dc: i32) {
    let Some((rows, cols)) = app.table(t).map(|(_, d)| (d.rows, d.cols)) else { return };
    if dr != 0 && app.categorized(t) {
        app.notify("Rows wait until categories are off.");
        return;
    }
    let empty_row = |app: &App, r: u32| app.table(t).is_some_and(|(_, d)| (0..d.cols).all(|c| d.cells[(r * d.cols + c) as usize].is_empty()));
    let empty_col = |app: &App, c: u32| app.table(t).is_some_and(|(_, d)| (0..d.rows).all(|r| d.cells[(r * d.cols + c) as usize].is_empty()));
    // only empty rows / columns at the end are taken away
    let mut del_r = 0;
    while dr < 0 && del_r < (-dr) as u32 && rows > del_r + 1 && empty_row(app, rows - 1 - del_r) {
        del_r += 1;
    }
    let mut del_c = 0;
    while dc < 0 && del_c < (-dc) as u32 && cols > del_c + 1 && empty_col(app, cols - 1 - del_c) {
        del_c += 1;
    }
    let res = app.edit("Resize Table", |d, h| -> Result<(), nm_core::kn_format::Error> {
        for k in 0..dr.max(0) as u32 {
            d.table_insert(h, t, Axis::Row, rows + k, rows + k - 1)?;
        }
        for k in 0..del_r {
            d.table_delete(h, t, Axis::Row, rows - 1 - k)?;
        }
        for k in 0..dc.max(0) as u32 {
            d.table_insert(h, t, Axis::Column, cols + k, cols + k - 1)?;
        }
        for k in 0..del_c {
            d.table_delete(h, t, Axis::Column, cols - 1 - k)?;
        }
        Ok(())
    });
    if let Some(Err(e)) = res {
        app.notify(format!("Couldn't resize the table: {e}"));
    }
}

/// Move the content of the cells in block `src` (shown rows) so its top-left lands on `to`:
/// formulas move as formulas, values as what they show (Numbers' drag of a selection).
fn move_cells(app: &mut App, t: ObjId, src: (u32, u32, u32, u32), to: (u32, u32)) {
    let mut moved: Vec<((u32, u32), Option<String>, String)> = Vec::new();
    for r in src.0..=src.2 {
        for c in src.1..=src.3 {
            let Some(mr) = app.mrow(t, r) else { continue };
            let shown = app.table(t).and_then(|(_, d)| d.cells.get((r * d.cols + c) as usize).cloned()).unwrap_or_default();
            moved.push(((r - src.0, c - src.1), app.formula_of(t, mr, c), shown));
        }
    }
    let src_cells: Vec<(u32, u32)> = (src.0..=src.2).flat_map(|r| (src.1..=src.3).map(move |c| (r, c))).filter_map(|(r, c)| Some((app.mrow(t, r)?, c))).collect();
    let dst: Vec<(Option<u32>, u32)> = moved.iter().map(|((dr, dc), _, _)| (app.mrow(t, to.0 + dr), to.1 + dc)).collect();
    let res = app.edit("Move Cells", |d, h| -> Result<(), nm_core::kn_format::Error> {
        for &(r, c) in &src_cells {
            d.set_cell(h, t, r, c, &CellInput::Empty)?;
        }
        for (((_, f, shown), (r, c)), _) in moved.iter().zip(dst.iter()).zip(0..) {
            let Some(r) = *r else { continue };
            match f {
                Some(f) => d.set_formula_text(h, t, r, *c, f)?,
                None if !shown.is_empty() => d.set_cell(h, t, r, *c, &CellInput::parse(shown))?,
                None => {}
            }
        }
        Ok(())
    });
    match res {
        Some(Err(e)) => app.notify(format!("Couldn't move the cells: {e}")),
        _ => app.sel = Some(Sel { table: t, anchor: to, cursor: (to.0 + src.2 - src.0, to.1 + src.3 - src.1) }),
    }
}

/// How deep the rulers are (screen points).
const RULER_BAND: f32 = 18.0;

/// View > Show Rulers: along the canvas's top and left, in Settings' units, from the sheet's
/// top-left (Numbers' rulers).
fn draw_rulers(painter: &egui::Painter, view: &View, rect: Rect, units: u8) {
    let band = RULER_BAND;
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + band));
    let left = Rect::from_min_max(rect.min, Pos2::new(rect.min.x + band, rect.max.y));
    let bg = kn_ui::theme::TOOLBAR_BG();
    painter.rect_filled(top, 0.0, bg);
    painter.rect_filled(left, 0.0, bg);
    let edge = Stroke::new(1.0, kn_ui::theme::SEPARATOR());
    painter.line_segment([top.left_bottom(), top.right_bottom()], edge);
    painter.line_segment([left.right_top(), left.right_bottom()], edge);
    let tick = Stroke::new(1.0, kn_ui::theme::TEXT_DIM());
    // points, inches, centimetres (Settings > Rulers)
    let (unit_pt, major, minor) = match units {
        1 => (72.0, 1.0, 0.125),
        2 => (72.0 / 2.54, 1.0, 0.25),
        _ => (1.0, 100.0, 10.0),
    };
    let mut every = 1.0f32;
    while major * every * unit_pt * view.zoom < 40.0 {
        every *= if every == 1.0 { 2.0 } else { 2.5 };
    }
    let minor_pt = minor * unit_pt;
    let per_major = (major / minor).round() as i64;
    let per_label = per_major * every as i64;
    for axis in 0..2 {
        let (a, b) = if axis == 0 { (view.to_sheet(Pos2::new(rect.min.x + band, 0.0)).x, view.to_sheet(Pos2::new(rect.max.x, 0.0)).x) } else { (view.to_sheet(Pos2::new(0.0, rect.min.y + band)).y, view.to_sheet(Pos2::new(0.0, rect.max.y)).y) };
        let (n0, n1) = ((a / minor_pt).floor() as i64, (b / minor_pt).ceil() as i64);
        if n1 - n0 > 6000 {
            continue;
        }
        for n in n0..=n1 {
            let v = n as f32 * minor_pt;
            let is_major = n.rem_euclid(per_major) == 0;
            if !is_major && minor_pt * view.zoom < 4.0 {
                continue;
            }
            let l = if is_major { band * 0.55 } else { band * 0.25 };
            let label = (n.rem_euclid(per_label) == 0).then(|| {
                let val = n as f32 * minor;
                if val.fract().abs() < 0.001 { format!("{val:.0}") } else { format!("{val}") }
            });
            if axis == 0 {
                let x = view.to_screen(Pos2::new(v, 0.0)).x;
                if x < rect.min.x + band {
                    continue;
                }
                painter.line_segment([Pos2::new(x, top.max.y - l), Pos2::new(x, top.max.y)], tick);
                if let Some(t) = label {
                    painter.text(Pos2::new(x + 2.0, top.min.y + 1.0), egui::Align2::LEFT_TOP, t, egui::FontId::proportional(9.0), kn_ui::theme::TEXT_DIM());
                }
            } else {
                let y = view.to_screen(Pos2::new(0.0, v)).y;
                if y < rect.min.y + band {
                    continue;
                }
                painter.line_segment([Pos2::new(left.max.x - l, y), Pos2::new(left.max.x, y)], tick);
                if let Some(t) = label {
                    painter.text(Pos2::new(left.min.x + 1.0, y + 2.0), egui::Align2::LEFT_TOP, t, egui::FontId::proportional(9.0), kn_ui::theme::TEXT_DIM());
                }
            }
        }
    }
    painter.rect_filled(Rect::from_min_size(rect.min, Vec2::splat(band)), 0.0, bg);
}

/// Type `ins` at the caret of the cell being edited (Insert > Line Break, Date & Time).
pub fn cell_insert(app: &mut App, ctx: &egui::Context, ins: &str) -> bool {
    let Some(e) = app.canvas.edit.as_mut() else { return false };
    let id = egui::Id::unique("cell-editor");
    let at = egui::TextEdit::load_state(ctx, id).and_then(|st| st.cursor.char_range()).map(|r| r.primary.index.0).unwrap_or(e.text.chars().count());
    let byte = e.text.char_indices().nth(at).map(|(b, _)| b).unwrap_or(e.text.len());
    e.text.insert_str(byte, ins);
    if let Some(mut st) = egui::TextEdit::load_state(ctx, id) {
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at + ins.chars().count()))));
        st.store(ctx, id);
    }
    true
}

/// Insert > Line > Draw With Pen: drag to draw a freehand line (a path shape in the pen's
/// colour and width when let go); the pen bar picks them, Done or Escape ends it.
fn pen_mode(app: &mut App, ui: &mut egui::Ui, view: &View, resp: &egui::Response) {
    use nm_core::sheets::PathEl;
    let ctx = ui.ctx().clone();
    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    if resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let sp = view.to_sheet(p);
            if let Some(pts) = app.pen.as_mut() {
                // a new sample every couple of screen points
                if pts.last().is_none_or(|q: &Pos2| q.distance(sp) * view.zoom > 2.5) {
                    pts.push(sp);
                }
            }
        }
    }
    if let Some(pts) = &app.pen {
        let line: Vec<Pos2> = pts.iter().map(|p| view.to_screen(*p)).collect();
        if line.len() > 1 {
            ui.painter().add(egui::Shape::line(line, Stroke::new(app.pen_width * view.zoom, app.pen_color)));
        }
    }
    if resp.drag_stopped() {
        let pts = app.pen.replace(Vec::new()).unwrap_or_default();
        if pts.len() > 1 {
            let (x0, y0) = pts.iter().fold((f32::INFINITY, f32::INFINITY), |a, p| (a.0.min(p.x), a.1.min(p.y)));
            let (x1, y1) = pts.iter().fold((f32::NEG_INFINITY, f32::NEG_INFINITY), |a, p| (a.0.max(p.x), a.1.max(p.y)));
            let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
            let els: Vec<PathEl> = pts.iter().enumerate().map(|(i, p)| if i == 0 { PathEl::Move(p.x - x0, p.y - y0) } else { PathEl::Line(p.x - x0, p.y - y0) }).collect();
            let c = app.pen_color;
            let stroke = nm_core::kn_model::format::StrokeSpec {
                color: nm_core::kn_model::Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: 1.0 },
                width: app.pen_width,
                dash: nm_core::kn_model::format::Dash::Solid,
            };
            // (Numbers' square, its outline the stroke, no fill)
            app.insert_shape_then(3, |d, hist, id| {
                d.set_fill(hist, id, &nm_core::kn_model::format::FillSpec::None);
                if let Some(mut g) = d.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry) {
                    g.x = x0;
                    g.y = y0;
                    g.w = w;
                    g.h = h;
                    d.set_frame(hist, id, g);
                }
                nm_core::sheets::set_path(d, hist, id, &els, (w, h));
                d.set_stroke(hist, id, Some(&stroke));
            });
            // keep drawing
            app.pen = Some(Vec::new());
            app.canvas.obj = None;
        }
    }
    // the pen bar: colour, width, Done
    let mut done = ui.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Area::new(egui::Id::unique("pen-bar")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -40.0]).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                for c in [Color32::BLACK, Color32::from_rgb(0x0a, 0x84, 0xff), Color32::from_rgb(0xff, 0x45, 0x3a), Color32::from_rgb(0x30, 0xd1, 0x58), Color32::from_rgb(0xff, 0xd6, 0x0a), Color32::WHITE] {
                    let (r, re) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 9.0, c);
                    ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.0, kn_ui::theme::SEPARATOR()));
                    if app.pen_color == c {
                        ui.painter().circle_stroke(r.center(), 11.0, Stroke::new(2.0, accent()));
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
