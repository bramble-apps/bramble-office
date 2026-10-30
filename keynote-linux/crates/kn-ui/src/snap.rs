//! Keynote's alignment guides: while moving or resizing, edges and centres snap to the slide and
//! to other objects, and equal spacing / equal sizes are found (Settings > Rulers > Alignment
//! Guides). Everything is in slide points.

use kn_model::Point;

/// A box: (left, top, right, bottom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bx {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Bx {
    pub fn new(lo: Point, hi: Point) -> Bx {
        Bx { x0: lo.x, y0: lo.y, x1: hi.x, y1: hi.y }
    }
    fn shifted(self, dx: f32, dy: f32) -> Bx {
        Bx { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    fn lo(&self, axis: usize) -> f32 {
        if axis == 0 { self.x0 } else { self.y0 }
    }
    fn hi(&self, axis: usize) -> f32 {
        if axis == 0 { self.x1 } else { self.y1 }
    }
    fn mid(&self, axis: usize) -> f32 {
        (self.lo(axis) + self.hi(axis)) / 2.0
    }
    fn len(&self, axis: usize) -> f32 {
        self.hi(axis) - self.lo(axis)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Opts {
    pub center: bool,
    pub edges: bool,
    pub spacing: bool,
    pub sizing: bool,
    /// Snap distance in slide points.
    pub tol: f32,
}

/// What to draw: an alignment line, an equal-gap marker or a same-size marker.
#[derive(Clone, Debug, PartialEq)]
pub enum Guide {
    Line(Point, Point),
    /// A gap between two boxes along an axis (drawn as a short bar with end ticks).
    Gap { axis: usize, from: f32, to: f32, at: f32 },
    /// A box that has the same width (axis 0) or height (axis 1) as the moving one.
    Size { axis: usize, bx: Bx },
}

thread_local! {
    /// The slide's user guides: (axis, position). Objects always snap to them.
    static USER: std::cell::RefCell<Vec<(usize, f32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Set the user guides for the next snaps (axis 0 = vertical guide at x, 1 = horizontal at y).
pub fn set_user_guides(g: Vec<(usize, f32)>) {
    USER.with(|u| *u.borrow_mut() = g);
}

/// One axis' alignment targets: (position, extent of the thing on the other axis).
fn targets(axis: usize, others: &[Bx], slide: Bx, o: &Opts) -> Vec<(f32, Option<Bx>)> {
    let mut t = Vec::new();
    if o.tol > 0.0 && (o.center || o.edges || o.spacing || o.sizing) {
        USER.with(|u| t.extend(u.borrow().iter().filter(|g| g.0 == axis).map(|g| (g.1, None))));
    }
    if o.center || o.edges {
        t.push((slide.mid(axis), None));
    }
    if o.edges {
        t.push((slide.lo(axis), None));
        t.push((slide.hi(axis), None));
    }
    for b in others {
        if o.center {
            t.push((b.mid(axis), Some(*b)));
        }
        if o.edges {
            t.push((b.lo(axis), Some(*b)));
            t.push((b.hi(axis), Some(*b)));
        }
    }
    t
}

fn features(b: &Bx, axis: usize, o: &Opts) -> Vec<f32> {
    let mut f = Vec::new();
    if o.center || o.edges {
        f.push(b.mid(axis));
    }
    if o.edges {
        f.push(b.lo(axis));
        f.push(b.hi(axis));
    }
    f
}

/// Lines for every feature of `b` that sits exactly on a target.
fn lines(b: &Bx, others: &[Bx], slide: Bx, o: &Opts, out: &mut Vec<Guide>) {
    for axis in 0..2 {
        let other = 1 - axis;
        for f in features(b, axis, o) {
            for (t, ob) in targets(axis, others, slide, o) {
                if (f - t).abs() > 0.05 {
                    continue;
                }
                let (a, z) = match ob {
                    Some(ob) => (b.lo(other).min(ob.lo(other)), b.hi(other).max(ob.hi(other))),
                    None => (slide.lo(other), slide.hi(other)),
                };
                let g = if axis == 0 { Guide::Line(Point::new(t, a), Point::new(t, z)) } else { Guide::Line(Point::new(a, t), Point::new(z, t)) };
                if !out.contains(&g) {
                    out.push(g);
                }
            }
        }
    }
}

/// Boxes beside `b` along `axis` (overlapping it on the other axis), sorted.
fn row(b: &Bx, others: &[Bx], axis: usize) -> Vec<Bx> {
    let other = 1 - axis;
    let mut r: Vec<Bx> = others.iter().copied().filter(|o| o.hi(other) > b.lo(other) && o.lo(other) < b.hi(other)).collect();
    r.sort_by(|a, c| a.lo(axis).total_cmp(&c.lo(axis)));
    r
}

/// Equal-spacing snap along one axis: the shift that makes a gap next to `b` equal to another
/// gap in its row (or centres it between its two neighbours).
fn spacing_snap(b: &Bx, others: &[Bx], axis: usize, tol: f32) -> Option<f32> {
    let r = row(b, others, axis);
    let left = r.iter().filter(|o| o.hi(axis) <= b.lo(axis) + tol).max_by(|a, c| a.hi(axis).total_cmp(&c.hi(axis)));
    let right = r.iter().filter(|o| o.lo(axis) >= b.hi(axis) - tol).min_by(|a, c| a.lo(axis).total_cmp(&c.lo(axis)));
    let mut gaps: Vec<f32> = r.windows(2).map(|w| w[1].lo(axis) - w[0].hi(axis)).filter(|g| *g > 0.5).collect();
    let mut best: Option<f32> = None;
    let mut consider = |shift: f32| {
        if shift.abs() <= tol && best.is_none_or(|b| shift.abs() < b.abs()) {
            best = Some(shift);
        }
    };
    if let (Some(l), Some(rt)) = (left, right) {
        // centred between the neighbours
        let want = (l.hi(axis) + rt.lo(axis) - b.len(axis)) / 2.0;
        consider(want - b.lo(axis));
        gaps.push(b.lo(axis) - l.hi(axis));
    }
    for g in gaps {
        if let Some(l) = left {
            consider(l.hi(axis) + g - b.lo(axis));
        }
        if let Some(rt) = right {
            consider(rt.lo(axis) - g - b.hi(axis));
        }
    }
    best
}

/// Gap markers for `b` when a gap next to it equals another gap in its row.
fn spacing_guides(b: &Bx, others: &[Bx], out: &mut Vec<Guide>) {
    for axis in 0..2 {
        let other = 1 - axis;
        let mut all = row(b, others, axis);
        all.push(*b);
        all.sort_by(|a, c| a.lo(axis).total_cmp(&c.lo(axis)));
        let gaps: Vec<(f32, f32, f32)> = all
            .windows(2)
            .map(|w| (w[0].hi(axis), w[1].lo(axis), (w[0].lo(other).max(w[1].lo(other)) + w[0].hi(other).min(w[1].hi(other))) / 2.0))
            .filter(|g| g.1 - g.0 > 0.5)
            .collect();
        let near_b = |g: &(f32, f32, f32)| (g.0 - b.hi(axis)).abs() < 0.05 || (g.1 - b.lo(axis)).abs() < 0.05;
        for g in gaps.iter().filter(|g| near_b(g)) {
            let same: Vec<&(f32, f32, f32)> = gaps.iter().filter(|h| ((h.1 - h.0) - (g.1 - g.0)).abs() < 0.05).collect();
            if same.len() >= 2 {
                for h in same {
                    let gd = Guide::Gap { axis, from: h.0, to: h.1, at: h.2 };
                    if !out.contains(&gd) {
                        out.push(gd);
                    }
                }
            }
        }
    }
}

/// Snap a move of the box `b` by `d`; returns the adjusted move and the guides to draw.
pub fn snap_move(b: Bx, others: &[Bx], slide: Bx, d: (f32, f32), o: &Opts) -> ((f32, f32), Vec<Guide>) {
    let mut d = d;
    let moved = b.shifted(d.0, d.1);
    for axis in 0..2 {
        let mut best: Option<f32> = None;
        for f in features(&moved, axis, o) {
            for (t, _) in targets(axis, others, slide, o) {
                let s = t - f;
                if s.abs() <= o.tol && best.is_none_or(|b| s.abs() < b.abs()) {
                    best = Some(s);
                }
            }
        }
        if best.is_none() && o.spacing {
            best = spacing_snap(&moved, others, axis, o.tol);
        }
        if let Some(s) = best {
            if axis == 0 {
                d.0 += s
            } else {
                d.1 += s
            }
        }
    }
    let fin = b.shifted(d.0, d.1);
    let mut g = Vec::new();
    lines(&fin, others, slide, o, &mut g);
    if o.spacing {
        spacing_guides(&fin, others, &mut g);
    }
    (d, g)
}

/// Snap the moving edges of a box being resized (unrotated objects). `edges` says which edges
/// move: (left, top, right, bottom). Returns the snapped box and guides.
pub fn snap_resize(b: Bx, edges: (bool, bool, bool, bool), others: &[Bx], slide: Bx, o: &Opts) -> (Bx, Vec<Guide>) {
    let mut b = b;
    let all_targets = |axis: usize| {
        // a resized edge snaps to edges and centres alike
        let eo = Opts { center: true, edges: true, ..*o };
        targets(axis, others, slide, &eo).into_iter().map(|t| t.0).collect::<Vec<f32>>()
    };
    let snap_val = |v: f32, ts: &[f32]| -> Option<f32> {
        ts.iter().map(|t| t - v).filter(|s| s.abs() <= o.tol).min_by(|a, c| a.abs().total_cmp(&c.abs()))
    };
    let (l, t, r, btm) = edges;
    for axis in 0..2 {
        let (lo_moves, hi_moves) = if axis == 0 { (l, r) } else { (t, btm) };
        if !lo_moves && !hi_moves {
            continue;
        }
        let ts = all_targets(axis);
        // same size as another object first (Keynote's relative sizing), then edges
        let mut done = false;
        if o.sizing {
            let len = b.len(axis);
            if let Some(s) = others.iter().map(|ob| ob.len(axis) - len).filter(|s| s.abs() <= o.tol).min_by(|a, c| a.abs().total_cmp(&c.abs())) {
                if hi_moves {
                    let v = b.hi(axis) + s;
                    set_hi(&mut b, axis, v);
                } else {
                    let v = b.lo(axis) - s;
                    set_lo(&mut b, axis, v);
                }
                done = true;
            }
        }
        if !done {
            if hi_moves {
                if let Some(s) = snap_val(b.hi(axis), &ts) {
                    let v = b.hi(axis) + s;
                    set_hi(&mut b, axis, v);
                }
            }
            if lo_moves {
                if let Some(s) = snap_val(b.lo(axis), &ts) {
                    let v = b.lo(axis) + s;
                    set_lo(&mut b, axis, v);
                }
            }
        }
    }
    let mut g = Vec::new();
    let eo = Opts { center: o.center, edges: true, ..*o };
    lines(&b, others, slide, &eo, &mut g);
    // keep only the lines touching a moving edge
    g.retain(|gd| match gd {
        Guide::Line(p, q) if p.x == q.x => (l && (p.x - b.x0).abs() < 0.05) || (r && (p.x - b.x1).abs() < 0.05),
        Guide::Line(p, _) => (t && (p.y - b.y0).abs() < 0.05) || (btm && (p.y - b.y1).abs() < 0.05),
        _ => true,
    });
    if o.sizing {
        for axis in 0..2 {
            for ob in others.iter().filter(|ob| (ob.len(axis) - b.len(axis)).abs() < 0.05) {
                g.push(Guide::Size { axis, bx: *ob });
                g.push(Guide::Size { axis, bx: b });
            }
        }
    }
    (b, g)
}

fn set_lo(b: &mut Bx, axis: usize, v: f32) {
    if axis == 0 {
        b.x0 = v.min(b.x1 - 1.0)
    } else {
        b.y0 = v.min(b.y1 - 1.0)
    }
}

fn set_hi(b: &mut Bx, axis: usize, v: f32) {
    if axis == 0 {
        b.x1 = v.max(b.x0 + 1.0)
    } else {
        b.y1 = v.max(b.y0 + 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(x0: f32, y0: f32, x1: f32, y1: f32) -> Bx {
        Bx { x0, y0, x1, y1 }
    }
    const O: Opts = Opts { center: true, edges: false, spacing: true, sizing: true, tol: 5.0 };

    #[test]
    fn centres_snap_to_slide_and_objects() {
        let slide = bx(0.0, 0.0, 1920.0, 1080.0);
        // 3 pt off the slide centre horizontally
        let (d, g) = snap_move(bx(900.0, 100.0, 1026.0, 200.0), &[], slide, (0.0, 0.0), &O);
        assert_eq!(d.0, -3.0);
        assert!(g.iter().any(|g| matches!(g, Guide::Line(p, q) if p.x == 960.0 && q.x == 960.0)));
        // centre to another object's centre
        let other = bx(100.0, 500.0, 300.0, 600.0);
        let (d, _) = snap_move(bx(100.0, 100.0, 296.0, 200.0), &[other], slide, (0.0, 0.0), &O);
        assert_eq!(d.0, 2.0);
    }

    #[test]
    fn edges_only_when_enabled() {
        let slide = bx(0.0, 0.0, 1920.0, 1080.0);
        let (d, _) = snap_move(bx(3.0, 300.0, 103.0, 400.0), &[], slide, (0.0, 0.0), &O);
        assert_eq!(d.0, 0.0);
        let (d, _) = snap_move(bx(3.0, 300.0, 103.0, 400.0), &[], slide, (0.0, 0.0), &Opts { edges: true, ..O });
        assert_eq!(d.0, -3.0);
    }

    #[test]
    fn equal_spacing() {
        let slide = bx(0.0, 0.0, 1920.0, 1080.0);
        let a = bx(0.0, 0.0, 100.0, 100.0);
        let b = bx(200.0, 0.0, 300.0, 100.0);
        // a third box 102 pt right of b: snaps to the same 100 pt gap
        let (d, g) = snap_move(bx(402.0, 0.0, 502.0, 100.0), &[a, b], slide, (0.0, 0.0), &O);
        assert_eq!(d.0, -2.0);
        assert_eq!(g.iter().filter(|g| matches!(g, Guide::Gap { .. })).count(), 2);
    }

    #[test]
    fn resize_matches_size() {
        let slide = bx(0.0, 0.0, 1920.0, 1080.0);
        let other = bx(500.0, 500.0, 700.0, 600.0);
        let (b, g) = snap_resize(bx(0.0, 0.0, 197.0, 50.0), (false, false, true, false), &[other], slide, &O);
        assert_eq!(b.x1, 200.0);
        assert!(g.iter().any(|g| matches!(g, Guide::Size { axis: 0, .. })));
    }
}
