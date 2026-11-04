//! Laying a note out like Notes 4.13: paragraph styles in the SF Pro stand-ins (Sfera, Sfera
//! Display at 20 pt and up, Sfera Mono), list labels and checklist circles in a hanging indent,
//! block quotes, collapsed sections, tables and attachment blocks. Sizes were measured on the
//! research screenshots (Retina, default text size): Title 24 pt bold, Body 16 pt with 19 pt
//! lines, text #454545, date line 14 pt #999999.

use std::collections::HashMap;
use std::sync::Arc;

use egui::text::{LayoutJob, LayoutSection, TextFormat};
use egui::{Align, Color32, FontFamily, FontId, Galley, Rect, Stroke, Vec2, pos2, vec2};
use nt_core::model::ParaStyle;

use super::doc::{AttKind, Doc, Fmt, Para, Table};

// ---- measured geometry (points)

/// Space between the pane's edges and the text.
pub const MARGIN_X: f32 = 23.5;
/// The date line's baseline below the pane's top, and the first line's baseline below that.
pub const DATE_BASELINE: f32 = 20.0;
pub const FIRST_BASELINE: f32 = 40.25;
pub const DATE_SIZE: f32 = 14.0;
/// One step of Format > Indentation and of list nesting; the list label and its text.
pub const INDENT_STEP: f32 = 20.5;
pub const LABEL_X: f32 = 5.0;
pub const LIST_TEXT_X: f32 = 26.25;
/// List labels: dashes and numbers end this far before the text; the bullet dot's centre and radius.
pub const LABEL_GAP: f32 = 9.25;
pub const BULLET_X: f32 = 12.75;
pub const BULLET_R: f32 = 3.5;
pub const NUMBER_GAP: f32 = 4.5;
/// Checklist items (NO2's 09 Checklist): the circle (19 pt, a 1.25 pt #BDBDBD ring, its centre
/// 8.75 pt into the margin's column), the text after it, the room above and below each item
/// (items 25 pt apart).
pub const CHECK_TEXT_X: f32 = 26.0;
pub const CHECK_D: f32 = 19.0;
pub const CHECK_RING: f32 = 1.25;
pub const CHECK_PAD: f32 = 3.0;
/// Titles and Headings in their lines (NO2's mixed note): raised, and the line's room changed.
pub const TITLE_RAISE: f32 = 2.5;
pub const TITLE_EXTRA: f32 = -1.5;
pub const HEADING_RAISE: f32 = 3.0;
pub const HEADING_EXTRA: f32 = 0.5;
/// Monostyled (NO2's 04): 14 pt in a #EAEAEA block the column's width, its text 10.5 pt in; the
/// block starts 4 pt below the line before and has 6 pt inside above and below its text.
pub const MONO_INSET: f32 = 10.5;
pub const MONO_GAP: f32 = 4.0;
pub const MONO_PAD: f32 = 6.0;
pub const QUOTE_TEXT_X: f32 = 9.5;
/// Line height as a multiple of the font size (16 pt body lines are 19 pt apart).
pub const LINE: f32 = 19.0 / 16.0;
/// Table cells (Notes' PDF of "19 Table", scaled to the 16 pt body).
pub const CELL_PAD_X: f32 = 6.4;
pub const CELL_PAD_Y: f32 = 2.2;
pub const TABLE_LINE: f32 = 1.0;
pub const MIN_COL: f32 = 44.0;
/// Highlights: a rounded band 1 pt shorter than the line at each end, 3 pt wider than the text.
pub const HL_PAD_X: f32 = 2.0;
pub const HL_PAD_RIGHT: f32 = 0.6;
pub const HL_RADIUS: f32 = 4.0;

// ---- colours (light; dark from Notes' dark appearance)

/// Notes' light paper in dark mode (Settings > "Use dark backgrounds for note content" off):
/// the note's colours as in light mode, the window around it dark.
static LIGHT_PAPER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_paper(light: bool) {
    LIGHT_PAPER.store(light, std::sync::atomic::Ordering::Relaxed);
}

/// The note's content is drawn dark (dark mode, with dark backgrounds).
pub fn dark() -> bool {
    !kn_ui::theme::is_light() && !LIGHT_PAPER.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn text_color() -> Color32 {
    if dark() { Color32::from_gray(223) } else { Color32::from_gray(69) }
}

pub fn dim_color() -> Color32 {
    if dark() { Color32::from_gray(128) } else { Color32::from_gray(153) }
}

pub fn bg_color() -> Color32 {
    if dark() { Color32::from_gray(30) } else { Color32::WHITE }
}

/// Notes' yellow (links, tags, the caret, ticked circles), as measured: #F2BB4B.
pub fn accent() -> Color32 {
    Color32::from_rgb(242, 187, 75)
}

/// Selected text: the accent over the background.
pub fn selection_color(focused: bool) -> Color32 {
    if !focused {
        return if dark() { Color32::from_gray(70) } else { Color32::from_gray(220) };
    }
    if dark() { Color32::from_rgb(110, 88, 40) } else { Color32::from_rgb(251, 230, 186) }
}

pub fn line_color() -> Color32 {
    if dark() { Color32::from_gray(90) } else { Color32::from_gray(170) }
}

pub fn card_color() -> Color32 {
    if dark() { Color32::from_gray(50) } else { Color32::from_rgb(229, 229, 233) }
}

/// The five highlight colours: (background, text), measured on "34 Highlight Colors".
pub fn highlight_colors(n: u8) -> (Color32, Color32) {
    let (bg, fg) = match n {
        1 => ((245, 226, 249), (187, 62, 217)),
        2 => ((252, 227, 230), (235, 68, 90)),
        3 => ((253, 239, 227), (240, 147, 67)),
        4 => ((230, 246, 244), (90, 197, 179)),
        _ => ((226, 237, 254), (59, 134, 247)),
    };
    if dark() {
        // the same hues, darker bands and lighter text
        let d = |c: (u8, u8, u8), k: f32| Color32::from_rgb((c.0 as f32 * k) as u8, (c.1 as f32 * k) as u8, (c.2 as f32 * k) as u8);
        return (d(fg, 0.35), Color32::from_rgb(fg.0.saturating_add(30), fg.1.saturating_add(30), fg.2.saturating_add(30)));
    }
    (Color32::from_rgb(bg.0, bg.1, bg.2), Color32::from_rgb(fg.0, fg.1, fg.2))
}

/// The dot colour of each highlight in the Aa popover's menu.
pub fn highlight_dot(n: u8) -> Color32 {
    match n {
        1 => Color32::from_rgb(193, 66, 224),
        2 => Color32::from_rgb(234, 66, 90),
        3 => Color32::from_rgb(240, 146, 62),
        4 => Color32::from_rgb(88, 196, 178),
        _ => Color32::from_rgb(58, 133, 246),
    }
}

// ---- fonts

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Text,
    Mono,
}

