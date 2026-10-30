//! Unreferenced objects and data: what a delete leaves behind. [`orphans`] walks the references
//! from the package's component roots (as the Mac's check does); [`Pages::purge`] removes deleted
//! objects with everything only they owned, their attachment and their Data entries.

use std::collections::{HashMap, HashSet};

use kn_format::{KeyFile, ObjId};
use kn_model::edit::History;

use crate::Pages;

/// Every object an archive refers to (declared references and the references in its payloads).
fn refs_of(a: &kn_format::Archive) -> Vec<ObjId> {
    let mut out = Vec::new();
    for (i, p) in a.payloads.iter().enumerate() {
        out.extend(a.object_references(i));
        p.collect_refs(&mut out);
    }
    out
}

/// The objects an archive declares it refers to (what keeps them alive for Pages; undeclared
/// payload references, like the calculation engine's to a table, are weak).
fn declared_of(a: &kn_format::Archive) -> Vec<ObjId> {
    (0..a.payloads.len()).flat_map(|i| a.object_references(i)).collect()
}

fn data_of(a: &kn_format::Archive) -> Vec<u64> {
    (0..a.payloads.len()).flat_map(|i| a.data_references(i)).collect()
}

/// (unreachable archives, unreferenced data ids), walking from the component roots.
pub fn orphans(k: &KeyFile) -> (Vec<ObjId>, Vec<u64>) {
    let mut all: HashMap<ObjId, &kn_format::Archive> = HashMap::new();
    for (_, a) in k.archives() {
        all.insert(a.id(), a);
    }
    // the roots, as the Mac's check takes them: the document (object 1) and the package-level
    // objects (ids under 1000). A component (a table's tiles, its model) is no root of its own:
    // a deleted table's .iwa files go with it
    let mut todo: Vec<ObjId> = all.keys().copied().filter(|i| *i == 1 || *i < 1000).collect();
    // Metadata.iwa is all roots
    if let Some(iwa) = k.iwa("Index/Metadata.iwa") {
        todo.extend(iwa.archives.iter().map(|a| a.id()));
    }
    let mut seen: HashSet<ObjId> = HashSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(a) = all.get(&id) {
            todo.extend(declared_of(a));
        }
    }
    let mut dead: Vec<ObjId> = all.keys().copied().filter(|i| !seen.contains(i)).collect();
    dead.sort_unstable();
    let used: HashSet<u64> = seen.iter().filter_map(|i| all.get(i)).flat_map(|a| data_of(a)).collect();
    let mut dead_data: Vec<u64> = k.package_metadata().map(|pm| pm.msgs("datas").filter_map(|d| d.u64("identifier")).filter(|d| !used.contains(d)).collect()).unwrap_or_default();
    dead_data.sort_unstable();
    (dead, dead_data)
}

impl Pages {
    /// Remove deleted drawables `roots` (already off their pages / out of the text) with the
    /// objects only they own, attachments pointing at them, and Data no longer used.
    pub fn purge(&mut self, h: &mut History, roots: &[ObjId]) {
        if roots.is_empty() {
            return;
        }
        // candidates: each root and what it owns in its own file
        let mut cand: HashSet<ObjId> = HashSet::new();
        for r in roots {
            match self.file().extract(*r) {
                Ok(ex) => cand.extend(ex.archives.iter().map(|a| a.id())),
                Err(_) => {
                    cand.insert(*r);
                }
            }
        }
        // attachments that point at a root and that nothing else uses
        for (_, a) in self.file().archives() {
            let m = a.obj();
            if matches!(m.type_name(), "TSWP.DrawableAttachmentArchive") && m.ref_id("drawable").is_some_and(|d| roots.contains(&d)) {
                cand.insert(a.id());
            }
        }
        // keep what anything outside the candidates still refers to (shared styles, …); repeat
        // until stable since a kept candidate keeps what it refers to
        loop {
            let mut outside: HashSet<ObjId> = HashSet::new();
            for (_, a) in self.file().archives() {
                if !cand.contains(&a.id()) {
                    outside.extend(refs_of(a));
                }
            }
            // a reference from another root's archive back at a root is fine (parent links)
            let keep: Vec<ObjId> = cand.iter().copied().filter(|c| outside.contains(c) && !roots.contains(c)).collect();
            if keep.is_empty() {
                break;
            }
            for k in keep {
                cand.remove(&k);
            }
        }
        if cand.is_empty() {
            return;
        }
        self.doc.touch_all(h);
        let data: HashSet<u64> = cand.iter().filter_map(|i| self.file().archive(*i)).flat_map(data_of).collect();
        let k = &mut self.doc.file;
        for id in &cand {
            k.remove_archive(*id);
        }
        if let Some(pm) = k.package_metadata_mut() {
            for c in pm.msgs_mut("components") {
                c.retain_msgs("object_uuid_map_entries", |e| !e.u64("identifier").is_some_and(|i| cand.contains(&i)));
                for d in c.msgs_mut("data_references") {
                    d.retain_msgs("object_reference_list", |o| !o.u64("object_identifier").is_some_and(|i| cand.contains(&i)));
                }
                c.retain_msgs("data_references", |d| d.count("object_reference_list") > 0);
            }
        }
        // data nobody uses any more
        let still: HashSet<u64> = k.archives().flat_map(|(_, a)| data_of(a)).collect();
        for d in data {
            if !still.contains(&d) {
                k.remove_data(d);
            }
        }
    }
}

