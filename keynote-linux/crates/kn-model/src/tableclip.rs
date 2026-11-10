//! Table cells on the clipboard between the suite's apps (Keydeck, Letters, Values, Notes).
//!
//! A copy is a [`TableClip`]: plain data for a block of cells (each cell's shown text, formula,
//! raw value, data format, look and borders), its merges and, for a whole table, its column
//! widths, row heights, name and header counts. It goes on the system clipboard as HTML
//! ([`TableClip::to_html`]): a normal `<table>` with inline CSS that other programs (LibreOffice,
//! browsers, Notes) read, with the exact Bramble data riding along in a `data-bramble-table`
//! attribute. [`TableClip::from_html`] takes that data back when it is there, and otherwise
//! reads a generic HTML table (LibreOffice Calc, a web page) as values with their basic look.
//!
//! Writing the cells ([`write_cell`]) is Values' paste, as Numbers does it: the copied cell's
//! own look replaces the destination's, the parts it shares with its table's usual look take
//! the destination table's. [`write`] is the whole paste for tables without categories
//! (Keydeck, Letters); Values keeps its own plan (categories, shown rows) and uses the same
//! [`write_cell`].

use std::collections::HashMap;

use kn_format::ObjId;
use serde_json::{json, Value};

use crate::edit::History;
use crate::format::{Dash, FillSpec, StrokeSpec, TextProp};
use crate::table::CellValue;
use crate::tablecells::{CellAlign, CellInput};
use crate::tablefmt::{BorderEdges, CellFormat};
use crate::tablerows::{Axis, HeaderKind};
use crate::{Align, Caps, CellRange, CharStyle, Color, Document, Fill, GradientKind, LineCap, LineJoin, LineSpacing, Stroke, TableCellStyle, TableData, VAlign};

// ------------------------------------------------------------------------------- one cell

#[derive(Clone, Debug, PartialEq)]
pub struct CellCopy {
    pub formula: Option<String>,
    pub shown: String,
    pub format: CellFormat,
    pub style: Option<TableCellStyle>,
    /// A cell a merge covers (not its first): Numbers pastes the merge as its first cell
    /// only and leaves the destination of the covered ones untouched (Mac N18 P11).
    pub covered: bool,
    /// The cell's own borders as they show (top, bottom, left, right): a paste brings them
    /// along. A covered cell keeps its part of the merge's outline here, so a merge made again
    /// gets its whole outline back.
    pub edges: [Option<Stroke>; 4],
    /// A merge's first cell: the merge's outline (top and left from this cell, bottom and
    /// right from its last cells), for when it pastes as one ordinary cell.
    pub outline: Option<[Option<Stroke>; 4]>,
    /// Header row, footer, header column or body ([`region`]).
    pub region: u8,
    /// The cell's stored value (a number stays a number whatever its format shows, text that
    /// looks like a number stays text). None: what `shown` parses as.
    pub value: Option<CellValue>,
}

impl Default for CellCopy {
    fn default() -> Self {
        CellCopy { formula: None, shown: String::new(), format: CellFormat::Automatic, style: None, covered: false, edges: Default::default(), outline: None, region: 3, value: None }
    }
}

/// A cell's part of its table: 0 header rows, 1 footer rows, 2 header columns, 3 body.
pub fn region(d: &TableData, r: u32, c: u32) -> u8 {
    if r < d.header_rows {
        0
    } else if r >= d.rows.saturating_sub(d.footer_rows) {
        1
    } else if c < d.header_cols {
        2
    } else {
        3
    }
}

/// The usual look of each region (header rows, footer, header columns, body) in table `d`:
/// what a cell has the same as this is not its own, so it doesn't travel with a copy (Numbers:
/// a header cell pasted in the body is body text, Mac N18 P10).
pub fn region_looks(d: &TableData) -> [Option<TableCellStyle>; 4] {
    let mut seen: [Vec<(TableCellStyle, usize)>; 4] = Default::default();
    // (a sample is enough on a large table)
    let step = ((d.rows * d.cols) / 4000).max(1) as usize;
    for (i, st) in d.cell_styles.iter().enumerate().step_by(step) {
        let (r, c) = (i as u32 / d.cols.max(1), i as u32 % d.cols.max(1));
        let v = &mut seen[region(d, r, c) as usize];
        match v.iter_mut().find(|x| x.0 == *st) {
            Some(x) => x.1 += 1,
            None => v.push((st.clone(), 1)),
        }
    }
    seen.map(|v| v.into_iter().max_by_key(|x| x.1).map(|x| x.0))
}

/// The table's usual inner grid lines (horizontal, vertical): borders like these are the table
/// style's, not a cell's own.
pub fn grid_lines(d: &TableData) -> (Option<Stroke>, Option<Stroke>) {
    let mode = |v: &mut dyn Iterator<Item = &Option<Stroke>>| {
        let mut seen: Vec<(Option<Stroke>, usize)> = Vec::new();
        for s in v {
            match seen.iter_mut().find(|x| x.0 == *s) {
                Some(x) => x.1 += 1,
                None => seen.push((s.clone(), 1)),
            }
        }
        seen.into_iter().max_by_key(|x| x.1).and_then(|x| x.0)
    };
    (mode(&mut d.h_strokes.iter()), mode(&mut d.v_strokes.iter()))
}

/// A cell's four borders (top, bottom, left, right) as table `d` shows them (shown rows).
pub fn cell_edges(d: &TableData, r: u32, c: u32) -> [Option<Stroke>; 4] {
    let h = |b: u32| d.h_strokes.get((b * d.cols + c) as usize).cloned().flatten();
    let v = |b: u32| d.v_strokes.get((r * (d.cols + 1) + b) as usize).cloned().flatten();
    [h(r), h(r + 1), v(c), v(c + 1)]
}