/// The size, face and weight a paragraph style uses (at text size `scale` = 1 by default).
pub fn style_font(style: ParaStyle, scale: f32) -> (f32, Face, bool) {
    let (size, face, bold) = match style {
        ParaStyle::Title => (24.0, Face::Text, true),
        // (NO2: Heading 19 pt, Subheading the body's 16 pt in bold, Monostyled 14 pt)
        ParaStyle::Heading => (19.0, Face::Text, true),
        ParaStyle::Subheading => (16.0, Face::Text, true),
        ParaStyle::Monostyled => (14.0, Face::Mono, false),
        _ => (16.0, Face::Text, false),
    };
    (size * scale, face, bold)
}

/// The PostScript name of the stand-in face for SF Pro / SF Pro Display / SF Mono.
pub fn face_name(face: Face, size: f32, bold: bool, italic: bool) -> &'static str {
    match (face, size >= 20.0, bold, italic) {
        (Face::Mono, _, false, false) => "SferaMono-Regular",
        (Face::Mono, _, true, false) => "SferaMono-Bold",
        (Face::Mono, _, false, true) => "SferaMono-Italic",
        (Face::Mono, _, true, true) => "SferaMono-BoldItalic",
        (Face::Text, true, false, false) => "SferaDisplay-Regular",
        (Face::Text, true, true, false) => "SferaDisplay-Bold",
        (Face::Text, true, false, true) => "SferaDisplay-Italic",
        (Face::Text, true, true, true) => "SferaDisplay-BoldItalic",
        (Face::Text, false, false, false) => "Sfera-Regular",
        (Face::Text, false, true, false) => "Sfera-Bold",
        (Face::Text, false, false, true) => "Sfera-Italic",
        (Face::Text, false, true, true) => "Sfera-BoldItalic",
    }
}

const FACES: &[&str] = &[
    "Sfera-Regular",
    "Sfera-Bold",
    "Sfera-Italic",
    "Sfera-BoldItalic",
    "Sfera-Semibold",
    "SferaDisplay-Regular",
    "SferaDisplay-Bold",
    "SferaDisplay-Italic",
    "SferaDisplay-BoldItalic",
    "SferaMono-Regular",
    "SferaMono-Bold",
    "SferaMono-Italic",
    "SferaMono-BoldItalic",
];

static FACE_DATA: std::sync::OnceLock<Vec<(String, Vec<u8>, u32)>> = std::sync::OnceLock::new();
static FACES_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Starts reading the faces from kn-render's matched fonts (off the UI thread).
pub fn preload_faces() {
    if FACE_DATA.get().is_none() {
        std::thread::spawn(|| {
            FACE_DATA.get_or_init(load_faces);
        });
    }
}

/// Markup's text fonts (display name, PostScript names regular / bold), offered when kn-render
/// has the face or its matched stand-in.
pub const MARKUP_FONTS: &[(&str, &str, &str)] = &[
    ("Helvetica", "Helvetica", "Helvetica-Bold"),
    ("Helvetica Neue", "HelveticaNeue", "HelveticaNeue-Bold"),
    ("Times New Roman", "TimesNewRomanPSMT", "TimesNewRomanPS-BoldMT"),
    ("Georgia", "Georgia", "Georgia-Bold"),
    ("Courier", "Courier", "Courier-Bold"),
    ("Menlo", "Menlo-Regular", "Menlo-Bold"),
    ("Noteworthy", "Noteworthy-Light", "Noteworthy-Bold"),
    ("Marker Felt", "MarkerFelt-Thin", "MarkerFelt-Wide"),
    ("SF Pro", "Sfera-Regular", "Sfera-Bold"),
];

static MARKUP_LOADED: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// The family for a Markup font (PostScript name): its own once loaded, else the interface's.
pub fn markup_family(ps: &str) -> FontFamily {
    let ok = FACES.contains(&ps) || MARKUP_LOADED.get().is_some_and(|v| v.iter().any(|n| n == ps));
    if ok && FACES_SET.load(std::sync::atomic::Ordering::Relaxed) { FontFamily::Name(ps.into()) } else { FontFamily::Proportional }
}

/// The Markup fonts that are there (display name, regular, bold).
pub fn markup_fonts() -> Vec<(&'static str, &'static str, &'static str)> {
    MARKUP_FONTS.iter().filter(|f| FACES.contains(&f.1) || MARKUP_LOADED.get().is_some_and(|v| v.iter().any(|n| n == f.1))).copied().collect()
}

