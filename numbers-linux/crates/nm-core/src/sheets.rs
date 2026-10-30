//! Sheet-level edits: rename, reorder, delete sheets; attach / detach drawables.
//!
//! TN.DocumentArchive.sheets lists the sheets in tab order and `sidebar_order` points to a
//! TSK.TreeNode tree (root -> one node per sheet {object: sheet} -> one node per drawable
//! {object: drawable}); a sheet's TN.SheetArchive.drawable_infos is its canvas, back to front
//! (research diffs 01-second-sheet, 02-second-table). Removed objects are only detached, as
//! Keydeck does for slides: their archives stay until the next save by Numbers cleans up.

use kn_format::{Archive, Msg, ObjId};
use kn_model::edit::History;
use kn_model::Document;

fn document(doc: &Document) -> Option<ObjId> {
    doc.file.first_of("TN.DocumentArchive").map(|(id, _)| id)
}

/// The sidebar tree's node for `object` among the children of `parent`.
fn child_node(doc: &Document, parent: ObjId, object: ObjId) -> Option<ObjId> {
    doc.file.obj(parent)?.ref_ids("children").into_iter().find(|n| doc.file.obj(*n).and_then(|m| m.ref_id("object")) == Some(object))
}

fn root_node(doc: &Document) -> Option<ObjId> {
    doc.file.obj(document(doc)?)?.ref_id("sidebar_order")
}

fn set_refs(doc: &mut Document, id: ObjId, field: &str, ids: &[ObjId]) {
    if let Some(a) = doc.file.archive_mut(id) {
        let m = a.obj_mut();
        m.remove(field);
        for i in ids {
            m.push_ref(field, *i);
        }
        a.refresh_object_references();
    }
}

pub fn sheet_ids(doc: &Document) -> Vec<ObjId> {
    document(doc).and_then(|d| doc.file.obj(d)).map(|d| d.ref_ids("sheets")).unwrap_or_default()
}

pub fn rename_sheet(doc: &mut Document, h: &mut History, sheet: ObjId, name: &str) {
    doc.touch(h, sheet);
    if let Some(m) = doc.file.obj_mut(sheet) {
        m.set_str("name", name);
    }
}

/// Move the sheet at `from` to position `to` (tab order and sidebar order).
pub fn move_sheet(doc: &mut Document, h: &mut History, from: usize, to: usize) -> bool {
    let Some(d) = document(doc) else { return false };
    let mut sheets = sheet_ids(doc);
    if from >= sheets.len() || to >= sheets.len() || from == to {
        return false;
    }
    let s = sheets.remove(from);
    sheets.insert(to, s);
    doc.touch(h, d);
    set_refs(doc, d, "sheets", &sheets);
    if let Some(root) = root_node(doc) {
        doc.touch(h, root);
        let nodes: Vec<ObjId> = sheets.iter().filter_map(|s| child_node(doc, root, *s)).collect();
        let others: Vec<ObjId> = doc.file.obj(root).map(|r| r.ref_ids("children")).unwrap_or_default().into_iter().filter(|n| !nodes.contains(n)).collect();
        let all: Vec<ObjId> = nodes.into_iter().chain(others).collect();
        set_refs(doc, root, "children", &all);
    }
    true
}

/// Delete a sheet (never the last one).
pub fn delete_sheet(doc: &mut Document, h: &mut History, index: usize) -> bool {
    let Some(d) = document(doc) else { return false };
    let mut sheets = sheet_ids(doc);
    if sheets.len() < 2 || index >= sheets.len() {
        return false;
    }
    let s = sheets.remove(index);
    let on_sheet: Vec<ObjId> = doc.file.obj(s).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default();
    forget_tables(doc, h, &on_sheet);
    doc.touch(h, d);
    set_refs(doc, d, "sheets", &sheets);
    if let Some(root) = root_node(doc) {
        doc.touch(h, root);
        let kids: Vec<ObjId> = doc.file.obj(root).map(|r| r.ref_ids("children")).unwrap_or_default();
        let node = child_node(doc, root, s);
        let kids: Vec<ObjId> = kids.into_iter().filter(|k| Some(*k) != node).collect();
        set_refs(doc, root, "children", &kids);
    }
    // the open sheet is remembered as an index
    if let Some((ui, m)) = doc.file.first_of("TN.UIStateArchive") {
        if m.u32("active_sheet_index").unwrap_or(0) as usize >= sheets.len() || index == m.u32("active_sheet_index").unwrap_or(0) as usize {
            doc.touch(h, ui);
            if let Some(m) = doc.file.obj_mut(ui) {
                m.set_u64("active_sheet_index", 0);
            }
        }
    }
    for f in doc.file.entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>() {
        if f == "Index/Document.iwa" || f == "Index/ViewState.iwa" {
            doc.file.ensure_external_refs(&f);
        }
    }
    true
}

/// Put drawable `id` on top of `sheet`'s canvas and in the sidebar tree.
pub fn attach_drawable(doc: &mut Document, h: &mut History, sheet: ObjId, id: ObjId) {
    doc.touch(h, sheet);
    if let Some(a) = doc.file.archive_mut(sheet) {
        let m = a.obj_mut();
        if !m.ref_ids("drawable_infos").contains(&id) {
            m.push_ref("drawable_infos", id);
        }
        a.refresh_object_references();
    }
    let Some(root) = root_node(doc) else { return };
    let Some(sheet_node) = child_node(doc, root, sheet) else { return };
    if child_node(doc, sheet_node, id).is_some() {
        return;
    }
    let Some(file) = doc.file.file_of(sheet_node).map(str::to_string) else { return };
    // the id is taken inside the structural edit, so undo knows the node was added
    doc.structural(h, &[sheet_node], |k| {
        let new_id = k.alloc_ids(1)[0];
        let mut node = Msg::new("TSK.TreeNode");
        node.set_ref("object", id);
        let mut a = Archive::new_object(new_id, node);
        a.refresh_object_references();
        k.insert_archive(&file, a);
        if let Some(sa) = k.archive_mut(sheet_node) {
            sa.obj_mut().push_ref("children", new_id);
            sa.refresh_object_references();
        }
        k.ensure_external_refs(&file);
    });
}

