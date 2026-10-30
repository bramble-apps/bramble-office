//! Inserting a table by "donor transplant": every object of a table Keynote made (the drawable,
//! its model and helpers in the CalculationEngine component, the cell tiles and data lists in
//! their own `Index/Tables/*.iwa` components, and the table's formula owners in the calculation
//! engine) is copied into this document with fresh object ids and fresh UUIDs. Shared objects
//! (cell/text styles, the table style preset, the stylesheet) are mapped onto this document's
//! own theme objects.
//!
//! What Keynote keeps for one table (worked out from R2-table-values.key, made by hand in
//! Keynote 15.4, and 05-table.key vs 00-base.key):
//! * `TST.TableInfoArchive` (the drawable) + captions, model, summary model, category order,
//!   uid maps, filter sets, group-by… all in `Index/CalculationEngine.iwa`. The slide's
//!   component declares the TableInfo as an external reference; the CalculationEngine component
//!   declares every table component, every theme style it uses, and (weakly) the slide and the
//!   Document's table style preset.
//! * Cell tiles, header buckets and data lists: one component each (`Index/Tables/…`).
//! * All the table's UUIDs (table_id, hidden states, group-by, formula owners…) share their
//!   upper 96 bits and differ in the low 32 bits ("owner uuid = table uuid + n"); the remap
//!   keeps that structure.
//! * Formula owners: 11 `TSCE.FormulaOwnerDependenciesArchive` (one per owner kind, uuid =
//!   table uuid + n) with an entry each in the engine's owner id map, plus their cell record
//!   tiles; `number_of_formulas` is recounted (the cell records of all formula owners).
//! * The header name manager gets a per-table entry whose precedent is a new cell (column k,
//!   row 1) of the header-name owner (uuid 666); that owner's dependencies record the cell.

use std::collections::{HashMap, HashSet};

use crate::keyfile::{Entry, EntryData, KeyFile};
use crate::msg::{Msg, ObjId, Val};
use crate::schema::{schema, Kind};
use crate::wire::{packed_varints, write_varint};
use crate::Error;


pub type Uuid16 = [u8; 16];

fn bad(s: impl Into<String>) -> Error {
    Error::Invalid(s.into())
}

pub fn uuid_of_tsp(m: &Msg) -> Uuid16 {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&m.u64("lower").unwrap_or(0).to_le_bytes());
    b[8..].copy_from_slice(&m.u64("upper").unwrap_or(0).to_le_bytes());
    b
}

pub fn uuid_of_cf(m: &Msg) -> Uuid16 {
    let mut b = [0u8; 16];
    if let Some(raw) = m.bytes("uuid_bytes").filter(|r| r.len() == 16) {
        b.copy_from_slice(raw);
        return b;
    }
    for (i, w) in ["uuid_w0", "uuid_w1", "uuid_w2", "uuid_w3"].iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&(m.u64(w).unwrap_or(0) as u32).to_le_bytes());
    }
    b
}

pub fn cf_msg(b: &Uuid16) -> Msg {
    let mut m = Msg::new("TSP.CFUUIDArchive");
    for (i, w) in ["uuid_w0", "uuid_w1", "uuid_w2", "uuid_w3"].iter().enumerate() {
        m.set_u64(w, u64::from(u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())));
    }
    m
}

pub fn uuid_of_str(s: &str) -> Option<Uuid16> {
    (s.len() == 36).then(|| uuid::Uuid::parse_str(s).ok().map(|u| *u.as_bytes())).flatten()
}

/// The "family" of a uuid: everything but the low 32 bits (None for small constants like 666).
pub fn family(b: &Uuid16) -> Option<[u8; 12]> {
    let f: [u8; 12] = b[4..].try_into().unwrap();
    (f != [0; 12]).then_some(f)
}

/// Fresh uuids that keep the donor's families: equal uuids stay equal, "base + n" stays "new
/// base + n".
#[derive(Default)]
struct UuidMap {
    families: HashMap<[u8; 12], [u8; 12]>,
}

impl UuidMap {
    fn map(&mut self, b: &Uuid16) -> Uuid16 {
        let Some(f) = family(b) else { return *b };
        let nf = *self.families.entry(f).or_insert_with(|| uuid::Uuid::new_v4().as_bytes()[4..].try_into().unwrap());
        let mut out = *b;
        out[4..].copy_from_slice(&nf);
        out
    }

    /// Remap every TSP.UUID, TSP.CFUUIDArchive and uuid-looking string in a message tree.
    fn remap(&mut self, m: &mut Msg) {
        match m.type_name() {
            "TSP.UUID" => {
                let b = uuid_of_tsp(m);
                let n = self.map(&b);
                if n != b {
                    m.set_u64("lower", u64::from_le_bytes(n[..8].try_into().unwrap()));
                    m.set_u64("upper", u64::from_le_bytes(n[8..].try_into().unwrap()));
                }
                return;
            }
            "TSP.CFUUIDArchive" => {
                let b = uuid_of_cf(m);
                let n = self.map(&b);
                if n != b {
                    if m.has("uuid_bytes") {
                        m.set_bytes("uuid_bytes", &n);
                    } else {
                        *m = cf_msg(&n);
                    }
                }
                return;
            }
            _ => {}
        }
        let tdef = m.ty.map(|t| schema().ty(t));
        for f in &mut m.fields {
            match &mut f.val {
                Val::Msg(s) => self.remap(s),
                Val::Bytes(raw) => {
                    let is_str = tdef.and_then(|t| t.fields.get(&f.num)).is_some_and(|d| d.kind == Kind::String);
                    if let Some(b) = is_str.then(|| std::str::from_utf8(raw).ok().and_then(uuid_of_str)).flatten() {
                        let n = self.map(&b);
                        let s = uuid::Uuid::from_bytes(n).to_string().to_uppercase();
                        *raw = s.into_bytes();
                    }
                }
                _ => {}
            }
        }
    }
}

