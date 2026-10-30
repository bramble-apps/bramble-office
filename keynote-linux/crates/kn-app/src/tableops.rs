//! Format > Table > Select / Autofill Cells and Edit > Clear All, on the cell at the table
//! cursor and the selected cell range.

use kn_format::ObjId;
use kn_model::tablecells::CellInput;

use crate::app::App;

/// (table, row0, col0, row1, col1) inclusive, ordered.
pub type Range = (ObjId, u32, u32, u32, u32);

fn table(app: &mut App, id: ObjId) -> Option<kn_model::TableData> {
    let sc = app.scene(app.cur)?;
    sc.drawables.into_iter().find(|d| d.id == id).and_then(|d| match d.kind {
        kn_model::DrawableKind::Table(t) => Some(t),
        _ => None,
    })
}

/// The selected range, else the cursor cell alone.
pub fn range(app: &App) -> Option<Range> {
    if let Some(r) = app.canvas.table_range {
        return Some(r);
    }
    let (id, r, c) = crate::canvas::table_cursor(app)?;
    Some((id, r, c, r, c))
}

fn cell(t: &kn_model::TableData, r: u32, c: u32) -> String {
    t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default()
}

pub fn run(app: &mut App, name: &str) -> bool {
    let Some((id, r0, c0, r1, c1)) = range(app) else { return false };
    let Some(t) = table(app, id) else { return false };
    let (cr, cc) = crate::canvas::table_cursor(app).map(|(_, r, c)| (r, c)).unwrap_or((r0, c0));
    let last_row = t.rows.saturating_sub(1);
    let last_col = t.cols.saturating_sub(1);
    let filled_in_row = |r: u32| (0..t.cols).filter(|&c| !cell(&t, r, c).trim().is_empty()).collect::<Vec<u32>>();
    let filled_in_col = |c: u32| (0..t.rows).filter(|&r| !cell(&t, r, c).trim().is_empty()).collect::<Vec<u32>>();
    let set_range = |app: &mut App, r: Range| app.canvas.table_range = Some(r);
    match name {
        "Select > Row" => set_range(app, (id, cr, 0, cr, last_col)),
        "Select > Column" => set_range(app, (id, 0, cc, last_row, cc)),
        "Select > First Value in Row" | "Select > Last Value in Row" | "Select > Extend to First Value in Row" | "Select > Extend to Last Value in Row" => {
            let f = filled_in_row(cr);
            let target = if name.contains("First") { f.first().copied().unwrap_or(0) } else { f.last().copied().unwrap_or(last_col) };
            if name.contains("Extend") {
                set_range(app, (id, cr, cc.min(target), cr, cc.max(target)));
            } else {
                app.canvas.table_cursor = Some((id, cr, target));
                app.canvas.table_range = None;
            }
        }
        "Select > First Value in Column" | "Select > Last Value in Column" | "Select > Extend to First Value in Column" | "Select > Extend to Last Value in Column" => {
            let f = filled_in_col(cc);
            let target = if name.contains("First") { f.first().copied().unwrap_or(0) } else { f.last().copied().unwrap_or(last_row) };
            if name.contains("Extend") {
                set_range(app, (id, cr.min(target), cc, cr.max(target), cc));
            } else {
                app.canvas.table_cursor = Some((id, target, cc));
                app.canvas.table_range = None;
            }
        }
        _ => {
            // edits: build the list of (row, col, new text)
            let mut edits: Vec<(u32, u32, String)> = Vec::new();
            match name {
                "Clear All" => {
                    for r in r0..=r1 {
                        for c in c0..=c1 {
                            edits.push((r, c, String::new()));
                        }
                    }
                }
                "Autofill from Row Above" if cr > 0 => edits.push((cr, cc, cell(&t, cr - 1, cc))),
                "Autofill from Column Before" if cc > 0 => edits.push((cr, cc, cell(&t, cr, cc - 1))),
                "Autofill Right" | "Autofill Left" | "Autofill Down" | "Autofill Up" | "Autofill" => {
                    // the first cell(s) on the side the fill starts from set the pattern: two
                    // numbers continue their step, anything else is copied
                    let (along_rows, reverse) = match name {
                        "Autofill Down" | "Autofill" if r1 > r0 => (true, false),
                        "Autofill Up" => (true, true),
                        "Autofill Left" => (false, true),
                        "Autofill Down" => (true, false),
                        _ => (false, false),
                    };
                    let lines: Vec<u32> = if along_rows { (c0..=c1).collect() } else { (r0..=r1).collect() };
                    for line in lines {
                        let mut idx: Vec<u32> = if along_rows { (r0..=r1).collect() } else { (c0..=c1).collect() };
                        if reverse {
                            idx.reverse();
                        }
                        let get = |k: u32| if along_rows { cell(&t, k, line) } else { cell(&t, line, k) };
                        let first = get(idx[0]);
                        let step = (idx.len() > 2).then(|| (first.trim().parse::<f64>().ok(), get(idx[1]).trim().parse::<f64>().ok())).and_then(|(a, b)| Some((a?, b? - a?)));
                        for (n, &k) in idx.iter().enumerate().skip(if step.is_some() { 2 } else { 1 }) {
                            let v = match step {
                                Some((a, d)) => {
                                    let x = a + d * n as f64;
                                    if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x}") }
                                }
                                None => first.clone(),
                            };
                            edits.push(if along_rows { (k, line, v) } else { (line, k, v) });
                        }
                    }
                }
                _ => return false,
            }
            if edits.is_empty() {
                return true;
            }
            let slide = app.current_slide().map(|s| s.slide);
            let mut err = None;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, name);
                for (r, c, v) in edits {
                    if let Err(e) = doc.set_cell(&mut app.hist, id, r, c, &CellInput::parse(&v)) {
                        err = Some(e);
                        break;
                    }
                }
                doc.end(&mut app.hist);
            }
            if let Some(e) = err {
                app.notify(format!("{e}"));
            }
            if let Some(s) = slide {
                app.bump(s);
            }
        }
    }
    true
}