/// The calculation engine forgets deleted tables (their formula owners and records, N3).
fn forget_tables(doc: &mut Document, h: &mut History, ids: &[ObjId]) {
    let tables: Vec<ObjId> = ids.iter().copied().filter(|i| doc.file.type_name(*i) == "TST.TableInfoArchive").collect();
    if tables.is_empty() {
        return;
    }
    // archives go: the whole document is kept for undo
    doc.touch_all(h);
    for t in tables {
        doc.file.remove_table_engine(t);
    }
}

/// Take drawables off `sheet` (canvas and sidebar tree).
pub fn detach_drawables(doc: &mut Document, h: &mut History, sheet: ObjId, ids: &[ObjId]) {
    forget_tables(doc, h, ids);
    doc.touch(h, sheet);
    if let Some(a) = doc.file.archive_mut(sheet) {
        a.obj_mut().retain_msgs("drawable_infos", |r| !ids.contains(&r.u64("identifier").unwrap_or(0)));
        a.refresh_object_references();
    }
    let Some(root) = root_node(doc) else { return };
    let Some(sheet_node) = child_node(doc, root, sheet) else { return };
    let kids: Vec<ObjId> = doc.file.obj(sheet_node).map(|n| n.ref_ids("children")).unwrap_or_default();
    let keep: Vec<ObjId> = kids.into_iter().filter(|k| doc.file.obj(*k).and_then(|m| m.ref_id("object")).is_none_or(|o| !ids.contains(&o))).collect();
    doc.touch(h, sheet_node);
    set_refs(doc, sheet_node, "children", &keep);
}

/// Insert -> Sheet: a copy of sheet `model` (its print headers and footers, guides and page
/// setup) with an empty canvas, named `name`, placed at tab position `at`. Returns its id.
pub fn add_sheet(doc: &mut Document, h: &mut History, model: ObjId, name: &str, at: usize) -> Result<ObjId, kn_format::Error> {
    let bad = |s: &str| kn_format::Error::Invalid(s.into());
    let d = document(doc).ok_or(bad("no TN.DocumentArchive"))?;
    let root = root_node(doc).ok_or(bad("no sidebar tree"))?;
    let file = doc.file.file_of(model).ok_or(bad("no sheet"))?.to_string();
    let drawables: Vec<ObjId> = doc.file.obj(model).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default();
    let comp = doc.file.component_for_file(&file);
    // what the sheet owns in its own file, without its drawables
    let mut owned: Vec<ObjId> = Vec::new();
    let mut todo = vec![model];
    while let Some(id) = todo.pop() {
        if owned.contains(&id) || drawables.contains(&id) || Some(id) == comp || id == d || id == root {
            continue;
        }
        let Some(a) = doc.file.archive(id) else { continue };
        if doc.file.file_of(id) != Some(file.as_str()) || doc.file.type_name(id) == "TSK.TreeNode" {
            continue;
        }
        owned.push(id);
        let mut refs = Vec::new();
        for (i, p) in a.payloads.iter().enumerate() {
            p.collect_refs(&mut refs);
            refs.extend(a.object_references(i));
        }
        todo.extend(refs);
    }
    let had_uuid: Vec<ObjId> = owned.iter().copied().filter(|i| doc.file.object_uuid_msg(*i).is_some()).collect();
    let touch = [d, root];
    let sheet_node_file = doc.file.file_of(root).unwrap_or("Index/Document.iwa").to_string();
    let name = name.to_string();
    doc.structural(h, &touch, |k| {
        let new_ids = k.alloc_ids(owned.len() as u64 + 1);
        let map: std::collections::HashMap<ObjId, ObjId> = owned.iter().copied().zip(new_ids.iter().copied()).collect();
        let node_id = new_ids[owned.len()];
        let mut copies = Vec::new();
        for old in &owned {
            let mut a = k.archive(*old).unwrap().clone();
            a.info.set_u64("identifier", map[old]);
            for i in 0..a.payloads.len() {
                let refs: Vec<ObjId> = a.object_references(i).into_iter().filter(|r| !drawables.contains(r)).map(|r| *map.get(&r).unwrap_or(&r)).collect();
                a.set_object_references(i, &refs);
                a.payloads[i].for_each_ref_mut(&mut |r| {
                    if let Some(n) = map.get(r) {
                        *r = *n;
                    }
                });
            }
            copies.push(a);
        }
        let new_sheet = map[&model];
        if let Some(s) = copies.iter_mut().find(|a| a.id() == new_sheet) {
            let m = s.obj_mut();
            m.remove("drawable_infos");
            m.set_str("name", &name);
            s.refresh_object_references();
        }
        k.iwa_mut(&file).unwrap().archives.extend(copies);
        let mut node = Msg::new("TSK.TreeNode");
        node.set_ref("object", new_sheet);
        let mut na = Archive::new_object(node_id, node);
        na.refresh_object_references();
        k.iwa_mut(&sheet_node_file).unwrap().archives.push(na);
        k.reindex();
        for old in &had_uuid {
            k.add_uuid_entry(&file, map[old]);
        }
        if let Some(a) = k.archive_mut(d) {
            let m = a.obj_mut();
            let n = m.count("sheets");
            m.insert_msg("sheets", at.min(n), kn_format::reference(new_sheet));
            a.refresh_object_references();
        }
        if let Some(a) = k.archive_mut(root) {
            let m = a.obj_mut();
            let n = m.count("children");
            m.insert_msg("children", at.min(n), kn_format::reference(node_id));
            a.refresh_object_references();
        }
        k.ensure_external_refs(&file);
        if sheet_node_file != file {
            k.ensure_external_refs(&sheet_node_file);
        }
        Ok(new_sheet)
    })
}

