//! Objects in a Pages document: inserting (copies of the objects Pages itself made in the
//! research samples, their styles mapped onto this document's), deleting, the z-order, Object
//! Placement (Stay on Page = TP.FloatingDrawablesArchive / Move with Text = an attachment in
//! the body) and text wrap; pages of page-layout documents.

use std::collections::HashMap;

use kn_format::{Extract, KeyFile, Msg, ObjId};
use kn_model::edit::History;
use kn_model::Document;

use crate::Pages;

pub const DONOR_SHAPE: &[u8] = include_bytes!("../data/donors/05-shape.pages");
pub const DONOR_TEXT_BOX: &[u8] = include_bytes!("../data/donors/06-text-box.pages");
pub const DONOR_IMAGE: &[u8] = include_bytes!("../data/donors/07-image.pages");
pub const DONOR_TABLE: &[u8] = include_bytes!("../data/donors/08-table.pages");
pub const DONOR_LINE: &[u8] = include_bytes!("../data/donors/11-line.pages");
pub const DONOR_CHART: &[u8] = include_bytes!("../data/donors/37-chart.pages");

/// What Insert can make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NewObject {
    TextBox,
    Shape(kn_model::insert::BasicShape),
    Line,
    Table,
    Chart,
}

/// Where a new object goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// Stay on Page: page index and top-left on the page.
    Floating { page: usize, x: f32, y: f32 },
    /// Inline with the text at stored byte offset `at` of the body.
    Inline { at: usize },
}

fn donor(bytes: &'static [u8], ty: &str) -> Result<(KeyFile, ObjId), kn_format::Error> {
    crate::init();
    // (read once: an import pastes one per table, and reading is slower than a copy)
    thread_local!(static READ: std::cell::RefCell<HashMap<usize, KeyFile>> = Default::default());
    let k = match READ.with(|r| r.borrow().get(&(bytes.as_ptr() as usize)).cloned()) {
        Some(k) => k,
        None => {
            let k = KeyFile::from_zip(bytes)?;
            READ.with(|r| r.borrow_mut().insert(bytes.as_ptr() as usize, k.clone()));
            k
        }
    };
    let id = k.first_of(ty).map(|(id, _)| id).ok_or_else(|| kn_format::Error::Invalid(format!("donor has no {ty}")))?;
    Ok((k, id))
}

/// Paste an extract into the document file `file` (no slide; Pages places it). Returns the new
/// root. Data references are dropped (the caller sets new data).
pub fn paste_extract(k: &mut KeyFile, ex: &Extract, file: &str, external: &HashMap<ObjId, ObjId>, parent: Option<ObjId>) -> Result<ObjId, kn_format::Error> {
    let new_ids = k.alloc_ids(ex.archives.len() as u64);
    let idmap: HashMap<ObjId, ObjId> = ex.archives.iter().map(|a| a.id()).zip(new_ids.iter().copied()).collect();
    let mut full = idmap.clone();
    full.extend(external.iter().map(|(a, b)| (*a, *b)));
    if let (Some(sp), Some(p)) = (ex.src_parent, parent) {
        full.insert(sp, p);
    }
    let mut out = Vec::new();
    for a in &ex.archives {
        let mut a = a.clone();
        let old = a.id();
        a.info.set_u64("identifier", idmap[&old]);
        for i in 0..a.payloads.len() {
            let refs: Vec<ObjId> = a.object_references(i).into_iter().map(|r| *full.get(&r).unwrap_or(&r)).collect();
            if !refs.is_empty() {
                a.set_object_references(i, &refs);
            }
            a.payloads[i].for_each_ref_mut(&mut |r| {
                if let Some(n) = full.get(r) {
                    *r = *n;
                }
            });
            a.set_data_references(i, &[]);
        }
        out.push(a);
    }
    let root = idmap[&ex.root];
    if let Some(iwa) = k.iwa_mut(file) {
        iwa.archives.extend(out);
    }
    k.reindex();
    if let Some(d) = k.obj_mut(root).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
        match parent {
            Some(p) => d.set_ref("parent", p),
            None => d.remove("parent"),
        }
    }
    for old in &ex.uuids {
        k.add_uuid_entry(file, idmap[old]);
    }
    if let Some(a) = k.archive_mut(root) {
        a.refresh_object_references();
    }
    k.ensure_external_refs(file);
    Ok(root)
}

