//! Arrange > Group / Ungroup. A TSD.GroupArchive (copied from a group Keynote made, R1-group.key)
//! holds its children; children geometry is relative to the group's unrotated frame and the
//! group's size is the union of its children (as Keynote stores it).

use std::sync::OnceLock;

use kn_format::{Archive, KeyFile, Msg, ObjId};

use crate::edit::History;
use crate::types::{Geometry, Point};
use crate::Document;

static DONOR: &[u8] = include_bytes!("../data/donor-group.key");

fn donor() -> &'static KeyFile {
    static D: OnceLock<KeyFile> = OnceLock::new();
    D.get_or_init(|| KeyFile::from_zip(DONOR).expect("embedded group donor"))
}

fn geom_of(k: &KeyFile, id: ObjId) -> Option<Geometry> {
    Some(crate::scene::geometry(k.obj(id)?.find_super("TSD.DrawableArchive")?))
}

fn set_geom(k: &mut KeyFile, id: ObjId, g: &Geometry, parent: ObjId) {
    let Some(d) = k.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) else { return };
    let geo = d.msg_mut_or_create("geometry");
    {
        let p = geo.msg_mut_or_create("position");
        p.set_f32("x", g.x);
        p.set_f32("y", g.y);
    }
    geo.set_f32("angle", g.angle.rem_euclid(360.0));
    d.set_ref("parent", parent);
}

/// Where a parent lists its drawables: a Keynote slide in z order plus owned_drawables, a
/// Numbers sheet (TN.SheetArchive) in drawable_infos alone (Mac N22: the group takes its
/// children's place there).
fn parent_lists(k: &KeyFile, parent: ObjId) -> (&'static str, Option<&'static str>) {
    match k.type_name(parent) {
        "TN.SheetArchive" => ("drawable_infos", None),
        // Pages: the floating drawables' stacking order (the caller keeps each page's own list
        // and the parent refs, which floating drawables don't have)
        "TP.DrawablesZOrderArchive" => ("drawables", None),
        _ => ("drawables_z_order", Some("owned_drawables")),
    }
}

fn retain_refs(m: &mut Msg, field: &str, keep: impl Fn(ObjId) -> bool) {
    m.retain_msgs(field, |r| keep(r.u64("identifier").unwrap_or(0)));
}

impl Document {
    /// The group an object belongs to (Edit > Select Parent), if any.
    pub fn group_of(&self, id: ObjId) -> Option<ObjId> {
        let parent = self.file.obj(id)?.find_super("TSD.DrawableArchive")?.ref_id("parent")?;
        (self.file.type_name(parent) == "TSD.GroupArchive").then_some(parent)
    }

