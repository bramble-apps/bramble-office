//! Drawing in a note (Notes' Insert Drawing and the Markup button): a drawing block edited in
//! place with macOS Markup's toolbar (markup.rs) — selection and the lasso, the pen, pencil,
//! marker and erasers, shapes, text boxes, signatures, styles and colours, undo / redo, Done —
//! and Markup over a picture, flattened into a new picture on Done. Strokes are the editor's ink
//! model (ink.rs), shapes nt-core's Markup objects (shapes.rs). While a drawing is open its edits
//! are undone one by one in the drawing; the note gets one undo step for the whole visit. A
//! canvas grows downwards as Notes' does when ink nears its bottom.

use egui::{Color32, Pos2, Rect, Stroke as Line, pos2, vec2};

use super::doc::{Att, AttKind};
use super::ink::{self, Builder, Drawing, DrawingExt, InkKind, Shape, ShapeKind, StrokeExt};
use super::layout;
use super::ops::Kind;
use super::shapes::{self, Handle};
use super::{Editor, Loc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Markup's selection arrow: picks, moves and resizes objects and strokes; a drag on nothing
    /// selects what its rectangle touches
    Select,
    Ink(InkKind),
    PixelEraser,
    ObjectEraser,
    /// a freeform lasso (PencilKit's; Notes' Mac palette has only the selection tool, whose
    /// freeform drag NO3 couldn't capture)
    #[allow(dead_code)]
    Lasso,
}

#[derive(Clone, Debug)]
pub struct Ruler {
    /// drawing points
    pub centre: Pos2,
    pub angle: f32,
}

/// A picture being marked up: the drawing over it and the picture.
#[derive(Clone, Debug)]
pub struct Markup {
    pub drawing: Drawing,
    pub picture: std::path::PathBuf,
}

/// The palette's popovers and the selected object's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pop {
    /// the tool picker: sizes and the inks
    Tools,
    /// the ink's colour
    Color,
    Shapes,
    Sign,
    /// the selected object's fill, stroke, text style, opacity, text colour
    Fill,
    Stroke,
    TextStyle,
    Opacity,
    TextColor,
}

/// What the Colors window (Show Colors…, the colour wheel buttons) is colouring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorTarget {
    Ink,
    Fill,
    Stroke,
    Text,
}

/// A handle being dragged: the object, the handle, where the drag began, the object then.
#[derive(Clone, Debug)]
pub struct ShapeDrag {
    pub id: String,
    pub handle: Handle,
    pub from: Pos2,
    pub orig: Shape,
}

/// Create Signature: the strokes drawn on its pad so far (pad points).
#[derive(Clone, Debug, Default)]
pub struct Signing {
    pub strokes: Vec<ink::Stroke>,
    pub building: Option<Builder>,
}

#[derive(Clone, Debug)]
pub struct DrawState {
    /// the note paragraph of the drawing (or picture)
    pub para: usize,
    pub att: String,
    pub markup: Option<Markup>,
    pub tool: Tool,
    /// the ink's colour (the Border Color well while an ink is chosen)
    pub color: [f32; 4],
    /// the chosen width (index into ink::ink_widths) per ink
    pub widths: Vec<(InkKind, usize)>,
    /// the ink the tool button shows (the last one chosen)
    pub last_ink: InkKind,
    /// the ink or eraser the tool button goes back to after the selection tool
    pub pen_tool: Tool,
    /// the eraser's size (index into ERASER_SIZES) and kind (Pixel | Object in the picker)
    pub eraser_size: usize,
    pub eraser_object: bool,
    pub building: Option<Builder>,
    pub lasso: Vec<Pos2>,
    /// the selection rectangle being dragged (Select on nothing)
    pub band: Option<(Pos2, Pos2)>,
    /// selected strokes and objects
    pub selected: Vec<String>,
    pub selected_shapes: Vec<String>,
    pub moving: Option<Pos2>,
    pub shape_drag: Option<ShapeDrag>,
    /// the text box being typed in
    pub text_edit: Option<String>,
    pub erasing: bool,
    /// where this erase gesture last erased (drawing points)
    pub erase_last: Option<Pos2>,
    pub ruler: Option<Ruler>,
    pub ruler_drag: Option<Pos2>,
    /// the style new objects get (and the menus change on the selection)
    pub style: shapes::Style,
    pub pop: Option<Pop>,
    /// where the toolbar, its popover and sheets were drawn (the pointer there isn't drawing)
    pub ui_rects: Vec<Rect>,
    pub colors: Option<ColorTarget>,
    pub color_panel: kn_ui::colors::ColorPanelState,
    pub signing: Option<Signing>,
    /// the drawing as it was before each edit of this visit, and the edits undone (shared: the
    /// state is copied every frame, a long drawing many times over)
    pub undo: Vec<std::sync::Arc<Drawing>>,
    pub redo: Vec<std::sync::Arc<Drawing>>,
    /// the note has its undo step for this visit
    pub noted: bool,
    /// when the palette began to slide in (its first frame)
    pub shown_at: Option<f64>,
}

/// PencilKit's palette colours: black, blue, green, yellow, red.
pub const COLORS: [[f32; 4]; 5] = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.478, 1.0, 1.0], [0.204, 0.78, 0.349, 1.0], [1.0, 0.8, 0.0, 1.0], [1.0, 0.231, 0.188, 1.0]];
/// Ink this close to a canvas' bottom grows it by GROW.
const GROW_NEAR: f32 = 40.0;
const GROW: f32 = 120.0;
/// The eraser's five sizes (radius, screen points).
pub const ERASER_SIZES: [f32; 5] = [3.0, 6.0, 10.0, 16.0, 24.0];
/// Handles take the pointer this near (screen points).
const REACH: f32 = 9.0;

impl DrawState {
    fn new(para: usize, att: String) -> DrawState {
        DrawState {
            para,
            att,
            markup: None,
            // Notes' default: the Monoline, its smallest size, black
            tool: Tool::Ink(InkKind::Monoline),
            color: COLORS[0],
            widths: Vec::new(),
            last_ink: InkKind::Monoline,
            pen_tool: Tool::Ink(InkKind::Monoline),
            // Notes' default: the pixel eraser, its middle size (NO3)
            eraser_size: 2,
            eraser_object: false,
            building: None,
            lasso: Vec::new(),
            band: None,
            selected: Vec::new(),
            selected_shapes: Vec::new(),
            moving: None,
            shape_drag: None,
            text_edit: None,
            erasing: false,
            erase_last: None,
            ruler: None,
            ruler_drag: None,
            style: shapes::Style::default(),
            pop: None,
            ui_rects: Vec::new(),
            colors: None,
            color_panel: Default::default(),
            signing: None,
            undo: Vec::new(),
            redo: Vec::new(),
            noted: false,
            shown_at: None,
        }
    }

    /// A new drawing's tools: as the last drawing left them (this app's own file).
    fn with_prefs(mut self) -> DrawState {
        #[cfg(not(test))]
        if let Some(p) = load_prefs() {
            p.apply(&mut self);
        }
        self
    }

    pub fn prefs(&self) -> ToolPrefs {
        ToolPrefs { tool: self.pen_tool_now(), eraser_size: self.eraser_size, eraser_object: self.eraser_object, last_ink: self.last_ink, widths: self.widths.clone(), color: self.color }
    }

    /// The ink or eraser in use (the selection tool aside).
    fn pen_tool_now(&self) -> Tool {
        if matches!(self.tool, Tool::Select | Tool::Lasso) { self.pen_tool } else { self.tool }
    }

    pub fn width_of(&self, ink: InkKind) -> f32 {
        let i = self.widths.iter().find(|w| w.0 == ink).map_or(0, |w| w.1);
        ink::ink_widths(ink)[i.min(4)]
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.selected_shapes.clear();
        self.text_edit = None;
    }