/// A block of cells grown to take in every merged cell it touches (Numbers selects merges
/// whole). A whole row or column stays as it is (Numbers, Mac N16).
pub fn with_merges(td: &TableData, b: (u32, u32, u32, u32)) -> (u32, u32, u32, u32) {
    if (b.0 == 0 && b.2 + 1 >= td.rows) || (b.1 == 0 && b.3 + 1 >= td.cols) {
        return b;
    }
    let (mut r0, mut c0, mut r1, mut c1) = b;
    loop {
        let before = (r0, c0, r1, c1);
        for m in &td.merges {
            let (mr1, mc1) = (m.row + m.rows - 1, m.col + m.cols - 1);
            if m.row <= r1 && mr1 >= r0 && m.col <= c1 && mc1 >= c0 {
                r0 = r0.min(m.row);
                c0 = c0.min(m.col);
                r1 = r1.max(mr1);
                c1 = c1.max(mc1);
            }
        }
        if (r0, c0, r1, c1) == before {
            return before;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PasteMode {
    /// Values, formulas, formats and styles.
    All,
    /// Values and formulas; the destination keeps its formats and styles.
    MatchStyle,
    /// The shown values only.
    Results,
}

/// Shift the relative cell references of a formula by (dr, dc); `$`-anchored parts stay.
pub fn shift_formula(text: &str, dr: i64, dc: i64) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_str = false;
    let col_name = |mut c: i64| {
        let mut s = String::new();
        c += 1;
        while c > 0 {
            let m = ((c - 1) % 26) as u8;
            s.insert(0, (b'A' + m) as char);
            c = (c - 1) / 26;
        }
        s
    };
    while i < b.len() {
        let ch = b[i];
        if ch == '"' {
            in_str = !in_str;
            out.push(ch);
            i += 1;
            continue;
        }
        let starts_ref = !in_str && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == '_' || b[i - 1] == '.'));
        if starts_ref {
            // [$]LETTERS[$]DIGITS, not followed by a letter, digit or "("
            let mut j = i;
            let col_abs = b.get(j) == Some(&'$');
            if col_abs {
                j += 1;
            }
            let l0 = j;
            while j < b.len() && b[j].is_ascii_uppercase() {
                j += 1;
            }
            let letters: String = b[l0..j].iter().collect();
            let row_abs = b.get(j) == Some(&'$');
            let mut k = if row_abs { j + 1 } else { j };
            let d0 = k;
            while k < b.len() && b[k].is_ascii_digit() {
                k += 1;
            }
            let digits: String = b[d0..k].iter().collect();
            let ends_ok = b.get(k).is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == '(' || *c == '_'));
            if !letters.is_empty() && letters.len() <= 3 && !digits.is_empty() && ends_ok {
                let col = letters.bytes().fold(0i64, |a, ch| a * 26 + i64::from(ch - b'A') + 1) - 1;
                let row: i64 = digits.parse().unwrap_or(1) - 1;
                let (nc, nr) = (if col_abs { col } else { col + dc }, if row_abs { row } else { row + dr });
                if nc < 0 || nr < 0 {
                    out.push_str("#REF!");
                } else {
                    out.push_str(&format!("{}{}{}{}", if col_abs { "$" } else { "" }, col_name(nc), if row_abs { "$" } else { "" }, nr + 1));
                }
                i = k;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

pub fn cell_align(a: Align) -> CellAlign {
    match a {
        Align::Center => CellAlign::Center,
        Align::Right => CellAlign::Right,
        Align::Justify => CellAlign::Justify,
        _ => CellAlign::Left,
    }
}

/// A cell fill as the fill to set.
pub fn fill_spec(f: &Fill) -> FillSpec {
    match f {
        Fill::None => FillSpec::None,
        Fill::Color(c) => FillSpec::Color(*c),
        Fill::Gradient { stops, kind, inflections, advanced, .. } => {
            let angle = match kind {
                GradientKind::Linear { angle } => *angle,
                _ => 270.0,
            };
            if *advanced || stops.len() > 2 || matches!(kind, GradientKind::Radial) {
                let stops = stops.iter().enumerate().map(|(i, s)| (s.0, s.1, inflections.get(i).copied().unwrap_or(0.5))).collect();
                return FillSpec::Advanced { stops, radial: matches!(kind, GradientKind::Radial), angle };
            }
            let from = stops.first().map(|s| s.1).unwrap_or(Color::WHITE);
            let to = stops.last().map(|s| s.1).unwrap_or(Color::BLACK);
            FillSpec::Gradient { from, to, angle }
        }
        Fill::Image { data_id, technique, reference, size, tint } => FillSpec::Image { data_id: *data_id, technique: *technique, size: *size, reference: *reference, tint: *tint },
    }
}

/// One pasted cell: where it goes (model row), what was copied, and the destination's look
/// before the paste (shown row) so only what differs is written.
pub struct Job {
    pub r: u32,
    pub c: u32,
    pub cc: CellCopy,
    /// The top-left of the copy's tile this cell is in (formulas shift by its distance).
    pub top_r: i64,
    pub left_c: i64,
    pub dest: Option<TableCellStyle>,
    pub dest_edges: [Option<Stroke>; 4],
    /// The usual look of the copied cell's region in its table, and of the destination's
    /// region in this one.
    pub src_look: Option<TableCellStyle>,
    pub dest_look: Option<TableCellStyle>,
    pub src_grid: (Option<Stroke>, Option<Stroke>),
    pub dest_grid: (Option<Stroke>, Option<Stroke>),
}

/// What a copied cell's value is written as.
fn cell_input(cc: &CellCopy) -> CellInput {
    match &cc.value {
        Some(CellValue::Number(v)) => CellInput::Number(*v),
        Some(CellValue::Date(s)) => CellInput::Date(*s),
        Some(CellValue::Text(s)) if !s.is_empty() => CellInput::Text(s.clone()),
        _ => CellInput::parse(&cc.shown),
    }
}

/// Write one copied cell over another, as Numbers' Paste does: the value or formula, the data
/// format, and (plain Paste) the copied cell's whole look in place of the destination's: fill,
/// text style, alignment, wrap, inset and borders, for empty cells too.
pub fn write_cell(d: &mut Document, hist: &mut History, t: ObjId, job: &Job, mode: PasteMode, origin: (u32, u32), portable: &dyn Fn(&CellFormat) -> CellFormat) -> Result<(), kn_format::Error> {
    let (r, c, cc) = (job.r, job.c, &job.cc);
    // a cell a re-made merge covers: emptied (the merge takes its first cell's content), its
    // part of the merge's outline drawn
    if cc.covered {
        d.set_cell(hist, t, r, c, &CellInput::Empty)?;
        if mode == PasteMode::All {
            write_edges(d, hist, t, job);
        }
        return Ok(());
    }
    // values and formulas (Paste Formula Results: what the formulas show)
    match (&cc.formula, mode) {
        (Some(f), PasteMode::All | PasteMode::MatchStyle) => {
            let (dr, dc) = (job.top_r - i64::from(origin.0), job.left_c - i64::from(origin.1));
            d.set_formula_text(hist, t, r, c, &shift_formula(f, dr, dc))?;
        }
        _ if cc.shown.is_empty() => d.set_cell(hist, t, r, c, &CellInput::Empty)?,
        _ => d.set_cell(hist, t, r, c, &cell_input(cc))?,
    }
    let b = (r, c, r, c);
    // the number format comes along in every kind of paste (when the copy has one); a plain
    // Paste also takes Automatic, so the cell ends up as the one copied
    let f = portable(&cc.format);
    if f != CellFormat::Automatic || mode == PasteMode::All {
        d.set_cells_format(hist, t, b, &f)?;
    }
    // the look: plain Paste only
    if mode != PasteMode::All {
        return Ok(());
    }
    let Some(src) = &cc.style else { return Ok(()) };
    let dest = job.dest.as_ref();
    // what the cell should look like: the copied cell's own changes, and where it has none of
    // its own (it looks as its region does), the destination region's look
    let own = |f: &dyn Fn(&TableCellStyle) -> bool| job.src_look.as_ref().is_none_or(|l| !f(l));
    let base = job.dest_look.as_ref().or(dest).unwrap_or(src);
    let pick = |is_own: bool| if is_own { src } else { base };
    let mut st = src.clone();
    st.fill = pick(own(&|l| l.fill == src.fill)).fill.clone();
    st.valign = pick(own(&|l| l.valign == src.valign)).valign;
    st.wrap = pick(own(&|l| l.wrap == src.wrap)).wrap;
    st.padding = pick(own(&|l| l.padding == src.padding)).padding;
    st.align = pick(own(&|l| l.align == src.align)).align;
    st.text.bold = pick(own(&|l| l.text.bold == src.text.bold)).text.bold;
    st.text.italic = pick(own(&|l| l.text.italic == src.text.italic)).text.italic;
    st.text.underline = pick(own(&|l| l.text.underline == src.text.underline)).text.underline;
    st.text.strike = pick(own(&|l| l.text.strike == src.text.strike)).text.strike;
    st.text.size = pick(own(&|l| l.text.size == src.text.size)).text.size;
    st.text.color = pick(own(&|l| l.text.color == src.text.color)).text.color;
    st.text.font_name = pick(own(&|l| l.text.font_name == src.text.font_name)).text.font_name.clone();
    let differs = |f: &dyn Fn(&TableCellStyle) -> bool| dest.is_none_or(|x| !f(x));
    // cell properties first: they give an empty cell a record its text style can hang on
    if differs(&|x| x.fill == st.fill) {
        // the region's own fill: the table style's again
        if job.dest_look.as_ref().is_some_and(|l| l.fill == st.fill) || matches!(st.fill, Fill::None) {
            d.set_cells_fill(hist, t, b, None)?;
        } else {
            d.set_cells_fill(hist, t, b, Some(&fill_spec(&st.fill)))?;
        }
    }
    if differs(&|x| x.valign == st.valign) {
        d.set_cells_valign(hist, t, b, st.valign)?;
    }
    if differs(&|x| x.wrap == st.wrap) {
        d.set_cells_wrap(hist, t, b, st.wrap)?;
    }
    if differs(&|x| x.padding == st.padding) {
        d.set_cells_padding(hist, t, b, st.padding)?;
    }
    // the text style (Keynote's model keeps one only on a cell with a record: an empty cell
    // with nothing else of its own can't hold it, and needs none to look the same)
    let tx = &st.text;
    let props = [
        (differs(&|x| x.text.bold == tx.bold), TextProp::Bold(tx.bold)),
        (differs(&|x| x.text.italic == tx.italic), TextProp::Italic(tx.italic)),
        (differs(&|x| x.text.underline == tx.underline), TextProp::Underline(tx.underline)),
        (differs(&|x| x.text.strike == tx.strike), TextProp::Strike(tx.strike)),
        (differs(&|x| x.text.size == tx.size), TextProp::Size(tx.size)),
        (differs(&|x| x.text.color == tx.color), TextProp::Color(tx.color)),
        (differs(&|x| x.text.font_name == tx.font_name), TextProp::Font(tx.font_name.clone())),
    ];
    let empty = cc.shown.is_empty() && cc.formula.is_none();
    for (_, p) in props.iter().filter(|(on, _)| *on) {
        match d.set_cells_text_prop(hist, t, b, p) {
            Err(_) if empty => break,
            r => {
                r?;
            }
        }
    }
    if differs(&|x| x.align == st.align) {
        match d.set_cells_alignment(hist, t, b, cell_align(st.align)) {
            Err(_) if empty => {}
            r => {
                r?;
            }
        }
    }
    write_edges(d, hist, t, job);
    Ok(())
}

/// A pasted cell's borders: the copied cell's own (lines like its table's usual grid are the
/// destination's grid), where the destination shows something else.
pub fn write_edges(d: &mut Document, hist: &mut History, t: ObjId, job: &Job) {
    let (r, c, cc) = (job.r, job.c, &job.cc);
    let b = (r, c, r, c);
    let spec = |s: &Stroke| StrokeSpec { color: s.color, width: s.width, dash: if s.dash.is_empty() { Dash::Solid } else if s.dash[0] < 0.5 { Dash::Dotted } else { Dash::Dashed } };
    let same = |a: &Option<Stroke>, h: &Option<Stroke>| match (a, h) {
        (Some(a), Some(h)) => a.color == h.color && (a.width - h.width).abs() < 0.01 && a.dash.is_empty() == h.dash.is_empty(),
        (None, None) => true,
        _ => false,
    };
    for (k, edge) in [BorderEdges::Top, BorderEdges::Bottom, BorderEdges::Left, BorderEdges::Right].into_iter().enumerate() {
        let (sg, dg) = if k < 2 { (&job.src_grid.0, &job.dest_grid.0) } else { (&job.src_grid.1, &job.dest_grid.1) };
        let copied = &cc.edges[k];
        let want = if same(copied, sg) { dg } else { copied };
        if !same(want, &job.dest_edges[k]) {
            // (a table with no stroke sidecar can't take borders: the rest still pastes)
            let _ = d.set_cells_border(hist, t, b, edge, want.as_ref().map(spec).as_ref());
        }
    }
}

// ------------------------------------------------------------------------------- the copy

/// Copied cells: a grid of rows (row offset, column offset from the top-left).
#[derive(Clone, Debug, PartialEq)]
pub struct TableClip {
    pub rows: u32,
    pub cols: u32,
    /// Row-major, `rows * cols`.
    pub cells: Vec<CellCopy>,
    /// Merged cells inside the copy: (row, column, rows, columns) from its top-left.
    pub merges: Vec<(u32, u32, u32, u32)>,
    /// Where the copy's top-left was (row, column): formulas shift by the distance.
    pub origin: (u32, u32),
    /// The source table's region looks and grid lines (see [`region_looks`], [`grid_lines`]).
    pub looks: [Option<TableCellStyle>; 4],
    pub grid: (Option<Stroke>, Option<Stroke>),
    /// Column widths and row heights (points; 0 unknown); empty when not known.
    pub col_widths: Vec<f32>,
    pub row_heights: Vec<f32>,
    pub name: String,
    /// The whole table was copied (as an object): it pastes as the same table.
    pub whole: bool,
    /// Header rows, header columns, footer rows (a whole table).
    pub headers: (u32, u32, u32),
    /// The process that put it on the clipboard ([`process_tag`]); empty for HTML from
    /// elsewhere.
    pub source: String,
    /// The document it came from ([`doc_key`]): custom formats only mean something there.
    pub doc: String,
}

impl TableClip {
    /// An empty clip of plain cells (text from Notes, a generic HTML table): every region
    /// looks as the default cell style, so what a cell has the same as that is not its own and
    /// the destination's look stays.
    pub fn plain(rows: u32, cols: u32) -> TableClip {
        let look = Some(TableCellStyle::default());
        let cell = CellCopy { style: look.clone(), ..Default::default() };
        TableClip {
            rows,
            cols,
            cells: vec![cell; (rows * cols) as usize],
            merges: Vec::new(),
            origin: (0, 0),
            looks: [look.clone(), look.clone(), look.clone(), look],
            grid: (None, None),
            col_widths: Vec::new(),
            row_heights: Vec::new(),
            name: String::new(),
            whole: false,
            headers: (0, 0, 0),
            source: String::new(),
            doc: String::new(),
        }
    }

    pub fn cell(&self, r: u32, c: u32) -> Option<&CellCopy> {
        self.cells.get((r * self.cols + c) as usize)
    }

    /// Whether this process put it on the clipboard.
    pub fn ours(&self) -> bool {
        self.source == process_tag()
    }

    /// The cells as tab-separated text (cells of several lines quoted, as spreadsheets
    /// exchange them; one empty cell is a line break, which the clipboard keeps).
    pub fn tsv(&self) -> String {
        let mut lines = Vec::new();
        for r in 0..self.rows {
            let line: Vec<String> = (0..self.cols)
                .map(|c| {
                    let shown = self.cell(r, c).map(|x| x.shown.as_str()).unwrap_or("");
                    if shown.contains(['\n', '\t', '"']) { format!("\"{}\"", shown.replace('"', "\"\"")) } else { shown.to_string() }
                })
                .collect();
            lines.push(line.join("\t"));
        }
        match lines.join("\n") {
            t if t.is_empty() => "\n".to_string(),
            t => t,
        }
    }
}

/// This process on the clipboard (to tell a paste of what it copied itself).
pub fn process_tag() -> &'static str {
    static TAG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TAG.get_or_init(|| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        format!("{}-{t:x}", std::process::id())
    })
}

/// A document as a short key (not its path: the key goes to other apps on the clipboard).
pub fn doc_key(doc: &Document) -> String {
    identity_key(&crate::insert::doc_identity(doc))
}

