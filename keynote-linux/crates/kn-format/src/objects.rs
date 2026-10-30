//! Creating objects: style variations, copies of drawables (within a document or from a donor
//! document), pictures. Ported from the research writer's "donor copy" approach (keywriter.py):
//! copy a real object Keynote made, give it fresh ids/uuids and repoint its references.

use std::collections::{HashMap, HashSet};

use crate::iwa::Archive;
use crate::keyfile::{Entry, EntryData, KeyFile};
use crate::msg::{Msg, ObjId, Val};
use crate::Error;

pub fn new_uuid(m: &mut Msg) {
    let u = uuid::Uuid::new_v4().as_u128();
    m.set_u64("lower", u as u64);
    m.set_u64("upper", (u >> 64) as u64);
}

/// Objects copied out of a document: a drawable and everything it owns in its .iwa file.
/// Independent of the source document, so it can serve as a clipboard.
#[derive(Debug, Clone)]
pub struct Extract {
    pub root: ObjId,
    pub archives: Vec<Archive>,
    /// The drawable's parent in the source (its slide), remapped to the target slide.
    pub src_parent: Option<ObjId>,
    /// Objects that had a uuid in the source component (they get fresh ones).
    pub uuids: HashSet<ObjId>,
    /// Referenced objects outside the copied set.
    pub external: Vec<ObjId>,
}

/// Number of properties a style overrides (Keynote keeps this count on every level).
pub fn fix_override_count(style: &mut Msg) {
    fn count(m: &Msg) -> u64 {
        let mut n = 0;
        let mut m = m;
        loop {
            for f in &m.fields {
                if (f.num == 11 || f.num == 12) && m.type_name() != "TSS.StyleArchive" {
                    if let Val::Msg(p) = &f.val {
                        let mut nums: Vec<u32> = p.fields.iter().map(|f| f.num).collect();
                        nums.dedup();
                        n += nums.len() as u64;
                        // font_color + tsd_fill count as one (the text colour)
                        if p.has("font_color") && p.has("tsd_fill") {
                            n -= 1;
                        }
                    }
                }
            }
            match m.msg("super") {
                Some(s) => m = s,
                None => break,
            }
        }
        n
    }
    let n = count(style);
    let mut m = style;
    loop {
        if m.has_field("override_count") {
            m.set_u64("override_count", n);
        }
        if m.msg("super").is_none() {
            break;
        }
        m = m.msg_mut("super").unwrap();
    }
}

/// Empty every property message of a style (all levels), leaving the structure.
fn clear_props(style: &mut Msg) {
    let mut m = style;
    loop {
        if m.type_name() == "TSS.StyleArchive" {
            break;
        }
        for f in &mut m.fields {
            match (&mut f.val, f.num) {
                (Val::Msg(p), 11 | 12) => p.fields.clear(),
                (Val::Bytes(b), 11 | 12) => b.clear(),
                (Val::Varint(v), 10) => *v = 0,
                _ => {}
            }
        }
        match m.msg_mut("super") {
            Some(s) => m = s,
            None => break,
        }
    }
}