/// Insert -> Table: a copy of table `donor_info` from `donor` (another Numbers document) on
/// `sheet` with its top-left corner at (x, y). Returns the new TST.TableInfoArchive id.
pub fn insert_table(doc: &mut Document, h: &mut History, sheet: ObjId, donor: &kn_format::KeyFile, donor_info: ObjId, x: f32, y: f32) -> Result<ObjId, kn_format::Error> {
    let mut touch = doc.file.table_insert_touches();
    touch.push(sheet);
    let id = doc.structural(h, &touch, |k| k.paste_table(donor, donor_info, sheet))?;
    attach_drawable(doc, h, sheet, id);
    // a name no other table has ("Table 2"…)
    let names: Vec<String> = doc.file.objects_of("TST.TableModelArchive").filter_map(|(_, m)| m.str("table_name").map(str::to_string)).collect();
    if let Some(model) = doc.file.obj(id).and_then(|m| m.ref_id("tableModel")) {
        let mine = doc.file.obj(model).and_then(|m| m.str("table_name")).unwrap_or("").to_string();
        if names.iter().filter(|n| **n == mine).count() > 1 {
            let name = (1..).map(|n| format!("Table {n}")).find(|n| !names.contains(n)).unwrap();
            doc.touch(h, model);
            if let Some(m) = doc.file.obj_mut(model) {
                m.set_str("table_name", &name);
            }
        }
    }
    let mut g = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
    g.x = x;
    g.y = y;
    doc.set_frame(h, id, g);
    Ok(id)
}

/// The donor of new tables: the Blank template's table.
pub fn blank_donor() -> Result<(kn_format::KeyFile, ObjId), kn_format::Error> {
    crate::init();
    let k = kn_format::KeyFile::from_zip(crate::BLANK)?;
    let info = k.first_of("TST.TableInfoArchive").map(|(id, _)| id).ok_or(kn_format::Error::Invalid("Blank has no table".into()))?;
    Ok((k, info))
}

/// The donor of new text boxes: Numbers' own text box (research sample 27), Apple-made, local use.
pub fn text_box_donor() -> Result<(kn_format::KeyFile, ObjId), kn_format::Error> {
    crate::init();
    let k = kn_format::KeyFile::from_zip(include_bytes!("../data/TextBox.numbers"))?;
    let id = k.first_of("TSWP.ShapeInfoArchive").map(|(id, _)| id).ok_or(kn_format::Error::Invalid("no text box".into()))?;
    Ok((k, id))
}

/// A text box's path as Numbers' Text button writes it: a closed 100 x 100 rectangle (Mac N13,
/// N-ui-text-box; an AppleScript-made one had moveTo (0, 0), lineTo (100, 100), N9).
pub fn set_text_box_path(doc: &mut Document, h: &mut History, id: ObjId) {
    doc.touch(h, id);
    let Some(m) = doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) else { return };
    let Some(p) = m.msg_mut("pathsource").and_then(|ps| ps.msg_mut("bezier_path_source")).map(|b| b.msg_mut_or_create("path")) else { return };
    p.remove("elements");
    let steps = [("moveTo", Some((0.0f32, 0.0f32))), ("lineTo", Some((100.0, 0.0))), ("lineTo", Some((100.0, 100.0))), ("lineTo", Some((0.0, 100.0))), ("closeSubpath", None), ("moveTo", Some((0.0, 0.0)))];
    for (ty, pt) in steps {
        let e = p.push_new("elements");
        e.set_enum("type", ty);
        if let Some((x, y)) = pt {
            let pt = e.push_new("points");
            pt.set_f32("x", x);
            pt.set_f32("y", y);
        }
    }
}

/// Insert -> Text Box (or any drawable of another Numbers document): a copy of `donor_id` on
/// `sheet` at (x, y), its styles mapped onto this document's. Returns the new drawable.
pub fn insert_object(doc: &mut Document, h: &mut History, sheet: ObjId, donor: &kn_format::KeyFile, donor_id: ObjId, x: f32, y: f32) -> Result<ObjId, kn_format::Error> {
    let ex = donor.extract(donor_id)?;
    if !kn_format::KeyFile::extract_data_ids(&ex).is_empty() {
        return Err(kn_format::Error::Invalid("objects with pictures can't be copied yet".into()));
    }
    let map = doc.file.map_external(donor, &ex.external)?;
    let id = doc.structural(h, &[sheet], |k| k.paste_extract(&ex, sheet, Some(&map)))?;
    attach_drawable(doc, h, sheet, id);
    let mut g = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
    g.x = x;
    g.y = y;
    doc.set_frame(h, id, g);
    Ok(id)
}

