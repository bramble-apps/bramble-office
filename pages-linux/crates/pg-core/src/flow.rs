//! Lay a Pages document out into pages.
//!
//! Word processing: the body ([`crate::body`]) flows down the text column of each page, then the
//! next column, then the next page. Paragraphs are shaped one at a time by kn-render (cached by
//! content and width), and their lines are placed one by one so that page / column / section
//! breaks, keep-with-next, widows and orphans, text wrap around floating objects and drop caps
//! can move or narrow them. Page layout: each page shows its own floating objects.
//!
//! Everything is in page points; a page's top-left is (0, 0).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use kn_format::ObjId;
use kn_model::{Drawable, Paragraph, TextBox};
use kn_render::fonts::Fonts;
use kn_render::text::{self, TextLayout};

use crate::body::{self, Body, BuildCtx, Break, SpanKind};
use crate::{PageSetup, Pages, Section};

/// One shaped piece of a paragraph: the whole paragraph, or (after a wrap / break narrowed the
/// column) the rest of it from some line on. Glyph and line byte offsets are shown-text offsets
/// of the whole body.
pub struct Piece {
    pub tl: TextLayout,
    /// Glyph indices of each line (labels included).
    pub line_glyphs: Vec<Vec<usize>>,
    /// Each line's band (top, bottom) in piece coordinates.
    pub bands: Vec<(f32, f32)>,
}

/// A line placed on a page: piece line `line` drawn with offset (dx, dy).
#[derive(Clone)]
pub struct PlacedLine {
    pub para: usize,
    pub piece: Arc<Piece>,
    pub line: usize,
    pub dx: f32,
    pub dy: f32,
    pub column: usize,
}

impl PlacedLine {
    pub fn band(&self) -> (f32, f32) {
        let (a, b) = self.piece.bands[self.line];
        (a + self.dy, b + self.dy)
    }
    pub fn range(&self) -> (usize, usize) {
        let l = &self.piece.tl.lines[self.line];
        (l.start, l.end)
    }
    pub fn x(&self) -> f32 {
        self.piece.tl.lines[self.line].x + self.dx
    }
}

/// Where a floating or anchored object sits.
#[derive(Clone, Debug)]
pub struct PlacedObject {
    pub id: ObjId,
    /// Frame on the page (points).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Inline in the text (moves with it, drawn where the paragraph is).
    pub inline: bool,
    /// Text wrap type (0 none/inline, 1.. around, 4 automatic, 5 page layout default…).
    pub wrap: u64,
    pub margin: f32,
    /// A section layout object (on the section's page template: every page of the section
    /// shows it behind the text, Pages' "wallpaper").
    pub layout: bool,
    /// A table continued over pages: the rows shown here (repeated header rows, then this
    /// page's). None: all of it.
    pub rows: Option<Arc<Vec<u32>>>,
    /// Rows broken across pages (a Word document's): this page's part of each, as (model row,
    /// each column's byte range of its cell text, the part's height).
    pub row_parts: Option<Arc<Vec<RowPart>>>,
}

/// One page's part of a table row broken across pages.
#[derive(Clone, Debug)]
pub struct RowPart {
    pub row: u32,
    /// Per column: the byte range of the cell's text shown here.
    pub ranges: Vec<(usize, usize)>,
    pub height: f32,
}

#[derive(Clone)]
pub struct HeaderFooter {
    /// Storage of the field, its piece and where it is drawn.
    pub storage: ObjId,
    pub piece: Arc<Piece>,
    pub body: Arc<Body>,
    pub dx: f32,
    pub dy: f32,
    /// The field's frame (for clicks): x, y, w, h.
    pub frame: (f32, f32, f32, f32),
    pub footer: bool,
    pub index: usize,
}

#[derive(Clone)]
pub struct Footnote {
    pub storage: ObjId,
    pub number: usize,
    pub piece: Arc<Piece>,
    pub body: Arc<Body>,
    pub dx: f32,
    pub dy: f32,
}

#[derive(Clone)]
pub struct Page {
    pub index: usize,
    /// The number shown by page-number fields.
    pub number: usize,
    pub section: usize,
    pub lines: Vec<PlacedLine>,
    pub objects: Vec<PlacedObject>,
    pub headers: Vec<HeaderFooter>,
    pub footers: Vec<HeaderFooter>,
    pub footnotes: Vec<Footnote>,
    pub background: Option<kn_model::Fill>,
    /// Shown-text range of the body on this page.
    pub start: usize,
    pub end: usize,
    /// Table of contents blocks: (y, entries) drawn as lines of their own.
    pub toc: Vec<TocBlock>,
    /// Tables on the page whose rows grew to fit their text (Pages sizes rows to the text;
    /// the stored heights are minimums).
    pub fitted_rows: HashMap<ObjId, Arc<Vec<f32>>>,
    /// Drop caps on the page: their characters (also among `lines`) and, for a shaped one,
    /// the box behind them.
    pub caps: Vec<CapBox>,
    /// The page's own size and margins (a Word section's), when not the document's.
    pub setup: Option<PageSetup>,
}

#[derive(Clone)]
pub struct CapBox {
    pub para: usize,
    pub piece: Arc<Piece>,
    /// (x, y, w, h) of the shape, page points; its fill and corner radius (points).
    pub rect: (f32, f32, f32, f32),
    pub fill: Option<kn_model::Color>,
    pub radius: f32,
}

#[derive(Clone)]
pub struct TocBlock {
    pub para: usize,
    /// The entries' text style (the TOC storage's own paragraph style).
    pub style: kn_model::CharStyle,
    pub y: f32,
    pub entries: Vec<TocEntry>,
}

#[derive(Clone, Debug)]
pub struct TocEntry {
    pub text: String,
    pub level: usize,
    pub page: usize,
    /// Shown offset of the heading in the body (click to go there).
    pub target: usize,
    pub y: f32,
    /// The entry's own style and indent (from the TOC's settings), else the block's.
    pub style: Option<kn_model::CharStyle>,
    pub indent: Option<f32>,
    /// Space before + after the entry (its entry style's).
    pub space: Option<f32>,
}

pub struct Layout {
    pub setup: PageSetup,
    pub pages: Vec<Page>,
    pub body: Option<Body>,
    /// Column rectangles of the text area: (x, width) per column.
    pub columns: Vec<(f32, f32)>,
    /// Headings for the table of contents and the sidebar: (shown offset, level, text, page).
    pub headings: Vec<(usize, usize, String, usize)>,
}

impl Layout {
    /// Page `i`'s size and margins (its section's, in a Word document; else the document's).
    pub fn page_setup(&self, i: usize) -> &PageSetup {
        self.pages.get(i).and_then(|p| p.setup.as_ref()).unwrap_or(&self.setup)
    }

    /// Column `col` of page `i` (x, width): the document's columns, or a Word section's page
    /// of its own with them laid out the same way across its text width.
    pub fn page_column(&self, i: usize, col: usize) -> (f32, f32) {
        let base = self.columns.get(col).copied().unwrap_or((self.setup.left, self.setup.text_width()));
        match self.pages.get(i).and_then(|p| p.setup.as_ref()) {
            Some(ps) => {
                let k = ps.text_width() / self.setup.text_width().max(1.0);
                (ps.left + (base.0 - self.setup.left) * k, base.1 * k)
            }
            None => base,
        }
    }

    /// The page and line holding shown offset `s` (caret placement).
    pub fn line_at(&self, s: usize) -> Option<(usize, &PlacedLine)> {
        let pi = self.pages.iter().rposition(|p| p.start <= s).unwrap_or(0);
        let p = self.pages.get(pi)?;
        // lines of a page are in text order; a position at a line's end belongs to it, unless
        // the next line starts there
        let mut best: Option<&PlacedLine> = None;
        for l in &p.lines {
            let (a, b) = l.range();
            if a <= s && s <= b {
                best = Some(l);
                if s < b {
                    break;
                }
            } else if a > s {
                break;
            }
        }
        if best.is_none() {
            // the position is in a later page's first line, or in an empty paragraph block
            for (i, pg) in self.pages.iter().enumerate().skip(pi) {
                if let Some(l) = pg.lines.iter().find(|l| l.range().0 >= s) {
                    return Some((i, l));
                }
            }
            return p.lines.last().map(|l| (pi, l));
        }
        best.map(|l| (pi, l))
    }

    /// Caret rectangle for shown offset `s`: (page, x, top, height) in page points.
    pub fn caret(&self, s: usize) -> Option<(usize, f32, f32, f32)> {
        let (pi, l) = self.line_at(s)?;
        let tl = &l.piece.tl;
        let line = &tl.lines[l.line];
        let gl = &l.piece.line_glyphs[l.line];
        let mut x = line.x;
        let mut found = false;
        for &gi in gl {
            let g = &tl.glyphs[gi];
            if g.label {
                continue;
            }
            if g.start == s {
                x = g.x;
                found = true;
                break;
            }
            if g.end <= s {
                x = g.x + g.w;
                found = true;
            }
        }
        if !found {
            x = gl.iter().map(|&i| &tl.glyphs[i]).find(|g| !g.label).map(|g| g.x).unwrap_or(line.x);
        }
        let (top, bottom) = l.band();
        Some((pi, x + l.dx, top, (bottom - top).max(line.height.min(bottom - top + 4.0))))
    }

    /// Shown offset nearest to page point (x, y) on page `page`.
    pub fn hit(&self, page: usize, x: f32, y: f32) -> Option<usize> {
        let p = self.pages.get(page)?;
        if p.lines.is_empty() {
            return Some(p.start);
        }
        // the line whose band contains y, else the nearest; across columns prefer the one under x
        let dist = |l: &PlacedLine| {
            let (a, b) = l.band();
            let dy = if y < a { a - y } else if y > b { y - b } else { 0.0 };
            let col = self.columns.get(l.column).copied().unwrap_or((0.0, f32::MAX));
            let dx = if x < col.0 { col.0 - x } else if x > col.0 + col.1 { x - col.0 - col.1 } else { 0.0 };
            dy + dx * 4.0
        };
        let l = p.lines.iter().min_by(|a, b| dist(a).total_cmp(&dist(b)))?;
        let tl = &l.piece.tl;
        let line = &tl.lines[l.line];
        let lx = x - l.dx;
        let mut best = line.start;
        let mut best_d = f32::INFINITY;
        for &gi in &l.piece.line_glyphs[l.line] {
            let g = &tl.glyphs[gi];
            if g.label {
                continue;
            }
            for (pos, gx) in [(g.start, g.x), (g.end, g.x + g.w)] {
                let d = (gx - lx).abs();
                if d < best_d {
                    best_d = d;
                    best = pos;
                }
            }
        }
        // never past the paragraph end (the shown '\n')
        Some(best.min(line.end.max(line.start)))
    }