    pub fn has_selection(&self) -> bool {
        !self.selected.is_empty() || !self.selected_shapes.is_empty()
    }
}

/// Where a drawing's fallback PNG is written (nt-core copies it into the library on save).
/// Each drawing of it is a new file (`<id>-<n>.png`), so what shows it by its path (the note
/// list's thumbnail) picks up the change; the one before goes.
pub fn fallback_png(id: &str, d: &Drawing, old: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let dir = cache_dir()?.join("drawings");
    std::fs::create_dir_all(&dir).ok()?;
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_nanos());
    let p = dir.join(format!("{id}-{n}.png"));
    // (written whole before it has its name: nothing reads half a picture)
    let tmp = p.with_extension("png.part");
    // (2 pixels a point, but at most FALLBACK_PX pixels: a long drawing's picture would be tens
    // of megapixels)
    let scale = (FALLBACK_PX / (d.width * d.height).max(1.0)).sqrt().min(2.0);
    std::fs::write(&tmp, ink::render_png(d, scale, None)).ok()?;
    std::fs::rename(&tmp, &p).ok()?;
    if let Some(o) = old.filter(|o| o.starts_with(&dir) && *o != p) {
        let _ = std::fs::remove_file(o);
    }
    Some(p)
}

/// A drawing's fallback picture drawn on a worker (a long drawing takes most of a second): its
/// attachment keeps the old picture until [`take_pictures`] hands the new one over.
pub struct Picture {
    pub id: String,
    pub drawing: Drawing,
    pub path: std::path::PathBuf,
}

/// The most pixels a fallback picture gets.
const FALLBACK_PX: f32 = 4_000_000.0;

/// Pictures being drawn (attachment, the strokes they're of), those done, and the next one to
/// draw for a drawing whose picture is being drawn (only the newest strokes).
static PICTURES: std::sync::Mutex<(Vec<(String, Drawing)>, Vec<Picture>, Vec<(Option<egui::Context>, String, Drawing)>)> = std::sync::Mutex::new((Vec::new(), Vec::new(), Vec::new()));

/// Whether fallback pictures are drawn at once, in the caller (tests: the picture is there when
/// write_note returns).
pub static PICTURES_AT_ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(cfg!(test));

/// Draws drawing `id`'s fallback picture off the UI thread (once for the same strokes).
pub fn fallback_later(ctx: Option<egui::Context>, id: &str, d: &Drawing) {
    let mut p = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
    if p.0.iter().any(|(i, x)| i == id && x == d) || p.1.iter().any(|x| x.id == id && x.drawing == *d) {
        return;
    }
    // (one picture of a drawing at a time: newer strokes wait, replacing older waiting ones)
    if p.0.iter().any(|(i, _)| i == id) {
        p.2.retain(|(_, i, _)| i != id);
        p.2.push((ctx, id.to_string(), d.clone()));
        return;
    }
    p.0.push((id.to_string(), d.clone()));
    drop(p);
    let (id, d) = (id.to_string(), d.clone());
    // (text boxes are laid out by egui: the editor's context on the worker too)
    let ectx = super::shapes::CTX.with_borrow(|c| c.clone());
    super::media::spawn_job(Box::new(move || {
        if let Some(c) = ectx {
            super::shapes::CTX.with_borrow_mut(|x| *x = Some(c));
        }
        let path = fallback_png(&id, &d, None);
        let mut p = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
        p.0.retain(|(i, x)| !(*i == id && *x == d));
        let next = p.2.iter().position(|(_, i, _)| *i == id).map(|k| p.2.remove(k));
        if let Some(path) = path {
            p.1.push(Picture { id, drawing: d, path });
        }
        drop(p);
        if let Some(c) = &ctx {
            c.request_repaint();
        }
        if let Some((c, i, d)) = next {
            fallback_later(c, &i, &d);
        }
    }));
}

/// The fallback pictures drawn since the last call.
pub fn take_pictures() -> Vec<Picture> {
    std::mem::take(&mut PICTURES.lock().unwrap_or_else(|e| e.into_inner()).1)
}

/// Puts a picture drawn on a worker into its attachment, when that still has the same strokes
/// (true), the old picture removed; a picture of strokes since changed is removed instead.
pub fn apply_picture(pic: &Picture, a: &mut nt_core::model::Attachment) -> bool {
    let dir = cache_dir().map(|d| d.join("drawings"));
    let ours = |p: &std::path::Path| dir.as_ref().is_some_and(|d| p.starts_with(d));
    if a.id != pic.id || a.drawing.as_ref() != Some(&pic.drawing) {
        return false;
    }
    if let Some(old) = a.fallback.replace(pic.path.clone()).filter(|o| *o != pic.path && ours(o)) {
        let _ = std::fs::remove_file(old);
    }
    true
}

/// A picture no attachment took (its strokes changed meanwhile, its note closed): removed.
pub fn drop_picture(pic: Picture) {
    let _ = std::fs::remove_file(pic.path);
}

fn cache_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache"))).map(|d| d.join("bramble-notes"))
}

// ---- the tools, remembered across drawings and launches (this app's own file)

/// What the palette remembers: the ink or eraser, the eraser's kind and size, each ink's size,
/// the colour.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolPrefs {
    pub tool: Tool,
    pub eraser_size: usize,
    pub eraser_object: bool,
    pub last_ink: InkKind,
    pub widths: Vec<(InkKind, usize)>,
    pub color: [f32; 4],
}

impl ToolPrefs {
    fn apply(&self, st: &mut DrawState) {
        st.tool = self.tool;
        st.pen_tool = self.tool;
        st.eraser_size = self.eraser_size.min(4);
        st.eraser_object = self.eraser_object;
        st.last_ink = self.last_ink;
        st.widths = self.widths.clone();
        st.color = self.color;
    }
}

const INK_NAMES: [(InkKind, &str); 8] = [(InkKind::Pen, "pen"), (InkKind::Monoline, "monoline"), (InkKind::Marker, "marker"), (InkKind::Pencil, "pencil"), (InkKind::Crayon, "crayon"), (InkKind::Fountain, "fountain"), (InkKind::Reed, "reed"), (InkKind::Watercolor, "watercolor")];

fn ink_name(k: InkKind) -> &'static str {
    INK_NAMES.iter().find(|n| n.0 == k).map_or("monoline", |n| n.1)
}

fn ink_of(s: &str) -> Option<InkKind> {
    INK_NAMES.iter().find(|n| n.1 == s).map(|n| n.0)
}

fn prefs_path() -> Option<std::path::PathBuf> {
    signature_dir().and_then(|d| d.parent().map(|p| p.join("markup-tools")))
}

/// Reads the remembered tools ("key value" lines).
pub fn load_prefs() -> Option<ToolPrefs> {
    let text = std::fs::read_to_string(prefs_path()?).ok()?;
    let mut p = ToolPrefs { tool: Tool::Ink(InkKind::Monoline), eraser_size: 2, eraser_object: false, last_ink: InkKind::Monoline, widths: Vec::new(), color: COLORS[0] };
    for line in text.lines() {
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match k {
            "tool" => {
                p.tool = match v {
                    "pixel-eraser" => Tool::PixelEraser,
                    "object-eraser" => Tool::ObjectEraser,
                    v => Tool::Ink(ink_of(v).unwrap_or(InkKind::Monoline)),
                }
            }
            "eraser" => p.eraser_object = v == "object",
            "eraser-size" => p.eraser_size = v.parse().unwrap_or(2),
            "ink" => p.last_ink = ink_of(v).unwrap_or(InkKind::Monoline),
            "size" => {
                if let Some((ink, i)) = v.split_once(' ').and_then(|(a, b)| Some((ink_of(a)?, b.parse().ok()?))) {
                    p.widths.push((ink, i));
                }
            }
            "color" => {
                let c: Vec<f32> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let [r, g, b, a] = c[..] {
                    p.color = [r, g, b, a];
                }
            }
            _ => {}
        }
    }
    Some(p)
}

