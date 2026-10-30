//! Tables over 256 rows: their cells live in several TST.Tile objects (Numbers 15.4,
//! research file N-functions.numbers, 356 rows):
//!   base_data_store.tiles {tile_size 256, tiles [{tileid 0, tile}, {tileid 1, tile}, …]}
//!   base_data_store.rowTileTree.nodes [{key 0, value 0}, {key 256, value 1}, …]
//!   each extra tile is its own component, Index/Tables/Tile-<id>.iwa (preferred locator
//!   "Tables/Tile"), rows numbered from 0 within it (tile_row_index), numrows = its row count.
//! Row headers stay in one bucket with table-wide indices.
//!
//! Row edits (insert / delete / sort / move) work on one tile: [`Document::gather_tiles`] puts
//! every row in tile 0 under its table-wide index first, [`Document::scatter_tiles`] deals them
//! out again afterwards, adding tiles as the table grows.

use kn_format::{Archive, Entry, EntryData, IwaFile, Msg, ObjId};

use crate::edit::History;
use crate::Document;

fn err(s: &str) -> kn_format::Error {
    kn_format::Error::Invalid(s.into())
}

/// (data store owner = the table model, tile size, [(tileid, tile)]) of a table.
fn tile_list(doc: &Document, table: ObjId) -> Option<(ObjId, u32, Vec<(u32, ObjId)>)> {
    let model = doc.file.obj(table)?.ref_id("tableModel")?;
    let tiles = doc.file.obj(model)?.msg("base_data_store")?.msg("tiles")?;
    let size = tiles.u32("tile_size").unwrap_or(256).max(1);
    let list = tiles.msgs("tiles").filter_map(|t| Some((t.u32("tileid")?, t.ref_id("tile")?))).collect();
    Some((model, size, list))
}

fn set_numrows(t: &mut Msg) {
    let n = t.count("rowInfos") as u64;
    t.set_u64("numrows", n);
}

impl Document {
    /// The cell tile holding table-wide row `row` (made when it doesn't exist yet), and the
    /// row's index within it.
    pub(crate) fn tile_for_row(&mut self, h: &mut History, table: ObjId, row: u32) -> Result<(ObjId, u32), kn_format::Error> {
        let (_, size, list) = tile_list(self, table).ok_or(err("no cell tiles"))?;
        let id = row / size;
        if let Some((_, t)) = list.iter().find(|(i, _)| *i == id) {
            return Ok((*t, row % size));
        }
        Ok((self.add_tile(h, table, id)?, row % size))
    }