    /// The page showing shown offset `s`.
    pub fn page_of(&self, s: usize) -> usize {
        self.line_at(s).map(|(p, _)| p).unwrap_or(0)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len().max(1)
    }
}

/// Shaped paragraphs kept between layouts (typing changes one paragraph, the rest are reused).
#[derive(Default)]
pub struct Cache {
    pieces: HashMap<u64, Arc<Piece>>,
    used: std::collections::HashSet<u64>,
    /// Tables' row heights grown to their text: (table, content hash) -> rows (None: as stored).
    tables: HashMap<(ObjId, u64), Option<Arc<Vec<f32>>>>,
    /// Laid out for a Word document (Word's metrics): pieces of the other kind don't fit.
    word: bool,
}

impl Cache {
    /// Forget pieces not used by the last layout (call after each layout).
    pub fn sweep(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.pieces.retain(|k, _| used.contains(k));
    }
}

fn hash_para(tb: &TextBox, p: &Paragraph, from: usize, width: f32, first: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tb.text[from.min(p.end)..p.end].hash(&mut h);
    format!("{:?}", (&p.runs, p.align, p.line_spacing, p.left_indent, p.right_indent, p.first_line_indent, &p.label, p.list_indent, p.label_indent, p.label_scale, p.first_text_indent, p.rtl)).hash(&mut h);
    format!("{:?}", (&p.tabs, p.default_tab)).hash(&mut h);
    (from - p.start).hash(&mut h);
    width.to_bits().hash(&mut h);
    first.hash(&mut h);
    p.start.hash(&mut h);
    // comment highlights over it
    for hl in tb.highlights.iter().filter(|hl| hl.1 > from && hl.0 < p.end) {
        format!("{hl:?}").hash(&mut h);
    }
    h.finish()
}

/// Shape paragraph `pi` from shown offset `from` at `width`. `first`: the piece starts the
/// paragraph (first-line indent and list label apply).
fn shape(fonts: &mut Fonts, cache: &mut Cache, tb: &TextBox, pi: usize, from: usize, width: f32, first: bool) -> Arc<Piece> {
    let p = &tb.paragraphs[pi];
    let key = hash_para(tb, p, from, width, first) ^ u64::from(pi == 0);
    cache.used.insert(key);
    if let Some(pc) = cache.pieces.get(&key) {
        return pc.clone();
    }
    // a one-paragraph text box of just this text, offsets rebased to 0
    let from = from.clamp(p.start, p.end);
    let base = from;
    let mut q = p.clone();
    q.start = 0;
    q.end = p.end - base;
    q.space_before = 0.0;
    q.space_after = 0.0;
    if !first {
        // (a paragraph's continuation starts at its left indent: the first-line indent is the
        // first line's, measured from the edge as the left one)
        q.first_line_indent = q.left_indent;
        q.label = None;
    }
    q.runs = p
        .runs
        .iter()
        .filter(|r| r.end > from || (r.start == r.end && r.start >= from))
        .map(|r| kn_model::Run { start: r.start.max(from) - base, end: r.end.max(from) - base, style: r.style.clone() })
        .collect();
    if q.runs.is_empty() {
        if let Some(r) = p.runs.last() {
            q.runs.push(kn_model::Run { start: q.end, end: q.end, style: r.style.clone() });
        }
    }
    // comment highlights: the text's background (the renderer draws run backgrounds)
    for &(a, b, c) in tb.highlights.iter().filter(|h| h.1 > from && h.0 < p.end) {
        let (a, b) = (a.max(from) - base, b.min(p.end) - base);
        let mut out = Vec::with_capacity(q.runs.len() + 2);
        for r in q.runs.drain(..) {
            if r.end <= a || r.start >= b || r.start == r.end {
                out.push(r);
                continue;
            }
            if r.start < a {
                out.push(kn_model::Run { start: r.start, end: a, style: r.style.clone() });
            }
            let mut st = r.style.clone();
            st.background = Some(c);
            out.push(kn_model::Run { start: r.start.max(a), end: r.end.min(b), style: st });
            if r.end > b {
                out.push(kn_model::Run { start: b, end: r.end, style: r.style });
            }
        }
        q.runs = out;
    }
    let one = TextBox {
        storage: tb.storage,
        text: tb.text[from..p.end].to_string(),
        paragraphs: vec![q],
        valign: kn_model::VAlign::Top,
        padding: [0.0; 4],
        shrink_to_fit: false,
        columns: 1,
        column_gap: 0.0,
        autogrow: false,
        highlights: tb.highlights.iter().filter(|h| h.1 > from && h.0 < p.end).map(|h| (h.0.max(from) - base, h.1.min(p.end) - base, h.2)).collect(),
        fields: Vec::new(),
    };
    let mut tl = text::layout(fonts, &one, width, 1.0e6, false, true);
    // a paragraph after the first continues the body: Pages spaces its first line from the line
    // before as continuous text (line spacing x ascent after that line's descent), not as a text
    // box's first line (line gap + ascent), so an empty or one-line paragraph is as tall as a line
    // of the paragraph around it
    if first && pi > 0 && from == p.start && !fonts.word {
        let g0 = tl.glyphs.iter().filter(|g| g.line == 0 && !g.label).max_by(|a, b| a.size.total_cmp(&b.size));
        let nominal = p.runs.first().map(|r| r.style.size).unwrap_or(12.0);
        let small_caps = p.runs.first().is_some_and(|r| r.style.caps == kn_model::Caps::Small);
        let size = g0.map(|g| if small_caps { g.size.max(nominal) } else { g.size }).unwrap_or(nominal);
        let font = p.runs.first().map(|r| r.style.font_name.clone()).unwrap_or_default();
        let (asc, desc, gap) = kn_render::text::font_vmetrics(fonts, &font, g0.map(|g| (g.font_id, g.weight)));
        let (asc, desc, gap) = (asc * size, desc * size, gap * size);
        let natural = asc + desc + gap;
        let want = match p.line_spacing {
            kn_model::LineSpacing::Relative(r) => r * asc,
            kn_model::LineSpacing::AtLeast(v) => natural.max(v) - desc,
            kn_model::LineSpacing::Exactly(v) => v - desc,
            kn_model::LineSpacing::AtMost(v) => natural.min(v) - desc,
            kn_model::LineSpacing::Between(v) => natural + v - desc,
        };
        let delta = want - (gap + asc);
        if delta.abs() > 0.01 && want > 0.0 {
            for g in &mut tl.glyphs {
                g.y += delta;
            }
            for l in &mut tl.lines {
                l.top += delta;
                l.baseline += delta;
            }
            for d in &mut tl.decos {
                d.y += delta;
            }
            for h in &mut tl.highlights {
                h.1 += delta;
            }
            tl.content_h += delta;
        }
    }
    for g in &mut tl.glyphs {
        g.start += base;
        g.end += base;
        g.para = pi;
    }
    for l in &mut tl.lines {
        l.start += base;
        l.end += base;
        l.para = pi;
    }
    let n = tl.lines.len();
    let mut line_glyphs = vec![Vec::new(); n];
    for (i, g) in tl.glyphs.iter().enumerate() {
        if g.line < n {
            line_glyphs[g.line].push(i);
        }
    }
    let mut bands = Vec::with_capacity(n);
    for i in 0..n {
        let top = if i == 0 { 0.0 } else { bands.last().map(|b: &(f32, f32)| b.1).unwrap_or(0.0) };
        // (a Word document: the line box, though a line spaced under single puts its baseline
        // below it; Pages keeps the baseline inside)
        let bottom = if fonts.word {
            if i + 1 < n { tl.lines[i + 1].top } else { tl.content_h }
        } else if i + 1 < n {
            tl.lines[i + 1].top.max(tl.lines[i].baseline)
        } else {
            tl.content_h.max(tl.lines[i].baseline)
        };
        bands.push((top, bottom.max(top)));
    }
    let pc = Arc::new(Piece { tl, line_glyphs, bands });
    cache.pieces.insert(key, pc.clone());
    pc
}

/// Columns of the text area of a section (x, width), from the body's column style.
fn columns_for(pages: &Pages, setup: &PageSetup, body: Option<&Body>) -> Vec<(f32, f32)> {
    let k = pages.file();
    let mut count = 1u32;
    let mut gap = 0.05f32;
    if let Some(st) = body.and_then(|b| k.obj(b.storage)) {
        let style_id = st.msg("table_layout_style").and_then(|t| t.msgs("entries").next().and_then(|e| e.ref_id("object")));
        let ch = kn_model::style::chain(k, style_id);
        if let Some(c) = kn_model::style::lookup(&ch, |m| m.msg("column_properties").and_then(|p| p.msg("columns")).and_then(|c| c.msg("equal_columns")).cloned()) {
            count = c.u32("count").unwrap_or(1).clamp(1, 10);
            gap = c.f32("gap").unwrap_or(0.05);
        }
    }
    let w = setup.text_width();
    let gap_pt = if gap <= 1.0 { gap * w } else { gap };
    let cw = ((w - gap_pt * (count - 1) as f32) / count as f32).max(20.0);
    (0..count).map(|i| (setup.left + i as f32 * (cw + gap_pt), cw)).collect()
}