fn load_faces() -> Vec<(String, Vec<u8>, u32)> {
    let mut fonts = kn_render::fonts::Fonts::new();
    let mut out = Vec::new();
    let mut markup = Vec::new();
    for (_, a, b) in MARKUP_FONTS {
        for name in [a, b] {
            if FACES.contains(name) {
                continue;
            }
            if let Some((_, bytes, index, _)) = fonts.face_data(name, false, false) {
                out.push((name.to_string(), bytes, index));
                markup.push(name.to_string());
            }
        }
    }
    let _ = MARKUP_LOADED.set(markup);
    for name in FACES {
        if let Some((_, bytes, index, _)) = fonts.face_data(name, false, false) {
            out.push((name.to_string(), bytes, index));
        }
    }
    out
}

/// Puts the faces into egui (all in one go, as kn-ui's cellfmt::face_family does one by one);
/// each family falls back to the interface fonts (emoji, symbols). True once they are in.
pub fn install_faces(ctx: &egui::Context, wait: bool) -> bool {
    // the faces are used only while egui has them (the shell's look sets fonts of its own too:
    // until ours are back, text falls back to the interface font instead of a missing family)
    let present = ctx.fonts(|f| FACES.iter().all(|n| f.definitions().families.contains_key(&FontFamily::Name((*n).into()))));
    FACES_SET.store(present, std::sync::atomic::Ordering::Relaxed);
    if present {
        return true;
    }
    let data = if wait { Some(FACE_DATA.get_or_init(load_faces)) } else { FACE_DATA.get() };
    let Some(data) = data else {
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
        return false;
    };
    let mut defs = ctx.fonts(|f| f.definitions().clone());
    let mut fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // after egui's own fonts: the system's colour emoji ("22 Emoji": 👋🏽 only it has) and musical
    // symbols (𝄞), used in the note's text only
    for (key, paths) in [
        ("nt-emoji", &["/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf", "/usr/share/fonts/noto/NotoColorEmoji.ttf", "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf"][..]),
        ("nt-music", &["/usr/share/fonts/truetype/noto/NotoMusic-Regular.ttf", "/usr/share/fonts/noto/NotoMusic-Regular.ttf"][..]),
    ] {
        if let Some(b) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
            defs.font_data.insert(key.into(), Arc::new(egui::FontData::from_owned(b)));
            fallback.push(key.into());
        }
    }
    for (name, bytes, index) in data {
        let mut fd = egui::FontData::from_owned(bytes.clone());
        fd.index = *index;
        defs.font_data.insert(name.clone(), Arc::new(fd));
        let mut list = vec![name.clone()];
        list.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(name.as_str().into()), list);
    }
    ctx.set_fonts(defs);
    ctx.request_repaint();
    false
}

/// SF Pro's tracking at text sizes (points per character): Apple's table up to 14 pt (the date
/// line measures exactly with it), 16 pt measured on Notes' body text (−0.61: 3 px narrower words
/// than the table's −0.31 on the 2x capture), the sizes above that continue the curve. Display
/// sizes and SF Mono have none (the Display stand-in already measures as Notes' titles).
pub fn tracking(face: Face, size: f32) -> f32 {
    if face == Face::Mono || size >= 20.0 {
        return 0.0;
    }
    const T: [(f32, f32); 10] = [(10.0, 0.12), (11.0, 0.06), (12.0, 0.0), (13.0, -0.08), (14.0, -0.15), (15.0, -0.38), (16.0, -0.61), (17.0, -0.66), (18.0, -0.70), (19.0, -0.72)];
    if size <= T[0].0 {
        return T[0].1;
    }
    for w in T.windows(2) {
        let ((a, ta), (b, tb)) = (w[0], w[1]);
        if size <= b {
            return ta + (tb - ta) * (size - a) / (b - a);
        }
    }
    -0.72
}

pub fn family(name: &str) -> FontFamily {
    if FACES_SET.load(std::sync::atomic::Ordering::Relaxed) { FontFamily::Name(name.into()) } else { FontFamily::Proportional }
}

pub fn font(face: Face, size: f32, bold: bool, italic: bool) -> FontId {
    FontId::new(size, family(face_name(face, size, bold, italic)))
}

// ---- shown text: inline tokens shown as their text, mapped back to the stored U+FFFC

#[derive(Clone, Debug, Default)]
pub struct Seg {
    pub byte: usize,
    pub blen: usize,
    pub ch: usize,
    pub clen: usize,
    pub token: bool,
}

/// Stored byte offsets <-> shown character indices of one paragraph.
#[derive(Clone, Debug, Default)]
pub struct Map {
    pub segs: Vec<Seg>,
    pub text: String,
}

impl Map {
    pub fn chars(&self) -> usize {
        self.segs.last().map_or(0, |s| s.ch + s.clen)
    }

    pub fn to_char(&self, byte: usize) -> usize {
        for s in &self.segs {
            if byte <= s.byte {
                return s.ch;
            }
            if byte < s.byte + s.blen {
                if s.token {
                    return s.ch + s.clen;
                }
                // (an offset inside a character counts from that character's start)
                let t = self.text_of(s);
                let mut k = (byte - s.byte).min(t.len());
                while !t.is_char_boundary(k) {
                    k -= 1;
                }
                return s.ch + t[..k].chars().count();
            }
        }
        self.chars()
    }

    fn text_of(&self, s: &Seg) -> &str {
        let a: usize = self.text.char_indices().nth(s.ch).map_or(self.text.len(), |(i, _)| i);
        &self.text[a..]
    }

    /// The stored offset of a shown character index (inside a token: its nearer edge).
    pub fn to_byte(&self, ch: usize) -> usize {
        for s in &self.segs {
            if ch < s.ch + s.clen {
                if s.token {
                    return if ch - s.ch <= s.clen / 2 { s.byte } else { s.byte + s.blen };
                }
                let t = self.text_of(s);
                return s.byte + t.char_indices().nth(ch - s.ch).map_or(t.len(), |(i, _)| i);
            }
        }
        self.segs.last().map_or(0, |s| s.byte + s.blen)
    }
}

// ---- boxes

#[derive(Clone, Debug, PartialEq)]
pub enum Label {
    None,
    Bullet,
    Dash,
    Number(u32),
    Check(bool),
}