impl Pages {
    fn floating_id(&self) -> Option<ObjId> {
        self.root()?.1.ref_id("floating_drawables")
    }
    fn zorder_id(&self) -> Option<ObjId> {
        self.root()?.1.ref_id("drawables_zorder")
    }

    /// Put drawable `id` on page `page` (Stay on Page), on top of the z-order.
    pub fn attach_floating(&mut self, h: &mut History, id: ObjId, page: usize) {
        if let Some(f) = self.floating_id() {
            self.doc.touch(h, f);
            if let Some(a) = self.doc.file.archive_mut(f) {
                let m = a.obj_mut();
                let has = m.msgs("page_groups").any(|g| g.u64("page_index").unwrap_or(0) == page as u64);
                if !has {
                    let g = m.push_new("page_groups");
                    g.set_u64("page_index", page as u64);
                }
                if let Some(g) = m.msgs_mut("page_groups").find(|g| g.u64("page_index").unwrap_or(0) == page as u64) {
                    g.push_new("drawables").set_ref("drawable", id);
                }
                a.refresh_object_references();
            }
        }
        self.add_to_zorder(h, id);
    }

    fn add_to_zorder(&mut self, h: &mut History, id: ObjId) {
        if let Some(z) = self.zorder_id() {
            self.doc.touch(h, z);
            if let Some(a) = self.doc.file.archive_mut(z) {
                if !a.obj().ref_ids("drawables").contains(&id) {
                    a.obj_mut().push_ref("drawables", id);
                }
                a.refresh_object_references();
            }
        }
    }

    /// Take drawables off their pages (floating) and out of the z-order.
    pub fn detach_floating(&mut self, h: &mut History, ids: &[ObjId]) {
        if let Some(f) = self.floating_id() {
            self.doc.touch(h, f);
            if let Some(a) = self.doc.file.archive_mut(f) {
                for g in a.obj_mut().msgs_mut("page_groups") {
                    g.retain_msgs("drawables", |d| !d.ref_id("drawable").is_some_and(|x| ids.contains(&x)));
                }
                a.obj_mut().retain_msgs("page_groups", |g| g.count("drawables") > 0);
                a.refresh_object_references();
            }
        }
        if let Some(z) = self.zorder_id() {
            self.doc.touch(h, z);
            if let Some(a) = self.doc.file.archive_mut(z) {
                a.obj_mut().retain_msgs("drawables", |d| !d.u64("identifier").is_some_and(|x| ids.contains(&x)));
                a.refresh_object_references();
            }
        }
    }

    /// The page a floating drawable is on.
    pub fn floating_page(&self, id: ObjId) -> Option<usize> {
        self.floating().into_iter().find(|(_, d)| *d == id).map(|(p, _)| p)
    }

    /// The attachment of an inline drawable and its stored byte offset in the body.
    pub fn inline_attachment(&self, id: ObjId) -> Option<(ObjId, usize)> {
        let body = self.body_storage()?;
        let st = self.file().obj(body)?;
        let t = crate::edit::object_table(st, "table_attachment");
        let text = self.doc.storage_text(body);
        for (ci, att) in t {
            let Some(aid) = att else { continue };
            let Some(a) = self.file().obj(aid) else { continue };
            let d = a.ref_id("drawable").or_else(|| a.msg("super").and_then(|s| s.ref_id("drawable")));
            if d == Some(id) {
                let mut u = 0;
                for (b, ch) in text.char_indices() {
                    if u == ci {
                        return Some((aid, b));
                    }
                    u += ch.len_utf16();
                }
            }
        }
        None
    }

