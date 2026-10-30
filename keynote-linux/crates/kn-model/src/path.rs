//! Shape outlines from TSD.PathSourceArchive, in frame-local coordinates (0..w, 0..h).

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use kn_format::Msg;

use crate::types::*;

fn pt(m: &Msg) -> Point {
    Point::new(m.f32("x").unwrap_or(0.0), m.f32("y").unwrap_or(0.0))
}

fn size(m: Option<&Msg>) -> Option<Size> {
    let m = m?;
    Some(Size { w: m.f32("width").unwrap_or(0.0), h: m.f32("height").unwrap_or(0.0) })
}

/// TSP.Path -> Path (raw coordinates).
pub fn from_tsp_path(m: &Msg) -> Path {
    let mut p = Path::default();
    for e in m.msgs("elements") {
        let pts: Vec<Point> = e.msgs("points").map(pt).collect();
        let el = match (e.enum_name("type"), pts.as_slice()) {
            (Some("moveTo"), [a, ..]) => PathEl::Move(*a),
            (Some("lineTo"), [a, ..]) => PathEl::Line(*a),
            (Some("quadCurveTo"), [a, b, ..]) => PathEl::Quad(*a, *b),
            (Some("curveTo"), [a, b, c, ..]) => PathEl::Cubic(*a, *b, *c),
            (Some("closeSubpath"), _) => PathEl::Close,
            _ => continue,
        };
        p.els.push(el);
    }
    // a trailing lone moveTo (Keynote appends one after closing) draws nothing
    if matches!(p.els.last(), Some(PathEl::Move(_))) && p.els.len() > 1 {
        p.els.pop();
    }
    p
}

/// Outline of a path source scaled to a frame of size `w` x `h`.
pub fn from_path_source(ps: &Msg, w: f32, h: f32) -> Path {
    if let Some(b) = ps.msg("bezier_path_source") {
        return bezier(b, w, h);
    }
    if let Some(c) = ps.msg("connection_line_path_source") {
        if let Some(b) = c.msg("super") {
            let mut path = from_tsp_path(b.msg("path").unwrap_or(&Msg::untyped()));
            let quadratic = c.enum_name("type") != Some("kTSDConnectionLineTypeOrthogonal");
            // stored as 3 points: start, control (the "bend" point the curve passes through), end
            let pts: Vec<Point> = path
                .els
                .iter()
                .filter_map(|e| match e {
                    PathEl::Move(p) | PathEl::Line(p) => Some(*p),
                    _ => None,
                })
                .collect();
            if pts.len() == 3 {
                path = Path::default();
                let (a, m, b) = (pts[0], pts[1], pts[2]);
                path.els.push(PathEl::Move(a));
                if quadratic {
                    // control point so the curve passes through m at t = 0.5
                    let c = Point::new(2.0 * m.x - (a.x + b.x) / 2.0, 2.0 * m.y - (a.y + b.y) / 2.0);
                    path.els.push(PathEl::Quad(c, b));
                } else {
                    path.els.push(PathEl::Line(Point::new(m.x, a.y)));
                    path.els.push(PathEl::Line(Point::new(m.x, b.y)));
                    path.els.push(PathEl::Line(b));
                }
            }
            let ns = size(b.msg("naturalSize")).unwrap_or(Size { w, h });
            return fit_natural(path, ns, w, h);
        }
    }
    if let Some(p) = ps.msg("point_path_source") {
        let ns = size(p.msg("naturalSize")).unwrap_or(Size { w, h });
        let point = p.msg("point").map(pt).unwrap_or_default();
        let path = match p.enum_name("type") {
            Some("kTSDRightSingleArrow") => arrow(ns, point, false, false),
            Some("kTSDLeftSingleArrow") => arrow(ns, point, false, true),
            Some("kTSDDoubleArrow") => arrow(ns, point, true, false),
            Some("kTSDStar") => star(ns, point.x.round().max(3.0) as u32, point.y),
            Some("kTSDPlus") => plus(ns, point),
            _ => Path::rect(ns.w, ns.h),
        };
        return fit_natural(path, ns, w, h);
    }
    if let Some(s) = ps.msg("scalar_path_source") {
        let ns = size(s.msg("naturalSize")).unwrap_or(Size { w, h });
        let v = s.f32("scalar").unwrap_or(0.0);
        let path = match s.enum_name("type") {
            Some("kTSDRegularPolygon") => polygon(ns, v.round().max(3.0) as u32),
            Some("kTSDChevron") => chevron(ns, v),
            _ => rounded_rect(ns.w, ns.h, v),
        };
        return fit_natural(path, ns, w, h);
    }
    if let Some(c) = ps.msg("callout_path_source") {
        let ns = size(c.msg("natural_size")).unwrap_or(Size { w, h });
        let path = callout(
            ns,
            c.msg("tail_position").map(pt).unwrap_or(Point::new(0.0, ns.h * 1.2)),
            c.f32("tail_size").unwrap_or(10.0),
            c.f32("corner_radius").unwrap_or(0.0),
            c.bool("center_tail").unwrap_or(false),
        );
        return fit_natural(path, ns, w, h);
    }
    if let Some(e) = ps.msg("editable_bezier_path_source") {
        let ns = size(e.msg("naturalSize")).unwrap_or(Size { w, h });
        return fit_natural(editable(e), ns, w, h);
    }
    Path::rect(w, h)
}