/// Re-point the calculation engine's internal owner numbers (TSCE messages only).
fn remap_owner_ids(m: &mut Msg, map: &HashMap<u64, u64>, missing: &mut HashSet<u64>) {
    const NAMES: [&str; 5] = ["internal_formula_owner_id", "internal_owner_id", "internal_owner_id_for_edge", "to_owner_id", "owner_id"];
    let tdef = m.ty.map(|t| schema().ty(t));
    let tsce = m.type_name().starts_with("TSCE.");
    for f in &mut m.fields {
        if let Val::Msg(s) = &mut f.val {
            remap_owner_ids(s, map, missing);
            continue;
        }
        let Some(d) = tdef.and_then(|t| t.fields.get(&f.num)) else { continue };
        if !tsce || !NAMES.contains(&d.name.as_str()) || d.kind == Kind::Message {
            continue;
        }
        let mut one = |v: u64| match map.get(&v) {
            Some(n) => *n,
            None => {
                missing.insert(v);
                v
            }
        };
        match &mut f.val {
            Val::Varint(v) => *v = one(*v),
            Val::Bytes(b) => {
                if let Ok(vals) = packed_varints(b) {
                    let mut out = Vec::new();
                    for v in vals {
                        write_varint(&mut out, one(v));
                    }
                    *b = out;
                }
            }
            _ => {}
        }
    }
}

/// Every object reachable from `root` (payload references + archive object_references).
fn reachable(k: &KeyFile, roots: &[ObjId], follow: impl Fn(ObjId) -> bool) -> (Vec<ObjId>, Vec<ObjId>) {
    let mut owned = Vec::new();
    let mut seen = HashSet::new();
    let mut external = Vec::new();
    let mut todo: Vec<ObjId> = roots.iter().rev().copied().collect();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        if !follow(id) {
            external.push(id);
            continue;
        }
        let Some(a) = k.archive(id) else {
            external.push(id);
            continue;
        };
        owned.push(id);
        let mut refs = Vec::new();
        for (i, p) in a.payloads.iter().enumerate() {
            p.collect_refs(&mut refs);
            refs.extend(a.object_references(i));
        }
        // the drawable's parent (the slide) is mapped, never copied
        let parent = a.obj().find_super("TSD.DrawableArchive").and_then(|d| d.ref_id("parent"));
        for r in refs.into_iter().rev() {
            if Some(r) == parent {
                if !external.contains(&r) {
                    external.push(r);
                }
                seen.insert(r);
                continue;
            }
            todo.push(r);
        }
    }
    (owned, external)
}

fn tracker_fods(k: &KeyFile, ce: ObjId) -> Vec<ObjId> {
    k.obj(ce).and_then(|m| m.msg("dependency_tracker")).map(|t| t.ref_ids("formula_owner_dependencies")).unwrap_or_default()
}

fn ce_root(k: &KeyFile) -> Option<ObjId> {
    k.first_of("TSCE.CalculationEngineArchive").map(|(i, _)| i)
}

/// Cell record tiles of `fod` (among `tiles`) left with no records are removed from it and
/// added to `gone`, as Numbers' re-save does (N13: the tracking owner's emptied tiles).
fn drop_empty_tiles(k: &mut KeyFile, fod: ObjId, tiles: &[ObjId], gone: &mut Vec<ObjId>) {
    let empty: Vec<ObjId> = tiles.iter().copied().filter(|t| k.obj(*t).is_some_and(|m| m.count("cell_records") == 0)).collect();
    if empty.is_empty() {
        return;
    }
    if let Some(a) = k.archive_mut(fod) {
        if let Some(t) = a.obj_mut().msg_mut("tiled_cell_dependencies") {
            t.retain_msgs("cell_record_tiles", |r| !r.u64("identifier").is_some_and(|i| empty.contains(&i)));
        }
        a.refresh_object_references();
    }
    gone.extend(empty);
}

/// The header-name owner's dependencies (formula owner uid = the header name manager's owner).
fn header_owner_fod(k: &KeyFile, ce: ObjId) -> Option<ObjId> {
    let hnm = k.obj(ce)?.ref_id("header_name_manager")?;
    let owner = uuid_of_tsp(k.obj(hnm)?.msg("owner_uid")?);
    let tracker = k.obj(ce)?.msg("dependency_tracker")?;
    tracker.ref_ids("formula_owner_dependencies").into_iter().find(|f| k.obj(*f).and_then(|m| m.msg("formula_owner_uid")).map(uuid_of_tsp) == Some(owner))
}

fn fod_tiles(k: &KeyFile, fod: ObjId) -> Vec<ObjId> {
    k.obj(fod).and_then(|m| m.msg("tiled_cell_dependencies")).map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default()
}

impl KeyFile {
    /// Existing objects [`KeyFile::paste_table`] modifies (besides the slide and the package
    /// metadata) — for undo snapshots.
    pub fn table_insert_touches(&self) -> Vec<ObjId> {
        let Some(ce) = ce_root(self) else { return Vec::new() };
        let mut v = vec![ce];
        if let Some(h) = self.obj(ce).and_then(|m| m.ref_id("header_name_manager")) {
            v.push(h);
        }
        if let Some(f) = header_owner_fod(self, ce) {
            v.push(f);
            v.extend(fod_tiles(self, f));
        }
        v
    }