    /// Insert a new object; returns its id.
    pub fn insert(&mut self, h: &mut History, what: NewObject, place: Placement) -> Result<ObjId, kn_format::Error> {
        let (dk, did) = match what {
            NewObject::TextBox => donor(DONOR_TEXT_BOX, "TSWP.ShapeInfoArchive")?,
            NewObject::Shape(_) => donor(DONOR_SHAPE, "TSWP.ShapeInfoArchive")?,
            NewObject::Line => donor(DONOR_LINE, "TSWP.ShapeInfoArchive")?,
            NewObject::Chart => donor(DONOR_CHART, "TSCH.ChartDrawableArchive")?,
            NewObject::Table => return self.insert_table(h, place),
        };
        let id = self.paste_donor(h, &dk, did, place)?;
        if what == NewObject::Chart {
            self.use_document_preset(h, id);
        }
        if let NewObject::Shape(s) = what {
            set_basic_shape(&mut self.doc, h, id, s);
        }
        if what == NewObject::TextBox {
            // a new text box is empty (the donor's says "Text box")
            if let Some(st) = self.doc.storage_of(id) {
                let t = self.doc.storage_text(st);
                if !t.is_empty() {
                    crate::edit::replace(&mut self.doc, h, st, 0, t.len(), "");
                }
            }
        }
        Ok(id)
    }

    /// Copy drawable `did` of `dk` into this document at `place`.
    pub fn paste_donor(&mut self, h: &mut History, dk: &KeyFile, did: ObjId, place: Placement) -> Result<ObjId, kn_format::Error> {
        let ex = dk.extract(did)?;
        let skip = |e: ObjId| Some(e) == ex.src_parent || dk.type_name(e) == "TSWP.StorageArchive" && dk.obj(e).and_then(|m| m.enum_name("kind")) == Some("BODY");
        const OC_FILE: &str = "Index/ObjectContainer.iwa";
        // charts' non-style archives live in the object container: copied, not mapped
        let (oc_ids, ext): (Vec<ObjId>, Vec<ObjId>) = ex.external.iter().copied().filter(|e| !skip(*e)).partition(|i| dk.file_of(*i) == Some(OC_FILE));
        let mut map = self.doc.file.map_external(dk, &ext)?;
        let file = self.root_id().and_then(|r| self.file().file_of(r)).unwrap_or("Index/Document.iwa").to_string();
        let parent = match place {
            Placement::Inline { .. } => self.body_storage(),
            Placement::Floating { .. } => None,
        };
        let have_oc = self.file().first_of("TSP.ObjectContainer").map(|(i, _)| i);
        let touch: Vec<ObjId> = have_oc.into_iter().collect();
        let id = self.doc.structural(h, &touch, |k| {
            if !oc_ids.is_empty() {
                let (oc_root, _) = dk.first_of("TSP.ObjectContainer").ok_or_else(|| kn_format::Error::Invalid("donor has no object container".into()))?;
                let oc = match have_oc {
                    Some(oc) => oc,
                    None => {
                        let new_root = k.alloc_ids(1)[0];
                        let mut a = dk.archive(oc_root).unwrap().clone();
                        a.info.set_u64("identifier", new_root);
                        a.obj_mut().remove("objects");
                        a.set_object_references(0, &[]);
                        k.add_entry(kn_format::Entry { name: OC_FILE.into(), data: kn_format::EntryData::Iwa(kn_format::IwaFile { archives: vec![a] }) });
                        let mut comp = dk.component(oc_root).unwrap().clone();
                        comp.set_u64("identifier", new_root);
                        comp.remove("object_uuid_map_entries");
                        comp.remove("external_references");
                        comp.remove("data_references");
                        k.package_metadata_mut().unwrap().push_msg("components", comp);
                        k.reindex();
                        new_root
                    }
                };
                let ocfile = k.file_of(oc).unwrap_or(OC_FILE).to_string();
                let new_ids = k.alloc_ids(oc_ids.len() as u64);
                let oc_map: HashMap<ObjId, ObjId> = oc_ids.iter().copied().zip(new_ids.iter().copied()).collect();
                for (old, new) in &oc_map {
                    let mut a = dk.archive(*old).unwrap().clone();
                    a.info.set_u64("identifier", *new);
                    for p in &mut a.payloads {
                        p.for_each_ref_mut(&mut |r| {
                            if let Some(n) = oc_map.get(r) {
                                *r = *n;
                            }
                        });
                    }
                    k.insert_archive(&ocfile, a);
                    if let Some(o) = k.obj_mut(oc) {
                        o.push_ref("objects", *new);
                    }
                    if let Some(a) = k.archive_mut(oc) {
                        a.refresh_object_references();
                    }
                    k.add_uuid_entry(&ocfile, *new);
                }
                map.extend(oc_map);
            }
            paste_extract(k, &ex, &file, &map, parent)
        })?;
        self.place(h, id, place);
        Ok(id)
    }