/// Style variations nothing uses any more (only the stylesheet lists them): Pages culls these
/// when it saves (TSS.StylesheetArchive.can_cull_styles). Named styles always stay.
pub fn cull_variations(k: &mut KeyFile) -> usize {
    let Some(sheet) = k.stylesheet_id() else { return 0 };
    let mut culled = 0;
    loop {
        let mut used: HashSet<ObjId> = HashSet::new();
        for (_, a) in k.archives() {
            if a.id() != sheet {
                used.extend(refs_of(a));
            }
        }
        let identified: HashSet<ObjId> = k.obj(sheet).map(|m| m.msgs("identifier_to_style_map").filter_map(|e| e.ref_id("style")).collect()).unwrap_or_default();
        let dead: Vec<ObjId> = k
            .obj(sheet)
            .map(|m| m.ref_ids("styles"))
            .unwrap_or_default()
            .into_iter()
            .filter(|id| !used.contains(id) && !identified.contains(id))
            .filter(|id| k.obj(*id).and_then(|m| m.find_super("TSS.StyleArchive")).is_some_and(|s| s.bool("is_variation") == Some(true)))
            .collect();
        if dead.is_empty() {
            break;
        }
        let set: HashSet<ObjId> = dead.iter().copied().collect();
        if let Some(a) = k.archive_mut(sheet) {
            let m = a.obj_mut();
            m.retain_msgs("styles", |r| !r.u64("identifier").is_some_and(|i| set.contains(&i)));
            for e in m.msgs_mut("parent_to_children_style_map") {
                e.retain_msgs("children", |r| !r.u64("identifier").is_some_and(|i| set.contains(&i)));
            }
            m.retain_msgs("parent_to_children_style_map", |e| e.count("children") > 0);
            a.refresh_object_references();
        }
        for id in &dead {
            k.remove_archive(*id);
        }
        culled += dead.len();
    }
    culled
}

/// Declared references to objects that aren't in the file come out of the declarations (Pages'
/// own stylesheets can name styles that aren't saved, but never declare them; an edit that
/// refreshes the declarations would otherwise list them).
pub fn undeclare_missing(k: &mut KeyFile) -> usize {
    let ids: HashSet<ObjId> = k.archives().map(|(_, a)| a.id()).collect();
    let fix: Vec<ObjId> = k.archives().filter(|(_, a)| (0..a.payloads.len()).any(|i| a.object_references(i).iter().any(|r| !ids.contains(r)))).map(|(_, a)| a.id()).collect();
    for id in &fix {
        if let Some(a) = k.archive_mut(*id) {
            for i in 0..a.payloads.len() {
                let refs: Vec<ObjId> = a.object_references(i).into_iter().filter(|r| ids.contains(r)).collect();
                a.set_object_references(i, &refs);
            }
        }
    }
    fix.len()
}

