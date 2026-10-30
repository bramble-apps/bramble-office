//! Opening Excel workbooks (.xlsx): each worksheet becomes a sheet with one table holding its
//! cells (shared strings, numbers, booleans, inline strings; formulas through Keydeck's parser,
//! else their cached result), with their look (xlstyle.rs): fonts, fills, borders, alignment,
//! column widths, row heights, merged cells and frozen header rows / columns. Charts are not
//! carried over yet.
//!
//! And File > Export To > Excel: a worksheet per table (named after the sheet, or "Sheet -
//! Table" when a sheet has several), with values (numbers, dates, durations, true / false and
//! text; formulas as their results), merged cells, column widths and row heights.

use crate::import::Field;

fn bad(s: impl Into<String>) -> kn_format::Error {
    kn_format::Error::Invalid(s.into())
}

fn read(z: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = z.by_name(name).ok()?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).ok()?;
    Some(s)
}

/// "B12" -> (row 11, col 1).
fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let split = r.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = r.split_at(split);
    let col = letters.bytes().try_fold(0usize, |a, b| b.is_ascii_uppercase().then(|| a * 26 + (b - b'A') as usize + 1))?;
    let row: usize = digits.parse().ok()?;
    (col > 0 && row > 0).then(|| (row - 1, col - 1))
}

/// Text of an <si> / <is> element: its <t> runs joined.
fn text_of(n: roxmltree::Node) -> String {
    n.descendants().filter(|d| d.has_tag_name("t")).filter_map(|t| t.text()).collect()
}

/// (sheet name, rows of fields) for every worksheet, in workbook order.
pub fn read_xlsx(bytes: &[u8]) -> Result<Vec<(String, Vec<Vec<Field>>)>, kn_format::Error> {
    Ok(read_xlsx_full(bytes)?.0.into_iter().map(|s| (s.name, s.rows)).collect())
}

/// A worksheet: its name, cells and layout.
pub struct XSheet {
    /// What its cells have that Numbers can't hold (rotation, diagonal lines, links).
    pub extras: std::collections::HashMap<(u32, u32), crate::xlstyle::Extra>,
    pub name: String,
    pub rows: Vec<Vec<Field>>,
    pub layout: crate::xlstyle::XLayout,
}