    /// Copy the table `donor_info` (a TST.TableInfoArchive of `donor`) onto `slide`.
    /// Returns the new TableInfo id.
    pub fn paste_table(&mut self, donor: &KeyFile, donor_info: ObjId, slide: ObjId) -> Result<ObjId, Error> {
        self.paste_table_with(donor, donor_info, slide, &mut HashMap::new())
    }

    /// [`KeyFile::paste_table`] sharing its uuid family mapping (old family -> new) with other
    /// pastes: tables copied together (Duplicate Sheet) keep pointing at each other's copies,
    /// since formulas name another table by its formula owner uid (N9: Numbers' Duplicate
    /// rewrites them to the copies' owners).
    pub fn paste_table_with(&mut self, donor: &KeyFile, donor_info: ObjId, slide: ObjId, families: &mut HashMap<[u8; 12], [u8; 12]>) -> Result<ObjId, Error> {
        let mut uuids = UuidMap { families: std::mem::take(families) };
        let r = self.paste_table_inner(donor, donor_info, slide, &mut uuids);
        *families = uuids.families;
        r
    }

    fn paste_table_inner(&mut self, donor: &KeyFile, donor_info: ObjId, slide: ObjId, uuids: &mut UuidMap) -> Result<ObjId, Error> {
        let d_ce = ce_root(donor).ok_or(bad("donor has no calculation engine"))?;
        let ce = ce_root(self).ok_or(bad("this document has no calculation engine"))?;
        let d_ce_file = donor.file_of(d_ce).unwrap().to_string();
        let ce_file = self.file_of(ce).unwrap().to_string();
        let slide_file = self.file_of(slide).ok_or(bad("no slide"))?.to_string();
        let d_model_id = donor.obj(donor_info).and_then(|m| m.ref_id("tableModel")).ok_or(bad("donor table has no model"))?;
        let d_table_uuid = donor.obj(d_model_id).and_then(|m| m.str("table_id")).and_then(uuid_of_str).ok_or(bad("donor table has no table_id"))?;
        let d_family = family(&d_table_uuid);

        // 1. the table's own objects: everything reachable inside the engine file and the
        // Tables/ components; the rest (styles, preset, stylesheet, slide) is shared
        let (mut owned, external) = reachable(donor, &[donor_info], |id| {
            let f = donor.file_of(id).unwrap_or("");
            id != d_ce && (f == d_ce_file || f.starts_with("Index/Tables/"))
        });
        if let Some(other) = owned.iter().find(|i| **i != donor_info && donor.type_name(**i) == "TST.TableInfoArchive") {
            return Err(bad(format!("donor table reaches another table #{other}")));
        }
        // its formula owners (uuid family of the table) and their cell record tiles
        let d_tracker = donor.obj(d_ce).and_then(|m| m.msg("dependency_tracker")).ok_or(bad("no dependency tracker"))?;
        let d_fods: Vec<ObjId> = d_tracker
            .ref_ids("formula_owner_dependencies")
            .into_iter()
            .filter(|f| donor.obj(*f).and_then(|m| m.msg("formula_owner_uid")).map(|u| family(&uuid_of_tsp(u))) == Some(d_family))
            .collect();
        let owned_set: HashSet<ObjId> = owned.iter().copied().collect();
        let (fod_objs, fod_ext) = reachable(donor, &d_fods, |id| donor.file_of(id) == Some(d_ce_file.as_str()) && id != d_ce && !owned_set.contains(&id));
        if let Some(x) = fod_ext.iter().find(|x| !owned_set.contains(x)) {
            return Err(bad(format!("formula owner refers to #{x} outside the table")));
        }
        owned.extend(fod_objs.iter().copied());

        // 2. map shared objects onto this document
        let mut ext_map: HashMap<ObjId, ObjId> = HashMap::new();
        let d_slide = donor.obj(donor_info).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|d| d.ref_id("parent"));
        if let Some(p) = d_slide {
            ext_map.insert(p, slide);
        }
        self.map_table_styles(donor, d_model_id, &mut ext_map)?;
        let rest: Vec<ObjId> = external.iter().copied().filter(|e| !ext_map.contains_key(e)).collect();
        ext_map.extend(self.map_external(donor, &rest)?);

        // 3. engine-internal owner numbers
        let t_tracker = self.obj(ce).and_then(|m| m.msg("dependency_tracker")).ok_or(bad("no dependency tracker"))?;
        let mut next_owner = t_tracker
            .msg("owner_id_map")
            .map(|m| m.msgs("map_entry").filter_map(|e| e.u64("internal_owner_id")).max().unwrap_or(0))
            .unwrap_or(0)
            .max(t_tracker.ref_ids("formula_owner_dependencies").iter().filter_map(|f| self.obj(*f)?.u64("internal_formula_owner_id")).max().unwrap_or(0));
        let t_owners: HashMap<Uuid16, u64> = t_tracker
            .msg("owner_id_map")
            .map(|m| m.msgs("map_entry").filter_map(|e| Some((uuid_of_cf(e.msg("owner_id")?), e.u64("internal_owner_id")?))).collect())
            .unwrap_or_default();
        let mut owner_map: HashMap<u64, u64> = HashMap::new();
        let mut new_owner_entries: Vec<(u64, Uuid16)> = Vec::new();
        for e in d_tracker.msg("owner_id_map").map(|m| m.msgs("map_entry").collect::<Vec<_>>()).unwrap_or_default() {
            let (Some(i), Some(u)) = (e.u64("internal_owner_id"), e.msg("owner_id").map(uuid_of_cf)) else { continue };
            if family(&u) == d_family {
                next_owner += 1;
                owner_map.insert(i, next_owner);
                new_owner_entries.push((next_owner, uuids.map(&u)));
            } else if let Some(t) = t_owners.get(&u) {
                owner_map.insert(i, *t); // e.g. the header-name owner (uuid 666)
            }
        }

