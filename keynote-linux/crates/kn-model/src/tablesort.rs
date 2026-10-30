//! Table > Sort Ascending / Sort Descending: the body rows reordered by one column's values
//! (header and footer rows stay put). Each row's stored cells move as a whole (the tile's row
//! records and the row header entries are renumbered), so every cell keeps its own text,
//! format and style references.

use std::cmp::Ordering;

use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::table::CellValue;
use crate::Document;

/// Numbers before text, empty cells last (also when sorting descending).
fn compare(a: Option<&CellValue>, b: Option<&CellValue>, ascending: bool) -> Ordering {
    let rank = |v: Option<&CellValue>| match v {
        Some(CellValue::Number(_) | CellValue::Duration(_) | CellValue::Date(_)) => 0,
        Some(CellValue::Bool(_)) => 1,
        Some(CellValue::Text(t)) if !t.is_empty() => 2,
        _ => 3,
    };
    let (ra, rb) = (rank(a), rank(b));
    if ra == 3 || rb == 3 || ra != rb {
        return ra.cmp(&rb);
    }
    let o = match (a, b) {
        (Some(CellValue::Number(x) | CellValue::Duration(x) | CellValue::Date(x)), Some(CellValue::Number(y) | CellValue::Duration(y) | CellValue::Date(y))) => x.total_cmp(y),
        (Some(CellValue::Bool(x)), Some(CellValue::Bool(y))) => x.cmp(y),
        (Some(CellValue::Text(x)), Some(CellValue::Text(y))) => x.to_lowercase().cmp(&y.to_lowercase()),
        _ => Ordering::Equal,
    };
    if ascending {
        o
    } else {
        o.reverse()
    }
}

/// Renumber the `index_field` of the messages in `field`, then put them in index order.
fn renumber(m: &mut Msg, field: &str, index_field: &str, new_index: &dyn Fn(u32) -> u32) {
    let mut items: Vec<Msg> = m.msgs(field).cloned().collect();
    for it in &mut items {
        if let Some(i) = it.u32(index_field) {
            it.set_u64(index_field, u64::from(new_index(i)));
        }
    }
    items.sort_by_key(|it| it.u32(index_field).unwrap_or(0));
    m.remove(field);
    for it in items {
        m.push_msg(field, it);
    }
}

impl Document {
    /// Sort the body rows of `table` by column `col`.
    pub fn sort_table_rows(&mut self, h: &mut History, table: ObjId, col: u32, ascending: bool) -> Result<(), kn_format::Error> {
        self.sort_table_rows_by(h, table, &[(col, ascending)], None)
    }

