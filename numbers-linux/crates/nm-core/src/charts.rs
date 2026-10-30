//! Insert -> Chart from the selected cells. The chart is a copy of the one Numbers 15.4 inserts
//! (Mac round N2 donors, one per type) carrying the selection's values in its own grid, as an
//! unlinked chart: its mediator has no formulas (as research sample 28's chart), so later edits
//! of the table don't change it (Numbers' table-linked charts need their own formula owner).
//! Its non-style parts go in the document's ObjectContainer (made from the donor's when
//! missing), its styles map onto the theme's.

use std::collections::HashMap;

use kn_format::{Entry, EntryData, IwaFile, KeyFile, ObjId};
use kn_model::chartdata::ChartGrid;
use kn_model::edit::History;
use kn_model::Document;

/// Chart types offered: (menu name, donor).
pub const CHART_TYPES: [(&str, &[u8]); 7] = [
    ("Column", include_bytes!("../data/Chart-2d-column.numbers")),
    ("Stacked Column", include_bytes!("../data/Chart-2d-stacked-column.numbers")),
    ("Bar", include_bytes!("../data/Chart-2d-bar.numbers")),
    ("Line", include_bytes!("../data/Chart-2d-line.numbers")),
    ("Area", include_bytes!("../data/Chart-2d-area.numbers")),
    ("Pie", include_bytes!("../data/Chart-2d-pie.numbers")),
    ("Donut", include_bytes!("../data/Chart-2d-donut.numbers")),
];

const OC_FILE: &str = "Index/ObjectContainer.iwa";

fn bad(s: &str) -> kn_format::Error {
    kn_format::Error::Invalid(s.into())
}

/// The cells a chart made from block (r0, c0, r1, c1) of `t` reads, as `grid_from_cells` lays
/// them out: each series' column range, the cells naming the series (the header row) and the
/// categories (a text first column). Empty names / categories when there are none.
#[allow(clippy::type_complexity)]
pub fn link_cells(t: &kn_model::TableData, block: (u32, u32, u32, u32)) -> (Vec<(u32, u32, u32, u32)>, Vec<(u32, u32)>, Vec<(u32, u32)>) {
    let (r0, c0, r1, c1) = block;
    let cell = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
    let num = |s: &str| -> Option<f64> {
        let clean: String = s.chars().filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'E' | 'e')).collect();
        clean.parse::<f64>().ok().filter(|_| !s.chars().any(|ch| ch.is_alphabetic() && ch != 'E' && ch != 'e'))
    };
    let header = r0 < t.header_rows && r1 > r0;
    let body0 = if header { r0 + 1 } else { r0 };
    let label_col = c1 > c0 && (body0..=r1).all(|r| num(&cell(r, c0)).is_none());
    let first_series = if label_col { c0 + 1 } else { c0 };
    let cols: Vec<u32> = (first_series..=c1).filter(|c| (body0..=r1).any(|r| num(&cell(r, *c)).is_some())).collect();
    let series = cols.iter().map(|&c| (body0, c, r1, c)).collect();
    let names = if header { cols.iter().map(|&c| (r0, c)).collect() } else { Vec::new() };
    let cats = if label_col { (body0..=r1).map(|r| (r, c0)).collect() } else { Vec::new() };
    (series, names, cats)
}

/// `link_cells` with the table's rows as series (Plot Rows as Series): each series a row named
/// by its header-column cell, the categories named by a text first row.
#[allow(clippy::type_complexity)]
pub fn link_cells_by_row(t: &kn_model::TableData, block: (u32, u32, u32, u32)) -> (Vec<(u32, u32, u32, u32)>, Vec<(u32, u32)>, Vec<(u32, u32)>) {
    let (r0, c0, r1, c1) = block;
    let cell = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
    let num = |s: &str| -> Option<f64> {
        let clean: String = s.chars().filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'E' | 'e')).collect();
        clean.parse::<f64>().ok().filter(|_| !s.chars().any(|ch| ch.is_alphabetic() && ch != 'E' && ch != 'e'))
    };
    let header = c0 < t.header_cols && c1 > c0;
    let body0 = if header { c0 + 1 } else { c0 };
    let label_row = r1 > r0 && (body0..=c1).all(|c| num(&cell(r0, c)).is_none());
    let first_series = if label_row { r0 + 1 } else { r0 };
    let rows: Vec<u32> = (first_series..=r1).filter(|r| (body0..=c1).any(|c| num(&cell(*r, c)).is_some())).collect();
    let series = rows.iter().map(|&r| (r, body0, r, c1)).collect();
    let names = if header { rows.iter().map(|&r| (r, c0)).collect() } else { Vec::new() };
    let cats = if label_row { (body0..=c1).map(|c| (r0, c)).collect() } else { Vec::new() };
    (series, names, cats)
}