/// Pixel size of a PNG or JPEG.
pub fn image_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") && data.len() >= 24 {
        let w = u32::from_be_bytes(data[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(data[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if data.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 9 < data.len() {
            if data[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            if matches!(marker, 0xc0 | 0xc1 | 0xc2) {
                let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

impl KeyFile {
    /// Recompute an archive's object_references from its payload (stylesheet refs left out,
    /// as Keynote does).
    pub fn refresh_refs(&mut self, id: ObjId) {
        let sheets: Vec<ObjId> = self.objects_of("TSS.StylesheetArchive").map(|(i, _)| i).collect();
        if let Some(a) = self.archive_mut(id) {
            a.refresh_object_references_except(&sheets);
        }
    }

    pub(crate) fn file_components(&self) -> HashMap<String, ObjId> {
        let mut out = HashMap::new();
        if let Some(pm) = self.package_metadata() {
            for c in pm.msgs("components") {
                if let (Some(loc), Some(id)) = (c.str("locator").or(c.str("preferred_locator")), c.u64("identifier")) {
                    out.insert(format!("Index/{loc}.iwa"), id);
                }
            }
        }
        out
    }

    /// Make the component stored in `file` declare every object it references in other
    /// components (Keynote reports the file as damaged otherwise). Only adds entries.
    pub fn ensure_external_refs(&mut self, file: &str) {
        let comps = self.file_components();
        self.ensure_external_refs_in(file, &comps);
    }

    /// [`KeyFile::ensure_external_refs`] with the file -> component map made once (a caller
    /// fixing many files: making it is slow in a document with thousands of components).
    pub(crate) fn ensure_external_refs_in(&mut self, file: &str, comps: &HashMap<String, ObjId>) {
        let Some(&me) = comps.get(file) else { return };
        let Some(iwa) = self.iwa(file) else { return };
        let local: HashSet<ObjId> = iwa.archives.iter().map(|a| a.id()).collect();
        let mut needed: Vec<(ObjId, Option<ObjId>)> = Vec::new();
        // (a set beside the list: a large document refers to thousands of components)
        let mut seen: HashSet<(ObjId, Option<ObjId>)> = HashSet::new();
        for a in &iwa.archives {
            for i in 0..a.payloads.len() {
                for r in a.object_references(i) {
                    if local.contains(&r) {
                        continue;
                    }
                    let Some(c) = self.file_of(r).and_then(|f| comps.get(f)).copied() else { continue };
                    if c == me {
                        continue;
                    }
                    let e = (c, if r == c { None } else { Some(r) });
                    if seen.insert(e) {
                        needed.push(e);
                    }
                }
            }
        }
        let Some(comp) = self.component_mut(me) else { return };
        let have: HashSet<(ObjId, Option<ObjId>)> = comp
            .msgs("external_references")
            .map(|x| (x.u64("component_identifier").unwrap_or(0), x.u64("object_identifier")))
            .collect();
        for (c, o) in needed {
            if have.contains(&(c, o)) {
                continue;
            }
            let mut xr = Msg::new("TSP.ComponentExternalReference");
            xr.set_u64("component_identifier", c);
            if let Some(o) = o {
                xr.set_u64("object_identifier", o);
            }
            comp.push_msg("external_references", xr);
        }
    }

    /// [`KeyFile::ensure_external_refs`] for only the archives `ids` of `file` (the ones an
    /// edit changed): no scan of the whole file and its thousands of components.
    pub fn ensure_external_refs_of(&mut self, file: &str, ids: &[ObjId]) {
        let Some(me) = self.component_for_file(file) else { return };
        let mut comp_of: HashMap<String, Option<ObjId>> = HashMap::new();
        let mut needed: Vec<(ObjId, Option<ObjId>)> = Vec::new();
        let mut seen: HashSet<(ObjId, Option<ObjId>)> = HashSet::new();
        for id in ids {
            let Some(a) = self.archive(*id) else { continue };
            for i in 0..a.payloads.len() {
                for r in a.object_references(i) {
                    let Some(f) = self.file_of(r) else { continue };
                    if f == file {
                        continue;
                    }
                    let c = match comp_of.get(f) {
                        Some(c) => *c,
                        None => {
                            let c = self.component_for_file(f);
                            comp_of.insert(f.to_string(), c);
                            c
                        }
                    };
                    let Some(c) = c.filter(|c| *c != me) else { continue };
                    let e = (c, if r == c { None } else { Some(r) });
                    if seen.insert(e) {
                        needed.push(e);
                    }
                }
            }
        }
        if needed.is_empty() {
            return;
        }
        let Some(comp) = self.component_mut(me) else { return };
        let have: HashSet<(ObjId, Option<ObjId>)> = comp
            .msgs("external_references")
            .map(|x| (x.u64("component_identifier").unwrap_or(0), x.u64("object_identifier")))
            .collect();
        for (c, o) in needed {
            if have.contains(&(c, o)) {
                continue;
            }
            let mut xr = Msg::new("TSP.ComponentExternalReference");
            xr.set_u64("component_identifier", c);
            if let Some(o) = o {
                xr.set_u64("object_identifier", o);
            }
            comp.push_msg("external_references", xr);
        }
    }

    /// An object's UUID as Keynote writes it in links ("F3EB27A9-E848-…").
    pub fn object_uuid(&self, id: ObjId) -> Option<String> {
        let pm = self.package_metadata()?;
        let e = pm.msgs("components").flat_map(|c| c.msgs("object_uuid_map_entries")).find(|e| e.u64("identifier") == Some(id))?;
        let b = crate::tables::uuid_of_tsp(e.msg("uuid")?);
        Some(uuid::Uuid::from_bytes(b).to_string().to_uppercase())
    }

    /// An object's UUID as stored (TSP.UUID lower / upper), if it has one.
    pub fn object_uuid_msg(&self, id: ObjId) -> Option<Msg> {
        let pm = self.package_metadata()?;
        pm.msgs("components").flat_map(|c| c.msgs("object_uuid_map_entries")).find(|e| e.u64("identifier") == Some(id))?.msg("uuid").cloned()
    }

    /// The object with this UUID string.
    pub fn object_by_uuid(&self, s: &str) -> Option<ObjId> {
        let want = crate::tables::uuid_of_str(&s.to_lowercase())?;
        let pm = self.package_metadata()?;
        pm.msgs("components").flat_map(|c| c.msgs("object_uuid_map_entries")).find(|e| e.msg("uuid").map(crate::tables::uuid_of_tsp) == Some(want)).and_then(|e| e.u64("identifier"))
    }

    /// Give `id` a fresh uuid in the object-uuid map of the component stored in `file`.
    pub fn add_uuid_entry(&mut self, file: &str, id: ObjId) {
        let Some(c) = self.component_for_file(file) else { return };
        let Some(comp) = self.component_mut(c) else { return };
        let mut e = Msg::new("TSP.ObjectUUIDMapEntry");
        e.set_u64("identifier", id);
        new_uuid(e.msg_mut_or_create("uuid"));
        comp.push_msg("object_uuid_map_entries", e);
    }

    /// The document stylesheet.
    pub fn stylesheet_id(&self) -> Option<ObjId> {
        self.objects_of("TSS.StylesheetArchive")
            .find(|(id, _)| self.file_of(*id) == Some("Index/DocumentStylesheet.iwa"))
            .or_else(|| self.first_of("TSS.StylesheetArchive"))
            .map(|(id, _)| id)
    }

    /// Create a variation of style `base` (a copy that only carries overridden properties, with
    /// `parent` = the named style), registered in the stylesheet like Keynote does. If `base` is
    /// itself a variation, the new one keeps its overrides and parent. Returns the new id.
    pub fn new_style_variation(&mut self, base: ObjId) -> Result<ObjId, Error> {
        let src = self.archive(base).cloned().ok_or(Error::Invalid(format!("no style {base}")))?;
        let mut p = src.payloads[0].clone();
        let tss = p.find_super("TSS.StyleArchive").ok_or(Error::Invalid("not a style".into()))?;
        let was_variation = tss.bool("is_variation") == Some(true) && tss.ref_id("parent").is_some();
        let parent = if was_variation { tss.ref_id("parent").unwrap() } else { base };
        let sheet = tss.ref_id("stylesheet").or_else(|| self.stylesheet_id());
        if !was_variation {
            clear_props(&mut p);
        }
        {
            let t = p.find_super_mut("TSS.StyleArchive").unwrap();
            t.remove("name");
            t.remove("style_identifier");
            t.set_ref("parent", parent);
            t.set_bool("is_variation", true);
        }
        fix_override_count(&mut p);
        let id = self.alloc_ids(1)[0];
        let mut a = src;
        a.info.set_u64("identifier", id);
        a.payloads.truncate(1);
        a.payloads[0] = p;
        a.refresh_object_references_except(&sheet.into_iter().collect::<Vec<_>>());
        let file = sheet
            .and_then(|s| self.file_of(s))
            .or_else(|| self.file_of(base))
            .unwrap_or("Index/DocumentStylesheet.iwa")
            .to_string();
        self.insert_archive(&file, a);
        if let Some(sheet) = sheet {
            if let Some(sa) = self.archive_mut(sheet) {
                let mut refs = sa.object_references(0);
                if !refs.contains(&parent) {
                    refs.push(parent);
                }
                refs.push(id);
                sa.set_object_references(0, &refs);
            }
            if let Some(sm) = self.obj_mut(sheet) {
                sm.push_ref("styles", id);
                let has = sm.msgs("parent_to_children_style_map").any(|e| e.ref_id("parent") == Some(parent));
                match has {
                    true => sm
                        .msgs_mut("parent_to_children_style_map")
                        .find(|e| e.ref_id("parent") == Some(parent))
                        .unwrap()
                        .push_ref("children", id),
                    false => {
                        let mut e = Msg::new("TSS.StylesheetArchive.StyleChildrenEntry");
                        e.set_ref("parent", parent);
                        e.push_ref("children", id);
                        sm.push_msg("parent_to_children_style_map", e);
                    }
                }
            }
        }
        self.add_uuid_entry(&file, id);
        Ok(id)
    }

    /// Copy drawable `root` and everything it owns in its own .iwa (text storage, captions…).
    pub fn extract(&self, root: ObjId) -> Result<Extract, Error> {
        let file = self.file_of(root).ok_or(Error::Invalid(format!("no object {root}")))?.to_string();
        let comp = self.component_for_file(&file);
        let iwa = self.iwa(&file).unwrap();
        let local: HashMap<ObjId, &Archive> = iwa.archives.iter().map(|a| (a.id(), a)).collect();
        let mut owned: Vec<ObjId> = Vec::new();
        let mut todo = vec![root];
        let mut external = Vec::new();
        while let Some(id) = todo.pop() {
            if owned.contains(&id) || Some(id) == comp {
                continue;
            }
            let Some(a) = local.get(&id) else {
                if !external.contains(&id) {
                    external.push(id);
                }
                continue;
            };
            owned.push(id);
            let mut refs = Vec::new();
            for (i, p) in a.payloads.iter().enumerate() {
                p.collect_refs(&mut refs);
                refs.extend(a.object_references(i));
            }
            // never climb to other drawables on the slide (connection lines point at them)
            let parent = a.obj().find_super("TSD.DrawableArchive").and_then(|d| d.ref_id("parent"));
            for r in refs {
                if Some(r) != parent {
                    todo.push(r);
                }
            }
        }
        let uuids: HashSet<ObjId> = comp
            .and_then(|c| self.component(c))
            .map(|c| c.msgs("object_uuid_map_entries").filter_map(|e| e.u64("identifier")).filter(|i| owned.contains(i)).collect())
            .unwrap_or_default();
        let src_parent = self.obj(root).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|d| d.ref_id("parent"));
        Ok(Extract {
            root,
            archives: owned.iter().map(|i| (*local[i]).clone()).collect(),
            src_parent,
            uuids,
            external,
        })
    }

    /// For an extract from another document: map its external references (styles, the
    /// stylesheet) onto this document's equivalents — same `style_identifier`, else the same
    /// name, else the parent's equivalent, else any named style of the same type.
    pub fn map_external(&self, src: &KeyFile, ids: &[ObjId]) -> Result<HashMap<ObjId, ObjId>, Error> {
        let mut by_ident: HashMap<(&str, String), ObjId> = HashMap::new();
        let mut by_name: HashMap<(&str, String), ObjId> = HashMap::new();
        let mut any: HashMap<&str, ObjId> = HashMap::new();
        for (_, a) in self.archives() {
            let m = a.obj();
            let Some(s) = m.find_super("TSS.StyleArchive") else { continue };
            if s.bool("is_variation") == Some(true) {
                continue;
            }
            let t = m.type_name();
            if let Some(i) = s.str("style_identifier") {
                by_ident.entry((t, i.to_string())).or_insert(a.id());
            }
            if let Some(n) = s.str("name") {
                by_name.entry((t, n.to_string())).or_insert(a.id());
            }
            any.entry(t).or_insert(a.id());
        }
        let sheet = self.stylesheet_id();
        let mut out = HashMap::new();
        for &id in ids {
            let mut cur = Some(id);
            let mut found = None;
            let mut depth = 0;
            while let (Some(c), None) = (cur, found) {
                depth += 1;
                if depth > 16 {
                    break;
                }
                let Some(m) = src.obj(c) else { break };
                let t = m.type_name();
                if t == "TSS.StylesheetArchive" {
                    found = sheet;
                    break;
                }
                if t == "TSCH.ChartStylePreset" {
                    // the theme's chart preset whose chart style has the same identifier
                    let ident = |k: &KeyFile, m: &Msg| {
                        m.ref_id("chart_style")
                            .and_then(|c| k.obj(c))
                            .and_then(|c| c.find_super("TSS.StyleArchive"))
                            .and_then(|s| s.str("style_identifier"))
                            .map(str::to_string)
                    };
                    let want = ident(src, m);
                    let presets: Vec<(ObjId, Option<String>)> = self.objects_of(t).map(|(i, pm)| (i, ident(self, pm))).collect();
                    found = presets.iter().find(|p| p.1.is_some() && p.1 == want).or(presets.first()).map(|p| p.0);
                    break;
                }
                let Some(s) = m.find_super("TSS.StyleArchive") else { break };
                found = s
                    .str("style_identifier")
                    .and_then(|i| by_ident.get(&(t, i.to_string())))
                    .or_else(|| s.str("name").and_then(|n| by_name.get(&(t, n.to_string()))))
                    .copied();
                if found.is_none() {
                    cur = s.ref_id("parent");
                    if cur.is_none() {
                        found = any.get(t).copied();
                    }
                }
            }
            match found {
                Some(f) => {
                    out.insert(id, f);
                }
                None => {
                    return Err(Error::Invalid(format!("can't map {} #{id} into this document", src.type_name(id))));
                }
            }
        }
        Ok(out)
    }

    /// Paste an extract onto `slide` (on top of its other objects). `external` maps references
    /// outside the extract (None = same document, keep them). Returns the new drawable id.
    pub fn paste_extract(&mut self, ex: &Extract, slide: ObjId, external: Option<&HashMap<ObjId, ObjId>>) -> Result<ObjId, Error> {
        self.paste_extract_with_data(ex, slide, external, None)
    }

    /// [`KeyFile::paste_extract`] from another document whose data files were copied with
    /// [`KeyFile::copy_data_from`] (`data` maps their ids).
    pub fn paste_extract_with_data(
        &mut self,
        ex: &Extract,
        slide: ObjId,
        external: Option<&HashMap<ObjId, ObjId>>,
        data: Option<&HashMap<u64, u64>>,
    ) -> Result<ObjId, Error> {
        let file = self.file_of(slide).ok_or(Error::Invalid("no slide".into()))?.to_string();
        let new_ids = self.alloc_ids(ex.archives.len() as u64);
        let idmap: HashMap<ObjId, ObjId> = ex.archives.iter().map(|a| a.id()).zip(new_ids.iter().copied()).collect();
        let mut full = idmap.clone();
        if let Some(p) = ex.src_parent {
            full.insert(p, slide);
        }
        if let Some(e) = external {
            full.extend(e.iter().map(|(a, b)| (*a, *b)));
        }
        let mut data_uses: Vec<(ObjId, u64)> = Vec::new();
        let mut new_archives = Vec::new();
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
                if let Some(dm) = data {
                    let refs: Vec<u64> = a.data_references(i).into_iter().filter_map(|d| dm.get(&d).copied()).collect();
                    a.set_data_references(i, &refs);
                    a.payloads[i].for_each_data_ref_mut(&mut |d| {
                        if let Some(n) = dm.get(d) {
                            *d = *n;
                        }
                    });
                    for d in refs {
                        data_uses.push((idmap[&old], d));
                    }
                } else if external.is_some() {
                    a.set_data_references(i, &[]); // donor data is replaced by the caller
                } else {
                    for d in a.data_references(i) {
                        data_uses.push((idmap[&old], d));
                    }
                }
            }
            new_archives.push(a);
        }
        let root = idmap[&ex.root];
        for a in new_archives {
            self.insert_archive(&file, a);
        }
        if let Some(d) = self.obj_mut(root).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            d.set_ref("parent", slide);
        }
        for old in &ex.uuids {
            self.add_uuid_entry(&file, idmap[old]);
        }
        for (obj, d) in data_uses {
            self.add_data_reference(&file, d, obj);
        }
        self.attach_to_slide(slide, root);
        self.ensure_external_refs(&file);
        Ok(root)
    }

    /// Put a drawable on top of a slide's z-order.
    pub fn attach_to_slide(&mut self, slide: ObjId, id: ObjId) {
        if let Some(a) = self.archive_mut(slide) {
            let s = a.obj_mut();
            // a Numbers sheet lists its drawables in one field (only with the Numbers schema)
            if s.type_name() == "TN.SheetArchive" {
                s.push_ref("drawable_infos", id);
            } else if s.field_def("owned_drawables").is_none() {
                // Pages: no slide to join (the caller places the drawable itself: in a page's
                // floating drawables or as an attachment in the body text)
                return;
            } else {
                s.push_ref("owned_drawables", id);
                s.push_ref("drawables_z_order", id);
            }
            a.refresh_object_references();
        }
    }

    /// Declare that `obj` (stored in `file`) uses data `data_id`.
    pub fn add_data_reference(&mut self, file: &str, data_id: u64, obj: ObjId) {
        // the object's own ArchiveInfo lists it too (Keynote writes both)
        if let Some(a) = self.archive_mut(obj) {
            let mut ids = a.data_references(0);
            if !ids.contains(&data_id) {
                ids.push(data_id);
                a.set_data_references(0, &ids);
            }
        }
        let Some(c) = self.component_for_file(file) else { return };
        let Some(comp) = self.component_mut(c) else { return };
        let mut orl = Msg::new("TSP.ComponentDataReference.ObjectReference");
        orl.set_u64("object_identifier", obj);
        orl.set_u64("count", 1);
        let has = comp.msgs("data_references").any(|d| d.u64("data_identifier") == Some(data_id));
        match has {
            true => {
                let d = comp.msgs_mut("data_references").find(|d| d.u64("data_identifier") == Some(data_id)).unwrap();
                if !d.msgs("object_reference_list").any(|o| o.u64("object_identifier") == Some(obj)) {
                    d.push_msg("object_reference_list", orl);
                }
            }
            false => {
                let mut d = Msg::new("TSP.ComponentDataReference");
                d.set_u64("data_identifier", data_id);
                d.push_msg("object_reference_list", orl);
                comp.push_msg("data_references", d);
            }
        }
    }

    /// Store a picture in Data/ and register its DataInfo (digest, length, pixel size, as
    /// Keynote writes them). Returns the data identifier.
    pub fn add_image_data(&mut self, bytes: Vec<u8>, file_name: &str) -> Result<u64, Error> {
        use sha1::Digest;
        let (w, h) = image_size(&bytes).ok_or(Error::Invalid("only PNG and JPEG pictures are supported".into()))?;
        // Keynote keys its data store by digest: the same bytes twice abort its loader (verified),
        // so identical data is shared
        if let Some(id) = self.data_with_digest(&sha1::Sha1::digest(&bytes)) {
            return Ok(id);
        }
        let pm = self.package_metadata().ok_or(Error::Invalid("no PackageMetadata".into()))?;
        // picture data ids have their own counter
        let data_id = pm.msgs("datas").filter_map(|d| d.u64("identifier")).max().unwrap_or(0) + 1;
        let (base, ext) = match file_name.rsplit_once('.') {
            Some((b, e)) => (b.to_string(), e.to_lowercase()),
            None => (file_name.to_string(), "png".to_string()),
        };
        let base: String = base.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let stored = format!("{base}-{data_id}.{ext}");
        let mut info = Msg::new("TSP.DataInfo");
        info.set_u64("identifier", data_id);
        info.set_bytes("digest", &sha1::Sha1::digest(&bytes));
        info.set_str("preferred_file_name", &format!("{base}.{ext}"));
        info.set_str("file_name", &stored);
        // attributes {#100 {#1 {w, h}, #3=0, #5=0, #7=0}}: the picture's pixel size
        let mut size = vec![0x0d];
        size.extend((w as f32).to_le_bytes());
        size.push(0x15);
        size.extend((h as f32).to_le_bytes());
        let mut ext100 = vec![0x0a, size.len() as u8];
        ext100.extend(size);
        ext100.extend([0x18, 0x00, 0x28, 0x00, 0x38, 0x00]);
        let mut attrs = vec![0xa2, 0x06, ext100.len() as u8];
        attrs.extend(ext100);
        let attr_num = info.field_def("attributes").unwrap().num;
        info.fields.push(crate::msg::Field { num: attr_num, val: Val::Bytes(attrs) });
        info.set_u64("materialized_length", bytes.len() as u64);
        self.package_metadata_mut().unwrap().push_msg("datas", info);
        self.add_entry(Entry { name: format!("Data/{stored}"), data: EntryData::Raw(std::sync::Arc::new(bytes)) });
        Ok(data_id)
    }

    /// Replace a data file's bytes (File > Reduce File Size). The stored name changes to
    /// `file_name` (made unique with the data id); the digest and length follow.
    pub fn replace_data_bytes(&mut self, data_id: u64, bytes: Vec<u8>, file_name: &str) -> bool {
        use sha1::Digest;
        // (theme artwork only named by a resource locator has no Data/ entry yet: it gets one)
        let old = self.data_entry(data_id).map(|e| e.name.clone());
        let known = self.package_metadata().is_some_and(|pm| pm.msgs("datas").any(|d| d.u64("identifier") == Some(data_id)));
        if old.is_none() && !known {
            return false;
        }
        let (base, ext) = file_name.rsplit_once('.').unwrap_or((file_name, "png"));
        let base: String = base.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let stored = format!("{base}-{data_id}.{ext}");
        let digest = sha1::Sha1::digest(&bytes);
        let len = bytes.len() as u64;
        if let Some(old) = old {
            self.remove_entry(&old);
        }
        self.add_entry(Entry { name: format!("Data/{stored}"), data: EntryData::Raw(std::sync::Arc::new(bytes)) });
        if let Some(pm) = self.package_metadata_mut() {
            for d in pm.msgs_mut("datas") {
                if d.u64("identifier") == Some(data_id) {
                    d.set_bytes("digest", &digest);
                    d.set_str("file_name", &stored);
                    d.set_str("preferred_file_name", &format!("{base}.{ext}"));
                    d.set_u64("materialized_length", len);
                    // the pixel size in `attributes` no longer applies; the bytes are ours now
                    d.remove("attributes");
                    d.remove("document_resource_locator");
                }
            }
        }
        true
    }

    /// The data id already holding bytes with this SHA-1 digest.
    pub fn data_with_digest(&self, digest: &[u8]) -> Option<u64> {
        self.package_metadata()?.msgs("datas").find(|d| d.bytes("digest") == Some(digest)).and_then(|d| d.u64("identifier"))
    }

    /// Add any file (movie, audio) as package data; returns its data id.
    pub fn add_file_data(&mut self, bytes: Vec<u8>, file_name: &str) -> Result<u64, Error> {
        use sha1::Digest;
        if let Some(id) = self.data_with_digest(&sha1::Sha1::digest(&bytes)) {
            return Ok(id);
        }
        let pm = self.package_metadata().ok_or(Error::Invalid("no PackageMetadata".into()))?;
        let data_id = pm.msgs("datas").filter_map(|d| d.u64("identifier")).max().unwrap_or(0) + 1;
        let (base, ext) = file_name.rsplit_once('.').map(|(b, e)| (b.to_string(), e.to_lowercase())).unwrap_or((file_name.to_string(), "dat".into()));
        let base: String = base.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let stored = format!("{base}-{data_id}.{ext}");
        let mut info = Msg::new("TSP.DataInfo");
        info.set_u64("identifier", data_id);
        info.set_bytes("digest", &sha1::Sha1::digest(&bytes));
        info.set_str("preferred_file_name", &format!("{base}.{ext}"));
        info.set_str("file_name", &stored);
        info.set_u64("materialized_length", bytes.len() as u64);
        self.package_metadata_mut().unwrap().push_msg("datas", info);
        self.add_entry(Entry { name: format!("Data/{stored}"), data: EntryData::Raw(std::sync::Arc::new(bytes)) });
        Ok(data_id)
    }

    /// Remove data file `data_id` entirely: its DataInfo, its Data/ entry and every component's
    /// reference to it.
    pub fn remove_data(&mut self, data_id: u64) {
        let Some(pm) = self.package_metadata_mut() else { return };
        let file = pm.msgs("datas").find(|d| d.u64("identifier") == Some(data_id)).and_then(|d| d.str("file_name")).map(str::to_string);
        pm.retain_msgs("datas", |d| d.u64("identifier") != Some(data_id));
        for c in pm.msgs_mut("components") {
            c.retain_msgs("data_references", |d| d.u64("data_identifier") != Some(data_id));
        }
        if let Some(f) = file {
            self.remove_entry(&format!("Data/{f}"));
        }
    }

    /// Data files an extract uses.
    pub fn extract_data_ids(ex: &Extract) -> Vec<u64> {
        let mut v: Vec<u64> = ex.archives.iter().flat_map(|a| (0..a.payloads.len()).flat_map(move |i| a.data_references(i))).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Copy data file `data_id` (and its DataInfo) from another document. Returns the new id.
    pub fn copy_data_from(&mut self, src: &KeyFile, data_id: u64) -> Option<u64> {
        let info = src.package_metadata()?.msgs("datas").find(|d| d.u64("identifier") == Some(data_id))?.clone();
        let (name, bytes) = src.data(data_id)?;
        let bytes = bytes.to_vec();
        let pm = self.package_metadata()?;
        let new_id = pm.msgs("datas").filter_map(|d| d.u64("identifier")).max().unwrap_or(0) + 1;
        let file = name.strip_prefix("Data/").unwrap_or(name);
        let (stem, ext) = file.rsplit_once('.').unwrap_or((file, "dat"));
        let stem = stem.rsplit_once('-').map(|(a, _)| a).unwrap_or(stem);
        let stored = format!("{stem}-{new_id}.{ext}");
        let mut info = info;
        info.set_u64("identifier", new_id);
        info.set_str("file_name", &stored);
        info.remove("document_resource_locator");
        self.package_metadata_mut()?.push_msg("datas", info);
        self.add_entry(Entry { name: format!("Data/{stored}"), data: EntryData::Raw(std::sync::Arc::new(bytes)) });
        Some(new_id)
    }

    /// Point a (pasted) TSD.ImageArchive at picture data `data_id` of `w`×`h` pixels.
    pub fn set_image_data(&mut self, image: ObjId, data_id: u64, w: f32, h: f32) {
        let Some(file) = self.file_of(image).map(str::to_string) else { return };
        if let Some(a) = self.archive_mut(image) {
            let m = a.obj_mut();
            m.set_ref("data", data_id);
            if m.has("thumbnailData") {
                m.set_ref("thumbnailData", data_id);
            }
            m.remove("adjustedImageData");
            m.remove("thumbnailAdjustedImageData");
            m.remove("traced_path");
            for f in ["naturalSize", "originalSize"] {
                let s = m.msg_mut_or_create(f);
                s.set_f32("width", w);
                s.set_f32("height", h);
            }
            a.set_data_references(0, &[data_id]);
        }
        self.add_data_reference(&file, data_id, image);
    }
}