    /// Organize > Sort: the body rows (or rows `lo..=hi` of them: Sort Selected Rows) sorted by
    /// several columns, the first deciding, the next breaking its ties (Numbers' sort rules).
    pub fn sort_table_rows_by(&mut self, h: &mut History, table: ObjId, keys: &[(u32, bool)], rows: Option<(u32, u32)>) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let t = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)).ok_or(err("not a table"))?;
        let (body0, body1) = (t.header_rows, t.rows.saturating_sub(t.footer_rows));
        let (first, end) = match rows {
            Some((lo, hi)) => (lo.max(body0), (hi + 1).min(body1)),
            None => (body0, body1),
        };
        if keys.is_empty() {
            return Ok(());
        }
        // merged cells in the rows being sorted can't move with them
        if t.merges.iter().any(|m| m.row < end && m.row + m.rows > first) {
            return Err(err("rows with merged cells can't be sorted yet"));
        }
        let model = self.file.obj(table).and_then(|m| m.ref_id("tableModel")).and_then(|m| self.file.obj(m)).ok_or(err("no table model"))?;
        let ds = model.msg("base_data_store").ok_or(err("no data store"))?;
        let has_formulas = ds.ref_id("formula_table").and_then(|l| self.file.obj(l)).is_some_and(|l| l.count("entries") > 0);
        if has_formulas {
            return Err(err("tables with formulas can't be sorted yet"));
        }
        // tile 0: a big table's other tiles are gathered into it while rows renumber
        let tile = ds.msg("tiles").and_then(|t| t.msgs("tiles").find(|t| t.u32("tileid") == Some(0))).and_then(|t| t.ref_id("tile")).ok_or(err("no cell tile"))?;
        let bucket = ds.msg("rowHeaders").and_then(|r| r.ref_ids("buckets").first().copied());
        if end <= first + 1 {
            return Ok(());
        }
        let values = self.cell_values(table);
        let mut order: Vec<u32> = (first..end).collect();
        order.sort_by(|a, b| keys.iter().map(|(col, asc)| compare(values.get(&(*a, *col)), values.get(&(*b, *col)), *asc)).find(|o| *o != Ordering::Equal).unwrap_or(Ordering::Equal));
        if order.iter().copied().eq(first..end) {
            return Ok(());
        }
        let mut to = vec![0u32; t.rows as usize];
        for (i, slot) in to.iter_mut().enumerate() {
            *slot = i as u32;
        }
        for (k, old) in order.iter().enumerate() {
            to[*old as usize] = first + k as u32;
        }
        self.renumber_rows(h, table, tile, bucket, to)?;
        Ok(())
    }

    /// Move rows `lo..=hi` so they sit before row `before` (0..=rows, in the current
    /// numbering), as dragging selected row tabs does in Keynote. Header and footer rows stay
    /// put. Returns where the moved rows start now.
    pub fn move_table_rows(&mut self, h: &mut History, table: ObjId, lo: u32, hi: u32, before: u32) -> Result<u32, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let t = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)).ok_or(err("not a table"))?;
        let (tile, bucket) = self.row_move_parts(table, &t)?;
        let (first, end) = (t.header_rows, t.rows.saturating_sub(t.footer_rows));
        if lo > hi || lo < first || hi >= end || before < first || before > end {
            return Err(err("only body rows can be moved"));
        }
        if (lo..=hi + 1).contains(&before) {
            return Ok(lo);
        }
        let mut order: Vec<u32> = (0..t.rows).filter(|r| !(lo..=hi).contains(r)).collect();
        let at = order.iter().position(|&r| r >= before).unwrap_or(order.len());
        order.splice(at..at, lo..=hi);
        let mut to = vec![0u32; t.rows as usize];
        for (k, old) in order.iter().enumerate() {
            to[*old as usize] = k as u32;
        }
        self.renumber_rows(h, table, tile, bucket, to)?;
        Ok(at as u32)
    }

    /// The cell tile and row header bucket whose rows a move renumbers (refused for tables
    /// that need more than renumbering: merges, formulas).
    fn row_move_parts(&self, table: ObjId, t: &crate::TableData) -> Result<(ObjId, Option<ObjId>), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        if !t.merges.is_empty() {
            return Err(err("rows of tables with merged cells can't be moved yet"));
        }
        let model = self.file.obj(table).and_then(|m| m.ref_id("tableModel")).and_then(|m| self.file.obj(m)).ok_or(err("no table model"))?;
        let ds = model.msg("base_data_store").ok_or(err("no data store"))?;
        if ds.ref_id("formula_table").and_then(|l| self.file.obj(l)).is_some_and(|l| l.count("entries") > 0) {
            return Err(err("rows of tables with formulas can't be moved yet"));
        }
        let tile = ds.msg("tiles").and_then(|t| t.msgs("tiles").find(|t| t.u32("tileid") == Some(0))).and_then(|t| t.ref_id("tile")).ok_or(err("no cell tile"))?;
        Ok((tile, ds.msg("rowHeaders").and_then(|r| r.ref_ids("buckets").first().copied())))
    }

    /// Give each row its new number (`to[old]`): the tile's row records and the row headers.
    fn renumber_rows(&mut self, h: &mut History, table: ObjId, tile: ObjId, bucket: Option<ObjId>, to: Vec<u32>) -> Result<(), kn_format::Error> {
        // a big table's rows are all in tile 0 while they renumber (tabletiles.rs)
        self.gather_tiles(h, table)?;
        let new_index = move |i: u32| to.get(i as usize).copied().unwrap_or(i);
        self.touch(h, tile);
        if let Some(m) = self.file.obj_mut(tile) {
            renumber(m, "rowInfos", "tile_row_index", &new_index);
        }
        if let Some(b) = bucket {
            self.touch(h, b);
            if let Some(m) = self.file.obj_mut(b) {
                renumber(m, "headers", "index", &new_index);
            }
        }
        self.scatter_tiles(h, table)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn sorts_body_rows_by_a_column() {
        use crate::tablecells::CellInput;
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides()[0].slide;
        let mut h = History::default();
        d.begin(&mut h, "Table");
        let t = d.insert(&mut h, s, &crate::insert::NewObject::BASIC_TABLE).unwrap();
        for (r, name, v) in [(1, "Ann", CellInput::Number(12.0)), (2, "Bob", CellInput::Number(7.5)), (3, "Cy", CellInput::Text("n/a".into())), (4, "Di", CellInput::Number(19.5))] {
            d.set_cell(&mut h, t, r, 0, &CellInput::Text(name.into())).unwrap();
            d.set_cell(&mut h, t, r, 1, &v).unwrap();
        }
        d.end(&mut h);
        d.begin(&mut h, "Sort");
        d.sort_table_rows(&mut h, t, 1, true).unwrap();
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let DrawableKind::Table(tb) = d.scene(&d.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == t).unwrap().kind else { panic!() };
        let cell = |r: u32, c: u32| tb.cells[(r * tb.cols + c) as usize].clone();
        let col: Vec<String> = (1..5).map(|r| cell(r, 1)).collect();
        assert_eq!(col, ["7.5", "12", "19.5", "n/a"], "numbers then text");
        let names: Vec<String> = (1..5).map(|r| cell(r, 0)).collect();
        assert_eq!(names, ["Bob", "Ann", "Di", "Cy"], "whole rows move together");
    }

    /// Two sort columns (the second breaks ties), and Sort Selected Rows leaving the other rows;
    /// a cell's fill and a row's own height go with its row.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn sorts_by_several_columns_and_selected_rows() {
        use crate::tablecells::CellInput;
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides()[0].slide;
        let mut h = History::default();
        d.begin(&mut h, "Table");
        let t = d.insert(&mut h, s, &crate::insert::NewObject::BASIC_TABLE).unwrap();
        for (r, name, team) in [(1, "Ann", "B"), (2, "Bob", "A"), (3, "Cy", "B"), (4, "Di", "A")] {
            d.set_cell(&mut h, t, r, 0, &CellInput::Text(name.into())).unwrap();
            d.set_cell(&mut h, t, r, 1, &CellInput::Text(team.into())).unwrap();
        }
        // Cy's name cell is red and Cy's row is tall
        d.set_cells_fill(&mut h, t, (3, 0, 3, 0), Some(&crate::format::FillSpec::Color(crate::Color::rgba(1.0, 0.0, 0.0, 1.0)))).unwrap();
        d.set_table_sizes(&mut h, t, true, &[(3, 61.0)]);
        d.end(&mut h);
        d.begin(&mut h, "Sort");
        // team ascending, then name descending
        d.sort_table_rows_by(&mut h, t, &[(1, true), (0, false)], None).unwrap();
        d.end(&mut h);
        let read = |d: &Document| {
            let DrawableKind::Table(tb) = d.scene(&d.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == t).unwrap().kind else { panic!() };
            tb
        };
        let tb = read(&d);
        let names: Vec<String> = (1..5).map(|r| tb.cells[(r * tb.cols) as usize].clone()).collect();
        assert_eq!(names, ["Di", "Bob", "Cy", "Ann"]);
        // Cy keeps its fill and its row height
        let cy = names.iter().position(|n| n == "Cy").unwrap() as u32 + 1;
        assert!(matches!(tb.cell_styles[(cy * tb.cols) as usize].fill, crate::Fill::Color(c) if c.r > 0.9 && c.g < 0.1));
        assert!((tb.row_heights[cy as usize] - 61.0).abs() < 0.5);
        // selected rows 1..=2 (Di, Bob) by name ascending: Bob, Di; rows 3..4 stay
        d.begin(&mut h, "Sort");
        d.sort_table_rows_by(&mut h, t, &[(0, true)], Some((1, 2))).unwrap();
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let tb = read(&d);
        let names: Vec<String> = (1..5).map(|r| tb.cells[(r * tb.cols) as usize].clone()).collect();
        assert_eq!(names, ["Bob", "Di", "Cy", "Ann"]);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn moves_rows_like_dragging_their_tabs() {
        use crate::tablecells::CellInput;
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides()[0].slide;
        let mut h = History::default();
        d.begin(&mut h, "Table");
        let t = d.insert(&mut h, s, &crate::insert::NewObject::BASIC_TABLE).unwrap();
        for (r, name) in [(1, "Ann"), (2, "Bob"), (3, "Cy"), (4, "Di")] {
            d.set_cell(&mut h, t, r, 0, &CellInput::Text(name.into())).unwrap();
        }
        d.end(&mut h);
        d.begin(&mut h, "Move Rows");
        // rows 3..4 (Cy, Di) dragged above row 1
        assert_eq!(d.move_table_rows(&mut h, t, 3, 4, 1).unwrap(), 1);
        d.end(&mut h);
        d.begin(&mut h, "Move Rows");
        // row 1 (now Cy) dragged below the last body row
        let DrawableKind::Table(tb) = d.scene(&d.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == t).unwrap().kind else { panic!() };
        let end = tb.rows - tb.footer_rows;
        assert_eq!(d.move_table_rows(&mut h, t, 1, 1, end).unwrap(), end - 1);
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let DrawableKind::Table(tb) = d.scene(&d.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == t).unwrap().kind else { panic!() };
        let names: Vec<String> = (1..4).map(|r| tb.cells[(r * tb.cols) as usize].clone()).collect();
        assert_eq!(names, ["Di", "Ann", "Bob"], "Cy and Di went above Ann, then Cy below the rest");
        assert_eq!(tb.cells[((end - 1) * tb.cols) as usize], "Cy");
    }
}