/// Duplicate Sheet: a copy of sheet `index` right after it, named "<name> copy", with copies of
/// its tables (fresh table identities, as a pasted table) and other objects.
pub fn duplicate_sheet(doc: &mut Document, h: &mut History, index: usize) -> Result<ObjId, kn_format::Error> {
    let ids = sheet_ids(doc);
    let src = *ids.get(index).ok_or(kn_format::Error::Invalid("no such sheet".into()))?;
    let name = doc.file.obj(src).and_then(|m| m.str("name")).unwrap_or("Sheet").to_string();
    let snapshot = doc.file.clone();
    let drawables: Vec<ObjId> = snapshot.obj(src).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default();
    let new = add_sheet(doc, h, src, &format!("{name} copy"), index + 1)?;
    // one uuid family map for all the sheet's tables: formulas in one copy that name another
    // table of the sheet then name its copy (as Numbers' Duplicate does, N9)
    let mut families = std::collections::HashMap::new();
    for d in drawables {
        let g = snapshot.obj(d).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
        if snapshot.type_name(d) == "TST.TableInfoArchive" {
            let mut touch = doc.file.table_insert_touches();
            touch.push(new);
            let id = doc.structural(h, &touch, |k| k.paste_table_with(&snapshot, d, new, &mut families))?;
            attach_drawable(doc, h, new, id);
            doc.set_frame(h, id, g);
        } else {
            let ex = snapshot.extract(d)?;
            if !kn_format::KeyFile::extract_data_ids(&ex).is_empty() {
                continue; // pictures: not yet
            }
            let id = doc.structural(h, &[new], |k| k.paste_extract(&ex, new, None))?;
            attach_drawable(doc, h, new, id);
            doc.set_frame(h, id, g);
        }
    }
    // the copies' formula edges into each other point at the copies' owners (Mac N13)
    if !families.is_empty() {
        let touch = doc.file.table_insert_touches();
        doc.structural(h, &touch, |k| k.relink_pasted_tables(&families));
    }
    Ok(new)
}

/// The toolbar Table popover's first page, as Numbers 15.4 inserts them (Mac round N2).
pub const TABLE_STYLES: [&str; 6] = ["Headers", "Basic", "Plain", "Headers and Footers", "Checklist", "Stocks"];

/// Donor of Table popover style `i` (0..6): the table Numbers inserted into a Blank document.
pub fn table_style_donor(i: usize) -> Result<(kn_format::KeyFile, ObjId), kn_format::Error> {
    crate::init();
    let bytes: &[u8] = match i {
        0 => include_bytes!("../data/TableStyle1.numbers"),
        1 => include_bytes!("../data/TableStyle2.numbers"),
        2 => include_bytes!("../data/TableStyle3.numbers"),
        3 => include_bytes!("../data/TableStyle4.numbers"),
        4 => include_bytes!("../data/TableStyle5.numbers"),
        _ => include_bytes!("../data/TableStyle6.numbers"),
    };
    let k = kn_format::KeyFile::from_zip(bytes)?;
    // the second table on Sheet 1 (the first is Blank's own)
    let doc = Document::from_file(k);
    let s = sheet_ids(&doc).first().copied().ok_or(kn_format::Error::Invalid("no sheet".into()))?;
    let id = doc.file.obj(s).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default().into_iter().filter(|d| doc.file.type_name(*d) == "TST.TableInfoArchive").nth(1).ok_or(kn_format::Error::Invalid("no inserted table".into()))?;
    Ok((doc.file, id))
}

/// Donor of a new sheet's table: the 10 × 5 table Numbers puts on a sheet made with "+".
pub fn new_sheet_table_donor() -> Result<(kn_format::KeyFile, ObjId), kn_format::Error> {
    crate::init();
    let doc = Document::from_file(kn_format::KeyFile::from_zip(include_bytes!("../data/NewSheet.numbers"))?);
    let s = *sheet_ids(&doc).get(1).ok_or(kn_format::Error::Invalid("no Sheet 2".into()))?;
    let id = doc.file.obj(s).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default().into_iter().find(|d| doc.file.type_name(*d) == "TST.TableInfoArchive").ok_or(kn_format::Error::Invalid("no table".into()))?;
    Ok((doc.file, id))
}

/// The toolbar Shape popover's shapes we offer, in its order (Numbers 15.4, Mac round N2 / N13).
pub const SHAPES: [&str; 5] = ["Line", "Rounded Rectangle", "Circle", "Square", "Arrow"];

/// Donor of shape `i` of [`SHAPES`]: the shape Numbers inserted from the Shape popover into a
/// Blank document (Mac round N13, N-ui-shape-*), with its default style.
pub fn shape_donor(i: usize) -> Result<(kn_format::KeyFile, ObjId), kn_format::Error> {
    crate::init();
    let bytes: &[u8] = match i {
        0 => include_bytes!("../data/Shape-line.numbers"),
        1 => include_bytes!("../data/Shape-rounded-rectangle.numbers"),
        2 => include_bytes!("../data/Shape-circle.numbers"),
        3 => include_bytes!("../data/Shape-square.numbers"),
        _ => include_bytes!("../data/Shape-arrow.numbers"),
    };
    let doc = Document::from_file(kn_format::KeyFile::from_zip(bytes)?);
    let s = sheet_ids(&doc).first().copied().ok_or(kn_format::Error::Invalid("no sheet".into()))?;
    let id = doc.file.obj(s).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default().into_iter().find(|d| doc.file.type_name(*d) != "TST.TableInfoArchive");
    Ok((doc.file, id.ok_or(kn_format::Error::Invalid("no shape in the donor".into()))?))
}