/// The path of a path source in its own stored coordinates (no fitting to a frame); used for
/// motion paths of Move actions, whose points are offsets in slide points.
pub fn raw_path_source(ps: &Msg) -> Option<Path> {
    if let Some(e) = ps.msg("editable_bezier_path_source") {
        return Some(editable(e));
    }
    let b = ps.msg("bezier_path_source")?;
    Some(from_tsp_path(b.msg("path")?))
}

fn fit_natural(p: Path, ns: Size, w: f32, h: f32) -> Path {
    let sx = if ns.w > 0.0 { w / ns.w } else { 1.0 };
    let sy = if ns.h > 0.0 { h / ns.h } else { 1.0 };
    p.scale(sx, sy)
}

/// Bezier path: the stored path is scaled so its bounding box matches naturalSize (as libetonyek does),
/// then naturalSize maps onto the frame.
fn bezier(b: &Msg, w: f32, h: f32) -> Path {
    let path = b.msg("path").map(from_tsp_path).unwrap_or_default();
    let ns = size(b.msg("naturalSize")).unwrap_or(Size { w, h });
    let Some((lo, hi)) = path.curve_bounds() else { return path };
    let (bw, bh) = (hi.x - lo.x, hi.y - lo.y);
    let sx = if bw > 1e-3 { ns.w / bw } else { 1.0 };
    let sy = if bh > 1e-3 { ns.h / bh } else { 1.0 };
    let path = path.scale(sx, sy);
    fit_natural(path, ns, w, h)
}

fn editable(e: &Msg) -> Path {
    let mut path = Path::default();
    for sp in e.msgs("subpaths") {
        let nodes: Vec<(Point, Point, Point)> = sp
            .msgs("nodes")
            .map(|n| {
                let c = n.msg("nodePoint").map(pt).unwrap_or_default();
                let i = n.msg("inControlPoint").map(pt).unwrap_or(c);
                let o = n.msg("outControlPoint").map(pt).unwrap_or(c);
                (i, c, o)
            })
            .collect();
        if nodes.is_empty() {
            continue;
        }
        let closed = sp.bool("closed").unwrap_or(false);
        path.els.push(PathEl::Move(nodes[0].1));
        let n = nodes.len();
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs {
            let a = nodes[i];
            let b = nodes[(i + 1) % n];
            if a.2 == a.1 && b.0 == b.1 {
                path.els.push(PathEl::Line(b.1));
            } else {
                path.els.push(PathEl::Cubic(a.2, b.0, b.1));
            }
        }
        if closed {
            path.close();
        }
    }
    path
}

/// Kappa for approximating quarter circles with cubics.
const K: f32 = 0.552_284_8;

pub fn ellipse(x: f32, y: f32, w: f32, h: f32) -> Path {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let mut p = Path::default();
    p.move_to(cx + rx, cy);
    p.cubic_to(cx + rx, cy + ry * K, cx + rx * K, cy + ry, cx, cy + ry);
    p.cubic_to(cx - rx * K, cy + ry, cx - rx, cy + ry * K, cx - rx, cy);
    p.cubic_to(cx - rx, cy - ry * K, cx - rx * K, cy - ry, cx, cy - ry);
    p.cubic_to(cx + rx * K, cy - ry, cx + rx, cy - ry * K, cx + rx, cy);
    p.close();
    p
}

pub fn rounded_rect(w: f32, h: f32, r: f32) -> Path {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    if r <= 0.0 {
        return Path::rect(w, h);
    }
    let k = r * (1.0 - K);
    let mut p = Path::default();
    p.move_to(r, 0.0);
    p.line_to(w - r, 0.0);
    p.cubic_to(w - k, 0.0, w, k, w, r);
    p.line_to(w, h - r);
    p.cubic_to(w, h - k, w - k, h, w - r, h);
    p.line_to(r, h);
    p.cubic_to(k, h, 0.0, h - k, 0.0, h - r);
    p.line_to(0.0, r);
    p.cubic_to(0.0, k, k, 0.0, r, 0.0);
    p.close();
    p
}

/// Arrow: `point.x` = head length (points), `point.y` = fraction of the height above the shaft.
fn arrow(ns: Size, point: Point, double: bool, left: bool) -> Path {
    let (w, h) = (ns.w, ns.h);
    let head = point.x.clamp(0.0, if double { w / 2.0 } else { w });
    let t = point.y.clamp(0.0, 0.5) * h;
    let mut p = Path::default();
    if double {
        p.move_to(0.0, h / 2.0);
        p.line_to(head, 0.0);
        p.line_to(head, t);
        p.line_to(w - head, t);
        p.line_to(w - head, 0.0);
        p.line_to(w, h / 2.0);
        p.line_to(w - head, h);
        p.line_to(w - head, h - t);
        p.line_to(head, h - t);
        p.line_to(head, h);
        p.close();
    } else {
        p.move_to(0.0, t);
        p.line_to(w - head, t);
        p.line_to(w - head, 0.0);
        p.line_to(w, h / 2.0);
        p.line_to(w - head, h);
        p.line_to(w - head, h - t);
        p.line_to(0.0, h - t);
        p.close();
        if left {
            p = p.map(|q| Point::new(w - q.x, q.y));
        }
    }
    p
}