        // 4. copy with fresh ids
        let new_ids = self.alloc_ids(owned.len() as u64);
        let idmap: HashMap<ObjId, ObjId> = owned.iter().copied().zip(new_ids.iter().copied()).collect();
        let mut full = idmap.clone();
        full.extend(ext_map.iter().map(|(a, b)| (*a, *b)));
        let mut missing_owner = HashSet::new();
        let mut unmapped = Vec::new();
        let mut by_file: Vec<(String, Vec<crate::Archive>)> = Vec::new();
        for old in &owned {
            let mut a = donor.archive(*old).unwrap().clone();
            a.info.set_u64("identifier", idmap[old]);
            for i in 0..a.payloads.len() {
                let refs: Vec<ObjId> = a
                    .object_references(i)
                    .into_iter()
                    .map(|r| {
                        full.get(&r).copied().unwrap_or_else(|| {
                            unmapped.push(r);
                            r
                        })
                    })
                    .collect();
                if !refs.is_empty() {
                    a.set_object_references(i, &refs);
                }
                a.payloads[i].for_each_ref_mut(&mut |r| match full.get(r) {
                    Some(n) => *r = *n,
                    None => unmapped.push(*r),
                });
                if !a.data_references(i).is_empty() {
                    return Err(bad("donor table uses data files"));
                }
                uuids.remap(&mut a.payloads[i]);
                remap_owner_ids(&mut a.payloads[i], &owner_map, &mut missing_owner);
            }
            let f = donor.file_of(*old).unwrap().to_string();
            match by_file.iter_mut().find(|(n, _)| *n == f) {
                Some((_, v)) => v.push(a),
                None => by_file.push((f, vec![a])),
            }
        }
        // stylesheet references are left out of object_references, so they may stay unmapped
        unmapped.retain(|r| donor.type_name(*r) != "TSS.StylesheetArchive");
        if let Some(r) = unmapped.first() {
            return Err(bad(format!("unmapped reference #{r} ({})", donor.type_name(*r))));
        }
        if let Some(o) = missing_owner.iter().next() {
            return Err(bad(format!("formula owner {o} can't be mapped into this document")));
        }

        // 5. place the copies: engine objects into this engine's file, each Tables/ file as a
        // new component
        let d_uuid_ids: HashSet<ObjId> = donor
            .component_for_file(&d_ce_file)
            .and_then(|c| donor.component(c))
            .map(|c| c.msgs("object_uuid_map_entries").filter_map(|e| e.u64("identifier")).collect())
            .unwrap_or_default();
        let mut new_files = Vec::new();
        let mut new_comps = Vec::new();
        let ce_pos = self.entries.iter().position(|e| e.name == ce_file).unwrap();
        let mut insert_at = self.entries.iter().rposition(|e| e.name.starts_with("Index/Tables/")).unwrap_or(ce_pos) + 1;
        for (f, archives) in by_file {
            if f == d_ce_file {
                self.iwa_mut(&ce_file).unwrap().archives.extend(archives);
                continue;
            }
            let d_comp_id = donor.component_for_file(&f).ok_or(bad(format!("{f} is not a component")))?;
            let d_comp = donor.component(d_comp_id).unwrap().clone();
            let new_root = *idmap.get(&d_comp_id).ok_or(bad(format!("component root of {f} not copied")))?;
            let base = f.trim_start_matches("Index/Tables/").trim_end_matches(".iwa");
            let kind = base.split('-').next().unwrap_or(base);
            let mut locator = format!("Tables/{kind}-{new_root}");
            let mut n = 2;
            while self.entry(&format!("Index/{locator}.iwa")).is_some() {
                locator = format!("Tables/{kind}-{new_root}-{n}");
                n += 1;
            }
            let mut comp = d_comp;
            comp.set_u64("identifier", new_root);
            for fld in ["locator", "preferred_locator"] {
                if comp.str(fld).is_some() {
                    comp.set_str(fld, &locator);
                }
            }
            comp.remove("external_references");
            comp.remove("data_references");
            for e in comp.msgs_mut("object_uuid_map_entries") {
                let id = e.u64("identifier").unwrap_or(0);
                e.set_u64("identifier", *idmap.get(&id).unwrap_or(&id));
                crate::objects::new_uuid(e.msg_mut_or_create("uuid"));
            }
            new_comps.push(comp);
            let name = format!("Index/{locator}.iwa");
            self.entries.insert(insert_at, Entry { name: name.clone(), data: EntryData::Iwa(crate::IwaFile { archives }) });
            insert_at += 1;
            new_files.push(name);
        }
        self.reindex(); // entry positions shifted
        let pm = self.package_metadata_mut().unwrap();
        for c in new_comps {
            pm.push_msg("components", c);
        }
        for old in &owned {
            if d_uuid_ids.contains(old) {
                self.add_uuid_entry(&ce_file, idmap[old]);
            }
        }