/// Numbers' default objects (Mac round N2 donor): the rectangle and the picture on Sheet 1.
pub fn object_donor(kind: &str) -> Result<(kn_format::KeyFile, ObjId), kn_format::Error> {
    crate::init();
    let doc = Document::from_file(kn_format::KeyFile::from_zip(include_bytes!("../data/Objects.numbers"))?);
    let s = sheet_ids(&doc).first().copied().ok_or(kn_format::Error::Invalid("no sheet".into()))?;
    let ds = doc.file.obj(s).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default();
    let id = match kind {
        "image" => ds.iter().copied().find(|d| doc.file.type_name(*d) == "TSD.ImageArchive"),
        // the shape that isn't a text box (the text box has text, the rectangle has none)
        _ => ds.iter().copied().filter(|d| doc.file.type_name(*d) == "TSWP.ShapeInfoArchive").find(|d| doc.storage_of(*d).is_none_or(|st| doc.storage_text(st).trim().is_empty())),
    };
    Ok((doc.file, id.ok_or(kn_format::Error::Invalid(format!("no {kind} in the donor")))?))
}

/// Insert -> Image: a picture (PNG / JPEG bytes) on `sheet` at (x, y), at most 400 pt on its
/// long side. Numbers' own picture object (donor) carries the new data.
pub fn insert_image(doc: &mut Document, h: &mut History, sheet: ObjId, bytes: Vec<u8>, file_name: &str, x: f32, y: f32) -> Result<ObjId, kn_format::Error> {
    let (w, hpx) = kn_format::objects::image_size(&bytes).ok_or(kn_format::Error::Invalid("only PNG and JPEG pictures can be inserted".into()))?;
    let (donor, did) = object_donor("image")?;
    let ex = donor.extract(did)?;
    let map = doc.file.map_external(&donor, &ex.external)?;
    let name = file_name.to_string();
    let id = doc.structural(h, &[sheet], |k| -> Result<ObjId, kn_format::Error> {
        let id = k.paste_extract(&ex, sheet, Some(&map))?;
        let data = k.add_image_data(bytes, &name)?;
        k.set_image_data(id, data, w as f32, hpx as f32);
        // Numbers adds the picture's outline in pixels (N11)
        if let Some(m) = k.obj_mut(id) {
            m.remove("traced_path");
            let p = m.msg_mut_or_create("traced_path");
            let (fw, fh) = (w as f32, hpx as f32);
            for (ty, pt) in [("moveTo", Some((0.0, 0.0))), ("lineTo", Some((fw, 0.0))), ("lineTo", Some((fw, fh))), ("lineTo", Some((0.0, fh))), ("closeSubpath", None), ("moveTo", Some((0.0, 0.0)))] {
                let e = p.push_new("elements");
                e.set_enum("type", ty);
                if let Some((x, y)) = pt {
                    let q = e.push_new("points");
                    q.set_f32("x", x);
                    q.set_f32("y", y);
                }
            }
        }
        Ok(id)
    })?;
    attach_drawable(doc, h, sheet, id);
    // the long side 400 pt (small pictures are scaled up too, N11)
    let k = 400.0 / (w.max(hpx) as f32).max(1.0);
    let mut g = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
    (g.x, g.y, g.w, g.h) = (x, y, w as f32 * k, hpx as f32 * k);
    doc.set_frame(h, id, g);
    Ok(id)
}

/// Arrange > Bring Forward / Bring to Front / Send Backward / Send to Back: a sheet's objects
/// are drawn in the order of its drawable_infos (back to front); its sidebar tree node keeps
/// the same order.
pub fn reorder(doc: &mut Document, h: &mut History, sheet: ObjId, ids: &[ObjId], how: kn_model::edit::ZMove) {
    use kn_model::edit::ZMove;
    let mut z: Vec<ObjId> = doc.file.obj(sheet).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default();
    let moving: Vec<ObjId> = z.iter().copied().filter(|i| ids.contains(i)).collect();
    if moving.is_empty() {
        return;
    }
    match how {
        ZMove::Front => {
            z.retain(|i| !ids.contains(i));
            z.extend(&moving);
        }
        ZMove::Back => {
            z.retain(|i| !ids.contains(i));
            z.splice(0..0, moving.iter().copied());
        }
        ZMove::Forward => {
            for i in (0..z.len().saturating_sub(1)).rev() {
                if ids.contains(&z[i]) && !ids.contains(&z[i + 1]) {
                    z.swap(i, i + 1);
                }
            }
        }
        ZMove::Backward => {
            for i in 1..z.len() {
                if ids.contains(&z[i]) && !ids.contains(&z[i - 1]) {
                    z.swap(i, i - 1);
                }
            }
        }
    }
    doc.touch(h, sheet);
    set_refs(doc, sheet, "drawable_infos", &z);
    let Some(node) = root_node(doc).and_then(|r| child_node(doc, r, sheet)) else { return };
    let kids: Vec<ObjId> = doc.file.obj(node).map(|m| m.ref_ids("children")).unwrap_or_default();
    let mut ordered: Vec<ObjId> = z.iter().filter_map(|d| kids.iter().copied().find(|k| doc.file.obj(*k).and_then(|m| m.ref_id("object")) == Some(*d))).collect();
    let rest: Vec<ObjId> = kids.iter().copied().filter(|k| !ordered.contains(k)).collect();
    ordered.extend(rest);
    if ordered != kids {
        doc.touch(h, node);
        set_refs(doc, node, "children", &ordered);
    }
}

/// Objects copied from a sheet (Edit > Copy, Duplicate Selection): tables are copied from a
/// snapshot of the document, everything else as Keydeck's object clip.
#[derive(Clone)]
pub struct Clip {
    pub objects: Option<kn_model::insert::Clip>,
    /// Tables: id in the snapshot and its position.
    pub tables: Vec<(ObjId, f32, f32)>,
    pub snapshot: std::sync::Arc<kn_format::KeyFile>,
}

