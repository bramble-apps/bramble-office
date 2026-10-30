//! Adding a comment to a cell (research 34-comment): a TSD.CommentStorageArchive in the file of
//! the table's COMMENT_STORAGE list, an entry {key, refcount, comment_storage} in that list, and
//! the key in the cell record (flag 0x80000, after every lower field). The author is a
//! TSK.AnnotationAuthorArchive in the document's author storage (as Keydeck's comments).

use kn_format::{Archive, Msg, ObjId};
use kn_model::edit::History;
use kn_model::tablecells::CellInput;
use kn_model::Document;

fn bad(s: &str) -> kn_format::Error {
    kn_format::Error::Invalid(s.into())
}

/// Add `text` by `author` as the comment of cell (row, col) (model row). The cell must have a
/// value; a comment it already has is replaced.
pub fn add_cell_comment(doc: &mut Document, h: &mut History, t: ObjId, row: u32, col: u32, text: &str, author: &str) -> Result<(), kn_format::Error> {
    let model = doc.file.obj(t).and_then(|m| m.ref_id("tableModel")).ok_or(bad("not a table"))?;
    let list = doc.file.obj(model).and_then(|m| m.msg("base_data_store")).and_then(|d| d.ref_id("commentStorageTable")).ok_or(bad("the table has no comment list"))?;
    let mut rec = doc
        .file
        .obj(model)
        .and_then(|m| kn_model::table::raw_cell_records(&doc.file, m).into_iter().find(|(rc, _)| *rc == (row, col)))
        .map(|x| x.1)
        .ok_or(bad("comments go on cells with something in them for now"))?;
    if rec.first() != Some(&5) || rec.len() < 12 {
        return Err(bad("this cell can't take a comment yet"));
    }
    let flags = u32::from_le_bytes(rec[8..12].try_into().unwrap());
    if flags >> 20 != 0 {
        return Err(bad("this cell can't take a comment yet"));
    }
    let (store, _) = doc.file.first_of("TSK.AnnotationAuthorStorageArchive").ok_or(bad("no author storage"))?;
    let list_file = doc.file.file_of(list).ok_or(bad("no comment list file"))?.to_string();
    let store_file = doc.file.file_of(store).ok_or(bad("no author file"))?.to_string();
    let existing_author = doc.file.obj(store).map(|s| s.ref_ids("annotation_author")).unwrap_or_default().into_iter().find(|a| doc.file.obj(*a).and_then(|m| m.str("name")) == Some(author));
    let key = doc.file.obj(list).and_then(|l| l.u32("nextListID")).unwrap_or(1).max(1);
    let had = flags & 0x80000 != 0;
    let text = text.to_string();
    let author = author.to_string();
    doc.structural(h, &[list, store], |k| -> Result<(), kn_format::Error> {
        let author_id = match existing_author {
            Some(a) => a,
            None => {
                let id = k.alloc_ids(1)[0];
                let mut a = Msg::new("TSK.AnnotationAuthorArchive");
                a.set_str("name", &author);
                let mut c = Msg::new("TSP.Color");
                c.set_enum("model", "rgb");
                for (n, v) in [("r", 0.98039216), ("g", 0.9372549), ("b", 0.3529412), ("a", 1.0)] {
                    c.set_f32(n, v);
                }
                c.set_enum("rgbspace", "srgb");
                a.push_msg("color", c);
                a.set_bool("is_public_author", false);
                let mut ar = Archive::new_object(id, a);
                ar.refresh_object_references();
                k.insert_archive(&store_file, ar);
                if let Some(sa) = k.archive_mut(store) {
                    sa.obj_mut().push_ref("annotation_author", id);
                    sa.refresh_object_references();
                }
                id
            }
        };
        let cid = k.alloc_ids(1)[0];
        let mut c = Msg::new("TSD.CommentStorageArchive");
        c.set_str("text", &text);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let mut date = Msg::new("TSP.Date");
        date.set_f64("seconds", now - 978_307_200.0);
        c.push_msg("creation_date", date);
        c.set_ref("author", author_id);
        kn_format::objects::new_uuid(c.msg_mut_or_create("storage_uuid"));
        let mut ca = Archive::new_object(cid, c);
        ca.refresh_object_references();
        k.insert_archive(&list_file, ca);
        k.add_uuid_entry(&list_file, cid);
        if let Some(la) = k.archive_mut(list) {
            let l = la.obj_mut();
            let e = l.push_new("entries");
            e.set_u64("key", key as u64);
            e.set_u64("refcount", 1);
            e.set_ref("comment_storage", cid);
            l.set_u64("nextListID", key as u64 + 1);
            la.refresh_object_references();
        }
        k.ensure_external_refs(&list_file);
        k.ensure_external_refs(&store_file);
        Ok(())
    })?;
    // the cell refers to it: the comment field is the last one
    if had {
        let n = rec.len();
        rec[n - 4..].copy_from_slice(&key.to_le_bytes());
    } else {
        rec[8..12].copy_from_slice(&(flags | 0x80000).to_le_bytes());
        rec.extend_from_slice(&key.to_le_bytes());
    }
    doc.set_cell(h, t, row, col, &CellInput::Raw(rec))
}