#[derive(Clone, Debug)]
pub struct TableBox {
    /// (x, width) of each column and (y, height) of each row, from the table's top left
    pub cols: Vec<(f32, f32)>,
    pub rows: Vec<(f32, f32)>,
    pub cells: Vec<Vec<Flow>>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Table(TableBox),
    /// a picture, PDF page, drawing or card of this size
    Media(Vec2),
    Divider,
}

#[derive(Clone, Debug)]
pub struct PBox {
    pub para: usize,
    /// from the flow's top: the box (line tops to bottoms)
    pub top: f32,
    pub height: f32,
    /// the text's left edge and width
    pub x: f32,
    pub width: f32,
    pub galley: Option<Arc<Galley>>,
    /// where to put the galley (alignment moves it; checklist items sit lower in their row)
    pub galley_x: f32,
    pub gy: f32,
    pub map: Map,
    pub block: Option<Block>,
    pub hidden: bool,
    pub label: Label,
    pub label_x: f32,
    pub quote_x: Option<f32>,
    /// a heading that can collapse (and is collapsed)
    pub heading: Option<bool>,
    pub size: f32,
}

impl PBox {
    pub fn bottom(&self) -> f32 {
        self.top + self.height
    }

    /// The first line's baseline, from the flow's top.
    pub fn baseline(&self) -> f32 {
        match &self.galley {
            Some(g) => self.top + self.gy + g.rows.first().map_or(self.size * 0.95, |r| r.pos.y + r.glyphs.first().map_or(r.row.size.y * 0.8, |gl| gl.pos.y)),
            None => self.top + self.height * 0.8,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Flow {
    pub boxes: Vec<PBox>,
    pub height: f32,
}

pub struct Lx<'a> {
    pub ctx: &'a egui::Context,
    pub doc: &'a Doc,
    pub scale: f32,
    /// media sizes in points the painter knows (picture files read)
    pub media: &'a dyn Fn(&str) -> Option<Vec2>,
}

fn heading_level(s: ParaStyle) -> Option<u8> {
    match s {
        ParaStyle::Title => Some(0),
        ParaStyle::Heading => Some(1),
        ParaStyle::Subheading => Some(2),
        _ => None,
    }
}

/// Which paragraphs a collapsed heading hides: those after it up to the next heading of its
/// level or above.
pub fn hidden_paras(paras: &[Para]) -> Vec<bool> {
    let mut hidden = vec![false; paras.len()];
    let mut hiding: Option<u8> = None;
    for (i, p) in paras.iter().enumerate() {
        let lvl = heading_level(p.style);
        if let (Some(h), Some(l)) = (hiding, lvl) {
            if l <= h {
                hiding = None;
            }
        }
        if hiding.is_some() {
            hidden[i] = true;
            continue;
        }
        if let Some(l) = lvl {
            if p.collapsed {
                hiding = Some(l);
            }
        }
    }
    hidden
}

/// The text format of a run in a paragraph.
pub fn run_format(p: &Para, f: &Fmt, scale: f32) -> TextFormat {
    let (size, face, bold) = style_font(p.style, scale);
    // imported text keeps its size (Notes' 13 pt = this editor's Body)
    let mut size = f.font.as_ref().and_then(|f| f.size).map_or(size, |s| s * 16.0 / 13.0 * scale);
    let mut valign = Align::BOTTOM;
    if f.script != 0 {
        size *= 0.72;
        valign = if f.script > 0 { Align::TOP } else { Align::BOTTOM };
    }
    let color = match (f.highlight, &f.link, f.color) {
        (Some(h), _, _) => highlight_colors(h).1,
        (None, Some(_), _) => accent(),
        // (Show Colors, or imported text's colour)
        (None, None, Some([r, g, b, a])) => Color32::from_rgba_unmultiplied((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, (a * 255.0) as u8),
        _ => text_color(),
    };
    let line = Stroke::new((size / 16.0).max(1.0), color);
    // (Show Fonts: one of the fonts the editor has, by its name)
    let family = f.font.as_ref().and_then(|x| x.name.as_deref()).and_then(|n| MARKUP_FONTS.iter().find(|m| m.0 == n || m.1 == n || m.2 == n)).map(|m| markup_family(if bold || f.bold { m.2 } else { m.1 }));
    TextFormat {
        font_id: match family {
            Some(fam @ FontFamily::Name(_)) => FontId::new(size, fam),
            _ => font(face, size, bold || f.bold, f.italic),
        },
        extra_letter_spacing: tracking(face, size / scale.max(0.01)) * scale,
        color,
        line_height: Some((style_font(p.style, scale).0 * LINE).round()),
        underline: if f.underline || f.link.is_some() { line } else { Stroke::NONE },
        strikethrough: if f.strike { line } else { Stroke::NONE },
        valign,
        ..Default::default()
    }
}

/// Emoji (followed by `next`), drawn from the colour emoji font: kn-ui's rule (pictographs, a
/// character VS16 asks to show as emoji, joiners, keycaps, tags) and the older symbols shown as
/// emoji without VS16 (☕ ⚡ ✅ ❌ ⭐). Other text symbols (❤ ✔ ★ ☐) and VS15 ones stay text.
fn is_emoji(c: char, next: Option<char>) -> bool {
    if next == Some('\u{FE0E}') {
        return false;
    }
    kn_ui::theme::is_emoji_char(c, next)
        || matches!(c as u32, 0x231A | 0x231B | 0x23E9..=0x23EC | 0x23F0 | 0x23F3 | 0x25FD | 0x25FE | 0x2614 | 0x2615 | 0x2648..=0x2653 | 0x267F | 0x2693 | 0x26A1 | 0x26AA | 0x26AB | 0x26BD | 0x26BE | 0x26C4 | 0x26C5 | 0x26CE | 0x26D4 | 0x26EA | 0x26F2 | 0x26F3 | 0x26F5 | 0x26FA | 0x26FD | 0x2705 | 0x270A | 0x270B | 0x2728 | 0x274C | 0x274E | 0x2753..=0x2755 | 0x2757 | 0x2795..=0x2797 | 0x27B0 | 0x27BF | 0x2B1B | 0x2B1C | 0x2B50 | 0x2B55)
}

/// The paragraph's text as an egui job, with the token map.
pub fn para_job(lx: &Lx, p: &Para, width: f32) -> (LayoutJob, Map) {
    let mut job = LayoutJob { wrap: egui::text::TextWrapping::wrap_at_width(width.max(1.0)), ..Default::default() };
    job.halign = Align::LEFT;
    job.justify = p.align == super::doc::Align::Justified;
    let mut map = Map::default();
    let mut ch = 0;
    for (r, f) in p.spans() {
        let mut fmt = run_format(p, f, lx.scale);
        let token = f.att.as_deref().and_then(|id| lx.doc.att(id));
        let start = job.text.len();
        let (shown, blen, is_token) = match token.map(|a| &a.kind) {
            Some(AttKind::Tag(t)) => {
                fmt.color = accent();
                (t.clone(), r.len(), true)
            }
            Some(AttKind::NoteLink { text, .. }) => {
                fmt.color = accent();
                // room for the little note icon drawn before it
                let a = job.text.len();
                job.text.push_str(text);
                job.sections.push(LayoutSection { leading_space: (NOTE_ICON_PRE + NOTE_ICON_W + NOTE_ICON_GAP) * lx.scale, byte_range: egui::text::ByteIndex(a)..egui::text::ByteIndex(job.text.len()), format: fmt });
                let n = text.chars().count();
                map.segs.push(Seg { byte: r.start, blen: r.len(), ch, clen: n, token: true });
                map.text.push_str(text);
                ch += n;
                continue;
            }
            Some(AttKind::Math(t)) => {
                // " = 14": the sign in the text colour, the result in the accent
                let t = t.replace(['\u{200e}', '\u{200f}'], "");
                let (sign, result) = t.split_once('=').map(|(a, b)| (format!("{a}="), b.to_string())).unwrap_or((String::new(), t.clone()));
                let a = job.text.len();
                job.text.push_str(&sign);
                job.sections.push(LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(a)..egui::text::ByteIndex(job.text.len()), format: fmt.clone() });
                fmt.color = accent();
                let n = sign.chars().count() + result.chars().count();
                let b = job.text.len();
                job.text.push_str(&result);
                job.sections.push(LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(b)..egui::text::ByteIndex(job.text.len()), format: fmt });
                map.segs.push(Seg { byte: r.start, blen: r.len(), ch, clen: n, token: true });
                map.text.push_str(&sign);
                map.text.push_str(&result);
                ch += n;
                continue;
            }
            Some(_) => ("\u{25A1}".to_string(), r.len(), true),
            None if f.att.is_some() => ("\u{25A1}".to_string(), r.len(), true),
            None => (p.text[r.clone()].to_string(), r.len(), false),
        };
        let n = shown.chars().count();
        job.text.push_str(&shown);
        // emoji keep their own colours in grey, highlighted or linked text (egui multiplies a
        // colour glyph by the text colour): their own sections, untinted
        // (from the "emoji" family, colour font first, so ❤️ isn't the text face's black heart)
        let emoji_font = kn_ui::theme::has_color_emoji().then(|| FontId::new(fmt.font_id.size, FontFamily::Name("emoji".into())));
        let as_emoji = |f: &mut TextFormat| {
            f.color = Color32::WHITE;
            if let Some(e) = &emoji_font {
                f.font_id = e.clone();
            }
        };
        let mut at = start;
        let mut emoji_run: Option<bool> = None;
        let mut run_start = start;
        let mut chars = shown.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            let e = is_emoji(c, chars.peek().map(|n| n.1)) || (emoji_run == Some(true) && matches!(c, '\u{1F3FB}'..='\u{1F3FF}'));
            if emoji_run.is_some_and(|r| r != e) {
                let mut f = fmt.clone();
                if emoji_run == Some(true) {
                    as_emoji(&mut f);
                }
                job.sections.push(LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(run_start)..egui::text::ByteIndex(start + i), format: f });
                run_start = start + i;
            }
            emoji_run = Some(e);
            at = start + i + c.len_utf8();
        }
        let mut f = fmt;
        if emoji_run == Some(true) {
            as_emoji(&mut f);
        }
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(run_start)..egui::text::ByteIndex(at.max(job.text.len())), format: f });
        map.segs.push(Seg { byte: r.start, blen, ch, clen: n, token: is_token });
        map.text.push_str(&shown);
        ch += n;
    }
    if job.sections.is_empty() {
        let fmt = run_format(p, &p.fmt_at(0), lx.scale);
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(0), format: fmt });
    }
    (job, map)
}

