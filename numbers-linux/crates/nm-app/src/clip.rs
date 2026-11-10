//! Copying and pasting table cells as Numbers does: a copy keeps each cell's value, formula,
//! data format and style; Edit > Paste puts all of it back (formulas shifted to their new
//! place, $-references kept), Paste and Match Style only the values and formulas (the
//! destination keeps its look), Paste Formula Results the shown values. The system clipboard
//! gets the cells as tab-separated text too, for other apps; text copied elsewhere pastes as
//! values.

use nm_core::kn_model::format::TextProp;
use nm_core::kn_model::tablecells::CellInput;
use nm_core::kn_model::tablefmt::CellFormat;
use nm_core::kn_model::tableclip::TableClip;
use nm_core::kn_model::{Stroke, TableCellStyle};

// (the cell copy and the writing of a pasted cell are the suite's, shared with Keydeck and Letters)
pub use nm_core::kn_model::tableclip::{cell_align, cell_edges, grid_lines, region, region_looks, write_cell, CellCopy, Job, PasteMode};
#[cfg(test)]
use nm_core::kn_model::tableclip::shift_formula;

use crate::app::{App, Sel};
use crate::canvas::commit;

/// The system clipboard's text is the cells Values copied (line ends as other apps may change
/// them: CR LF, or the last line break dropped).
pub fn is_ours(app: &App, text: &str) -> bool {
    let norm = |t: &str| t.replace("\r\n", "\n").trim_end_matches(['\n', '\r']).to_string();
    app.cell_clip.as_ref().is_some_and(|c| norm(&c.tsv) == norm(text))
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
    /// The source table's region looks and grid lines (see [`region_looks`], [`grid_lines`]).
    pub looks: [Option<TableCellStyle>; 4],
    pub grid: (Option<Stroke>, Option<Stroke>),
    /// A whole table pasted from another app: its column widths and row heights (points, 0
    /// unknown), name and header counts (rows, columns, footer), for a new table made of it.
    pub sizes: (Vec<f32>, Vec<f32>),
    pub name: String,
    pub headers: Option<(u32, u32, u32)>,
}

/// The copy as the suite's clipboard table (the HTML other apps read).
pub fn table_clip(clip: &CellClip) -> TableClip {
    TableClip {
        rows: clip.rows,
        cols: clip.cols,
        cells: clip.cells.clone(),
        merges: clip.merges.clone(),
        origin: clip.origin,
        looks: clip.looks.clone(),
        grid: clip.grid.clone(),
        col_widths: clip.sizes.0.clone(),
        row_heights: clip.sizes.1.clone(),
        name: clip.name.clone(),
        whole: clip.headers.is_some(),
        headers: clip.headers.unwrap_or_default(),
        source: nm_core::kn_model::tableclip::process_tag().to_string(),
        doc: nm_core::kn_model::tableclip::identity_key(&clip.doc),
    }
}

/// Cells from another app's clipboard table as Values' own copy (`tsv`: the clipboard's text).
pub fn cell_clip_of(tc: &TableClip, tsv: &str) -> CellClip {
    CellClip {
        rows: tc.rows,
        cols: tc.cols,
        cells: tc.cells.clone(),
        tsv: tsv.to_string(),
        origin: tc.origin,
        doc: tc.doc.clone(),
        merges: tc.merges.clone(),
        whole: None,
        looks: tc.looks.clone(),
        grid: tc.grid.clone(),
        sizes: (tc.col_widths.clone(), tc.row_heights.clone()),
        name: if tc.whole { tc.name.clone() } else { String::new() },
        headers: tc.whole.then_some(tc.headers),
    }
}

/// Puts the copy on the system clipboard: its HTML table (for the suite's other apps,
/// LibreOffice, browsers) with the tab-separated text. False when the clipboard couldn't take
/// it (the caller puts the text there instead).
pub fn put_on_clipboard(clip: &TableClip, tsv: &str) -> bool {
    kn_ui::clipboard::set_html(&clip.to_html(), tsv)
}

/// A table another app (or another Values window, or a page) put on the clipboard becomes this
/// window's copy, so a paste takes it whole: values, formats, looks, borders, merges. False
/// when the clipboard holds no table, or holds this window's own copy (`text`: what the paste
/// brought, the clipboard's text).
pub fn adopt_html(app: &mut App, text: &str) -> bool {
    // (copied objects paste first, as they always have)
    if app.obj_clip_active || is_ours(app, text) {
        return false;
    }
    match kn_ui::clipboard::html() {
        Some(html) => adopt_table_html(app, &html, text),
        None => false,
    }
}

pub fn adopt_table_html(app: &mut App, html: &str, text: &str) -> bool {
    if !nm_core::kn_model::tableclip::has_table(html) {
        return false;
    }
    let Some(tc) = TableClip::from_html(html) else { return false };
    app.cell_clip = Some(cell_clip_of(&tc, text));
    app.obj_clip_active = false;
    true
}

/// A table selected as an object, copied: the whole table on the system clipboard too, for the
/// suite's other apps (Values itself pastes its own object copy).
pub fn copy_table_object(app: &App, t: nm_core::kn_format::ObjId) {
    let Some(doc) = app.doc() else { return };
    if let Some(tc) = nm_core::kn_model::tableclip::read(doc, t, (0, 0, 0, 0), true) {
        put_on_clipboard(&tc, &tc.tsv());
    }
}

/// Everything a copy keeps of cell (r, c) (shown row) of table `t`.
pub fn cell_copy(app: &mut App, t: nm_core::kn_format::ObjId, r: u32, c: u32) -> Option<CellCopy> {
    let (shown, style) = app.table(t).map(|(_, d)| {
        let i = (r * d.cols + c) as usize;
        (d.cells.get(i).cloned().unwrap_or_default(), d.cell_styles.get(i).cloned())
    })?;
    let mr = app.mrow(t, r);
    let formula = mr.and_then(|mr| app.formula_of(t, mr, c));
    let format = mr.and_then(|mr| app.doc().map(|d| d.cell_format(t, mr, c))).unwrap_or(CellFormat::Automatic);
    let covered = app.table(t).and_then(|(_, d)| d.merge_at(r, c)).is_some_and(|m| (m.row, m.col) != (r, c));
    let edges = app.table(t).map(|(_, d)| cell_edges(d, r, c)).unwrap_or_default();
    // a merge's outline: the right / bottom borders from its last cells
    let outline = app.table(t).and_then(|(_, d)| match d.merge_at(r, c) {
        Some(m) if !covered => {
            let (last_r, last_c) = (m.row + m.rows - 1, m.col + m.cols - 1);
            let (a, b) = (cell_edges(d, r, c), cell_edges(d, last_r, last_c));
            Some([a[0].clone(), b[1].clone(), a[2].clone(), b[3].clone()])
        }
        _ => None,
    });
    let region = app.table(t).map(|(_, d)| region(d, r, c)).unwrap_or(3);
    Some(CellCopy { formula, shown, format, style, covered, edges, outline, region, value: None })
}