        // 6. register the formula owners in the engine
        let new_fods: Vec<ObjId> = d_fods.iter().map(|f| idmap[f]).collect();
        {
            let a = self.archive_mut(ce).unwrap();
            let tr = a.obj_mut().msg_mut("dependency_tracker").unwrap();
            // Keynote drops the legacy inline owner infos once a document has tables (each one
            // duplicates a formula_owner_dependencies archive)
            tr.remove("formula_owner_info");
            let map = tr.msg_mut_or_create("owner_id_map");
            for (i, u) in &new_owner_entries {
                let mut e = Msg::new("TSCE.OwnerIDMapArchive.OwnerIDMapArchiveEntry");
                e.set_u64("internal_owner_id", *i);
                e.push_msg("owner_id", cf_msg(u));
                map.push_msg("map_entry", e);
            }
            let at = tr.count("formula_owner_dependencies");
            for (j, f) in new_fods.iter().enumerate() {
                tr.insert_msg("formula_owner_dependencies", at + j, crate::msg::reference(*f));
            }
            let mut refs = a.object_references(0);
            refs.extend(new_fods.iter().copied());
            a.set_object_references(0, &refs);
        }
        self.add_header_name(donor, d_ce, ce, &d_table_uuid, uuids)?;

        // 7. on the slide
        let info = idmap[&donor_info];
        self.attach_to_slide(slide, info);
        let file_comps = self.file_components();
        self.ensure_external_refs_in(&slide_file, &file_comps);
        for f in &new_files {
            self.ensure_external_refs_in(f, &file_comps);
        }
        self.ensure_external_refs_in(&ce_file, &file_comps);
        // payload references Keynote leaves out of object_references (the parent slide, the
        // Document's style preset) are still declared, as weak references
        let ce_comp = self.component_for_file(&ce_file).ok_or(bad("calculation engine is not a component"))?;
        let comps = file_comps;
        let mut weak: Vec<(ObjId, Option<ObjId>)> = Vec::new();
        let mut seen: HashSet<(ObjId, Option<ObjId>)> = HashSet::new();
        for id in idmap.values() {
            if self.file_of(*id) != Some(ce_file.as_str()) {
                continue;
            }
            let mut refs = Vec::new();
            self.obj(*id).unwrap().collect_refs(&mut refs);
            for r in refs {
                let Some(c) = self.file_of(r).and_then(|f| comps.get(f)).copied() else { continue };
                if c != ce_comp && self.type_name(r) != "TSS.StylesheetArchive" {
                    let key = (c, (r != c).then_some(r));
                    if seen.insert(key) {
                        weak.push(key);
                    }
                }
            }
        }
        let d_comp = donor.component_for_file(&d_ce_file).and_then(|c| donor.component(c)).cloned();
        let comp = self.component_mut(ce_comp).unwrap();
        let have: HashSet<(ObjId, Option<ObjId>)> =
            comp.msgs("external_references").map(|x| (x.u64("component_identifier").unwrap_or(0), x.u64("object_identifier"))).collect();
        for (c, o) in weak {
            if have.contains(&(c, o)) {
                continue;
            }
            let mut xr = Msg::new("TSP.ComponentExternalReference");
            xr.set_u64("component_identifier", c);
            if let Some(o) = o {
                xr.set_u64("object_identifier", o);
            }
            xr.set_bool("is_weak", true);
            comp.push_msg("external_references", xr);
        }
        // the engine now uses the format of the donor's (formula owner archives, no inline
        // infos): same component versions and feature flags, and the package read version
        let mut read = comp.u64s("document_read_version");
        if let Some(d) = &d_comp {
            for f in ["document_read_version", "document_write_version"] {
                let (dv, tv) = (d.u64s(f), comp.u64s(f));
                if dv > tv {
                    comp.set_u64s_packed(f, &dv);
                }
            }
            read = comp.u64s("document_read_version");
            for fi in d.msgs("feature_infos") {
                if !comp.msgs("feature_infos").any(|t| t.str("identifier") == fi.str("identifier")) {
                    comp.push_msg("feature_infos", fi.clone());
                }
            }
        }
        let pm = self.package_metadata_mut().unwrap();
        if pm.u64s("read_version") < read {
            pm.set_u64s_packed("read_version", &read);
        }
        // the copies' fresh row / column uids in the order Numbers keeps them (N3)
        let maps: Vec<ObjId> = idmap.values().copied().filter(|id| self.type_name(*id) == "TST.ColumnRowUIDMapArchive").collect();
        for m in maps {
            if let Some(m) = self.obj_mut(m) {
                sort_uid_map(m);
            }
        }
        self.recount_formulas();
        Ok(info)
    }

    /// Map the donor table's styles by role: the donor model's style preset network → this
    /// document's preset with the same preset_id (so the table gets this theme's look).
    fn map_table_styles(&self, donor: &KeyFile, d_model: ObjId, out: &mut HashMap<ObjId, ObjId>) -> Result<(), Error> {
        let network = |k: &KeyFile, preset: ObjId| k.obj(preset).and_then(|p| p.ref_id("style_network")).and_then(|n| k.obj(n).cloned());
        let Some(d_preset) = donor.obj(d_model).and_then(|m| m.ref_id("table_style_preset")) else { return Ok(()) };
        let Some(d_net) = network(donor, d_preset) else { return Ok(()) };
        let want = d_net.u64("preset_id");
        let presets: Vec<(ObjId, Msg)> = self.objects_of("TST.TableStylePresetArchive").filter_map(|(i, _)| Some((i, network(self, i)?))).collect();
        let Some((t_preset, t_net)) = presets.iter().find(|(_, n)| n.u64("preset_id") == want).or(presets.first()) else {
            return Err(bad("this theme has no table styles"));
        };
        out.insert(d_preset, *t_preset);
        let tdef = d_net.ty.map(|t| schema().ty(t)).unwrap();
        for f in &d_net.fields {
            let (Val::Msg(r), Some(d)) = (&f.val, tdef.fields.get(&f.num)) else { continue };
            let (Some(src), Some(dst)) = (r.u64("identifier"), t_net.ref_id(&d.name)) else { continue };
            out.entry(src).or_insert(dst);
        }
        Ok(())
    }

    /// Give the new table its entry in the header name manager, with a fresh precedent cell of
    /// the header-name owner.
    fn add_header_name(&mut self, donor: &KeyFile, d_ce: ObjId, ce: ObjId, d_table: &Uuid16, uuids: &mut UuidMap) -> Result<(), Error> {
        let Some(d_hnm) = donor.obj(d_ce).and_then(|m| m.ref_id("header_name_manager")) else { return Ok(()) };
        let Some(per) = donor.obj(d_hnm).and_then(|h| h.msgs("per_tables").find(|p| p.msg("table_uid").map(uuid_of_tsp).as_ref() == Some(d_table)).cloned()) else {
            return Ok(());
        };
        let Some(hnm) = self.obj(ce).and_then(|m| m.ref_id("header_name_manager")) else { return Ok(()) };
        let d_col = per.msg("per_table_precedent").and_then(|c| c.u64("column"));
        let d_row = per.msg("per_table_precedent").and_then(|c| c.u64("row")).unwrap_or(1);
        // donor's record of that precedent cell in the header-name owner
        let d_record = header_owner_fod(donor, d_ce).and_then(|f| {
            let is = |r: &&Msg| r.u64("column") == d_col && r.u64("row") == Some(d_row);
            donor.obj(f)?.msg("cell_dependencies")?.msgs("cell_record").find(is).cloned()
        });
        // a column no other precedent / name fragment uses
        let fod = header_owner_fod(self, ce);
        let mut used: Vec<u64> = Vec::new();
        let h = self.obj(hnm).unwrap();
        used.extend(h.msgs("per_tables").filter_map(|p| p.msg("per_table_precedent")?.u64("column")));
        for t in h.ref_ids("name_frag_tiles") {
            if let Some(tm) = self.obj(t) {
                used.extend(tm.msgs("name_frag_entries").filter_map(|e| e.msg("name_precedent")?.u64("column")));
            }
        }
        if let Some(f) = fod {
            let m = self.obj(f).unwrap();
            used.extend(m.msg("cell_dependencies").map(|c| c.msgs("cell_record").filter_map(|r| r.u64("column")).collect::<Vec<_>>()).unwrap_or_default());
            for t in fod_tiles(self, f) {
                used.extend(self.obj(t).map(|m| m.msgs("cell_records").filter_map(|r| r.u64("column")).collect::<Vec<_>>()).unwrap_or_default());
            }
        }
        let col = used.into_iter().max().map(|m| m + 1).unwrap_or(1).max(1);
        let mut per = per;
        uuids.remap(&mut per);
        // the header uid lists in uid order, as Numbers keeps them (N11)
        for f in ["header_row_uids", "header_column_uids"] {
            let mut l: Vec<Msg> = per.msgs(f).cloned().collect();
            if l.windows(2).any(|w| (w[0].u64("upper"), w[0].u64("lower")) > (w[1].u64("upper"), w[1].u64("lower"))) {
                l.sort_by_key(|u| (u.u64("upper").unwrap_or(0), u.u64("lower").unwrap_or(0)));
                per.remove(f);
                for u in l {
                    per.push_msg(f, u);
                }
            }
        }
        per.msg_mut_or_create("per_table_precedent").set_u64("column", col);
        {
            let h = self.obj_mut(hnm).unwrap();
            let at = h.count("per_tables");
            h.insert_msg("per_tables", at, per);
        }
        if let (Some(f), Some(mut rec)) = (fod, d_record) {
            rec.set_u64("column", col);
            if let Some(cd) = self.obj_mut(f).and_then(|m| m.msg_mut("cell_dependencies")) {
                cd.push_msg("cell_record", rec.clone());
            }
            let tile = fod_tiles(self, f)
                .into_iter()
                .find(|t| self.obj(*t).is_some_and(|m| m.u64("tile_column_begin").unwrap_or(0) == 0 && m.u64("tile_row_begin").unwrap_or(0) == 0));
            if let Some(t) = tile {
                let mut r2 = Msg::new("TSCE.CellRecordExpandedArchive");
                r2.fields = rec.fields;
                self.obj_mut(t).unwrap().push_msg("cell_records", r2);
            }
        }
        Ok(())
    }
}