/// Lays out a flow of paragraphs (the note, or a table cell) `width` points wide.
pub fn layout_flow(lx: &Lx, paras: &[Para], width: f32, body: bool) -> Flow {
    layout_flow_with(lx, paras, width, body, None)
}

/// The note's paragraph layouts kept between frames, each with what it was laid out from: a
/// frame lays out only paragraphs that changed, and of those only the ones in view (and a
/// time budget's worth more); the rest keep an estimated height until they're reached.
#[derive(Default)]
pub struct FlowCache {
    /// by paragraph, as last laid out (None: estimated, or hidden)
    entries: Vec<Option<Entry>>,
    /// estimated paragraphs the budget will lay out in later frames
    pending: bool,
}

struct Entry {
    key: Key,
    pbox: PBox,
}

#[derive(PartialEq)]
struct Key {
    para: Para,
    /// the attachments it shows (a block, or inline tokens' text)
    atts: Vec<super::doc::Att>,
    media: Option<Vec2>,
    label: Label,
    width: f32,
    mono: (bool, bool),
    /// the colours, the scale, and whether the editor's faces were in (laid out with the
    /// interface font until they are)
    stamp: (Color32, bool, u32, bool, bool),
}

impl FlowCache {
    /// Whether any paragraph still has only an estimated height.
    pub fn estimates(&self) -> bool {
        self.pending
    }
}