/// Lay out the whole document.
pub fn layout(pages: &Pages, fonts: &mut Fonts, cache: &mut Cache, opts: &Options) -> Layout {
    // a Word document: Word's font stand-ins and line metrics (kept for drawing it too)
    fonts.word = pages.word_layout;
    if cache.word != pages.word_layout {
        cache.pieces.clear();
        cache.tables.clear();
        cache.word = pages.word_layout;
    }
    let mut setup = pages.setup();
    // Screen View: one long page as wide as the window, the text in a readable column, no
    // headers / footers, page breaks or floating objects
    let screen = opts.screen_width.filter(|_| setup.body);
    if let Some(w) = screen {
        let col = (w - 48.0).clamp(160.0, 680.0);
        setup.width = w.max(col + 48.0);
        setup.left = (setup.width - col) / 2.0;
        setup.right = setup.left;
        setup.top = 36.0;
        setup.bottom = 36.0;
        setup.height = SCREEN_HEIGHT;
        setup.headers = false;
        setup.footers = false;
    }
    let k = pages.file();
    let sections = pages.sections();
    let floating = if screen.is_some() { Vec::new() } else { pages.floating() };
    let ctx = kn_model::text::TextContext { slide_number: 1 };
    let drawable = |id: ObjId| kn_model::scene::drawable(k, id, ctx);

    let body = if setup.body { pages.body_storage().and_then(|b| body::build(k, b, None, BuildCtx { show_deletions: opts.show_deletions, show_markup: opts.show_markup, word_labels: pages.word_layout, ..Default::default() })) } else { None };
    let mut columns = if screen.is_some() { vec![(setup.left, setup.text_width())] } else { columns_for(pages, &setup, body.as_ref()) };
    // the document's page setup (a Word section may have its own)
    let base_setup = setup.clone();
    let mut out_pages: Vec<Page> = Vec::new();
    let mut headings = Vec::new();
    // the room the last drop cap takes: (page, column, its right edge, its bottom); the lines
    // beside it start after it, in its paragraph and the next ones (Pages)
    let mut cap_room: Option<(usize, usize, f32, f32)> = None;

    let new_page = |index: usize, section: usize| Page {
        index,
        number: index + 1,
        section,
        lines: Vec::new(),
        objects: Vec::new(),
        headers: Vec::new(),
        footers: Vec::new(),
        footnotes: Vec::new(),
        background: None,
        start: 0,
        end: 0,
        toc: Vec::new(),
        fitted_rows: HashMap::new(),
        caps: Vec::new(),
        setup: None,
    };

    if let Some(body) = &body {
        let tb = &body.tb;
        // a header or footer reaching past its margin moves the body away from it (Pages)
        let (hf_top, hf_bottom) = if screen.is_some() { (0.0, f32::MAX) } else { header_footer_extent(pages, &sections, &setup, fonts, cache) };
        let mut top = setup.top.max(hf_top);
        let mut bottom = (setup.height - setup.bottom).min(hf_bottom);
        let mut page = new_page(0, 0);
        // a Word section with its own page size, orientation or margins: its pages take them
        // (set as its first page starts)
        macro_rules! enter_section {
            ($si:expr) => {{
                if screen.is_none() && !pages.section_pages.is_empty() {
                    let own = sections.get($si).and_then(|s| pages.section_pages.get(&s.id));
                    setup = own.cloned().unwrap_or_else(|| base_setup.clone());
                    top = setup.top.max(hf_top);
                    bottom = (setup.height - setup.bottom).min(hf_bottom);
                    columns = columns_for(pages, &setup, Some(body));
                    page.setup = own.cloned();
                }
            }};
        }
        enter_section!(0);
        let mut col = 0usize;
        let mut y = top;
        let mut section_i = 0usize;
        // where an object's wrap area is: moved by the drag in progress, if any
        let moved = |id: ObjId, mut g: kn_model::Geometry| {
            if let Some((_, dx, dy)) = opts.moving.iter().find(|m| m.0 == id) {
                g.x += dx;
                g.y += dy;
            }
            g
        };
        // the floating objects of each page that push text (computed per page as we go)
        let obstacles_for = |pi: usize| -> Vec<(f32, f32, f32, f32, u64)> {
            floating
                .iter()
                .filter(|(p, _)| *p == pi)
                .filter_map(|(_, id)| {
                    let m = k.obj(*id)?;
                    let d = m.find_super("TSD.DrawableArchive")?;
                    let wrap = d.msg("exterior_text_wrap");
                    let ty = wrap.and_then(|w| w.u64("type")).unwrap_or(4);
                    let margin = wrap.and_then(|w| w.f32("margin")).unwrap_or(12.0);
                    let g = moved(*id, kn_model::scene::geometry(d));
                    // 2 = no wrap in Pages' numbering (None); 0 = inline
                    if ty == 2 || ty == 0 {
                        return None;
                    }
                    Some((g.x - margin, g.y - margin, g.x + g.w + margin, g.y + g.h + margin, ty))
                })
                .collect()
        };
        let mut obstacles = obstacles_for(0);
        let mut footnote_h = 0.0f32;
        let mut footnotes_here: Vec<(ObjId, usize)> = Vec::new();

        // move to the next column / page
        macro_rules! next_column {
            ($force_page:expr) => {{
                if screen.is_some() {
                    // Screen View: a break is a little space
                    if !page.lines.is_empty() {
                        y += 24.0;
                    }
                } else if !$force_page && col + 1 < columns.len() {
                    col += 1;
                    y = top;
                } else {
                    finish_page(&mut page, &footnotes_here, k, fonts, cache, &setup, footnote_h);
                    let idx = out_pages.len() + 1;
                    out_pages.push(std::mem::replace(&mut page, new_page(idx, section_i)));
                    col = 0;
                    y = top;
                    obstacles = obstacles_for(idx);
                    footnote_h = 0.0;
                    footnotes_here.clear();
                }
            }};
        }

        let n = tb.paragraphs.len();
        let mut pi = 0usize;
        let mut prev_space_after = 0.0f32;
        while pi < n {
            let p = &tb.paragraphs[pi];
            let ex = &body.extra[pi];
            if let Some(sec) = ex.section {
                if let Some(si) = sections.iter().position(|s| s.id == sec) {
                    section_i = si;
                    if page.lines.is_empty() {
                        page.section = si;
                        enter_section!(si);
                        // (the page starts at its own top margin)
                        if !pages.section_pages.is_empty() && screen.is_none() {
                            y = top;
                        }
                    }
                }
            }
            let at_top = y <= top + 0.01;
            if ex.page_break_before && !at_top && !page.lines.is_empty() {
                next_column!(true);
            }
            if !(y <= top + 0.01) {
                // (a Word document: the larger of the space after and before, as Word and
                // OnlyOffice space two paragraphs; Pages adds them)
                y += if fonts.word { prev_space_after.max(p.space_before).max(0.0) } else { prev_space_after.max(0.0) + p.space_before };
            }
            // a Word document: a top border's distance and width above the text
            if fonts.word && ex.border.is_some() && (ex.border_edges & 1 != 0) {
                y += ex.border_offset.unwrap_or(0.0) + ex.border.map(|b| b.1).unwrap_or(0.0);
            }
            let col_x = columns[col].0;
            let col_w = columns[col].1;

            // a table of contents: its entries as a block
            if let Some(toc) = ex.toc {
                // the TOC's own settings say which paragraph styles it lists (and in what style);
                // without them, the heading styles
                let spec = toc_spec(k, toc);
                let entries = if spec.is_empty() { toc_entries(body, &headings_of(body, k)) } else { toc_entries_from(body, k, &spec) };
                let style = k
                    .obj(toc)
                    .and_then(|m| m.find_super("TSWP.ShapeInfoArchive"))
                    .and_then(|i| i.ref_id("owned_storage"))
                    .and_then(|s| k.obj(s))
                    .and_then(|s| s.msg("table_para_style").and_then(|t| t.msgs("entries").find_map(|e| e.ref_id("object"))))
                    ;
                let (style, gap) = style.map(|ps| toc_entry_style(k, ps)).unwrap_or_else(|| (kn_model::CharStyle { size: 12.0, ..Default::default() }, 0.0));
                let row = (style.size * 1.2 + gap).max(12.0);
                // row by row, continuing on the next page when the page is full
                let mut cur: Vec<TocEntry> = Vec::new();
                let mut block_y = y;
                for e in &entries {
                    let r = e.style.as_ref().map(|s| (s.size * 1.2 + e.space.unwrap_or(gap)).max(12.0)).unwrap_or(row);
                    if y + r > bottom - footnote_h && !(y <= top + 0.01) {
                        page.toc.push(TocBlock { para: pi, style: style.clone(), y: block_y, entries: std::mem::take(&mut cur) });
                        next_column!(false);
                        block_y = y;
                    }
                    cur.push(TocEntry { y, ..e.clone() });
                    y += r;
                }
                if entries.is_empty() {
                    y += row;
                }
                let h = 0.0f32;
                page.toc.push(TocBlock { para: pi, style, y: block_y, entries: cur });
                // the paragraph's line (empty) so the caret has a place
                let pc = shape(fonts, cache, tb, pi, p.start, col_w, true);
                page.lines.push(PlacedLine { para: pi, piece: pc, line: 0, dx: col_x, dy: y + h - 14.0, column: col });
                prev_space_after = p.space_after;
                pi += 1;
                if let Some(b) = ex.brk {
                    match b {
                        Break::Column => next_column!(false),
                        Break::Page | Break::Section => next_column!(true),
                    }
                    prev_space_after = 0.0;
                }
                continue;
            }

            // inline drawables (a paragraph that holds only objects): a block of their height
            if !ex.drawables.is_empty() {
                let mut block_h = 0.0f32;
                let text_empty = tb.text[p.start..p.end].trim().is_empty();
                for id in &ex.drawables {
                    let Some(m) = k.obj(*id) else { continue };
                    let Some(d) = m.find_super("TSD.DrawableArchive") else { continue };
                    let wrap = d.msg("exterior_text_wrap").and_then(|w| w.u64("type")).unwrap_or(0);
                    let mut g = kn_model::scene::geometry(d);
                    if let Some(rows) = fit_table(k, fonts, cache, *id) {
                        g.h = rows.iter().sum();
                    }
                    if wrap == 0 {
                        block_h = block_h.max(g.h);
                    }
                }
                // (objects among text sit on their lines: placed after the layout)
                if block_h > 0.0 && text_empty {
                    // like big characters: side by side while they fit the column, then a new
                    // row; onto the next page when a row doesn't fit what's left
                    let left = col_x + p.left_indent;
                    let mut x = left;
                    let mut row_h = 0.0f32;
                    let placed: Vec<(ObjId, kn_model::Geometry, u64)> = ex
                        .drawables
                        .iter()
                        .filter_map(|id| {
                            let d = k.obj(*id)?.find_super("TSD.DrawableArchive")?;
                            let wrap = d.msg("exterior_text_wrap").and_then(|w| w.u64("type")).unwrap_or(0);
                            let mut g = kn_model::scene::geometry(d);
                            if let Some(rows) = fit_table(k, fonts, cache, *id) {
                                g.h = rows.iter().sum();
                            }
                            (wrap == 0).then_some((*id, g, wrap))
                        })
                        .collect();
                    let alone = placed.len() == 1;
                    for (id, mut g, wrap) in placed {
                        // a table taller than what's left of the page goes on over the next
                        // pages, split between rows (its header rows repeated), as in Pages and
                        // Word
                        let mut shown: Option<Arc<Vec<u32>>> = None;
                        let mut shown_parts: Option<Arc<Vec<RowPart>>> = None;
                        // (Word fits to the point, no slack)
                        let slack = if fonts.word { 0.0 } else { 0.5 };
                        if alone && wrap == 0 && screen.is_none() && y + g.h > bottom - footnote_h + slack && k.type_name(id) == "TST.TableInfoArchive" {
                            let info = k.obj(id);
                            let header = info.and_then(|m| kn_model::table::table_shape(k, m)).map(|t| t.header_rows as usize).unwrap_or(0);
                            let heights: Vec<f32> = match fit_table(k, fonts, cache, id) {
                                Some(r) => r.to_vec(),
                                None => info.and_then(|m| kn_model::table::table(k, m)).map(|t| t.row_heights).unwrap_or_default(),
                            };
                            let n = heights.len();
                            let header = header.min(n);
                            // (a Word document breaks a row across pages too, a single-row table
                            // as well)
                            let break_rows = fonts.word;
                            if n > header + 1 || (break_rows && n > header) {
                                let head_h: f32 = heights[..header].iter().sum();
                                let mut start = header;
                                let mut whole = true;
                                // a row continued from the page before: where each cell goes on
                                let mut carry: Option<(usize, Vec<usize>)> = None;
                                loop {
                                    let avail = bottom - footnote_h - y;
                                    if std::env::var_os("CS_DEBUG_SPLIT").is_some() {
                                        eprintln!("split table {id} at y {y:.2} avail {avail:.2} start {start} heights {heights:?} carry {carry:?}");
                                    }
                                    let mut take: Vec<u32> = (0..header as u32).collect();
                                    let mut parts: Vec<RowPart> = Vec::new();
                                    let mut h = head_h;
                                    let mut i = start;
                                    // the rest of a row broken on the page before
                                    if let Some((row, from)) = carry.take() {
                                        let cells = cell_lines(k, fonts, id, row);
                                        let (ranges, used, rest) = break_row(&cells, &from, avail - h);
                                        take.push(row as u32);
                                        if rest.is_some() {
                                            parts.push(RowPart { row: row as u32, ranges, height: used });
                                            h += used;
                                            carry = rest.map(|r| (row, r));
                                            i = row;
                                        } else {
                                            let need = cells.iter().map(|c| c.rest_height(from.get(c.col).copied().unwrap_or(0))).fold(0.0f32, f32::max);
                                            parts.push(RowPart { row: row as u32, ranges, height: need });
                                            h += need;
                                            i = row + 1;
                                        }
                                    }
                                    while carry.is_none() && i < n && h + heights[i] <= avail + slack {
                                        take.push(i as u32);
                                        h += heights[i];
                                        i += 1;
                                    }
                                    // the next row broken here: what fits of each cell
                                    if break_rows && carry.is_none() && i < n && avail - h > 1.0 {
                                        let cells = cell_lines(k, fonts, id, i);
                                        let from = vec![0usize; cells.iter().map(|c| c.col + 1).max().unwrap_or(0)];
                                        let (ranges, used, rest) = break_row(&cells, &from, avail - h);
                                        if std::env::var_os("CS_DEBUG_SPLIT").is_some() {
                                            eprintln!("break row {i}: room {:.2} used {:.2} rest {:?} cells {:?}", avail - h, used, rest, cells.iter().map(|c| (c.col, c.pad, c.lines.clone())).collect::<Vec<_>>());
                                        }
                                        if let (Some(rest), true) = (rest, used > 0.0) {
                                            take.push(i as u32);
                                            parts.push(RowPart { row: i as u32, ranges, height: used });
                                            h += used;
                                            carry = Some((i, rest));
                                        }
                                    }
                                    if carry.is_none() && i == start && parts.is_empty() {
                                        // not one row fits here: on the next page (a row taller
                                        // than a page goes alone)
                                        if !(y <= top + 0.01) {
                                            next_column!(false);
                                            continue;
                                        }
                                        take.push(i as u32);
                                        h += heights[i];
                                        i += 1;
                                    }
                                    if i >= n && carry.is_none() {
                                        g.h = h;
                                        shown = (!whole).then(|| Arc::new(take));
                                        shown_parts = (!parts.is_empty()).then(|| Arc::new(parts));
                                        break;
                                    }
                                    let cx = columns[col].0;
                                    let ox = match p.align {
                                        kn_model::Align::Center => cx + (columns[col].1 - g.w) / 2.0,
                                        kn_model::Align::Right => cx + columns[col].1 - g.w,
                                        _ => cx + p.left_indent,
                                    };
                                    page.objects.push(PlacedObject { id, x: ox, y, w: g.w, h, inline: true, wrap, margin: 0.0, layout: false, rows: Some(Arc::new(take)), row_parts: (!parts.is_empty()).then(|| Arc::new(parts)) });
                                    next_column!(false);
                                    whole = false;
                                    start = i;
                                }
                            }
                        }
                        if x > left && x + g.w > col_x + col_w + 0.5 {
                            y += row_h;
                            row_h = 0.0;
                            x = left;
                        }
                        if y + g.h > bottom - footnote_h && !(y <= top + 0.01) {
                            next_column!(false);
                            row_h = 0.0;
                            x = left;
                        }
                        let ox = match (p.align, alone) {
                            (kn_model::Align::Center, true) => col_x + (col_w - g.w) / 2.0,
                            (kn_model::Align::Right, true) => col_x + col_w - g.w,
                            _ => x,
                        };
                        page.objects.push(PlacedObject { id, x: ox, y, w: g.w, h: g.h, inline: true, wrap, margin: 0.0, layout: false, rows: shown, row_parts: shown_parts });
                        x += g.w + 4.0;
                        row_h = row_h.max(g.h);
                    }
                    let block_h = row_h;
                    if text_empty {
                        let pc = shape(fonts, cache, tb, pi, p.start, col_w, true);
                        let lh = pc.bands.first().map(|b| b.1 - b.0).unwrap_or(14.0);
                        page.lines.push(PlacedLine { para: pi, piece: pc, line: 0, dx: x, dy: y + block_h - lh, column: col });
                        y += block_h;
                        // a Word document: a picture's line is spaced like a text line, the
                        // multiple's extra under it (tables aren't)
                        if fonts.word && !ex.drawables.iter().any(|id| k.type_name(*id) == "TST.TableInfoArchive") {
                            if let kn_model::LineSpacing::Relative(r) = p.line_spacing {
                                if r > 1.0 {
                                    y += lh - lh / r;
                                }
                            }
                        }
                        prev_space_after = p.space_after;
                        after_paragraph(&mut page, ex.brk, &mut pi);
                        if let Some(b) = ex.brk {
                            match b {
                                Break::Column => next_column!(false),
                                Break::Page | Break::Section => next_column!(true),
                            }
                        }
                        continue;
                    }
                    y += block_h;
                }
                // anchored ones (wrap ≠ 0) sit on this page where the file says
                for id in &ex.drawables {
                    let Some(m) = k.obj(*id) else { continue };
                    let Some(d) = m.find_super("TSD.DrawableArchive") else { continue };
                    let wrap = d.msg("exterior_text_wrap");
                    let ty = wrap.and_then(|w| w.u64("type")).unwrap_or(0);
                    if ty != 0 {
                        let g = kn_model::scene::geometry(d);
                        let margin = wrap.and_then(|w| w.f32("margin")).unwrap_or(12.0);
                        page.objects.push(PlacedObject { id: *id, x: g.x, y: g.y, w: g.w, h: g.h, inline: false, wrap: ty, margin, layout: false, rows: None, row_parts: None });
                        if ty != 2 {
                            let g = moved(*id, g);
                            obstacles.push((g.x - margin, g.y - margin, g.x + g.w + margin, g.y + g.h + margin, ty));
                        }
                    }
                }
            }

            // keep together / with next and orphan control: does the paragraph (or its first
            // lines) fit what is left of the column?
            let full = shape(fonts, cache, tb, pi, p.start, col_w - p.right_indent.max(0.0) * 0.0, true);
            let total_h = full.tl.content_h;
            let first_h = full.bands.first().map(|b| b.1).unwrap_or(14.0);
            let room = bottom - footnote_h - y;
            // a paragraph holding an inline table (a Word document's): the table's height, all
            // of it when Word keeps it on one page, else its first row
            let inline_table = |exi: &crate::body::ParaExtra| -> Option<f32> {
                if !pages.word_layout {
                    return None;
                }
                let id = exi.drawables.iter().copied().find(|d| k.type_name(*d) == "TST.TableInfoArchive")?;
                let t = kn_model::table::table(k, k.obj(id)?)?;
                Some(if pages.keep_tables.contains(&id) { t.row_heights.iter().sum() } else { t.row_heights.first().copied().unwrap_or(0.0) })
            };
            if !(y <= top + 0.01) && !page.lines.is_empty() {
                let mut need = if ex.keep_together { total_h } else if ex.widow_control && full.bands.len() >= 2 { full.bands.get(1).map(|b| b.1).unwrap_or(first_h) } else { first_h };
                if let Some(th) = inline_table(ex).filter(|_| pages.keep_tables.iter().any(|t| ex.drawables.contains(t))) {
                    need = need.max(th);
                }
                if ex.keep_with_next && pi + 1 < n {
                    let q = &tb.paragraphs[pi + 1];
                    let next = shape(fonts, cache, tb, pi + 1, q.start, col_w, true);
                    let next_h = inline_table(&body.extra[pi + 1]).unwrap_or_else(|| next.bands.first().map(|b| b.1).unwrap_or(14.0));
                    need = total_h + q.space_before + p.space_after + next_h;
                }
                if need > room && need <= bottom - top {
                    next_column!(false);
                }
            }

            // drop cap: the first characters big beside the first lines (placed with the
            // paragraph's first line, so it follows the paragraph to another column / page)
            let mut cap_lines = 0u32;
            let mut from = p.start;
            let mut pending_cap: Option<PendingCap> = None;
            if let Some((chars, lines)) = ex.drop_cap {
                // leading tabs / spaces aren't the cap (Pages puts the "9" of "\t9Involve…" in it)
                let cap_start = tb.text[p.start..p.end].char_indices().find(|(_, c)| !c.is_whitespace()).map(|(i, _)| p.start + i).unwrap_or(p.end);
                let cap_end = tb.text[cap_start..p.end].char_indices().nth(chars as usize).map(|(i, _)| cap_start + i).unwrap_or(p.end);
                if cap_end > cap_start && cap_end < p.end {
                    let line_h = full.bands.first().map(|b| b.1 - b.0).unwrap_or(14.0);
                    let fs = line_h / 1.2;
                    let base1 = full.tl.lines.first().map(|l| l.baseline).unwrap_or(fs);
                    let lines = lines.max(1);
                    if let Some((fill, radius, padding, scale)) = ex.drop_cap_shape {
                        // Pages' Shape drop cap: a box `lines` lines tall, the characters
                        // centred in it at `scale`
                        let box_h = line_h * lines as f32 - 2.0;
                        let glyph_h = box_h * scale * 0.8;
                        let cap_size = glyph_h / 0.72;
                        let cap = cap_piece(fonts, cache, tb, pi, cap_start, cap_end, cap_size);
                        let pad = padding.max(box_h * 0.12);
                        let box_w = cap.tl.content_w + 2.0 * pad;
                        let base = cap.tl.lines.first().map(|l| l.baseline).unwrap_or(cap_size);
                        pending_cap = Some(PendingCap {
                            piece: cap.clone(),
                            glyph: (pad, 1.0 + (box_h + glyph_h) / 2.0 - base),
                            rect: Some((0.0, 1.0, box_w, box_h, fill, radius.max(0.0) * box_w.min(box_h))),
                            width: box_w + 6.0,
                            height: line_h * lines as f32,
                        });
                    } else {
                        // the capitals reach from the first line's cap height down to the last
                        // cap line's baseline (cap height ~0.7 em)
                        let cap_size = ((lines - 1) as f32 * line_h + fs * 0.7) / 0.7;
                        let cap = cap_piece(fonts, cache, tb, pi, cap_start, cap_end, cap_size);
                        let rel = base1 + (lines - 1) as f32 * line_h - cap.tl.lines.first().map(|l| l.baseline).unwrap_or(cap_size);
                        pending_cap = Some(PendingCap { piece: cap.clone(), glyph: (0.0, rel), rect: None, width: cap.tl.content_w + 4.0, height: line_h * lines as f32 });
                    }
                    cap_lines = lines;
                    from = cap_end;
                }
            }

            // place the lines
            let mut first = from == p.start;
            let mut placed_in_para = 0usize;
            let mut guard = 0;
            'pieces: loop {
                guard += 1;
                if guard > 10_000 {
                    break;
                }
                let col_x = columns[col].0;
                let col_w = columns[col].1;
                // the width available at this y: obstacles and the drop cap narrow it
                let est_h = full.bands.first().map(|b| b.1 - b.0).unwrap_or(14.0);
                let (mut x0, mut x1) = (col_x, col_x + col_w);
                if let Some((pg, c, xr, bt)) = cap_room {
                    if pg == page.index && c == col && y < bt - 0.5 {
                        x0 = x0.max(xr);
                    }
                }
                if pending_cap.as_ref().is_some_and(|_| cap_lines > 0) {
                    x0 += pending_cap.as_ref().map(|c| c.width).unwrap_or(0.0);
                }
                let mut blocked_until: Option<f32> = None;
                for &(ox0, oy0, ox1, oy1, ty) in &obstacles {
                    if oy1 <= y || oy0 >= y + est_h || ox1 <= x0 || ox0 >= x1 {
                        continue;
                    }
                    // 3 = above and below; else go around on the wider side
                    let left_room = ox0 - x0;
                    let right_room = x1 - ox1;
                    if ty == 3 || (left_room < 60.0 && right_room < 60.0) {
                        blocked_until = Some(blocked_until.map_or(oy1, |b: f32| b.max(oy1)));
                    } else if left_room >= right_room {
                        x1 = ox0;
                    } else {
                        x0 = ox1;
                    }
                }
                if let Some(b) = blocked_until {
                    y = b;
                    if y + est_h > bottom - footnote_h {
                        next_column!(false);
                    }
                    continue 'pieces;
                }
                let width = (x1 - x0).max(20.0);
                let pc = if first && (width - col_w).abs() < 0.01 { full.clone() } else { shape(fonts, cache, tb, pi, from, width, first) };
                let nlines = pc.tl.lines.len();
                let mut li = 0;
                while li < nlines {
                    let (a, b) = pc.bands[li];
                    let h = b - a;
                    // the next line would overflow the column
                    if y + h > bottom - footnote_h + 0.01 && !(y <= top + 0.01) {
                        // widow control: don't leave a paragraph's last line alone on the next page
                        if ex.widow_control && li + 1 == nlines && li >= 2 && placed_in_para >= 2 {
                            if let Some(last) = page.lines.pop() {
                                let start = last.range().0;
                                next_column!(false);
                                from = start;
                                first = false;
                                placed_in_para -= 1;
                                continue 'pieces;
                            }
                        }
                        let start = pc.tl.lines[li].start;
                        next_column!(false);
                        if li == 0 && first {
                            continue 'pieces;
                        }
                        from = start;
                        first = false;
                        continue 'pieces;
                    }
                    // the width changed (an obstacle starts or ends here, or the drop cap is done)
                    if li > 0 {
                        let (mut nx0, mut nx1) = (col_x, col_x + col_w);
                        if let Some((pg, c, xr, bt)) = cap_room {
                            if pg == page.index && c == col && y < bt - 0.5 {
                                nx0 = nx0.max(xr);
                            }
                        }
                        let mut blocked = false;
                        for &(ox0, oy0, ox1, oy1, ty) in &obstacles {
                            if oy1 <= y || oy0 >= y + h || ox1 <= nx0 || ox0 >= nx1 {
                                continue;
                            }
                            let left_room = ox0 - nx0;
                            let right_room = nx1 - ox1;
                            if ty == 3 || (left_room < 60.0 && right_room < 60.0) {
                                blocked = true;
                            } else if left_room >= right_room {
                                nx1 = ox0;
                            } else {
                                nx0 = ox1;
                            }
                        }
                        if blocked || (nx0 - x0).abs() > 0.5 || (nx1 - x1).abs() > 0.5 {
                            from = pc.tl.lines[li].start;
                            first = false;
                            continue 'pieces;
                        }
                    }
                    let dy = y - a;
                    // footnotes referenced on this line take room at the page bottom
                    let (ls, le) = (pc.tl.lines[li].start, pc.tl.lines[li].end);
                    for sp in body.spans.iter().filter(|s| s.start >= ls && s.start < le.max(ls + 1)) {
                        if let SpanKind::Footnote { storage, number } = sp.kind {
                            if !footnotes_here.iter().any(|f| f.0 == storage) {
                                footnotes_here.push((storage, number));
                                footnote_h += footnote_height(k, storage, number, fonts, cache, columns[0].1 + (columns.len() - 1) as f32 * 0.0) + if footnotes_here.len() == 1 { 18.0 } else { 4.0 };
                            }
                        }
                    }
                    if let Some(cap) = pending_cap.take() {
                        // the paragraph's first line: its drop cap goes beside it, here
                        let cx = col_x + p.left_indent;
                        page.lines.push(PlacedLine { para: pi, piece: cap.piece.clone(), line: 0, dx: cx + cap.glyph.0, dy: y + cap.glyph.1, column: col });
                        if let Some((rx, ry, rw, rh, fill, radius)) = cap.rect {
                            page.caps.push(CapBox { para: pi, piece: cap.piece.clone(), rect: (cx + rx, y + ry, rw, rh), fill: Some(fill), radius });
                        } else {
                            page.caps.push(CapBox { para: pi, piece: cap.piece.clone(), rect: (cx, y, cap.width, cap.height), fill: None, radius: 0.0 });
                        }
                        cap_room = Some((page.index, col, cx + cap.width, y + cap.height));
                    }
                    page.lines.push(PlacedLine { para: pi, piece: pc.clone(), line: li, dx: x0, dy, column: col });
                    y += h;
                    placed_in_para += 1;
                    li += 1;
                }
                break;
            }
            let _ = cap_lines;
            if ex.toc.is_none() && (ex.show_in_toc || is_heading(k, ex.style)) {
                headings.push((p.start, heading_level(k, ex.style), tb.text[p.start..p.end].to_string(), out_pages.len()));
            }
            prev_space_after = p.space_after;
            // a Word document: a bottom border's distance and width under the text
            if fonts.word && ex.border.is_some() && (ex.border_edges & 2 != 0) {
                y += ex.border_offset.unwrap_or(0.0) + ex.border.map(|b| b.1).unwrap_or(0.0);
            }
            let brk = ex.brk;
            pi += 1;
            if let Some(b) = brk {
                match b {
                    Break::Column => next_column!(false),
                    Break::Page | Break::Section => next_column!(true),
                }
                if b == Break::Section {
                    // the next paragraph starts the next section (its entry in table_section)
                    if section_i + 1 < sections.len() {
                        section_i += 1;
                        page.section = section_i;
                        enter_section!(section_i);
                        if !pages.section_pages.is_empty() && screen.is_none() {
                            y = top;
                        }
                    }
                }
                prev_space_after = 0.0;
            }
        }
        if screen.is_some() {
            // the page ends under the text (footnotes follow it)
            setup.height = (y + footnote_h + setup.bottom + 24.0).max(400.0);
            finish_page(&mut page, &footnotes_here, k, fonts, cache, &setup, footnote_h);
        } else {
            finish_page(&mut page, &footnotes_here, k, fonts, cache, &setup, footnote_h);
        }
        out_pages.push(page);
        // inline objects among text: on the baseline where their stand-in character is
        for page in out_pages.iter_mut() {
            let mut add = Vec::new();
            for l in &page.lines {
                let p = &tb.paragraphs[l.para];
                if tb.text[p.start..p.end].trim().is_empty() {
                    continue;
                }
                let (a, b) = l.range();
                for sp in body.spans.iter().filter(|sp| sp.start >= a && sp.start < b.max(a + 1)) {
                    let body::SpanKind::Object(d) = sp.kind else { continue };
                    let Some((w, h)) = body::inline_size(k, d) else { continue };
                    let Some(line) = l.piece.tl.lines.get(l.line) else { continue };
                    let (x, _, _) = piece_caret(&l.piece, sp.start);
                    add.push(PlacedObject { id: d, x: l.dx + x, y: l.dy + line.baseline - h, w, h, inline: true, wrap: 0, margin: 0.0, layout: false, rows: None, row_parts: None });
                }
            }
            page.objects.extend(add);
        }
        // text ranges per page
        let total = tb.text.len();
        for i in 0..out_pages.len() {
            let start = out_pages[i].lines.iter().map(|l| l.range().0).min().unwrap_or(if i > 0 { out_pages[i - 1].end } else { 0 });
            out_pages[i].start = start;
        }
        for i in 0..out_pages.len() {
            out_pages[i].end = out_pages.get(i + 1).map(|p| p.start).unwrap_or(total);
        }
    } else {
        // page layout: as many pages as the file has (or that hold objects)
        let n = floating.iter().map(|(p, _)| p + 1).max().unwrap_or(0).max(pages.saved_page_count()).max(opts.min_pages);
        for i in 0..n {
            out_pages.push(new_page(i, 0));
        }
    }

    // floating objects, back to front, behind the objects in the text
    let mut placed: HashMap<usize, usize> = HashMap::new();
    for (pi, id) in &floating {
        if let Some(page) = out_pages.get_mut(*pi) {
            let at = placed.entry(*pi).or_insert(0);
            if let Some(m) = k.obj(*id) {
                if let Some(d) = m.find_super("TSD.DrawableArchive") {
                    let g = kn_model::scene::geometry(d);
                    let wrap = d.msg("exterior_text_wrap");
                    *at += 1;
                    page.objects.insert(
                        (*at - 1).min(page.objects.len()),
                        PlacedObject { id: *id, x: g.x, y: g.y, w: g.w, h: g.h, inline: false, wrap: wrap.and_then(|w| w.u64("type")).unwrap_or(4), margin: wrap.and_then(|w| w.f32("margin")).unwrap_or(12.0), layout: false, rows: None, row_parts: None },
                    );
                }
            }
        }
    }
    let _ = drawable;

    // page numbers per section, backgrounds, headers and footers
    let count = out_pages.len();
    let mut number = 0usize;
    let mut last_section = usize::MAX;
    let mut page_in_section = 0usize;
    for page in out_pages.iter_mut() {
        let sec = sections.get(page.section);
        if page.section != last_section {
            last_section = page.section;
            page_in_section = 0;
            if let Some(s) = sec {
                if s.page_number_kind != 0 || number == 0 {
                    number = if s.page_number_kind != 0 { s.page_number_start.max(1) as usize - 1 } else { number };
                }
            }
        } else {
            page_in_section += 1;
        }
        number += 1;
        page.number = number;
        page.background = sec.and_then(|s| s.background.clone());
        if let (Some(s), None) = (sec, screen) {
            layout_objects(pages, s, page, page_in_section);
            let ps = page.setup.clone().unwrap_or_else(|| base_setup.clone());
            header_footers(pages, s, page, page_in_section, count, &ps, fonts, cache);
        }
    }
    // tables grow to their text
    for page in out_pages.iter_mut() {
        for o in page.objects.iter_mut() {
            if k.type_name(o.id) == "TST.TableInfoArchive" {
                if let Some(rows) = fit_table(k, fonts, cache, o.id) {
                    o.h = rows.iter().sum();
                    page.fitted_rows.insert(o.id, rows);
                }
            }
        }
    }
    // headings now know their pages
    for h in &mut headings {
        let p = out_pages.iter().rposition(|p| p.start <= h.0).unwrap_or(0);
        h.3 = out_pages.get(p).map(|p| p.number).unwrap_or(1);
    }
    // TOC entries show the pages
    let numbers: Vec<(usize, usize)> = out_pages.iter().map(|p| (p.start, p.number)).collect();
    for page in &mut out_pages {
        for b in &mut page.toc {
            for e in &mut b.entries {
                e.page = numbers.iter().rev().find(|(s, _)| *s <= e.target).map(|(_, n)| *n).unwrap_or(1);
            }
        }
    }
    cache.sweep();
    // (the document's own setup; a page of a section with its own carries that)
    let _ = setup;
    let columns = if pages.section_pages.is_empty() || screen.is_some() { columns } else { columns_for(pages, &base_setup, body.as_ref()) };
    Layout { setup: base_setup, pages: out_pages, body, columns, headings }
}