/// Table > Merge Cells as Numbers merges: when only one of the cells holds something, the merged
/// cell keeps it as it was (its value, number format, formula and look), wherever it was in
/// the block; otherwise the cells' contents join as text (tabs between columns, line breaks
/// between rows, Keydeck's merge_cells).
pub fn merge_cells(app: &mut App, t: nm_core::kn_format::ObjId, b: (u32, u32, u32, u32)) {
    let (r0, c0, r1, c1) = b;
    let filled: Vec<(u32, u32)> = match app.table(t) {
        Some((_, d)) => (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c))).filter(|&(r, c)| !d.cells.get((r * d.cols + c) as usize).is_none_or(|v| v.is_empty())).collect(),
        None => return,
    };
    let lone = match filled.as_slice() {
        [one] if *one != (r0, c0) => cell_copy(app, t, one.0, one.1).map(|cc| (*one, cc)),
        _ => None,
    };
    let (Some(mr0), Some(mr1)) = (app.mrow(t, r0), app.mrow(t, r1)) else { return };
    let looks = app.table(t).map(|(_, d)| (region_looks(d), grid_lines(d)));
    let dest = app.table(t).map(|(_, d)| (d.cell_styles.get((r0 * d.cols + c0) as usize).cloned(), cell_edges(d, r0, c0), region(d, r0, c0)));
    let lone = lone.and_then(|((r, c), cc)| Some(((app.mrow(t, r)?, c, r), cc)));
    let res = app.edit("Merge Cells", |d, h| -> Result<(), nm_core::kn_format::Error> {
        if let (Some(((mr, c, r), cc)), Some((looks, grid)), Some((dest, dest_edges, dest_region))) = (lone, looks, dest) {
            // the one cell's content moves to the merge's first cell, as a move would (its
            // formula's references unchanged), with its look
            let job = Job { r: mr0, c: c0, top_r: i64::from(r), left_c: i64::from(c), src_look: looks[cc.region as usize].clone(), dest_look: looks[dest_region as usize].clone(), cc: CellCopy { edges: dest_edges.clone(), ..cc }, dest, dest_edges, src_grid: grid.clone(), dest_grid: grid };
            write_cell(d, h, t, &job, PasteMode::All, (r, c), &|f| f.clone())?;
            d.set_cell(h, t, mr, c, &CellInput::Empty)?;
        }
        d.merge_cells(h, t, nm_core::kn_model::CellRange { row: mr0, col: c0, rows: mr1 - mr0 + 1, cols: c1 - c0 + 1 })
    });
    if let Some(Err(e)) = res {
        app.notify(format!("Couldn't merge the cells: {e}"));
    }
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
            let cc = cell_copy(app, t, r, c)?;
            // a cell of several lines goes to other apps quoted, as spreadsheets exchange it
            let shown = &cc.shown;
            line.push(if shown.contains(['\n', '\t', '"']) { format!("\"{}\"", shown.replace('"', "\"\"")) } else { shown.clone() });
            cells.push(cc);
        }
        lines.push(line.join("\t"));
    }
    // (one empty cell is a line break: the clipboard drops an empty paste, and then pasting
    // the empty cell over another wouldn't reach us to clear it)
    let tsv = match lines.join("\n") {
        t if t.is_empty() => "\n".to_string(),
        t => t,
    };
    let merges = app
        .table(t)
        .map(|(_, d)| d.merges.iter().filter(|m| m.row >= r0 && m.row + m.rows - 1 <= r1 && m.col >= c0 && m.col + m.cols - 1 <= c1).map(|m| (m.row - r0, m.col - c0, m.rows, m.cols)).collect())
        .unwrap_or_default();
    let (rows, cols) = app.table(t).map(|(_, d)| (d.rows, d.cols)).unwrap_or((0, 0));
    let (looks, grid) = app.table(t).map(|(_, d)| (region_looks(d), grid_lines(d))).unwrap_or_default();
    let whole = if c0 == 0 && c1 + 1 == cols { Some(true) } else if r0 == 0 && r1 + 1 == rows { Some(false) } else { None };
    // the stored values (a number stays a number whatever its format shows)
    if let Some(values) = app.doc().map(|d| d.cell_values(t)) {
        let w = c1 - c0 + 1;
        for (k, cc) in cells.iter_mut().enumerate() {
            let (r, c) = (r0 + k as u32 / w, c0 + k as u32 % w);
            cc.value = app.mrow(t, r).and_then(|mr| values.get(&(mr, c)).cloned());
        }
    }
    app.cell_clip = Some(CellClip { rows: r1 - r0 + 1, cols: c1 - c0 + 1, cells, tsv: tsv.clone(), origin: (r0, c0), doc: app.doc().map(nm_core::kn_model::insert::doc_identity).unwrap_or_default(), merges, whole, looks, grid, sizes: Default::default(), name: String::new(), headers: None });
    app.obj_clip_active = false;
    Some(tsv)
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
    let (dest_looks, dest_grid) = app.table(t).map(|(_, d)| (region_looks(d), grid_lines(d))).unwrap_or_default();
    // what to write where (model rows), with how the destination looks now (shown rows)
    let mut jobs: Vec<Job> = Vec::new();
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
            // a merge pasted as one ordinary cell: the merge's outline is that cell's border; a
            // merge made again takes each of its cells' own edges (covered ones too), so every
            // side of it is drawn whole
            let cc = match &cc.outline {
                Some(o) if !remerge => CellCopy { edges: o.clone(), ..cc },
                _ => cc,
            };
            let (dest, dest_edges, dest_region) = app.table(t).map(|(_, d)| (d.cell_styles.get((r * d.cols + c) as usize).cloned(), cell_edges(d, r, c), region(d, r, c))).unwrap_or((None, Default::default(), 3));
            let src_look = clip.looks[cc.region as usize].clone();
            let dest_look = dest_looks[dest_region as usize].clone();
            jobs.push(Job { r: mr, c, cc, top_r: i64::from(r) - i64::from(i % clip.rows), left_c: i64::from(c) - i64::from(j % clip.cols), dest, dest_edges, src_look, dest_look, src_grid: clip.grid.clone(), dest_grid: dest_grid.clone() });
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
    // (the rows / columns made for it and the paste are one step to undo)
    let merge_before = app.continue_edit;
    app.continue_edit |= more_r > 0 || more_c > 0;
    let res = app.edit(label, |d, hist| -> Result<(), nm_core::kn_format::Error> {
        for &(r, c) in &unmerge {
            d.unmerge_cells(hist, t, r, c);
        }
        // (a cell that can't take part of its look doesn't stop the rest of the paste)
        let mut failed = None;
        for job in &jobs {
            if let Err(e) = write_cell(d, hist, t, job, mode, origin, &portable) {
                failed.get_or_insert(e);
            }
        }
        for m in &new_merges {
            d.merge_cells(hist, t, *m)?;
        }
        failed.map_or(Ok(()), Err)
    });
    app.continue_edit = merge_before;
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
    let names: Vec<String> = app.drawables.iter().filter_map(|d| match &d.kind { nm_core::kn_model::DrawableKind::Table(t) => Some(t.name.clone()), _ => None }).collect();
    let name = format!("{}-1", names.first().cloned().unwrap_or_else(|| "Table 1".into()));
    // a whole table from another app keeps its name (when the sheet has none like it)
    let name = if !clip.name.trim().is_empty() && !names.iter().any(|n| n == clip.name.trim()) { clip.name.trim().to_string() } else { name };
    let (hr, hc, fr) = clip.headers.unwrap_or_default();
    let made = app.edit("Paste", |d, h| -> Result<nm_core::kn_format::ObjId, nm_core::kn_format::Error> {
        use nm_core::kn_model::tablerows::{Axis, HeaderKind};
        let t = nm_core::sheets::insert_table(d, h, sheet, &donor, dinfo, 2.0, 550.0)?;
        for (k, n) in [(HeaderKind::Rows, hr), (HeaderKind::Columns, hc), (HeaderKind::Footer, fr)] {
            d.set_table_header_count(h, t, k, 0)?;
            if n > 0 {
                let _ = d.set_table_header_count(h, t, k, n);
            }
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
            // the column widths and row heights of a whole table from another app (the same step)
            let size = |v: &[f32]| -> Vec<(u32, f32)> { v.iter().enumerate().filter(|(_, s)| **s > 0.5).map(|(i, s)| (i as u32, *s)).collect() };
            let (w, ht) = (size(&clip.sizes.0), size(&clip.sizes.1));
            if !w.is_empty() || !ht.is_empty() {
                let before = app.continue_edit;
                app.continue_edit = true;
                app.edit("Paste", |d, h| {
                    d.set_table_sizes(h, t, false, &w);
                    d.set_table_sizes(h, t, true, &ht);
                });
                app.continue_edit = before;
                app.refresh();
            }
        }
        Some(Err(e)) => app.notify(format!("Couldn't paste: {e}")),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use nm_core::kn_format::ObjId;
    use nm_core::kn_model::format::{Dash, FillSpec, StrokeSpec};
    use nm_core::kn_model::tablefmt::BorderEdges;
    use nm_core::kn_model::{CellRange, Color, DrawableKind, Fill};

    /// A Values window's state on a new Blank spreadsheet (no window), its settings in a scratch
    /// folder so the user's aren't read or written.
    fn app() -> App {
        static ENV: std::sync::Once = std::sync::Once::new();
        ENV.call_once(|| {
            let dir = std::env::temp_dir().join(format!("values-clip-tests-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            // SAFETY: set once, before any test reads the environment through App
            unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
            nm_core::init();
        });
        let mut a = App::with_ctx(&egui::Context::default(), None);
        a.refresh();
        a
    }

    fn tables(a: &App) -> Vec<ObjId> {
        a.drawables.iter().filter(|d| matches!(d.kind, DrawableKind::Table(_))).map(|d| d.id).collect()
    }

    fn put(a: &mut App, t: ObjId, r: u32, c: u32, v: &str) {
        a.edit("t", |d, h| if v.starts_with('=') { d.set_formula_text(h, t, r, c, v) } else { nm_core::typed::set_typed(d, h, t, r, c, v) }).unwrap().unwrap();
        a.refresh();
    }

    fn red() -> Color {
        Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
    }

    fn fill(a: &mut App, t: ObjId, b: (u32, u32, u32, u32)) {
        a.edit("t", |d, h| d.set_cells_fill(h, t, b, Some(&FillSpec::Color(red())))).unwrap().unwrap();
        a.refresh();
    }

    fn sel(a: &mut App, t: ObjId, from: (u32, u32), to: (u32, u32)) {
        a.sel = Some(Sel { table: t, anchor: from, cursor: to });
    }

    fn cell(a: &App, t: ObjId, r: u32, c: u32) -> (String, TableCellStyle) {
        let (_, d) = a.table(t).unwrap();
        let i = (r * d.cols + c) as usize;
        (d.cells[i].clone(), d.cell_styles[i].clone())
    }

    fn copy_paste(a: &mut App, t: ObjId, from: ((u32, u32), (u32, u32)), to_t: ObjId, to: ((u32, u32), (u32, u32))) {
        sel(a, t, from.0, from.1);
        copy(a).unwrap();
        sel(a, to_t, to.0, to.1);
        assert!(paste(a, PasteMode::All));
        a.refresh();
        assert!(a.message.is_none(), "paste reported {:?}", a.message);
    }

    /// A range with empty, unstyled cells in it pastes whole (an empty cell used to stop the
    /// paste: its text style couldn't be set), range to range.
    #[test]
    fn range_to_range_with_empty_cells() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 1, 1, "a");
        put(&mut a, t, 2, 2, "d");
        fill(&mut a, t, (2, 2, 2, 2));
        // B2:C3 holds a, (empty), (empty), d; C3 red
        copy_paste(&mut a, t, ((1, 1), (2, 2)), t, ((5, 1), (6, 2)));
        assert_eq!(cell(&a, t, 5, 1).0, "a");
        assert_eq!(cell(&a, t, 5, 2).0, "");
        assert_eq!(cell(&a, t, 6, 2).0, "d");
        assert_eq!(cell(&a, t, 6, 2).1.fill, Fill::Color(red()));
        assert_ne!(cell(&a, t, 5, 1).1.fill, Fill::Color(red()));
    }

    /// One cell selected: the whole range goes in from it (the table grows to take it), every
    /// cell, and the selection becomes the pasted range (Numbers, Mac N18 P3).
    #[test]
    fn range_to_single_cell() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 1, 0, "x");
        put(&mut a, t, 3, 1, "y");
        let rows = a.table(t).unwrap().1.rows;
        copy_paste(&mut a, t, ((1, 0), (3, 1)), t, ((rows - 1, 3), (rows - 1, 3)));
        let (_, d) = a.table(t).unwrap();
        assert_eq!(d.rows, rows + 2, "the table grew for the paste");
        assert_eq!(cell(&a, t, rows - 1, 3).0, "x");
        assert_eq!(cell(&a, t, rows + 1, 4).0, "y");
        let s = a.sel.unwrap();
        assert_eq!(s.block(), (rows - 1, 3, rows + 1, 4));
    }

    /// Empty cells' look comes along: their fill, merges and borders, and it replaces what
    /// the destination had (a filled cell gets the copied cell's lack of fill).
    #[test]
    fn empty_formatted_cells() {
        let mut a = app();
        let t = tables(&a)[0];
        fill(&mut a, t, (1, 1, 1, 1));
        let stroke = StrokeSpec { color: red(), width: 2.0, dash: Dash::Solid };
        a.edit("t", |d, h| d.set_cells_border(h, t, (1, 1, 1, 1), BorderEdges::Bottom, Some(&stroke))).unwrap().unwrap();
        a.edit("t", |d, h| d.merge_cells(h, t, CellRange { row: 2, col: 1, rows: 1, cols: 2 })).unwrap().unwrap();
        a.refresh();
        // the destination: filled where the copy has no fill
        fill(&mut a, t, (5, 2, 5, 2));
        copy_paste(&mut a, t, ((1, 1), (2, 2)), t, ((5, 1), (5, 1)));
        assert_eq!(cell(&a, t, 5, 1).1.fill, Fill::Color(red()), "an empty cell's fill");
        assert_ne!(cell(&a, t, 5, 2).1.fill, Fill::Color(red()), "the destination's fill replaced");
        let (_, d) = a.table(t).unwrap();
        assert!(d.merges.iter().any(|m| (m.row, m.col, m.rows, m.cols) == (6, 1, 1, 2)), "the merge made again: {:?}", d.merges);
        let bottom = cell_edges(d, 5, 1)[1].clone();
        assert!(bottom.is_some_and(|s| s.color == red() && (s.width - 2.0).abs() < 0.01), "the border came along");
    }

    /// A merged cell copied on its own onto one ordinary cell: one ordinary cell with the
    /// merge's value and look (Mac N20); onto a selection its size: the merge again.
    #[test]
    fn merged_to_unmerged() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 1, 1, "m");
        fill(&mut a, t, (1, 1, 1, 1));
        a.edit("t", |d, h| d.merge_cells(h, t, CellRange { row: 1, col: 1, rows: 2, cols: 1 })).unwrap().unwrap();
        a.refresh();
        put(&mut a, t, 6, 3, "old");
        copy_paste(&mut a, t, ((1, 1), (1, 1)), t, ((5, 3), (5, 3)));
        assert_eq!(cell(&a, t, 5, 3).0, "m");
        assert_eq!(cell(&a, t, 5, 3).1.fill, Fill::Color(red()));
        assert_eq!(cell(&a, t, 6, 3).0, "old", "the cell under it is left alone");
        assert!(a.table(t).unwrap().1.merge_at(5, 3).is_none());
        // a selection the merge's size: merged again
        copy_paste(&mut a, t, ((1, 1), (1, 1)), t, ((5, 4), (6, 4)));
        let m = a.table(t).unwrap().1.merge_at(5, 4).copied();
        assert_eq!(m.map(|m| (m.row, m.rows)), Some((5, 2)));
    }

    /// Cells copied from one table into another: values, formats and fills.
    #[test]
    fn cross_table() {
        let mut a = app();
        let t = tables(&a)[0];
        a.insert_table(0);
        a.refresh();
        let u = *tables(&a).iter().find(|x| **x != t).unwrap();
        put(&mut a, t, 1, 1, "12");
        put(&mut a, t, 1, 2, "text");
        put(&mut a, t, 2, 1, "=B2*2");
        fill(&mut a, t, (1, 2, 1, 2));
        copy_paste(&mut a, t, ((1, 1), (2, 2)), u, ((2, 0), (2, 0)));
        assert_eq!(cell(&a, u, 2, 0).0, "12");
        assert_eq!(cell(&a, u, 2, 1).0, "text");
        assert_eq!(cell(&a, u, 2, 1).1.fill, Fill::Color(red()));
        let mr = a.mrow(u, 3).unwrap();
        assert_eq!(a.formula_of(u, mr, 0).map(|f| f.replace('×', "*")).as_deref(), Some("=A3*2"), "the formula follows its new place");
        assert_eq!(cell(&a, u, 3, 0).0, "24");
    }

    /// Cells as another Values window or Keydeck / Letters would send them: through the
    /// clipboard's HTML. Pasted, they give what Values' own copy gives: values, formats, fills,
    /// formula, merges and every border.
    #[test]
    fn html_paste_matches_own_paste() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        put(&mut a, t, 3, 1, "1234.5");
        a.edit("t", |d, h| d.set_cells_format(h, t, (3, 1, 3, 1), &CellFormat::Currency { code: "EUR".into(), decimals: Some(2), thousands: true, negative: 0, accounting: false })).unwrap().unwrap();
        put(&mut a, t, 3, 3, "=B4*2");
        a.refresh();
        // our own paste
        copy_paste(&mut a, t, ((1, 1), (3, 3)), t, ((6, 1), (6, 1)));
        // the same copy as another app's HTML
        sel(&mut a, t, (1, 1), (3, 3));
        let tsv = copy(&mut a).unwrap();
        let html = table_clip(a.cell_clip.as_ref().unwrap()).to_html();
        a.cell_clip = None;
        assert!(adopt_table_html(&mut a, &html, &tsv));
        assert!(is_ours(&a, &tsv), "the adopted copy is the paste's");
        sel(&mut a, t, (11, 1), (11, 1));
        assert!(paste(&mut a, PasteMode::All));
        a.refresh();
        assert!(a.message.is_none(), "{:?}", a.message);
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(cell(&a, t, 6 + i, 1 + j), cell(&a, t, 11 + i, 1 + j), "cell +{i},+{j}");
            }
        }
        assert_edges_match(&a, t, (6, 1), t, (11, 1), (3, 3));
        let (_, d) = a.table(t).unwrap();
        assert_eq!(d.merge_at(11, 1).map(|m| (m.rows, m.cols)), Some((2, 3)));
        assert_eq!(cell(&a, t, 13, 1).0, cell(&a, t, 3, 1).0, "the currency format came along");
        let mr = a.mrow(t, 13).unwrap();
        assert_eq!(a.formula_of(t, mr, 3).map(|f| f.replace('×', "*")).as_deref(), Some("=B14*2"));
    }

    static KEYDECK_TABLE: &[u8] = include_bytes!("../../../../keynote-linux/crates/kn-model/data/donors/R2-table-values.key");

    /// A table made in Keydeck / Letters (the same engine): its cells, copied as the suite's
    /// HTML, pasted into Values cells and onto the empty sheet as a whole new table.
    #[test]
    fn keydeck_table_into_values() {
        use nm_core::kn_model::tableclip;
        use nm_core::kn_model::tablecells::CellAlign;
        let mut k = nm_core::kn_model::Document::from_file(nm_core::kn_format::KeyFile::from_zip(KEYDECK_TABLE).unwrap());
        let kt = k.file.objects_of("TST.TableInfoArchive").next().unwrap().0;
        let mut h = nm_core::kn_model::edit::History::default();
        k.begin(&mut h, "t");
        tableclip::resize(&mut k, &mut h, kt, 3, 3).unwrap();
        k.set_cell(&mut h, kt, 1, 0, &CellInput::Text("Total".into())).unwrap();
        k.set_cell(&mut h, kt, 1, 1, &CellInput::Number(0.5)).unwrap();
        k.set_cells_format(&mut h, kt, (1, 1, 1, 1), &CellFormat::Percent { decimals: Some(0), thousands: false, negative: 0 }).unwrap();
        k.set_cells_fill(&mut h, kt, (1, 0, 1, 0), Some(&FillSpec::Color(red()))).unwrap();
        k.set_cells_text_prop(&mut h, kt, (1, 0, 1, 0), &TextProp::Italic(true)).unwrap();
        // (Keydeck's tables centre their text: a cell set apart is right-aligned)
        k.set_cells_alignment(&mut h, kt, (1, 0, 1, 0), CellAlign::Right).unwrap();
        k.set_cell(&mut h, kt, 2, 0, &CellInput::Text("wide".into())).unwrap();
        k.merge_cells(&mut h, kt, CellRange { row: 2, col: 0, rows: 1, cols: 2 }).unwrap();
        k.set_table_sizes(&mut h, kt, false, &[(2, 140.0)]);
        k.end(&mut h);
        // cells into cells
        let cells = tableclip::read(&k, kt, (1, 0, 2, 1), false).unwrap();
        let mut a = app();
        let t = tables(&a)[0];
        sel(&mut a, t, (4, 1), (4, 1));
        assert!(adopt_table_html(&mut a, &cells.to_html(), &cells.tsv()));
        assert!(paste(&mut a, PasteMode::All));
        a.refresh();
        assert_eq!(cell(&a, t, 4, 1).0, "Total");
        assert!(cell(&a, t, 4, 1).1.text.italic);
        assert_eq!(cell(&a, t, 4, 1).1.fill, Fill::Color(red()));
        assert_eq!(cell(&a, t, 4, 2).0, "50%");
        assert_eq!(cell(&a, t, 4, 1).1.align, nm_core::kn_model::Align::Right);
        assert_eq!(a.doc().unwrap().cell_values(t).get(&(4, 2)), Some(&nm_core::kn_model::table::CellValue::Number(0.5)));
        assert_eq!(a.table(t).unwrap().1.merge_at(5, 1).map(|m| (m.row, m.col, m.cols)), Some((5, 1, 2)));
        // the whole table onto the empty sheet: a new table of its size, widths and name
        let whole = tableclip::read(&k, kt, (0, 0, 0, 0), true).unwrap();
        a.sel = None;
        assert!(adopt_table_html(&mut a, &whole.to_html(), &whole.tsv()));
        let before = tables(&a);
        paste_new_table(&mut a);
        a.refresh();
        let u = *tables(&a).iter().find(|x| !before.contains(x)).expect("a new table");
        let (_, d) = a.table(u).unwrap();
        assert_eq!((d.rows, d.cols), (3, 3));
        assert!((d.col_widths[2] - 140.0).abs() < 0.5, "{:?}", d.col_widths);
        assert_eq!(d.header_rows, whole.headers.0);
        assert_eq!(cell(&a, u, 1, 1).0, "50%");
    }

    /// The clipboard's text is recognised as the copied cells even when another app changed
    /// its line ends.
    #[test]
    fn own_text_recognised() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 1, 1, "a");
        sel(&mut a, t, (1, 1), (2, 2));
        let tsv = copy(&mut a).unwrap();
        assert!(is_ours(&a, &tsv));
        assert!(is_ours(&a, &format!("{}\r\n", tsv.replace('\n', "\r\n"))));
        assert!(is_ours(&a, tsv.trim_end_matches('\n')));
        assert!(!is_ours(&a, "b"));
    }

    /// Merging cells of which only one holds something keeps it as it was (a number stays a
    /// number with its format and fill), wherever it was; several join as text.
    #[test]
    fn merge_keeps_a_lone_value() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 2, 2, "5");
        fill(&mut a, t, (2, 2, 2, 2));
        a.edit("t", |d, h| d.set_cells_format(h, t, (2, 2, 2, 2), &CellFormat::Percent { decimals: Some(0), thousands: false, negative: 0 })).unwrap().unwrap();
        a.refresh();
        let shown = cell(&a, t, 2, 2).0;
        merge_cells(&mut a, t, (1, 1, 2, 2));
        a.refresh();
        let (_, d) = a.table(t).unwrap();
        assert!(d.merge_at(1, 1).is_some_and(|m| (m.rows, m.cols) == (2, 2)));
        assert_eq!(cell(&a, t, 1, 1).0, shown);
        assert_eq!(cell(&a, t, 1, 1).1.fill, Fill::Color(red()));
        let mr = a.mrow(t, 1).unwrap();
        assert!(matches!(a.doc().unwrap().cell_format(t, mr, 1), CellFormat::Percent { .. }));
        // two values: joined as text
        put(&mut a, t, 5, 1, "x");
        put(&mut a, t, 5, 2, "y");
        merge_cells(&mut a, t, (5, 1, 5, 2));
        a.refresh();
        assert_eq!(cell(&a, t, 5, 1).0, "x\ty");
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 120, 255, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A pasted picture with a cell selected is the cell's Image Fill; with nothing selected a
    /// picture on the sheet, selected.
    #[test]
    fn pasted_pictures() {
        let mut a = app();
        let t = tables(&a)[0];
        sel(&mut a, t, (2, 2), (2, 2));
        paste_picture(&mut a, png(8, 6), "Pasted Image.png");
        a.refresh();
        assert!(matches!(cell(&a, t, 2, 2).1.fill, Fill::Image { .. }), "{:?}", cell(&a, t, 2, 2).1.fill);
        a.sel = None;
        let n = a.drawables.len();
        paste_picture(&mut a, png(40, 30), "Pasted Image.png");
        a.refresh();
        assert_eq!(a.drawables.len(), n + 1);
        let id = a.canvas.obj.expect("the picture is selected");
        assert!(a.drawables.iter().any(|d| d.id == id && matches!(d.kind, DrawableKind::Image(_))));
        assert!(a.message.is_none(), "{:?}", a.message);
    }

    fn blue() -> Color {
        Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 }
    }

    /// Two borders look the same (colour, width, solid or not).
    fn same_edge(a: &Option<Stroke>, b: &Option<Stroke>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => a.color == b.color && (a.width - b.width).abs() < 0.01 && a.dash.is_empty() == b.dash.is_empty(),
            (None, None) => true,
            _ => false,
        }
    }

    /// B2:D4 of table `t`: B2:D3 a 2×3 merge with a red 2 pt outline and a red fill, B4:D4
    /// plain cells with blue 1 pt borders all round (inner strokes between them too).
    fn bordered_block(a: &mut App, t: ObjId) {
        let thin = StrokeSpec { color: blue(), width: 1.0, dash: Dash::Solid };
        let thick = StrokeSpec { color: red(), width: 2.0, dash: Dash::Solid };
        put(a, t, 1, 1, "m");
        put(a, t, 3, 2, "x");
        fill(a, t, (1, 1, 1, 1));
        a.edit("t", |d, h| d.set_cells_border(h, t, (3, 1, 3, 3), BorderEdges::All, Some(&thin))).unwrap().unwrap();
        a.edit("t", |d, h| d.set_cells_border(h, t, (1, 1, 2, 3), BorderEdges::Outside, Some(&thick))).unwrap().unwrap();
        a.edit("t", |d, h| d.merge_cells(h, t, CellRange { row: 1, col: 1, rows: 2, cols: 3 })).unwrap().unwrap();
        a.refresh();
    }

    /// Every border of block `n` (rows, cols) at `to` in table `u` as in the block at `from`
    /// in table `t`: outer edges and inner ones, merged cells' edges included.
    fn assert_edges_match(a: &App, t: ObjId, from: (u32, u32), u: ObjId, to: (u32, u32), n: (u32, u32)) {
        let (_, src) = a.table(t).unwrap();
        let (_, dst) = a.table(u).unwrap();
        for i in 0..n.0 {
            for j in 0..n.1 {
                let (s, d) = (cell_edges(src, from.0 + i, from.1 + j), cell_edges(dst, to.0 + i, to.1 + j));
                for k in 0..4 {
                    // (lines inside a merge don't show: only the drawn ones count)
                    let inner = match k {
                        0 => i > 0 && src.merge_at(from.0 + i, from.1 + j).is_some_and(|m| m.contains(from.0 + i - 1, from.1 + j)),
                        1 => src.merge_at(from.0 + i, from.1 + j).is_some_and(|m| m.contains(from.0 + i + 1, from.1 + j)),
                        2 => j > 0 && src.merge_at(from.0 + i, from.1 + j).is_some_and(|m| m.contains(from.0 + i, from.1 + j - 1)),
                        _ => src.merge_at(from.0 + i, from.1 + j).is_some_and(|m| m.contains(from.0 + i, from.1 + j + 1)),
                    };
                    if inner {
                        continue;
                    }
                    assert!(same_edge(&s[k], &d[k]), "cell +{i},+{j} edge {k}: copied {:?}, pasted {:?}", s[k], d[k]);
                }
            }
        }
    }

    /// A merged block with an outline, and bordered plain cells under it, pasted onto plain
    /// cells: the merge comes back with every side of its outline whole (the covered cells'
    /// parts too, not only its first cell's), and the plain cells' borders inside and out.
    #[test]
    fn merged_borders_paste_whole() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        copy_paste(&mut a, t, ((1, 1), (3, 3)), t, ((6, 1), (6, 1)));
        let (_, d) = a.table(t).unwrap();
        assert!(d.merges.iter().any(|m| (m.row, m.col, m.rows, m.cols) == (6, 1, 2, 3)), "{:?}", d.merges);
        assert_edges_match(&a, t, (1, 1), t, (6, 1), (3, 3));
        // the bottom of the merge, under its second and third columns (its covered cells)
        let (_, d) = a.table(t).unwrap();
        for c in 1..=3 {
            assert!(cell_edges(d, 7, c)[1].as_ref().is_some_and(|s| s.color == red() && (s.width - 2.0).abs() < 0.01), "bottom under column {c}");
            assert!(cell_edges(d, 6, c)[0].as_ref().is_some_and(|s| s.color == red()), "top over column {c}");
        }
        for r in 6..=7 {
            assert!(cell_edges(d, r, 3)[3].as_ref().is_some_and(|s| s.color == red()), "right side, row {r}");
            assert!(cell_edges(d, r, 1)[2].as_ref().is_some_and(|s| s.color == red()), "left side, row {r}");
        }
    }

    /// The same block pasted into another table, and onto cells that are merged differently
    /// (the destination's merge is undone, the copy's made, every edge whole).
    #[test]
    fn merged_borders_paste_cross_table_and_over_merges() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        a.insert_table(0);
        a.refresh();
        let u = *tables(&a).iter().find(|x| **x != t).unwrap();
        copy_paste(&mut a, t, ((1, 1), (3, 3)), u, ((2, 0), (2, 0)));
        assert!(a.table(u).unwrap().1.merges.iter().any(|m| (m.row, m.col, m.rows, m.cols) == (2, 0, 2, 3)));
        assert_edges_match(&a, t, (1, 1), u, (2, 0), (3, 3));
        // onto a destination merged another way (a 2×2 across where the copy's merge ends)
        a.edit("t", |d, h| d.merge_cells(h, t, CellRange { row: 10, col: 2, rows: 2, cols: 2 })).unwrap().unwrap();
        a.refresh();
        copy_paste(&mut a, t, ((1, 1), (3, 3)), t, ((9, 1), (9, 1)));
        let (_, d) = a.table(t).unwrap();
        assert!(d.merges.iter().any(|m| (m.row, m.col, m.rows, m.cols) == (9, 1, 2, 3)), "{:?}", d.merges);
        assert!(!d.merges.iter().any(|m| (m.row, m.col) == (10, 2)), "the old merge is undone");
        assert_edges_match(&a, t, (1, 1), t, (9, 1), (3, 3));
        // onto another merge of the same size
        copy_paste(&mut a, t, ((1, 1), (3, 3)), t, ((9, 1), (9, 1)));
        assert_edges_match(&a, t, (1, 1), t, (9, 1), (3, 3));
    }

    /// A lone merged cell onto one plain cell: one plain cell with the merge's whole outline.
    #[test]
    fn lone_merge_outline_on_one_cell() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        copy_paste(&mut a, t, ((1, 1), (1, 1)), t, ((8, 5), (8, 5)));
        let (_, d) = a.table(t).unwrap();
        assert!(d.merge_at(8, 5).is_none());
        let e = cell_edges(d, 8, 5);
        assert!(e.iter().all(|s| s.as_ref().is_some_and(|s| s.color == red() && (s.width - 2.0).abs() < 0.01)), "{e:?}");
    }

    /// Everything the table shows: size, rows' and columns' sizes, values, looks, merges and
    /// borders.
    fn table_state(a: &App, t: ObjId) -> String {
        let (_, d) = a.table(t).unwrap();
        format!("{:?}", (d.rows, d.cols, &d.row_heights, &d.col_widths, &d.cells, &d.merges, &d.h_strokes, &d.v_strokes, &d.cell_styles))
    }

    /// Where two table states first differ (for the failure message).
    fn first_difference(a: &[String], b: &[String]) -> String {
        for (x, y) in a.iter().zip(b) {
            if let Some(i) = x.bytes().zip(y.bytes()).position(|(p, q)| p != q) {
                let lo = i.saturating_sub(300);
                return format!("\n  was: …{}…\n  now: …{}…", &x[lo..(i + 200).min(x.len())], &y[lo..(i + 200).min(y.len())]);
            }
        }
        String::new()
    }

    /// `act` is one undo step: undo puts every table back exactly as it was, redo makes it
    /// again exactly.
    fn one_step(a: &mut App, label: &str, act: impl FnOnce(&mut App)) {
        a.refresh();
        let ts = tables(a);
        let before: Vec<String> = ts.iter().map(|t| table_state(a, *t)).collect();
        let n = a.hist.undo_len();
        act(a);
        a.refresh();
        let after: Vec<String> = ts.iter().map(|t| table_state(a, *t)).collect();
        assert_ne!(before, after, "{label}: nothing changed");
        assert_eq!(a.hist.undo_len(), n + 1, "{label}: one undo step");
        a.undo();
        a.refresh();
        let undone: Vec<String> = ts.iter().map(|t| table_state(a, *t)).collect();
        assert!(undone == before, "{label}: undo didn't put the tables back{}", first_difference(&before, &undone));
        a.redo();
        a.refresh();
        let redone: Vec<String> = ts.iter().map(|t| table_state(a, *t)).collect();
        assert!(redone == after, "{label}: redo didn't make it again{}", first_difference(&after, &redone));
        assert_eq!(a.hist.undo_len(), n + 1);
    }

    #[test]
    fn undo_paste_range_to_range() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        fill(&mut a, t, (7, 2, 7, 2));
        put(&mut a, t, 6, 1, "old");
        sel(&mut a, t, (1, 1), (3, 3));
        copy(&mut a).unwrap();
        sel(&mut a, t, (6, 1), (8, 3));
        one_step(&mut a, "range to range", |a| {
            paste(a, PasteMode::All);
        });
    }

    #[test]
    fn undo_paste_range_to_one_cell_growing() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        let (rows, cols) = a.table(t).map(|(_, d)| (d.rows, d.cols)).unwrap();
        sel(&mut a, t, (1, 1), (3, 3));
        copy(&mut a).unwrap();
        // the table grows by rows and columns for it: the same one step
        sel(&mut a, t, (rows - 1, cols - 1), (rows - 1, cols - 1));
        one_step(&mut a, "range to one cell", |a| {
            paste(a, PasteMode::All);
        });
        assert_eq!(a.table(t).map(|(_, d)| (d.rows, d.cols)), Some((rows + 2, cols + 2)));
    }

    #[test]
    fn undo_paste_merged_to_unmerged() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        sel(&mut a, t, (1, 1), (1, 1));
        copy(&mut a).unwrap();
        sel(&mut a, t, (8, 4), (8, 4));
        one_step(&mut a, "merged to one cell", |a| {
            paste(a, PasteMode::All);
        });
        sel(&mut a, t, (8, 1), (9, 3));
        one_step(&mut a, "merged to its size", |a| {
            paste(a, PasteMode::All);
        });
    }

    #[test]
    fn undo_paste_cross_table() {
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        a.insert_table(0);
        a.refresh();
        let u = *tables(&a).iter().find(|x| **x != t).unwrap();
        sel(&mut a, t, (1, 1), (3, 3));
        copy(&mut a).unwrap();
        sel(&mut a, u, (1, 1), (1, 1));
        one_step(&mut a, "cross table", |a| {
            paste(a, PasteMode::All);
        });
    }

    #[test]
    fn undo_merge_cells() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 2, 2, "5");
        fill(&mut a, t, (2, 2, 2, 2));
        one_step(&mut a, "merge keeping a lone value", |a| merge_cells(a, t, (1, 1, 2, 2)));
        put(&mut a, t, 5, 1, "x");
        put(&mut a, t, 5, 2, "y");
        one_step(&mut a, "merge joining values", |a| merge_cells(a, t, (5, 1, 5, 2)));
    }

    /// Two pastes in a row (menu clicks moments apart) are two steps, not one.
    #[test]
    fn two_pastes_two_steps() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 1, 1, "a");
        sel(&mut a, t, (1, 1), (1, 1));
        copy(&mut a).unwrap();
        let n = a.hist.undo_len();
        sel(&mut a, t, (4, 1), (4, 1));
        paste(&mut a, PasteMode::All);
        sel(&mut a, t, (5, 1), (5, 1));
        paste(&mut a, PasteMode::All);
        assert_eq!(a.hist.undo_len(), n + 2);
        a.undo();
        a.refresh();
        assert_eq!(cell(&a, t, 4, 1).0, "a");
        assert_eq!(cell(&a, t, 5, 1).0, "");
    }

    /// The add-rows / columns handle let go: the rows and columns it showed are one step.
    #[test]
    fn undo_grow_drag() {
        let mut a = app();
        let t = tables(&a)[0];
        put(&mut a, t, 1, 1, "a");
        let (rows, cols) = a.table(t).map(|(_, d)| (d.rows, d.cols)).unwrap();
        one_step(&mut a, "add rows and columns", |a| crate::canvas::grow_table(a, t, 5, 2));
        assert_eq!(a.table(t).map(|(_, d)| (d.rows, d.cols)), Some((rows + 5, cols + 2)));
        one_step(&mut a, "take empty rows away", |a| crate::canvas::grow_table(a, t, -7, 0));
        assert_eq!(a.table(t).map(|(_, d)| d.rows), Some(rows - 2));
    }

    /// The table an add-rows / columns drag shows while dragging is the one the drop makes:
    /// sizes, cells, looks and lines (no jump when the handle is let go).
    #[test]
    fn grow_preview_matches_the_drop() {
        for (dr, dc) in [(3, 0), (0, 2), (4, 3), (-2, 0), (0, -1), (-3, -2)] {
            let mut a = app();
            let t = tables(&a)[0];
            put(&mut a, t, 1, 1, "a");
            fill(&mut a, t, (1, 1, 1, 1));
            let base = a.table(t).unwrap().1.clone();
            let (er, ec) = crate::canvas::grow_limits(&base, dr, dc);
            let shown = crate::canvas::grown_table(&base, er, ec);
            crate::canvas::grow_table(&mut a, t, er, ec);
            a.refresh();
            let (_, made) = a.table(t).unwrap();
            assert_eq!((shown.rows, shown.cols), (made.rows, made.cols), "{dr},{dc}");
            assert_eq!(shown.row_heights, made.row_heights, "{dr},{dc} row heights");
            assert_eq!(shown.col_widths, made.col_widths, "{dr},{dc} column widths");
            assert_eq!(shown.cells, made.cells, "{dr},{dc}");
            for i in 0..made.cell_styles.len() {
                assert!(shown.cell_styles[i] == made.cell_styles[i], "{dr},{dc}: cell {} {} look", i as u32 / made.cols, i as u32 % made.cols);
            }
            for (i, (x, y)) in shown.h_strokes.iter().zip(&made.h_strokes).enumerate() {
                assert!(same_edge(x, y), "{dr},{dc}: horizontal line {} {}: {x:?} {y:?}", i as u32 / made.cols, i as u32 % made.cols);
            }
            for (i, (x, y)) in shown.v_strokes.iter().zip(&made.v_strokes).enumerate() {
                assert!(same_edge(x, y), "{dr},{dc}: vertical line row {} {}: {x:?} {y:?}", i as u32 / (made.cols + 1), i as u32 % (made.cols + 1));
            }
        }
    }

    /// Typing wrapped text that makes its row grow: one step; undo shrinks the row back.
    #[test]
    fn undo_typing_that_grows_a_row() {
        let mut a = app();
        let t = tables(&a)[0];
        a.edit("t", |d, h| d.set_cells_wrap(h, t, (2, 1, 2, 1), true)).unwrap().unwrap();
        a.refresh();
        let h0 = a.table(t).unwrap().1.row_heights[2];
        one_step(&mut a, "typing", |a| {
            a.canvas.edit = Some(crate::canvas::CellEdit { table: t, row: 2, col: 1, text: "a long line of text that wraps over several lines in its narrow cell".into(), focus: true, typed: true, click: None });
            a.refresh();
            commit(a);
        });
        assert!(a.table(t).unwrap().1.row_heights[2] > h0 + 1.0, "the row grew");
        a.undo();
        a.refresh();
        assert!((a.table(t).unwrap().1.row_heights[2] - h0).abs() < 0.01, "and shrinks back");
    }

    /// The merged, bordered block pasted onto plain cells and onto a merge, saved for a
    /// snapshot: `NM_MERGED_OUT=file.numbers cargo test -p nm-app make_merged_paste -- --ignored`.
    #[test]
    #[ignore]
    fn make_merged_paste() {
        let Some(out) = std::env::var_os("NM_MERGED_OUT") else { return };
        let mut a = app();
        let t = tables(&a)[0];
        bordered_block(&mut a, t);
        copy_paste(&mut a, t, ((1, 1), (3, 3)), t, ((6, 1), (6, 1)));
        a.edit("t", |d, h| d.merge_cells(h, t, CellRange { row: 11, col: 2, rows: 2, cols: 2 })).unwrap().unwrap();
        a.refresh();
        copy_paste(&mut a, t, ((1, 1), (3, 3)), t, ((10, 1), (10, 1)));
        a.wb.as_mut().unwrap().doc.save_as(std::path::Path::new(&out)).unwrap();
    }

    /// A 200 × 20 table full of numbers for frame-time checks: `NM_BIG_OUT=file.numbers
    /// cargo test -p nm-app make_big_table -- --ignored`.
    #[test]
    #[ignore]
    fn make_big_table() {
        let Some(out) = std::env::var_os("NM_BIG_OUT") else { return };
        let mut a = app();
        let t = tables(&a)[0];
        let (rows, cols) = a.table(t).map(|(_, d)| (d.rows, d.cols)).unwrap();
        crate::canvas::grow_table(&mut a, t, 200 - rows as i32, 20 - cols as i32);
        a.refresh();
        a.edit("t", |d, h| -> Result<(), nm_core::kn_format::Error> {
            for r in 1..200 {
                for c in 1..20 {
                    d.set_cell(h, t, r, c, &CellInput::parse(&format!("{}", r * 100 + c)))?;
                }
            }
            Ok(())
        }).unwrap().unwrap();
        a.wb.as_mut().unwrap().doc.save_as(std::path::Path::new(&out)).unwrap();
    }

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
    app.continue_edit = true;
    paste(app, PasteMode::All);
    app.continue_edit = false;
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