/// [`doc_key`] of a [`crate::insert::doc_identity`].
pub fn identity_key(identity: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn table_data(doc: &Document, t: ObjId) -> Option<TableData> {
    doc.file.obj(t).and_then(|m| crate::table::table(&doc.file, m))
}

/// The cells of `block` (row0, col0, row1, col1, grown to whole merges) of table `t`, everything
/// a paste needs; `whole` also keeps the table's sizes, name and headers.
pub fn read(doc: &Document, t: ObjId, block: (u32, u32, u32, u32), whole: bool) -> Option<TableClip> {
    let td = table_data(doc, t)?;
    if td.rows == 0 || td.cols == 0 {
        return None;
    }
    let b = (block.0.min(td.rows - 1), block.1.min(td.cols - 1), block.2.min(td.rows - 1), block.3.min(td.cols - 1));
    let (r0, c0, r1, c1) = if whole { (0, 0, td.rows - 1, td.cols - 1) } else { with_merges(&td, b) };
    // (each of these reads the whole table: once, not per cell)
    let values = doc.cell_values(t);
    let formats = doc.cell_formats(t);
    let formulas: std::collections::HashSet<(u32, u32)> = doc.formula_cells(t).into_iter().map(|x| x.0).collect();
    let mut cells = Vec::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            let i = (r * td.cols + c) as usize;
            let merge = td.merge_at(r, c);
            let covered = merge.is_some_and(|m| (m.row, m.col) != (r, c));
            let outline = match merge {
                Some(m) if !covered => {
                    let (a, b) = (cell_edges(&td, r, c), cell_edges(&td, m.row + m.rows - 1, m.col + m.cols - 1));
                    Some([a[0].clone(), b[1].clone(), a[2].clone(), b[3].clone()])
                }
                _ => None,
            };
            cells.push(CellCopy {
                formula: if formulas.contains(&(r, c)) { doc.formula_text(t, r, c) } else { None },
                shown: td.cells.get(i).cloned().unwrap_or_default(),
                format: formats.get(&(r, c)).cloned().unwrap_or(CellFormat::Automatic),
                style: td.cell_styles.get(i).cloned(),
                covered,
                edges: cell_edges(&td, r, c),
                outline,
                region: region(&td, r, c),
                value: values.get(&(r, c)).cloned(),
            });
        }
    }
    let merges = td.merges.iter().filter(|m| m.row >= r0 && m.row + m.rows - 1 <= r1 && m.col >= c0 && m.col + m.cols - 1 <= c1).map(|m| (m.row - r0, m.col - c0, m.rows, m.cols)).collect();
    let whole = whole || (r0, c0, r1, c1) == (0, 0, td.rows - 1, td.cols - 1);
    Some(TableClip {
        rows: r1 - r0 + 1,
        cols: c1 - c0 + 1,
        cells,
        merges,
        origin: (r0, c0),
        looks: region_looks(&td),
        grid: grid_lines(&td),
        col_widths: if whole { td.col_widths.clone() } else { Vec::new() },
        row_heights: if whole { td.row_heights.clone() } else { Vec::new() },
        name: if whole { td.name.clone() } else { String::new() },
        whole,
        headers: if whole { (td.header_rows, td.header_cols, td.footer_rows) } else { (0, 0, 0) },
        source: process_tag().to_string(),
        doc: doc_key(doc),
    })
}

/// Paste `clip` into the cells `sel` of table `t` (no categories: rows are the table's own),
/// as Values does (Numbers 15.4, Mac N18): from one selected cell the whole copy goes in (the
/// table grows to take it); a selected range is filled by repeating the copy, cut off at its
/// edges; copied merges are made again, merges in the way undone. The caller opens and closes
/// the undo step. Returns the pasted block.
pub fn write(doc: &mut Document, h: &mut History, t: ObjId, sel: (u32, u32, u32, u32), clip: &TableClip, mode: PasteMode) -> Result<(u32, u32, u32, u32), kn_format::Error> {
    let err = |s: &str| kn_format::Error::Invalid(s.into());
    if clip.rows == 0 || clip.cols == 0 {
        return Err(err("nothing to paste"));
    }
    let td = table_data(doc, t).ok_or(err("not a table"))?;
    if td.rows == 0 || td.cols == 0 {
        return Err(err("the table has no cells"));
    }
    let sel = (sel.0.min(td.rows - 1), sel.1.min(td.cols - 1), sel.2.min(td.rows - 1), sel.3.min(td.cols - 1));
    let (r0, c0, r1, c1) = with_merges(&td, sel);
    let (hh, w) = (r1 - r0 + 1, c1 - c0 + 1);
    let one_merge = with_merges(&td, (r0, c0, r0, c0)) == (r0, c0, r1, c1);
    let single = (hh == 1 && w == 1) || one_merge;
    let (th, tw) = if single { (clip.rows, clip.cols) } else { (hh, w) };
    let into_merge = one_merge && clip.rows == 1 && clip.cols == 1 && (hh > 1 || w > 1);
    let lone = clip.merges.len() == 1 && clip.merges[0] == (0, 0, clip.rows, clip.cols);
    let remerge = mode == PasteMode::All && !clip.merges.is_empty() && (!lone || (hh >= clip.rows && w >= clip.cols));
    // from one cell the table grows to take the whole copy
    let (more_r, more_c) = if single { ((r0 + th).saturating_sub(td.rows), (c0 + tw).saturating_sub(td.cols)) } else { (0, 0) };
    for k in 0..more_r {
        doc.table_insert(h, t, Axis::Row, td.rows + k, td.rows + k - 1)?;
    }
    for k in 0..more_c {
        doc.table_insert(h, t, Axis::Column, td.cols + k, td.cols + k - 1)?;
    }
    let td = if more_r > 0 || more_c > 0 { table_data(doc, t).ok_or(err("not a table"))? } else { td };
    let (rows, cols) = (td.rows, td.cols);
    let dest_merges: Vec<CellRange> = td.merges.iter().filter(|m| m.row < r0 + th && m.row + m.rows > r0 && m.col < c0 + tw && m.col + m.cols > c0).copied().collect();
    let inside_dest_merge = |r: u32, c: u32| dest_merges.iter().any(|m| m.contains(r, c) && (m.row, m.col) != (r, c));
    let (dest_looks, dest_grid) = (region_looks(&td), grid_lines(&td));
    let mut jobs: Vec<Job> = Vec::new();
    for i in 0..th {
        for j in 0..tw {
            let (r, c) = (r0 + i, c0 + j);
            if r >= rows || c >= cols || (into_merge && inside_dest_merge(r, c)) {
                continue;
            }
            let k = ((i % clip.rows) * clip.cols + (j % clip.cols)) as usize;
            let Some(cc) = clip.cells.get(k).cloned() else { continue };
            if cc.covered && !remerge {
                continue;
            }
            // a merge pasted as one ordinary cell: the merge's outline is that cell's border
            let cc = match &cc.outline {
                Some(o) if !remerge => CellCopy { edges: o.clone(), ..cc },
                _ => cc,
            };
            let ix = (r * cols + c) as usize;
            let src_look = clip.looks[cc.region.min(3) as usize].clone();
            let dest_look = dest_looks[region(&td, r, c) as usize].clone();
            jobs.push(Job { r, c, cc, top_r: i64::from(r) - i64::from(i % clip.rows), left_c: i64::from(c) - i64::from(j % clip.cols), dest: td.cell_styles.get(ix).cloned(), dest_edges: cell_edges(&td, r, c), src_look, dest_look, src_grid: clip.grid.clone(), dest_grid: dest_grid.clone() });
        }
    }
    let unmerge: Vec<(u32, u32)> = if into_merge { Vec::new() } else { dest_merges.iter().map(|m| (m.row, m.col)).collect() };
    let mut new_merges: Vec<CellRange> = Vec::new();
    if remerge {
        for ti in 0..th.div_ceil(clip.rows) {
            for tj in 0..tw.div_ceil(clip.cols) {
                for &(mr, mc, nr, nc) in &clip.merges {
                    let (r, c) = (r0 + ti * clip.rows + mr, c0 + tj * clip.cols + mc);
                    if r + nr <= (r0 + th).min(rows) && c + nc <= (c0 + tw).min(cols) {
                        new_merges.push(CellRange { row: r, col: c, rows: nr, cols: nc });
                    }
                }
            }
        }
    }
    // a custom format pasted into another document: its pattern as a plain date format
    let same_doc = clip.doc == doc_key(doc);
    let portable = |f: &CellFormat| -> CellFormat { if same_doc { f.clone() } else { portable_format(f) } };
    for &(r, c) in &unmerge {
        doc.unmerge_cells(h, t, r, c);
    }
    // (a cell that can't take part of its look doesn't stop the rest of the paste)
    let mut failed = None;
    for job in &jobs {
        if let Err(e) = write_cell(doc, h, t, job, mode, clip.origin, &portable) {
            failed.get_or_insert(e);
        }
    }
    for m in &new_merges {
        doc.merge_cells(h, t, *m)?;
    }
    doc.recalc_formulas(h, t);
    failed.map_or(Ok(((r0 + th - 1).min(rows - 1), (c0 + tw - 1).min(cols - 1))), Err).map(|(er, ec)| (r0, c0, er, ec))
}

/// A data format as it means the same in another document (a custom format: its pattern as a
/// plain date format, else Automatic).
pub fn portable_format(f: &CellFormat) -> CellFormat {
    match f {
        CellFormat::Custom { format } => match (format.u32("format_type"), format.str("custom_format_string")) {
            (Some(271), Some(p)) => CellFormat::DateTime { pattern: p.trim().to_string() },
            _ => CellFormat::Automatic,
        },
        f => f.clone(),
    }
}

/// Table `t` (just made) as the copy: its headers (a whole table's), the copy's size, the cells,
/// and when known its column widths, row heights and name. The caller opens and closes the undo
/// step.
pub fn write_new(doc: &mut Document, h: &mut History, t: ObjId, clip: &TableClip) -> Result<(), kn_format::Error> {
    let (hr, hc, fr) = if clip.whole { clip.headers } else { (0, 0, 0) };
    for (k, n) in [(HeaderKind::Rows, hr), (HeaderKind::Columns, hc), (HeaderKind::Footer, fr)] {
        let _ = doc.set_table_header_count(h, t, k, n.min(if matches!(k, HeaderKind::Columns) { clip.cols } else { clip.rows }));
    }
    resize(doc, h, t, clip.rows, clip.cols)?;
    write(doc, h, t, (0, 0, 0, 0), clip, PasteMode::All)?;
    let sizes = |v: &[f32]| -> Vec<(u32, f32)> { v.iter().enumerate().filter(|(_, s)| **s > 0.5).map(|(i, s)| (i as u32, *s)).collect() };
    let (w, ht) = (sizes(&clip.col_widths), sizes(&clip.row_heights));
    if !w.is_empty() {
        doc.set_table_sizes(h, t, false, &w);
    }
    if !ht.is_empty() {
        doc.set_table_sizes(h, t, true, &ht);
    }
    let name = clip.name.trim();
    if !name.is_empty() && !doc.file.objects_of("TST.TableModelArchive").any(|(_, m)| m.str("table_name") == Some(name)) {
        if let Some(m) = doc.file.obj(t).and_then(|m| m.ref_id("tableModel")) {
            doc.touch(h, m);
            if let Some(o) = doc.file.obj_mut(m) {
                o.set_str("table_name", name);
            }
        }
    }
    Ok(())
}