/// What the layout shows.
#[derive(Clone)]
pub struct Options {
    pub show_deletions: bool,
    pub show_markup: bool,
    /// Page-layout documents: at least this many pages (pages added this session).
    pub min_pages: usize,
    /// Screen View: the window's width in points (word-processing documents reflow to it).
    pub screen_width: Option<f32>,
    /// Objects being dragged and how far (dx, dy): text wraps around where they are now, not
    /// where the file says (Pages reflows live while an object moves).
    pub moving: Vec<(ObjId, f32, f32)>,
}

/// Screen View's page height while laying out (cut to the text after).
const SCREEN_HEIGHT: f32 = 1.0e6;

impl Default for Options {
    fn default() -> Self {
        Options { show_deletions: true, show_markup: true, min_pages: 1, screen_width: None, moving: Vec::new() }
    }
}

fn after_paragraph(_page: &mut Page, _brk: Option<Break>, pi: &mut usize) {
    *pi += 1;
}

fn is_heading(k: &kn_format::KeyFile, style: Option<ObjId>) -> bool {
    let name = style.and_then(|s| k.obj(s)).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.str("name")).unwrap_or("");
    name == "Title" || name.starts_with("Heading")
}

fn heading_level(k: &kn_format::KeyFile, style: Option<ObjId>) -> usize {
    let name = style.and_then(|s| k.obj(s)).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.str("name")).unwrap_or("");
    match name {
        "Title" => 0,
        "Heading" => 1,
        "Heading 2" => 2,
        "Heading 3" => 3,
        _ => 1,
    }
}