pub fn copy_objects(doc: &Document, ids: &[ObjId]) -> Option<Clip> {
    let (tables, rest): (Vec<ObjId>, Vec<ObjId>) = ids.iter().copied().partition(|i| doc.file.type_name(*i) == "TST.TableInfoArchive");
    let objects = if rest.is_empty() { None } else { doc.copy_objects(&rest) };
    let tables: Vec<(ObjId, f32, f32)> = tables
        .into_iter()
        .map(|t| {
            let g = doc.file.obj(t).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
            (t, g.x, g.y)
        })
        .collect();
    if objects.is_none() && tables.is_empty() {
        return None;
    }
    Some(Clip { objects, tables, snapshot: std::sync::Arc::new(doc.file.clone()) })
}

/// Paste copied objects onto `sheet`, moved by `offset` points (Duplicate: 20). Returns the
/// new objects.
pub fn paste_objects(doc: &mut Document, h: &mut History, sheet: ObjId, clip: &Clip, offset: f32) -> Result<Vec<ObjId>, kn_format::Error> {
    let mut out = Vec::new();
    for &(t, x, y) in &clip.tables {
        out.push(insert_table(doc, h, sheet, &clip.snapshot, t, x + offset, y + offset)?);
    }
    if let Some(c) = &clip.objects {
        for id in doc.paste_objects(h, sheet, c, offset)? {
            attach_drawable(doc, h, sheet, id);
            out.push(id);
        }
    }
    Ok(out)
}

/// Hide Sheet / Unhide: the sheet stays in the document, out of the tab bar.
pub fn set_sheet_hidden(doc: &mut Document, h: &mut History, sheet: ObjId, hidden: bool) {
    doc.touch(h, sheet);
    if let Some(m) = doc.file.obj_mut(sheet) {
        if hidden {
            m.set_bool("is_hidden", true);
        } else {
            m.remove("is_hidden");
        }
    }
}

/// Insert > Shape's other shapes (Triangle, Star…): a shape's outline replaced with one of
/// Keynote's preset outlines (the same TSD.PathSourceArchive Numbers stores), sized to the
/// shape's frame. The shape keeps its Numbers style.
pub fn set_basic_shape(doc: &mut Document, h: &mut History, id: ObjId, shape: kn_model::insert::BasicShape) -> bool {
    let Some(mut g) = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry) else { return false };
    let ps = shape.path_source();
    // in the preset's own proportions (a double arrow is wider than tall)
    let natural = [("point_path_source", "naturalSize"), ("scalar_path_source", "naturalSize"), ("bezier_path_source", "naturalSize"), ("callout_path_source", "natural_size")]
        .iter()
        .find_map(|(src, f)| ps.msg(src).and_then(|s| s.msg(f)).and_then(|n| Some((n.f32("width")?, n.f32("height")?))))
        .filter(|(w, h)| *w > 1.0 && *h > 1.0);
    // (Keynote's presets are made for a 1920-point slide: kept in shape, the longer side
    // as long as Numbers' square)
    if let Some((w, hh)) = natural {
        let k = g.w.max(g.h) / w.max(hh);
        g.w = (w * k).round();
        g.h = (hh * k).round();
    }
    doc.touch(h, id);
    let Some(sh) = doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) else { return false };
    sh.remove("pathsource");
    sh.push_msg("pathsource", ps);
    // (set_frame fits the new outline to the frame)
    doc.set_frame(h, id, g);
    true
}

/// Table > Freeze Header Rows / Columns: the table model's header_rows_frozen /
/// header_columns_frozen (the headers stay in view while the table scrolls under them).
pub fn set_headers_frozen(doc: &mut Document, h: &mut History, table: ObjId, rows: bool, on: bool) -> bool {
    let Some(model) = doc.file.obj(table).and_then(|m| m.ref_id("tableModel")) else { return false };
    doc.touch(h, model);
    let Some(m) = doc.file.obj_mut(model) else { return false };
    m.set_bool(if rows { "header_rows_frozen" } else { "header_columns_frozen" }, on);
    true
}

/// Whether a table's header rows (columns) are frozen.
pub fn headers_frozen(doc: &Document, table: ObjId, rows: bool) -> bool {
    doc.file.obj(table).and_then(|m| m.ref_id("tableModel")).and_then(|m| doc.file.obj(m)).and_then(|m| m.bool(if rows { "header_rows_frozen" } else { "header_columns_frozen" })).unwrap_or(false)
}

/// Rename table `info` (its title above the table).
pub fn rename_table(doc: &mut Document, h: &mut History, info: ObjId, name: &str) -> bool {
    let Some(m) = doc.file.obj(info).and_then(|t| t.ref_id("tableModel")) else { return false };
    doc.touch(h, m);
    let Some(o) = doc.file.obj_mut(m) else { return false };
    o.set_str("table_name", name);
    true
}

/// Format > Sheet > Tab Color: the sheet's tab colour (None: the plain tab). Numbers keeps it
/// in the sheet's style, TN.SheetStyleArchive.sheet_properties.tab_color (an empty tab_color is
/// no colour), read through the style's parent (Mac N23 A).
pub fn tab_color(k: &kn_format::keyfile::KeyFile, sheet: ObjId) -> Option<kn_model::Color> {
    let mut style = k.obj(sheet)?.ref_id("style");
    for _ in 0..4 {
        let m = k.obj(style?)?;
        if let Some(tc) = m.msg("sheet_properties").and_then(|p| p.msg("tab_color")) {
            return tc.msg("color").map(|c| kn_model::Color { r: c.f32("r").unwrap_or(0.0), g: c.f32("g").unwrap_or(0.0), b: c.f32("b").unwrap_or(0.0), a: c.f32("a").unwrap_or(1.0) });
        }
        style = m.find_super("TSS.StyleArchive").and_then(|s| s.ref_id("parent"));
    }
    None
}

