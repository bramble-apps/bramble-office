//! Control bar icons drawn to match Logic 13's where the icon font has nothing close
//! (win-tracks-1-instrument-track.png at 2x): Smart Controls (a knob with an upright pointer in seven
//! scale dots), the Browser (two stacked rounded squares, a music note on the front one), the
//! Inspector (two toggle switches), Low Latency (a shield with an ×), the Browsers pane (pages
//! with a music note). Outline icons in 1.6 pt strokes, about 19 pt, as Logic's.

use egui::{pos2, vec2, Color32, Pos2, Rect, Stroke};

use crate::glyphs::{self, ph};

const W: f32 = 1.6;

/// Smart Controls, as Logic 13's: a ring (the knob) with its pointer straight up, and seven
/// dots round it (left, upper left, top, upper right, right, lower left, lower right) like a
/// knob's scale. Ring and pointer in the bar's 1.6 pt stroke, as Studio's other icons.
pub fn smart_controls(p: &egui::Painter, r: Rect, c: Color32) {
    // (measured on Logic's at 2x: ring 4.7 pt, dots 8.6 pt out; the stroke a touch lighter than
    // the bar's 1.6 pt so the knob doesn't look heavier than its neighbours)
    let ctr = r.center() + vec2(0.0, 1.0);
    let (rad, w) = (4.7, 1.4);
    p.circle_stroke(ctr, rad, Stroke::new(w, c));
    p.line_segment([ctr, ctr - vec2(0.0, rad)], Stroke::new(w * 0.85, c));
    for deg in [-90.0f32, -45.0, 0.0, 45.0, 90.0, 135.0, -135.0] {
        // (0 = up, clockwise)
        let a = deg.to_radians();
        p.circle_filled(ctr + vec2(a.sin(), -a.cos()) * 8.6, 0.75, c);
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
    // Traced from Logic 13's icon at 2x, in points: the top (off) toggle is a 15 x 7 pill
    // outline in a 1 pt stroke with a 6 x 3.5 pill knob 2 pt in from its left end; 1 pt below
    // it the bottom (on) toggle is a solid 15 x 6.5 pill with a 6 x 4.5 pill cut out 1 pt in
    // from its right end.
    let top = Rect::from_min_size(pos2(r.center().x - 7.5, r.center().y - 7.25), vec2(15.0, 7.0));
    p.rect_stroke(top, 3.5, Stroke::new(1.0, c), egui::StrokeKind::Inside);
    let knob = Rect::from_min_size(pos2(top.min.x + 2.0, top.center().y - 1.75), vec2(6.0, 3.5));
    p.rect_filled(knob, 1.75, c);
    let bottom = Rect::from_min_size(pos2(top.min.x, top.max.y + 1.0), vec2(15.0, 6.5));
    p.rect_filled(bottom, 3.25, c);
    let cut = Rect::from_min_size(pos2(bottom.max.x - 1.0 - 6.0, bottom.center().y - 2.25), vec2(6.0, 4.5));
    p.rect_filled(cut, 2.25, cell);
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