fn headings_of(body: &Body, k: &kn_format::KeyFile) -> Vec<(usize, usize, String)> {
    body.tb
        .paragraphs
        .iter()
        .zip(&body.extra)
        .filter(|(p, ex)| ex.toc.is_none() && ex.drawables.is_empty() && (ex.show_in_toc || is_heading(k, ex.style)) && heading_level(k, ex.style) > 0 && !body.tb.text[p.start..p.end].trim().is_empty())
        .map(|(p, ex)| {
            // the heading's text without footnote numbers
            let text: String = body.tb.text[p.start..p.end]
                .char_indices()
                .filter(|(i, _)| !body.spans.iter().any(|s| matches!(s.kind, crate::body::SpanKind::Footnote { .. }) && (s.start..s.end).contains(&(p.start + i))))
                .map(|(_, c)| c)
                .collect();
            (p.start, heading_level(k, ex.style), text)
        })
        .collect()
}

/// A TOC entry style (TSWP.TOCEntryStyleArchive, its properties in the paragraph style it
/// extends): the text style and the space between entries.
fn toc_entry_style(k: &kn_format::KeyFile, ps: kn_format::ObjId) -> (kn_model::CharStyle, f32) {
    let mut cs = kn_model::CharStyle::default();
    let mut gap = None;
    for m in kn_model::style::chain(k, Some(ps)).iter().rev() {
        let m = m.find_super("TSWP.ParagraphStyleArchive").unwrap_or(m);
        if let Some(p) = m.msg("char_properties") {
            kn_model::style::apply_char_props(&mut cs, p);
        }
        if let Some(p) = m.msg("para_properties") {
            let (b, a) = (p.f32("space_before"), p.f32("space_after"));
            if b.is_some() || a.is_some() {
                gap = Some(b.unwrap_or(0.0) + a.unwrap_or(0.0));
            }
        }
    }
    (cs, gap.unwrap_or(0.0))
}