/// Every worksheet with its layout, and the workbook's cell styles.
pub fn read_xlsx_full(bytes: &[u8]) -> Result<(Vec<XSheet>, Vec<crate::xlstyle::XStyle>), kn_format::Error> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let wb_xml = read(&mut z, "xl/workbook.xml").ok_or(bad("not an Excel workbook (no xl/workbook.xml)"))?;
    let rels_xml = read(&mut z, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let strings_xml = read(&mut z, "xl/sharedStrings.xml").unwrap_or_default();
    let rels_doc = roxmltree::Document::parse(&rels_xml).ok();
    let target = |id: &str| -> Option<String> {
        let t = rels_doc.as_ref()?.descendants().find(|n| n.has_tag_name("Relationship") && n.attribute("Id") == Some(id))?.attribute("Target")?;
        Some(if let Some(abs) = t.strip_prefix('/') { abs.to_string() } else { format!("xl/{t}") })
    };
    let theme = read(&mut z, "xl/theme/theme1.xml").map(|t| crate::xlstyle::theme_colors(&t)).unwrap_or_default();
    let strings_doc = roxmltree::Document::parse(&strings_xml).ok();
    let sis: Vec<roxmltree::Node> = strings_doc.as_ref().map(|d| d.root_element().children().filter(|n| n.has_tag_name("si")).collect()).unwrap_or_default();
    let strings: Vec<String> = sis.iter().map(|n| text_of(*n)).collect();
    let string_fonts: Vec<Option<crate::xlstyle::RunFont>> = sis.iter().map(|n| crate::xlstyle::RunFont::of(*n, &theme)).collect();

    let styles = read(&mut z, "xl/styles.xml").map(|x| crate::xlstyle::read_styles(&x, &theme)).unwrap_or_default();
    let wb = roxmltree::Document::parse(&wb_xml).map_err(|e| bad(format!("workbook.xml: {e}")))?;
    let mut out = Vec::new();
    for s in wb.descendants().filter(|n| n.has_tag_name("sheet")) {
        let name = s.attribute("name").unwrap_or("Sheet").to_string();
        let rid = s.attributes().find(|a| a.name() == "id").map(|a| a.value()).unwrap_or("");
        let path = target(rid).unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", out.len() + 1));
        let Some(xml) = read(&mut z, &path) else { continue };
        let doc = roxmltree::Document::parse(&xml).map_err(|e| bad(format!("{path}: {e}")))?;
        // the sheet's links: <hyperlinks> -> r:id in its rels (or a place in the workbook)
        let rels_path = match path.rsplit_once('/') {
            Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
            None => format!("_rels/{path}.rels"),
        };
        let sheet_rels = read(&mut z, &rels_path).unwrap_or_default();
        let links: Vec<((u32, u32), String)> = {
            let rd = roxmltree::Document::parse(&sheet_rels).ok();
            doc.descendants()
                .filter(|n| n.has_tag_name("hyperlink"))
                .filter_map(|h| {
                    let at = crate::xlstyle::cell_ref_pub(h.attribute("ref")?.split(':').next()?)?;
                    let rid = h.attributes().find(|a| a.name() == "id").map(|a| a.value());
                    let target = match rid {
                        Some(id) => rd.as_ref()?.descendants().find(|n| n.attribute("Id") == Some(id))?.attribute("Target")?.to_string(),
                        None => format!("#{}", h.attribute("location")?),
                    };
                    Some((at, target))
                })
                .collect()
        };
        let mut rows: Vec<Vec<Field>> = Vec::new();
        let mut run_font: std::collections::HashMap<(u32, u32), crate::xlstyle::RunFont> = Default::default();
        for (auto_r, row) in doc.descendants().filter(|n| n.has_tag_name("row")).enumerate() {
            let r = row.attribute("r").and_then(|v| v.parse::<usize>().ok()).map(|v| v - 1).unwrap_or(auto_r);
            for (auto_c, c) in row.children().filter(|n| n.has_tag_name("c")).enumerate() {
                let (rr, cc) = c.attribute("r").and_then(cell_ref).unwrap_or((r, auto_c));
                if rr > 9999 || cc > 254 {
                    continue;
                }
                let v = c.children().find(|n| n.has_tag_name("v")).and_then(|v| v.text()).unwrap_or("");
                let shown = match c.attribute("t") {
                    Some("s") => v.parse::<usize>().ok().and_then(|i| strings.get(i).cloned()).unwrap_or_default(),
                    Some("inlineStr") => c.children().find(|n| n.has_tag_name("is")).map(text_of).unwrap_or_default(),
                    Some("b") => if v == "1" { "TRUE" } else { "FALSE" }.to_string(),
                    Some("e") => v.to_string(),
                    // a number with a date format is a date: shown as the format shows it
                    _ => match (v.parse::<f64>(), c.attribute("s").and_then(|i| i.parse::<usize>().ok()).and_then(|i| styles.get(i))) {
                        (Ok(x), Some(st)) if st.is_date() => crate::xlstyle::format_date(x, st.numfmt.as_deref().unwrap_or("")),
                        _ => v.to_string(),
                    },
                };
                let date = c.attribute("s").and_then(|i| i.parse::<usize>().ok()).and_then(|i| styles.get(i)).is_some_and(|st| st.is_date());
                // (a date comes in as the text it shows, so a formula working on dates would
                // calculate from that text: its saved result is kept instead)
                let formula = c.children().find(|n| n.has_tag_name("f")).and_then(|f| f.text()).filter(|f| !f.is_empty() && !date).map(|f| format!("={f}"));
                // a cell with only a style (spreadsheets format whole columns, often far past the
                // data) doesn't make the table bigger
                if shown.is_empty() && formula.is_none() {
                    continue;
                }
                let rf = match c.attribute("t") {
                    Some("s") => v.parse::<usize>().ok().and_then(|i| string_fonts.get(i).cloned().flatten()),
                    Some("inlineStr") => c.children().find(|n| n.has_tag_name("is")).and_then(|n| crate::xlstyle::RunFont::of(n, &theme)),
                    _ => None,
                };
                if let Some(rf) = rf {
                    run_font.insert((rr as u32, cc as u32), rf);
                }
                if rows.len() <= rr {
                    rows.resize(rr + 1, Vec::new());
                }
                if rows[rr].len() <= cc {
                    rows[rr].resize(cc + 1, Field::default());
                }
                // a date (no formula): the value as a date cell with the format as a pattern
                let date = match (formula.is_none() && date, v.parse::<f64>()) {
                    (true, Ok(x)) => c.attribute("s").and_then(|i| i.parse::<usize>().ok()).and_then(|i| styles.get(i)).and_then(|st| st.numfmt.as_deref()).map(|code| ((x - 36892.0) * 86400.0, crate::xlstyle::icu_pattern(code))),
                    _ => None,
                };
                rows[rr][cc] = Field { shown, formula, date };
            }
        }
        // (the table's size: the cells with values, those with a fill or borders near them (a
        // workbook can format whole columns far past its data: font-only cells don't count),
        // and merges)
        let max = (rows.len().saturating_sub(1) as u32, rows.iter().map(Vec::len).max().unwrap_or(1).saturating_sub(1) as u32);
        let mut layout = crate::xlstyle::read_layout(&doc, (max.0 + 256, max.1 + 64));
        let shown = layout.cell_style.iter().filter(|(_, i)| styles.get(**i as usize).is_some_and(|st| st.fill.is_some() || st.gradient.is_some() || st.borders.iter().any(Option::is_some) || st.diag.is_some())).map(|(rc, _)| *rc).fold(max, |m, (r, c)| (m.0.max(r), m.1.max(c)));
        let max = (shown.0.max(links.iter().map(|((r, _), _)| *r).max().unwrap_or(0)), shown.1.max(links.iter().map(|((_, c), _)| *c).max().unwrap_or(0)));
        layout.run_font = run_font;
        layout.merges.retain(|(r, c, _, _)| *r <= max.0 && *c <= max.1);
        while rows.len() <= max.0 as usize {
            rows.push(Vec::new());
        }
        if let Some(first) = rows.first_mut() {
            if first.len() <= max.1 as usize {
                first.resize(max.1 as usize + 1, Field::default());
            }
        }
        let need = layout.merges.iter().fold(max, |m, (r, c, rr, cc)| (m.0.max(r + rr - 1), m.1.max(c + cc - 1)));
        while rows.len() <= need.0 as usize {
            rows.push(Vec::new());
        }
        if let Some(first) = rows.first_mut() {
            if first.len() <= need.1 as usize {
                first.resize(need.1 as usize + 1, Field::default());
            }
        }
        let mut extras: std::collections::HashMap<(u32, u32), crate::xlstyle::Extra> = std::collections::HashMap::new();
        for (rc, si) in &layout.cell_style {
            let Some(st) = styles.get(*si as usize) else { continue };
            if (st.rotation != 0 || st.diag.is_some()) && rc.0 as usize <= max.0 as usize && rc.1 <= max.1 {
                let e = extras.entry(*rc).or_default();
                e.rotation = st.rotation;
                e.diag = st.diag.clone();
            }
        }
        for (rc, url) in links {
            extras.entry(rc).or_default().link = Some(url);
        }
        out.push(XSheet { extras, name, rows, layout });
    }
    if out.is_empty() {
        return Err(bad("the workbook has no worksheets"));
    }
    Ok((out, styles))
}

/// Open an .xlsx as a new workbook (one sheet per worksheet, tables named "Table 1").
pub fn import_xlsx(path: &std::path::Path) -> Result<crate::Workbook, kn_format::Error> {
    let bytes = std::fs::read(path)?;
    let (sheets, styles) = read_xlsx_full(&bytes)?;
    let named: Vec<(String, String, Vec<Vec<Field>>)> = sheets.iter().map(|s| (s.name.clone(), "Table 1".to_string(), s.rows.clone())).collect();
    let mut wb = crate::import::workbook_from_sheets(&named)?;
    // the look: sizes, merges, headers and cell styles, as Numbers opens a workbook
    let mut h = kn_model::edit::History::default();
    wb.doc.begin(&mut h, "Import");
    let ids: Vec<_> = wb.sheets().iter().map(|s| wb.tables(s).first().map(|t| t.info)).collect();
    for (x, info) in sheets.iter().zip(ids) {
        if let Some(info) = info {
            let size = (x.rows.len() as u32, x.rows.iter().map(Vec::len).max().unwrap_or(1) as u32);
            apply_look(&mut wb.doc, &mut h, info, &x.layout, &styles, size);
        }
    }
    wb.doc.end(&mut h);
    Ok(wb)
}

