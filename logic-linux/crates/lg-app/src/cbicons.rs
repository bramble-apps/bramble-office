//! Control bar icons drawn to match Logic 13's where the icon font has nothing close
//! (win-tracks-1-instrument-track.png at 2x): Smart Controls (a knob with a pointer under five
//! short rays), the Browser (two stacked rounded squares, a music note on the front one), the
//! Inspector (two toggle switches), Low Latency (a shield with an ×), the Browsers pane (pages
//! with a music note). Outline icons in 1.6 pt strokes, about 19 pt, as Logic's.

use egui::{pos2, vec2, Color32, Pos2, Rect, Stroke};

use crate::glyphs::{self, ph};

const W: f32 = 1.6;

/// Smart Controls: a ring (the knob) with its pointer up and to the left, and five rays over it.
pub fn smart_controls(p: &egui::Painter, r: Rect, c: Color32) {
    let ctr = r.center() + vec2(0.0, 2.0);
    let rad = 5.2;
    p.circle_stroke(ctr, rad, Stroke::new(W, c));
    let a = (-105.0f32).to_radians();
    p.line_segment([ctr, ctr + vec2(a.cos(), a.sin()) * (rad - 1.2)], Stroke::new(W, c));
    // the rays: left, upper left, up, upper right, right
    for deg in [-180.0f32, -135.0, -90.0, -45.0, 0.0] {
        let d = vec2(deg.to_radians().cos(), deg.to_radians().sin());
        p.line_segment([ctr + d * (rad + 2.6), ctr + d * (rad + 4.4)], Stroke::new(W, c));
    }
}

/// The Browser: a back square peeking out above and left of a front square with a music note.
pub fn browser(p: &egui::Painter, r: Rect, c: Color32, fill: Color32) {
    let s = 14.0;
    let back = Rect::from_min_size(r.center() - vec2(s / 2.0 + 2.5, s / 2.0 + 2.5), vec2(s, s));
    let front = back.translate(vec2(4.5, 4.5));
    p.rect_stroke(back, 3.0, Stroke::new(W, c), egui::StrokeKind::Middle);
    // (Logic's front square is solid, its note cut out in the button's colour)
    p.rect_filled(front.expand(1.6), 3.8, fill);
    p.rect_filled(front.expand(W / 2.0), 3.4, c);
    glyphs::paint(p, front, ph::MUSIC_NOTE_SIMPLE, 9.5, fill);
}

/// The Inspector: two toggle switches, the top one an outline with its knob left, the bottom
/// one filled with its knob (cut out) at the right, as Logic's.
pub fn inspector(p: &egui::Painter, r: Rect, c: Color32, cell: Color32) {
    for k in [0.0f32, 1.0] {
        let pill = Rect::from_center_size(pos2(r.center().x, r.center().y - 4.6 + k * 9.2), vec2(18.0, 8.0));
        if k == 0.0 {
            p.rect_stroke(pill, 4.0, Stroke::new(W, c), egui::StrokeKind::Middle);
            p.circle_filled(pos2(pill.min.x + 4.0, pill.center().y), 2.6, c);
        } else {
            p.rect_filled(pill.expand(W / 2.0), 4.8, c);
            p.circle_filled(pos2(pill.max.x - 4.0, pill.center().y), 2.6, cell);
        }
    }
}

/// Low Latency: a shield with an × in it.
pub fn low_latency(p: &egui::Painter, r: Rect, c: Color32) {
    glyphs::paint(p, r, ph::SHIELD, 19.0, c);
    let ctr = r.center() + vec2(0.0, -0.5);
    let d = 2.6;
    p.line_segment([ctr + vec2(-d, -d), ctr + vec2(d, d)], Stroke::new(W, c));
    p.line_segment([ctr + vec2(-d, d), ctr + vec2(d, -d)], Stroke::new(W, c));
}

/// The Browsers pane: pages with a music note at their lower right.
pub fn files(p: &egui::Painter, r: Rect, c: Color32) {
    glyphs::paint(p, r.translate(vec2(-2.5, -1.0)), ph::FILES, 18.0, c);
    let n = Rect::from_center_size(r.center() + vec2(5.0, 4.5), vec2(11.0, 11.0));
    glyphs::paint(p, n, ph::MUSIC_NOTE_SIMPLE, 11.0, c);
}

/// Capture as Recording: a dashed red ring round a red dot.
pub fn capture(p: &egui::Painter, r: Rect, red: Color32) {
    let ctr: Pos2 = r.center();
    let n = 12;
    for k in 0..n {
        let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = a0 + std::f32::consts::TAU / n as f32 * 0.55;
        let pts: Vec<Pos2> = (0..=4).map(|j| {
            let a = a0 + (a1 - a0) * j as f32 / 4.0;
            ctr + vec2(a.cos(), a.sin()) * 8.0
        }).collect();
        p.add(egui::Shape::line(pts, Stroke::new(W, red)));
    }
    p.circle_filled(ctr, 4.2, red);
}

/// Catch as Logic 13's (ui-tracks-with-pianoroll.png): the playhead (a line under a small pin
/// head) with a chevron pointing at it from each side.
pub fn catch(p: &egui::Painter, r: Rect, c: Color32) {
    let ctr = r.center();
    let top = ctr.y - 7.0;
    let bot = ctr.y + 7.0;
    p.line_segment([pos2(ctr.x, top + 3.0), pos2(ctr.x, bot)], Stroke::new(W, c));
    // the pin head: a small trapezoid
    p.add(egui::Shape::convex_polygon(vec![pos2(ctr.x - 3.2, top), pos2(ctr.x + 3.2, top), pos2(ctr.x + 1.6, top + 3.6), pos2(ctr.x - 1.6, top + 3.6)], c, Stroke::NONE));
    for s in [-1.0f32, 1.0] {
        let tip = pos2(ctr.x + s * 2.6, ctr.y + 2.0);
        let back = ctr.x + s * 6.6;
        p.add(egui::Shape::line(vec![pos2(back, tip.y - 3.6), tip, pos2(back, tip.y + 3.6)], Stroke::new(W, c)));
    }
}

/// Show/Hide Automation as Logic's: a flat line into a node, a rise to a second node, a flat
/// line out.
pub fn automation(p: &egui::Painter, r: Rect, c: Color32) {
    let ctr = r.center();
    let a = pos2(ctr.x - 3.8, ctr.y + 3.0);
    let b = pos2(ctr.x + 3.8, ctr.y - 3.0);
    let st = Stroke::new(W, c);
    p.line_segment([pos2(ctr.x - 9.0, a.y), pos2(a.x - 1.8, a.y)], st);
    p.line_segment([a + vec2(1.4, -1.1), b + vec2(-1.4, 1.1)], st);
    p.line_segment([pos2(b.x + 1.8, b.y), pos2(ctr.x + 9.0, b.y)], st);
    p.circle_stroke(a, 1.8, st);
    p.circle_stroke(b, 1.8, st);
}

/// Editors as Logic's: a thin pencil, a single stroke from lower left to upper right with a
/// small point.
pub fn pencil(p: &egui::Painter, r: Rect, c: Color32) {
    let ctr = r.center();
    let a = ctr + vec2(-4.6, 4.6);
    let b = ctr + vec2(6.0, -6.0);
    p.line_segment([a, b], Stroke::new(2.4, c));
    p.add(egui::Shape::convex_polygon(vec![ctr + vec2(-7.2, 7.2), a + vec2(-1.3, -0.4), a + vec2(0.4, 1.3)], c, Stroke::NONE));
}