    /// Put a (new or detached) drawable at `place`.
    pub fn place(&mut self, h: &mut History, id: ObjId, place: Placement) {
        match place {
            Placement::Floating { page, x, y } => {
                self.attach_floating(h, id, page);
                let mut g = self.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
                g.x = x;
                g.y = y;
                self.doc.set_frame(h, id, g);
                let wrap = if self.is_word_processing() { 4 } else { 5 };
                self.set_wrap(h, id, wrap);
                self.doc.touch(h, id);
                if let Some(d) = self.doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
                    d.remove("parent");
                }
            }
            Placement::Inline { at } => {
                let Some(body) = self.body_storage() else { return };
                let file = self.file().file_of(body).unwrap_or("Index/Document.iwa").to_string();
                let att = self.doc.structural(h, &[], |k| {
                    let aid = k.alloc_ids(1)[0];
                    let mut m = Msg::new("TSWP.DrawableAttachmentArchive");
                    m.set_ref("drawable", id);
                    m.set_u64("h_offset_type", 0);
                    m.set_f32("h_offset", 0.0);
                    m.set_u64("v_offset_type", 0);
                    m.set_f32("v_offset", 0.0);
                    let mut a = kn_format::Archive::new_object(aid, m);
                    a.refresh_object_references();
                    k.insert_archive(&file, a);
                    aid
                });
                crate::edit::insert_attachment(&mut self.doc, h, body, at, att);
                self.doc.touch(h, id);
                if let Some(d) = self.doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
                    d.set_ref("parent", body);
                    d.msg_mut_or_create("exterior_text_wrap").set_u64("type", 0);
                }
                if let Some(a) = self.doc.file.archive_mut(id) {
                    a.refresh_object_references();
                }
                self.add_to_zorder(h, id);
            }
        }
    }

    /// Insert > Table (Pages' Basic table from the research sample).
    pub fn insert_table(&mut self, h: &mut History, place: Placement) -> Result<ObjId, kn_format::Error> {
        let (dk, did) = donor(DONOR_TABLE, "TST.TableInfoArchive")?;
        let target = self.root_id().ok_or_else(|| kn_format::Error::Invalid("no document".into()))?;
        let mut touch = self.doc.file.table_insert_touches();
        touch.push(target);
        let id = self.doc.structural(h, &touch, |k| k.paste_table(&dk, did, target))?;
        let names: Vec<String> = self.file().objects_of("TST.TableModelArchive").filter_map(|(_, m)| m.str("table_name").map(str::to_string)).collect();
        if let Some(model) = self.file().obj(id).and_then(|m| m.ref_id("tableModel")) {
            let mine = self.file().obj(model).and_then(|m| m.str("table_name")).unwrap_or("").to_string();
            if names.iter().filter(|n| **n == mine).count() > 1 {
                let name = (1..).map(|n| format!("Table {n}")).find(|n| !names.contains(n)).unwrap();
                self.doc.touch(h, model);
                if let Some(m) = self.doc.file.obj_mut(model) {
                    m.set_str("table_name", &name);
                }
            }
        }
        // a new table is empty (the donor has an "x" in its first cell)
        if let Some(t) = self.file().obj(id).and_then(|m| kn_model::table::table(self.file(), m)) {
            for r in 0..t.rows {
                for c in 0..t.cols {
                    if t.cells.get((r * t.cols + c) as usize).is_some_and(|v| !v.is_empty()) {
                        let _ = self.doc.set_cell(h, id, r, c, &kn_model::tablecells::CellInput::Empty);
                    }
                }
            }
        }
        self.place(h, id, place);
        Ok(id)
    }

    /// Insert > Choose…: a picture (PNG / JPEG), at most `max_w` (and 400 pt) wide.
    pub fn insert_image(&mut self, h: &mut History, bytes: Vec<u8>, name: &str, place: Placement, max_w: f32) -> Result<ObjId, kn_format::Error> {
        let (w, hp) = kn_format::objects::image_size(&bytes).ok_or_else(|| kn_format::Error::Invalid("only PNG and JPEG pictures can be inserted".into()))?;
        let (dk, did) = donor(DONOR_IMAGE, "TSD.ImageArchive")?;
        let id = self.paste_donor(h, &dk, did, place)?;
        let name = name.to_string();
        let file = self.file().file_of(id).unwrap_or("Index/Document.iwa").to_string();
        self.doc.structural(h, &[id], |k| -> Result<(), kn_format::Error> {
            let data = k.add_image_data(bytes, &name)?;
            k.set_image_data(id, data, w as f32, hp as f32);
            k.add_data_reference(&file, data, id);
            if let Some(m) = k.obj_mut(id) {
                m.remove("thumbnailData");
            }
            Ok(())
        })?;
        let s = (max_w.min(400.0) / w as f32).min(1.0);
        let mut g = self.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
        g.w = w as f32 * s;
        g.h = hp as f32 * s;
        self.doc.set_frame(h, id, g);
        Ok(id)
    }

    /// Delete drawables (floating ones off their pages, inline ones out of the text).
    pub fn delete_objects(&mut self, h: &mut History, ids: &[ObjId]) {
        let mut inline: Vec<(usize, ObjId)> = ids.iter().filter_map(|id| self.inline_attachment(*id).map(|(_, b)| (b, *id))).collect();
        inline.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some(body) = self.body_storage() {
            for (b, _) in &inline {
                crate::edit::replace(&mut self.doc, h, body, *b, b + '\u{FFFC}'.len_utf8(), "");
            }
        }
        self.detach_floating(h, ids);
        self.drop_from_section_layouts(h, ids);
        let tables: Vec<ObjId> = ids.iter().copied().filter(|i| self.file().type_name(*i) == "TST.TableInfoArchive").collect();
        if !tables.is_empty() {
            self.doc.touch_all(h);
            for t in tables {
                self.doc.file.remove_table_engine(t);
            }
        }
        // the objects themselves (and their attachments, captions, text, pictures) go
        self.purge(h, ids);
    }

    /// Object Placement: Stay on Page (floating) or Move with Text (inline).
    pub fn set_placement(&mut self, h: &mut History, id: ObjId, place: Placement) {
        if let Some((_, b)) = self.inline_attachment(id) {
            if let Some(body) = self.body_storage() {
                crate::edit::replace(&mut self.doc, h, body, b, b + '\u{FFFC}'.len_utf8(), "");
            }
        }
        self.detach_floating(h, &[id]);
        self.place(h, id, place);
    }

    /// Text Wrap: 0 inline, 2 none, 3 above and below, 4 automatic, 5 around.
    pub fn set_wrap(&mut self, h: &mut History, id: ObjId, ty: u64) {
        self.doc.touch(h, id);
        if let Some(d) = self.doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            let w = d.msg_mut_or_create("exterior_text_wrap");
            w.set_u64("type", ty);
            if w.f32("margin").is_none() {
                w.set_f32("margin", 12.0);
            }
            if w.f32("alpha_threshold").is_none() {
                w.set_f32("alpha_threshold", 0.5);
            }
        }
    }

    pub fn wrap_of(&self, id: ObjId) -> (u64, f32) {
        let w = self.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|d| d.msg("exterior_text_wrap"));
        (w.and_then(|w| w.u64("type")).unwrap_or(4), w.and_then(|w| w.f32("margin")).unwrap_or(12.0))
    }

    pub fn set_wrap_margin(&mut self, h: &mut History, id: ObjId, margin: f32) {
        self.doc.touch(h, id);
        if let Some(d) = self.doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            d.msg_mut_or_create("exterior_text_wrap").set_f32("margin", margin.clamp(0.0, 200.0));
        }
    }

    /// Arrange: move drawables in the z-order.
    pub fn reorder(&mut self, h: &mut History, ids: &[ObjId], how: kn_model::edit::ZMove) {
        use kn_model::edit::ZMove;
        let Some(zid) = self.zorder_id() else { return };
        let mut z = self.zorder();
        let moving: Vec<ObjId> = z.iter().copied().filter(|i| ids.contains(i)).collect();
        if moving.is_empty() {
            return;
        }
        let first = z.iter().position(|i| ids.contains(i)).unwrap_or(0);
        let last = z.iter().rposition(|i| ids.contains(i)).unwrap_or(0);
        z.retain(|i| !ids.contains(i));
        // the body storage stays at the bottom
        let floor = usize::from(z.first().is_some_and(|f| self.file().type_name(*f) == "TSWP.StorageArchive"));
        let at = match how {
            ZMove::Front => z.len(),
            ZMove::Back => floor,
            ZMove::Forward => (last + 2 - moving.len()).min(z.len()),
            ZMove::Backward => first.saturating_sub(1).max(floor),
        };
        for (k, id) in moving.into_iter().enumerate() {
            z.insert((at + k).min(z.len()), id);
        }
        self.doc.touch(h, zid);
        if let Some(a) = self.doc.file.archive_mut(zid) {
            let m = a.obj_mut();
            m.remove("drawables");
            for id in &z {
                m.push_ref("drawables", *id);
            }
            a.refresh_object_references();
        }
        if let Some(f) = self.floating_id() {
            self.doc.touch(h, f);
            if let Some(a) = self.doc.file.archive_mut(f) {
                for g in a.obj_mut().msgs_mut("page_groups") {
                    let mut ds: Vec<Msg> = g.msgs("drawables").cloned().collect();
                    ds.sort_by_key(|d| d.ref_id("drawable").and_then(|x| z.iter().position(|o| *o == x)).unwrap_or(usize::MAX));
                    g.remove("drawables");
                    for d in ds {
                        g.push_msg("drawables", d);
                    }
                }
                a.refresh_object_references();
            }
        }
    }

    /// Page layout: delete page `page` (its objects go; later pages move up).
    pub fn delete_layout_page(&mut self, h: &mut History, page: usize) {
        let on_page: Vec<ObjId> = self.floating().into_iter().filter(|(p, _)| *p == page).map(|(_, id)| id).collect();
        self.delete_objects(h, &on_page);
        if let Some(f) = self.floating_id() {
            self.doc.touch(h, f);
            if let Some(a) = self.doc.file.archive_mut(f) {
                for g in a.obj_mut().msgs_mut("page_groups") {
                    let p = g.u64("page_index").unwrap_or(0) as usize;
                    if p > page {
                        g.set_u64("page_index", (p - 1) as u64);
                    }
                }
            }
        }
        let n = self.saved_page_count().saturating_sub(1).max(1);
        self.set_saved_page_count(h, n);
    }

    /// Page layout: a new blank page after `after`; objects of later pages move down.
    pub fn add_layout_page(&mut self, h: &mut History, after: usize) {
        if let Some(f) = self.floating_id() {
            self.doc.touch(h, f);
            if let Some(a) = self.doc.file.archive_mut(f) {
                for g in a.obj_mut().msgs_mut("page_groups") {
                    let p = g.u64("page_index").unwrap_or(0) as usize;
                    if p > after {
                        g.set_u64("page_index", (p + 1) as u64);
                    }
                }
            }
        }
        let n = self.saved_page_count().max(self.floating().iter().map(|(p, _)| p + 1).max().unwrap_or(0));
        self.set_saved_page_count(h, n + 1);
    }

    /// The page count kept in the view state (page-layout documents).
    pub fn set_saved_page_count(&mut self, h: &mut History, n: usize) {
        let Some((id, _)) = self.file().first_of("TP.LayoutStateArchive") else { return };
        self.doc.touch(h, id);
        if let Some(m) = self.doc.file.obj_mut(id) {
            m.set_u64("last_page_count", n as u64);
            if let Some(sec) = m.msgs_mut("section_hints").next() {
                let first = sec.msgs("page_hints").next().cloned();
                if let Some(first) = first {
                    sec.remove("page_hints");
                    for _ in 0..n {
                        sec.push_msg("page_hints", first.clone());
                    }
                }
            }
        }
    }
}

