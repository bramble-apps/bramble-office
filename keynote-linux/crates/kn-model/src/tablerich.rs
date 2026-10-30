//! Rich text cells: a cell whose text has its own fonts, sizes and styles (one word bold, a
//! line in a smaller size …), stored as Keynote, Pages and Numbers do: the cell record (type 9,
//! field 0x10) keys an entry of the table's RICH_TEXT_PAYLOAD list, whose
//! TST.RichTextPayloadArchive holds a TSWP storage of kind CELL. The storage is then styled
//! like any other text ([`Document::set_text_prop_range`], [`Document::set_para_prop_range`]).

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::tablecells::CellInput;
use crate::Document;

/// TST.TableDataList listType for rich text payloads.
const RICH_TEXT_PAYLOAD: u64 = 8;

impl Document {
    /// Make cell (row, col) of `table` a rich text cell holding `text` (paragraphs split at
    /// '\n'), in the cell's text style; returns its storage, to style ranges of.
    pub fn set_cell_rich(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, text: &str) -> Result<ObjId, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let model_id = self.file.obj(table).and_then(|m| m.ref_id("tableModel")).ok_or(err("not a table"))?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?;
        let ds = model.msg("base_data_store").ok_or(err("no data store"))?;
        let strings = ds.ref_id("stringTable").ok_or(err("no string table"))?;
        let header_rows = model.u32("number_of_header_rows").unwrap_or(0);
        let header_cols = model.u32("number_of_header_columns").unwrap_or(0);
        let rows = model.u32("number_of_rows").unwrap_or(0);
        let footer_rows = model.u32("number_of_footer_rows").unwrap_or(0);
        // the cell's text style: its region's (the paragraph style its runs start from)
        let region_style = if row < header_rows {
            "header_row_text_style"
        } else if row + footer_rows >= rows && footer_rows > 0 {
            "footer_row_text_style"
        } else if col < header_cols {
            "header_column_text_style"
        } else {
            "body_text_style"
        };
        let para_style = model.ref_id(region_style).or(model.ref_id("body_text_style"));
        let file = self.file.file_of(strings).ok_or(err("string table file"))?.to_string();
        let existing = ds.ref_id("rich_text_table");

        // the payload list (made on first use, beside the string table)
        let list = match existing {
            Some(l) => l,
            None => {
                let id = self.file.alloc_ids(1)[0];
                let mut l = Msg::new("TST.TableDataList");
                l.set_u64("listType", RICH_TEXT_PAYLOAD);
                l.set_u64("nextListID", 1);
                self.file.insert_archive(&file, Archive::new_object(id, l));
                self.touch(h, model_id);
                let a = self.file.archive_mut(model_id).ok_or(err("table model"))?;
                a.obj_mut().msg_mut_or_create("base_data_store").set_ref("rich_text_table", id);
                a.refresh_object_references();
                id
            }
        };
        self.touch(h, list);

        // the storage, as Pages writes a cell's (kind CELL, one entry per attribute table)
        let sheet = self.file.first_of("TSWP.StorageArchive").and_then(|(_, m)| m.ref_id("style_sheet"));
        let list_style = self.style_by_identifier("TSWP.ListStyleArchive", "text-0-liststyle-None");
        let ids = self.file.alloc_ids(2);
        let (sid, pid) = (ids[0], ids[1]);
        let mut st = Msg::new("TSWP.StorageArchive");
        st.set_enum("kind", "CELL");
        if let Some(s) = sheet {
            st.set_ref("style_sheet", s);
        }
        st.set_str("text", text);
        let entry = |obj: Option<ObjId>| {
            let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
            e.set_u64("character_index", 0);
            if let Some(o) = obj {
                e.set_ref("object", o);
            }
            e
        };
        st.msg_mut_or_create("table_para_style").push_msg("entries", entry(para_style));
        st.msg_mut_or_create("table_list_style").push_msg("entries", entry(list_style));
        st.msg_mut_or_create("table_char_style").push_msg("entries", entry(None));
        for t in ["table_para_data", "table_para_starts", "table_para_bidi"] {
            let mut e = Msg::new("TSWP.ParaDataAttributeTable.ParaDataAttribute");
            e.set_u64("character_index", 0);
            e.set_u64("first", 0);
            e.set_u64("second", 0);
            st.msg_mut_or_create(t).push_msg("entries", e);
        }
        st.set_bool("in_document", true);
        self.file.insert_archive(&file, Archive::new_object(sid, st));
        if let Some(a) = self.file.archive_mut(sid) {
            a.refresh_object_references();
        }
        let mut p = Msg::new("TST.RichTextPayloadArchive");
        p.set_ref("storage", sid);
        self.file.insert_archive(&file, Archive::new_object(pid, p));
        if let Some(a) = self.file.archive_mut(pid) {
            a.refresh_object_references();
        }
        let key = {
            let a = self.file.archive_mut(list).ok_or(err("rich text list"))?;
            let key = crate::tablecells::list_add(a.obj_mut(), |_| false, |e| e.set_ref("rich_text_payload", pid));
            a.refresh_object_references();
            key
        };
        self.file.refresh_refs(sid);
        self.set_cell(h, table, row, col, &CellInput::RichText(key))?;
        Ok(sid)
    }
}

#[cfg(test)]
mod tests {
    use crate::format::TextProp;
    use crate::{Document, DrawableKind};

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn rich_cells_keep_their_runs() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/05-table.key");
        let mut d = Document::open(&p).unwrap();
        let s = d.slides();
        let id = d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let mut h = crate::edit::History::default();
        d.begin(&mut h, "Rich");
        let st = d.set_cell_rich(&mut h, id, 1, 1, "Name\nline two").unwrap();
        assert!(d.set_text_prop_range(&mut h, st, 0, 4, &TextProp::Bold(true)));
        assert!(d.set_text_prop_range(&mut h, st, 5, 13, &TextProp::Size(8.0)));
        d.end(&mut h);
        // saved and opened again
        let out = std::env::temp_dir().join(format!("kn-rich-{}.key", std::process::id()));
        d.save_as(&out).unwrap();
        let d2 = Document::open(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        let s2 = d2.slides();
        let sc = d2.scene(&s2, 0).unwrap();
        let DrawableKind::Table(t) = &sc.drawables.iter().find(|x| x.id == id).unwrap().kind else { unreachable!() };
        let i = (t.cols + 1) as usize;
        assert_eq!(t.cells[i], "Name\nline two");
        let tb = t.rich[i].as_ref().expect("a rich cell");
        let run_at = |at: usize| tb.paragraphs.iter().flat_map(|p| p.runs.iter()).find(|r| r.start <= at && at < r.end).unwrap().style.clone();
        assert!(run_at(1).bold);
        assert!(!run_at(6).bold);
        assert!((run_at(6).size - 8.0).abs() < 0.01);
    }
}