/// Set (or clear) the sheet's tab colour: in the sheet's style variation, made from its style
/// the first time, as Numbers does (a variation of the default sheet style with only
/// tab_color overridden; sRGB).
pub fn set_tab_color(doc: &mut Document, h: &mut History, sheet: ObjId, c: Option<kn_model::Color>) -> Result<(), kn_format::Error> {
    let err = |s: &str| kn_format::Error::Invalid(s.into());
    let style = doc.file.obj(sheet).and_then(|m| m.ref_id("style")).ok_or(err("this sheet has no style"))?;
    let is_variation = doc.file.obj(style).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.bool("is_variation")) == Some(true);
    let target = if is_variation {
        doc.touch(h, style);
        style
    } else {
        let sheet_ss = doc.file.obj(style).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.ref_id("stylesheet")).or_else(|| doc.file.stylesheet_id());
        let mut touch = vec![sheet];
        touch.extend(sheet_ss);
        let v = doc.structural(h, &touch, |k| k.new_style_variation(style))?;
        if let Some(m) = doc.file.obj_mut(sheet) {
            m.set_ref("style", v);
        }
        doc.file.refresh_refs(sheet);
        // the variation overrides the tab colour alone (Numbers' carries no fill)
        if let Some(m) = doc.file.obj_mut(v) {
            m.remove("sheet_properties");
        }
        v
    };
    let m = doc.file.obj_mut(target).ok_or(err("no style"))?;
    let tc = m.msg_mut_or_create("sheet_properties").msg_mut_or_create("tab_color");
    tc.remove("color");
    if let Some(c) = c {
        // as the swatches store it: an sRGB colour (model 1, rgbspace 1, headroom 1)
        let col = tc.msg_mut_or_create("color");
        col.set_u64("model", 1);
        col.set_f32("r", c.r);
        col.set_f32("g", c.g);
        col.set_f32("b", c.b);
        col.set_f32("a", c.a);
        col.set_u64("rgbspace", 1);
        col.set_f32("headroom", 1.0);
    }
    if let Some(m) = doc.file.obj_mut(target) {
        let n = m.msg("sheet_properties").map(|p| p.fields.len()).unwrap_or(0) as u64;
        m.set_u64("override_count", n.max(1));
    }
    Ok(())
}

/// A path element in the shape's own points (0,0 top-left .. its natural size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathEl {
    Move(f32, f32),
    Line(f32, f32),
    /// Cubic curve: two control points, then the end point.
    Curve((f32, f32), (f32, f32), (f32, f32)),
    Close,
}

/// Give shape `id` the outline `els` (a bezier path source with natural size `size`), fitted
/// to its frame (the Shape popover's capsule, parallelogram and curve; Draw with Pen).
pub fn set_path(doc: &mut Document, h: &mut History, id: ObjId, els: &[PathEl], size: (f32, f32)) -> bool {
    let Some(g) = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry) else { return false };
    let mut ps = kn_format::Msg::new("TSD.PathSourceArchive");
    ps.set_bool("horizontalFlip", false);
    ps.set_bool("verticalFlip", false);
    {
        let b = ps.msg_mut_or_create("bezier_path_source");
        let ns = b.msg_mut_or_create("naturalSize");
        ns.set_f32("width", size.0.max(1.0));
        ns.set_f32("height", size.1.max(1.0));
        let path = b.msg_mut_or_create("path");
        for el in els {
            let mut e = kn_format::Msg::new("TSP.Path.Element");
            let pts: Vec<(f32, f32)> = match *el {
                PathEl::Move(x, y) => {
                    e.set_enum("type", "moveTo");
                    vec![(x, y)]
                }
                PathEl::Line(x, y) => {
                    e.set_enum("type", "lineTo");
                    vec![(x, y)]
                }
                PathEl::Curve(a, b, c) => {
                    e.set_enum("type", "curveTo");
                    vec![a, b, c]
                }
                PathEl::Close => {
                    e.set_enum("type", "closeSubpath");
                    vec![]
                }
            };
            for (x, y) in pts {
                let mut p = kn_format::Msg::new("TSP.Point");
                p.set_f32("x", x);
                p.set_f32("y", y);
                e.push_msg("points", p);
            }
            path.push_msg("elements", e);
        }
    }
    doc.touch(h, id);
    let Some(sh) = doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) else { return false };
    sh.remove("pathsource");
    sh.push_msg("pathsource", ps);
    doc.set_frame(h, id, g);
    true
}

/// The Shape popover's outlines Keynote's presets don't have, in a 100 × 100 box (the capsule
/// 200 × 100): (name, closed, elements, natural size).
pub fn extra_shape(name: &str) -> Option<(Vec<PathEl>, (f32, f32))> {
    use PathEl::*;
    let k = 0.5523; // a quarter circle as a cubic
    Some(match name {
        // a capsule: two half circles joined by straight sides
        "Capsule" => {
            let r = 50.0;
            (
                vec![
                    Move(r, 0.0),
                    Line(200.0 - r, 0.0),
                    Curve((200.0 - r + r * k, 0.0), (200.0, r - r * k), (200.0, r)),
                    Curve((200.0, r + r * k), (200.0 - r + r * k, 100.0), (200.0 - r, 100.0)),
                    Line(r, 100.0),
                    Curve((r - r * k, 100.0), (0.0, r + r * k), (0.0, r)),
                    Curve((0.0, r - r * k), (r - r * k, 0.0), (r, 0.0)),
                    Close,
                ],
                (200.0, 100.0),
            )
        }
        "Parallelogram" => (vec![Move(30.0, 0.0), Line(130.0, 0.0), Line(100.0, 100.0), Line(0.0, 100.0), Close], (130.0, 100.0)),
        // an open quarter curve (a line, no fill)
        "Curve" => (vec![Move(0.0, 100.0), Curve((0.0, 45.0), (45.0, 0.0), (100.0, 0.0))], (100.0, 100.0)),
        _ => return None,
    })
}