impl Pages {
    /// Arrange > Group: floating drawables of one page become one group in their place.
    pub fn group(&mut self, h: &mut History, ids: &[ObjId]) -> Result<ObjId, kn_format::Error> {
        let page = ids.iter().find_map(|i| self.floating_page(*i)).ok_or_else(|| kn_format::Error::Invalid("only objects that stay on the page can be grouped".into()))?;
        let members: Vec<ObjId> = ids.iter().copied().filter(|i| self.floating_page(*i) == Some(page)).collect();
        let z = self.zorder_id().ok_or_else(|| kn_format::Error::Invalid("no z-order".into()))?;
        let g = self.doc.group(h, z, &members)?;
        // floating drawables have no parent; the page group lists the group in their place
        self.doc.touch(h, g);
        if let Some(d) = self.doc.file.obj_mut(g).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            d.remove("parent");
        }
        self.detach_floating(h, &members);
        self.attach_floating(h, g, page);
        self.doc.file.refresh_refs(g);
        Ok(g)
    }

    /// Arrange > Ungroup.
    pub fn ungroup(&mut self, h: &mut History, group: ObjId) -> Result<Vec<ObjId>, kn_format::Error> {
        let page = self.floating_page(group).unwrap_or(0);
        let z = self.zorder_id().ok_or_else(|| kn_format::Error::Invalid("no z-order".into()))?;
        let kids = self.doc.ungroup(h, z, group)?;
        self.detach_floating(h, &[group]);
        for k in &kids {
            self.doc.touch(h, *k);
            if let Some(d) = self.doc.file.obj_mut(*k).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
                d.remove("parent");
            }
            self.attach_floating(h, *k, page);
        }
        Ok(kids)
    }
}

