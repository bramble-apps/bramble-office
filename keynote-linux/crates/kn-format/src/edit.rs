//! Structural edits: cloning and deleting slides. Ported from the research writer
//! (keywriter.py), whose output was verified by opening it in real Keynote.

use std::collections::HashMap;

use crate::keyfile::{Entry, EntryData, KeyFile};
use crate::msg::{reference, Msg, ObjId};
use crate::Error;

use crate::objects::new_uuid;

impl KeyFile {
    /// (slide node id, slide id) in presentation order (top level of the slide tree).
    pub fn slide_nodes(&self) -> Vec<(ObjId, ObjId)> {
        let Some((_, show)) = self.first_of("KN.ShowArchive") else { return Vec::new() };
        let Some(tree) = show.msg("slideTree") else { return Vec::new() };
        tree.ref_ids("slides")
            .into_iter()
            .filter_map(|n| Some((n, self.obj(n)?.ref_id("slide")?)))
            .collect()
    }

    fn show_id(&self) -> Result<ObjId, Error> {
        self.first_of("KN.ShowArchive").map(|(id, _)| id).ok_or(Error::Invalid("no KN.ShowArchive".into()))
    }

    /// Root id of the Document component (holds the show and slide nodes).
    fn document_component(&self) -> Option<ObjId> {
        self.component_for_file("Index/Document.iwa")
    }

    /// Duplicate the slide at top-level position `index`, inserting the copy at `insert_at`.
    /// Returns (new node id, new slide id).
    pub fn clone_slide(&mut self, index: usize, insert_at: usize) -> Result<(ObjId, ObjId), Error> {
        let nodes = self.slide_nodes();
        let (node_id, slide_id) = *nodes.get(index).ok_or(Error::Invalid("no such slide".into()))?;
        self.clone_slide_from(slide_id, node_id, insert_at)
    }

    /// Copy the component of `slide_id` (a slide, or a layout's template slide) as a new slide at
    /// top-level position `insert_at`; `node_id` is an existing slide node used as the template
    /// for the new node. Returns (new node id, new slide id).
    pub fn clone_slide_from(&mut self, slide_id: ObjId, node_id: ObjId, insert_at: usize) -> Result<(ObjId, ObjId), Error> {
        self.clone_slide_between(None, slide_id, node_id, insert_at)
    }