fn toc_entries(_body: &Body, heads: &[(usize, usize, String)]) -> Vec<TocEntry> {
    heads.iter().map(|(s, lvl, t)| TocEntry { text: t.clone(), level: *lvl, page: 0, target: *s, y: 0.0, style: None, indent: None, space: None }).collect()
}

/// A TOC's settings (TSWP.TOCSettingsArchive): the paragraph styles it lists, each with its
/// entry style.
fn toc_spec(k: &kn_format::KeyFile, toc_info: ObjId) -> Vec<(ObjId, Option<ObjId>)> {
    let Some(settings) = k.obj(toc_info).and_then(|m| m.ref_id("toc_settings")).and_then(|s| k.obj(s)) else { return Vec::new() };
    settings.msgs("entries").filter(|e| e.bool("show_in_toc").unwrap_or(false)).filter_map(|e| Some((e.ref_id("paragraph_style")?, e.ref_id("toc_entry_style")))).collect()
}

/// The paragraphs a TOC's settings list (a paragraph counts when its style, or a style it
/// varies, is listed), with each entry's style and indent.
fn toc_entries_from(body: &Body, k: &kn_format::KeyFile, spec: &[(ObjId, Option<ObjId>)]) -> Vec<TocEntry> {
    let listed = |st: Option<ObjId>| -> Option<Option<ObjId>> {
        let ch = kn_model::style::chain(k, st);
        ch.iter().find_map(|m| spec.iter().find(|(ps, _)| k.obj(*ps).is_some_and(|x| std::ptr::eq(x, *m))).map(|(_, es)| *es))
    };
    let mut out = Vec::new();
    for (p, ex) in body.tb.paragraphs.iter().zip(&body.extra) {
        if ex.toc.is_some() {
            continue;
        }
        let Some(es) = listed(ex.style) else { continue };
        let text: String = body.tb.text[p.start..p.end]
            .char_indices()
            .filter(|(i, c)| *c != crate::body::OBJECT_CHAR && !body.spans.iter().any(|s| matches!(s.kind, crate::body::SpanKind::Footnote { .. }) && (s.start..s.end).contains(&(p.start + i))))
            .map(|(_, c)| c)
            .collect();
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let (style, indent, space) = match es {
            Some(es) => {
                let (cs, gap) = toc_entry_style(k, es);
                let ind = kn_model::style::chain(k, Some(es)).iter().find_map(|m| m.find_super("TSWP.ParagraphStyleArchive").unwrap_or(m).msg("para_properties").and_then(|pp| pp.f32("left_indent")));
                (Some(cs), ind, Some(gap))
            }
            None => (None, None, None),
        };
        out.push(TocEntry { text, level: 1, page: 0, target: p.start, y: 0.0, style, indent, space });
    }
    out
}

/// A drop cap waiting for its paragraph's first line: its characters, where they sit and the
/// shape (relative to the line's top-left), and the room it takes (width, height).
struct PendingCap {
    piece: Arc<Piece>,
    glyph: (f32, f32),
    rect: Option<(f32, f32, f32, f32, kn_model::Color, f32)>,
    width: f32,
    height: f32,
}

/// The drop cap characters, shaped big.
fn cap_piece(fonts: &mut Fonts, cache: &mut Cache, tb: &TextBox, pi: usize, a: usize, b: usize, size: f32) -> Arc<Piece> {
    let p = &tb.paragraphs[pi];
    let mut q = p.clone();
    q.label = None;
    q.first_line_indent = 0.0;
    q.left_indent = 0.0;
    q.align = kn_model::Align::Left;
    q.line_spacing = kn_model::LineSpacing::Relative(1.0);
    q.runs = p
        .runs
        .iter()
        .filter(|r| r.end > a && r.start < b)
        .map(|r| {
            let mut s = r.style.clone();
            s.size = size;
            kn_model::Run { start: r.start.max(a), end: r.end.min(b), style: s }
        })
        .collect();
    q.start = a;
    q.end = b;
    let mut tb2 = TextBox { storage: tb.storage, text: tb.text.clone(), paragraphs: vec![q], valign: kn_model::VAlign::Top, padding: [0.0; 4], shrink_to_fit: false, columns: 1, column_gap: 0.0, autogrow: true, highlights: Vec::new(), fields: Vec::new() };
    // cache by content
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ("cap", &tb.text[a..b], size.to_bits(), a).hash(&mut h);
        format!("{:?}", tb2.paragraphs[0].runs).hash(&mut h);
        h.finish()
    };
    cache.used.insert(key);
    if let Some(pc) = cache.pieces.get(&key) {
        return pc.clone();
    }
    // the cap is laid out from the full text so offsets stay global
    tb2.text = tb.text[..b].to_string();
    let tl = text::layout(fonts, &tb2, 1.0e5, 1.0e5, true, true);
    let n = tl.lines.len();
    let line_glyphs = (0..n).map(|li| tl.glyphs.iter().enumerate().filter(|(_, g)| g.line == li).map(|(i, _)| i).collect()).collect();
    let bands = tl.lines.iter().map(|l| (l.top, l.top + l.height)).collect();
    let pc = Arc::new(Piece { tl, line_glyphs, bands });
    cache.pieces.insert(key, pc.clone());
    pc
}