/// An imported worksheet's look on its table (`size`: its rows and columns).
fn apply_look(doc: &mut kn_model::Document, h: &mut kn_model::edit::History, t: kn_format::ObjId, l: &crate::xlstyle::XLayout, styles: &[crate::xlstyle::XStyle], size: (u32, u32)) {
    use kn_model::format::{FillSpec, TextProp};
    use kn_model::tablefmt::BorderEdges;
    use kn_model::tablelook::Gridline;
    use kn_model::tablerows::HeaderKind;
    let (nr, nc) = size;
    let t0 = std::time::Instant::now();
    let timing = std::env::var_os("NM_XLSX_TIMING").is_some();
    let lap = |what: &str| if timing { eprintln!("{what}: {:?}", t0.elapsed()) };
    // header rows and columns: the frozen ones (Numbers exports its headers as frozen panes),
    // fewer if a merged cell would cross the header's edge (Numbers never merges across it)
    let mut hr = l.frozen_rows.min(nr.saturating_sub(1));
    let mut hc = l.frozen_cols.min(nc.saturating_sub(1));
    while let Some((r, ..)) = l.merges.iter().find(|(r, _, rr, _)| *r < hr && r + rr > hr) {
        hr = *r;
    }
    while let Some((_, c, ..)) = l.merges.iter().find(|(_, c, _, cc)| *c < hc && c + cc > hc) {
        hc = *c;
    }
    let _ = doc.set_table_header_count(h, t, HeaderKind::Rows, hr);
    let _ = doc.set_table_header_count(h, t, HeaderKind::Columns, hc);
    let l = &crate::xlstyle::XLayout { frozen_rows: hr, frozen_cols: hc, ..l.clone() };
    // a worksheet has no table name to show
    doc.set_table_name_shown(h, t, false);
    // Excel's grid isn't part of the document: only the cells' own borders show
    for g in [Gridline::BodyRows, Gridline::BodyColumns, Gridline::HeaderRow, Gridline::HeaderColumn, Gridline::Footer] {
        let _ = doc.set_table_gridline(h, t, g, false);
    }
    let _ = doc.set_table_outline(h, t, None);
    // sizes
    let cols: Vec<(u32, f32)> = (0..nc).filter_map(|c| l.widths.get(&c).copied().or(l.default_width).map(|w| (c, w))).collect();
    if !cols.is_empty() {
        doc.set_table_sizes(h, t, false, &cols);
    }
    let rows: Vec<(u32, f32)> = (0..nr).filter_map(|r| l.heights.get(&r).copied().or(l.default_height).map(|v| (r, v.max(8.0)))).collect();
    if !rows.is_empty() {
        doc.set_table_sizes(h, t, true, &rows);
    }
    for (r, c, rr, cc) in &l.merges {
        let _ = doc.merge_cells(h, t, kn_model::CellRange { row: *r, col: *c, rows: *rr, cols: *cc });
    }
    lap("sizes, merges");
    // styles: each property over the largest blocks of cells that share its value (one table
    // edit per block, not per cell)
    let default = crate::xlstyle::XStyle::default();
    // each cell's style, with the font of its text's runs over it
    let eff: Vec<crate::xlstyle::XStyle> = (0..nr)
        .flat_map(|r| (0..nc).map(move |c| (r, c)))
        .map(|(r, c)| {
            let st = styles.get(l.cell_style.get(&(r, c)).copied().unwrap_or(0) as usize).unwrap_or(&default);
            l.run_font.get(&(r, c)).map(|f| f.over(st)).unwrap_or_else(|| st.clone())
        })
        .collect();
    let style = |r: u32, c: u32| &eff[(r * nc + c) as usize];
    let header = |r: u32, c: u32| r < l.frozen_rows || c < l.frozen_cols;
    let fills = blocks(nr, nc, |r, c| match (style(r, c).gradient, style(r, c).fill) {
        (Some(g), _) => Some(format!("{g:?}")),
        (None, Some(f)) => Some(format!("{f:?}")),
        // headers are filled by the table style: Excel's plain cell is plain
        (None, None) if header(r, c) => Some("none".into()),
        (None, None) => None,
    });
    for (k, b) in fills {
        let st = style(b.0, b.1);
        let f = match (st.gradient, st.fill) {
            (Some((from, to, angle)), _) => FillSpec::Gradient { from, to, angle },
            (None, Some(c)) => FillSpec::Color(c),
            (None, None) => FillSpec::None,
        };
        let _ = k;
        let _ = doc.set_cells_fill(h, t, b, Some(&f));
    }
    lap("fills");
    // number formats (dates came in as date cells with their pattern; General stays Automatic)
    let fmt = |r: u32, c: u32| style(r, c).numfmt.as_deref().and_then(excel_cell_format);
    for (_, b) in blocks(nr, nc, |r, c| fmt(r, c).map(|f| format!("{f:?}"))) {
        if let Some(f) = fmt(b.0, b.1) {
            let _ = doc.set_cells_format(h, t, b, &f);
        }
    }
    lap("number formats");
    let props: Vec<(&str, Box<dyn Fn(&crate::xlstyle::XStyle, bool) -> Option<TextProp>>)> = vec![
        ("font", Box::new(|s, _| s.font.clone().map(TextProp::Font))),
        ("size", Box::new(|s, _| s.size.map(TextProp::Size))),
        ("bold", Box::new(|s, hd| (s.bold || hd).then_some(TextProp::Bold(s.bold)))),
        ("italic", Box::new(|s, _| s.italic.then_some(TextProp::Italic(true)))),
        ("underline", Box::new(|s, _| s.underline.then_some(TextProp::Underline(true)))),
        ("strike", Box::new(|s, _| s.strike.then_some(TextProp::Strike(true)))),
        ("color", Box::new(|s, _| s.color.map(TextProp::Color))),
    ];
    for (name, f) in &props {
        lap(name);
        for (_, b) in blocks(nr, nc, |r, c| f(style(r, c), header(r, c)).map(|p| format!("{p:?}"))) {
            if let Some(p) = f(style(b.0, b.1), header(b.0, b.1)) {
                let _ = doc.set_cells_text_prop(h, t, b, &p);
            }
        }
    }
    lap("text props");
    for (_, b) in blocks(nr, nc, |r, c| style(r, c).halign.map(|a| format!("{a:?}"))) {
        if let Some(a) = style(b.0, b.1).halign {
            let _ = doc.set_cells_alignment(h, t, b, a);
        }
    }
    // vertical alignment: Excel's default is the bottom
    let valign = |r: u32, c: u32| style(r, c).valign.unwrap_or(kn_model::VAlign::Bottom);
    for (_, b) in blocks(nr, nc, |r, c| Some(format!("{:?}", valign(r, c)))) {
        let _ = doc.set_cells_valign(h, t, b, valign(b.0, b.1));
    }
    for (_, b) in blocks(nr, nc, |r, c| style(r, c).wrap.then(|| "wrap".to_string())) {
        let _ = doc.set_cells_wrap(h, t, b, true);
    }
    lap("align, wrap");
    // borders: each shared edge is one line in Numbers, Excel's two cells each may give one
    // (the upper cell's bottom, the lower's top): the heavier is drawn, as Excel and OnlyOffice
    // show it. Then equal lines go on in blocks: a block's top and the lines between its rows,
    // its left and the lines between its columns
    let heavier = |a: Option<&crate::xlstyle::Edge>, b: Option<&crate::xlstyle::Edge>| -> Option<crate::xlstyle::Edge> {
        match (a, b) {
            (Some(x), Some(y)) => Some(if y.width > x.width { y.clone() } else { x.clone() }),
            (x, y) => x.or(y).cloned(),
        }
    };
    // horizontal edge b (0..=nr) at column c; vertical edge b (0..=nc) in row r
    let h_edge = |b: u32, c: u32| heavier(if b > 0 { style(b - 1, c).borders[3].as_ref() } else { None }, if b < nr { style(b, c).borders[2].as_ref() } else { None });
    let v_edge = |r: u32, b: u32| heavier(if b > 0 { style(r, b - 1).borders[1].as_ref() } else { None }, if b < nc { style(r, b).borders[0].as_ref() } else { None });
    // the top of rows 0..nr, and the bottom of the last row
    for (_, b) in blocks(nr, nc, |r, c| h_edge(r, c).map(|e| format!("{e:?}"))) {
        let Some(e) = h_edge(b.0, b.1) else { continue };
        let _ = doc.set_cells_border(h, t, b, BorderEdges::Top, Some(&e.stroke()));
        if b.2 > b.0 {
            let _ = doc.set_cells_border(h, t, b, BorderEdges::Middle, Some(&e.stroke()));
        }
    }
    for (_, b) in blocks(1, nc, |_, c| h_edge(nr, c).map(|e| format!("{e:?}"))) {
        if let Some(e) = h_edge(nr, b.1) {
            let _ = doc.set_cells_border(h, t, (nr - 1, b.1, nr - 1, b.3), BorderEdges::Bottom, Some(&e.stroke()));
        }
    }
    for (_, b) in blocks(nr, nc, |r, c| v_edge(r, c).map(|e| format!("{e:?}"))) {
        let Some(e) = v_edge(b.0, b.1) else { continue };
        let _ = doc.set_cells_border(h, t, b, BorderEdges::Left, Some(&e.stroke()));
        if b.3 > b.1 {
            let _ = doc.set_cells_border(h, t, b, BorderEdges::Center, Some(&e.stroke()));
        }
    }
    for (_, b) in blocks(nr, 1, |r, _| v_edge(r, nc).map(|e| format!("{e:?}"))) {
        if let Some(e) = v_edge(b.0, nc) {
            let _ = doc.set_cells_border(h, t, (b.0, nc - 1, b.2, nc - 1), BorderEdges::Right, Some(&e.stroke()));
        }
    }
    lap("styles");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal workbook as Excel writes one (two sheets, shared strings, a formula).
    fn sample() -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default();
        let files = [
            ("xl/workbook.xml", r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId2"/></sheets></workbook>"#),
            ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#),
            ("xl/sharedStrings.xml", r#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Item</t></si><si><t>Cost</t></si><si><r><t>Re</t></r><r><t>nt</t></r></si></sst>"#),
            ("xl/worksheets/sheet1.xml", r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>1200</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Food</t></is></c><c r="B3"><v>350.5</v></c></row><row r="4"><c r="B4"><f>SUM(B2:B3)</f><v>1550.5</v></c></row></sheetData></worksheet>"#),
            ("xl/worksheets/sheet2.xml", r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="b"><v>1</v></c><c r="C1"><f>Budget!B4*2</f><v>3101</v></c></row></sheetData></worksheet>"#),
        ];
        for (n, body) in files {
            z.start_file(n, o).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn number_formats_from_excel() {
        use kn_model::tablefmt::CellFormat as F;
        assert_eq!(excel_cell_format("0%"), Some(F::Percent { decimals: Some(0), thousands: false, negative: 0 }));
        assert_eq!(excel_cell_format("0.00%"), Some(F::Percent { decimals: Some(2), thousands: false, negative: 0 }));
        assert_eq!(excel_cell_format("\"$\"#,##0.00"), Some(F::Currency { code: "USD".into(), decimals: Some(2), thousands: true, negative: 0, accounting: false }));
        assert_eq!(excel_cell_format("$0.00"), Some(F::Currency { code: "USD".into(), decimals: Some(2), thousands: false, negative: 0, accounting: false }));
        assert_eq!(excel_cell_format("[$€-407] #,##0"), Some(F::Currency { code: "EUR".into(), decimals: Some(0), thousands: true, negative: 0, accounting: false }));
        assert_eq!(excel_cell_format("#,##0;[Red]-#,##0"), Some(F::Number { decimals: Some(0), thousands: true, negative: 0 }));
        assert_eq!(excel_cell_format("0.00E+00"), Some(F::Scientific { decimals: Some(2) }));
        assert_eq!(excel_cell_format("@"), Some(F::Text));
        assert_eq!(excel_cell_format("General"), None);
        assert_eq!(excel_cell_format("m/d/yyyy"), None);
        // and back: what Values writes reads as the same format
        for f in [F::Percent { decimals: Some(1), thousands: false, negative: 0 }, F::Currency { code: "GBP".into(), decimals: Some(2), thousands: true, negative: 0, accounting: false }] {
            assert_eq!(excel_cell_format(&excel_number_format(&f).unwrap()), Some(f));
        }
    }

    #[test]
    fn formulas_in_excel_operators() {
        assert_eq!(excel_formula("B3×2÷C1"), "B3*2/C1");
        assert_eq!(excel_formula("IF(A1≠2,\"a×b\",A1≥3)"), "IF(A1<>2,\"a×b\",A1>=3)");
    }

    #[test]
    fn reads_and_imports() {
        let sheets = read_xlsx(&sample()).unwrap();
        assert_eq!(sheets.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["Budget", "Notes"]);
        assert_eq!(sheets[0].1[1][0].shown, "Rent");
        assert_eq!(sheets[0].1[3][1].formula.as_deref(), Some("=SUM(B2:B3)"));
        let named: Vec<_> = sheets.into_iter().map(|(n, r)| (n, "Table 1".to_string(), r)).collect();
        let mut wb = crate::import::workbook_from_sheets(&named).unwrap();
        let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/n-edits"));
        std::fs::create_dir_all(&dir).unwrap();
        wb.doc.save_as(dir.join("N25-xlsx-import.numbers")).unwrap();
        let back = crate::Workbook::open(dir.join("N25-xlsx-import.numbers")).unwrap();
        let s = back.sheets();
        assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Budget", "Notes"]);
        let t = back.table_data(back.tables(&s[0])[0].info).unwrap();
        assert_eq!(t.cells[(3 * t.cols + 1) as usize], "1550.5");
        assert!(back.doc.formula_text(back.tables(&s[0])[0].info, 3, 1).is_some(), "SUM stays a formula");
        // a cross-sheet Excel formula keeps its cached value
        let t2 = back.table_data(back.tables(&s[1])[0].info).unwrap();
        assert_eq!(t2.cells[2], "3101");
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Column `c` (0-based) as Excel letters.
fn col_letters(mut c: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// A worksheet name Excel accepts: at most 31 characters, none of []:*?/\, unique.
fn sheet_name(want: &str, used: &mut Vec<String>) -> String {
    let clean: String = want.chars().map(|c| if "[]:*?/\\".contains(c) { '-' } else { c }).collect();
    let clean = clean.trim().trim_matches('\'').to_string();
    let base: String = if clean.is_empty() { "Sheet".into() } else { clean.chars().take(31).collect() };
    let mut name = base.clone();
    let mut n = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
        let tail = format!(" {n}");
        name = format!("{}{tail}", base.chars().take(31 - tail.chars().count()).collect::<String>());
        n += 1;
    }
    used.push(name.clone());
    name
}

/// A Numbers (ICU) date pattern as an Excel number format: "MMM d, yyyy" -> "mmm d, yyyy",
/// "EEEE" -> "dddd", "a" -> "AM/PM", quoted text kept.
fn excel_date_format(icu: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = icu.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        match c {
            '\'' => {
                // 'text' -> "text"
                let end = chars[i + 1..].iter().position(|x| *x == '\'').map(|p| i + 1 + p).unwrap_or(chars.len());
                let lit: String = chars[i + 1..end].iter().collect();
                out.push_str(&format!("\"{lit}\""));
                i = end + 1;
                continue;
            }
            'M' | 'L' => out.push_str(&"m".repeat(n.min(4))),
            'd' => out.push_str(&"d".repeat(n.min(2))),
            'E' => out.push_str(if n >= 4 { "dddd" } else { "ddd" }),
            'y' => out.push_str(if n == 2 { "yy" } else { "yyyy" }),
            'h' | 'K' => out.push_str(&"h".repeat(n.min(2))),
            'H' | 'k' => out.push_str(&"h".repeat(n.min(2))),
            'm' => out.push_str(&"m".repeat(n.min(2))),
            's' => out.push_str(&"s".repeat(n.min(2))),
            'a' => out.push_str("AM/PM"),
            _ => {
                for _ in 0..n {
                    out.push(c);
                }
            }
        }
        i += n;
    }
    out
}

/// A formula as Numbers shows it, in Excel's operators (× ÷ ≠ ≤ ≥ -> * / <> <= >=), string
/// literals left as they are.
fn excel_formula(f: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for ch in f.chars() {
        if ch == '"' {
            quoted = !quoted;
        }
        match ch {
            '×' if !quoted => out.push('*'),
            '÷' if !quoted => out.push('/'),
            '≠' if !quoted => out.push_str("<>"),
            '≤' if !quoted => out.push_str("<="),
            '≥' if !quoted => out.push_str(">="),
            c => out.push(c),
        }
    }
    out
}

/// A Numbers number format as an Excel format code (None: General).
/// The Numbers format of an Excel number format code ([`excel_number_format`] the other way):
/// percent, currency ($ € £ or [$ISO]), scientific, text, or a number with fixed decimals and
/// separators. General, dates and codes Numbers has no format for stay Automatic (`None`).
fn excel_cell_format(code: &str) -> Option<kn_model::tablefmt::CellFormat> {
    use kn_model::tablefmt::CellFormat as F;
    // the positive section, without colours, conditions and literal text
    let first = code.split(';').next().unwrap_or("");
    let mut iso = None;
    let mut plain = String::new();
    let mut chars = first.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '[' => {
                let inner: String = chars.by_ref().take_while(|&c| c != ']').collect();
                if let Some(cur) = inner.strip_prefix('$') {
                    let sym = cur.split('-').next().unwrap_or("");
                    iso = Some(match sym { "$" => "USD", "€" => "EUR", "£" => "GBP", s if s.len() == 3 => s, _ => "USD" }.to_string());
                }
            }
            '"' => {
                let lit: String = chars.by_ref().take_while(|&c| c != '"').collect();
                plain.push_str(&lit);
            }
            '\\' | '_' | '*' => {
                if let Some(next) = chars.next() {
                    if ch == '\\' {
                        plain.push(next);
                    }
                }
            }
            c => plain.push(c),
        }
    }
    let t = plain.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("general") || crate::xlstyle::is_date_code(code) {
        return None;
    }
    if t == "@" {
        return Some(F::Text);
    }
    if !t.contains('0') && !t.contains('#') {
        return None;
    }
    let decimals = t.split_once('.').map(|(_, d)| d.chars().take_while(|c| matches!(c, '0' | '#')).filter(|&c| c == '0').count() as u32).unwrap_or(0);
    let thousands = t.contains(',');
    if t.contains("E+") || t.contains("e+") {
        return Some(F::Scientific { decimals: Some(decimals) });
    }
    if t.contains('%') {
        return Some(F::Percent { decimals: Some(decimals), thousands, negative: 0 });
    }
    let code = iso.or_else(|| {
        [("$", "USD"), ("€", "EUR"), ("£", "GBP"), ("¥", "JPY")].iter().find(|(s, _)| t.contains(s)).map(|(_, c)| c.to_string())
    });
    match code {
        Some(code) => Some(F::Currency { code, decimals: Some(decimals), thousands, negative: 0, accounting: false }),
        None => Some(F::Number { decimals: Some(decimals), thousands, negative: 0 }),
    }
}

fn excel_number_format(f: &kn_model::tablefmt::CellFormat) -> Option<String> {
    use kn_model::tablefmt::CellFormat as F;
    let digits = |d: &Option<u32>, thousands: bool| {
        let int = if thousands { "#,##0" } else { "0" };
        match d {
            Some(n) if *n > 0 => format!("{int}.{}", "0".repeat(*n as usize)),
            _ => int.to_string(),
        }
    };
    match f {
        F::Number { decimals: Some(_), thousands, .. } => Some(digits(&match f { F::Number { decimals, .. } => *decimals, _ => None }, *thousands)),
        F::Currency { code, decimals, thousands, .. } => {
            let n = digits(&decimals.or(Some(2)), *thousands);
            Some(match code.as_str() {
                "USD" => format!("\"$\"{n}"),
                "EUR" => format!("{n}\\ \"€\""),
                "GBP" => format!("\"£\"{n}"),
                c => format!("[${c}] {n}"),
            })
        }
        F::Percent { decimals, .. } => Some(format!("{}%", digits(decimals, false))),
        F::Scientific { decimals } => Some(format!("{}E+00", digits(decimals, false))),
        F::Text => Some("@".into()),
        F::DateTime { pattern } => Some(excel_date_format(pattern)),
        _ => None,
    }
}

/// Write `wb` as an Excel workbook at `out`; returns the number of worksheets. The tables keep
/// their look (fonts, fills, borders, alignment, wrap, number formats), merges, sizes, header
/// rows and columns (as frozen panes), formulas (with their results) and sheet tab colours.
pub fn write_xlsx(wb: &crate::Workbook, out: &std::path::Path) -> Result<usize, kn_format::Error> {
    write_xlsx_with(wb, out, false)
}

/// The hidden part holding the whole Values document in a compatibility-mode .xlsx, and the
/// fingerprint of the Excel parts it was saved with (another app saving the file changes them).
const VALUES_PART: &str = "values/document.numbers";
const VALUES_PRINT: &str = "values/fingerprint.txt";

/// FNV-1a over the workbook's Excel parts (workbook, styles, strings, worksheets), in name order.
fn fingerprint(parts: &mut Vec<(String, Vec<u8>)>) -> String {
    parts.retain(|(n, _)| n == "xl/workbook.xml" || n == "xl/styles.xml" || n == "xl/sharedStrings.xml" || (n.starts_with("xl/worksheets/") && n.ends_with(".xml")));
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h: u64 = 0xcbf29ce484222325;
    for (n, b) in parts.iter() {
        for x in n.as_bytes().iter().chain(b.iter()) {
            h ^= *x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

/// What a workbook's cells have that Numbers can't hold: sheet name -> (row, column) -> extras.
pub type Extras = std::collections::HashMap<String, std::collections::HashMap<(u32, u32), crate::xlstyle::Extra>>;

/// The extras of an .xlsx's cells.
pub fn read_extras(bytes: &[u8]) -> Extras {
    read_xlsx_full(bytes).map(|(sheets, _)| sheets.into_iter().filter(|s| !s.extras.is_empty()).map(|s| (s.name, s.extras)).collect()).unwrap_or_default()
}

/// How an .xlsx was opened.
pub struct XlsxOpened {
    pub workbook: crate::Workbook,
    /// What the cells have that Numbers can't hold, by sheet name (Values keeps it for Excel).
    pub extras: Extras,
    /// It holds a Values copy (compatibility mode).
    pub compat: bool,
    /// It had a Values copy, but the Excel parts were changed since (by another app): the
    /// workbook was read from them, and the Values-only extras are gone.
    pub changed_elsewhere: bool,
}

/// Open an .xlsx: from its Values copy when it has one the Excel parts still match, else
/// from the Excel parts with their look.
pub fn open_xlsx(path: &std::path::Path) -> Result<XlsxOpened, kn_format::Error> {
    let bytes = std::fs::read(path)?;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..]))?;
    let mut parts = Vec::new();
    let mut copy: Option<Vec<u8>> = None;
    let mut print: Option<String> = None;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let name = f.name().map(|n| n.to_string()).unwrap_or_default();
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut b)?;
        match name.as_str() {
            VALUES_PART => copy = Some(b),
            VALUES_PRINT => print = Some(String::from_utf8_lossy(&b).trim().to_string()),
            _ => parts.push((name, b)),
        }
    }
    let had = copy.is_some();
    if let (Some(copy), Some(print)) = (copy, print) {
        if fingerprint(&mut parts) == print {
            crate::init();
            let k = kn_format::KeyFile::from_zip(&copy)?;
            let mut doc = kn_model::Document::from_file(k);
            doc.path = None;
            doc.dirty = false;
            return Ok(XlsxOpened { workbook: crate::Workbook { doc }, extras: read_extras(&bytes), compat: true, changed_elsewhere: false });
        }
    }
    let mut workbook = import_xlsx(path)?;
    workbook.doc.dirty = false;
    Ok(XlsxOpened { workbook, extras: read_extras(&bytes), compat: had, changed_elsewhere: had })
}

/// [`write_xlsx`]; with `keep_values` the whole Values document goes in a hidden part too
/// (compatibility mode), for Values to open again with nothing lost.
pub fn write_xlsx_with(wb: &crate::Workbook, out: &std::path::Path, keep_values: bool) -> Result<usize, kn_format::Error> {
    write_xlsx_full(wb, out, keep_values, &Extras::new())
}

/// [`write_xlsx_with`], with the cells' extras (by sheet name) written back: rotation, diagonal
/// lines, links.
pub fn write_xlsx_full(wb: &crate::Workbook, out: &std::path::Path, keep_values: bool, extras: &Extras) -> Result<usize, kn_format::Error> {
    use crate::xlstyle::CellLookOut;
    use kn_model::table::CellValue;
    use std::io::Write;
    let mut sheets_xml: Vec<(String, String)> = Vec::new();
    // each worksheet's links (cell, address): external ones go in its rels
    let mut sheet_links: Vec<Vec<(String, String)>> = Vec::new();
    let mut used = Vec::new();
    let customs = kn_model::table::custom_format_map(wb.file());
    let mut book = crate::xlstyle::StyleBook::default();
    for s in wb.sheets() {
        let tables = wb.tables(&s);
        let tab = crate::sheets::tab_color(wb.file(), s.id);
        let ext = extras.get(&s.name);
        for t in &tables {
            let Some(td) = wb.table_data(t.info) else { continue };
            let values = wb.doc.cell_values(t.info);
            let name = sheet_name(&if tables.len() > 1 { format!("{} - {}", s.name, t.name) } else { s.name.clone() }, &mut used);
            let mut x = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#);
            if let Some(c) = tab {
                let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                x.push_str(&format!(r#"<sheetPr><tabColor rgb="FF{:02X}{:02X}{:02X}"/></sheetPr>"#, b(c.r), b(c.g), b(c.b)));
            }
            // the table's grid lines are written as cell borders: Excel's own grid is off; the
            // header rows and columns stay in view (frozen) as in Numbers
            x.push_str(r#"<sheetViews><sheetView showGridLines="0" workbookViewId="0">"#);
            if td.header_rows > 0 || td.header_cols > 0 {
                let mut pane = String::from("<pane");
                if td.header_cols > 0 {
                    pane.push_str(&format!(r#" xSplit="{}""#, td.header_cols));
                }
                if td.header_rows > 0 {
                    pane.push_str(&format!(r#" ySplit="{}""#, td.header_rows));
                }
                let active = match (td.header_rows > 0, td.header_cols > 0) {
                    (true, true) => "bottomRight",
                    (true, false) => "bottomLeft",
                    _ => "topRight",
                };
                pane.push_str(&format!(r#" topLeftCell="{}{}" activePane="{active}" state="frozen"/>"#, col_letters(td.header_cols), td.header_rows + 1));
                x.push_str(&pane);
            }
            x.push_str("</sheetView></sheetViews>");
            // widths: points -> Excel's character units (7 px a character at 96 dpi)
            x.push_str("<cols>");
            for (c, w) in td.col_widths.iter().enumerate() {
                x.push_str(&format!(r#"<col min="{0}" max="{0}" width="{1:.2}" customWidth="1"/>"#, c + 1, w * 96.0 / 72.0 / 7.0));
            }
            x.push_str("</cols><sheetData>");
            let stroke = |s: &Option<kn_model::Stroke>| s.as_ref().filter(|s| s.width > 0.0 && s.color.a > 0.0).map(|s| (s.width, if s.dash.is_empty() { 0u8 } else if s.dash.first().is_some_and(|d| *d <= 1.0) { 2 } else { 1 }, s.color));
            for r in 0..td.rows {
                let h = td.row_heights.get(r as usize).copied().unwrap_or(20.0);
                if h <= 0.0 {
                    x.push_str(&format!(r#"<row r="{}" hidden="1">"#, r + 1));
                } else {
                    x.push_str(&format!(r#"<row r="{}" ht="{h:.1}" customHeight="1">"#, r + 1));
                }
                for c in 0..td.cols {
                    let at = format!("{}{}", col_letters(c), r + 1);
                    let i = (r * td.cols + c) as usize;
                    // the cell's look (a merged cell's from its top-left)
                    let anchor = td.merge_at(r, c).map(|m| (m.row * td.cols + m.col) as usize).unwrap_or(i);
                    let mut look = CellLookOut::default();
                    if let Some(st) = td.cell_styles.get(anchor) {
                        let ch = &st.text;
                        look.font = Some((ch.font_name.clone(), ch.size, ch.bold, ch.italic, ch.underline, ch.strike, ch.color));
                        match &st.fill {
                            kn_model::Fill::Color(f) if f.a > 0.0 => look.fill = Some(*f),
                            kn_model::Fill::Gradient { kind: kn_model::GradientKind::Linear { angle }, stops, .. } if stops.len() >= 2 => {
                                look.gradient = Some((stops[0].1, stops[stops.len() - 1].1, *angle));
                            }
                            _ => {}
                        }
                        look.halign = match st.align {
                            kn_model::Align::Left => Some("left"),
                            kn_model::Align::Right => Some("right"),
                            kn_model::Align::Center => Some("center"),
                            kn_model::Align::Justify => Some("justify"),
                            kn_model::Align::Natural => None,
                        };
                        look.valign = Some(match st.valign {
                            kn_model::VAlign::Top => "top",
                            kn_model::VAlign::Middle => "center",
                            kn_model::VAlign::Bottom => "bottom",
                        });
                        look.wrap = st.wrap;
                    }
                    let hs = |b: u32| td.h_strokes.get((b * td.cols + c) as usize).and_then(|s| stroke(s));
                    let vs = |b: u32| td.v_strokes.get((r * (td.cols + 1) + b) as usize).and_then(|s| stroke(s));
                    look.borders = [vs(c), vs(c + 1), hs(r), hs(r + 1)];
                    if let Some(e) = ext.and_then(|m| m.get(&(r, c))) {
                        look.rotation = e.rotation;
                        look.diag = e.diag.clone();
                    }
                    let value = values.get(&(r, c));
                    let fmt = wb.doc.cell_format(t.info, r, c);
                    look.numfmt = match (value, &fmt) {
                        (Some(CellValue::Date(_)), kn_model::tablefmt::CellFormat::Custom { format }) => kn_model::table::date_pattern_with(&customs, format).map(|p| excel_date_format(&p)),
                        (Some(CellValue::Date(_)), f) => excel_number_format(f).or_else(|| Some("m/d/yyyy".into())),
                        (Some(CellValue::Duration(_)), _) => Some("[h]:mm:ss".into()),
                        (_, f) => excel_number_format(f),
                    };
                    let st = book.xf(&look);
                    let sa = if st > 0 { format!(r#" s="{st}""#) } else { String::new() };
                    // a formula Excel can read (one table's own cells: no Numbers "Sheet::Table::" references)
                    let f = wb.doc.formula_text(t.info, r, c).filter(|f| !f.contains("::")).map(|f| format!("<f>{}</f>", xml_escape(&excel_formula(f.trim_start_matches('=')))));
                    let f = f.unwrap_or_default();
                    match value {
                        Some(CellValue::Number(v)) => x.push_str(&format!(r#"<c r="{at}"{sa}>{f}<v>{v}</v></c>"#)),
                        // seconds from 2001-01-01 -> Excel's day number (1900 system: 36892)
                        Some(CellValue::Date(v)) => x.push_str(&format!(r#"<c r="{at}"{sa}>{f}<v>{}</v></c>"#, v / 86400.0 + 36892.0)),
                        Some(CellValue::Duration(v)) => x.push_str(&format!(r#"<c r="{at}"{sa}>{f}<v>{}</v></c>"#, v / 86400.0)),
                        Some(CellValue::Bool(b)) => x.push_str(&format!(r#"<c r="{at}"{sa} t="b">{f}<v>{}</v></c>"#, u8::from(*b))),
                        Some(CellValue::Text(t)) if !t.is_empty() => {
                            if f.is_empty() {
                                x.push_str(&format!(r#"<c r="{at}"{sa} t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#, xml_escape(t)));
                            } else {
                                x.push_str(&format!(r#"<c r="{at}"{sa} t="str">{f}<v>{}</v></c>"#, xml_escape(t)));
                            }
                        }
                        _ if st > 0 => x.push_str(&format!(r#"<c r="{at}"{sa}/>"#)),
                        _ => {}
                    }
                }
                x.push_str("</row>");
            }
            x.push_str("</sheetData>");
            if !td.merges.is_empty() {
                x.push_str(&format!(r#"<mergeCells count="{}">"#, td.merges.len()));
                for m in &td.merges {
                    x.push_str(&format!(r#"<mergeCell ref="{}{}:{}{}"/>"#, col_letters(m.col), m.row + 1, col_letters(m.col + m.cols - 1), m.row + m.rows));
                }
                x.push_str("</mergeCells>");
            }
            let mut links: Vec<(String, String)> = ext.map(|m| m.iter().filter_map(|((r, c), e)| Some((format!("{}{}", col_letters(*c), r + 1), e.link.clone()?))).collect()).unwrap_or_default();
            links.sort();
            if !links.is_empty() {
                x.push_str("<hyperlinks>");
                let mut n = 0;
                for (at, url) in &links {
                    match url.strip_prefix('#') {
                        Some(place) => x.push_str(&format!(r#"<hyperlink ref="{at}" location="{}"/>"#, xml_escape(place))),
                        None => {
                            n += 1;
                            x.push_str(&format!(r#"<hyperlink ref="{at}" r:id="rIdL{n}"/>"#));
                        }
                    }
                }
                x.push_str("</hyperlinks>");
            }
            x.push_str("</worksheet>");
            sheets_xml.push((name, x));
            sheet_links.push(links.into_iter().filter(|(_, u)| !u.starts_with('#')).collect());
        }
    }
    if sheets_xml.is_empty() {
        return Err(bad("there are no tables to export"));
    }
    let n = sheets_xml.len();
    let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#);
    let mut wbx = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#);
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    for (i, (name, _)) in sheets_xml.iter().enumerate() {
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#, i + 1));
        wbx.push_str(&format!(r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#, xml_escape(name), i + 1, i + 1));
        rels.push_str(&format!(r#"<Relationship Id="rId{0}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{0}.xml"/>"#, i + 1));
    }
    ct.push_str("</Types>");
    wbx.push_str("</sheets></workbook>");
    rels.push_str(&format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#, n + 1));
    let styles = book.styles_xml();
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    // written next to the file, then moved over it (a failed save leaves the old file)
    let tmp = out.with_extension("xlsx.partial");
    let f = std::fs::File::create(&tmp)?;
    let mut z = zip::ZipWriter::new(f);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let put = |z: &mut zip::ZipWriter<std::fs::File>, name: &str, body: &str| -> Result<(), kn_format::Error> {
        z.start_file(name, opt).map_err(|e| bad(e.to_string()))?;
        z.write_all(body.as_bytes())?;
        Ok(())
    };
    let ct = if keep_values { ct.replace("</Types>", r#"<Default Extension="numbers" ContentType="application/octet-stream"/><Default Extension="txt" ContentType="text/plain"/></Types>"#) } else { ct };
    put(&mut z, "[Content_Types].xml", &ct)?;
    put(&mut z, "_rels/.rels", root_rels)?;
    put(&mut z, "xl/workbook.xml", &wbx)?;
    put(&mut z, "xl/_rels/workbook.xml.rels", &rels)?;
    put(&mut z, "xl/styles.xml", &styles)?;
    for (i, (_, x)) in sheets_xml.iter().enumerate() {
        put(&mut z, &format!("xl/worksheets/sheet{}.xml", i + 1), x)?;
        if let Some(links) = sheet_links.get(i).filter(|l| !l.is_empty()) {
            let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
            for (n, (_, url)) in links.iter().enumerate() {
                rels.push_str(&format!(r#"<Relationship Id="rIdL{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#, n + 1, xml_escape(url)));
            }
            rels.push_str("</Relationships>");
            put(&mut z, &format!("xl/worksheets/_rels/sheet{}.xml.rels", i + 1), &rels)?;
        }
    }
    if keep_values {
        let mut parts: Vec<(String, Vec<u8>)> = vec![("xl/workbook.xml".into(), wbx.clone().into_bytes()), ("xl/styles.xml".into(), styles.clone().into_bytes())];
        parts.extend(sheets_xml.iter().enumerate().map(|(i, (_, x))| (format!("xl/worksheets/sheet{}.xml", i + 1), x.clone().into_bytes())));
        let print = fingerprint(&mut parts);
        let copy = wb.doc.file.to_zip()?;
        z.start_file(VALUES_PART, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).map_err(|e| bad(e.to_string()))?;
        z.write_all(&copy)?;
        put(&mut z, VALUES_PRINT, &print)?;
    }
    z.finish().map_err(|e| bad(e.to_string()))?;
    std::fs::rename(&tmp, out)?;
    Ok(n)
}

/// The cells with a key, as blocks of equal keys (row 0, col 0, row 1, col 1; greedy: across,
/// then down as far as whole rows of the block match).
fn blocks(nr: u32, nc: u32, key: impl Fn(u32, u32) -> Option<String>) -> Vec<(String, (u32, u32, u32, u32))> {
    let keys: Vec<Option<String>> = (0..nr).flat_map(|r| (0..nc).map(move |c| (r, c))).map(|(r, c)| key(r, c)).collect();
    let at = |r: u32, c: u32| keys[(r * nc + c) as usize].as_ref();
    let mut done = vec![false; keys.len()];
    let mut out = Vec::new();
    for r in 0..nr {
        for c in 0..nc {
            let Some(k) = at(r, c) else { continue };
            if done[(r * nc + c) as usize] {
                continue;
            }
            let mut c1 = c;
            while c1 + 1 < nc && !done[(r * nc + c1 + 1) as usize] && at(r, c1 + 1) == Some(k) {
                c1 += 1;
            }
            let mut r1 = r;
            while r1 + 1 < nr && (c..=c1).all(|cc| !done[((r1 + 1) * nc + cc) as usize] && at(r1 + 1, cc) == Some(k)) {
                r1 += 1;
            }
            for rr in r..=r1 {
                for cc in c..=c1 {
                    done[(rr * nc + cc) as usize] = true;
                }
            }
            out.push((k.clone(), (r, c, r1, c1)));
        }
    }
    out
}