/// The saved selection (Index/ViewState.iwa) names only objects that are still there (a
/// deleted object stays selected in the view state otherwise).
pub fn clear_dead_selections(k: &mut KeyFile) -> usize {
    let ids: HashSet<ObjId> = k.archives().map(|(_, a)| a.id()).collect();
    let sel: Vec<ObjId> = k
        .archives()
        .filter(|(_, a)| matches!(a.obj().type_name(), "TP.CanvasSelectionArchive" | "TSD.DrawableSelectionArchive" | "TSD.CanvasSelectionArchive" | "TSK.SelectionPathArchive"))
        .map(|(_, a)| a.id())
        .collect();
    let mut fixed = 0;
    for id in sel {
        let Some(a) = k.archive_mut(id) else { continue };
        let m = a.obj_mut();
        let before = format!("{m:?}").len();
        for f in ["infos", "excluded_infos", "additional_infos", "non_interactive_infos", "ordered_selections"] {
            m.retain_msgs(f, |r| r.u64("identifier").is_none_or(|i| ids.contains(&i)));
        }
        for f in ["container", "deprecated_container"] {
            if m.ref_id(f).is_some_and(|i| !ids.contains(&i)) {
                m.remove(f);
            }
        }
        if format!("{m:?}").len() != before {
            a.refresh_object_references();
            fixed += 1;
        }
    }
    fixed
}

/// .iwa files left with no objects (a deleted table's tiles and lists) go, with their
/// component and the other components' references to it.
pub fn drop_empty_components(k: &mut KeyFile) -> usize {
    let empty: Vec<String> = k
        .entries
        .iter()
        .map(|e| e.name.clone())
        .filter(|n| n.starts_with("Index/") && n.ends_with(".iwa") && n != "Index/Metadata.iwa")
        .filter(|n| k.iwa(n).is_some_and(|f| f.archives.is_empty()))
        .collect();
    let mut dropped = 0;
    for name in empty {
        let comp = k.component_for_file(&name);
        k.remove_entry(&name);
        dropped += 1;
        let Some(comp) = comp else { continue };
        if let Some(pm) = k.package_metadata_mut() {
            pm.retain_msgs("components", |c| c.u64("identifier") != Some(comp));
            for c in pm.msgs_mut("components") {
                c.retain_msgs("external_references", |e| e.u64("component_identifier") != Some(comp));
                c.retain_msgs("versioned_external_references", |e| e.u64("component_identifier") != Some(comp));
            }
        }
    }
    if dropped > 0 {
        k.reindex();
    }
    dropped
}

/// Remove every unreachable archive and unused Data entry (and their bookkeeping).
pub fn sweep(k: &mut KeyFile) -> (usize, usize) {
    cull_variations(k);
    // tables about to go come out of the calculation engine first (as Numbers does)
    let dead_tables: Vec<ObjId> = orphans(k).0.into_iter().filter(|i| k.type_name(*i) == "TST.TableInfoArchive").collect();
    for t in dead_tables {
        k.remove_table_engine(t);
    }
    let (dead, dead_data) = orphans(k);
    let set: HashSet<ObjId> = dead.iter().copied().collect();
    for id in &dead {
        k.remove_archive(*id);
    }
    if let Some(pm) = k.package_metadata_mut() {
        for c in pm.msgs_mut("components") {
            c.retain_msgs("object_uuid_map_entries", |e| !e.u64("identifier").is_some_and(|i| set.contains(&i)));
            c.retain_msgs("external_references", |e| !e.u64("object_identifier").is_some_and(|i| set.contains(&i)));
            for d in c.msgs_mut("data_references") {
                d.retain_msgs("object_reference_list", |o| !o.u64("object_identifier").is_some_and(|i| set.contains(&i)));
            }
            c.retain_msgs("data_references", |d| d.count("object_reference_list") > 0);
        }
    }
    for d in &dead_data {
        k.remove_data(*d);
    }
    drop_empty_components(k);
    clear_dead_selections(k);
    undeclare_missing(k);
    (dead.len(), dead_data.len())
}

impl Pages {
    /// Save to `path`: a cleaned copy (no unreachable objects or unused data, as Pages saves)
    /// is written; the document in memory keeps everything so undo still works.
    pub fn save_as(&mut self, path: impl AsRef<std::path::Path>) -> Result<(), kn_format::Error> {
        self.doc.declare_external_refs();
        let mut k = self.doc.file.clone();
        sweep(&mut k);
        let path = path.as_ref();
        if let Some((pw, hint)) = &self.doc.password {
            k.save_encrypted(path, pw, hint)?;
        } else if self.doc.package {
            k.save_package(path)?;
        } else {
            k.save(path)?;
        }
        self.doc.path = Some(path.to_path_buf());
        self.doc.dirty = false;
        Ok(())
    }
}