/// Table `t` to `rows` x `cols` (rows / columns added like the last one, taken off the end).
pub fn resize(doc: &mut Document, h: &mut History, t: ObjId, rows: u32, cols: u32) -> Result<(), kn_format::Error> {
    let size = |d: &Document| d.file.obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|m| d.file.obj(m)).map(|m| (m.u32("number_of_rows").unwrap_or(0), m.u32("number_of_columns").unwrap_or(0))).unwrap_or((0, 0));
    let (r, c) = size(doc);
    for _ in rows.max(1)..r {
        let n = size(doc).0;
        doc.table_delete(h, t, Axis::Row, n - 1)?;
    }
    for k in r..rows {
        doc.table_insert(h, t, Axis::Row, k, k.saturating_sub(1))?;
    }
    for _ in cols.max(1)..c {
        let n = size(doc).1;
        doc.table_delete(h, t, Axis::Column, n - 1)?;
    }
    for k in c..cols {
        doc.table_insert(h, t, Axis::Column, k, k.saturating_sub(1))?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------- the data

fn color_j(c: &Color) -> Value {
    json!([c.r, c.g, c.b, c.a])
}

fn color_of(v: &Value) -> Option<Color> {
    let a = v.as_array()?;
    let f = |i: usize| a.get(i).and_then(Value::as_f64).map(|x| x as f32);
    Some(Color { r: f(0)?, g: f(1)?, b: f(2)?, a: f(3).unwrap_or(1.0) })
}

fn f32_of(v: &Value, k: &str) -> Option<f32> {
    v.get(k).and_then(Value::as_f64).map(|x| x as f32)
}

fn bool_of(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(Value::as_bool)
}

fn u32_of(v: &Value, k: &str) -> Option<u32> {
    v.get(k).and_then(Value::as_u64).map(|x| x as u32)
}

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn stroke_j(s: &Option<Stroke>) -> Value {
    match s {
        None => Value::Null,
        Some(s) => json!({
            "c": color_j(&s.color),
            "w": s.width,
            "cap": s.cap as u8,
            "join": s.join as u8,
            "m": s.miter_limit,
            "d": s.dash,
            "p": s.phase,
        }),
    }
}

fn stroke_of(v: &Value) -> Option<Stroke> {
    if v.is_null() {
        return None;
    }
    Some(Stroke {
        color: color_of(v.get("c")?)?,
        width: f32_of(v, "w").unwrap_or(1.0),
        cap: match u32_of(v, "cap") {
            Some(1) => LineCap::Round,
            Some(2) => LineCap::Square,
            _ => LineCap::Butt,
        },
        join: match u32_of(v, "join") {
            Some(1) => LineJoin::Round,
            Some(2) => LineJoin::Bevel,
            _ => LineJoin::Miter,
        },
        miter_limit: f32_of(v, "m").unwrap_or(4.0),
        dash: v.get("d").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default(),
        phase: f32_of(v, "p").unwrap_or(0.0),
        brush: None,
    })
}

fn fill_j(f: &Fill) -> Value {
    match f {
        Fill::None => Value::Null,
        Fill::Color(c) => json!({"c": color_j(c)}),
        Fill::Gradient { kind, stops, opacity, inflections, advanced } => json!({
            "g": match kind { GradientKind::Linear { angle } => json!(angle), GradientKind::Radial => Value::Null },
            "s": stops.iter().map(|(p, c)| json!([p, color_j(c)])).collect::<Vec<_>>(),
            "o": opacity,
            "i": inflections,
            "a": advanced,
        }),
        // (a picture's data is the document's own: its stand-in colour travels)
        Fill::Image { reference, .. } => reference.map(|c| json!({"c": color_j(&c)})).unwrap_or(Value::Null),
    }
}

fn fill_of(v: &Value) -> Fill {
    if let Some(c) = v.get("c").and_then(color_of) {
        return Fill::Color(c);
    }
    if let Some(s) = v.get("s").and_then(Value::as_array) {
        let stops: Vec<(f32, Color)> = s.iter().filter_map(|x| Some((x.get(0)?.as_f64()? as f32, color_of(x.get(1)?)?))).collect();
        if !stops.is_empty() {
            let kind = match v.get("g").and_then(Value::as_f64) {
                Some(a) => GradientKind::Linear { angle: a as f32 },
                None => GradientKind::Radial,
            };
            let inflections = v.get("i").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default();
            return Fill::Gradient { kind, stops, opacity: f32_of(v, "o").unwrap_or(1.0), inflections, advanced: bool_of(v, "a").unwrap_or(false) };
        }
    }
    Fill::None
}

fn align_n(a: Align) -> u8 {
    match a {
        Align::Left => 0,
        Align::Right => 1,
        Align::Center => 2,
        Align::Justify => 3,
        Align::Natural => 4,
    }
}

fn align_of(n: Option<u32>) -> Align {
    match n {
        Some(1) => Align::Right,
        Some(2) => Align::Center,
        Some(3) => Align::Justify,
        Some(4) => Align::Natural,
        _ => Align::Left,
    }
}

fn style_j(s: &TableCellStyle) -> Value {
    let t = &s.text;
    let (lk, lv) = match s.para.line_spacing {
        LineSpacing::Relative(v) => (0, v),
        LineSpacing::AtLeast(v) => (1, v),
        LineSpacing::Exactly(v) => (2, v),
        LineSpacing::AtMost(v) => (3, v),
        LineSpacing::Between(v) => (4, v),
    };
    json!({
        "fill": fill_j(&s.fill),
        "font": t.font_name,
        "size": t.size,
        "b": t.bold,
        "i": t.italic,
        "u": t.underline,
        "x": t.strike,
        "color": color_j(&t.color),
        "script": t.script,
        "shift": t.baseline_shift,
        "caps": match t.caps { Caps::None => 0, Caps::All => 1, Caps::Small => 2, Caps::Title => 3, Caps::Start => 4 },
        "track": t.tracking,
        "bg": t.background.as_ref().map(color_j),
        "al": align_n(s.align),
        "va": match s.valign { VAlign::Top => 0, VAlign::Middle => 1, VAlign::Bottom => 2 },
        "pad": s.padding,
        "wrap": s.wrap,
        "ls": [lk, lv],
        "sb": s.para.space_before,
        "sa": s.para.space_after,
        "li": s.para.left_indent,
        "ri": s.para.right_indent,
        "fi": s.para.first_line_indent,
    })
}

fn style_of(v: &Value) -> Option<TableCellStyle> {
    if !v.is_object() {
        return None;
    }
    let d = TableCellStyle::default();
    let dt = CharStyle::default();
    let text = CharStyle {
        font_name: str_of(v, "font").map(str::to_string).unwrap_or(dt.font_name),
        size: f32_of(v, "size").unwrap_or(dt.size),
        bold: bool_of(v, "b").unwrap_or(false),
        italic: bool_of(v, "i").unwrap_or(false),
        color: v.get("color").and_then(color_of).unwrap_or(dt.color),
        underline: bool_of(v, "u").unwrap_or(false),
        strike: bool_of(v, "x").unwrap_or(false),
        script: v.get("script").and_then(Value::as_i64).unwrap_or(0) as i8,
        baseline_shift: f32_of(v, "shift").unwrap_or(0.0),
        caps: match u32_of(v, "caps") {
            Some(1) => Caps::All,
            Some(2) => Caps::Small,
            Some(3) => Caps::Title,
            Some(4) => Caps::Start,
            _ => Caps::None,
        },
        tracking: f32_of(v, "track").unwrap_or(0.0),
        background: v.get("bg").and_then(color_of),
        ..dt
    };
    let pad = v.get("pad").and_then(Value::as_array).filter(|a| a.len() == 4).map(|a| [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(4.0) as f32)).unwrap_or(d.padding);
    let ls = v.get("ls").and_then(Value::as_array);
    let lv = ls.and_then(|a| a.get(1)).and_then(Value::as_f64).unwrap_or(1.0) as f32;
    let line_spacing = match ls.and_then(|a| a.first()).and_then(Value::as_u64) {
        Some(1) => LineSpacing::AtLeast(lv),
        Some(2) => LineSpacing::Exactly(lv),
        Some(3) => LineSpacing::AtMost(lv),
        Some(4) => LineSpacing::Between(lv),
        _ => LineSpacing::Relative(lv),
    };
    Some(TableCellStyle {
        fill: v.get("fill").map(fill_of).unwrap_or(Fill::None),
        text,
        align: align_of(u32_of(v, "al")),
        valign: match u32_of(v, "va") {
            Some(0) => VAlign::Top,
            Some(2) => VAlign::Bottom,
            _ => VAlign::Middle,
        },
        padding: pad,
        wrap: bool_of(v, "wrap").unwrap_or(true),
        para: crate::CellPara {
            line_spacing,
            space_before: f32_of(v, "sb").unwrap_or(0.0),
            space_after: f32_of(v, "sa").unwrap_or(0.0),
            left_indent: f32_of(v, "li").unwrap_or(0.0),
            right_indent: f32_of(v, "ri").unwrap_or(0.0),
            first_line_indent: f32_of(v, "fi").unwrap_or(0.0),
        },
    })
}

fn format_j(f: &CellFormat) -> Value {
    let dec = |d: &Option<u32>| d.map_or(Value::Null, |d| json!(d));
    match portable_format(f) {
        CellFormat::Automatic | CellFormat::Custom { .. } => json!({"k": "auto"}),
        CellFormat::Number { decimals, thousands, negative } => json!({"k": "num", "d": dec(&decimals), "t": thousands, "n": negative}),
        CellFormat::Currency { code, decimals, thousands, negative, accounting } => json!({"k": "cur", "code": code, "d": dec(&decimals), "t": thousands, "n": negative, "acc": accounting}),
        CellFormat::Percent { decimals, thousands, negative } => json!({"k": "pct", "d": dec(&decimals), "t": thousands, "n": negative}),
        CellFormat::Fraction { accuracy } => json!({"k": "frac", "a": accuracy}),
        CellFormat::Scientific { decimals } => json!({"k": "sci", "d": dec(&decimals)}),
        CellFormat::Base { base, places, minus } => json!({"k": "base", "b": base, "p": places, "m": minus}),
        CellFormat::Text => json!({"k": "text"}),
        CellFormat::DateTime { pattern } => json!({"k": "date", "p": pattern}),
        CellFormat::Duration { style, largest, smallest, automatic } => json!({"k": "dur", "s": style, "l": largest, "sm": smallest, "a": automatic}),
    }
}

fn format_of(v: &Value) -> CellFormat {
    let dec = v.get("d").and_then(Value::as_u64).map(|d| d as u32);
    let t = bool_of(v, "t").unwrap_or(false);
    let n = u32_of(v, "n").unwrap_or(0);
    match str_of(v, "k").unwrap_or("auto") {
        "num" => CellFormat::Number { decimals: dec, thousands: t, negative: n },
        "cur" => CellFormat::Currency { code: str_of(v, "code").unwrap_or("USD").to_string(), decimals: dec, thousands: t, negative: n, accounting: bool_of(v, "acc").unwrap_or(false) },
        "pct" => CellFormat::Percent { decimals: dec, thousands: t, negative: n },
        "frac" => CellFormat::Fraction { accuracy: u32_of(v, "a").unwrap_or(u32::MAX) },
        "sci" => CellFormat::Scientific { decimals: dec },
        "base" => CellFormat::Base { base: u32_of(v, "b").unwrap_or(10), places: u32_of(v, "p").unwrap_or(0), minus: bool_of(v, "m").unwrap_or(true) },
        "text" => CellFormat::Text,
        "date" => CellFormat::DateTime { pattern: str_of(v, "p").unwrap_or("").to_string() },
        "dur" => CellFormat::Duration { style: u32_of(v, "s").unwrap_or(0), largest: u32_of(v, "l").unwrap_or(0), smallest: u32_of(v, "sm").unwrap_or(0), automatic: bool_of(v, "a").unwrap_or(true) },
        _ => CellFormat::Automatic,
    }
}

fn value_j(v: &Option<CellValue>) -> Value {
    match v {
        None => Value::Null,
        Some(CellValue::Empty) => json!({}),
        Some(CellValue::Number(n)) => json!({"n": n}),
        Some(CellValue::Date(n)) => json!({"date": n}),
        Some(CellValue::Duration(n)) => json!({"dur": n}),
        Some(CellValue::Bool(b)) => json!({"b": b}),
        Some(CellValue::Text(s)) => json!({"t": s}),
    }
}

fn value_of(v: &Value) -> Option<CellValue> {
    let o = v.as_object()?;
    Some(if let Some(n) = o.get("n").and_then(Value::as_f64) {
        CellValue::Number(n)
    } else if let Some(n) = o.get("date").and_then(Value::as_f64) {
        CellValue::Date(n)
    } else if let Some(n) = o.get("dur").and_then(Value::as_f64) {
        CellValue::Duration(n)
    } else if let Some(b) = o.get("b").and_then(Value::as_bool) {
        CellValue::Bool(b)
    } else if let Some(s) = o.get("t").and_then(Value::as_str) {
        CellValue::Text(s.to_string())
    } else {
        CellValue::Empty
    })
}

fn edges_j(e: &[Option<Stroke>; 4]) -> Value {
    Value::Array(e.iter().map(stroke_j).collect())
}

fn edges_of(v: &Value) -> [Option<Stroke>; 4] {
    let a = v.as_array();
    [0, 1, 2, 3].map(|i| a.and_then(|a| a.get(i)).and_then(stroke_of))
}

impl TableClip {
    /// The exact data as JSON.
    pub fn to_json(&self) -> String {
        let cells: Vec<Value> = self
            .cells
            .iter()
            .map(|c| {
                json!({
                    "f": c.formula,
                    "s": c.shown,
                    "fmt": format_j(&c.format),
                    "st": c.style.as_ref().map(style_j),
                    "cv": c.covered,
                    "e": edges_j(&c.edges),
                    "o": c.outline.as_ref().map(edges_j),
                    "rg": c.region,
                    "v": value_j(&c.value),
                })
            })
            .collect();
        json!({
            "v": 1,
            "rows": self.rows,
            "cols": self.cols,
            "cells": cells,
            "merges": self.merges.iter().map(|m| json!([m.0, m.1, m.2, m.3])).collect::<Vec<_>>(),
            "origin": [self.origin.0, self.origin.1],
            "looks": self.looks.iter().map(|l| l.as_ref().map(style_j)).collect::<Vec<_>>(),
            "grid": [stroke_j(&self.grid.0), stroke_j(&self.grid.1)],
            "cw": self.col_widths,
            "rh": self.row_heights,
            "name": self.name,
            "whole": self.whole,
            "hdr": [self.headers.0, self.headers.1, self.headers.2],
            "src": self.source,
            "doc": self.doc,
        })
        .to_string()
    }

    pub fn from_json(s: &str) -> Option<TableClip> {
        let v: Value = serde_json::from_str(s).ok()?;
        let rows = u32_of(&v, "rows")?;
        let cols = u32_of(&v, "cols")?;
        if rows == 0 || cols == 0 || rows.saturating_mul(cols) > 4_000_000 {
            return None;
        }
        let mut cells: Vec<CellCopy> = v
            .get("cells")?
            .as_array()?
            .iter()
            .map(|c| CellCopy {
                formula: str_of(c, "f").map(str::to_string),
                shown: str_of(c, "s").unwrap_or("").to_string(),
                format: c.get("fmt").map(format_of).unwrap_or(CellFormat::Automatic),
                style: c.get("st").and_then(style_of),
                covered: bool_of(c, "cv").unwrap_or(false),
                edges: c.get("e").map(edges_of).unwrap_or_default(),
                outline: c.get("o").filter(|o| !o.is_null()).map(edges_of),
                region: u32_of(c, "rg").unwrap_or(3).min(3) as u8,
                value: c.get("v").and_then(value_of),
            })
            .collect();
        cells.resize((rows * cols) as usize, CellCopy::default());
        let nums = |k: &str| -> Vec<f32> { v.get(k).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default() };
        let pair = |k: &str, i: usize| v.get(k).and_then(|a| a.get(i)).and_then(Value::as_u64).unwrap_or(0) as u32;
        let looks = v.get("looks").and_then(Value::as_array);
        let grid = v.get("grid").and_then(Value::as_array);
        Some(TableClip {
            rows,
            cols,
            cells,
            merges: v
                .get("merges")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|m| Some((m.get(0)?.as_u64()? as u32, m.get(1)?.as_u64()? as u32, m.get(2)?.as_u64()? as u32, m.get(3)?.as_u64()? as u32))).filter(|m| m.2 >= 1 && m.3 >= 1 && m.0 + m.2 <= rows && m.1 + m.3 <= cols).collect())
                .unwrap_or_default(),
            origin: (pair("origin", 0), pair("origin", 1)),
            looks: [0, 1, 2, 3].map(|i| looks.and_then(|l| l.get(i)).and_then(style_of)),
            grid: (grid.and_then(|g| g.first()).and_then(stroke_of), grid.and_then(|g| g.get(1)).and_then(stroke_of)),
            col_widths: nums("cw"),
            row_heights: nums("rh"),
            name: str_of(&v, "name").unwrap_or("").to_string(),
            whole: bool_of(&v, "whole").unwrap_or(false),
            headers: (pair("hdr", 0), pair("hdr", 1), pair("hdr", 2)),
            source: str_of(&v, "src").unwrap_or("").to_string(),
            doc: str_of(&v, "doc").unwrap_or("").to_string(),
        })
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for ch in b.chunks(3) {
        let n = (u32::from(ch[0]) << 16) | (u32::from(*ch.get(1).unwrap_or(&0)) << 8) | u32::from(*ch.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= ch.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ------------------------------------------------------------------------------- HTML out

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: &Color) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

fn css_color(c: &Color) -> String {
    if c.a < 0.999 { format!("rgba({},{},{},{:.3})", (c.r * 255.0).round(), (c.g * 255.0).round(), (c.b * 255.0).round(), c.a) } else { hex(c) }
}

fn css_border(s: &Option<Stroke>) -> String {
    match s {
        Some(s) if s.width > 0.0 && s.color.a > 0.0 => {
            let kind = if s.dash.is_empty() { "solid" } else if s.dash[0] < 0.5 { "dotted" } else { "dashed" };
            format!("{}pt {kind} {}", fmt_num(s.width), css_color(&s.color))
        }
        _ => "none".into(),
    }
}

fn fmt_num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl TableClip {
    /// The cells as an HTML table other programs read (inline CSS: fills, fonts, alignment,
    /// borders, merged cells as colspan / rowspan), carrying the exact data for the suite's apps.
    pub fn to_html(&self) -> String {
        let data = base64_encode(self.to_json().as_bytes());
        let mut out = String::new();
        out.push_str("<meta charset=\"utf-8\">");
        out.push_str(&format!("<table data-bramble-table=\"{data}\" cellspacing=\"0\" cellpadding=\"0\" style=\"border-collapse:collapse\">"));
        if self.col_widths.len() == self.cols as usize && self.col_widths.iter().all(|w| *w > 0.0) {
            out.push_str("<colgroup>");
            for w in &self.col_widths {
                out.push_str(&format!("<col width=\"{}\" style=\"width:{}pt\">", (w / 0.75).round(), fmt_num(*w)));
            }
            out.push_str("</colgroup>");
        }
        for r in 0..self.rows {
            match self.row_heights.get(r as usize) {
                Some(h) if *h > 0.0 => out.push_str(&format!("<tr style=\"height:{}pt\">", fmt_num(*h))),
                _ => out.push_str("<tr>"),
            }
            for c in 0..self.cols {
                let Some(cc) = self.cell(r, c) else { continue };
                let merge = self.merges.iter().find(|m| m.0 <= r && r < m.0 + m.2 && m.1 <= c && c < m.1 + m.3);
                if cc.covered || merge.is_some_and(|m| (m.0, m.1) != (r, c)) {
                    continue;
                }
                out.push_str("<td");
                if let Some(m) = merge {
                    if m.3 > 1 {
                        out.push_str(&format!(" colspan=\"{}\"", m.3));
                    }
                    if m.2 > 1 {
                        out.push_str(&format!(" rowspan=\"{}\"", m.2));
                    }
                }
                if let Some(CellValue::Number(n)) = &cc.value {
                    out.push_str(&format!(" sdval=\"{n}\""));
                }
                // (the old attributes too: some spreadsheets read only these)
                if let Some(st) = &cc.style {
                    if let Fill::Color(c) = &st.fill {
                        if c.a > 0.0 {
                            out.push_str(&format!(" bgcolor=\"{}\"", hex(c)));
                        }
                    }
                    match st.align {
                        Align::Center => out.push_str(" align=\"center\""),
                        Align::Right => out.push_str(" align=\"right\""),
                        _ => {}
                    }
                }
                let edges = cc.outline.as_ref().unwrap_or(&cc.edges);
                let mut css = Vec::new();
                let mut text = esc(&cc.shown).replace("\r\n", "\n").replace('\n', "<br>");
                if let Some(st) = &cc.style {
                    match &st.fill {
                        Fill::Color(c) if c.a > 0.0 => css.push(format!("background-color:{}", css_color(c))),
                        Fill::Gradient { stops, .. } if !stops.is_empty() => css.push(format!("background-color:{}", css_color(&stops[0].1))),
                        _ => {}
                    }
                    let t = &st.text;
                    css.push(format!("color:{}", css_color(&t.color)));
                    css.push(format!("font-size:{}pt", fmt_num(t.size)));
                    if !t.font_name.is_empty() {
                        css.push(format!("font-family:'{}'", t.font_name.replace(['\'', '"', ';'], "")));
                    }
                    if t.bold {
                        css.push("font-weight:bold".into());
                        text = format!("<b>{text}</b>");
                    }
                    if t.italic {
                        css.push("font-style:italic".into());
                        text = format!("<i>{text}</i>");
                    }
                    if t.underline || t.strike {
                        css.push(format!("text-decoration:{}", [(t.underline, "underline"), (t.strike, "line-through")].iter().filter(|x| x.0).map(|x| x.1).collect::<Vec<_>>().join(" ")));
                        if t.underline {
                            text = format!("<u>{text}</u>");
                        }
                        if t.strike {
                            text = format!("<s>{text}</s>");
                        }
                    }
                    match st.align {
                        Align::Center => css.push("text-align:center".into()),
                        Align::Right => css.push("text-align:right".into()),
                        Align::Justify => css.push("text-align:justify".into()),
                        Align::Left => css.push("text-align:left".into()),
                        Align::Natural => {}
                    }
                    css.push(format!("vertical-align:{}", match st.valign {
                        VAlign::Top => "top",
                        VAlign::Middle => "middle",
                        VAlign::Bottom => "bottom",
                    }));
                    css.push(format!("padding:{}pt {}pt {}pt {}pt", fmt_num(st.padding[1]), fmt_num(st.padding[2]), fmt_num(st.padding[3]), fmt_num(st.padding[0])));
                    if !st.wrap {
                        css.push("white-space:nowrap".into());
                    }
                }
                for (k, side) in ["top", "bottom", "left", "right"].iter().enumerate() {
                    css.push(format!("border-{side}:{}", css_border(&edges[k])));
                }
                out.push_str(&format!(" style=\"{}\">{text}</td>", esc(&css.join(";"))));
            }
            out.push_str("</tr>");
        }
        out.push_str("</table>");
        out
    }

    /// A table from clipboard HTML: the suite's exact data when it is there, else the first
    /// HTML table (values with their basic look: fills, bold / italic / underline / strike,
    /// colours, sizes, alignment, borders, merged cells, sizes).
    pub fn from_html(html: &str) -> Option<TableClip> {
        if let Some(i) = html.find("data-bramble-table=\"") {
            let rest = &html[i + "data-bramble-table=\"".len()..];
            if let Some(end) = rest.find('"') {
                if let Some(c) = base64_decode(&rest[..end]).and_then(|b| String::from_utf8(b).ok()).and_then(|s| TableClip::from_json(&s)) {
                    return Some(c);
                }
            }
        }
        parse_html_table(html)
    }
}

/// Whether clipboard HTML holds a table at all (cheap check before parsing).
pub fn has_table(html: &str) -> bool {
    let low = html.to_ascii_lowercase();
    low.contains("<table") && (low.contains("<td") || low.contains("<th"))
}

/// The table to paste as a new table where text could go too (a document's body, a slide, a
/// note): the suite's own tables always; a page's table only when what was copied is just the
/// table (its text is tab-separated cells), not a page of text with a table in it.
pub fn table_paste(html: &str, text: &str) -> Option<TableClip> {
    if !has_table(html) {
        return None;
    }
    let clip = TableClip::from_html(html)?;
    let ours = !clip.source.is_empty();
    (ours || text.trim().is_empty() || (text.contains('\t') && text.trim().lines().count() <= clip.rows as usize + 1)).then_some(clip)
}

// ------------------------------------------------------------------------------- HTML in

/// What CSS / attributes say about a cell (None: not said).
#[derive(Clone, Debug, Default)]
struct Look {
    fill: Option<Color>,
    color: Option<Color>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    size: Option<f32>,
    font: Option<String>,
    align: Option<Align>,
    valign: Option<VAlign>,
    wrap: Option<bool>,
    /// top, bottom, left, right: Some(None) no border
    borders: [Option<Option<Stroke>>; 4],
    width: Option<f32>,
    height: Option<f32>,
}

impl Look {
    /// `o` over this one.
    fn over(&self, o: &Look) -> Look {
        Look {
            fill: o.fill.or(self.fill),
            color: o.color.or(self.color),
            bold: o.bold.or(self.bold),
            italic: o.italic.or(self.italic),
            underline: o.underline.or(self.underline),
            strike: o.strike.or(self.strike),
            size: o.size.or(self.size),
            font: o.font.clone().or(self.font.clone()),
            align: o.align.or(self.align),
            valign: o.valign.or(self.valign),
            wrap: o.wrap.or(self.wrap),
            borders: [0, 1, 2, 3].map(|i| o.borders[i].clone().or(self.borders[i].clone())),
            width: o.width.or(self.width),
            height: o.height.or(self.height),
        }
    }
}

fn named_color(n: &str) -> Option<(u8, u8, u8)> {
    Some(match n {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "fuchsia" | "magenta" => (255, 0, 255),
        "aqua" | "cyan" => (0, 255, 255),
        "teal" => (0, 128, 128),
        "navy" => (0, 0, 128),
        "maroon" => (128, 0, 0),
        "olive" => (128, 128, 0),
        "silver" => (192, 192, 192),
        "gray" | "grey" => (128, 128, 128),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgray" | "darkgrey" => (169, 169, 169),
        _ => return None,
    })
}

/// A CSS colour (None: none said, or transparent).
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim().to_ascii_lowercase();
    let rgb = |r: u8, g: u8, b: u8, a: f32| Color { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a };
    if let Some(h) = s.strip_prefix('#') {
        let h: String = h.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        let n = |i: usize, l: usize| u8::from_str_radix(&h[i..i + l], 16).ok();
        return match h.len() {
            3 => Some(rgb(n(0, 1)? * 17, n(1, 1)? * 17, n(2, 1)? * 17, 1.0)),
            6 | 8 => Some(rgb(n(0, 2)?, n(2, 2)?, n(4, 2)?, if h.len() == 8 { f32::from(n(6, 2)?) / 255.0 } else { 1.0 })),
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("rgba(").or(s.strip_prefix("rgb(")) {
        let parts: Vec<&str> = inner.trim_end_matches(')').split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
        let ch = |p: &str| -> Option<u8> { if let Some(pc) = p.strip_suffix('%') { Some((pc.parse::<f32>().ok()? * 2.55).round().clamp(0.0, 255.0) as u8) } else { Some(p.parse::<f32>().ok()?.round().clamp(0.0, 255.0) as u8) } };
        let a = parts.get(3).and_then(|p| if let Some(pc) = p.strip_suffix('%') { pc.parse::<f32>().ok().map(|v| v / 100.0) } else { p.parse().ok() }).unwrap_or(1.0);
        if a <= 0.0 {
            return None;
        }
        return Some(rgb(ch(parts.first()?)?, ch(parts.get(1)?)?, ch(parts.get(2)?)?, a));
    }
    if s == "transparent" || s == "none" || s == "inherit" || s == "initial" {
        return None;
    }
    named_color(&s).map(|(r, g, b)| rgb(r, g, b, 1.0))
}

/// A CSS length in points (bare numbers are pixels, as HTML attributes are).
fn parse_len(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    let num_end = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(s.len());
    let v: f32 = s[..num_end].parse().ok()?;
    let unit = s[num_end..].trim();
    Some(match unit {
        "pt" => v,
        "px" | "" => v * 0.75,
        "in" => v * 72.0,
        "cm" => v * 72.0 / 2.54,
        "mm" => v * 72.0 / 25.4,
        "pc" => v * 12.0,
        "em" | "rem" => v * 12.0,
        _ => return None,
    })
}

fn parse_border(s: &str) -> Option<Option<Stroke>> {
    let low = s.trim().to_ascii_lowercase();
    if low == "none" || low == "0" || low.starts_with("hidden") || low.starts_with("0 ") || low.starts_with("0px") || low.starts_with("0pt") {
        return Some(None);
    }
    let (mut width, mut dash, mut color) = (None, Vec::new(), None);
    // (rgb(…) keeps its spaces together)
    let mut parts: Vec<String> = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in low.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ' ' if depth == 0 => {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    let mut none = false;
    for p in &parts {
        match p.as_str() {
            "none" | "hidden" => none = true,
            "solid" | "double" | "groove" | "ridge" | "inset" | "outset" => {}
            "dashed" => dash = vec![2.0, 2.0],
            "dotted" => dash = vec![0.1, 2.0],
            "thin" => width = Some(0.75),
            "medium" => width = Some(2.25),
            "thick" => width = Some(3.75),
            p => {
                if let Some(c) = parse_color(p) {
                    color = Some(c);
                } else if let Some(w) = parse_len(p) {
                    width = Some(w);
                }
            }
        }
    }
    if none || width.is_some_and(|w| w <= 0.0) {
        return Some(None);
    }
    Some(Some(Stroke { color: color.unwrap_or(Color::BLACK), width: width.unwrap_or(0.75), cap: LineCap::Butt, join: LineJoin::Miter, miter_limit: 4.0, dash, phase: 0.0, brush: None }))
}

/// A font family worth keeping (not a generic or system stand-in).
fn plain_family(s: &str) -> Option<String> {
    let f = s.split(',').next()?.trim().trim_matches(['\'', '"']).trim();
    let generic = ["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "inherit", "initial", "-apple-system", "blinkmacsystemfont", "ui-sans-serif", "ui-serif", "ui-monospace"];
    (!f.is_empty() && !generic.contains(&f.to_ascii_lowercase().as_str())).then(|| f.to_string())
}

fn parse_css(style: &str, look: &mut Look) {
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().trim_end_matches("!important").trim());
        let lv = v.to_ascii_lowercase();
        match k.as_str() {
            "background" | "background-color" => {
                if let Some(c) = v.split_whitespace().find_map(parse_color).or_else(|| parse_color(v)) {
                    look.fill = Some(c);
                }
            }
            "color" => look.color = parse_color(v).or(look.color),
            "font-weight" => look.bold = Some(lv == "bold" || lv == "bolder" || lv.parse::<u32>().is_ok_and(|w| w >= 600)),
            "font-style" => look.italic = Some(lv == "italic" || lv == "oblique"),
            "text-decoration" | "text-decoration-line" => {
                look.underline = Some(lv.contains("underline"));
                look.strike = Some(lv.contains("line-through"));
            }
            "font-size" => {
                if let Some(s) = parse_len(v).filter(|s| *s > 0.0 && *s < 1000.0) {
                    look.size = Some(s);
                }
            }
            "font-family" => look.font = plain_family(v).or(look.font.take()),
            "text-align" => {
                look.align = match lv.as_str() {
                    "center" => Some(Align::Center),
                    "right" | "end" => Some(Align::Right),
                    "justify" => Some(Align::Justify),
                    "left" | "start" => Some(Align::Left),
                    _ => look.align,
                }
            }
            "vertical-align" => {
                look.valign = match lv.as_str() {
                    "top" | "text-top" => Some(VAlign::Top),
                    "middle" => Some(VAlign::Middle),
                    "bottom" | "text-bottom" | "baseline" => Some(VAlign::Bottom),
                    _ => look.valign,
                }
            }
            "white-space" => look.wrap = Some(!(lv.starts_with("nowrap") || lv == "pre")),
            "border" => {
                if let Some(b) = parse_border(v) {
                    look.borders = [Some(b.clone()), Some(b.clone()), Some(b.clone()), Some(b)];
                }
            }
            "border-top" | "border-bottom" | "border-left" | "border-right" => {
                let i = ["border-top", "border-bottom", "border-left", "border-right"].iter().position(|x| *x == k).unwrap_or(0);
                if let Some(b) = parse_border(v) {
                    look.borders[i] = Some(b);
                }
            }
            "width" | "min-width" => look.width = parse_len(v).or(look.width),
            "height" | "min-height" => look.height = parse_len(v).or(look.height),
            _ => {}
        }
    }
}

/// An attribute's value from a tag's text (after its name).
fn attr(tag: &str, name: &str) -> Option<String> {
    let b = tag.as_bytes();
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find(name).map(|i| i + from) {
        from = i + name.len();
        let before_ok = i == 0 || b[i - 1].is_ascii_whitespace();
        let rest = low[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let at = tag.len() - rest.len() + 1;
        let v = tag[at..].trim_start();
        let val = match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or(""),
            _ => v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(decode_entities(val));
    }
    None
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|e| e + 1).unwrap_or(rest.len());
        let name = &rest[1..end];
        let ch: Option<char> = if let Some(n) = name.strip_prefix("#x").or(name.strip_prefix("#X")) {
            u32::from_str_radix(n, 16).ok().and_then(char::from_u32)
        } else if let Some(n) = name.strip_prefix('#') {
            n.parse().ok().and_then(char::from_u32)
        } else {
            match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "ndash" => Some('–'),
                "mdash" => Some('—'),
                "hellip" => Some('…'),
                "euro" => Some('€'),
                "pound" => Some('£'),
                "yen" => Some('¥'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                "laquo" => Some('«'),
                "raquo" => Some('»'),
                "lsquo" => Some('‘'),
                "rsquo" => Some('’'),
                "ldquo" => Some('“'),
                "rdquo" => Some('”'),
                _ => None,
            }
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end..];
                if rest.starts_with(';') {
                    rest = &rest[1..];
                }
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[derive(Default)]
struct RawCell {
    text: String,
    look: Look,
    span: (u32, u32),
    /// every text run's (bold, italic, underline, strike, colour, size, font)
    runs: Vec<(bool, bool, bool, bool, Option<Color>, Option<f32>, Option<String>)>,
    header: bool,
}

/// `<font size=n>` in points.
fn font_size(n: &str) -> Option<f32> {
    let n = n.trim();
    let v: i32 = n.trim_start_matches(['+', '-']).parse().ok()?;
    let v = if n.starts_with('+') { 3 + v } else if n.starts_with('-') { 3 - v } else { v };
    Some([7.5, 10.0, 12.0, 13.5, 18.0, 24.0, 36.0][(v.clamp(1, 7) - 1) as usize])
}

/// The first HTML table in `html` (LibreOffice, browsers, word processors).
fn parse_html_table(html: &str) -> Option<TableClip> {
    if !has_table(html) {
        return None;
    }
    let mut rows: Vec<Vec<RawCell>> = Vec::new();
    let mut row_looks: Vec<Look> = Vec::new();
    let mut col_widths: Vec<Option<f32>> = Vec::new();
    let mut cell: Option<RawCell> = None;
    let mut row_look = Look::default();
    // inline formatting inside a cell: (tag, what it says)
    let mut inline: Vec<(String, Look)> = Vec::new();
    let mut depth = 0u32;
    let (mut skip, mut pre) = (0u32, 0u32);
    let mut caption: Option<String> = None;
    let mut in_caption = false;
    let mut done = false;
    let mut k = 0;
    let b = html.as_bytes();
    let push_text = |cell: &mut Option<RawCell>, inline: &[(String, Look)], s: &str| {
        let Some(c) = cell.as_mut() else { return };
        if !s.trim().is_empty() {
            let mut l = c.look.clone();
            for (_, x) in inline {
                l = l.over(x);
            }
            c.runs.push((l.bold.unwrap_or(c.header), l.italic.unwrap_or(false), l.underline.unwrap_or(false), l.strike.unwrap_or(false), l.color, l.size, l.font.clone()));
        }
        c.text.push_str(s);
    };
    while k < b.len() && !done {
        if b[k] == b'<' {
            if html[k..].starts_with("<!--") {
                k = html[k..].find("-->").map(|e| k + e + 3).unwrap_or(b.len());
                continue;
            }
            let Some(end) = html[k..].find('>').map(|e| k + e) else { break };
            let tag = &html[k + 1..end];
            k = end + 1;
            let closing = tag.starts_with('/');
            let body = tag.trim_start_matches('/');
            let name: String = body.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            let style = || attr(body, "style").unwrap_or_default();
            match name.as_str() {
                "script" | "style" | "head" | "title" => {
                    if closing {
                        skip = skip.saturating_sub(1);
                    } else if !body.ends_with('/') {
                        skip += 1;
                    }
                }
                _ if skip > 0 => {}
                "table" => {
                    if closing {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            done = true;
                        }
                    } else {
                        depth += 1;
                    }
                }
                _ if depth == 0 => {}
                // a table inside a cell: its text only
                "tr" | "td" | "th" if depth > 1 => {
                    if closing {
                        push_text(&mut cell, &inline, if name == "tr" { "\n" } else { " " });
                    }
                }
                "caption" => in_caption = !closing,
                "col" if !closing => {
                    let n = attr(body, "span").and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).clamp(1, 1000);
                    let mut l = Look::default();
                    parse_css(&style(), &mut l);
                    let w = l.width.or_else(|| attr(body, "width").and_then(|w| parse_len(&w)));
                    col_widths.extend(std::iter::repeat_n(w, n));
                }
                "tr" => {
                    if let Some(c) = cell.take() {
                        if rows.is_empty() {
                            rows.push(Vec::new());
                            row_looks.push(Look::default());
                        }
                        rows.last_mut().unwrap().push(c);
                    }
                    inline.clear();
                    if !closing {
                        let mut l = Look::default();
                        parse_css(&style(), &mut l);
                        if let Some(c) = attr(body, "bgcolor").and_then(|c| parse_color(&c)) {
                            l.fill = l.fill.or(Some(c));
                        }
                        if let Some(h) = attr(body, "height").and_then(|h| parse_len(&h)) {
                            l.height = l.height.or(Some(h));
                        }
                        row_look = l.clone();
                        rows.push(Vec::new());
                        row_looks.push(l);
                    }
                }
                "td" | "th" => {
                    if let Some(c) = cell.take() {
                        if rows.is_empty() {
                            rows.push(Vec::new());
                            row_looks.push(Look::default());
                        }
                        rows.last_mut().unwrap().push(c);
                    }
                    inline.clear();
                    if !closing {
                        let mut l = Look::default();
                        if let Some(c) = attr(body, "bgcolor").and_then(|c| parse_color(&c)) {
                            l.fill = Some(c);
                        }
                        l.align = match attr(body, "align").map(|a| a.to_ascii_lowercase()).as_deref() {
                            Some("center") => Some(Align::Center),
                            Some("right") => Some(Align::Right),
                            Some("left") => Some(Align::Left),
                            Some("justify") => Some(Align::Justify),
                            _ => None,
                        };
                        l.valign = match attr(body, "valign").map(|a| a.to_ascii_lowercase()).as_deref() {
                            Some("top") => Some(VAlign::Top),
                            Some("middle") | Some("center") => Some(VAlign::Middle),
                            Some("bottom") | Some("baseline") => Some(VAlign::Bottom),
                            _ => None,
                        };
                        l.width = attr(body, "width").and_then(|w| parse_len(&w));
                        l.height = attr(body, "height").and_then(|h| parse_len(&h));
                        if attr(body, "nowrap").is_some() || body.to_ascii_lowercase().split_whitespace().any(|w| w == "nowrap") {
                            l.wrap = Some(false);
                        }
                        parse_css(&style(), &mut l);
                        let span = |a: &str| attr(body, a).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(1).clamp(1, 1000);
                        let mut rl = row_look.clone();
                        rl.width = None;
                        cell = Some(RawCell { look: rl.over(&l), span: (span("rowspan"), span("colspan")), header: name == "th", ..Default::default() });
                    }
                }
                "br" => push_text(&mut cell, &inline, "\n"),
                "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    if let Some(c) = cell.as_mut() {
                        if !c.text.is_empty() && !c.text.ends_with('\n') {
                            c.text.push('\n');
                        }
                    }
                    if !closing {
                        let mut l = Look::default();
                        parse_css(&style(), &mut l);
                        if name.starts_with('h') {
                            l.bold = Some(true);
                        }
                        inline.push((name.clone(), l));
                    } else if let Some(i) = inline.iter().rposition(|(n, _)| *n == name) {
                        inline.truncate(i);
                    }
                }
                "pre" => pre = if closing { pre.saturating_sub(1) } else { pre + 1 },
                "b" | "strong" | "i" | "em" | "u" | "ins" | "s" | "strike" | "del" | "font" | "span" | "a" | "sup" | "sub" | "small" | "big" | "code" => {
                    if closing {
                        if let Some(i) = inline.iter().rposition(|(n, _)| *n == name) {
                            inline.truncate(i);
                        }
                    } else if !body.ends_with('/') {
                        let mut l = Look::default();
                        match name.as_str() {
                            "b" | "strong" => l.bold = Some(true),
                            "i" | "em" => l.italic = Some(true),
                            "u" | "ins" => l.underline = Some(true),
                            "s" | "strike" | "del" => l.strike = Some(true),
                            "font" => {
                                l.color = attr(body, "color").and_then(|c| parse_color(&c));
                                l.size = attr(body, "size").and_then(|s| font_size(&s));
                                l.font = attr(body, "face").and_then(|f| plain_family(&f));
                            }
                            _ => {}
                        }
                        parse_css(&style(), &mut l);
                        inline.push((name.clone(), l));
                    }
                }
                _ => {}
            }
            continue;
        }
        let next = html[k..].find('<').map(|e| k + e).unwrap_or(b.len());
        let raw = &html[k..next];
        k = next;
        if skip > 0 || depth == 0 {
            continue;
        }
        let text = decode_entities(raw);
        let text = if pre > 0 {
            text
        } else {
            // runs of white space are one space
            let lead = text.starts_with(|c: char| c.is_whitespace() && c != '\u{a0}');
            let mut s = String::with_capacity(text.len());
            let mut space = false;
            for ch in text.chars() {
                if ch.is_whitespace() && ch != '\u{a0}' {
                    space = true;
                } else {
                    if space && !s.is_empty() {
                        s.push(' ');
                    }
                    space = false;
                    s.push(ch);
                }
            }
            if space && !s.is_empty() {
                s.push(' ');
            }
            if lead && cell.as_ref().is_some_and(|c| !c.text.is_empty() && !c.text.ends_with(['\n', ' '])) {
                s.insert(0, ' ');
            }
            s
        };
        if in_caption {
            caption.get_or_insert_with(String::new).push_str(&text);
        } else {
            push_text(&mut cell, &inline, &text);
        }
    }
    if let Some(c) = cell.take() {
        if rows.is_empty() {
            rows.push(Vec::new());
            row_looks.push(Look::default());
        }
        rows.last_mut().unwrap().push(c);
    }
    // (rows of nothing at the end: a stray </tr>)
    while rows.last().is_some_and(Vec::is_empty) {
        rows.pop();
    }
    if rows.iter().all(Vec::is_empty) {
        return None;
    }
    // where each cell goes, rowspans taking their place in the rows below
    let nrows = rows.len() as u32;
    let mut taken: HashMap<(u32, u32), ()> = HashMap::new();
    let mut placed: Vec<(u32, u32, RawCell)> = Vec::new();
    let mut ncols = 0u32;
    for (r, row) in rows.into_iter().enumerate() {
        let r = r as u32;
        let mut c = 0u32;
        for cell in row {
            while taken.contains_key(&(r, c)) {
                c += 1;
            }
            let (rs, cs) = ((cell.span.0).min(nrows - r).max(1), cell.span.1);
            if u64::from(nrows) * u64::from(c + cs) > 1_000_000 {
                return None;
            }
            for i in 0..rs {
                for j in 0..cs {
                    taken.insert((r + i, c + j), ());
                }
            }
            ncols = ncols.max(c + cs);
            placed.push((r, c, RawCell { span: (rs, cs), ..cell }));
            c += cs;
        }
    }
    if ncols == 0 {
        return None;
    }
    let mut clip = TableClip::plain(nrows, ncols);
    let mut widths: Vec<Option<f32>> = col_widths.into_iter().take(ncols as usize).collect();
    widths.resize(ncols as usize, None);
    let mut heights: Vec<Option<f32>> = row_looks.iter().map(|l| l.height).collect();
    heights.resize(nrows as usize, None);
    for (r, c, raw) in placed {
        let (rs, cs) = raw.span;
        let l = &raw.look;
        let mut st = TableCellStyle::default();
        if let Some(f) = l.fill {
            st.fill = Fill::Color(f);
        }
        // the cell's text style: what every run of it has (else what the cell says)
        let all = |f: &dyn Fn(&(bool, bool, bool, bool, Option<Color>, Option<f32>, Option<String>)) -> bool| !raw.runs.is_empty() && raw.runs.iter().all(f);
        st.text.bold = if raw.runs.is_empty() { l.bold.unwrap_or(raw.header) } else { all(&|x| x.0) };
        st.text.italic = if raw.runs.is_empty() { l.italic.unwrap_or(false) } else { all(&|x| x.1) };
        st.text.underline = if raw.runs.is_empty() { l.underline.unwrap_or(false) } else { all(&|x| x.2) };
        st.text.strike = if raw.runs.is_empty() { l.strike.unwrap_or(false) } else { all(&|x| x.3) };
        let first = raw.runs.first();
        let same = |f: &dyn Fn(&(bool, bool, bool, bool, Option<Color>, Option<f32>, Option<String>)) -> bool| raw.runs.iter().all(f);
        if let Some(col) = first.and_then(|x| x.4).filter(|c0| same(&|x| x.4 == Some(*c0))).or(if raw.runs.is_empty() { l.color } else { None }) {
            st.text.color = col;
        }
        if let Some(s) = first.and_then(|x| x.5).filter(|s0| same(&|x| x.5 == Some(*s0))).or(if raw.runs.is_empty() { l.size } else { None }) {
            st.text.size = s;
        }
        if let Some(f) = first.and_then(|x| x.6.clone()).filter(|f0| same(&|x| x.6.as_deref() == Some(f0.as_str()))).or(if raw.runs.is_empty() { l.font.clone() } else { None }) {
            st.text.font_name = f;
        }
        if let Some(a) = l.align {
            st.align = a;
        }
        if let Some(v) = l.valign {
            st.valign = v;
        }
        if let Some(w) = l.wrap {
            st.wrap = w;
        }
        let edges: [Option<Stroke>; 4] = l.borders.clone().map(Option::flatten);
        let text = raw.text.split('\n').map(|s| s.trim_matches(|c: char| c.is_whitespace() && c != '\u{a0}').replace('\u{a0}', " ")).collect::<Vec<_>>().join("\n");
        let text = text.trim_matches('\n').to_string();
        if cs == 1 {
            if let (Some(w), Some(slot)) = (l.width, widths.get_mut(c as usize)) {
                slot.get_or_insert(w);
            }
        }
        if rs == 1 {
            if let (Some(hh), Some(slot)) = (l.height, heights.get_mut(r as usize)) {
                slot.get_or_insert(hh);
            }
        }
        if rs > 1 || cs > 1 {
            clip.merges.push((r, c, rs, cs));
        }
        for i in 0..rs {
            for j in 0..cs {
                let Some(cc) = clip.cells.get_mut(((r + i) * ncols + c + j) as usize) else { continue };
                // each cell of a merge keeps its part of the outline
                cc.edges = [
                    if i == 0 { edges[0].clone() } else { None },
                    if i + 1 == rs { edges[1].clone() } else { None },
                    if j == 0 { edges[2].clone() } else { None },
                    if j + 1 == cs { edges[3].clone() } else { None },
                ];
                cc.style = Some(st.clone());
                if (i, j) == (0, 0) {
                    cc.shown = text.clone();
                    cc.outline = (rs > 1 || cs > 1).then(|| edges.clone());
                } else {
                    cc.covered = true;
                }
            }
        }
    }
    if widths.iter().any(Option::is_some) {
        clip.col_widths = widths.into_iter().map(|w| w.unwrap_or(0.0)).collect();
    }
    if heights.iter().any(Option::is_some) {
        clip.row_heights = heights.into_iter().map(|h| h.unwrap_or(0.0)).collect();
    }
    clip.name = caption.map(|c| c.trim().to_string()).unwrap_or_default();
    Some(clip)
}