/// Points on a regular polygon/star, first one at the top, stretched so the outline fills w x h.
fn fill_box(pts: Vec<Point>, w: f32, h: f32) -> Path {
    let minx = pts.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let maxx = pts.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
    let miny = pts.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let maxy = pts.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
    let sx = w / (maxx - minx).max(1e-6);
    let sy = h / (maxy - miny).max(1e-6);
    let mut p = Path::default();
    for (i, q) in pts.iter().enumerate() {
        let (x, y) = ((q.x - minx) * sx, (q.y - miny) * sy);
        if i == 0 {
            p.move_to(x, y);
        } else {
            p.line_to(x, y);
        }
    }
    p.close();
    p
}

fn polygon(ns: Size, sides: u32) -> Path {
    let pts = (0..sides)
        .map(|i| {
            let a = -FRAC_PI_2 + TAU * i as f32 / sides as f32;
            Point::new(a.cos(), a.sin())
        })
        .collect();
    fill_box(pts, ns.w, ns.h)
}

fn star(ns: Size, points: u32, inner: f32) -> Path {
    let n = points * 2;
    let pts = (0..n)
        .map(|i| {
            let a = -FRAC_PI_2 + PI * i as f32 / points as f32;
            let r = if i % 2 == 0 { 1.0 } else { inner.clamp(0.05, 1.0) };
            Point::new(a.cos() * r, a.sin() * r)
        })
        .collect();
    // Keynote keeps the star's proportions within the frame horizontally, but fills the height.
    fill_box(pts, ns.w, ns.h)
}

fn plus(ns: Size, point: Point) -> Path {
    let (w, h) = (ns.w, ns.h);
    let t = point.x.clamp(0.0, w.min(h) / 2.0);
    let (x0, x1) = (w / 2.0 - t, w / 2.0 + t);
    let (y0, y1) = (h / 2.0 - t, h / 2.0 + t);
    let mut p = Path::default();
    p.move_to(x0, 0.0);
    p.line_to(x1, 0.0);
    p.line_to(x1, y0);
    p.line_to(w, y0);
    p.line_to(w, y1);
    p.line_to(x1, y1);
    p.line_to(x1, h);
    p.line_to(x0, h);
    p.line_to(x0, y1);
    p.line_to(0.0, y1);
    p.line_to(0.0, y0);
    p.line_to(x0, y0);
    p.close();
    p
}

fn chevron(ns: Size, v: f32) -> Path {
    let (w, h) = (ns.w, ns.h);
    let d = v.clamp(0.0, w);
    let mut p = Path::default();
    p.move_to(0.0, 0.0);
    p.line_to(w - d, 0.0);
    p.line_to(w, h / 2.0);
    p.line_to(w - d, h);
    p.line_to(0.0, h);
    p.line_to(d, h / 2.0);
    p.close();
    p
}

/// Callout / quote bubble: rounded body filling the natural size plus a tail to `tip`.
fn callout(ns: Size, tip: Point, tail_size: f32, radius: f32, center_tail: bool) -> Path {
    let (w, h) = (ns.w, ns.h);
    let r = radius.min(w.min(h) / 2.0);
    let mut body = if r >= w.min(h) / 2.0 - 0.5 && (w - h).abs() > 1.0 && radius > w.min(h) / 2.0 {
        ellipse(0.0, 0.0, w, h)
    } else {
        rounded_rect(w, h, r)
    };
    let inside = tip.x >= 0.0 && tip.x <= w && tip.y >= 0.0 && tip.y <= h;
    if inside || tail_size <= 0.0 {
        return body;
    }
    let c = Point::new(w / 2.0, h / 2.0);
    let (dx, dy) = (tip.x - c.x, tip.y - c.y);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (nx, ny) = (-dy / len, dx / len);
    // base of the tail: the centre for "center tail", otherwise just inside the edge facing the tip
    let base = if center_tail {
        c
    } else {
        let tx = if dx.abs() > 1e-3 { (w / 2.0) / dx.abs() } else { f32::INFINITY };
        let ty = if dy.abs() > 1e-3 { (h / 2.0) / dy.abs() } else { f32::INFINITY };
        let t = tx.min(ty) * 0.9;
        Point::new(c.x + dx * t, c.y + dy * t)
    };
    let half = tail_size.max(2.0);
    let mut tail = Path::default();
    // same winding as the body (clockwise in y-down) so a nonzero fill unions them
    tail.move_to(base.x - nx * half, base.y - ny * half);
    tail.line_to(tip.x, tip.y);
    tail.line_to(base.x + nx * half, base.y + ny * half);
    tail.close();
    body.els.extend(tail.els);
    body
}