    /// Like [`clone_slide_from`], but the slide (and its node) may come from another document
    /// `source` that shares this document's stylesheet/layout object ids (e.g. the theme file a
    /// new presentation was made from).
    pub fn clone_slide_between(&mut self, source: Option<&KeyFile>, slide_id: ObjId, node_id: ObjId, insert_at: usize) -> Result<(ObjId, ObjId), Error> {
        let (comp, src_name, src, node_arch) = {
            let s: &KeyFile = source.unwrap_or(self);
            let comp = s.component(slide_id).cloned().ok_or(Error::Invalid("slide is not a component".into()))?;
            let locator = comp.str("locator").or(comp.str("preferred_locator")).unwrap_or_default().to_string();
            // (only a generic name: Keynote's file is "<name>-<id>")
            let src_name = [format!("Index/{locator}.iwa"), format!("Index/{locator}-{slide_id}.iwa")].into_iter().find(|n| s.iwa(n).is_some()).unwrap_or(format!("Index/{locator}.iwa"));
            let src = s.iwa(&src_name).cloned().ok_or(Error::Invalid(format!("missing {src_name}")))?;
            let node_arch = s.archive(node_id).cloned().ok_or(Error::Invalid("missing slide node".into()))?;
            (comp, src_name, src, node_arch)
        };

        // 1. fresh ids for every object in the slide's file (+1 for the new slide node)
        let old_ids: Vec<ObjId> = src.archives.iter().map(|a| a.id()).collect();
        let new_ids = self.alloc_ids(old_ids.len() as u64 + 1);
        let idmap: HashMap<ObjId, ObjId> = old_ids.iter().copied().zip(new_ids.iter().copied()).collect();
        let new_node_id = *new_ids.last().unwrap();
        let new_slide_id = *idmap.get(&slide_id).ok_or(Error::Invalid(format!("slide {slide_id} isn't in {src_name}")))?;

        // 2. copy + remap
        let mut clone = src.clone();
        for a in &mut clone.archives {
            let id = a.id();
            a.info.set_u64("identifier", idmap[&id]);
            for i in 0..a.payloads.len() {
                let refs: Vec<ObjId> = a.object_references(i).into_iter().map(|r| *idmap.get(&r).unwrap_or(&r)).collect();
                if !refs.is_empty() {
                    a.set_object_references(i, &refs);
                }
                a.payloads[i].for_each_ref_mut(&mut |r| {
                    if let Some(n) = idmap.get(r) {
                        *r = *n;
                    }
                });
            }
        }

        // 3. register the component
        let mut newcomp = comp.clone();
        newcomp.set_u64("identifier", new_slide_id);
        let new_locator = format!("Slide-{new_slide_id}");
        // always named exactly (a copy of a layout's template slide has only the generic
        // "Slide", and was then looked for in a file "Slide.iwa": found by the deck stress test)
        newcomp.set_str("locator", &new_locator);
        if newcomp.str("preferred_locator").is_some() {
            newcomp.set_str("preferred_locator", "Slide"); // what Keynote writes for slides
        }
        for e in newcomp.msgs_mut("object_uuid_map_entries") {
            let id = e.u64("identifier").unwrap_or(0);
            e.set_u64("identifier", *idmap.get(&id).unwrap_or(&id));
            new_uuid(e.msg_mut_or_create("uuid"));
        }
        for dr in newcomp.msgs_mut("data_references") {
            for orl in dr.msgs_mut("object_reference_list") {
                let id = orl.u64("object_identifier").unwrap_or(0);
                orl.set_u64("object_identifier", *idmap.get(&id).unwrap_or(&id));
            }
        }
        self.package_metadata_mut().unwrap().push_msg("components", newcomp);
        let pos = self.entries.iter().position(|e| e.name == src_name).map(|p| p + 1).unwrap_or(self.entries.len());
        self.entries.insert(pos, Entry { name: format!("Index/{new_locator}.iwa"), data: EntryData::Iwa(clone) });
        self.reindex(); // entry positions shifted

        // 4. slide node (in Document.iwa)
        let mut node_arch = node_arch;
        node_arch.info.set_u64("identifier", new_node_id);
        let node = &mut node_arch.payloads[0];
        for f in ["thumbnails", "thumbnailSizes", "database_thumbnail", "database_thumbnails", "uniqueIdentifier", "copyFromSlideIdentifier", "children"] {
            node.remove(f);
        }
        node.set_ref("slide", new_slide_id);
        node.set_bool("thumbnailsAreDirty", true);
        node_arch.set_object_references(0, &[new_slide_id]);
        node_arch.set_data_references(0, &[]);
        self.iwa_mut("Index/Document.iwa").ok_or(Error::Invalid("no Document.iwa".into()))?.archives.push(node_arch);
        self.reindex();

        // 5. slide tree + show references
        let show_id = self.show_id()?;
        {
            let show = self.archive_mut(show_id).unwrap();
            let tree = show.payloads[0].msg_mut("slideTree").unwrap();
            tree.insert_msg("slides", insert_at, reference(new_node_id));
            let mut refs = show.object_references(0);
            refs.push(new_node_id);
            show.set_object_references(0, &refs);
        }

        // 6. the Document component declares the new cross-component reference
        if let Some(dc) = self.document_component() {
            let mut xr = Msg::new("TSP.ComponentExternalReference");
            xr.set_u64("component_identifier", new_slide_id);
            self.component_mut(dc).unwrap().push_msg("external_references", xr);
        }
        self.reindex();
        Ok((new_node_id, new_slide_id))
    }