    /// Make an empty cell tile `tileid` for the table, as Numbers stores one: its own component.
    pub(crate) fn add_tile(&mut self, h: &mut History, table: ObjId, tileid: u32) -> Result<ObjId, kn_format::Error> {
        let (model, size, list) = tile_list(self, table).ok_or(err("no cell tiles"))?;
        let first = list.iter().find(|(i, _)| *i == 0).map(|x| x.1).or(list.first().map(|x| x.1)).ok_or(err("no cell tile"))?;
        let first_file = self.file.file_of(first).ok_or(err("cell tile isn't stored"))?.to_string();
        let model_file = self.file.file_of(model).ok_or(err("table model isn't stored"))?.to_string();
        let comp_template = self.file.component_for_file(&first_file).and_then(|c| self.file.component(c)).cloned().ok_or(err("cell tile has no component"))?;
        let storage = self.file.obj(first).and_then(|t| t.u64("storage_version")).unwrap_or(5);
        self.structural(h, &[model], |k| {
            let id = k.alloc_ids(1)[0];
            let mut t = Msg::new("TST.Tile");
            t.set_u64("maxColumn", 0);
            t.set_u64("maxRow", 0);
            t.set_u64("numCells", 0);
            t.set_u64("numrows", 0);
            t.set_u64("storage_version", storage);
            t.set_bool("last_saved_in_BNC", true);
            let mut a = Archive::new_object(id, t);
            a.refresh_object_references();
            // its own component, next to the table's other Tables/ files
            let locator = format!("Tables/Tile-{id}");
            let name = format!("Index/{locator}.iwa");
            let at = k.entries.iter().rposition(|e| e.name.starts_with("Index/Tables/")).map(|i| i + 1).unwrap_or(k.entries.len());
            k.entries.insert(at, Entry { name: name.clone(), data: EntryData::Iwa(IwaFile { archives: vec![a] }) });
            k.reindex();
            let mut comp = comp_template.clone();
            comp.set_u64("identifier", id);
            comp.set_str("locator", &locator);
            comp.set_str("preferred_locator", "Tables/Tile");
            for f in ["external_references", "data_references", "object_uuid_map_entries"] {
                comp.remove(f);
            }
            if let Some(pm) = k.package_metadata_mut() {
                pm.push_msg("components", comp);
            }
            // listed in the data store and its row tile tree
            if let Some(ds) = k.archive_mut(model).and_then(|a| a.obj_mut().msg_mut("base_data_store")) {
                let tiles = ds.msg_mut_or_create("tiles");
                let e = tiles.push_new("tiles");
                e.set_u64("tileid", u64::from(tileid));
                e.set_ref("tile", id);
                // the next tile id to hand out (Keynote / Numbers: 2 once tile 1 exists, N10)
                if ds.u64("nextRowStripID").unwrap_or(0) < u64::from(tileid) + 1 {
                    ds.set_u64("nextRowStripID", u64::from(tileid) + 1);
                }
                let tree = ds.msg_mut_or_create("rowTileTree");
                if !tree.msgs("nodes").any(|n| n.u32("value") == Some(tileid)) {
                    let n = tree.push_new("nodes");
                    n.set_u64("key", u64::from(tileid * size));
                    n.set_u64("value", u64::from(tileid));
                }
            }
            k.refresh_refs(model);
            k.ensure_external_refs(&model_file);
            k.ensure_external_refs(&name);
            Ok(id)
        })
    }

    /// Put every row of the table's cell tiles into tile 0, numbered table-wide (a row edit
    /// then works on one tile).
    pub(crate) fn gather_tiles(&mut self, h: &mut History, table: ObjId) -> Result<(), kn_format::Error> {
        let (_, size, list) = tile_list(self, table).ok_or(err("no cell tiles"))?;
        let Some(first) = list.iter().find(|(i, _)| *i == 0).map(|x| x.1) else { return Err(err("no first cell tile")) };
        let others: Vec<(u32, ObjId)> = list.into_iter().filter(|(i, _)| *i != 0).collect();
        if others.iter().all(|(_, t)| self.file.obj(*t).is_none_or(|m| m.count("rowInfos") == 0)) {
            return Ok(());
        }
        self.touch(h, first);
        for (id, t) in &others {
            self.touch(h, *t);
            let rows: Vec<Msg> = self.file.obj(*t).map(|m| m.msgs("rowInfos").cloned().collect()).unwrap_or_default();
            if let Some(m) = self.file.obj_mut(*t) {
                m.remove("rowInfos");
                set_numrows(m);
            }
            if let Some(f) = self.file.obj_mut(first) {
                for mut ri in rows {
                    let local = ri.u32("tile_row_index").unwrap_or(0);
                    ri.set_u64("tile_row_index", u64::from(id * size + local));
                    f.push_msg("rowInfos", ri);
                }
            }
        }
        if let Some(f) = self.file.obj_mut(first) {
            let mut rows: Vec<Msg> = f.msgs("rowInfos").cloned().collect();
            rows.sort_by_key(|r| r.u32("tile_row_index").unwrap_or(0));
            f.remove("rowInfos");
            for r in rows {
                f.push_msg("rowInfos", r);
            }
            set_numrows(f);
        }
        Ok(())
    }

