//! Format > Shapes and Lines > Unite / Intersect / Subtract / Exclude Shapes: the outlines are
//! combined into one new shape (an editable path) with the backmost shape's look.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::{Document, DrawableKind, PathEl, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoolOp {
    Unite,
    Intersect,
    /// The front shapes are cut out of the backmost one.
    Subtract,
    Exclude,
}

/// An outline as polygons (curves flattened), in slide points.
fn polygons(path: &crate::Path, g: &crate::Geometry) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    let mut last = Point::new(0.0, 0.0);
    let push = |cur: &mut Vec<[f64; 2]>, p: Point| {
        let s = g.to_slide(p);
        cur.push([s.x as f64, s.y as f64]);
    };
    for el in &path.els {
        match el {
            PathEl::Move(p) => {
                if cur.len() > 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                push(&mut cur, *p);
                last = *p;
            }
            PathEl::Line(p) => {
                push(&mut cur, *p);
                last = *p;
            }
            PathEl::Quad(c, p) => {
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    push(&mut cur, Point::new(u * u * last.x + 2.0 * u * t * c.x + t * t * p.x, u * u * last.y + 2.0 * u * t * c.y + t * t * p.y));
                }
                last = *p;
            }
            PathEl::Cubic(c1, c2, p) => {
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    let x = u * u * u * last.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p.x;
                    let y = u * u * u * last.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p.y;
                    push(&mut cur, Point::new(x, y));
                }
                last = *p;
            }
            PathEl::Close => {
                if cur.len() > 2 {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if cur.len() > 2 {
        out.push(cur);
    }
    out
}

/// A combined outline: its path source (in its own frame), the frame's place and size on the
/// page, and the style of the backmost shape (the result looks like it).
pub struct BooleanShape {
    pub path_source: Msg,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub style: Option<ObjId>,
    /// The shapes combined (back to front): the caller removes them.
    pub originals: Vec<ObjId>,
}

impl Document {
    /// Combine shapes (drawables of one slide or sheet, back to front, as drawn): the outline
    /// of the result, for the caller to insert where it belongs.
    pub fn boolean_shape(&self, drawables: &[crate::Drawable], ids: &[ObjId], op: BoolOp) -> Result<BooleanShape, kn_format::Error> {
        let shapes: Vec<&crate::Drawable> = drawables.iter().filter(|d| ids.contains(&d.id) && matches!(&d.kind, DrawableKind::Shape(s) if !s.is_line)).collect();
        if shapes.len() < 2 {
            return Err(kn_format::Error::Invalid("select two or more shapes".into()));
        }
        let outline = |d: &crate::Drawable| match &d.kind {
            DrawableKind::Shape(s) => polygons(&s.path, &d.geom),
            _ => Vec::new(),
        };
        let mut acc = outline(shapes[0]);
        for d in &shapes[1..] {
            let clip = outline(d);
            let rule = match op {
                BoolOp::Unite => OverlayRule::Union,
                BoolOp::Intersect => OverlayRule::Intersect,
                BoolOp::Subtract => OverlayRule::Difference,
                BoolOp::Exclude => OverlayRule::Xor,
            };
            let res = acc.overlay(&clip, rule, FillRule::NonZero);
            acc = res.into_iter().flatten().collect();
        }
        if acc.is_empty() {
            return Err(kn_format::Error::Invalid("the shapes don't overlap".into()));
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for c in &acc {
            for p in c {
                x0 = x0.min(p[0]);
                y0 = y0.min(p[1]);
                x1 = x1.max(p[0]);
                y1 = y1.max(p[1]);
            }
        }
        let (w, hh) = ((x1 - x0).max(1.0) as f32, (y1 - y0).max(1.0) as f32);
        let back = shapes[0].id;
        let style = self.file.obj(back).and_then(|m| m.find_super("TSD.ShapeArchive")).and_then(|s| s.ref_id("style"));
        let el = |kind: &str, p: Option<[f64; 2]>| {
            let mut e = Msg::new("TSP.Path.Element");
            e.set_enum("type", kind);
            if let Some(p) = p {
                let mut m = Msg::new("TSP.Point");
                m.set_f32("x", (p[0] - x0) as f32);
                m.set_f32("y", (p[1] - y0) as f32);
                e.push_msg("points", m);
            }
            e
        };
        let mut path = Msg::new("TSP.Path");
        for c in &acc {
            path.push_msg("elements", el("moveTo", Some(c[0])));
            for p in &c[1..] {
                path.push_msg("elements", el("lineTo", Some(*p)));
            }
            path.push_msg("elements", el("closeSubpath", None));
        }
        let mut ps = Msg::new("TSD.PathSourceArchive");
        ps.set_bool("horizontalFlip", false);
        ps.set_bool("verticalFlip", false);
        {
            let b = ps.msg_mut_or_create("bezier_path_source");
            let ns = b.msg_mut_or_create("naturalSize");
            ns.set_f32("width", w);
            ns.set_f32("height", hh);
            b.push_msg("path", path);
        }
        Ok(BooleanShape { path_source: ps, x: x0 as f32, y: y0 as f32, w, h: hh, style, originals: shapes.iter().map(|d| d.id).collect() })
    }

    /// Give shape `id` a combined outline (its path source, frame and style).
    pub fn apply_boolean_shape(&mut self, h: &mut History, id: ObjId, b: &BooleanShape) {
        self.touch(h, id);
        if let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
            sh.remove("pathsource");
            sh.push_msg("pathsource", b.path_source.clone());
            if let Some(st) = b.style {
                sh.set_ref("style", st);
            }
            if let Some(d) = sh.find_super_mut("TSD.DrawableArchive") {
                let g = d.msg_mut_or_create("geometry");
                let p = g.msg_mut_or_create("position");
                p.set_f32("x", b.x);
                p.set_f32("y", b.y);
                let s = g.msg_mut_or_create("size");
                s.set_f32("width", b.w);
                s.set_f32("height", b.h);
                g.set_f32("angle", 0.0);
            }
        }
        self.file.refresh_refs(id);
        if let Some(f) = self.file.file_of(id).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
    }

    /// Combine shapes `ids` (on slide index `si`); returns the new shape.
    pub fn shapes_boolean(&mut self, h: &mut History, si: usize, ids: &[ObjId], op: BoolOp) -> Result<ObjId, kn_format::Error> {
        let slides = self.slides();
        let scene = self.scene(&slides, si).ok_or(kn_format::Error::Invalid("no slide".into()))?;
        let b = self.boolean_shape(&scene.drawables, ids, op)?;
        let slide = scene.slide;
        let id = self.insert(h, slide, &crate::insert::NewObject::Shape(crate::insert::BasicShape::Rectangle))?;
        self.apply_boolean_shape(h, id, &b);
        self.delete_drawables(h, slide, &b.originals);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insert::{BasicShape, NewObject};

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn unite_two_rectangles() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key")).unwrap();
        let s = d.slides();
        let mut h = History::default();
        d.begin(&mut h, "Shapes");
        let a = d.insert(&mut h, s[0].slide, &NewObject::Shape(BasicShape::Rectangle)).unwrap();
        let b = d.insert(&mut h, s[0].slide, &NewObject::Shape(BasicShape::Oval)).unwrap();
        d.move_by(&mut h, b, 60.0, 40.0);
        d.end(&mut h);
        d.begin(&mut h, "Unite");
        let u = d.shapes_boolean(&mut h, 0, &[a, b], BoolOp::Unite).unwrap();
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let s2 = d2.slides();
        let sc = d2.scene(&s2, 0).unwrap();
        assert!(sc.drawables.iter().all(|x| x.id != a && x.id != b));
        let n = sc.drawables.iter().find(|x| x.id == u).unwrap();
        // bigger than either original
        assert!(n.geom.w > 100.0 && n.geom.h > 100.0);
    }
}