/// A TST.ColumnRowUIDMapArchive with its sorted uid lists in order: ascending by (upper,
/// lower) as unsigned 64-bit, the index maps rebuilt to match (Numbers re-sorts lists left in
/// a donor's order, N3). `*_index_for_uid[i]` is the row / column of the i-th sorted uid,
/// `*_uid_for_index[r]` the sorted position of row / column r's uid.
pub fn sort_uid_map(m: &mut Msg) {
    for (uids, index_for, uid_for) in [("sorted_column_uids", "column_index_for_uid", "column_uid_for_index"), ("sorted_row_uids", "row_index_for_uid", "row_uid_for_index")] {
        let list: Vec<Msg> = m.msgs(uids).cloned().collect();
        let idx = m.u64s(index_for);
        if list.is_empty() || list.len() != idx.len() {
            continue;
        }
        let key = |u: &Msg| (u.u64("upper").unwrap_or(0), u.u64("lower").unwrap_or(0));
        // already in order: leave it (and its encoding) alone
        if list.windows(2).all(|w| key(&w[0]) <= key(&w[1])) {
            continue;
        }
        let packed = |m: &Msg, name: &str| m.field_def(name).is_some_and(|d| m.fields.iter().any(|f| f.num == d.num && matches!(f.val, Val::Bytes(_))));
        let (p1, p2) = (packed(m, index_for), packed(m, uid_for));
        let mut pairs: Vec<(Msg, u64)> = list.into_iter().zip(idx).collect();
        pairs.sort_by_key(|(u, _)| key(u));
        let mut uid_for_index = vec![0u64; pairs.len()];
        for (pos, (_, at)) in pairs.iter().enumerate() {
            if let Some(slot) = uid_for_index.get_mut(*at as usize) {
                *slot = pos as u64;
            }
        }
        m.remove(uids);
        for (u, _) in &pairs {
            m.push_msg(uids, u.clone());
        }
        let index_for_uid: Vec<u64> = pairs.iter().map(|p| p.1).collect();
        if p1 { m.set_u64s_packed(index_for, &index_for_uid) } else { m.set_u64s(index_for, &index_for_uid) }
        if p2 { m.set_u64s_packed(uid_for, &uid_for_index) } else { m.set_u64s(uid_for, &uid_for_index) }
    }
}