/// A chart grid from a table's cells (r0, c0, r1, c1): each column a series named by its
/// header-row cell, each row a category named by the first column when that holds text.
pub fn grid_from_cells(t: &kn_model::TableData, block: (u32, u32, u32, u32)) -> ChartGrid {
    let (r0, c0, r1, c1) = block;
    let cell = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
    let num = |s: &str| -> Option<f64> {
        let clean: String = s.chars().filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'E' | 'e')).collect();
        clean.parse::<f64>().ok().filter(|_| !s.chars().any(|ch| ch.is_alphabetic() && ch != 'E' && ch != 'e'))
    };
    // the header row in the block names the series; a text first column names the categories
    let header = r0 < t.header_rows && r1 > r0;
    let body0 = if header { r0 + 1 } else { r0 };
    let label_col = c1 > c0 && (body0..=r1).all(|r| num(&cell(r, c0)).is_none());
    let first_series = if label_col { c0 + 1 } else { c0 };
    let mut g = ChartGrid { row_names: Vec::new(), col_names: Vec::new(), values: Vec::new() };
    // text-only columns (e.g. a group column) aren't series
    for c in (first_series..=c1).filter(|c| (body0..=r1).any(|r| num(&cell(r, *c)).is_some())) {
        g.row_names.push(if header { cell(r0, c) } else { format!("Series {}", g.row_names.len() + 1) });
        g.values.push((body0..=r1).map(|r| num(&cell(r, c)).unwrap_or(f64::NAN)).collect());
    }
    for r in body0..=r1 {
        g.col_names.push(if label_col { cell(r, c0) } else { (r - body0 + 1).to_string() });
    }
    g
}

/// The size a chart of type `kind` is inserted at (its donor's frame), for finding room.
pub fn chart_size(kind: usize) -> Option<(f32, f32)> {
    let donor = KeyFile::from_zip(CHART_TYPES.get(kind)?.1).ok()?;
    let (_, m) = donor.first_of("TSCH.ChartDrawableArchive")?;
    let size = m.find_super("TSD.DrawableArchive")?.msg("geometry")?.msg("size")?;
    Some((size.f32("width")?, size.f32("height")?))
}