/// Shape a whole small storage (header field, footnote) at `width`; returns the piece and its height.
pub fn storage_piece(k: &kn_format::KeyFile, storage: ObjId, ctx: BuildCtx, width: f32, align: Option<kn_model::Align>, fonts: &mut Fonts, cache: &mut Cache) -> Option<(Arc<Piece>, Arc<Body>)> {
    let mut b = body::build(k, storage, None, ctx)?;
    // a header / footer field's own position stands in for "natural" alignment only; an
    // alignment set on the paragraph wins (Pages)
    if let Some(a) = align {
        for p in &mut b.tb.paragraphs {
            if p.align == kn_model::Align::Natural {
                p.align = a;
            }
        }
    }
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ("storage", storage, width.to_bits(), &b.tb.text).hash(&mut h);
        format!("{:?}", b.tb.paragraphs).hash(&mut h);
        h.finish()
    };
    cache.used.insert(key);
    if let Some(pc) = cache.pieces.get(&key) {
        return Some((pc.clone(), Arc::new(b)));
    }
    let mut tb = b.tb.clone();
    tb.autogrow = true;
    let tl = text::layout(fonts, &tb, width, 1.0e6, false, true);
    let n = tl.lines.len();
    let mut line_glyphs = vec![Vec::new(); n];
    for (i, g) in tl.glyphs.iter().enumerate() {
        if g.line < n {
            line_glyphs[g.line].push(i);
        }
    }
    let bands = (0..n)
        .map(|i| {
            let top = if i == 0 { 0.0 } else { tl.lines[i].top };
            let bottom = if i + 1 < n { tl.lines[i + 1].top } else { tl.content_h };
            (top, bottom.max(top))
        })
        .collect();
    let pc = Arc::new(Piece { tl, line_glyphs, bands });
    cache.pieces.insert(key, pc.clone());
    Some((pc, Arc::new(b)))
}

fn footnote_height(k: &kn_format::KeyFile, storage: ObjId, number: usize, fonts: &mut Fonts, cache: &mut Cache, width: f32) -> f32 {
    storage_piece(k, storage, BuildCtx { first_footnote: number, ..Default::default() }, width, None, fonts, cache).map(|(p, _)| p.tl.content_h).unwrap_or(14.0)
}

/// Place the page's footnotes at the bottom of its text area.
fn finish_page(page: &mut Page, notes: &[(ObjId, usize)], k: &kn_format::KeyFile, fonts: &mut Fonts, cache: &mut Cache, setup: &PageSetup, total_h: f32) {
    if notes.is_empty() {
        return;
    }
    let mut y = setup.height - setup.bottom - total_h + 18.0;
    for &(storage, number) in notes {
        if let Some((pc, body)) = storage_piece(k, storage, BuildCtx { first_footnote: number, ..Default::default() }, setup.text_width(), None, fonts, cache) {
            let h = pc.tl.content_h;
            page.footnotes.push(Footnote { storage, number, piece: pc, body, dx: setup.left, dy: y });
            y += h + 4.0;
        }
    }
}

/// The section's layout objects (the page template's section_template_drawables) on this page,
/// behind everything else.
fn layout_objects(pages: &Pages, s: &Section, page: &mut Page, page_in_section: usize) {
    let Some(master) = s.master(page_in_section, page.number) else { return };
    let k = pages.file();
    let ids = k.obj(master).map(|m| m.ref_ids("section_template_drawables")).unwrap_or_default();
    let mut objs: Vec<PlacedObject> = Vec::new();
    for id in ids {
        let Some(d) = k.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")) else { continue };
        let g = kn_model::scene::geometry(d);
        objs.push(PlacedObject { id, x: g.x, y: g.y, w: g.w, h: g.h, inline: false, wrap: 2, margin: 0.0, layout: true, rows: None, row_parts: None });
    }
    page.objects.splice(0..0, objs);
}

#[allow(clippy::too_many_arguments)]
/// How far down the tallest header reaches and how far up the tallest footer starts, over every
/// section's masters (fields with text only; 0 / page height when there are none).
fn header_footer_extent(pages: &Pages, sections: &[Section], setup: &PageSetup, fonts: &mut Fonts, cache: &mut Cache) -> (f32, f32) {
    // (Word ends the body at the footer's top / starts it under the header, no gap)
    let gap: f32 = if fonts.word { 0.0 } else { 6.0 };
    let k = pages.file();
    let w = setup.text_width();
    let (mut top, mut bottom) = (0.0f32, setup.height);
    for s in sections {
        for master in [s.first, s.even, s.odd].into_iter().flatten() {
            for footer in [false, true] {
                if (footer && !setup.footers) || (!footer && !setup.headers) {
                    continue;
                }
                for sid in pages.header_footer_storages(master, footer).iter().take(3) {
                    if pages.doc.storage_text(*sid).trim().is_empty() {
                        continue;
                    }
                    let ctx = BuildCtx { page: 1, page_count: 1, ..Default::default() };
                    let Some((pc, _)) = storage_piece(k, *sid, ctx, w, None, fonts, cache) else { continue };
                    let h = pc.tl.content_h;
                    if footer {
                        bottom = bottom.min(setup.height - setup.footer_margin - h - gap);
                    } else {
                        top = top.max(setup.header_margin + h + gap);
                    }
                }
            }
        }
    }
    (top, bottom)
}

fn header_footers(pages: &Pages, s: &Section, page: &mut Page, page_in_section: usize, count: usize, setup: &PageSetup, fonts: &mut Fonts, cache: &mut Cache) {
    if page_in_section == 0 && s.first_page_hides_header_footer {
        return;
    }
    let Some(master) = s.master(page_in_section, page.number) else { return };
    let k = pages.file();
    let w = setup.text_width();
    for footer in [false, true] {
        if (footer && !setup.footers) || (!footer && !setup.headers) {
            continue;
        }
        let storages = pages.header_footer_storages(master, footer);
        for (i, sid) in storages.iter().enumerate().take(3) {
            let align = match i {
                0 => kn_model::Align::Left,
                1 => kn_model::Align::Center,
                _ => kn_model::Align::Right,
            };
            let ctx = BuildCtx { page: page.number, page_count: count, ..Default::default() };
            // every field spans the text width (Pages: a right-aligned centre field puts its
            // text at the right margin, as the user's booklet's page numbers in Pages' PDF)
            let fx = setup.left;
            let Some((pc, body)) = storage_piece(k, *sid, ctx, w, Some(align), fonts, cache) else { continue };
            // (a Word document's footer sits by its own height: no 14 pt minimum)
            let h = if fonts.word { pc.tl.content_h } else { pc.tl.content_h.max(14.0) };
            let (dy, fy) = if footer { (setup.height - setup.footer_margin - h, setup.height - setup.footer_margin - h) } else { (setup.header_margin, setup.header_margin) };
            let hf = HeaderFooter { storage: *sid, piece: pc, body, dx: fx, dy, frame: (setup.left + i as f32 * w / 3.0, fy, w / 3.0, h), footer, index: i };
            if footer {
                page.footers.push(hf);
            } else {
                page.headers.push(hf);
            }
        }
    }
}

/// The drawables on a page as the renderer wants them, with their frames moved where the
/// layout put them (inline objects).
pub fn page_drawables(pages: &Pages, page: &Page) -> Vec<Drawable> {
    let k = pages.file();
    let ctx = kn_model::text::TextContext { slide_number: page.number };
    page.objects
        .iter()
        .filter_map(|o| {
            let mut d = kn_model::scene::drawable(k, o.id, ctx)?;
            if let (Some(rows), kn_model::DrawableKind::Table(t)) = (page.fitted_rows.get(&o.id), &mut d.kind) {
                t.row_heights = rows.as_ref().clone();
                d.geom.h = rows.iter().sum();
            }
            // this page's part of a table continued over pages
            if let (Some(rows), kn_model::DrawableKind::Table(t)) = (&o.rows, &mut d.kind) {
                slice_rows(t, rows);
                // rows broken across pages: this page's text of each cell, at the part's height
                for part in o.row_parts.iter().flat_map(|p| p.iter()) {
                    let Some(r) = t.row_map.iter().position(|m| *m == part.row) else { continue };
                    if let Some(hh) = t.row_heights.get_mut(r) {
                        *hh = part.height;
                    }
                    for (c, (a, b)) in part.ranges.iter().enumerate() {
                        clip_cell(t, r * t.cols as usize + c, *a, *b);
                    }
                }
                d.geom.h = t.row_heights.iter().sum();
            }
            if o.inline {
                d.geom.x = o.x;
                d.geom.y = o.y;
            }
            Some(d)
        })
        .collect()
}

/// A cell's text lines as the table lays them out (frame-local, padding included).
struct CellLines {
    col: usize,
    /// (top, bottom, first byte) of each line.
    lines: Vec<(f32, f32, usize)>,
    pad: (f32, f32),
    len: usize,
    /// Space under the last line (its paragraph's space after).
    tail: f32,
}

impl CellLines {
    /// The height the cell needs for its text from byte `from` on.
    fn rest_height(&self, from: usize) -> f32 {
        let rest: Vec<&(f32, f32, usize)> = self.lines.iter().filter(|l| l.2 >= from).collect();
        match (rest.first(), rest.last()) {
            (Some(a), Some(b)) => b.1 - a.0 + self.tail + self.pad.0 + self.pad.1,
            _ => self.pad.0 + self.pad.1,
        }
    }
}

/// Each cell of model row `row` of table `id`, with its lines.
fn cell_lines(k: &kn_format::KeyFile, fonts: &mut Fonts, id: ObjId, row: usize) -> Vec<CellLines> {
    let Some(d) = kn_model::scene::drawable(k, id, kn_model::text::TextContext { slide_number: 1 }) else { return Vec::new() };
    let kn_model::DrawableKind::Table(t) = &d.kind else { return Vec::new() };
    let mut out = Vec::new();
    for col in 0..t.cols as usize {
        let cs = match t.merge_at(row as u32, col as u32) {
            Some(m) if m.row as usize != row || m.col as usize != col => continue,
            Some(m) => m.cols as usize,
            None => 1,
        };
        let i = row * t.cols as usize + col;
        let s = t.cells.get(i).map(String::as_str).unwrap_or("");
        let Some(style) = t.cell_styles.get(i) else { continue };
        let w: f32 = t.col_widths.iter().skip(col).take(cs).sum();
        let mut tb = kn_render::table::cell_box(t, i, s, style);
        tb.valign = kn_model::VAlign::Top;
        tb.padding = [style.padding[0], 0.0, style.padding[2], 0.0];
        let tl = text::layout(fonts, &tb, w, 1.0e5, !style.wrap, true);
        let tail = tl.lines.last().map(|l| (tl.content_h - l.top - l.height).max(0.0)).unwrap_or(0.0);
        out.push(CellLines { col, lines: tl.lines.iter().map(|l| (l.top, l.top + l.height, l.start)).collect(), pad: (style.padding[1], style.padding[3]), len: tb.text.len(), tail });
    }
    out
}