impl KeyFile {
    /// dependency_tracker.number_of_formulas is the number of cell records of all the formula
    /// owners (holds in every Keynote and Numbers file checked, N3): set it so.
    pub fn recount_formulas(&mut self) {
        let Some(ce) = ce_root(self) else { return };
        let fods = self.obj(ce).and_then(|m| m.msg("dependency_tracker")).map(|t| t.ref_ids("formula_owner_dependencies")).unwrap_or_default();
        let n: usize = fods.iter().filter_map(|f| self.obj(*f)).map(|m| m.msg("cell_dependencies").map(|c| c.count("cell_record")).unwrap_or(0)).sum();
        if let Some(t) = self.obj_mut(ce).and_then(|m| m.msg_mut("dependency_tracker")) {
            t.set_u64("number_of_formulas", n as u64);
        }
    }

    /// After tables were copied together with [`KeyFile::paste_table_with`] (Duplicate Sheet):
    /// the copies' engine records that still name an original table's internal owner (a
    /// formula's edge into another copied table) are pointed at that table's copy, as Numbers'
    /// own duplicate does (N13: internal_owner_id_for_edge [21] -> [43]).
    pub fn relink_pasted_tables(&mut self, families: &HashMap<[u8; 12], [u8; 12]>) {
        let Some(ce) = ce_root(self) else { return };
        let owners: HashMap<Uuid16, u64> = self
            .obj(ce)
            .and_then(|m| m.msg("dependency_tracker"))
            .and_then(|t| t.msg("owner_id_map"))
            .map(|m| m.msgs("map_entry").filter_map(|e| Some((uuid_of_cf(e.msg("owner_id")?), e.u64("internal_owner_id")?))).collect())
            .unwrap_or_default();
        let mut map: HashMap<u64, u64> = HashMap::new();
        for (u, i) in &owners {
            let Some(nf) = family(u).and_then(|f| families.get(&f)) else { continue };
            let mut n = *u;
            n[4..].copy_from_slice(nf);
            if let Some(j) = owners.get(&n) {
                map.insert(*i, *j);
            }
        }
        if map.is_empty() {
            return;
        }
        let copies: HashSet<[u8; 12]> = families.values().copied().collect();
        for f in tracker_fods(self, ce) {
            let Some(m) = self.obj(f) else { continue };
            if !m.msg("formula_owner_uid").and_then(|u| family(&uuid_of_tsp(u))).is_some_and(|fam| copies.contains(&fam)) {
                continue;
            }
            let mut objs = vec![f];
            objs.extend(m.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default());
            objs.extend(m.msg("tiled_range_dependencies").map(|t| t.ref_ids("range_precedents_tile")).unwrap_or_default());
            for o in objs {
                if let Some(a) = self.archive_mut(o) {
                    for p in &mut a.payloads {
                        remap_owner_ids(p, &map, &mut HashSet::new());
                    }
                }
            }
        }
    }