/// Remove the comment of cell (row, col) (model row), if it has one: the key goes from the
/// cell record (flag 0x80000 and its field), and its comment list entry is released (dropped
/// when no cell uses it any more). Returns whether there was one.
pub fn remove_cell_comment(doc: &mut Document, h: &mut History, t: ObjId, row: u32, col: u32) -> Result<bool, kn_format::Error> {
    let model = doc.file.obj(t).and_then(|m| m.ref_id("tableModel")).ok_or(bad("not a table"))?;
    let Some(mut rec) = doc.file.obj(model).and_then(|m| kn_model::table::raw_cell_records(&doc.file, m).into_iter().find(|(rc, _)| *rc == (row, col))).map(|x| x.1) else { return Ok(false) };
    if rec.first() != Some(&5) || rec.len() < 12 {
        return Ok(false);
    }
    let flags = u32::from_le_bytes(rec[8..12].try_into().unwrap());
    if flags & 0x80000 == 0 {
        return Ok(false);
    }
    // the comment key's place: after every lower field
    let mut at = 12;
    for bit in 0..19 {
        if flags & (1 << bit) != 0 {
            at += match bit {
                0 => 16,
                1 | 2 => 8,
                _ => 4,
            };
        }
    }
    let Some(key) = rec.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())) else { return Ok(false) };
    rec.drain(at..at + 4);
    rec[8..12].copy_from_slice(&(flags & !0x80000).to_le_bytes());
    if let Some(list) = doc.file.obj(model).and_then(|m| m.msg("base_data_store")).and_then(|d| d.ref_id("commentStorageTable")) {
        doc.touch(h, list);
        if let Some(l) = doc.file.obj_mut(list) {
            let mut gone = false;
            if let Some(e) = l.msgs_mut("entries").find(|e| e.u32("key") == Some(key)) {
                let rc = e.u32("refcount").unwrap_or(1);
                if rc > 1 {
                    e.set_u64("refcount", u64::from(rc - 1));
                } else {
                    gone = true;
                }
            }
            if gone {
                l.retain_msgs("entries", |e| e.u32("key") != Some(key));
            }
        }
        doc.file.refresh_refs(list);
    }
    doc.set_cell(h, t, row, col, &CellInput::Raw(rec))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn add_and_read_back() {
        let mut wb = crate::Workbook::open(format!("{}/../../testdata/29-base-data.numbers", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let mut h = kn_model::edit::History::default();
        wb.doc.begin(&mut h, "c");
        super::add_cell_comment(&mut wb.doc, &mut h, t, 2, 2, "Checked with the invoice", "Values").unwrap();
        wb.doc.end(&mut h);
        let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/n-edits"));
        std::fs::create_dir_all(&dir).unwrap();
        wb.doc.save_as(dir.join("N26-cell-comment.numbers")).unwrap();
        let back = crate::Workbook::open(dir.join("N26-cell-comment.numbers")).unwrap();
        let c = crate::cell_comments(back.file(), t);
        assert_eq!(c[&(2, 2)], ("Checked with the invoice".to_string(), "Values".to_string()));
        let td = back.table_data(t).unwrap();
        assert_eq!(td.cells[(2 * td.cols + 2) as usize], "120", "value kept");
        // and taken away again: no comment, the value kept
        let mut wb = back;
        wb.doc.begin(&mut h, "r");
        assert!(super::remove_cell_comment(&mut wb.doc, &mut h, t, 2, 2).unwrap());
        wb.doc.end(&mut h);
        wb.doc.save_as(dir.join("N26-cell-comment-removed.numbers")).unwrap();
        let back = crate::Workbook::open(dir.join("N26-cell-comment-removed.numbers")).unwrap();
        assert!(crate::cell_comments(back.file(), t).is_empty());
        let td = back.table_data(t).unwrap();
        assert_eq!(td.cells[(2 * td.cols + 2) as usize], "120", "value kept");
    }
}