    /// Deal tile 0's rows out to the tiles their table-wide index belongs in (tile_size rows
    /// each), making tiles as needed.
    pub(crate) fn scatter_tiles(&mut self, h: &mut History, table: ObjId) -> Result<(), kn_format::Error> {
        let (_, size, list) = tile_list(self, table).ok_or(err("no cell tiles"))?;
        let Some(first) = list.iter().find(|(i, _)| *i == 0).map(|x| x.1) else { return Err(err("no first cell tile")) };
        let far = self.file.obj(first).is_some_and(|f| f.msgs("rowInfos").any(|r| r.u32("tile_row_index").unwrap_or(0) >= size));
        if !far {
            return Ok(());
        }
        self.touch(h, first);
        let rows: Vec<Msg> = self.file.obj(first).map(|m| m.msgs("rowInfos").cloned().collect()).unwrap_or_default();
        let mut by_tile: std::collections::BTreeMap<u32, Vec<Msg>> = std::collections::BTreeMap::new();
        for mut ri in rows {
            let row = ri.u32("tile_row_index").unwrap_or(0);
            ri.set_u64("tile_row_index", u64::from(row % size));
            by_tile.entry(row / size).or_default().push(ri);
        }
        for (id, rows) in by_tile.iter().filter(|(id, _)| **id != 0) {
            let t = match tile_list(self, table).and_then(|(_, _, l)| l.into_iter().find(|(i, _)| i == id)) {
                Some((_, t)) => t,
                None => self.add_tile(h, table, *id)?,
            };
            self.touch(h, t);
            if let Some(m) = self.file.obj_mut(t) {
                for r in rows {
                    m.push_msg("rowInfos", r.clone());
                }
                let mut all: Vec<Msg> = m.msgs("rowInfos").cloned().collect();
                all.sort_by_key(|r| r.u32("tile_row_index").unwrap_or(0));
                m.remove("rowInfos");
                for r in all {
                    m.push_msg("rowInfos", r);
                }
                set_numrows(m);
            }
        }
        if let Some(f) = self.file.obj_mut(first) {
            f.remove("rowInfos");
            for r in by_tile.remove(&0).unwrap_or_default() {
                f.push_msg("rowInfos", r);
            }
            set_numrows(f);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::tablecells::CellInput;
    use crate::tablerows::Axis;
    use crate::*;

    fn first_table(d: &Document) -> ObjId {
        d.file.first_of("TST.TableInfoArchive").map(|(i, _)| i).unwrap()
    }

    /// A Keynote table grown past 256 rows: cells in two tiles, values where they were put,
    /// saved and read back, rows inserted / deleted across the tile boundary, undo.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn table_over_256_rows() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/05-table.key");
        let Ok(mut d) = Document::open(&path) else { return };
        let t = first_table(&d);
        let mut h = edit::History::default();
        let rows = |d: &Document| d.file.obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|m| d.file.obj(m)).and_then(|m| m.u32("number_of_rows")).unwrap();
        d.begin(&mut h, "grow");
        while rows(&d) < 300 {
            let n = rows(&d);
            d.table_insert(&mut h, t, Axis::Row, n, n - 1).unwrap();
        }
        for r in [1u32, 255, 256, 257, 299] {
            d.set_cell(&mut h, t, r, 0, &CellInput::Number(f64::from(r))).unwrap();
        }
        d.end(&mut h);
        let cell = |d: &Document, r: u32| {
            let td = crate::table::table(&d.file, d.file.obj(t).unwrap()).unwrap();
            td.cells[(r * td.cols) as usize].clone()
        };
        for r in [1u32, 255, 256, 257, 299] {
            assert_eq!(cell(&d, r), r.to_string(), "row {r}");
        }
        let tiles = |d: &Document| super::tile_list(d, t).unwrap().2.len();
        assert_eq!(tiles(&d), 2);
        let strip = |d: &Document| d.file.obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|m| d.file.obj(m)).and_then(|m| m.msg("base_data_store")?.u64("nextRowStripID"));
        assert_eq!(strip(&d), Some(2));
        // an inserted row at 10 pushes 255 into tile 1 and 299 to 300
        d.begin(&mut h, "insert");
        d.table_insert(&mut h, t, Axis::Row, 10, 9).unwrap();
        d.end(&mut h);
        assert_eq!(cell(&d, 256), "255");
        assert_eq!(cell(&d, 300), "299");
        assert_eq!(cell(&d, 1), "1");
        // and deleting row 5 brings it back
        d.begin(&mut h, "delete");
        d.table_delete(&mut h, t, Axis::Row, 5).unwrap();
        d.end(&mut h);
        assert_eq!(cell(&d, 255), "255");
        assert_eq!(cell(&d, 299), "299");
        // formulas far down: their calculation records sit in the engine tiles covering them
        d.begin(&mut h, "formulas");
        d.set_formula_text(&mut h, t, 150, 1, "=A2+1").unwrap();
        d.set_formula_text(&mut h, t, 299, 1, "=A300*2").unwrap();
        d.end(&mut h);
        let td = crate::table::table(&d.file, d.file.obj(t).unwrap()).unwrap();
        assert_eq!(td.cells[(299 * td.cols + 1) as usize], "598");
        let begins: Vec<(u64, u64)> = d
            .file
            .objects_of("TSCE.CellRecordTileArchive")
            .filter(|(_, m)| m.msgs("cell_records").any(|r| r.u32("row").is_some_and(|r| r == 150 || r == 299)))
            .map(|(_, m)| (m.u64("tile_row_begin").unwrap_or(0), m.u64("tile_column_begin").unwrap_or(0)))
            .collect();
        assert!(begins.contains(&(128, 0)) && begins.contains(&(256, 0)), "{begins:?}");
        // saved and reopened (a copy stays in the temp folder for key_validate)
        let out = std::env::temp_dir().join(format!("kn-tiles-{}.key", std::process::id()));
        d.save_as(&out).unwrap();
        let _ = std::fs::copy(&out, std::env::temp_dir().join("kn-big-table-300-rows.key"));
        let back = Document::open(&out).unwrap();
        assert_eq!(cell(&back, 256), "256");
        assert_eq!(cell(&back, 299), "299");
        let _ = std::fs::remove_file(&out);
        // undo all the way: one tile again
        while d.undo(&mut h) {}
        assert_eq!(tiles(&d), 1);
        assert!(rows(&d) < 256);
    }

    /// Deleting a table takes it out of the calculation engine (Numbers' own delete, N3):
    /// 05-table.key goes back to 00-base.key's one formula record; undo brings it back.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn delete_table_cleans_engine() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/05-table.key");
        let Ok(mut d) = Document::open(&path) else { return };
        let t = first_table(&d);
        let slide = d.file.obj(t).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|m| m.ref_id("parent")).unwrap();
        let count = |d: &Document| {
            let (_, ce) = d.file.first_of("TSCE.CalculationEngineArchive").unwrap();
            let tr = ce.msg("dependency_tracker").unwrap();
            (tr.ref_ids("formula_owner_dependencies").len(), tr.u64("number_of_formulas"))
        };
        let before = count(&d);
        let mut h = edit::History::default();
        d.begin(&mut h, "delete");
        d.delete_drawables(&mut h, slide, &[t]);
        d.end(&mut h);
        assert_eq!(count(&d).1, Some(1));
        assert!(count(&d).0 < before.0);
        let out = std::env::temp_dir().join(format!("kn-deltable-{}.key", std::process::id()));
        d.save_as(&out).unwrap();
        let back = Document::open(&out).unwrap();
        assert_eq!(count(&back).1, Some(1));
        let _ = std::fs::remove_file(&out);
        d.undo(&mut h);
        assert_eq!(count(&d), before);
    }

    /// Keynote's own 300-row table (Mac round N10) has the layout ours makes.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn keynote_reference_layout() {
        let path = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("CascadeProjects/numbers-research-src/out-N10/K-keynote-table-300-rows.key");
        let Ok(d) = Document::open(&path) else { return };
        let t = first_table(&d);
        let ds = d.file.obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|m| d.file.obj(m)).and_then(|m| m.msg("base_data_store").cloned()).unwrap();
        assert_eq!(ds.u64("nextRowStripID"), Some(2));
        let tree: Vec<(u64, u64)> = ds.msg("rowTileTree").unwrap().msgs("nodes").map(|n| (n.u64("key").unwrap_or(0), n.u64("value").unwrap_or(0))).collect();
        assert_eq!(tree, [(0, 0), (256, 1)]);
        assert_eq!(super::tile_list(&d, t).unwrap().2.iter().map(|x| x.0).collect::<Vec<_>>(), [0, 1]);
        let td = crate::table::table(&d.file, d.file.obj(t).unwrap()).unwrap();
        assert_eq!(td.rows, 300);
    }
}