/// A shape's outline replaced with one of the suite's preset outlines (as Values does).
pub fn set_basic_shape(doc: &mut Document, h: &mut History, id: ObjId, shape: kn_model::insert::BasicShape) -> bool {
    let Some(mut g) = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry) else { return false };
    let ps = shape.path_source();
    let natural = [("point_path_source", "naturalSize"), ("scalar_path_source", "naturalSize"), ("bezier_path_source", "naturalSize"), ("callout_path_source", "natural_size")]
        .iter()
        .find_map(|(src, f)| ps.msg(src).and_then(|s| s.msg(f)).and_then(|n| Some((n.f32("width")?, n.f32("height")?))))
        .filter(|(w, h)| *w > 1.0 && *h > 1.0);
    if let Some((w, hh)) = natural {
        let k = g.w.max(g.h) / w.max(hh);
        g.w = (w * k).round();
        g.h = (hh * k).round();
    }
    doc.touch(h, id);
    let Some(sh) = doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) else { return false };
    sh.remove("pathsource");
    sh.push_msg("pathsource", ps);
    doc.set_frame(h, id, g);
    true
}

impl Pages {
    /// The page template (TP.SectionTemplateArchive) page `pi` of layout `l` uses.
    pub fn page_template_of(&self, l: &crate::flow::Layout, pi: usize) -> Option<ObjId> {
        let page = l.pages.get(pi)?;
        let in_section = l.pages[..pi].iter().rev().take_while(|p| p.section == page.section).count();
        self.sections().get(page.section)?.master(in_section, page.number)
    }

