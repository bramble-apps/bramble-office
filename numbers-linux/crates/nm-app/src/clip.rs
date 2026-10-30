//! Copying and pasting table cells as Numbers does: a copy keeps each cell's value, formula,
//! data format and style; Edit > Paste puts all of it back (formulas shifted to their new
//! place, $-references kept), Paste and Match Style only the values and formulas (the
//! destination keeps its look), Paste Formula Results the shown values. The system clipboard
//! gets the cells as tab-separated text too, for other apps; text copied elsewhere pastes as
//! values.

use nm_core::kn_model::format::TextProp;
use nm_core::kn_model::tablecells::{CellAlign, CellInput};
use nm_core::kn_model::tablefmt::CellFormat;
use nm_core::kn_model::{Align, TableCellStyle};

use crate::app::{App, Sel};
use crate::canvas::commit;

#[derive(Clone, Debug)]
pub struct CellCopy {
    pub formula: Option<String>,
    pub shown: String,
    pub format: CellFormat,
    pub style: Option<TableCellStyle>,
    /// A cell a merge covers (not its first): Numbers pastes the merge as its first cell
    /// only and leaves the destination of the covered ones untouched (Mac N18 P11).
    pub covered: bool,
}

/// Copied cells: a grid of rows (row offset, column offset from the top-left).
#[derive(Clone, Debug)]
pub struct CellClip {
    pub rows: u32,
    pub cols: u32,
    pub cells: Vec<CellCopy>,
    /// What went on the system clipboard (to know a paste is of these cells).
    pub tsv: String,
    /// Where the copy's top-left was (shown row, column): formulas shift by the distance.
    pub origin: (u32, u32),
    /// The document copied from (custom formats only mean something there).
    pub doc: String,
    /// Merged cells inside the copy: (row, column, rows, columns) from its top-left.
    pub merges: Vec<(u32, u32, u32, u32)>,
    /// Whole rows (true) or whole columns (false) were copied: Insert > Copied Rows / Columns.
    pub whole: Option<bool>,
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

/// Copy the selected cells (merges taken in whole). Returns the tab-separated text.
pub fn copy(app: &mut App) -> Option<String> {
    let sel = app.sel?;
    let t = sel.table;
    let (r0, c0, r1, c1) = app.with_merges(t, sel.block());
    let mut cells = Vec::new();
    let mut lines = Vec::new();
    for r in r0..=r1 {
        let mut line = Vec::new();
        for c in c0..=c1 {
            let (shown, style) = app.table(t).map(|(_, d)| {
                let i = (r * d.cols + c) as usize;
                (d.cells.get(i).cloned().unwrap_or_default(), d.cell_styles.get(i).cloned())
            })?;
            let mr = app.mrow(t, r);
            let formula = mr.and_then(|mr| app.formula_of(t, mr, c));
            let format = mr.and_then(|mr| app.doc().map(|d| d.cell_format(t, mr, c))).unwrap_or(CellFormat::Automatic);
            // a cell of several lines goes to other apps quoted, as spreadsheets exchange it
            line.push(if shown.contains(['\n', '\t', '"']) { format!("\"{}\"", shown.replace('"', "\"\"")) } else { shown.clone() });
            let covered = app.table(t).and_then(|(_, d)| d.merge_at(r, c)).is_some_and(|m| (m.row, m.col) != (r, c));
            cells.push(CellCopy { formula, shown, format, style, covered });
        }
        lines.push(line.join("\t"));
    }
    let tsv = lines.join("\n");
    let merges = app
        .table(t)
        .map(|(_, d)| d.merges.iter().filter(|m| m.row >= r0 && m.row + m.rows - 1 <= r1 && m.col >= c0 && m.col + m.cols - 1 <= c1).map(|m| (m.row - r0, m.col - c0, m.rows, m.cols)).collect())
        .unwrap_or_default();
    let (rows, cols) = app.table(t).map(|(_, d)| (d.rows, d.cols)).unwrap_or((0, 0));
    let whole = if c0 == 0 && c1 + 1 == cols { Some(true) } else if r0 == 0 && r1 + 1 == rows { Some(false) } else { None };
    app.cell_clip = Some(CellClip { rows: r1 - r0 + 1, cols: c1 - c0 + 1, cells, tsv: tsv.clone(), origin: (r0, c0), doc: app.doc().map(nm_core::kn_model::insert::doc_identity).unwrap_or_default(), merges, whole });
    app.obj_clip_active = false;
    Some(tsv)
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

fn cell_align(a: Align) -> CellAlign {
    match a {
        Align::Center => CellAlign::Center,
        Align::Right => CellAlign::Right,
        Align::Justify => CellAlign::Justify,
        _ => CellAlign::Left,
    }
}

/// Paste the copied cells at the selection, as Numbers 15.4 does (Mac N18): from one selected
/// cell the whole copy goes in (the table grows to take it); a selected range is filled by
/// repeating the copy, cut off at its edges. A copied merge pastes as plain cells; merges in
/// the way of a full paste are undone, while one copied cell fills a selected merge.
pub fn paste(app: &mut App, mode: PasteMode) -> bool {
    let (Some(clip), Some(sel)) = (app.cell_clip.clone(), app.sel) else { return false };
    commit(app);
    let t = sel.table;
    let (r0, c0, r1, c1) = app.with_merges(t, sel.block());
    let (h, w) = (r1 - r0 + 1, c1 - c0 + 1);
    // one merged cell selected is one cell
    let one_merge = app.with_merges(t, (sel.cursor.0, sel.cursor.1, sel.cursor.0, sel.cursor.1)) == (r0, c0, r1, c1);
    let single = (h == 1 && w == 1) || one_merge;
    let (th, tw) = if single { (clip.rows, clip.cols) } else { (h, w) };
    let into_merge = one_merge && clip.rows == 1 && clip.cols == 1 && (h > 1 || w > 1);
    // merges inside a copied range are made again where they land; a lone merged cell pastes
    // as one ordinary cell unless the selection already spans its size (Numbers, Mac N20)
    let lone = clip.merges.len() == 1 && clip.merges[0] == (0, 0, clip.rows, clip.cols);
    let remerge = mode == PasteMode::All && !clip.merges.is_empty() && !app.categorized(t) && (!lone || (h >= clip.rows && w >= clip.cols));
    // from one cell the table grows to take the whole copy (not while categories show)
    let Some((rows0, cols0)) = app.table(t).map(|(_, d)| (d.rows, d.cols)) else { return false };
    let (more_r, more_c) = if single && !app.categorized(t) { ((r0 + th).saturating_sub(rows0), (c0 + tw).saturating_sub(cols0)) } else { (0, 0) };
    let label = match mode {
        PasteMode::All => "Paste",
        PasteMode::MatchStyle => "Paste and Match Style",
        PasteMode::Results => "Paste Formula Results",
    };
    if more_r > 0 || more_c > 0 {
        let res = app.edit(label, |d, hist| -> Result<(), nm_core::kn_format::Error> {
            use nm_core::kn_model::tablerows::Axis;
            for k in 0..more_r {
                d.table_insert(hist, t, Axis::Row, rows0 + k, rows0 + k - 1)?;
            }
            for k in 0..more_c {
                d.table_insert(hist, t, Axis::Column, cols0 + k, cols0 + k - 1)?;
            }
            Ok(())
        });
        if let Some(Err(e)) = res {
            app.notify(format!("Couldn't make room for the paste: {e}"));
        }
        app.refresh();
    }
    let Some((rows, cols)) = app.table(t).map(|(_, d)| (d.rows, d.cols)) else { return false };
    let dest_merges: Vec<nm_core::kn_model::CellRange> = app.table(t).map(|(_, d)| d.merges.iter().filter(|m| m.row <= r0 + th - 1 && m.row + m.rows > r0 && m.col <= c0 + tw - 1 && m.col + m.cols > c0).copied().collect()).unwrap_or_default();
    let inside_dest_merge = |r: u32, c: u32| dest_merges.iter().any(|m| m.contains(r, c) && (m.row, m.col) != (r, c));
    // what to write where (model rows)
    let mut jobs: Vec<(u32, u32, CellCopy, i64, i64)> = Vec::new();
    for i in 0..th {
        for j in 0..tw {
            let (r, c) = (r0 + i, c0 + j);
            if r >= rows || c >= cols {
                continue;
            }
            if into_merge && inside_dest_merge(r, c) {
                continue;
            }
            let Some(mr) = app.mrow(t, r) else { continue };
            let k = ((i % clip.rows) * clip.cols + (j % clip.cols)) as usize;
            let Some(cc) = clip.cells.get(k).cloned() else { continue };
            if cc.covered && !remerge {
                continue;
            }
            jobs.push((mr, c, cc, i64::from(r) - i64::from(i % clip.rows), i64::from(c) - i64::from(j % clip.cols)));
        }
    }
    let unmerge: Vec<(u32, u32)> = if !app.categorized(t) && !into_merge { dest_merges.iter().map(|m| (m.row, m.col)).collect() } else { Vec::new() };
    let origin = clip.origin;
    // where the copy's merges go (repeated with the tiles, inside the table)
    let new_merges: Vec<nm_core::kn_model::CellRange> = if remerge {
        let mut v = Vec::new();
        for ti in 0..th.div_ceil(clip.rows) {
            for tj in 0..tw.div_ceil(clip.cols) {
                for &(mr, mc, nr, nc) in &clip.merges {
                    let (r, c) = (r0 + ti * clip.rows + mr, c0 + tj * clip.cols + mc);
                    if r + nr <= (r0 + th).min(rows) && c + nc <= (c0 + tw).min(cols) {
                        v.push(nm_core::kn_model::CellRange { row: r, col: c, rows: nr, cols: nc });
                    }
                }
            }
        }
        v
    } else {
        Vec::new()
    };
    // a custom format pasted into another document: its pattern as a plain date format
    let same_doc = app.doc().map(nm_core::kn_model::insert::doc_identity).is_some_and(|d| d == clip.doc);
    let portable = |f: &CellFormat| -> CellFormat {
        match f {
            CellFormat::Custom { format } if !same_doc => match (format.u32("format_type"), format.str("custom_format_string")) {
                (Some(271), Some(p)) => CellFormat::DateTime { pattern: p.trim().to_string() },
                _ => CellFormat::Automatic,
            },
            f => f.clone(),
        }
    };
    let res = app.edit(label, |d, hist| -> Result<(), nm_core::kn_format::Error> {
        for &(r, c) in &unmerge {
            d.unmerge_cells(hist, t, r, c);
        }
        for (r, c, cc, top_r, left_c) in &jobs {
            // a cell a re-made merge covers: emptied (the merge takes its first cell's content)
            if cc.covered {
                d.set_cell(hist, t, *r, *c, &CellInput::Empty)?;
                continue;
            }
            // values and formulas (Paste Formula Results: what the formulas show)
            match (&cc.formula, mode) {
                (Some(f), PasteMode::All | PasteMode::MatchStyle) => {
                    let (dr, dc) = (*top_r - i64::from(origin.0), *left_c - i64::from(origin.1));
                    d.set_formula_text(hist, t, *r, *c, &shift_formula(f, dr, dc))?;
                }
                _ if cc.shown.is_empty() => d.set_cell(hist, t, *r, *c, &CellInput::Empty)?,
                _ => d.set_cell(hist, t, *r, *c, &CellInput::parse(&cc.shown))?,
            }
            let b = (*r, *c, *r, *c);
            // the number format comes along in every kind of paste (when the copy has one)
            let f = portable(&cc.format);
            if f != CellFormat::Automatic {
                d.set_cells_format(hist, t, b, &f)?;
            }
            // text style and fill: plain Paste only
            if mode != PasteMode::All {
                continue;
            }
            if let Some(st) = &cc.style {
                let fill = crate::editing::fill_spec(&st.fill);
                d.set_cells_fill(hist, t, b, Some(&fill))?;
                let tx = &st.text;
                for p in [TextProp::Bold(tx.bold), TextProp::Italic(tx.italic), TextProp::Underline(tx.underline), TextProp::Strike(tx.strike), TextProp::Size(tx.size), TextProp::Color(tx.color), TextProp::Font(tx.font_name.clone())] {
                    d.set_cells_text_prop(hist, t, b, &p)?;
                }
                d.set_cells_alignment(hist, t, b, cell_align(st.align))?;
                // vertical alignment and wrap are the cell's own (not its text style's)
                d.set_cells_valign(hist, t, b, st.valign)?;
                d.set_cells_wrap(hist, t, b, st.wrap)?;
            }
        }
        for m in &new_merges {
            d.merge_cells(hist, t, *m)?;
        }
        Ok(())
    });
    match res {
        Some(Err(e)) => app.notify(format!("Couldn't paste every cell: {e}")),
        _ => {
            let (er, ec) = ((r0 + th - 1).min(rows - 1), (c0 + tw - 1).min(cols - 1));
            app.sel = Some(Sel { table: t, anchor: (r0, c0), cursor: (er, ec) });
        }
    }
    true
}

fn table_model(d: &nm_core::kn_model::Document, t: nm_core::kn_format::ObjId) -> Option<nm_core::kn_format::ObjId> {
    d.file.obj(t).and_then(|m| m.ref_id("tableModel"))
}

fn table_size(d: &nm_core::kn_model::Document, t: nm_core::kn_format::ObjId) -> (u32, u32) {
    table_model(d, t).and_then(|m| d.file.obj(m)).map(|m| (m.u32("number_of_rows").unwrap_or(0), m.u32("number_of_columns").unwrap_or(0))).unwrap_or((0, 0))
}

fn set_table_name(d: &mut nm_core::kn_model::Document, h: &mut nm_core::kn_model::edit::History, t: nm_core::kn_format::ObjId, name: &str) {
    let Some(m) = table_model(d, t) else { return };
    d.touch(h, m);
    if let Some(o) = d.file.obj_mut(m) {
        o.set_str("table_name", name);
    }
}

/// The copy holds formulas (Paste Formula Results is offered only then, Mac N18).
pub fn has_formulas(app: &App) -> bool {
    app.cell_clip.as_ref().is_some_and(|c| c.cells.iter().any(|x| x.formula.is_some()))
}

/// Cut: the cells are emptied, their style too (Numbers clears both at cut time).
pub fn clear_style(app: &mut App) {
    let Some(sel) = app.sel else { return };
    let t = sel.table;
    let blocks = app.model_blocks(t, sel.block());
    app.edit("Cut", |d, h| -> Result<(), nm_core::kn_format::Error> {
        for b in &blocks {
            d.set_cells_fill(h, t, *b, Some(&nm_core::kn_model::format::FillSpec::None))?;
            d.set_cells_format(h, t, *b, &CellFormat::Automatic)?;
            for p in [TextProp::Bold(false), TextProp::Italic(false), TextProp::Underline(false), TextProp::Strike(false)] {
                d.set_cells_text_prop(h, t, *b, &p)?;
            }
        }
        Ok(())
    });
}

/// Copied cells pasted on the empty canvas: a new table of just their size, with no headers,
/// named after the sheet's first ("Table 1-1"), at (2, 550) — Numbers 15.4 (Mac N18).
pub fn paste_new_table(app: &mut App) {
    let Some(clip) = app.cell_clip.clone() else { return };
    let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
    let Ok((donor, dinfo)) = nm_core::sheets::table_style_donor(2) else { return };
    let name = format!("{}-1", app.drawables.iter().find_map(|d| match &d.kind { nm_core::kn_model::DrawableKind::Table(t) => Some(t.name.clone()), _ => None }).unwrap_or_else(|| "Table 1".into()));
    let made = app.edit("Paste", |d, h| -> Result<nm_core::kn_format::ObjId, nm_core::kn_format::Error> {
        use nm_core::kn_model::tablerows::{Axis, HeaderKind};
        let t = nm_core::sheets::insert_table(d, h, sheet, &donor, dinfo, 2.0, 550.0)?;
        for k in [HeaderKind::Rows, HeaderKind::Columns, HeaderKind::Footer] {
            d.set_table_header_count(h, t, k, 0)?;
        }
        // the copy's size
        let (rows, cols) = table_size(d, t);
        for _ in clip.rows..rows {
            { let n = table_size(d, t).0; d.table_delete(h, t, Axis::Row, n - 1)?; }
        }
        for k in rows..clip.rows {
            d.table_insert(h, t, Axis::Row, k, k - 1)?;
        }
        for _ in clip.cols..cols {
            { let n = table_size(d, t).1; d.table_delete(h, t, Axis::Column, n - 1)?; }
        }
        for k in cols..clip.cols {
            d.table_insert(h, t, Axis::Column, k, k - 1)?;
        }
        set_table_name(d, h, t, &name);
        Ok(t)
    });
    match made {
        Some(Ok(t)) => {
            app.canvas.obj = None;
            app.sel = Some(Sel::cell(t, 0, 0));
            app.refresh();
            paste(app, PasteMode::All);
        }
        Some(Err(e)) => app.notify(format!("Couldn't paste: {e}")),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_transpose_references() {
        assert_eq!(transpose_formula("=B3+$A$1*C2"), "=C2+$A$1*B3");
        assert_eq!(transpose_formula("=SUM(A1:C1)&\"B2\""), "=SUM(A1:A3)&\"B2\"");
    }

    #[test]
    fn formulas_shift_relative_references() {
        assert_eq!(shift_formula("=C2*2", 2, 1), "=D4*2");
        assert_eq!(shift_formula("=$C$2+C2", 1, 1), "=$C$2+D3");
        assert_eq!(shift_formula("=SUM(A1:B3)", 1, 0), "=SUM(A2:B4)");
        assert_eq!(shift_formula("=LOG10(A1)&\"B2\"", 1, 0), "=LOG10(A2)&\"B2\"");
        assert_eq!(shift_formula("=A1", -1, 0), "=#REF!");
        assert_eq!(shift_formula("=$A1+A$1", 1, 1), "=$A2+B$1");
    }
}

/// Insert > Copied Rows / Copied Columns: the copied rows go in above the selection (columns
/// before it) and are pasted there, as one "Paste" step (Mac N23 C6, C7).
pub fn insert_copied(app: &mut App, rows: bool) {
    let (Some(clip), Some(sel)) = (app.cell_clip.clone(), app.sel) else { return };
    if clip.whole != Some(rows) || app.categorized(sel.table) {
        return;
    }
    commit(app);
    let t = sel.table;
    let (r0, c0, _, _) = sel.block();
    let n = if rows { clip.rows } else { clip.cols };
    let at = if rows { r0 } else { c0 };
    let res = app.edit("Paste", |d, h| -> Result<(), nm_core::kn_format::Error> {
        use nm_core::kn_model::tablerows::Axis;
        let axis = if rows { Axis::Row } else { Axis::Column };
        for k in 0..n {
            d.table_insert(h, t, axis, at + k, at + n)?;
        }
        Ok(())
    });
    if let Some(Err(e)) = res {
        app.notify(format!("Couldn't insert the copied {}: {e}", if rows { "rows" } else { "columns" }));
        return;
    }
    let Some((trows, tcols)) = app.table(t).map(|(_, d)| (d.rows, d.cols)) else { return };
    app.sel = Some(if rows { crate::app::Sel { table: t, anchor: (at, 0), cursor: (at + n - 1, tcols - 1) } } else { crate::app::Sel { table: t, anchor: (0, at), cursor: (trows - 1, at + n - 1) } });
    app.merge_next_edit = true;
    paste(app, PasteMode::All);
    app.merge_next_edit = false;
}

/// Table > Add Row / Column: the new line takes the look of the one it was added next to
/// (`like`, before the insert), fill, text style and borders, as Numbers does; cells a merge
/// grows over are left to the merge.
pub fn insert_like(d: &mut nm_core::kn_model::Document, h: &mut nm_core::kn_model::edit::History, t: nm_core::kn_format::ObjId, axis: nm_core::kn_model::tablerows::Axis, at: u32, like: u32) -> Result<(), nm_core::kn_format::Error> {
    use nm_core::kn_model::format::{Dash, StrokeSpec};
    use nm_core::kn_model::tablefmt::BorderEdges;
    use nm_core::kn_model::tablerows::Axis;
    use nm_core::kn_model::{Stroke, TableData};
    let data = |d: &nm_core::kn_model::Document| d.file.obj(t).and_then(|m| nm_core::kn_model::table::table(&d.file, m));
    let before = data(d);
    d.table_insert(h, t, axis, at, like)?;
    let (Some(old), Some(new)) = (before, data(d)) else { return Ok(()) };
    let col = axis == Axis::Column;
    // (line, cross) -> (row, column); along strokes sit between lines, across ones between cross cells
    let rc = |line: u32, cross: u32| if col { (cross, line) } else { (line, cross) };
    let along = |x: &TableData, b: u32, cross: u32| -> Option<Stroke> {
        if col { x.v_strokes.get((cross * (x.cols + 1) + b) as usize) } else { x.h_strokes.get((b * x.cols + cross) as usize) }.cloned().flatten()
    };
    let across = |x: &TableData, line: u32, cb: u32| -> Option<Stroke> {
        if col { x.h_strokes.get((cb * x.cols + line) as usize) } else { x.v_strokes.get((line * (x.cols + 1) + cb) as usize) }.cloned().flatten()
    };
    let same_merge = |a: (u32, u32), b: (u32, u32)| new.merge_at(a.0, a.1).is_some_and(|m| m.contains(b.0, b.1));
    let spec = |s: &Stroke| StrokeSpec { color: s.color, width: s.width, dash: if s.dash.is_empty() { Dash::Solid } else if s.dash[0] < 0.5 { Dash::Dotted } else { Dash::Dashed } };
    let (ocols, cross_n) = (old.cols, if col { new.rows } else { new.cols });
    for cross in 0..cross_n {
        let (r, c) = rc(at, cross);
        let block = (r, c, r, c);
        let merged = new.merge_at(r, c).is_some_and(|m| m.rows > 1 || m.cols > 1);
        // fill and text
        let (lr, lc) = rc(like, cross);
        if let (false, Some(st)) = (merged, old.cell_styles.get((lr * ocols + lc) as usize)) {
            if st.fill != nm_core::kn_model::Fill::None {
                d.set_cells_fill(h, t, block, Some(&crate::editing::fill_spec(&st.fill)))?;
            }
            let tx = &st.text;
            for p in [TextProp::Bold(tx.bold), TextProp::Italic(tx.italic), TextProp::Size(tx.size), TextProp::Color(tx.color), TextProp::Font(tx.font_name.clone())] {
                // (an empty cell has no text style to change yet: only cells with a value take it)
                let _ = d.set_cells_text_prop(h, t, block, &p);
            }
            let _ = d.set_cells_alignment(h, t, block, cell_align(st.align));
            let _ = d.set_cells_valign(h, t, block, st.valign);
            let _ = d.set_cells_wrap(h, t, block, st.wrap);
        }
        // the strokes on both sides of the new cell (where none is there and no merge spans them)
        for (b, from, edge) in [(at, like, if col { BorderEdges::Left } else { BorderEdges::Top }), (at + 1, like + 1, if col { BorderEdges::Right } else { BorderEdges::Bottom })] {
            let other = if b == at { at.checked_sub(1) } else { Some(at + 1) };
            let inside = other.is_some_and(|o| same_merge(rc(at, cross), rc(o, cross)));
            if let (false, None, Some(s)) = (inside, along(&new, b, cross), along(&old, from, cross)) {
                d.set_cells_border(h, t, block, edge, Some(&spec(&s)))?;
            }
        }
        // the stroke above (left of) the new cell, and below the last one
        let mut edges = vec![(cross, if col { BorderEdges::Top } else { BorderEdges::Left })];
        if cross + 1 == cross_n {
            edges.push((cross + 1, if col { BorderEdges::Bottom } else { BorderEdges::Right }));
        }
        for (cb, edge) in edges {
            let inside = cb > 0 && cb < cross_n && same_merge(rc(at, cb - 1), rc(at, cb));
            if let (false, None, Some(s)) = (inside, across(&new, at, cb), across(&old, like, cb)) {
                d.set_cells_border(h, t, block, edge, Some(&spec(&s)))?;
            }
        }
    }
    Ok(())
}

/// A formula's cell references with rows and columns swapped (Table > Transpose Rows and
/// Columns): B3 -> C2, $A$1 stays, references to other tables (Table::B3) too.
pub fn transpose_formula(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_str = false;
    let col_num = |s: &str| s.chars().fold(0u32, |a, ch| a * 26 + (ch as u32 - 'A' as u32 + 1));
    while i < b.len() {
        let ch = b[i];
        if ch == '"' {
            in_str = !in_str;
        }
        let prev_ok = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == '_');
        if !in_str && prev_ok && (ch == '$' || ch.is_ascii_uppercase()) {
            // [$]LETTERS[$]DIGITS
            let mut j = i;
            let abs_c = b.get(j) == Some(&'$');
            if abs_c {
                j += 1;
            }
            let c0 = j;
            while j < b.len() && b[j].is_ascii_uppercase() {
                j += 1;
            }
            let letters: String = b[c0..j].iter().collect();
            let abs_r = b.get(j) == Some(&'$');
            let k = if abs_r { j + 1 } else { j };
            let mut e = k;
            while e < b.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            let ends = e == b.len() || !(b[e].is_ascii_alphanumeric() || b[e] == '(' || b[e] == '_');
            if !letters.is_empty() && letters.len() <= 3 && e > k && ends {
                let digits: String = b[k..e].iter().collect();
                let (c, r) = (col_num(&letters), digits.parse::<u32>().unwrap_or(1));
                // the column becomes the row and the row the column
                out.push_str(&format!("{}{}{}{}", if abs_r { "$" } else { "" }, crate::canvas::col_name(r - 1), if abs_c { "$" } else { "" }, c));
                i = e;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Table > Transpose Rows and Columns: rows become columns (values, formulas with their
/// references swapped, number formats, fills, text styles, header rows / columns), as Numbers.
pub fn transpose_table(app: &mut App, t: nm_core::kn_format::ObjId) {
    use nm_core::kn_model::table::CellValue;
    use nm_core::kn_model::tablerows::{Axis, HeaderKind};
    commit(app);
    let Some(td) = app.wb.as_ref().and_then(|w| w.table_data(t)) else { return };
    if !td.merges.is_empty() {
        return app.notify("Unmerge the table's cells to transpose it.");
    }
    let (rows, cols) = (td.rows, td.cols);
    let mut formulas = std::collections::HashMap::new();
    for r in 0..rows {
        for c in 0..cols {
            if let Some(f) = app.formula_of(t, r, c) {
                formulas.insert((r, c), f);
            }
        }
    }
    let Some(w) = app.wb.as_ref() else { return };
    let vals = w.doc.cell_values(t);
    let mut cells = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let fmt = w.doc.cell_format(t, r, c);
            let st = td.cell_styles.get((r * cols + c) as usize).cloned();
            cells.push((r, c, vals.get(&(r, c)).cloned(), formulas.remove(&(r, c)), fmt, st));
        }
    }
    let (hr, hc) = (td.header_rows, td.header_cols);
    let res = app.edit("Transpose", |d, h| -> Result<(), nm_core::kn_format::Error> {
        // the new shape: cols rows by rows columns
        let mut nr = rows;
        while nr < cols {
            d.table_insert(h, t, Axis::Row, nr, nr - 1)?;
            nr += 1;
        }
        let mut nc = cols;
        while nc < rows {
            d.table_insert(h, t, Axis::Column, nc, nc - 1)?;
            nc += 1;
        }
        d.set_table_header_count(h, t, HeaderKind::Rows, hc.min(cols))?;
        d.set_table_header_count(h, t, HeaderKind::Columns, hr.min(rows))?;
        let all = (0, 0, nr.max(cols) - 1, nc.max(rows) - 1);
        d.set_cells_fill(h, t, all, Some(&nm_core::kn_model::format::FillSpec::None))?;
        for r in 0..nr {
            for c in 0..nc {
                d.set_cell(h, t, r, c, &CellInput::Empty)?;
            }
        }
        for (r, c, v, f, fmt, st) in &cells {
            let (r2, c2) = (*c, *r);
            match (f, v) {
                (Some(f), _) => d.set_formula_text(h, t, r2, c2, &transpose_formula(f))?,
                (None, Some(CellValue::Number(x))) => d.set_cell(h, t, r2, c2, &CellInput::Number(*x))?,
                (None, Some(CellValue::Date(x))) => d.set_cell(h, t, r2, c2, &CellInput::Date(*x))?,
                (None, Some(CellValue::Duration(x))) => d.set_cell(h, t, r2, c2, &CellInput::Number(*x))?,
                (None, Some(CellValue::Bool(b))) => d.set_cell(h, t, r2, c2, &CellInput::Text(if *b { "TRUE".into() } else { "FALSE".into() }))?,
                (None, Some(CellValue::Text(s))) => d.set_cell(h, t, r2, c2, &CellInput::Text(s.clone()))?,
                _ => {}
            }
            let b = (r2, c2, r2, c2);
            if *fmt != CellFormat::Automatic {
                let _ = d.set_cells_format(h, t, b, fmt);
            }
            if let Some(st) = st {
                if st.fill != nm_core::kn_model::Fill::None {
                    d.set_cells_fill(h, t, b, Some(&crate::editing::fill_spec(&st.fill)))?;
                }
                if !matches!(v, None | Some(CellValue::Empty)) || f.is_some() {
                    let tx = &st.text;
                    for p in [TextProp::Bold(tx.bold), TextProp::Italic(tx.italic), TextProp::Size(tx.size), TextProp::Color(tx.color), TextProp::Font(tx.font_name.clone())] {
                        let _ = d.set_cells_text_prop(h, t, b, &p);
                    }
                    let _ = d.set_cells_alignment(h, t, b, cell_align(st.align));
                }
            }
        }
        // extra rows / columns left over
        for _ in cols..nr {
            d.table_delete(h, t, Axis::Row, cols)?;
        }
        for _ in rows..nc {
            d.table_delete(h, t, Axis::Column, rows)?;
        }
        Ok(())
    });
    if let Some(Err(e)) = res {
        app.notify(format!("Couldn't transpose the table: {e}"));
    }
}