/// Insert a chart of type `kind` (index into CHART_TYPES) on `sheet` at (x, y) showing `grid`.
pub fn insert_chart(doc: &mut Document, h: &mut History, sheet: ObjId, kind: usize, grid: &ChartGrid, x: f32, y: f32) -> Result<ObjId, kn_format::Error> {
    crate::init();
    let donor = KeyFile::from_zip(CHART_TYPES.get(kind).ok_or(bad("no such chart type"))?.1)?;
    let (chart, _) = donor.first_of("TSCH.ChartDrawableArchive").ok_or(bad("the donor has no chart"))?;
    let ex = donor.extract(chart)?;
    let (oc_ids, style_ids): (Vec<ObjId>, Vec<ObjId>) = ex.external.iter().partition(|i| donor.file_of(**i) == Some(OC_FILE));
    let mut map: HashMap<ObjId, ObjId> = doc.file.map_external(&donor, &style_ids)?;
    let (oc_root, _) = donor.first_of("TSP.ObjectContainer").ok_or(bad("the donor has no object container"))?;
    let have_oc = doc.file.first_of("TSP.ObjectContainer").map(|(i, _)| i);
    if have_oc.is_none() && doc.file.iwa(OC_FILE).is_some() {
        return Err(bad("unexpected object container layout"));
    }
    let touch: Vec<ObjId> = have_oc.into_iter().chain([sheet]).collect();
    let id = doc.structural(h, &touch, |k| -> Result<ObjId, kn_format::Error> {
        // the document's object container (the donor's, emptied, when there is none yet)
        let oc = match have_oc {
            Some(oc) => oc,
            None => {
                let new_root = k.alloc_ids(1)[0];
                let mut a = donor.archive(oc_root).unwrap().clone();
                a.info.set_u64("identifier", new_root);
                a.obj_mut().remove("objects");
                a.set_object_references(0, &[]);
                k.add_entry(Entry { name: OC_FILE.into(), data: EntryData::Iwa(IwaFile { archives: vec![a] }) });
                let mut comp = donor.component(oc_root).ok_or(bad("donor container has no component"))?.clone();
                comp.set_u64("identifier", new_root);
                for f in ["object_uuid_map_entries", "external_references", "data_references"] {
                    comp.remove(f);
                }
                k.package_metadata_mut().ok_or(bad("no package metadata"))?.push_msg("components", comp);
                k.reindex();
                new_root
            }
        };
        let file = k.file_of(oc).unwrap_or(OC_FILE).to_string();
        let new_ids = k.alloc_ids(oc_ids.len() as u64);
        let oc_map: HashMap<ObjId, ObjId> = oc_ids.iter().copied().zip(new_ids.iter().copied()).collect();
        for (old, new) in &oc_map {
            let mut a = donor.archive(*old).unwrap().clone();
            a.info.set_u64("identifier", *new);
            for i in 0..a.payloads.len() {
                let refs: Vec<ObjId> = a.object_references(i).into_iter().map(|r| *oc_map.get(&r).or(map.get(&r)).unwrap_or(&r)).collect();
                a.set_object_references(i, &refs);
                a.payloads[i].for_each_ref_mut(&mut |r| {
                    if let Some(n) = oc_map.get(r).or(map.get(r)) {
                        *r = *n;
                    }
                });
            }
            k.insert_archive(&file, a);
            if let Some(o) = k.obj_mut(oc) {
                o.push_ref("objects", *new);
            }
            k.add_uuid_entry(&file, *new);
        }
        k.refresh_refs(oc);
        map.extend(oc_map);
        let before = k.last_object_identifier();
        let id = k.paste_extract(&ex, sheet, Some(&map))?;
        // unlinked: the copied mediator (made by this paste) keeps no formulas
        let mediators: Vec<ObjId> = k.objects_of("TN.ChartMediatorArchive").map(|(i, _)| i).filter(|m| *m > before).collect();
        for m in mediators {
            if let Some(mm) = k.obj_mut(m) {
                if mm.has("formulas") {
                    mm.remove("formulas");
                    let f = mm.msg_mut_or_create("formulas");
                    f.set_u64("direction", 0);
                    f.set_u64("scheme", 0);
                }
            }
        }
        k.ensure_external_refs(&file);
        if let Some(f) = k.file_of(id).map(str::to_string) {
            k.ensure_external_refs(&f);
        }
        Ok(id)
    })?;
    crate::sheets::attach_drawable(doc, h, sheet, id);
    // pie and donut charts keep their categories as rows (Numbers' own pie, N2)
    let round = matches!(CHART_TYPES[kind].0, "Pie" | "Donut");
    let grid = if round { transpose(grid) } else { grid.clone() };
    if !doc.set_chart_grid(h, id, &grid) {
        return Err(bad("couldn't put the values in the chart"));
    }
    let mut g = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
    g.x = x;
    g.y = y;
    doc.set_frame(h, id, g);
    Ok(id)
}

fn transpose(g: &ChartGrid) -> ChartGrid {
    let cats = g.col_names.len();
    ChartGrid {
        row_names: g.col_names.clone(),
        col_names: g.row_names.clone(),
        values: (0..cats).map(|c| g.values.iter().map(|row| row.get(c).copied().unwrap_or(f64::NAN)).collect()).collect(),
    }
}