    /// Arrange > Section Layouts > Move Objects to Section Layout: floating objects become the
    /// page template's layout objects (shown behind the text on every page using it).
    pub fn move_to_section_layout(&mut self, h: &mut History, ids: &[ObjId], template: ObjId) {
        self.detach_floating(h, ids);
        self.doc.touch(h, template);
        if let Some(a) = self.doc.file.archive_mut(template) {
            for id in ids {
                a.obj_mut().push_new("section_template_drawables").set_u64("identifier", *id);
            }
            a.refresh_object_references();
        }
    }

    /// Layout objects back onto page `page` as ordinary floating objects.
    pub fn move_from_section_layout(&mut self, h: &mut History, ids: &[ObjId], page: usize) {
        self.drop_from_section_layouts(h, ids);
        for id in ids {
            self.attach_floating(h, *id, page);
        }
    }

    /// The objects out of every page template's layout objects.
    fn drop_from_section_layouts(&mut self, h: &mut History, ids: &[ObjId]) {
        let templates: Vec<ObjId> = self.doc.file.archives().filter(|(_, a)| a.obj().type_name() == "TP.SectionTemplateArchive").map(|(_, a)| a.id()).collect();
        for t in templates {
            if !self.file().obj(t).is_some_and(|m| m.ref_ids("section_template_drawables").iter().any(|d| ids.contains(d))) {
                continue;
            }
            self.doc.touch(h, t);
            if let Some(a) = self.doc.file.archive_mut(t) {
                a.obj_mut().retain_msgs("section_template_drawables", |r| !r.u64("identifier").is_some_and(|i| ids.contains(&i)));
                a.refresh_object_references();
            }
        }
    }
}