#[cfg(test)]
mod tests {
    use super::*;

    static DONOR: &[u8] = include_bytes!("../data/donors/R2-table-values.key");

    fn doc() -> (Document, ObjId) {
        let d = Document::from_file(kn_format::KeyFile::from_zip(DONOR).unwrap());
        let t = d.file.objects_of("TST.TableInfoArchive").next().unwrap().0;
        (d, t)
    }

    fn red() -> Color {
        Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
    }

    fn td(d: &Document, t: ObjId) -> TableData {
        table_data(d, t).unwrap()
    }

    /// A table with values, a formula, formats, a fill, bold red text, centred text, a border
    /// and a merge.
    fn plain(d: &mut Document, t: ObjId) {
        let mut h = History::default();
        d.begin(&mut h, "t");
        resize(d, &mut h, t, 4, 3).unwrap();
        for k in [HeaderKind::Rows, HeaderKind::Columns, HeaderKind::Footer] {
            let _ = d.set_table_header_count(&mut h, t, k, 0);
        }
        d.end(&mut h);
    }

    fn styled(d: &mut Document, t: ObjId) {
        plain(d, t);
        let mut h = History::default();
        d.begin(&mut h, "t");
        d.set_cell(&mut h, t, 0, 0, &CellInput::Text("Name".into())).unwrap();
        d.set_cell(&mut h, t, 0, 1, &CellInput::Number(1234.5)).unwrap();
        d.set_cells_format(&mut h, t, (0, 1, 0, 1), &CellFormat::Currency { code: "EUR".into(), decimals: Some(2), thousands: true, negative: 0, accounting: false }).unwrap();
        d.set_cell(&mut h, t, 1, 1, &CellInput::Number(0.25)).unwrap();
        d.set_cells_format(&mut h, t, (1, 1, 1, 1), &CellFormat::Percent { decimals: Some(0), thousands: false, negative: 0 }).unwrap();
        d.set_cell(&mut h, t, 1, 0, &CellInput::Text("007".into())).unwrap();
        d.set_cells_format(&mut h, t, (1, 0, 1, 0), &CellFormat::Text).unwrap();
        d.set_formula_text(&mut h, t, 2, 1, "=B1*2").unwrap();
        d.set_cells_fill(&mut h, t, (0, 0, 0, 0), Some(&FillSpec::Color(red()))).unwrap();
        d.set_cells_text_prop(&mut h, t, (0, 0, 0, 0), &TextProp::Bold(true)).unwrap();
        d.set_cells_text_prop(&mut h, t, (0, 0, 0, 0), &TextProp::Color(Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 })).unwrap();
        d.set_cells_text_prop(&mut h, t, (1, 0, 1, 0), &TextProp::Italic(true)).unwrap();
        d.set_cells_alignment(&mut h, t, (1, 1, 1, 1), CellAlign::Center).unwrap();
        d.set_cells_border(&mut h, t, (0, 1, 0, 1), BorderEdges::Bottom, Some(&StrokeSpec { color: red(), width: 2.0, dash: Dash::Solid })).unwrap();
        d.set_cell(&mut h, t, 3, 0, &CellInput::Text("merged".into())).unwrap();
        d.merge_cells(&mut h, t, CellRange { row: 3, col: 0, rows: 1, cols: 2 }).unwrap();
        d.end(&mut h);
    }

    /// What a cell shows and looks like (text, fill, bold, italic, colour, alignment, edges).
    fn state(d: &Document, t: ObjId, b: (u32, u32, u32, u32)) -> Vec<String> {
        let x = td(d, t);
        let vals = d.cell_values(t);
        let mut out = Vec::new();
        for r in b.0..=b.2 {
            for c in b.1..=b.3 {
                let i = (r * x.cols + c) as usize;
                let s = &x.cell_styles[i];
                out.push(format!(
                    "{}|{:?}|{:?}|{:?}|{}{}|{:?}|{:?}|{:?}|{:?}",
                    x.cells[i],
                    vals.get(&(r, c)),
                    d.cell_format(t, r, c),
                    s.fill,
                    s.text.bold,
                    s.text.italic,
                    s.text.color,
                    s.align,
                    cell_edges(&x, r, c).map(|e| e.map(|e| (e.color, e.width))),
                    x.merge_at(r, c),
                ));
            }
        }
        out
    }

    #[test]
    fn json_and_base64_round_trip() {
        let (mut d, t) = doc();
        styled(&mut d, t);
        let c = read(&d, t, (0, 0, 3, 2), false).unwrap();
        let back = TableClip::from_json(&c.to_json()).unwrap();
        assert_eq!(back, c);
        for s in ["", "a", "ab", "abc", "abcd", "héllo wörld"] {
            assert_eq!(base64_decode(&base64_encode(s.as_bytes())).unwrap(), s.as_bytes());
        }
        let h = c.to_html();
        assert_eq!(TableClip::from_html(&h).unwrap(), c);
        assert!(h.contains("colspan=\"2\""), "{h}");
        assert!(h.contains("font-weight:bold"));
        assert!(h.contains("background-color:#ff0000"));
    }

    /// Cells copied from one table and pasted through the clipboard HTML into another document
    /// end up as they were: values, formats, formula, fill, text style, alignment, borders,
    /// merge.
    #[test]
    fn cells_round_trip_through_html() {
        let (mut a, ta) = doc();
        styled(&mut a, ta);
        let html = read(&a, ta, (0, 0, 3, 1), false).unwrap().to_html();
        let (mut b, tb) = doc();
        plain(&mut b, tb);
        let clip = TableClip::from_html(&html).unwrap();
        let mut h = History::default();
        b.begin(&mut h, "Paste");
        let block = write(&mut b, &mut h, tb, (0, 0, 0, 0), &clip, PasteMode::All).unwrap();
        b.end(&mut h);
        assert_eq!(block, (0, 0, 3, 1));
        let (sa, sb) = (state(&a, ta, (0, 0, 3, 1)), state(&b, tb, (0, 0, 3, 1)));
        for (x, y) in sa.iter().zip(&sb) {
            assert_eq!(x, y);
        }
        assert!(b.formula_text(tb, 2, 1).is_some());
        assert_eq!(b.formula_text(tb, 2, 1), a.formula_text(ta, 2, 1));
        assert_eq!(b.cell_values(tb).get(&(1, 0)), Some(&CellValue::Text("007".into())), "text that looks like a number stays text");
        // one undo step
        assert!(b.undo(&mut h));
        assert_ne!(state(&b, tb, (0, 0, 0, 0))[0], sa[0]);
        assert!(!b.undo(&mut h));
    }

    /// A whole table pasted as a new one: same size, headers, sizes and cells.
    #[test]
    fn whole_table_as_a_new_table() {
        let (mut a, ta) = doc();
        styled(&mut a, ta);
        let mut h = History::default();
        a.begin(&mut h, "t");
        a.set_table_sizes(&mut h, ta, false, &[(0, 150.0)]);
        a.set_table_header_count(&mut h, ta, HeaderKind::Rows, 1).unwrap();
        a.end(&mut h);
        let clip = TableClip::from_html(&read(&a, ta, (0, 0, 0, 0), true).unwrap().to_html()).unwrap();
        assert!(clip.whole);
        let (mut b, tb) = doc();
        let mut h = History::default();
        b.begin(&mut h, "Paste");
        write_new(&mut b, &mut h, tb, &clip).unwrap();
        b.end(&mut h);
        let (x, y) = (td(&a, ta), td(&b, tb));
        assert_eq!((y.rows, y.cols, y.header_rows), (x.rows, x.cols, x.header_rows));
        assert!((y.col_widths[0] - 150.0).abs() < 0.5, "{:?}", y.col_widths);
        assert_eq!(state(&a, ta, (0, 0, 3, 2)), state(&b, tb, (0, 0, 3, 2)));
    }

    /// LibreOffice Calc's HTML: values, bold, colours, fills, alignment, a merged cell, widths.
    #[test]
    fn generic_html_table() {
        let html = r##"<html><head><meta http-equiv="content-type" content="text/html; charset=utf-8"/><style type="text/css">td { font-family:"Liberation Sans"; }</style></head><body>
<table cellspacing="0" border="0">
	<colgroup width="120"></colgroup>
	<colgroup width="85"></colgroup>
	<tr>
		<td height="17" align="left" bgcolor="#FFFF00"><b><font color="#C9211E">Item</font></b></td>
		<td align="right" sdval="12.5" sdnum="1033;">12.5</td>
	</tr>
	<tr>
		<td colspan=2 height="17" align="center" style="border-top: 1px solid #000000; border-bottom: 1px solid #000000">Total &amp; more</td>
		</tr>
	<tr>
		<td>a<br>b</td>
		<td><i>x</i> y</td>
	</tr>
</table></body></html>"##;
        let c = TableClip::from_html(html).unwrap();
        assert_eq!((c.rows, c.cols), (3, 2));
        let a = c.cell(0, 0).unwrap();
        assert_eq!(a.shown, "Item");
        let st = a.style.as_ref().unwrap();
        assert!(st.text.bold);
        assert_eq!(st.fill, Fill::Color(Color { r: 1.0, g: 1.0, b: 0.0, a: 1.0 }));
        assert_eq!(hex(&st.text.color), "#c9211e");
        assert_eq!(c.cell(0, 1).unwrap().shown, "12.5");
        assert_eq!(c.cell(0, 1).unwrap().style.as_ref().unwrap().align, Align::Right);
        assert_eq!(c.merges, vec![(1, 0, 1, 2)]);
        let m = c.cell(1, 0).unwrap();
        assert_eq!(m.shown, "Total & more");
        assert!(m.edges[0].is_some() && m.edges[1].is_some() && m.edges[2].is_none());
        assert!(c.cell(1, 1).unwrap().covered);
        assert_eq!(c.cell(2, 0).unwrap().shown, "a\nb");
        let y = c.cell(2, 1).unwrap();
        assert_eq!(y.shown, "x y");
        assert!(!y.style.as_ref().unwrap().text.italic, "only part of it is italic");
        assert_eq!(c.tsv(), "Item\t12.5\nTotal & more\t\n\"a\nb\"\tx y");
        // pasted into a table: the destination keeps its own font and size, the cell its fill
        let (mut d, t) = doc();
        let before = td(&d, t).cell_styles[0].text.size;
        let mut h = History::default();
        d.begin(&mut h, "Paste");
        write(&mut d, &mut h, t, (0, 0, 0, 0), &c, PasteMode::All).unwrap();
        d.end(&mut h);
        let x = td(&d, t);
        assert_eq!(x.cells[0], "Item");
        assert!(x.cell_styles[0].text.bold);
        assert_eq!(x.cell_styles[0].text.size, before);
        assert!(matches!(x.cell_styles[0].fill, Fill::Color(_)));
        assert!(x.merge_at(1, 0).is_some_and(|m| m.cols == 2));
        assert_eq!(d.cell_values(t).get(&(0, 1)), Some(&CellValue::Number(12.5)));
    }

    #[test]
    fn web_table_without_rows_markup() {
        // a table copied from a browser: thead / th, entities, nested inline styles
        let c = TableClip::from_html("<table><thead><tr><th>A</th><th style=\"text-align:right\">B&nbsp;C</th></tr></thead><tbody><tr><td><span style=\"color:rgb(0, 128, 0); font-weight:700\">ok</span></td><td>2</td></tr></tbody></table>").unwrap();
        assert_eq!((c.rows, c.cols), (2, 2));
        assert!(c.cell(0, 0).unwrap().style.as_ref().unwrap().text.bold, "th is bold");
        assert_eq!(c.cell(0, 1).unwrap().shown, "B C");
        let ok = c.cell(1, 0).unwrap().style.clone().unwrap();
        assert!(ok.text.bold);
        assert_eq!(hex(&ok.text.color), "#008000");
        assert!(TableClip::from_html("<p>no table</p>").is_none());
    }
}