pub fn save_prefs(p: &ToolPrefs) {
    let Some(path) = prefs_path() else { return };
    let tool = match p.tool {
        Tool::PixelEraser => "pixel-eraser".to_string(),
        Tool::ObjectEraser => "object-eraser".to_string(),
        Tool::Ink(k) => ink_name(k).to_string(),
        _ => ink_name(p.last_ink).to_string(),
    };
    let mut out = format!("tool {tool}\neraser {}\neraser-size {}\nink {}\n", if p.eraser_object { "object" } else { "pixel" }, p.eraser_size, ink_name(p.last_ink));
    for (k, i) in &p.widths {
        out += &format!("size {} {i}\n", ink_name(*k));
    }
    out += &format!("color {} {} {} {}\n", p.color[0], p.color[1], p.color[2], p.color[3]);
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, out);
}

// ---- saved signatures (Markup's Sign menu), kept by this app

fn signature_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))).map(|d| d.join("bramble-notes").join("signatures"))
}

/// The saved signatures, oldest first: (file name, strokes).
pub fn signatures() -> Vec<(String, Drawing)> {
    let Some(dir) = signature_dir() else { return Vec::new() };
    let mut v: Vec<(String, Drawing)> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let d = nt_core::paper::decode_ink(&std::fs::read(e.path()).ok()?)?;
        name.ends_with(".ink").then_some((name, d))
    }).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

pub fn save_signature(strokes: &[ink::Stroke]) -> Option<String> {
    let dir = signature_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let b = strokes.iter().fold(Rect::NOTHING, |r, s| r.union(s.bounds()));
    let d = Drawing { width: b.max.x.max(1.0), height: b.max.y.max(1.0), strokes: strokes.to_vec(), ..Default::default() };
    let name = format!("{:013}.ink", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()));
    std::fs::write(dir.join(&name), nt_core::paper::encode_ink(&d)).ok()?;
    Some(name)
}