    /// Group drawables of a parent (at least two). Returns the new group's id.
    pub fn group(&mut self, h: &mut History, parent: ObjId, ids: &[ObjId]) -> Result<ObjId, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let (z_field, owned_field) = parent_lists(&self.file, parent);
        let z = self.file.obj(parent).ok_or(err("no parent"))?.ref_ids(z_field);
        let members: Vec<ObjId> = z.iter().copied().filter(|i| ids.contains(i)).collect();
        if members.len() < 2 {
            return Err(err("select at least two objects to group"));
        }
        let geoms: Vec<Geometry> = members.iter().filter_map(|i| geom_of(&self.file, *i)).collect();
        // group frame = union of the children's rotated bounds
        let (mut lo, mut hi) = (Point::new(f32::INFINITY, f32::INFINITY), Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY));
        for g in &geoms {
            let (a, b) = g.bounds();
            lo = Point::new(lo.x.min(a.x), lo.y.min(a.y));
            hi = Point::new(hi.x.max(b.x), hi.y.max(b.y));
        }
        let file = self.file.file_of(parent).ok_or(err("parent file"))?.to_string();
        let mut touch = vec![parent];
        touch.extend(&members);
        let top = z.iter().rposition(|i| members.contains(i)).unwrap_or(z.len() - 1);
        let group_id = self.structural(h, &touch, |k| -> Result<ObjId, kn_format::Error> {
            let src = donor();
            let (gid_src, gmsg) = src.first_of("TSD.GroupArchive").ok_or(err("donor"))?;
            let d = gmsg.find_super("TSD.DrawableArchive").ok_or(err("donor"))?;
            let captions: Vec<ObjId> = ["title", "caption"].iter().filter_map(|f| d.ref_id(f)).collect();
            let ids = k.alloc_ids(1 + captions.len() as u64);
            let gid = ids[0];
            // captions first (plain copies with new ids)
            for (i, c) in captions.iter().enumerate() {
                let mut a: Archive = src.archive(*c).ok_or(err("donor caption"))?.clone();
                a.info.set_u64("identifier", ids[1 + i]);
                k.insert_archive(&file, a);
                k.add_uuid_entry(&file, ids[1 + i]);
            }
            let mut a: Archive = src.archive(gid_src).ok_or(err("donor group"))?.clone();
            a.info.set_u64("identifier", gid);
            {
                let g = a.obj_mut();
                g.remove("children");
                for m in &members {
                    g.push_ref("children", *m);
                }
                let dr = g.find_super_mut("TSD.DrawableArchive").unwrap();
                dr.set_ref("parent", parent);
                for (f, nid) in ["title", "caption"].iter().zip(ids[1..].iter()) {
                    if dr.has(f) {
                        dr.set_ref(f, *nid);
                    }
                }
                let geo = dr.msg_mut_or_create("geometry");
                let p = geo.msg_mut_or_create("position");
                p.set_f32("x", lo.x);
                p.set_f32("y", lo.y);
                let s = geo.msg_mut_or_create("size");
                s.set_f32("width", hi.x - lo.x);
                s.set_f32("height", hi.y - lo.y);
                geo.set_f32("angle", 0.0);
            }
            k.insert_archive(&file, a);
            k.add_uuid_entry(&file, gid);
            // children: relative positions, parent = group
            for (m, g) in members.iter().zip(&geoms) {
                let rel = Geometry { x: g.x - lo.x, y: g.y - lo.y, ..*g };
                set_geom(k, *m, &rel, gid);
                k.refresh_refs(*m);
            }
            // parent: the group replaces its children, at the topmost child's place
            if let Some(sa) = k.archive_mut(parent) {
                let s = sa.obj_mut();
                let mut zz: Vec<ObjId> = s.ref_ids(z_field);
                let at = zz.iter().position(|i| *i == z[top]).unwrap_or(zz.len());
                zz.insert(at + 1, gid);
                zz.retain(|i| !members.contains(i));
                s.remove(z_field);
                for i in zz {
                    s.push_ref(z_field, i);
                }
                if let Some(owned) = owned_field {
                    retain_refs(s, owned, |i| !members.contains(&i));
                    s.push_ref(owned, gid);
                }
            }
            k.refresh_refs(parent);
            k.refresh_refs(gid);
            k.ensure_external_refs(&file);
            Ok(gid)
        })?;
        Ok(group_id)
    }

    /// Ungroup: children go back onto the parent where the group was, keeping their position on
    /// screen (the group's rotation is folded into each child). Returns the children.
    pub fn ungroup(&mut self, h: &mut History, parent: ObjId, group: ObjId) -> Result<Vec<ObjId>, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let gm = self.file.obj(group).ok_or(err("no group"))?;
        if gm.type_name() != "TSD.GroupArchive" {
            return Err(err("not a group"));
        }
        let children = gm.ref_ids("children");
        let gg = geom_of(&self.file, group).ok_or(err("group geometry"))?;
        let mut touch = vec![parent, group];
        touch.extend(&children);
        let (z_field, owned_field) = parent_lists(&self.file, parent);
        self.structural(h, &touch, |k| {
            for c in &children {
                let Some(cg) = geom_of(k, *c) else { continue };
                let centre = gg.to_slide(cg.center());
                let flip = gg.hflip != gg.vflip;
                let angle = if flip { gg.angle - cg.angle } else { gg.angle + cg.angle };
                let ng = Geometry { x: centre.x - cg.w / 2.0, y: centre.y - cg.h / 2.0, angle, ..cg };
                set_geom(k, *c, &ng, parent);
                k.refresh_refs(*c);
            }
            if let Some(sa) = k.archive_mut(parent) {
                let s = sa.obj_mut();
                let mut zz = s.ref_ids(z_field);
                if let Some(at) = zz.iter().position(|i| *i == group) {
                    zz.splice(at..=at, children.iter().copied());
                }
                s.remove(z_field);
                for i in zz {
                    s.push_ref(z_field, i);
                }
                if let Some(owned) = owned_field {
                    retain_refs(s, owned, |i| i != group);
                    for c in &children {
                        s.push_ref(owned, *c);
                    }
                }
            }
            k.refresh_refs(parent);
        });
        Ok(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn group_and_ungroup_keep_positions() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        // "Shapes (default shape + properties)" slide has several shapes
        let (i, sc) = (0..s.len())
            .map(|i| (i, d.scene(&s, i).unwrap()))
            .find(|(_, sc)| sc.drawables.iter().filter(|x| x.placeholder.is_none() && !x.from_template && matches!(x.kind, DrawableKind::Shape(_))).count() >= 3)
            .unwrap();
        let shapes: Vec<_> = sc.drawables.iter().filter(|x| x.placeholder.is_none() && !x.from_template).take(2).cloned().collect();
        let ids: Vec<ObjId> = shapes.iter().map(|x| x.id).collect();
        let mut h = History::default();
        d.begin(&mut h, "Group");
        let g = d.group(&mut h, s[i].slide, &ids).unwrap();
        d.end(&mut h);
        // saved file: the slide shows one group with both shapes, drawn where they were
        let d2 = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc2 = d2.scene(&d2.slides(), i).unwrap();
        let grp = sc2.drawables.iter().find(|x| x.id == g).unwrap();
        let DrawableKind::Group(kids) = &grp.kind else { panic!("not a group") };
        assert_eq!(kids.len(), 2);
        for (k, orig) in kids.iter().zip(&shapes) {
            let c = grp.geom.to_slide(k.geom.center());
            assert!((c.x - orig.geom.center().x).abs() < 0.01 && (c.y - orig.geom.center().y).abs() < 0.01);
        }
        assert!(!sc2.drawables.iter().any(|x| ids.contains(&x.id)), "children not on the slide directly");
        // ungroup restores the original geometry
        d.begin(&mut h, "Ungroup");
        d.ungroup(&mut h, s[i].slide, g).unwrap();
        d.end(&mut h);
        let sc3 = d.scene(&d.slides(), i).unwrap();
        for orig in &shapes {
            let now = sc3.drawables.iter().find(|x| x.id == orig.id).unwrap();
            assert!((now.geom.x - orig.geom.x).abs() < 0.01 && (now.geom.y - orig.geom.y).abs() < 0.01 && (now.geom.angle - orig.geom.angle).abs() < 0.01);
        }
        // and undo twice brings back the untouched slide
        assert!(d.undo(&mut h) && d.undo(&mut h));
        assert_eq!(d.scene(&d.slides(), i).unwrap().drawables.len(), sc.drawables.len());
    }
}
