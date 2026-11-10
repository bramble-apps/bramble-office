//! Format > Table > Select / Autofill Cells and Edit > Clear All, on the cell at the table
//! cursor and the selected cell range.

use kn_format::ObjId;
use kn_model::tablecells::CellInput;
use kn_model::tableclip::{self, PasteMode, TableClip};

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

/// Edit > Copy with table cells selected: the cells with their values, formulas, formats and
/// looks (Keydeck's own copy), and their shown values as tab-separated text.
pub fn copy_cells(app: &mut App) -> Option<String> {
    let (id, r0, c0, r1, c1) = range(app)?;
    let t = table(app, id)?;
    let grid: Vec<Vec<String>> = (r0..=r1.min(t.rows.saturating_sub(1))).map(|r| (c0..=c1.min(t.cols.saturating_sub(1))).map(|c| cell(&t, r, c)).collect()).collect();
    let tsv = grid.iter().map(|row| row.join("\t")).collect::<Vec<_>>().join("\n");
    app.cell_clip = app.doc.as_ref().and_then(|d| tableclip::read(d, id, (r0, c0, r1, c1), false));
    // (one empty cell is a line break: the clipboard drops an empty paste)
    Some(if tsv.is_empty() { "\n".to_string() } else { tsv })
}

/// Puts `clip` on the system clipboard as the suite's table (HTML that Values, Letters, Notes,
/// LibreOffice and browsers read) with its text; else the text alone. Copied objects stop being
/// what a paste brings.
fn put(app: &mut App, ctx: &egui::Context, clip: Option<&TableClip>, tsv: String) {
    app.clip = None;
    if !clip.is_some_and(|c| kn_ui::clipboard::set_html(&c.to_html(), &tsv)) {
        ctx.copy_text(tsv);
    }
}

/// Copy / Cut with cells selected: [`copy_cells`] on the system clipboard.
pub fn put_cells(app: &mut App, ctx: &egui::Context) -> bool {
    let Some(tsv) = copy_cells(app) else { return false };
    let clip = app.cell_clip.clone();
    put(app, ctx, clip.as_ref(), tsv);
    true
}

/// The selected table as an object (not its cells).
pub fn table_object(app: &App) -> Option<ObjId> {
    if cells_active(app) || app.canvas.editing.is_some() {
        return None;
    }
    let [id] = app.sel[..] else { return None };
    app.doc.as_ref().is_some_and(|d| d.file.type_name(id) == "TST.TableInfoArchive").then_some(id)
}

/// Copy with a table selected: the whole table on the clipboard (pasted in Keydeck or another
/// suite app it is a new table like it).
pub fn put_table(app: &mut App, ctx: &egui::Context, id: ObjId) -> bool {
    let Some(clip) = app.doc.as_ref().and_then(|d| tableclip::read(d, id, (0, 0, 0, 0), true)) else { return false };
    let tsv = clip.tsv();
    put(app, ctx, Some(&clip), tsv);
    true
}

/// Text pasted from the system clipboard (not Keydeck's own objects): a table copied in a
/// suite app, LibreOffice or a page goes into the selected cells whole (values, formats,
/// looks, merges), or with no cells selected becomes a new table; other text goes into the
/// cells as tab-separated values, or onto the slide as a text box.
pub fn paste_text(app: &mut App, text: &str) {
    let html = kn_ui::clipboard::html().filter(|h| tableclip::has_table(h));
    if cells_active(app) {
        let clip = html.as_deref().and_then(TableClip::from_html);
        // (Keydeck's own copy: as it was taken)
        let clip = match (clip, app.cell_clip.clone()) {
            (Some(c), Some(own)) if c.ours() && c.tsv() == own.tsv() => Some(own),
            (c, _) => c,
        };
        match clip {
            Some(c) => paste_clip(app, "Paste", &c, PasteMode::All),
            None => paste_cells(app, "Paste", Some(text)),
        }
        return;
    }
    if let Some(clip) = html.and_then(|h| tableclip::table_paste(&h, text)) {
        paste_new_table(app, &clip);
        return;
    }
    crate::editing::paste_text_as_box(app, text);
}

/// Copied cells into the selected ones, as Values pastes: from one cell the whole copy goes in
/// (the table grows to take it), a selected range is filled by repeating it; formats, looks,
/// borders and merges come along (Paste Formula Results: the shown values).
pub fn paste_clip(app: &mut App, label: &str, clip: &TableClip, mode: PasteMode) {
    let Some((id, r0, c0, r1, c1)) = range(app) else { return };
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, label);
    let res = tableclip::write(doc, &mut app.hist, id, (r0, c0, r1, c1), clip, mode);
    doc.end(&mut app.hist);
    match res {
        Ok((a, b, c, d)) => app.canvas.table_range = Some((id, a, b, c, d)),
        Err(e) => app.notify(format!("Couldn't paste every cell: {e}")),
    }
    app.bump(slide);
}

/// A table from the clipboard as a new table on the slide, as copied (size, cells, column
/// widths, row heights): one undo step.
pub fn paste_new_table(app: &mut App, clip: &TableClip) {
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    app.canvas.cancel();
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, "Paste");
    // (Keynote's Basic table, then made the copy's size)
    let res = doc.insert(&mut app.hist, slide, &kn_model::insert::NewObject::BASIC_TABLE).and_then(|id| tableclip::write_new(doc, &mut app.hist, id, clip).map(|_| id));
    doc.end(&mut app.hist);
    match res {
        Ok(id) => app.sel = vec![id],
        Err(e) => app.notify(format!("Couldn't paste the table: {e}")),
    }
    app.bump(slide);
}