/// What else the system clipboard holds when a paste arrives, before its text: copied picture
/// files, or a picture (a screenshot, an image copied in a browser). Text that is Values' own
/// cells, or that reads as a table (tabs or several lines: a spreadsheet's copy also carries a
/// picture of the cells), stays text. True when a picture was pasted.
pub fn paste_pictures(app: &mut App, text: Option<&str>) -> bool {
    if text.is_some_and(|t| is_ours(app, t)) {
        return false;
    }
    let pictures: Vec<(Vec<u8>, String)> = kn_ui::clipboard::files()
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "heic"].contains(&e.to_ascii_lowercase().as_str())))
        .filter_map(|p| Some((std::fs::read(&p).ok()?, p.file_name()?.to_string_lossy().into_owned())))
        .collect();
    if !pictures.is_empty() {
        for (bytes, name) in pictures {
            paste_picture(app, bytes, &name);
        }
        return true;
    }
    if text.is_some_and(|t| t.contains(['\t', '\n'])) {
        return false;
    }
    match kn_ui::clipboard::image() {
        Some(img) => {
            paste_picture(app, img.png, "Pasted Image.png");
            true
        }
        None => false,
    }
}

/// A picture's bytes as PNG or JPEG (the formats a document keeps); others are converted.
fn storable(bytes: Vec<u8>, name: &str) -> Option<(Vec<u8>, String)> {
    if nm_core::kn_format::objects::image_size(&bytes).is_some() {
        return Some((bytes, name.to_string()));
    }
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    let stem = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Pasted Image".into());
    Some((out.into_inner(), format!("{stem}.png")))
}