/// The document's language and region (File > Advanced > Language & Region): an ICU locale such
/// as "en_US" or "fr_CA" (TSK.DocumentArchive.locale_identifier), None when it follows the
/// computer's.
pub fn document_locale(doc: &Document) -> Option<String> {
    let d = document(doc).and_then(|d| doc.file.obj(d))?;
    d.find_super("TSK.DocumentArchive").and_then(|k| k.str("locale_identifier")).map(str::to_string)
}

/// Set the document's language and region (as Numbers' Change Document Locale: the document
/// archive and the calculation engine's saved locale).
pub fn set_document_locale(doc: &mut Document, h: &mut History, locale: &str) -> bool {
    let Some(d) = document(doc) else { return false };
    doc.touch(h, d);
    if let Some(k) = doc.file.obj_mut(d).and_then(|m| m.find_super_mut("TSK.DocumentArchive")) {
        k.set_str("locale_identifier", locale);
        k.set_bool("has_user_defined_locale", true);
        k.set_bool("has_floating_locale", false);
    }
    if let Some((ce, _)) = doc.file.first_of("TSCE.CalculationEngineArchive") {
        doc.touch(h, ce);
        if let Some(m) = doc.file.obj_mut(ce) {
            m.set_str("saved_locale_identifier", locale);
        }
    }
    true
}

/// A sheet's print setup (Numbers' Print Setup sidebar), as TN.SheetArchive keeps it, with the
/// document's paper size.
#[derive(Clone, Debug, PartialEq)]
pub struct PageSetup {
    pub portrait: bool,
    /// Fit: the content scaled down to the page width.
    pub autofit: bool,
    /// Content scale when not fitted (1.0 = 100 %).
    pub scale: f32,
    /// Down then over (true), or over then down.
    pub down_first: bool,
    /// Top, left, bottom, right (points).
    pub margins: [f32; 4],
    pub page_numbers: bool,
    pub start_page: Option<i32>,
    pub backgrounds: bool,
    pub comments: bool,
    pub repeat_headers: bool,
    /// Paper size (points), the document's.
    pub paper: (f32, f32),
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup { portrait: true, autofit: true, scale: 1.0, down_first: true, margins: [36.0; 4], page_numbers: true, start_page: None, backgrounds: true, comments: false, repeat_headers: true, paper: (612.0, 792.0) }
    }
}

pub fn page_setup(doc: &Document, sheet: ObjId) -> PageSetup {
    let mut p = PageSetup::default();
    if let Some(s) = doc.file.obj(sheet) {
        p.portrait = s.bool("in_portrait_page_orientation").unwrap_or(true);
        p.autofit = s.bool("is_autofit_on").unwrap_or(true);
        p.scale = s.f32("content_scale").filter(|x| *x > 0.0).unwrap_or(1.0);
        p.down_first = s.enum_name("page_order").is_none_or(|o| !o.contains("Right") && !o.contains("Across"));
        if let Some(m) = s.msg("print_margins") {
            p.margins = [m.f32("top").unwrap_or(36.0), m.f32("left").unwrap_or(36.0), m.f32("bottom").unwrap_or(36.0), m.f32("right").unwrap_or(36.0)];
        }
        p.page_numbers = s.bool("show_page_numbers").unwrap_or(true);
        p.start_page = s.bool("using_start_page_number").unwrap_or(false).then(|| s.u32("start_page_number").unwrap_or(1) as i32);
        p.backgrounds = s.bool("print_backgrounds").unwrap_or(true);
        p.comments = s.bool("should_print_comments").unwrap_or(false);
        p.repeat_headers = s.bool("show_repeating_headers").unwrap_or(true);
    }
    if let Some(sz) = document(doc).and_then(|d| doc.file.obj(d)).and_then(|d| d.msg("page_size")) {
        if let (Some(w), Some(h)) = (sz.f32("width"), sz.f32("height")) {
            if w > 10.0 && h > 10.0 {
                p.paper = (w, h);
            }
        }
    }
    p
}

pub fn set_page_setup(doc: &mut Document, h: &mut History, sheet: ObjId, p: &PageSetup) {
    doc.touch(h, sheet);
    if let Some(s) = doc.file.obj_mut(sheet) {
        s.set_bool("in_portrait_page_orientation", p.portrait);
        s.set_bool("is_autofit_on", p.autofit);
        s.set_f32("content_scale", p.scale);
        let m = s.msg_mut_or_create("print_margins");
        for (f, v) in ["top", "left", "bottom", "right"].iter().zip(p.margins) {
            m.set_f32(f, v);
        }
        s.set_bool("show_page_numbers", p.page_numbers);
        s.set_bool("using_start_page_number", p.start_page.is_some());
        if let Some(n) = p.start_page {
            s.set_u64("start_page_number", n.max(1) as u64);
        }
        s.set_bool("print_backgrounds", p.backgrounds);
        s.set_bool("should_print_comments", p.comments);
        s.set_bool("show_repeating_headers", p.repeat_headers);
    }
    if let Some(d) = document(doc) {
        doc.touch(h, d);
        if let Some(m) = doc.file.obj_mut(d) {
            let sz = m.msg_mut_or_create("page_size");
            sz.set_f32("width", p.paper.0);
            sz.set_f32("height", p.paper.1);
        }
    }
}