impl Pages {
    /// Insert a movie or sound (Media > Choose Photo or Video / Choose Music / Record Audio):
    /// kn-model's TSD.MovieArchive, then placed on the page (floating) or in the text, at most
    /// `max_w` wide.
    pub fn insert_movie(&mut self, h: &mut History, m: kn_model::insert::NewMovie, place: Placement, max_w: f32) -> Result<ObjId, kn_format::Error> {
        let root = self.root_id().ok_or_else(|| kn_format::Error::Invalid("no document".into()))?;
        let id = self.doc.insert_movie(h, root, m)?;
        self.doc.touch(h, id);
        if let Some(d) = self.doc.file.obj_mut(id).and_then(|o| o.find_super_mut("TSD.DrawableArchive")) {
            d.remove("parent");
        }
        let mut g = self.file().obj(id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
        let k = (max_w / g.w.max(1.0)).min(1.0);
        g.w *= k;
        g.h *= k;
        match place {
            Placement::Floating { page, x, y } => {
                g.x = x;
                g.y = y;
                self.doc.set_frame(h, id, g);
                self.attach_floating(h, id, page);
            }
            Placement::Inline { at } => {
                self.doc.set_frame(h, id, g);
                self.attach_floating(h, id, 0);
                self.set_placement(h, id, Placement::Inline { at });
            }
        }
        Ok(id)
    }
}

impl Pages {
    /// Insert > Line > Straight / Curved / Right Angle Connection Line: kn-model's connection line,
    /// joining `ends` (two floating objects on one page) when given, else free on `page` with its
    /// top left at `at`.
    pub fn insert_connection_line(&mut self, h: &mut History, kind: kn_model::insert::Connection, ends: Option<(ObjId, ObjId)>, page: usize, at: (f32, f32)) -> Result<ObjId, kn_format::Error> {
        let page = match ends {
            Some((a, b)) => match (self.floating_page(a), self.floating_page(b)) {
                (Some(pa), Some(pb)) if pa == pb => pa,
                _ => return Err(kn_format::Error::Invalid("connection lines join two objects set to Stay on Page, on the same page".into())),
            },
            None => page,
        };
        let id = self.insert(h, NewObject::Line, Placement::Floating { page, x: at.0, y: at.1 })?;
        self.doc.make_connection_line(h, id, kind, ends)?;
        if ends.is_none() {
            let mut g = self.file().obj(id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
            (g.x, g.y) = at;
            self.doc.set_frame(h, id, g);
        }
        Ok(id)
    }
}

impl Pages {
    /// After objects moved or resized: the connection lines joining them follow.
    pub fn update_connections(&mut self, h: &mut History, moved: &[ObjId]) {
        let lines: Vec<ObjId> = self.file().objects_of("TSD.ConnectionLineArchive").map(|(i, _)| i).collect();
        if !lines.is_empty() {
            self.doc.update_connections_in(h, &lines, moved);
        }
    }
}

impl Pages {
    /// A chart pasted from the donor brings a copy of the donor's chart style preset; Pages'
    /// charts use the document theme's (Chart Styles would show a 7th, duplicate swatch). Point
    /// the chart at the theme's preset with the same colours (else the first) and drop the copy.
    fn use_document_preset(&mut self, h: &mut History, chart: ObjId) {
        let k = self.file();
        let theirs: Vec<ObjId> = {
            let mut v: Vec<ObjId> = k
                .archives()
                .filter(|(_, a)| a.obj().type_name() == "TP.ThemeArchive")
                .flat_map(|(_, a)| (0..a.payloads.len()).flat_map(move |i| a.object_references(i)))
                .filter(|r| k.type_name(*r) == "TSCH.ChartStylePreset")
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let Some(copy) = k.obj(chart).and_then(|c| c.msg("unity")).and_then(|u| u.ref_id("preset")) else { return };
        if theirs.is_empty() || theirs.contains(&copy) {
            return;
        }
        let colours = self.doc.chart_preset_colors(copy);
        let target = theirs.iter().copied().find(|p| self.doc.chart_preset_colors(*p) == colours).unwrap_or(theirs[0]);
        self.doc.touch(h, chart);
        if let Some(u) = self.doc.file.obj_mut(chart).and_then(|c| c.msg_mut("unity")) {
            u.set_ref("preset", target);
        }
        self.doc.file.refresh_refs(chart);
        self.purge(h, &[copy]);
    }
}