pub fn delete_signature(name: &str) {
    if let Some(dir) = signature_dir() {
        if !name.contains('/') {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
}

impl Editor {
    /// The drawing being edited.
    pub fn drawing_mut(&mut self) -> Option<&mut Drawing> {
        let st = self.draw.as_mut()?;
        if let Some(m) = st.markup.as_mut() {
            return Some(&mut m.drawing);
        }
        let id = st.att.clone();
        match self.doc.att_mut(&id).map(|a| &mut a.kind) {
            // (shared with undo steps until changed: copied on the first change)
            Some(AttKind::Drawing { ink: Some(d), .. }) => Some(std::sync::Arc::make_mut(d)),
            _ => None,
        }
    }

    pub fn drawing(&self) -> Option<&Drawing> {
        let st = self.draw.as_ref()?;
        if let Some(m) = st.markup.as_ref() {
            return Some(&m.drawing);
        }
        match self.doc.att(&st.att).map(|a| &a.kind) {
            Some(AttKind::Drawing { ink: Some(d), .. }) => Some(d),
            _ => None,
        }
    }

    /// Before an edit of the drawing: the drawing as it is goes on its undo list (and, the first
    /// time this visit, the note gets its undo step).
    pub fn draw_change(&mut self) {
        let Some(d) = self.drawing().cloned() else { return };
        let markup = self.draw.as_ref().is_some_and(|s| s.markup.is_some());
        let noted = self.draw.as_ref().is_some_and(|s| s.noted);
        if !markup && !noted {
            self.push_undo(Kind::Other);
        }
        if let Some(st) = self.draw.as_mut() {
            st.undo.push(std::sync::Arc::new(d));
            if st.undo.len() > 200 {
                st.undo.remove(0);
            }
            st.redo.clear();
            st.noted = true;
        }
    }

    /// Undo / Redo while drawing: the drawing's last edit. False when there is none.
    pub fn draw_undo(&mut self, redo: bool) -> bool {
        let Some(cur) = self.drawing().cloned() else { return false };
        let Some(st) = self.draw.as_mut() else { return false };
        let prev = if redo { st.redo.pop() } else { st.undo.pop() };
        let Some(prev) = prev else { return false };
        let cur = std::sync::Arc::new(cur);
        if redo { st.undo.push(cur) } else { st.redo.push(cur) }
        st.shape_drag = None;
        st.moving = None;
        st.text_edit = None;
        if let Some(d) = self.drawing_mut() {
            *d = std::sync::Arc::unwrap_or_clone(prev);
        }
        // what was selected may be gone
        let (ids, sids): (Vec<String>, Vec<String>) = self.drawing().map(|d| (d.strokes.iter().map(|s| s.id.clone()).collect(), d.shapes.iter().map(|s| s.id.clone()).collect())).unwrap_or_default();
        if let Some(st) = self.draw.as_mut() {
            st.selected.retain(|x| ids.contains(x));
            st.selected_shapes.retain(|x| sids.contains(x));
        }
        self.changed = true;
        true
    }

    /// A click on a drawing in the note (as Notes' in-place drawings): on a shape, text box or
    /// signature, Markup opens with that object selected and picked up (dragging moves it); on
    /// anything else, Markup opens ready to draw. False when `para` isn't a drawing.
    pub fn click_drawing(&mut self, para: usize, p: Pos2, r: Rect) -> bool {
        let Some(id) = self.doc.paras.get(para).and_then(|x| x.sole_att()).map(str::to_string) else { return false };
        let hit = match self.doc.att(&id).map(|a| &a.kind) {
            Some(AttKind::Drawing { ink, .. }) => ink.as_ref().and_then(|d| {
                let k = r.width() / d.width.max(1.0);
                let q = pos2((p.x - r.min.x) / k, (p.y - r.min.y) / k);
                d.shapes.iter().rev().find(|s| shapes::hits(s, q, 5.0 / k)).map(|s| (s.id.clone(), q))
            }),
            _ => return false,
        };
        self.head = Loc { cell: None, para, off: 0 };
        self.anchor = self.head;
        self.markup();
        if let Some((sid, q)) = hit {
            self.draw_change();
            if let Some(st) = self.draw.as_mut() {
                if !matches!(st.tool, Tool::Select | Tool::Lasso) {
                    st.pen_tool = st.tool;
                }
                st.tool = Tool::Select;
                st.selected_shapes = vec![sid];
                st.moving = Some(q);
            }
        }
        true
    }

    /// Format / Attachments > Insert Drawing: a new canvas at the caret, being drawn on.
    pub fn insert_drawing(&mut self) {
        let att = Att::new(nt_core::model::UTI_PAPER, AttKind::Drawing { path: None, width: ink::NEW_W, height: ink::NEW_H, ink: Some(std::sync::Arc::new(Drawing::new())) });
        let id = att.id.clone();
        self.insert_block(att);
        if let Some(para) = self.doc.paras.iter().position(|p| p.sole_att() == Some(id.as_str())) {
            let mut st = DrawState::new(para, id).with_prefs();
            // (the insert itself was the note's undo step)
            st.noted = true;
            self.draw = Some(st);
        }
    }

    /// The Markup button: on a picture, marks it up; on a drawing, draws in it; else a new drawing.
    pub fn markup(&mut self) {
        if self.draw.is_some() {
            self.finish_drawing();
            return;
        }
        let mut para = self.head.cell.map_or(self.head.para, |c| c.para);
        let is_drawing = |ed: &Editor, i: usize| ed.doc.paras.get(i).and_then(|p| p.sole_att()).and_then(|id| ed.doc.att(id)).is_some_and(|a| matches!(a.kind, AttKind::Drawing { .. }));
        // the caret just below a drawing (where Done leaves it): that drawing, not a new one
        if !is_drawing(self, para) && para > 0 && is_drawing(self, para - 1) && self.head.cell.is_none() && self.head.off == 0 {
            para -= 1;
        }
        let att = self.doc.paras.get(para).and_then(|p| p.sole_att()).and_then(|id| self.doc.att(id)).cloned();
        match att.map(|a| (a.id.clone(), a.kind)) {
            Some((id, AttKind::Drawing { ink, path, width, height })) => {
                if ink.is_none() {
                    // an Apple drawing whose strokes can't be read yet: draw over its picture
                    if let Some(p) = path {
                        return self.start_markup(para, id, p, width, height);
                    }
                    let d = Drawing { width: width.max(1.0), height: height.max(1.0), strokes: Vec::new(), ..Default::default() };
                    if let Some(a) = self.doc.att_mut(&id) {
                        a.kind = AttKind::Drawing { path: None, width, height, ink: Some(std::sync::Arc::new(d)) };
                    }
                }
                self.draw = Some(DrawState::new(para, id).with_prefs());
            }
            Some((id, AttKind::Image { path: Some(p), width, height, .. })) => self.start_markup(para, id, p, width, height),
            _ => self.insert_drawing(),
        }
    }

    fn start_markup(&mut self, para: usize, id: String, picture: std::path::PathBuf, w: f32, h: f32) {
        let (w, h) = image::image_dimensions(&picture).map(|(a, b)| (a as f32, b as f32)).unwrap_or((w.max(1.0), h.max(1.0)));
        let mut st = DrawState::new(para, id).with_prefs();
        st.markup = Some(Markup { drawing: Drawing { width: w, height: h, strokes: Vec::new(), ..Default::default() }, picture });
        self.draw = Some(st);
    }

    /// Done: back to the text; Markup is flattened into a new picture (the old one stays for undo).
    pub fn finish_drawing(&mut self) {
        self.end_text_edit();
        let Some(st) = self.draw.take() else { return };
        // the palette slides out (its popovers close at once), from where it is now
        let hidden = super::markup::hidden_share(st.shown_at, self.now);
        let mut ghost = st.clone();
        ghost.pop = None;
        ghost.signing = None;
        ghost.colors = None;
        ghost.clear_selection();
        self.markup_exit = Some((ghost, super::markup::exit_start(hidden, self.now)));
        if let Some(m) = st.markup {
            if m.drawing.strokes.is_empty() && m.drawing.shapes.is_empty() {
                return;
            }
            let Ok(img) = image::open(&m.picture).map(|i| i.to_rgba8()) else { return };
            let png = ink::render_png(&m.drawing, 1.0, Some(&img));
            let Some(dir) = cache_dir().map(|d| d.join("markup")) else { return };
            let _ = std::fs::create_dir_all(&dir);
            let mut att = Att::new("public.png", AttKind::Image { path: None, width: m.drawing.width, height: m.drawing.height, view: nt_core::model::ViewSize::Large, name: String::new() });
            let path = dir.join(format!("{}.png", att.id));
            if std::fs::write(&path, png).is_err() {
                return;
            }
            att.kind = AttKind::Image { path: Some(path), width: m.drawing.width, height: m.drawing.height, view: nt_core::model::ViewSize::Large, name: String::new() };
            self.push_undo(Kind::Other);
            let new = att.id.clone();
            self.doc.atts.push(att);
            if let Some(p) = self.doc.paras.get_mut(st.para) {
                let n = p.len();
                p.set(0..n, &|f: &mut super::doc::Fmt| f.att = Some(new.clone()));
            }
            self.changed = true;
        }
        // a drawing left with nothing in it goes from the note (Undo brings it back)
        let empty = self.doc.att(&st.att).is_some_and(|a| matches!(&a.kind, AttKind::Drawing { ink: Some(d), .. } if d.strokes.is_empty() && d.shapes.is_empty()));
        if empty && self.doc.paras.get(st.para).and_then(|p| p.sole_att()) == Some(st.att.as_str()) {
            if !st.noted {
                self.push_undo(Kind::Other);
            }
            self.doc.paras.remove(st.para);
            if self.doc.paras.is_empty() {
                self.doc.paras.push(super::doc::Para::default());
            }
            let l = Loc { cell: None, para: st.para.min(self.doc.paras.len() - 1), off: 0 };
            self.anchor = l;
            self.head = l;
            self.changed = true;
            return;
        }
        // (written again: the drawing's picture is drawn now it is closed)
        self.changed = true;
        let l = Loc { cell: None, para: (st.para + 1).min(self.doc.paras.len() - 1), off: 0 };
        self.anchor = l;
        self.head = l;
    }

    /// Adds a finished stroke (one undo step); a canvas near its bottom grows.
    fn add_stroke(&mut self, s: ink::Stroke) {
        self.draw_change();
        let markup = self.draw.as_ref().is_some_and(|d| d.markup.is_some());
        if let Some(d) = self.drawing_mut() {
            d.strokes.push(s);
            if !markup {
                grow_to_fit(d);
            }
        }
        self.changed = true;
    }

    /// Shapes / Text / a signature from the toolbar: the object in the middle of what shows of the
    /// canvas, selected, with the selection arrow.
    pub fn add_shape(&mut self, mut s: Shape) {
        self.end_text_edit();
        let Some(d) = self.drawing() else { return };
        let (w, h) = (d.width, d.height);
        // the middle of the canvas' part on screen
        let c = match (canvas(self), self.view.rect) {
            (Some((r, k)), Some(v)) => {
                let vis = r.intersect(v);
                if vis.is_positive() { pos2((vis.center().x - r.min.x) / k, (vis.center().y - r.min.y) / k) } else { pos2(w / 2.0, h / 2.0) }
            }
            _ => pos2(w / 2.0, h / 2.0),
        };
        // as big on screen as on a canvas at its own size (a large photo shows small)
        if let Some((_, k)) = canvas(self) {
            let k = 1.0 / k.max(0.01);
            if (k - 1.0).abs() > 0.05 {
                let o = shapes::centre(&s);
                s.frame = [o.x - s.frame[2] * k / 2.0, o.y - s.frame[3] * k / 2.0, s.frame[2] * k, s.frame[3] * k];
                for p in &mut s.points {
                    *p = [o.x + (p[0] - o.x) * k, o.y + (p[1] - o.y) * k];
                }
                s.line_width *= k;
                s.font_size *= k;
            }
        }
        let dc = c - shapes::centre(&s);
        shift_shape(&mut s, dc);
        if s.kind == ShapeKind::Text {
            if let Some(ctx) = shapes::CTX.with_borrow(|c| c.clone()) {
                s.frame[3] = shapes::text_height(&ctx, &s);
            }
        }
        // whole, inside the canvas: never half off an edge (the canvas' visible part may be a
        // sliver at the window's bottom)
        let markup = self.draw.as_ref().is_some_and(|d| d.markup.is_some());
        let fix = inside(shapes::bounds(&s), w, h, markup);
        shift_shape(&mut s, fix);
        self.draw_change();
        let id = s.id.clone();
        let text = s.kind == ShapeKind::Text;
        if let Some(d) = self.drawing_mut() {
            d.shapes.push(s);
            if !markup {
                grow_to_fit(d);
            }
        }
        if let Some(st) = self.draw.as_mut() {
            // selected, the tool as it was (Notes)
            st.clear_selection();
            st.selected_shapes.push(id.clone());
            st.pop = None;
            if text {
                st.text_edit = Some(id);
            }
        }
        self.changed = true;
    }

    /// Applies a style change to the selected objects (and to what comes next).
    pub fn restyle(&mut self, f: &dyn Fn(&mut shapes::Style, Option<&mut Shape>)) {
        let Some(st) = self.draw.as_mut() else { return };
        f(&mut st.style, None);
        let sel = st.selected_shapes.clone();
        if sel.is_empty() {
            return;
        }
        self.draw_change();
        let ctx = shapes::CTX.with_borrow(|c| c.clone());
        if let Some(d) = self.drawing_mut() {
            for s in d.shapes.iter_mut().filter(|s| sel.contains(&s.id)) {
                let mut dummy = shapes::Style::default();
                f(&mut dummy, Some(s));
                if s.kind == ShapeKind::Text {
                    if let Some(ctx) = &ctx {
                        s.frame[3] = shapes::text_height(ctx, s);
                    }
                }
            }
        }
        self.changed = true;
    }

    /// Delete: the selected strokes and objects go.
    pub fn delete_drawing_selection(&mut self) {
        let Some(st) = self.draw.as_ref() else { return };
        if !st.has_selection() || st.text_edit.is_some() {
            return;
        }
        let (sel, ssel) = (st.selected.clone(), st.selected_shapes.clone());
        self.draw_change();
        if let Some(d) = self.drawing_mut() {
            d.strokes.retain(|s| !sel.contains(&s.id));
            d.shapes.retain(|s| !ssel.contains(&s.id));
        }
        if let Some(st) = self.draw.as_mut() {
            st.clear_selection();
        }
        self.changed = true;
    }

    /// Typing in a text box ends: an empty one goes.
    pub fn end_text_edit(&mut self) {
        let Some(id) = self.draw.as_mut().and_then(|s| s.text_edit.take()) else { return };
        let empty = self.drawing().and_then(|d| d.shapes.iter().find(|s| s.id == id)).is_some_and(|s| s.kind == ShapeKind::Text && s.text.trim().is_empty());
        if empty {
            if let Some(d) = self.drawing_mut() {
                d.shapes.retain(|s| s.id != id);
            }
            if let Some(st) = self.draw.as_mut() {
                st.selected_shapes.retain(|s| *s != id);
            }
        }
        self.changed = true;
    }
}

fn shift_shape(s: &mut Shape, d: egui::Vec2) {
    s.frame[0] += d.x;
    s.frame[1] += d.y;
    for p in &mut s.points {
        p[0] += d.x;
        p[1] += d.y;
    }
}

/// A drawing grows downwards (in GROW steps, as Notes' does) until its lowest ink or object is
/// GROW_NEAR above its bottom; it never clips one.
pub fn grow_to_fit(d: &mut Drawing) {
    let low = d.strokes.iter().map(|s| s.bounds().max.y).chain(d.shapes.iter().map(|s| shapes::bounds(s).max.y)).fold(0.0f32, f32::max);
    while low > d.height - GROW_NEAR {
        d.height += GROW;
    }
}

/// The shift that brings `b` back inside the canvas' sides and top (and bottom, on a picture);
/// what is wider than the canvas lines up with its left.
fn inside(b: Rect, w: f32, h: f32, markup: bool) -> egui::Vec2 {
    if !b.is_finite() {
        return vec2(0.0, 0.0);
    }
    let x = if b.min.x < 0.0 || b.width() > w { -b.min.x } else if b.max.x > w { w - b.max.x } else { 0.0 };
    let y = if b.min.y < 0.0 { -b.min.y } else if markup && b.max.y > h { (h - b.max.y).max(-b.min.y) } else { 0.0 };
    vec2(x, y)
}

/// How far the selection may move by `dv` and stay whole on the canvas: not past its left, right
/// or top edges; past the bottom a drawing grows (a picture being marked up doesn't).
fn clamp_move(b: Rect, dv: egui::Vec2, w: f32, h: f32, markup: bool) -> egui::Vec2 {
    if !b.is_finite() {
        return dv;
    }
    let x = if b.width() > w { 0.0 } else { dv.x.clamp(-b.min.x.max(0.0), (w - b.max.x).max(0.0)) };
    let y = dv.y.max(-b.min.y.max(0.0));
    let y = if markup { y.min((h - b.max.y).max(0.0)) } else { y };
    vec2(x, y)
}

/// The drawing's (or picture's) rectangle on screen and its points-to-screen scale.
pub fn canvas(ed: &Editor) -> Option<(Rect, f32)> {
    let st = ed.draw.as_ref()?;
    let r = ed.block_rect(st.para)?;
    let d = ed.drawing()?;
    Some((r, r.width() / d.width.max(1.0)))
}

/// Pointer and keys while drawing. True when it took the pointer (the text doesn't get it).
pub fn input(ed: &mut Editor, ui: &egui::Ui, resp: &egui::Response) -> bool {
    shapes::CTX.with_borrow_mut(|c| *c = Some(ui.ctx().clone()));
    ed.now = ui.ctx().input(|i| i.time);
    if ed.draw.is_none() {
        return false;
    }
    let ctx = ui.ctx().clone();
    // the drawing's paragraph may have moved (an edit above it)
    if let Some(st) = ed.draw.as_mut() {
        if ed.doc.paras.get(st.para).and_then(|p| p.sole_att()) != Some(st.att.as_str()) {
            match ed.doc.paras.iter().position(|p| p.sole_att() == Some(st.att.as_str())) {
                Some(i) => st.para = i,
                None => {
                    ed.draw = None;
                    return false;
                }
            }
        }
    }
    let Some((rect, scale)) = canvas(ed) else { return false };
    let to_canvas = |p: Pos2| pos2((p.x - rect.min.x) / scale, (p.y - rect.min.y) / scale);
    let (pos, pressed, down, released, time, force, mods, moves, dt, dclick) = ctx.input(|i| {
        let force = i.events.iter().rev().find_map(|e| match e {
            egui::Event::Touch { force, .. } => *force,
            _ => None,
        });
        // every pointer sample since the last frame, not only the latest: a full-rate line
        let moves: Vec<Pos2> = i.events.iter().filter_map(|e| match e {
            egui::Event::PointerMoved(p) => Some(*p),
            _ => None,
        }).collect();
        (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released(), i.time, force, i.modifiers, moves, i.stable_dt as f64, i.pointer.button_double_clicked(egui::PointerButton::Primary))
    });
    keys(ed, &ctx);
    if ed.draw.is_none() {
        return true;
    }
    if ed.draw.as_ref().is_some_and(|s| s.signing.is_some()) {
        // Create Signature's sheet has the pointer
        return true;
    }
    let Some(p) = pos else { return false };
    // nothing is drawn until the palette has slid in (a press then is the drawing's, unused)
    if pressed && super::markup::hidden_share(ed.draw.as_ref().and_then(|s| s.shown_at), time) > 0.0 && rect.contains(p) {
        return true;
    }
    let busy = ed.draw.as_ref().is_some_and(|d| d.building.is_some() || d.moving.is_some() || d.erasing || !d.lasso.is_empty() || d.ruler_drag.is_some() || d.band.is_some() || d.shape_drag.is_some());
    let on_ui = ed.draw.as_ref().is_some_and(|d| d.ui_rects.iter().any(|r| r.expand(2.0).contains(p)));
    // over the drawing the eraser shows as its ring, not the pointer
    if !on_ui && rect.contains(p) && ed.draw.as_ref().is_some_and(|d| matches!(d.tool, Tool::PixelEraser | Tool::ObjectEraser)) {
        ctx.set_cursor_icon(egui::CursorIcon::None);
        ctx.request_repaint();
    }
    if on_ui && !busy {
        return true;
    }
    if dclick && resp.hovered() && rect.contains(p) {
        // a double click on a text box types in it
        let q = to_canvas(p);
        // (a shape takes text too, in its middle)
        let hit = ed.drawing().and_then(|d| d.shapes.iter().rev().find(|s| !shapes::is_line(s) && s.kind != ShapeKind::Signature && shapes::hits(s, q, 2.0)).map(|s| s.id.clone()));
        if let (Some(id), Some(st)) = (hit, ed.draw.as_mut()) {
            st.selected.clear();
            st.selected_shapes = vec![id.clone()];
            st.text_edit = Some(id);
            st.moving = None;
            st.shape_drag = None;
            return true;
        }
    }
    if pressed && resp.hovered() {
        let q = to_canvas(p);
        // a press beside the text box being typed in ends the typing
        if let Some(id) = ed.draw.as_ref().and_then(|s| s.text_edit.clone()) {
            let inside = ed.drawing().and_then(|d| d.shapes.iter().find(|s| s.id == id)).is_some_and(|s| shapes::hits(s, q, 4.0 / scale));
            if inside {
                return true;
            }
            ed.end_text_edit();
        }
        if let Some(st) = ed.draw.as_mut() {
            st.pop = None;
        }
        // a selected object's handles, wherever the tool
        if let Some((id, h)) = handle_under(ed, q, scale) {
            {
                ed.draw_change();
                let orig = ed.drawing().and_then(|d| d.shapes.iter().find(|s| s.id == id).cloned());
                if let (Some(orig), Some(st)) = (orig, ed.draw.as_mut()) {
                    if h == Handle::Move {
                        st.moving = Some(q);
                    } else {
                        st.shape_drag = Some(ShapeDrag { id, handle: h, from: q, orig });
                    }
                }
                return true;
            }
        }
        if !rect.contains(p) {
            // a press outside the drawing ends it (Notes)
            ed.finish_drawing();
            return false;
        }
        let st = ed.draw.as_mut().unwrap();
        // the ruler: dragged by its middle
        if let Some(r) = &st.ruler {
            if on_ruler(r, q, 0.0) {
                st.ruler_drag = Some(q);
                return true;
            }
        }
        match st.tool {
            Tool::Ink(kind) => {
                let w = st.width_of(kind);
                let q = st.ruler.as_ref().map_or(q, |r| ink::snap_to_ruler(q, r.centre, r.angle, RULER_W / 2.0, 24.0));
                st.clear_selection();
                st.building = Some(Builder::new(kind, st.color, w, q, time, force));
            }
            Tool::PixelEraser | Tool::ObjectEraser => {
                st.erasing = true;
                ed.draw_change();
                erase(ed, q, scale);
            }
            Tool::Select => select_press(ed, q, scale, mods.shift || mods.command),
            Tool::Lasso => {
                if selection_bounds(ed).is_some_and(|b| b.expand(6.0 / scale).contains(q)) {
                    ed.draw_change();
                    ed.draw.as_mut().unwrap().moving = Some(q);
                } else {
                    let st = ed.draw.as_mut().unwrap();
                    st.clear_selection();
                    st.lasso = vec![q];
                }
            }
        }
        return true;
    }
    if down {
        let q = to_canvas(p);
        let st = ed.draw.as_mut().unwrap();
        if let Some(a) = st.ruler_drag {
            if let Some(r) = st.ruler.as_mut() {
                if mods.shift {
                    // Shift-drag turns it
                    r.angle += (q.x - a.x) * 0.01;
                } else {
                    r.centre += q - a;
                }
            }
            st.ruler_drag = Some(q);
            ctx.request_repaint();
            return true;
        }
        if let Some(b) = st.building.as_mut() {
            // the samples spread over the time since the last frame
            let n = moves.len().max(1);
            let samples = if moves.is_empty() { vec![p] } else { moves };
            for (k, m) in samples.into_iter().enumerate() {
                let q = to_canvas(m);
                let q = st.ruler.as_ref().map_or(q, |r| ink::snap_to_ruler(q, r.centre, r.angle, RULER_W / 2.0, 24.0));
                b.add(q, time - dt * (n - 1 - k) as f64 / n as f64, force);
            }
            ctx.request_repaint();
            return true;
        }
        if st.erasing {
            for m in if moves.is_empty() { vec![p] } else { moves } {
                erase(ed, to_canvas(m), scale);
            }
            ctx.request_repaint();
            return true;
        }
        if let Some(sd) = st.shape_drag.clone() {
            let ctx2 = ctx.clone();
            let markup = st.markup.is_some();
            if let Some(d) = ed.drawing_mut() {
                // a handle stays on the canvas (a drawing grows below), so the object stays whole
                let q = pos2(q.x.clamp(0.0, d.width), if markup { q.y.clamp(0.0, d.height) } else { q.y.max(0.0) });
                if let Some(s) = d.shapes.iter_mut().find(|s| s.id == sd.id) {
                    shapes::drag(s, &sd.orig, sd.handle, sd.from, q, mods.shift);
                    if s.kind == ShapeKind::Text {
                        s.frame[3] = shapes::text_height(&ctx2, s);
                    }
                    // (a turned box's corner can still reach past an edge: brought back in)
                    let fix = inside(shapes::bounds(s), d.width, d.height, markup);
                    shift_shape(s, fix);
                }
                if !markup {
                    grow_to_fit(d);
                }
            }
            ctx.request_repaint();
            return true;
        }
        if let Some(a) = st.moving {
            let (sel, ssel) = (st.selected.clone(), st.selected_shapes.clone());
            let markup = st.markup.is_some();
            // whole on the canvas: the selection stops at the sides and the top (it catches the
            // pointer up again when it comes back), the drawing grows under it
            let dv = match (selection_bounds(ed), ed.drawing()) {
                (Some(b), Some(d)) => clamp_move(b, q - a, d.width, d.height, markup),
                _ => q - a,
            };
            if let Some(st) = ed.draw.as_mut() {
                st.moving = Some(a + dv);
            }
            if let Some(d) = ed.drawing_mut() {
                for s in d.strokes.iter_mut().filter(|s| sel.contains(&s.id)) {
                    s.moved(dv);
                }
                for s in d.shapes.iter_mut().filter(|s| ssel.contains(&s.id)) {
                    shift_shape(s, dv);
                }
                if !markup {
                    grow_to_fit(d);
                }
            }
            ctx.request_repaint();
            return true;
        }
        if let Some(b) = st.band.as_mut() {
            b.1 = q;
            ctx.request_repaint();
            return true;
        }
        if !st.lasso.is_empty() {
            if st.lasso.last().is_none_or(|l| (*l - q).length() > 2.0) {
                st.lasso.push(q);
            }
            ctx.request_repaint();
            return true;
        }
    }
    if released {
        let st = ed.draw.as_mut().unwrap();
        st.ruler_drag = None;
        // a move, a reshape or an erase is written to the note once, when it ends
        st.erase_last = None;
        if std::mem::take(&mut st.erasing) | st.moving.take().is_some() | st.shape_drag.take().is_some() {
            ed.changed = true;
        }
        let st = ed.draw.as_mut().unwrap();
        if let Some(b) = st.building.take() {
            let s = b.finish();
            ed.add_stroke(s);
            return true;
        }
        if let Some((a, b)) = st.band.take() {
            let r = Rect::from_two_pos(a, b);
            let (strokes, objs) = ed.drawing().map(|d| (d.strokes.iter().filter(|s| s.bounds().intersects(r)).map(|s| s.id.clone()).collect(), d.shapes.iter().filter(|s| shapes::bounds(s).intersects(r)).map(|s| s.id.clone()).collect())).unwrap_or_default();
            let st = ed.draw.as_mut().unwrap();
            st.selected = strokes;
            st.selected_shapes = objs;
            return true;
        }
        if !st.lasso.is_empty() {
            let poly = std::mem::take(&mut st.lasso);
            let (sel, objs) = ed.drawing().map(|d| (ink::lasso(&d.strokes, &poly), d.shapes.iter().filter(|s| ink::inside(&poly, shapes::centre(s))).map(|s| s.id.clone()).collect())).unwrap_or_default();
            let st = ed.draw.as_mut().unwrap();
            st.selected = sel;
            st.selected_shapes = objs;
            return true;
        }
    }
    rect.contains(p) || busy
}

/// A selected object's handle (or body) under `q`: the object on its own selected.
fn handle_under(ed: &Editor, q: Pos2, scale: f32) -> Option<(String, Handle)> {
    let st = ed.draw.as_ref()?;
    let d = ed.drawing()?;
    if st.selected_shapes.len() == 1 && st.selected.is_empty() {
        let s = d.shapes.iter().find(|s| s.id == st.selected_shapes[0])?;
        return shapes::handle_at(s, q, REACH / scale, scale).map(|h| (s.id.clone(), h));
    }
    None
}

/// The selection arrow pressed at `q`: an object or stroke there is selected (Shift adds it) and
/// moved; inside the selection, it moves; on nothing, a selection rectangle.
fn select_press(ed: &mut Editor, q: Pos2, scale: f32, add: bool) {
    let slack = 5.0 / scale;
    let hit_shape = ed.drawing().and_then(|d| d.shapes.iter().rev().find(|s| shapes::hits(s, q, slack)).map(|s| s.id.clone()));
    let hit_stroke = if hit_shape.is_none() { ed.drawing().and_then(|d| d.strokes.iter().rev().find(|s| s.hits(q, slack)).map(|s| s.id.clone())) } else { None };
    let st = ed.draw.as_mut().unwrap();
    match (hit_shape, hit_stroke) {
        (Some(id), _) => {
            if !st.selected_shapes.contains(&id) {
                if !add {
                    st.clear_selection();
                }
                st.selected_shapes.push(id);
            }
        }
        (None, Some(id)) => {
            if !st.selected.contains(&id) {
                if !add {
                    st.clear_selection();
                }
                st.selected.push(id);
            }
        }
        (None, None) => {
            if !selection_bounds(ed).is_some_and(|b| b.contains(q)) {
                let st = ed.draw.as_mut().unwrap();
                st.clear_selection();
                st.band = Some((q, q));
                return;
            }
        }
    }
    ed.draw_change();
    ed.draw.as_mut().unwrap().moving = Some(q);
}

/// Keys while drawing: Esc ends the typing, then the selection, then the drawing; Delete takes
/// the selection; the arrows nudge it; Ctrl+Z / Shift+Ctrl+Z undo and redo in the drawing.
fn keys(ed: &mut Editor, ctx: &egui::Context) {
    let typing = ed.draw.as_ref().is_some_and(|s| s.text_edit.is_some() || s.signing.is_some());
    let keys = ctx.input(|i| i.events.iter().filter_map(|e| match e {
        egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
        _ => None,
    }).collect::<Vec<_>>());
    for (k, m) in keys {
        match k {
            egui::Key::Escape => {
                let st = ed.draw.as_mut().unwrap();
                if st.signing.is_some() {
                    st.signing = None;
                } else if st.text_edit.is_some() {
                    ed.end_text_edit();
                } else if st.pop.is_some() {
                    st.pop = None;
                } else if st.has_selection() {
                    st.clear_selection();
                } else {
                    ed.finish_drawing();
                    return;
                }
            }
            _ if typing => {}
            egui::Key::Backspace | egui::Key::Delete => ed.delete_drawing_selection(),
            egui::Key::Z if m.command => {
                ed.draw_undo(m.shift);
            }
            egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown => {
                let st = ed.draw.as_ref().unwrap();
                if !st.has_selection() {
                    continue;
                }
                let step = if m.shift { 10.0 } else { 1.0 };
                let dv = match k {
                    egui::Key::ArrowLeft => vec2(-step, 0.0),
                    egui::Key::ArrowRight => vec2(step, 0.0),
                    egui::Key::ArrowUp => vec2(0.0, -step),
                    _ => vec2(0.0, step),
                };
                let (sel, ssel) = (st.selected.clone(), st.selected_shapes.clone());
                let markup = st.markup.is_some();
                let dv = match (selection_bounds(ed), ed.drawing()) {
                    (Some(b), Some(d)) => clamp_move(b, dv, d.width, d.height, markup),
                    _ => dv,
                };
                ed.draw_change();
                if let Some(d) = ed.drawing_mut() {
                    for s in d.strokes.iter_mut().filter(|s| sel.contains(&s.id)) {
                        s.moved(dv);
                    }
                    for s in d.shapes.iter_mut().filter(|s| ssel.contains(&s.id)) {
                        shift_shape(s, dv);
                    }
                    if !markup {
                        grow_to_fit(d);
                    }
                }
                ed.changed = true;
            }
            _ => {}
        }
    }
}

/// The object eraser's reach (screen points): it takes what it touches, whatever the size.
const OBJECT_REACH: f32 = 3.0;

/// The eraser's radius on screen: the chosen size for the pixel eraser, a touch for the object one.
pub fn eraser_radius(st: &DrawState) -> f32 {
    if st.tool == Tool::ObjectEraser { OBJECT_REACH } else { ERASER_SIZES[st.eraser_size.min(4)] }
}

/// Erases from the last place this gesture erased to `q`, in steps of half the eraser, so a
/// quick sweep leaves nothing between samples.
fn erase(ed: &mut Editor, q: Pos2, scale: f32) {
    let Some(st) = ed.draw.as_mut() else { return };
    let pixel = st.tool == Tool::PixelEraser;
    let r = eraser_radius(st) / scale.max(0.1);
    let from = st.erase_last.replace(q).unwrap_or(q);
    let n = (((q - from).length() / (r / 2.0).max(0.5)).ceil() as usize).max(1);
    if let Some(d) = ed.drawing_mut() {
        for k in 1..=n {
            let at = from + (q - from) * (k as f32 / n as f32);
            if pixel {
                ink::erase_at(&mut d.strokes, at, r);
            } else {
                d.strokes.retain(|s| !s.hits(at, r));
                // the object eraser takes Markup objects too
                d.shapes.retain(|s| !shapes::hits(s, at, r));
            }
        }
    }
}

fn selection_bounds(ed: &Editor) -> Option<Rect> {
    let st = ed.draw.as_ref()?;
    if !st.has_selection() {
        return None;
    }
    let d = ed.drawing()?;
    let mut r = Rect::NOTHING;
    for s in d.strokes.iter().filter(|s| st.selected.contains(&s.id)) {
        r = r.union(s.bounds());
    }
    for s in d.shapes.iter().filter(|s| st.selected_shapes.contains(&s.id)) {
        r = r.union(shapes::bounds(s));
    }
    r.is_finite().then_some(r)
}

pub const RULER_W: f32 = 60.0;

fn on_ruler(r: &Ruler, q: Pos2, slack: f32) -> bool {
    let dir = vec2(r.angle.cos(), r.angle.sin());
    let nrm = vec2(-dir.y, dir.x);
    let rel = q - r.centre;
    rel.dot(nrm).abs() < RULER_W / 2.0 - 6.0 + slack
}

/// Draws the strokes and objects of a drawing block (and, while it is edited, the ink in
/// progress, the lasso, the selection and the ruler). `under` is the picture being marked up.
pub fn paint_drawing(ed: &Editor, painter: &egui::Painter, id: &str, r: Rect, under: Option<egui::TextureId>) {
    let editing = ed.draw.as_ref().filter(|d| d.att == id);
    let d = match (editing.and_then(|e| e.markup.as_ref()), ed.doc.att(id).map(|a| &a.kind)) {
        (Some(m), _) => Some(&m.drawing),
        (None, Some(AttKind::Drawing { ink: Some(d), .. })) => Some(&**d),
        _ => None,
    };
    let Some(d) = d else { return };
    let scale = r.width() / d.width.max(1.0);
    let to_screen = |p: Pos2| r.min + p.to_vec2() * scale;
    let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
    let typing = editing.and_then(|e| e.text_edit.as_deref());
    let under = under.map(|t| (t, vec2(d.width, d.height)));
    // the shapes, then the ink over them (as Apple's drawings stack them)
    for s in &d.shapes {
        if typing == Some(s.id.as_str()) {
            // (its text is in the text field over it)
            let mut s = s.clone();
            s.text.clear();
            shapes::paint(&clip, &s, &to_screen, scale, under);
        } else {
            shapes::paint(&clip, s, &to_screen, scale, under);
        }
    }
    paint_cached(&clip, &d.strokes, r.min, scale);
    let Some(st) = editing else { return };
    if let Some(b) = &st.building {
        ink::paint_stroke(&clip, &b.shown(), &to_screen, scale);
    }
    // the eraser under the pointer: a ring its size (the pointer itself is hidden there)
    if matches!(st.tool, Tool::PixelEraser | Tool::ObjectEraser) {
        let over_ui = |q: Pos2| st.ui_rects.iter().any(|u| u.contains(q));
        if let Some(q) = painter.ctx().input(|i| i.pointer.hover_pos()).filter(|q| r.contains(*q) && !over_ui(*q)) {
            let rad = eraser_radius(st);
            clip.circle_filled(q, rad, Color32::from_white_alpha(40));
            clip.circle_stroke(q, rad + 0.5, Line::new(1.0, Color32::from_white_alpha(200)));
            clip.circle_stroke(q, rad, Line::new(1.0, Color32::from_gray(110)));
        }
    }
    let accent = layout::accent();
    if st.lasso.len() > 1 {
        let pts: Vec<Pos2> = st.lasso.iter().map(|p| to_screen(*p)).collect();
        clip.add(egui::Shape::dashed_line(&pts, Line::new(1.2, accent), 5.0, 4.0));
    }
    if let Some((a, b)) = st.band {
        let br = Rect::from_two_pos(to_screen(a), to_screen(b));
        clip.rect_filled(br, 0.0, Color32::from_rgba_unmultiplied(10, 132, 255, 30));
        clip.rect_stroke(br, 0.0, Line::new(1.0, Color32::from_rgb(10, 132, 255)), egui::StrokeKind::Inside);
    }
    // the selection: one object shows its handles; strokes or several things a dashed box
    let one = st.selected.is_empty() && st.selected_shapes.len() == 1;
    if one {
        if let Some(s) = d.shapes.iter().find(|s| s.id == st.selected_shapes[0]) {
            shapes::paint_selection(painter, s, &to_screen, scale);
        }
    } else if st.has_selection() {
        let mut b = Rect::NOTHING;
        for s in d.strokes.iter().filter(|s| st.selected.contains(&s.id)) {
            b = b.union(s.bounds());
        }
        for s in d.shapes.iter().filter(|s| st.selected_shapes.contains(&s.id)) {
            b = b.union(shapes::bounds(s));
        }
        if b.is_finite() {
            let sr = Rect::from_min_max(to_screen(b.min), to_screen(b.max)).expand(6.0);
            clip.add(egui::Shape::dashed_line(&[sr.left_top(), sr.right_top(), sr.right_bottom(), sr.left_bottom(), sr.left_top()], Line::new(1.2, accent), 5.0, 4.0));
        }
    }
    if let Some(ru) = &st.ruler {
        let dir = vec2(ru.angle.cos(), ru.angle.sin());
        let nrm = vec2(-dir.y, dir.x);
        let len = (d.width + d.height) * 1.5;
        let c = ru.centre;
        let corners = [c - dir * len - nrm * RULER_W / 2.0, c + dir * len - nrm * RULER_W / 2.0, c + dir * len + nrm * RULER_W / 2.0, c - dir * len + nrm * RULER_W / 2.0].map(&to_screen);
        clip.add(egui::Shape::convex_polygon(corners.to_vec(), Color32::from_rgba_unmultiplied(120, 120, 128, 70), Line::new(1.0, Color32::from_gray(150))));
        // ticks every 10 points
        let mut k = -len;
        while k < len {
            let a = c + dir * k - nrm * RULER_W / 2.0;
            let l = if (k / 10.0).round() as i32 % 5 == 0 { 12.0 } else { 6.0 };
            clip.line_segment([to_screen(a), to_screen(a + nrm * l)], Line::new(1.0, Color32::from_gray(90)));
            k += 10.0;
        }
    }
}

/// A finished stroke's mesh, kept between frames: made at the canvas origin (without the
/// stroke's move), and the copy placed where it was last drawn.
struct Cached {
    key: u64,
    /// the stroke's bounds at `key`'s scale, before it's placed (for culling)
    bbox: Rect,
    /// its mesh, once it has been in view
    base: Option<std::sync::Arc<egui::Mesh>>,
    at: egui::Vec2,
    placed: Option<std::sync::Arc<egui::Mesh>>,
    pass: u64,
}

thread_local! {
    static MESHES: std::cell::RefCell<std::collections::HashMap<String, Cached>> = Default::default();
}

/// What a stroke's mesh is made from (all but its move).
fn mesh_key(s: &ink::Stroke, scale: f32) -> u64 {
    // (a multiply-xor fold: a long drawing's thousands of strokes are checked every frame)
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u32| h = (h ^ v as u64).wrapping_mul(0x0000_0100_0000_01b3).rotate_left(5);
    mix(scale.to_bits());
    mix(s.ink as u32);
    mix(s.width.to_bits());
    for v in s.color.iter().chain(&s.transform[..4]) {
        mix(v.to_bits());
    }
    mix(s.points.len() as u32);
    for p in &s.points {
        mix(p.x.to_bits());
        mix(p.y.to_bits());
        mix(p.size.to_bits());
        mix(p.opacity.to_bits());
    }
    h
}

/// A stroke's bounds at `scale`, unplaced: its points and its width each side.
fn stroke_bbox(s: &ink::Stroke, scale: f32) -> Rect {
    // (turned or scaled by its own transform: never culled)
    if s.transform[..4] != [1.0, 0.0, 0.0, 1.0] {
        return Rect::EVERYTHING;
    }
    let mut r = Rect::NOTHING;
    let mut w: f32 = s.width;
    for p in &s.points {
        r.extend_with(pos2(p.x * scale, p.y * scale));
        w = w.max(p.size);
    }
    r.expand(w * scale * 2.0 + 2.0)
}

/// Paints finished strokes from their kept meshes: a stroke is tessellated again only when it
/// changes or the zoom does; moved (the lasso) or scrolled, its mesh is only shifted.
fn paint_cached(painter: &egui::Painter, strokes: &[ink::Stroke], origin: Pos2, scale: f32) {
    let pass = painter.ctx().cumulative_pass_nr();
    let clip = painter.clip_rect();
    MESHES.with_borrow_mut(|cache| {
        for s in strokes {
            let at = origin.to_vec2() + vec2(s.transform[4], s.transform[5]) * scale;
            // (out of view: not even checked; checked again when it comes back)
            if let Some(c) = cache.get_mut(&s.id) {
                if !c.bbox.translate(at).intersects(clip) {
                    c.pass = pass;
                    continue;
                }
            }
            let key = mesh_key(s, scale);
            let c = match cache.get_mut(&s.id) {
                Some(c) if c.key == key => c,
                _ => {
                    cache.insert(s.id.clone(), Cached { key, bbox: stroke_bbox(s, scale), base: None, placed: None, at: egui::Vec2::ZERO, pass });
                    cache.get_mut(&s.id).unwrap()
                }
            };
            c.pass = pass;
            if !c.bbox.translate(at).intersects(clip) {
                continue;
            }
            let base = c.base.get_or_insert_with(|| {
                let mut still = s.clone();
                still.transform[4] = 0.0;
                still.transform[5] = 0.0;
                std::sync::Arc::new(ink::mesh(&still, &|p| pos2(p.x * scale, p.y * scale), scale))
            });
            if c.at != at || c.placed.is_none() {
                let mut m = (**base).clone();
                m.translate(at);
                c.placed = Some(std::sync::Arc::new(m));
                c.at = at;
            }
            if let Some(m) = &c.placed {
                painter.add(egui::Shape::Mesh(m.clone()));
            }
        }
        // strokes no drawing has shown for a while go
        if cache.len() > strokes.len() + 64 {
            cache.retain(|_, c| pass.saturating_sub(c.pass) < 120);
        }
    });
}