/// Delete with table cells selected: empty them (the table stays).
pub fn clear_cells(app: &mut App) -> bool {
    let Some((id, r0, c0, r1, c1)) = range(app) else { return false };
    let Some(t) = table(app, id) else { return false };
    let Some(s) = app.current_slide().map(|s| s.slide) else { return false };
    let Some(doc) = app.doc.as_mut() else { return false };
    doc.begin(&mut app.hist, "Delete");
    for r in r0..=r1.min(t.rows.saturating_sub(1)) {
        for c in c0..=c1.min(t.cols.saturating_sub(1)) {
            if !cell(&t, r, c).is_empty() {
                let _ = doc.set_cell(&mut app.hist, id, r, c, &kn_model::tablecells::CellInput::Empty);
            }
        }
    }
    doc.end(&mut app.hist);
    app.bump(s);
    true
}

/// Whether table cells (not the table object) are what Copy / Paste act on.
pub fn cells_active(app: &App) -> bool {
    app.canvas.editing.is_none() && app.canvas.cell.is_none() && range(app).is_some()
}

/// Edit > Copy with table cells selected: their shown values (also as tab-separated text).
pub fn copy_cells(app: &mut App) -> Option<String> {
    let (id, r0, c0, r1, c1) = range(app)?;
    let t = table(app, id)?;
    let grid: Vec<Vec<String>> = (r0..=r1.min(t.rows.saturating_sub(1))).map(|r| (c0..=c1.min(t.cols.saturating_sub(1))).map(|c| cell(&t, r, c)).collect()).collect();
    let tsv = grid.iter().map(|row| row.join("\t")).collect::<Vec<_>>().join("\n");
    app.cell_clip = Some(grid);
    Some(tsv)
}

/// Edit > Paste (and Paste Formula Results) into the cells from the table cursor: the copied
/// values, or tab-separated text from the clipboard. Cells past the table's edge are left out.
pub fn paste_cells(app: &mut App, label: &str, text: Option<&str>) {
    let Some((id, r0, c0, ..)) = range(app) else { return };
    let grid: Vec<Vec<String>> = match text {
        Some(t) => t.lines().map(|l| l.split('\t').map(str::to_string).collect()).collect(),
        None => match app.cell_clip.clone() {
            Some(g) => g,
            None => return,
        },
    };
    let Some(t) = table(app, id) else { return };
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, label);
    let mut err = None;
    for (i, row) in grid.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            let (r, c) = (r0 + i as u32, c0 + j as u32);
            if r >= t.rows || c >= t.cols {
                continue;
            }
            if let Err(e) = doc.set_cell(&mut app.hist, id, r, c, &CellInput::parse(v)) {
                err = Some(e);
            }
        }
    }
    doc.end(&mut app.hist);
    if let Some(e) = err {
        app.notify(format!("Couldn't paste every cell: {e}"));
    }
    let rows = grid.len().max(1) as u32;
    let cols = grid.iter().map(Vec::len).max().unwrap_or(1).max(1) as u32;
    app.canvas.table_range = Some((id, r0, c0, (r0 + rows - 1).min(t.rows - 1), (c0 + cols - 1).min(t.cols - 1)));
    app.bump(slide);
}