    /// The calculation engine forgets a deleted table, as Numbers' own delete does (N3): its
    /// formula owners (the table's uuid family) with their cell record / range tiles, their
    /// owner map entries, its header name entry and that entry's precedent record, tracked
    /// references into it; then the formula count. Returns the archives removed.
    pub fn remove_table_engine(&mut self, info: ObjId) -> Vec<ObjId> {
        let mut gone = Vec::new();
        let Some(ce) = ce_root(self) else { return gone };
        let Some(uuid) = self.obj(info).and_then(|m| m.ref_id("tableModel")).and_then(|t| self.obj(t)).and_then(|m| m.str("table_id")).and_then(uuid_of_str) else { return gone };
        let Some(fam) = family(&uuid) else { return gone };
        let tracker = self.obj(ce).and_then(|m| m.msg("dependency_tracker"));
        let fods: Vec<ObjId> = tracker
            .map(|t| t.ref_ids("formula_owner_dependencies"))
            .unwrap_or_default()
            .into_iter()
            .filter(|f| self.obj(*f).and_then(|m| m.msg("formula_owner_uid")).map(|u| family(&uuid_of_tsp(u))) == Some(Some(fam)))
            // Numbers keeps the table's kind-1 owner (N3 reference)
            .filter(|f| self.obj(*f).and_then(|m| m.u64("owner_kind")) != Some(1))
            .collect();
        let internal: Vec<u64> = fods.iter().filter_map(|f| self.obj(*f)?.u64("internal_formula_owner_id")).collect();
        // the kind-1 owner stays, but no longer names the table nor keeps record tiles (else
        // Numbers' re-save brings the table back, N11)
        let kept: Vec<ObjId> = tracker
            .map(|t| t.ref_ids("formula_owner_dependencies"))
            .unwrap_or_default()
            .into_iter()
            .filter(|f| self.obj(*f).is_some_and(|m| m.u64("owner_kind") == Some(1) && m.msg("formula_owner_uid").map(|u| family(&uuid_of_tsp(u))) == Some(Some(fam))))
            .collect();
        let mut kept_tiles = Vec::new();
        for f in &kept {
            if let Some(a) = self.archive_mut(*f) {
                let m = a.obj_mut();
                m.remove("formula_owner");
                if let Some(t) = m.msg_mut("tiled_cell_dependencies") {
                    kept_tiles.extend(t.ref_ids("cell_record_tiles"));
                    t.remove("cell_record_tiles");
                }
                a.refresh_object_references();
            }
        }
        gone.extend(kept_tiles);
        for f in &fods {
            gone.push(*f);
            if let Some(m) = self.obj(*f) {
                gone.extend(m.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default());
                gone.extend(m.msg("tiled_range_dependencies").map(|t| t.ref_ids("range_precedents_tile")).unwrap_or_default());
            }
        }
        // its header name entry, and the header-name owner's record of that entry's precedent
        let hnm = self.obj(ce).and_then(|m| m.ref_id("header_name_manager"));
        let precedent = hnm.and_then(|h| self.obj(h)).and_then(|h| h.msgs("per_tables").find(|p| p.msg("table_uid").map(uuid_of_tsp) == Some(uuid)).and_then(|p| p.msg("per_table_precedent")).map(|c| (c.u64("column"), c.u64("row").unwrap_or(0))));
        let header_fod = header_owner_fod(self, ce);
        if let Some(h) = hnm.and_then(|h| self.obj_mut(h)) {
            h.retain_msgs("per_tables", |p| p.msg("table_uid").map(uuid_of_tsp) != Some(uuid));
        }
        if let (Some((col, row)), Some(hf)) = (precedent, header_fod) {
            let at = |r: &Msg| r.u64("column") == col && r.u64("row").unwrap_or(0) == row;
            let tiles = self.obj(hf).and_then(|m| m.msg("tiled_cell_dependencies")).map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default();
            if let Some(cd) = self.obj_mut(hf).and_then(|m| m.msg_mut("cell_dependencies")) {
                cd.retain_msgs("cell_record", |r| !at(r));
            }
            for t in &tiles {
                if let Some(m) = self.obj_mut(*t) {
                    m.retain_msgs("cell_records", |r| !at(r));
                }
            }
            drop_empty_tiles(self, hf, &tiles, &mut gone);
        }
        // tracked references that point into the table, and the records the tracking owner
        // (owner kind 7) keeps of them at (row 0, column = formula_id)
        // (a formula names another table by its formula owner uid, of the table's family, N9)
        let into = |r: &Msg| {
            r.msg("ast").is_some_and(|a| a.msgs("AST_node").any(|n| n.msg("AST_cross_table_reference_extra_info").and_then(|x| x.msg("table_id")).map(|u| family(&uuid_of_cf(u))) == Some(Some(fam))))
        };
        let stores: Vec<ObjId> = self.objects_of("TSCE.TrackedReferenceStoreArchive").map(|(i, _)| i).collect();
        let mut dropped: Vec<u64> = Vec::new();
        for s in stores {
            if let Some(m) = self.obj_mut(s) {
                dropped.extend(m.msgs("contained_tracked_reference").filter(|r| into(r)).filter_map(|r| r.u64("formula_id")));
                m.retain_msgs("contained_tracked_reference", |r| !into(r));
            }
        }
        if !dropped.is_empty() {
            let trackers: Vec<ObjId> = tracker_fods(self, ce).into_iter().filter(|f| self.obj(*f).and_then(|m| m.u64("owner_kind")) == Some(7)).collect();
            let hit = |r: &Msg| r.u64("row").unwrap_or(0) == 0 && r.u64("column").is_some_and(|c| dropped.contains(&c));
            for f in trackers {
                let tiles = self.obj(f).and_then(|m| m.msg("tiled_cell_dependencies")).map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default();
                if let Some(cd) = self.obj_mut(f).and_then(|m| m.msg_mut("cell_dependencies")) {
                    cd.retain_msgs("cell_record", |r| !hit(r));
                }
                for t in &tiles {
                    if let Some(m) = self.obj_mut(*t) {
                        m.retain_msgs("cell_records", |r| !hit(r));
                    }
                }
                drop_empty_tiles(self, f, &tiles, &mut gone);
            }
        }
        // the engine's lists
        if let Some(t) = self.obj_mut(ce).and_then(|m| m.msg_mut("dependency_tracker")) {
            t.retain_msgs("formula_owner_dependencies", |r| !r.u64("identifier").is_some_and(|i| fods.contains(&i)));
            if let Some(map) = t.msg_mut("owner_id_map") {
                map.retain_msgs("map_entry", |e| !e.u64("internal_owner_id").is_some_and(|i| internal.contains(&i)));
            }
        }
        for g in &gone {
            self.remove_archive(*g);
        }
        self.refresh_refs(ce);
        if let Some(f) = self.file_of(ce).map(str::to_string) {
            self.ensure_external_refs(&f);
        }
        self.recount_formulas();
        gone
    }
}
