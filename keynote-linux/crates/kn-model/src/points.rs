//! Format > Shapes and Lines > Make Editable and point editing: shapes as Keynote's editable
//! bezier path (subpaths of nodes with in / out control points; sharp, smooth or bezier).

use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::{Document, PathEl, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeKind {
    Sharp,
    Bezier,
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub inc: Point,
    pub pt: Point,
    pub out: Point,
    pub kind: NodeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub nodes: Vec<Node>,
    pub closed: bool,
}

/// Editable nodes of a shape, in its frame (0..w, 0..h).
#[derive(Clone, Debug, PartialEq)]
pub struct Editable {
    pub subpaths: Vec<Subpath>,
}

fn pt(m: Option<&Msg>) -> Point {
    m.map(|p| Point::new(p.f32("x").unwrap_or(0.0), p.f32("y").unwrap_or(0.0))).unwrap_or_default()
}

fn pt_msg(p: Point) -> Msg {
    let mut m = Msg::new("TSP.Point");
    m.set_f32("x", p.x);
    m.set_f32("y", p.y);
    m
}

/// Nodes from any outline (in frame coordinates).
pub fn from_path(path: &crate::Path) -> Editable {
    let mut subs: Vec<Subpath> = Vec::new();
    let mut cur: Option<Subpath> = None;
    let mut last = Point::default();
    let sharp = |p: Point| Node { inc: p, pt: p, out: p, kind: NodeKind::Sharp };
    for el in &path.els {
        match *el {
            PathEl::Move(p) => {
                if let Some(s) = cur.take() {
                    subs.push(s);
                }
                cur = Some(Subpath { nodes: vec![sharp(p)], closed: false });
                last = p;
            }
            PathEl::Line(p) => {
                cur.get_or_insert(Subpath { nodes: vec![sharp(last)], closed: false }).nodes.push(sharp(p));
                last = p;
            }
            PathEl::Quad(c, p) => {
                let c1 = Point::new(last.x + 2.0 / 3.0 * (c.x - last.x), last.y + 2.0 / 3.0 * (c.y - last.y));
                let c2 = Point::new(p.x + 2.0 / 3.0 * (c.x - p.x), p.y + 2.0 / 3.0 * (c.y - p.y));
                let s = cur.get_or_insert(Subpath { nodes: vec![sharp(last)], closed: false });
                if let Some(n) = s.nodes.last_mut() {
                    n.out = c1;
                    n.kind = NodeKind::Bezier;
                }
                s.nodes.push(Node { inc: c2, pt: p, out: p, kind: NodeKind::Bezier });
                last = p;
            }
            PathEl::Cubic(c1, c2, p) => {
                let s = cur.get_or_insert(Subpath { nodes: vec![sharp(last)], closed: false });
                if let Some(n) = s.nodes.last_mut() {
                    n.out = c1;
                    n.kind = NodeKind::Bezier;
                }
                s.nodes.push(Node { inc: c2, pt: p, out: p, kind: NodeKind::Bezier });
                last = p;
            }
            PathEl::Close => {
                if let Some(mut s) = cur.take() {
                    // a closing node on top of the first one is the first one
                    if s.nodes.len() > 2 {
                        let (f, l) = (s.nodes[0].pt, s.nodes[s.nodes.len() - 1].pt);
                        if (f.x - l.x).abs() < 0.01 && (f.y - l.y).abs() < 0.01 {
                            let lastn = s.nodes.pop().unwrap();
                            s.nodes[0].inc = lastn.inc;
                        }
                    }
                    s.closed = true;
                    subs.push(s);
                }
            }
        }
    }
    if let Some(s) = cur {
        subs.push(s);
    }
    Editable { subpaths: subs }
}

impl Editable {
    fn to_msg(&self, w: f32, h: f32) -> Msg {
        let mut eb = Msg::new("TSD.EditableBezierPathSourceArchive");
        for s in &self.subpaths {
            let mut sm = Msg::new("TSD.EditableBezierPathSourceArchive.Subpath");
            for n in &s.nodes {
                let mut nm = Msg::new("TSD.EditableBezierPathSourceArchive.Node");
                nm.push_msg("inControlPoint", pt_msg(n.inc));
                nm.push_msg("nodePoint", pt_msg(n.pt));
                nm.push_msg("outControlPoint", pt_msg(n.out));
                nm.set_enum("type", match n.kind {
                    NodeKind::Sharp => "sharp",
                    NodeKind::Bezier => "bezier",
                    NodeKind::Smooth => "smooth",
                });
                sm.push_msg("nodes", nm);
            }
            sm.set_bool("closed", s.closed);
            eb.push_msg("subpaths", sm);
        }
        let mut ns = Msg::new("TSP.Size");
        ns.set_f32("width", w.max(1.0));
        ns.set_f32("height", h.max(1.0));
        eb.push_msg("naturalSize", ns);
        eb
    }

    /// Smooth node `i` of subpath `s`: handles along the line between its neighbours.
    pub fn set_kind(&mut self, s: usize, i: usize, kind: NodeKind) {
        let Some(sp) = self.subpaths.get_mut(s) else { return };
        let n = sp.nodes.len();
        if i >= n {
            return;
        }
        let prev = if i > 0 { sp.nodes[i - 1].pt } else if sp.closed { sp.nodes[n - 1].pt } else { sp.nodes[i].pt };
        let next = if i + 1 < n { sp.nodes[i + 1].pt } else if sp.closed { sp.nodes[0].pt } else { sp.nodes[i].pt };
        let node = &mut sp.nodes[i];
        node.kind = kind;
        match kind {
            NodeKind::Sharp => {
                node.inc = node.pt;
                node.out = node.pt;
            }
            NodeKind::Smooth | NodeKind::Bezier => {
                let (dx, dy) = ((next.x - prev.x) / 6.0, (next.y - prev.y) / 6.0);
                node.inc = Point::new(node.pt.x - dx, node.pt.y - dy);
                node.out = Point::new(node.pt.x + dx, node.pt.y + dy);
            }
        }
    }
}

impl Document {
    /// The shape's editable nodes in its frame, if it is an editable bezier.
    pub fn editable_nodes(&self, id: ObjId) -> Option<Editable> {
        let ps = self.file.obj(id)?.find_super("TSD.ShapeArchive")?.msg("pathsource")?;
        let e = ps.msg("editable_bezier_path_source")?;
        let g = crate::scene::geometry(self.file.obj(id)?.find_super("TSD.DrawableArchive")?);
        let ns = e.msg("naturalSize");
        let (nw, nh) = (ns.and_then(|n| n.f32("width")).unwrap_or(g.w).max(1e-3), ns.and_then(|n| n.f32("height")).unwrap_or(g.h).max(1e-3));
        let (sx, sy) = (g.w / nw, g.h / nh);
        let sc = |p: Point| Point::new(p.x * sx, p.y * sy);
        let subpaths = e
            .msgs("subpaths")
            .map(|s| Subpath {
                closed: s.bool("closed").unwrap_or(false),
                nodes: s
                    .msgs("nodes")
                    .map(|n| Node {
                        inc: sc(pt(n.msg("inControlPoint"))),
                        pt: sc(pt(n.msg("nodePoint"))),
                        out: sc(pt(n.msg("outControlPoint"))),
                        kind: match n.enum_name("type") {
                            Some("bezier") => NodeKind::Bezier,
                            Some("smooth") => NodeKind::Smooth,
                            _ => NodeKind::Sharp,
                        },
                    })
                    .collect(),
            })
            .collect();
        Some(Editable { subpaths })
    }

    /// Store nodes (in the shape's frame) as its editable bezier path.
    pub fn set_editable_nodes(&mut self, h: &mut History, id: ObjId, e: &Editable) -> bool {
        let Some(g) = self.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(crate::scene::geometry) else { return false };
        self.touch(h, id);
        let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) else { return false };
        let (hf, vf) = sh.msg("pathsource").map(|p| (p.bool("horizontalFlip").unwrap_or(false), p.bool("verticalFlip").unwrap_or(false))).unwrap_or((false, false));
        sh.remove("pathsource");
        let mut ps = Msg::new("TSD.PathSourceArchive");
        ps.set_bool("horizontalFlip", hf);
        ps.set_bool("verticalFlip", vf);
        ps.push_msg("editable_bezier_path_source", e.to_msg(g.w, g.h));
        sh.push_msg("pathsource", ps);
        true
    }

    /// Format > Shapes and Lines > Break Apart: each separate outline of a shape becomes a shape
    /// of its own (same look). Returns the shapes.
    pub fn break_apart(&mut self, h: &mut History, slide: ObjId, id: ObjId, outline: &crate::Path) -> Result<Vec<ObjId>, kn_format::Error> {
        let e = self.break_apart_parts(id, outline)?;
        let clip = self.copy_objects(&[id]).ok_or(kn_format::Error::Invalid("can't copy the shape".into()))?;
        let mut out = vec![id];
        for _ in 1..e.subpaths.len() {
            out.extend(self.paste_objects(h, slide, &clip, 0.0)?);
        }
        for (k, sid) in out.iter().enumerate() {
            let one = Editable { subpaths: vec![e.subpaths[k].clone()] };
            self.set_editable_nodes(h, *sid, &one);
        }
        Ok(out)
    }

    /// Break Apart's parts: each separate outline of the shape as its own editable nodes (the
    /// caller copies the shape once per part and gives each its part).
    pub fn break_apart_parts(&self, id: ObjId, outline: &crate::Path) -> Result<Editable, kn_format::Error> {
        let e = self.editable_nodes(id).unwrap_or_else(|| from_path(outline));
        if e.subpaths.len() < 2 {
            return Err(kn_format::Error::Invalid("this shape has only one part".into()));
        }
        Ok(e)
    }

    /// Format > Shapes and Lines > Make Editable.
    pub fn make_editable(&mut self, h: &mut History, id: ObjId, outline: &crate::Path) -> bool {
        if self.editable_nodes(id).is_some() {
            return true;
        }
        let e = from_path(outline);
        !e.subpaths.is_empty() && self.set_editable_nodes(h, id, &e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insert::{BasicShape, NewObject};

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn make_editable_and_move_a_point() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key")).unwrap();
        let s = d.slides();
        let mut h = History::default();
        d.begin(&mut h, "Star");
        let id = d.insert(&mut h, s[0].slide, &NewObject::Shape(BasicShape::Star)).unwrap();
        d.end(&mut h);
        let sc = d.scene(&s, 0).unwrap();
        let path = match &sc.drawables.iter().find(|x| x.id == id).unwrap().kind {
            crate::DrawableKind::Shape(sh) => sh.path.clone(),
            _ => unreachable!(),
        };
        d.begin(&mut h, "Edit");
        assert!(d.make_editable(&mut h, id, &path));
        let mut e = d.editable_nodes(id).unwrap();
        assert_eq!(e.subpaths.len(), 1);
        assert!(e.subpaths[0].closed);
        assert_eq!(e.subpaths[0].nodes.len(), 10);
        e.subpaths[0].nodes[0].pt = Point::new(1.0, 2.0);
        e.set_kind(0, 1, NodeKind::Smooth);
        assert!(d.set_editable_nodes(&mut h, id, &e));
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let e2 = d2.editable_nodes(id).unwrap();
        assert!((e2.subpaths[0].nodes[0].pt.x - 1.0).abs() < 0.01);
        assert_eq!(e2.subpaths[0].nodes[1].kind, NodeKind::Smooth);
    }
}

impl Document {
    /// Format > Shapes and Lines > Make Motion Path from Shape: `object` gets a Move action along
    /// the outline of `shape` (starting where the object is); the shape is removed.
    pub fn motion_path_from_shape(&mut self, h: &mut History, slide: ObjId, shape: ObjId, shape_geom: crate::Geometry, outline: &crate::Path, object: ObjId) -> Result<ObjId, kn_format::Error> {
        let e = from_path(outline);
        let sp = e.subpaths.first().ok_or(kn_format::Error::Invalid("the shape has no outline".into()))?;
        // slide coordinates, then offsets from the path's first point
        let to = |p: Point| shape_geom.to_slide(p);
        let o = to(sp.nodes[0].pt);
        let rel = |p: Point| {
            let s = to(p);
            Point::new(s.x - o.x, s.y - o.y)
        };
        let nodes: Vec<Node> = sp.nodes.iter().map(|n| Node { inc: rel(n.inc), pt: rel(n.pt), out: rel(n.out), kind: n.kind }).collect();
        let (mut x0, mut y0, mut x1, mut y1) = (0f32, 0f32, 0f32, 0f32);
        for n in &nodes {
            x0 = x0.min(n.pt.x);
            y0 = y0.min(n.pt.y);
            x1 = x1.max(n.pt.x);
            y1 = y1.max(n.pt.y);
        }
        let ed = Editable { subpaths: vec![Subpath { nodes, closed: sp.closed }] };
        let mut ps = Msg::new("TSD.PathSourceArchive");
        ps.set_bool("horizontalFlip", false);
        ps.set_bool("verticalFlip", false);
        ps.push_msg("editable_bezier_path_source", ed.to_msg(x1 - x0, y1 - y0));
        let build = self.add_build(h, slide, object, crate::buildedit::BuildKind::Action, "apple:action-motion-path", 2.0)?;
        self.touch(h, build);
        if let Some(a) = self.file.obj_mut(build).and_then(|b| b.msg_mut("attributes")) {
            a.remove("action_motionPathSource");
            a.remove("motionPathString");
            a.push_msg("action_motionPathSource", ps);
        }
        self.delete_drawables(h, slide, &[shape]);
        Ok(build)
    }
}

impl Document {
    /// A shape's path source (Save to My Shapes), with its frame size.
    pub fn path_source_of(&self, id: ObjId) -> Option<(Msg, f32, f32)> {
        let m = self.file.obj(id)?;
        let ps = m.find_super("TSD.ShapeArchive")?.msg("pathsource")?.clone();
        let g = crate::scene::geometry(m.find_super("TSD.DrawableArchive")?);
        Some((ps, g.w, g.h))
    }

    /// Insert a shape with a saved path source (My Shapes), centred on the slide.
    pub fn insert_with_path_source(&mut self, h: &mut History, slide: ObjId, ps: Msg, w: f32, hh: f32) -> Result<ObjId, kn_format::Error> {
        let id = self.insert(h, slide, &crate::insert::NewObject::Shape(crate::insert::BasicShape::Rectangle))?;
        let size = self.slide_size();
        self.touch(h, id);
        if let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
            sh.remove("pathsource");
            sh.push_msg("pathsource", ps);
            if let Some(d) = sh.find_super_mut("TSD.DrawableArchive") {
                let g = d.msg_mut_or_create("geometry");
                let p = g.msg_mut_or_create("position");
                p.set_f32("x", ((size.w - w) / 2.0).round());
                p.set_f32("y", ((size.h - hh) / 2.0).round());
                let s = g.msg_mut_or_create("size");
                s.set_f32("width", w);
                s.set_f32("height", hh);
            }
        }
        Ok(id)
    }

    /// Give shape `id` the outline `ps` at `w` x `hh`, keeping its position (a library shape
    /// put in place of a freshly inserted one).
    pub fn set_shape_path_source(&mut self, h: &mut History, id: ObjId, ps: Msg, w: f32, hh: f32) {
        self.touch(h, id);
        if let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
            sh.remove("pathsource");
            sh.push_msg("pathsource", ps);
            if let Some(d) = sh.find_super_mut("TSD.DrawableArchive") {
                let s = d.msg_mut_or_create("geometry").msg_mut_or_create("size");
                s.set_f32("width", w);
                s.set_f32("height", hh);
            }
        }
    }
}