/// A pasted picture, as Numbers places it: with a cell selected or being edited, the cell's
/// fill (Image Fill, scaled to fill the cell); otherwise a picture on the sheet, in the middle
/// of what is in view, selected.
pub fn paste_picture(app: &mut App, bytes: Vec<u8>, name: &str) {
    let Some((bytes, name)) = storable(bytes, name) else {
        return app.notify("Couldn't paste the picture: its format can't be read.");
    };
    let cell = app.canvas.edit.as_ref().map(|e| (e.table, e.row, e.col)).or_else(|| app.sel.filter(|_| app.canvas.obj.is_none()).map(|s| (s.table, s.cursor.0, s.cursor.1)));
    if let Some((t, r, c)) = cell {
        commit(app);
        let Some(mr) = app.mrow(t, r) else { return };
        let res = app.edit("Paste", |d, h| -> Result<(), nm_core::kn_format::Error> {
            let size = nm_core::kn_format::objects::image_size(&bytes).map(|(w, h)| (w as f32, h as f32));
            let data_id = d.file.add_image_data(bytes, &name)?;
            let fill = nm_core::kn_model::format::FillSpec::Image { data_id, technique: nm_core::kn_model::ImageFillTechnique::ScaleToFill, size, reference: None, tint: None };
            d.set_cells_fill(h, t, (mr, c, mr, c), Some(&fill))?;
            Ok(())
        });
        if let Some(Err(e)) = res {
            app.notify(format!("Couldn't paste the picture: {e}"));
        }
        return;
    }
    let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
    let (x, y) = app.free_spot_pub();
    match app.edit("Paste", |d, h| nm_core::sheets::insert_image(d, h, sheet, bytes, &name, x, y)) {
        Some(Ok(id)) => {
            app.sel = None;
            app.canvas.obj = Some(id);
            app.refresh();
            // in the middle of the view (Numbers, Mac N15)
            if let (Some(g), Some(c)) = (app.drawables.iter().find(|d| d.id == id).map(crate::tiles::frame), app.canvas.view_center()) {
                let (dx, dy) = (c.x - (g.x + g.w / 2.0), c.y - (g.y + g.h / 2.0));
                app.continue_edit = true;
                app.edit("Paste", |d, h| d.move_by(h, id, dx, dy));
                app.continue_edit = false;
            }
        }
        Some(Err(e)) => app.notify(format!("Couldn't paste the picture: {e}")),
        None => {}
    }
}