/// Break a row's cells to fit `room` points: each column's byte range shown, the part's height,
/// and where each cell goes on (None: nothing left).
fn break_row(cells: &[CellLines], from: &[usize], room: f32) -> (Vec<(usize, usize)>, f32, Option<Vec<usize>>) {
    let cols = cells.iter().map(|c| c.col + 1).max().unwrap_or(0);
    let mut ranges = vec![(0usize, 0usize); cols];
    let mut next = vec![0usize; cols];
    let mut used = 0.0f32;
    let mut left = false;
    let mut any = false;
    for c in cells {
        let start = from.get(c.col).copied().unwrap_or(0);
        let rest: Vec<&(f32, f32, usize)> = c.lines.iter().filter(|l| l.2 >= start).collect();
        let top = rest.first().map(|l| l.0).unwrap_or(0.0);
        let fit = room - c.pad.0 - c.pad.1;
        let mut n_fit = rest.iter().take_while(|l| l.1 - top <= fit + 0.01).count();
        // all of it: with the space after its last paragraph too
        if n_fit == rest.len() && n_fit > 0 && rest[n_fit - 1].1 - top + c.tail > fit + 0.01 {
            n_fit -= 1;
        }
        let end = rest.get(n_fit).map(|l| l.2).unwrap_or(c.len);
        ranges[c.col] = (start.min(c.len), end);
        next[c.col] = end;
        if n_fit < rest.len() {
            left = true;
        }
        any |= n_fit > 0;
        let h = if n_fit > 0 { rest[n_fit - 1].1 - top } else { 0.0 };
        used = used.max(h + c.pad.0 + c.pad.1);
    }
    // (not one line fits: no part here)
    if !any {
        used = 0.0;
    }
    (ranges, used, left.then_some(next))
}

/// Show only bytes `a..b` of cell `i`'s text (a row broken across pages).
fn clip_cell(t: &mut kn_model::TableData, i: usize, a: usize, b: usize) {
    let fix = |s: &str, v: usize| {
        let mut v = v.min(s.len());
        while !s.is_char_boundary(v) {
            v -= 1;
        }
        v
    };
    if let Some(s) = t.cells.get_mut(i) {
        let (x, y) = (fix(s, a), fix(s, b));
        *s = s[x..y.max(x)].trim_start_matches('\n').to_string();
    }
    if let Some(Some(tb)) = t.rich.get_mut(i) {
        let (x, y) = (fix(&tb.text, a), fix(&tb.text, b));
        let y = y.max(x);
        let mut paras: Vec<kn_model::Paragraph> = tb
            .paragraphs
            .iter()
            .filter(|p| p.end >= x && p.start < y.max(x + 1))
            .cloned()
            .map(|mut p| {
                // (a paragraph going on from the page before: no space before it again)
                if p.start < x {
                    p.space_before = 0.0;
                }
                p.start = p.start.max(x) - x;
                p.end = p.end.min(y).max(p.start + x) - x;
                p.runs = p.runs.into_iter().filter(|r| r.end > x && r.start < y || r.start == r.end).map(|mut r| {
                    r.start = r.start.clamp(x, y) - x;
                    r.end = r.end.clamp(x, y) - x;
                    r
                }).collect();
                p
            })
            .collect();
        if paras.is_empty() {
            if let Some(mut p) = tb.paragraphs.last().cloned() {
                p.start = 0;
                p.end = 0;
                p.runs.truncate(1);
                for r in &mut p.runs {
                    r.start = 0;
                    r.end = 0;
                }
                paras.push(p);
            }
        }
        tb.text = tb.text[x..y].to_string();
        tb.paragraphs = paras;
    }
}

/// Keep only `rows` of a table (model rows, in order): what a page shows of a table continued
/// over pages. Merges that cross the cut are left out; `row_map` says which row is which.
pub fn slice_rows(t: &mut kn_model::TableData, rows: &[u32]) {
    let cols = t.cols as usize;
    let n = t.rows as usize;
    let rows: Vec<usize> = rows.iter().map(|r| *r as usize).filter(|r| *r < n).collect();
    fn per_row<T: Clone>(v: &[T], rows: &[usize], w: usize) -> Vec<T> {
        if v.len() < rows.iter().max().map(|r| (r + 1) * w).unwrap_or(0) {
            return v.to_vec();
        }
        rows.iter().flat_map(|r| v[r * w..(r + 1) * w].iter().cloned()).collect()
    }
    t.cells = per_row(&t.cells, &rows, cols);
    t.cell_styles = per_row(&t.cell_styles, &rows, cols);
    if !t.rich.is_empty() {
        t.rich = per_row(&t.rich, &rows, cols);
    }
    t.v_strokes = per_row(&t.v_strokes, &rows, cols + 1);
    // horizontal lines: each shown row's top, and the bottom of the last
    if t.h_strokes.len() >= (n + 1) * cols {
        let mut h = Vec::with_capacity((rows.len() + 1) * cols);
        for r in &rows {
            h.extend(t.h_strokes[r * cols..(r + 1) * cols].iter().cloned());
        }
        if let Some(last) = rows.last() {
            h.extend(t.h_strokes[(last + 1) * cols..(last + 2) * cols].iter().cloned());
        }
        t.h_strokes = h;
    }
    t.row_heights = rows.iter().map(|r| t.row_heights.get(*r).copied().unwrap_or(20.0)).collect();
    if !t.auto_rows.is_empty() {
        t.auto_rows = rows.iter().map(|r| t.auto_rows.get(*r).copied().unwrap_or(false)).collect();
    }
    t.merges = t
        .merges
        .iter()
        .filter_map(|m| {
            let at = rows.iter().position(|r| *r == m.row as usize)?;
            let all = (0..m.rows as usize).all(|k| rows.get(at + k) == Some(&(m.row as usize + k)));
            all.then(|| kn_model::CellRange { row: at as u32, ..*m })
        })
        .collect();
    let header = t.header_rows as usize;
    t.header_rows = rows.iter().take_while(|r| **r < header).count() as u32;
    t.footer_rows = if rows.last() == Some(&(n - 1)) { t.footer_rows.min(rows.len() as u32) } else { 0 };
    t.rows = rows.len() as u32;
    t.row_map = rows.iter().map(|r| *r as u32).collect();
}

/// A table's row heights grown to fit each cell's text (None when the stored ones are enough).
/// Cells spanning several rows are left to their rows.
pub fn fit_table(k: &kn_format::KeyFile, fonts: &mut Fonts, cache: &mut Cache, id: ObjId) -> Option<Arc<Vec<f32>>> {
    let d = kn_model::scene::drawable(k, id, kn_model::text::TextContext { slide_number: 1 })?;
    let kn_model::DrawableKind::Table(t) = &d.kind else { return None };
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&t.cells, &t.row_heights.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), &t.col_widths.iter().map(|x| x.to_bits()).collect::<Vec<_>>()).hash(&mut h);
        format!("{:?}", t.cell_styles).hash(&mut h);
        h.finish()
    };
    if let Some(r) = cache.tables.get(&(id, key)) {
        return r.clone();
    }
    let mut rows = t.row_heights.clone();
    rows.resize(t.rows as usize, 40.0);
    let mut grew = false;
    for row in 0..t.rows {
        for col in 0..t.cols {
            let (rs, cs) = match t.merge_at(row, col) {
                Some(m) if m.row != row || m.col != col => continue,
                Some(m) => (m.rows, m.cols),
                None => (1, 1),
            };
            if rs > 1 {
                continue;
            }
            let i = (row * t.cols + col) as usize;
            let s = t.cells.get(i).map(String::as_str).unwrap_or("");
            if s.trim().is_empty() {
                continue;
            }
            let Some(style) = t.cell_styles.get(i) else { continue };
            let w: f32 = t.col_widths.iter().skip(col as usize).take(cs as usize).sum();
            // the same text box the table renderer draws (cell paragraph spacing and indents)
            let mut tb = kn_render::table::cell_box(t, i, s, style);
            tb.valign = kn_model::VAlign::Top;
            tb.padding = style.padding;
            let tl = text::layout(fonts, &tb, w, 1.0e5, !style.wrap, true);
            let need = tl.content_h + style.padding[1] + style.padding[3];
            if need > rows[row as usize] + 0.5 {
                rows[row as usize] = need;
                grew = true;
            }
        }
    }
    let out = grew.then(|| Arc::new(rows));
    cache.tables.insert((id, key), out.clone());
    out
}

/// Caret (x, top, height) for shown offset `s` in a piece (piece coordinates).
pub fn piece_caret(pc: &Piece, s: usize) -> (f32, f32, f32) {
    let tl = &pc.tl;
    if tl.lines.is_empty() {
        return (0.0, 0.0, 14.0);
    }
    let li = tl.lines.iter().rposition(|l| l.start <= s).unwrap_or(0);
    let line = &tl.lines[li];
    let mut x = line.x;
    let mut found = false;
    for &gi in &pc.line_glyphs[li] {
        let g = &tl.glyphs[gi];
        if g.label {
            continue;
        }
        if g.start == s {
            x = g.x;
            found = true;
            break;
        }
        if g.end <= s {
            x = g.x + g.w;
            found = true;
        }
    }
    if !found {
        x = pc.line_glyphs[li].iter().map(|&i| &tl.glyphs[i]).find(|g| !g.label).map(|g| g.x).unwrap_or(line.x);
    }
    let (a, b) = pc.bands[li];
    (x, a, (b - a).max(4.0))
}

/// Shown offset nearest to piece point (x, y).
pub fn piece_hit(pc: &Piece, x: f32, y: f32) -> usize {
    let tl = &pc.tl;
    let Some(li) = (0..tl.lines.len()).min_by(|&a, &b| {
        let d = |i: usize| {
            let (t, bt) = pc.bands[i];
            if y < t { t - y } else if y > bt { y - bt } else { 0.0 }
        };
        d(a).total_cmp(&d(b))
    }) else {
        return 0;
    };
    let line = &tl.lines[li];
    let mut best = line.start;
    let mut best_d = f32::INFINITY;
    for &gi in &pc.line_glyphs[li] {
        let g = &tl.glyphs[gi];
        if g.label {
            continue;
        }
        for (pos, gx) in [(g.start, g.x), (g.end, g.x + g.w)] {
            let d = (gx - x).abs();
            if d < best_d {
                best_d = d;
                best = pos;
            }
        }
    }
    best.min(line.end.max(line.start))
}