fn signature(ps: &Msg) -> Option<(String, String)> {
    for f in ["point_path_source", "scalar_path_source", "callout_path_source"] {
        if let Some(m) = ps.msg(f) {
            return Some((f.to_string(), m.enum_name("type").unwrap_or("").to_string()));
        }
    }
    None
}

impl Document {
    /// Format > Shapes and Lines > Reset Text and Object Handles: an arrow, star, rounded
    /// rectangle, polygon or callout gets its green handle back where it started.
    pub fn reset_handles(&mut self, h: &mut History, id: ObjId) -> bool {
        let Some((ps, w, hh)) = self.path_source_of(id) else { return false };
        let Some(sig) = signature(&ps) else { return false };
        let Some(base) = crate::insert::BasicShape::ALL.iter().map(|b| b.path_source()).find(|p| signature(p).as_ref() == Some(&sig)) else { return false };
        let mut fresh = base;
        crate::edit::resize_path_source(&mut fresh, w, hh);
        fresh.set_bool("horizontalFlip", ps.bool("horizontalFlip").unwrap_or(false));
        fresh.set_bool("verticalFlip", ps.bool("verticalFlip").unwrap_or(false));
        self.touch(h, id);
        if let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
            sh.remove("pathsource");
            sh.push_msg("pathsource", fresh);
        }
        true
    }
}

#[cfg(test)]
mod para_style_tests {
    use crate::edit::History;
    use crate::Document;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn apply_a_paragraph_style() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/12-title-layout-text.key")).unwrap();
        let styles = d.paragraph_styles();
        assert!(styles.iter().any(|s| s.1 == "Title"), "{styles:?}");
        let s = d.slides();
        let title = d.file.obj(s[0].slide).and_then(|m| m.ref_id("titlePlaceholder")).unwrap();
        let storage = d.storage_of(title).unwrap();
        let body = styles.iter().find(|s| s.1.starts_with("Body")).unwrap().0;
        let mut h = History::default();
        d.begin(&mut h, "Style");
        assert!(d.set_paragraph_style(&mut h, storage, body));
        d.end(&mut h);
        assert!(d.paragraph_style_name(storage).unwrap().starts_with("Body"));
    }
}