    /// Delete slides by top-level position (tree entry, node, component and its .iwa).
    /// Orphaned data files are left in place; Keynote cleans them up on its next save.
    pub fn delete_slides(&mut self, indices: &[usize]) -> Result<(), Error> {
        let nodes = self.slide_nodes();
        let victims: Vec<(ObjId, ObjId)> = indices.iter().filter_map(|i| nodes.get(*i).copied()).collect();
        if victims.len() >= nodes.len() {
            return Err(Error::Invalid("a presentation needs at least one slide".into()));
        }
        let node_ids: Vec<ObjId> = victims.iter().map(|v| v.0).collect();
        let slide_ids: Vec<ObjId> = victims.iter().map(|v| v.1).collect();
        let show_id = self.show_id()?;
        {
            let show = self.archive_mut(show_id).unwrap();
            let tree = show.payloads[0].msg_mut("slideTree").unwrap();
            tree.retain_msgs("slides", |r| !node_ids.contains(&r.u64("identifier").unwrap_or(0)));
            let refs: Vec<ObjId> = show.object_references(0).into_iter().filter(|r| !node_ids.contains(r)).collect();
            show.set_object_references(0, &refs);
        }
        if let Some(doc) = self.iwa_mut("Index/Document.iwa") {
            doc.archives.retain(|a| !node_ids.contains(&a.id()));
        }
        let mut files = Vec::new();
        {
            let pm = self.package_metadata_mut().unwrap();
            for c in pm.msgs("components") {
                if let Some(id) = c.u64("identifier").filter(|i| slide_ids.contains(i)) {
                    let loc = c.str("locator").or(c.str("preferred_locator")).unwrap_or_default();
                    files.push(format!("Index/{loc}.iwa"));
                    // (a generic name: Keynote's file is "<name>-<id>")
                    files.push(format!("Index/{loc}-{id}.iwa"));
                }
            }
            pm.retain_msgs("components", |c| !slide_ids.contains(&c.u64("identifier").unwrap_or(0)));
        }
        self.entries.retain(|e| !files.contains(&e.name));
        self.reindex();
        if let Some(dc) = self.document_component() {
            if let Some(c) = self.component_mut(dc) {
                c.retain_msgs("external_references", |x| {
                    !(slide_ids.contains(&x.u64("component_identifier").unwrap_or(0)) && x.u64("object_identifier").is_none())
                });
            }
        }
        self.reindex();
        // what pointed at the deleted slides from outside them: their tables' records (kept in
        // the calculation engine's file) lose their parent, the saved view selection forgets them
        let ids: Vec<ObjId> = self.archives().map(|(_, a)| a.id()).collect();
        let mut gone_tables = Vec::new();
        for id in ids {
            let tn = self.type_name(id);
            let Some(a) = self.archive_mut(id) else { continue };
            match tn {
                "TST.TableInfoArchive" => {
                    let Some(d) = a.obj_mut().find_super_mut("TSD.DrawableArchive") else { continue };
                    if d.ref_id("parent").is_some_and(|p| slide_ids.contains(&p)) {
                        d.remove("parent");
                        a.refresh_object_references();
                        gone_tables.push(id);
                    }
                }
                "KN.SlideCollectionSelectionArchive" => {
                    let m = a.obj_mut();
                    let before = m.fields.len();
                    m.retain_msgs("slide_nodes", |r| !node_ids.contains(&r.u64("identifier").unwrap_or(0)));
                    if m.ref_id("slide_node_to_edit").is_some_and(|n| node_ids.contains(&n)) {
                        m.remove("slide_node_to_edit");
                    }
                    m.retain_msgs("selected_paragraph_indexes_for_slide_nodes", |o| !o.ref_id("slide_node").is_some_and(|n| node_ids.contains(&n)));
                    if m.fields.len() != before {
                        a.refresh_object_references();
                    }
                }
                _ => {}
            }
        }
        // and the calculation engine forgets those tables (as Numbers' own delete, N3)
        for t in gone_tables {
            self.remove_table_engine(t);
        }
        Ok(())
    }

    /// Move a top-level slide from one position to another.
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<(), Error> {
        let show_id = self.show_id()?;
        let show = self.archive_mut(show_id).unwrap();
        let tree = show.payloads[0].msg_mut("slideTree").unwrap();
        let refs: Vec<Msg> = tree.msgs("slides").cloned().collect();
        if from >= refs.len() {
            return Err(Error::Invalid("no such slide".into()));
        }
        let mut refs = refs;
        let r = refs.remove(from);
        refs.insert(to.min(refs.len()), r);
        tree.remove("slides");
        for r in refs {
            tree.push_msg("slides", r);
        }
        Ok(())
    }
}
