//! Numbers' own toolbar glyphs, drawn like the Mac's (N2 screenshots): Insert (a frame with
//! selection dots and a plus), Add Category (a list with triangle bullets), Pivot Table (a grid
//! with a turn arrow), Shape (a square over a circle) and Organize (three narrowing lines).
//! Everything else (Format's brush too) uses Keydeck's icons.

use egui::{pos2, vec2, Color32, Pos2, Rect, Shape, Stroke};

/// Draw `id`'s glyph in `r` (a square); false when it has none of its own.
pub fn draw(p: &egui::Painter, r: Rect, id: &str, col: Color32) -> bool {
    let s = r.width();
    let w = (s * 0.075).max(1.3);
    let st = Stroke::new(w, col);
    // a point of the unit square
    let at = |x: f32, y: f32| -> Pos2 { pos2(r.min.x + x * s, r.min.y + y * s) };
    match id {
        "Insert" => {
            // a wide frame broken at two corners, dots there, a plus inside
            let (x0, y0, x1, y1) = (0.04, 0.18, 0.96, 0.84);
            let gap = 0.11;
            p.line_segment([at(x0 + gap, y0), at(x1, y0)], st);
            p.line_segment([at(x1, y0), at(x1, y1 - gap)], st);
            p.line_segment([at(x1 - gap, y1), at(x0, y1)], st);
            p.line_segment([at(x0, y1), at(x0, y0 + gap)], st);
            p.circle_filled(at(x0, y0), w * 0.95, col);
            p.circle_filled(at(x1, y1), w * 0.95, col);
            let c = at(0.5, 0.51);
            let a = s * 0.17;
            p.line_segment([c - vec2(a, 0.0), c + vec2(a, 0.0)], st);
            p.line_segment([c - vec2(0.0, a), c + vec2(0.0, a)], st);
        }
        "Category" => {
            for k in 0..3 {
                let y = 0.26 + k as f32 * 0.24;
                p.add(Shape::convex_polygon(vec![at(0.06, y - 0.075), at(0.2, y), at(0.06, y + 0.075)], col, Stroke::NONE));
                p.line_segment([at(0.34, y), at(0.96, y)], st);
            }
        }
        "Pivot" => {
            // a 2×2 grid at the top left, a two-headed turn arrow under its right
            let g = Rect::from_min_max(at(0.06, 0.04), at(0.62, 0.6));
            p.rect_stroke(g, s * 0.08, st, egui::StrokeKind::Middle);
            p.line_segment([pos2(g.center().x, g.min.y), pos2(g.center().x, g.max.y)], st);
            p.line_segment([pos2(g.min.x, g.center().y), pos2(g.max.x, g.center().y)], st);
            let c = at(0.5, 0.58);
            let rad = s * 0.36;
            let pts: Vec<Pos2> = (0..=12).map(|k| {
                let t = (15.0 + k as f32 * 5.0_f32).to_radians();
                c + vec2(t.cos(), t.sin()) * rad
            }).collect();
            let (a, b) = (pts[0], pts[pts.len() - 1]);
            p.add(Shape::line(pts, st));
            let head = s * 0.12;
            // heads: at the right end pointing up, at the bottom end pointing left
            p.add(Shape::convex_polygon(vec![a + vec2(0.0, -head * 1.1), a + vec2(-head * 0.75, head * 0.2), a + vec2(head * 0.75, head * 0.2)], col, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![b + vec2(-head * 1.1, 0.0), b + vec2(0.2 * head, -head * 0.75), b + vec2(0.2 * head, head * 0.75)], col, Stroke::NONE));
        }
        "Shape" => {
            // a circle behind a rounded square: only the circle's part outside the square shows
            let q = Rect::from_min_max(at(0.36, 0.36), at(0.94, 0.94));
            let (c, rad) = (at(0.36, 0.36), s * 0.3);
            let n = 72;
            let pt = |k: usize| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                c + vec2(a.cos(), a.sin()) * rad
            };
            let outside = |p: Pos2| !q.expand(w * 0.6).contains(p);
            let mut run: Vec<Pos2> = Vec::new();
            for k in 0..=n {
                let p0 = pt(k);
                if outside(p0) {
                    run.push(p0);
                } else if run.len() > 1 {
                    p.add(Shape::line(std::mem::take(&mut run), st));
                } else {
                    run.clear();
                }
            }
            if run.len() > 1 {
                p.add(Shape::line(run, st));
            }
            p.rect_stroke(q, s * 0.1, st, egui::StrokeKind::Middle);
        }
        "Organize" => {
            for (k, half) in [0.42, 0.3, 0.17].iter().enumerate() {
                let y = 0.3 + k as f32 * 0.2;
                p.line_segment([at(0.5 - half, y), at(0.5 + half, y)], Stroke::new(w * 1.05, col));
            }
        }
        _ => return false,
    }
    true
}