/// Edit > Paste (and Paste Formula Results) into the cells from the table cursor: the copied
/// cells, or tab-separated text from the clipboard. Cells past the table's edge are left out.
pub fn paste_cells(app: &mut App, label: &str, text: Option<&str>) {
    let Some((id, r0, c0, ..)) = range(app) else { return };
    let grid: Vec<Vec<String>> = match text {
        Some(t) => t.lines().map(|l| l.split('\t').map(str::to_string).collect()).collect(),
        None => {
            if let Some(clip) = app.cell_clip.clone() {
                paste_clip(app, label, &clip, if label == "Paste Formula Results" { PasteMode::Results } else { PasteMode::All });
            }
            return;
        }
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

#[cfg(test)]
mod clip_tests {
    use super::*;
    use kn_model::format::{FillSpec, TextProp};
    use kn_model::{Color, Fill};

    static TABLE_DOC: &[u8] = include_bytes!("../../kn-model/data/donors/R2-table-values.key");

    /// A Keydeck window on a presentation with a table on its slide (no window), its settings in
    /// a scratch folder.
    fn app() -> (App, ObjId) {
        static ENV: std::sync::Once = std::sync::Once::new();
        ENV.call_once(|| {
            let dir = std::env::temp_dir().join(format!("keydeck-clip-tests-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            // SAFETY: set once, before any test reads the environment through App
            unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        });
        let mut a = App::with_ctx(&egui::Context::default(), Vec::new());
        let d = kn_model::Document::from_file(kn_format::KeyFile::from_zip(TABLE_DOC).unwrap());
        let t = d.file.objects_of("TST.TableInfoArchive").next().unwrap().0;
        a.new_document(d);
        (a, t)
    }

    fn red() -> Color {
        Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
    }

    /// Cells copied in Keydeck and pasted through the clipboard's HTML (as another Keydeck,
    /// Letters or Values sends them) and as Keydeck's own copy give the same cells.
    #[test]
    fn own_copy_and_html_paste_alike() {
        let (mut a, t) = app();
        let doc = a.doc.as_mut().unwrap();
        let mut h = kn_model::edit::History::default();
        doc.begin(&mut h, "t");
        tableclip::resize(doc, &mut h, t, 6, 3).unwrap();
        doc.set_cell(&mut h, t, 1, 0, &CellInput::Text("a".into())).unwrap();
        doc.set_cell(&mut h, t, 1, 1, &CellInput::Number(3.0)).unwrap();
        doc.set_cells_fill(&mut h, t, (1, 0, 1, 0), Some(&FillSpec::Color(red()))).unwrap();
        doc.set_cells_text_prop(&mut h, t, (1, 1, 1, 1), &TextProp::Bold(true)).unwrap();
        doc.end(&mut h);
        a.canvas.table_range = Some((t, 1, 0, 1, 1));
        assert!(copy_cells(&mut a).is_some());
        let html = a.cell_clip.as_ref().unwrap().to_html();
        // Keydeck's own copy into row 3, the HTML into row 4
        a.canvas.table_range = Some((t, 3, 0, 3, 0));
        paste_cells(&mut a, "Paste", None);
        a.canvas.table_range = Some((t, 4, 0, 4, 0));
        paste_clip(&mut a, "Paste", &TableClip::from_html(&html).unwrap(), PasteMode::All);
        let d = a.doc.as_ref().unwrap();
        let td = kn_model::table::table(&d.file, d.file.obj(t).unwrap()).unwrap();
        let at = |r: u32, c: u32| (td.cells[(r * td.cols + c) as usize].clone(), td.cell_styles[(r * td.cols + c) as usize].clone());
        // (the table is banded: a row's own band comes with where it lands)
        let mine = |r: u32, c: u32| {
            let (v, st) = at(r, c);
            (v, st.text.bold, st.text.italic, st.align)
        };
        for c in 0..2 {
            assert_eq!(mine(3, c), mine(1, c));
            assert_eq!(mine(4, c), mine(1, c));
        }
        assert_eq!(at(3, 0).1.fill, Fill::Color(red()));
        assert_eq!(at(4, 0).1.fill, Fill::Color(red()));
        assert!(at(4, 1).1.text.bold);
        assert_eq!(a.canvas.table_range, Some((t, 4, 0, 4, 1)));
    }

    /// A table pasted with no cells selected: a new table on the slide, one undo step.
    #[test]
    fn new_table_from_html() {
        let (mut a, _) = app();
        let before = a.doc.as_ref().unwrap().file.objects_of("TST.TableInfoArchive").count();
        let clip = TableClip::from_html("<table><tr><td><b>x</b></td><td>2</td><td>3</td></tr></table>").unwrap();
        paste_new_table(&mut a, &clip);
        assert!(a.toast.is_none(), "{:?}", a.toast.as_ref().map(|t| &t.0));
        let d = a.doc.as_ref().unwrap();
        assert_eq!(d.file.objects_of("TST.TableInfoArchive").count(), before + 1);
        let id = a.sel[0];
        let td = kn_model::table::table(&d.file, d.file.obj(id).unwrap()).unwrap();
        assert_eq!((td.rows, td.cols), (1, 3));
        assert_eq!(td.cells[0], "x");
        assert!(td.cell_styles[0].text.bold);
        let doc = a.doc.as_mut().unwrap();
        assert!(doc.undo(&mut a.hist));
        assert_eq!(a.doc.as_ref().unwrap().file.objects_of("TST.TableInfoArchive").count(), before);
    }
}