/// What a cached layout lays out: paragraphs in view (`view`, flow coordinates) and those in
/// `must`; while `budget` lasts, those within `near` too, and any others but huge ones.
pub struct Window<'a> {
    pub cache: &'a mut FlowCache,
    pub view: (f32, f32),
    pub near: (f32, f32),
    pub must: &'a [usize],
    pub budget: std::time::Duration,
}

/// Paragraphs longer than this (bytes) aren't laid out ahead of being in view: one of 50,000
/// characters takes egui 30 ms.
const HUGE_PARA: usize = 4000;

/// The attachments a paragraph shows (its block, or its inline tokens).
fn para_atts<'a>(doc: &'a Doc, p: &Para) -> Vec<&'a super::doc::Att> {
    p.runs.iter().filter_map(|r| r.fmt.att.as_deref()).filter_map(|id| doc.att(id)).collect()
}

fn text_hash(t: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// [`layout_flow`] with last frame's layouts (`win`): see [`FlowCache`].
pub fn layout_flow_with(lx: &Lx, paras: &[Para], width: f32, body: bool, mut win: Option<Window>) -> Flow {
    let t0 = std::time::Instant::now();
    let stamp = (text_color(), dark(), lx.scale.to_bits(), FACES_SET.load(std::sync::atomic::Ordering::Relaxed), kn_ui::theme::has_color_emoji());
    // last frame's entries, taken as they're found again (by place, else by text)
    let mut old: Vec<Option<Entry>> = win.as_mut().map(|w| std::mem::take(&mut w.cache.entries)).unwrap_or_default();
    let mut fresh: Vec<Option<Entry>> = Vec::new();
    let mut by_text: Option<HashMap<u64, Vec<usize>>> = None;
    let mut shift: isize = 0;
    let mut pending = false;
    let hidden = if body { hidden_paras(paras) } else { vec![false; paras.len()] };
    let mut boxes = Vec::with_capacity(paras.len());
    let mut y = 0.0;
    // numbered lists count per indent level
    let mut counters: Vec<u32> = Vec::new();
    for (i, p) in paras.iter().enumerate() {
        let (size, _, _) = style_font(p.style, lx.scale);
        if hidden[i] {
            boxes.push(PBox {
                para: i,
                top: y,
                height: 0.0,
                x: 0.0,
                width,
                galley: None,
                galley_x: 0.0,
                gy: 0.0,
                map: Map::default(),
                block: None,
                hidden: true,
                label: Label::None,
                label_x: 0.0,
                quote_x: None,
                heading: None,
                size,
            });
            if win.is_some() {
                fresh.push(None);
            }
            continue;
        }
        let lvl = p.indent as usize;
        if p.style == ParaStyle::Numbered {
            counters.resize(lvl + 1, 0);
            let n = match (p.start, counters[lvl]) {
                (Some(s), _) if i == 0 || paras[i - 1].style != ParaStyle::Numbered || (paras[i - 1].indent as usize) < lvl => s,
                (_, c) => c + 1,
            };
            counters[lvl] = n;
        } else if !p.is_list() || p.indent == 0 {
            counters.truncate(if p.is_list() { lvl } else { 0 });
        } else {
            counters.truncate(lvl);
        }
        let label = match p.style {
            ParaStyle::Bulleted => Label::Bullet,
            ParaStyle::Dashed => Label::Dash,
            ParaStyle::Numbered => Label::Number(counters[lvl]),
            ParaStyle::Checklist => Label::Check(p.checked),
            _ => Label::None,
        };
        let mut x = lvl as f32 * INDENT_STEP * lx.scale;
        let mono = p.style == ParaStyle::Monostyled;
        if mono {
            x += MONO_INSET * lx.scale;
        }
        let quote_x = p.quote.then_some(x);
        if p.quote {
            x += QUOTE_TEXT_X * lx.scale;
        }
        let label_x = x + LABEL_X * lx.scale;
        x += match label {
            Label::None => 0.0,
            Label::Check(_) => CHECK_TEXT_X * lx.scale,
            _ => LIST_TEXT_X * lx.scale,
        };
        let w = (width - x - if mono { MONO_INSET * lx.scale } else { 0.0 }).max(MIN_COL);
        let block_att = lx.doc.block_of(p);
        // a run of Monostyled paragraphs is one block: room above its first and below its last
        let mono_ends = if mono {
            let shown = |j: usize| !hidden[j] && paras[j].style == ParaStyle::Monostyled;
            (i == 0 || !shown(i - 1), i + 1 >= paras.len() || !shown(i + 1))
        } else {
            (false, false)
        };
        // ---- last frame's layout of it, or (out of view) an estimate
        let mut key = None;
        if let Some(wn) = win.as_mut() {
            let media = block_att.and_then(|a| (lx.media)(&a.id));
            let same = |e: &Entry| {
                e.key.para == *p && e.key.label == label && e.key.width == w && e.key.mono == mono_ends && e.key.stamp == stamp && e.key.media == media && e.key.atts.iter().eq(para_atts(lx.doc, p))
            };
            let guess = i as isize + shift;
            let mut found = None;
            for j in [guess, i as isize] {
                if j >= 0 && (j as usize) < old.len() && old[j as usize].as_ref().is_some_and(same) {
                    found = Some(j as usize);
                    break;
                }
            }
            if found.is_none() && !old.is_empty() {
                let map = by_text.get_or_insert_with(|| {
                    let mut m: HashMap<u64, Vec<usize>> = HashMap::new();
                    for (j, e) in old.iter().enumerate() {
                        if let Some(e) = e {
                            m.entry(text_hash(&e.key.para.text)).or_default().push(j);
                        }
                    }
                    m
                });
                found = map.get(&text_hash(&p.text)).and_then(|js| js.iter().copied().find(|&j| old[j].as_ref().is_some_and(same)));
            }
            if let Some(j) = found {
                shift = j as isize - i as isize;
                let e = old[j].take().unwrap();
                let mut b = e.pbox.clone();
                b.para = i;
                b.top = y;
                y += b.height;
                boxes.push(b);
                fresh.push(Some(e));
                continue;
            }
            // not laid out: in view, needed (the caret's), or while the budget lasts
            let line = (size * LINE).round();
            let est = match block_att.map(|a| &a.kind) {
                Some(AttKind::Table(t)) => t.cells.len() as f32 * (line + 2.0 * CELL_PAD_Y * lx.scale) + TABLE_LINE,
                Some(AttKind::Divider) => DIVIDER_H * lx.scale,
                Some(kind) => media_size(lx, &block_att.unwrap().id, kind, w).y + MEDIA_PAD * lx.scale,
                None => ((p.text.chars().count() as f32 * size * 0.5 / w).ceil().max(1.0)) * line,
            };
            let within = |r: (f32, f32)| y <= r.1 && y + est >= r.0;
            let budget = t0.elapsed() < wn.budget;
            // (a huge paragraph, pasted source say, is laid out only when it's reached)
            let lay = within(wn.view) || wn.must.contains(&i) || budget && (within(wn.near) || p.text.len() <= HUGE_PARA);
            if !lay {
                boxes.push(PBox {
                    para: i,
                    top: y,
                    height: est,
                    x,
                    width: w,
                    galley: None,
                    galley_x: x,
                    gy: 0.0,
                    map: Map::default(),
                    block: None,
                    hidden: false,
                    label,
                    label_x,
                    quote_x,
                    heading: heading_level(p.style).filter(|_| body).map(|_| p.collapsed),
                    size,
                });
                y += est;
                fresh.push(None);
                pending |= p.text.len() <= HUGE_PARA;
                continue;
            }
            key = Some(Key { para: p.clone(), atts: para_atts(lx.doc, p).into_iter().cloned().collect(), media, label: label.clone(), width: w, mono: mono_ends, stamp });
        }
        if let Some(att) = block_att {
            let (block, h) = match &att.kind {
                AttKind::Table(t) => {
                    let tb = layout_table(lx, t, w);
                    let h = tb.rows.last().map_or(0.0, |r| r.0 + r.1) + TABLE_LINE;
                    (Block::Table(tb), h)
                }
                AttKind::Divider => (Block::Divider, DIVIDER_H * lx.scale),
                kind => {
                    let s = media_size(lx, &att.id, kind, w);
                    (Block::Media(s), s.y)
                }
            };
            // a block sits on its own line, with a little room around it (the gallery's "30 File")
            let pad = if matches!(block, Block::Media(_)) { MEDIA_PAD * lx.scale } else { 0.0 };
            boxes.push(PBox {
                para: i,
                top: y,
                height: h + pad,
                x,
                width: w,
                galley: None,
                galley_x: x,
                gy: 0.0,
                map: Map { segs: vec![Seg { byte: 0, blen: p.len(), ch: 0, clen: 1, token: true }], text: "\u{FFFC}".into() },
                block: Some(block),
                hidden: false,
                label,
                label_x,
                quote_x,
                heading: None,
                size,
            });
            y += h + pad;
            if win.is_some() {
                fresh.push(key.map(|key| Entry { key, pbox: boxes.last().unwrap().clone() }));
            }
            continue;
        }
        let (job, map) = para_job(lx, p, w);
        let galley = lx.ctx.fonts_mut(|f| f.layout_job(job));
        let gw = galley.rect.width();
        let galley_x = x + match p.align {
            super::doc::Align::Center => ((w - gw) / 2.0).max(0.0),
            super::doc::Align::Right => (w - gw).max(0.0),
            // (a right-to-left paragraph with no alignment of its own sits on the right)
            super::doc::Align::Natural if p.direction == Some(2) => (w - gw).max(0.0),
            _ => 0.0,
        };
        let pad = if p.style == ParaStyle::Checklist { CHECK_PAD * lx.scale } else { 0.0 };
        // a run of Monostyled paragraphs is one block: room above its first and below its last
        let (mono_top, mono_bottom) = if mono {
            let (first, last) = mono_ends;
            (if first { (MONO_GAP + MONO_PAD) * lx.scale } else { 0.0 }, if last { (MONO_GAP + MONO_PAD) * lx.scale } else { 0.0 })
        } else {
            (0.0, 0.0)
        };
        // (NO2's mixed note) a Title or Heading sits higher in its line than egui puts it, the room
        // after it different: what Notes' gaps between lines measure
        let (raise, extra) = match p.style {
            ParaStyle::Title => (TITLE_RAISE, TITLE_EXTRA),
            ParaStyle::Heading => (HEADING_RAISE, HEADING_EXTRA),
            _ => (0.0, 0.0),
        };
        let h = galley.rect.height().max((size * LINE).round()) + 2.0 * pad + mono_top + mono_bottom + extra * lx.scale;
        let pad = pad + mono_top - raise * lx.scale;
        boxes.push(PBox {
            para: i,
            top: y,
            height: h,
            x,
            width: w,
            galley: Some(galley),
            galley_x,
            gy: pad,
            map,
            block: None,
            hidden: false,
            label,
            label_x,
            quote_x,
            heading: heading_level(p.style).filter(|_| body).map(|_| p.collapsed),
            size,
        });
        y += h;
        if win.is_some() {
            fresh.push(key.map(|key| Entry { key, pbox: boxes.last().unwrap().clone() }));
        }
    }
    if let Some(wn) = win {
        wn.cache.entries = fresh;
        wn.cache.pending = pending;
    }
    Flow { boxes, height: y }
}

/// Column widths: dragged ones kept, the rest share what is left (each at least MIN_COL).
pub fn column_widths(t: &Table, width: f32) -> Vec<f32> {
    let n = t.cols().max(1);
    let fixed: f32 = t.widths.iter().flatten().sum();
    let free = t.widths.iter().filter(|w| w.is_none()).count().max(1) as f32;
    let share = ((width - fixed) / free).max(MIN_COL);
    (0..n).map(|c| t.widths.get(c).copied().flatten().unwrap_or(share).max(MIN_COL)).collect()
}

pub fn layout_table(lx: &Lx, t: &Table, width: f32) -> TableBox {
    let widths = column_widths(t, width - TABLE_LINE);
    let mut cols = Vec::new();
    let mut x = 0.0;
    for w in &widths {
        cols.push((x, *w));
        x += w;
    }
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut y = 0.0;
    for r in 0..t.rows() {
        let mut row = Vec::new();
        let mut h: f32 = 0.0;
        for (c, &(_, w)) in cols.iter().enumerate() {
            let f = layout_flow(lx, &t.cells[r][c], w - 2.0 * CELL_PAD_X * lx.scale, false);
            h = h.max(f.height);
            row.push(f);
        }
        let h = h + 2.0 * CELL_PAD_Y * lx.scale;
        rows.push((y, h));
        cells.push(row);
        y += h;
    }
    TableBox { cols, rows, cells }
}

/// How big an attachment block is in a column `w` wide.
pub fn media_size(lx: &Lx, id: &str, kind: &AttKind, w: f32) -> Vec2 {
    let known = (lx.media)(id);
    match kind {
        AttKind::Drawing { ink: Some(d), .. } => vec2(w, w * d.height / d.width.max(1.0)),
        // View As Small: a card, as a PDF's (NO3 2b)
        AttKind::Image { view: nt_core::model::ViewSize::Small, .. } => vec2(w.min(super::pdf::CARD.x * lx.scale), super::pdf::CARD.y * lx.scale),
        AttKind::Image { width, height, .. } | AttKind::Drawing { width, height, .. } => {
            let s = if *width > 0.0 && *height > 0.0 { vec2(*width, *height) } else { known.unwrap_or(vec2(0.0, 0.0)) };
            let s = if s.x <= 0.0 || s.y <= 0.0 { vec2(320.0, 200.0) } else { s };
            // pictures at their own size in points, narrowed to the column (the gallery's "28 Image":
            // the 320 × 200 picture is 3.17 times its title's width, as 320 pt is here)
            // (drawings fill the column, as Notes' canvases do)
            let iw = if matches!(kind, AttKind::Drawing { .. }) { w } else { s.x.min(w) };
            vec2(iw, iw * s.y / s.x)
        }
        AttKind::Pdf { view, thumbs, .. } => super::pdf::size(*view, *thumbs, w, lx.scale),
        AttKind::File { .. } | AttKind::Url { .. } => vec2(w.min(FILE_CARD.x), FILE_CARD.y),
        // (at the attachment's own size when the library knows it, drawings across the column)
        AttKind::Missing { what, width, height, .. } if *width > 0.0 && *height > 0.0 => {
            let iw = if *what == super::doc::Missing::Drawing { w } else { width.min(w) };
            vec2(iw.max(FILE_CARD.y), (iw * height / width).max(FILE_CARD.y))
        }
        AttKind::Missing { .. } => vec2(w.min(FILE_CARD.x), FILE_CARD.y),
        _ => vec2(w, 20.0),
    }
}

/// Room above a picture or card.
pub const MEDIA_PAD: f32 = 4.5;
/// A divider line's paragraph and where its line is (the gallery's "20 Divider": the baselines
/// around it 66 pt apart).
pub const DIVIDER_H: f32 = 47.0;
pub const DIVIDER_LINE: f32 = 23.0;
/// A note link's icon and the gap after it.
pub const NOTE_ICON_W: f32 = 12.5;
pub const NOTE_ICON_GAP: f32 = 3.0;
pub const NOTE_ICON_PRE: f32 = 3.5;
/// A file card (measured on the gallery's "30 File" miniature, scaled to the editor).
pub const FILE_CARD: Vec2 = vec2(283.0, 62.0);

// ---- positions in a laid out paragraph

/// Rectangles (galley-relative) covering shown characters `a..b`, one per row.
pub fn char_rects(g: &Galley, a: usize, b: usize, full_rows: bool) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut start = 0;
    for row in &g.rows {
        let n = row.char_count_excluding_newline().0;
        let (ra, rb) = (a.max(start), b.min(start + n));
        let ends_inside = b > start + n && row.ends_with_newline;
        if ra < rb || (ra == rb && ra == start && b > start + n && n == 0) {
            let x0 = row.pos.x + row.x_offset(egui::text::CharIndex(ra - start));
            let x1 = row.pos.x + row.x_offset(egui::text::CharIndex(rb - start));
            let (y0, y1) = if full_rows { (row.min_y(), row.max_y()) } else { (row.min_y(), row.max_y()) };
            out.push(Rect::from_min_max(pos2(x0, y0), pos2(x1.max(x0 + if ends_inside { 4.0 } else { 0.0 }), y1)));
        }
        start += row.char_count_including_newline().0;
    }
    out
}

/// The caret's rectangle (galley-relative) before shown character `ch`.
pub fn caret_rect(g: &Galley, ch: usize) -> Rect {
    g.pos_from_cursor(egui::text::CCursor::new(ch))
}

/// The shown character nearest a galley-relative point.
pub fn hit(g: &Galley, p: Vec2) -> usize {
    g.cursor_from_pos(p).index.0
}

