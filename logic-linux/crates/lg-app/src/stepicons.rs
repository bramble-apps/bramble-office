//! The Step Sequencer's own small icons, drawn as Logic 13 draws them (measured on
//! ui-step-sequencer.png, @2x): the icon font has nothing close for these. Each is drawn
//! centred on `c`, in `col`, about 14 pt tall, with Logic's 1.2 pt lines.

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Stroke};

const W: f32 = 1.2;

fn line(p: &Painter, a: Pos2, b: Pos2, col: Color32) {
    p.line_segment([a, b], Stroke::new(W, col));
}

/// "›" (MIDI In's and Live Pattern Recording's arrow; MIDI Out's after the socket).
pub fn chevron(p: &Painter, c: Pos2, col: Color32) {
    p.line(vec![pos2(c.x - 1.5, c.y - 3.5), pos2(c.x + 1.5, c.y), pos2(c.x - 1.5, c.y + 3.5)], Stroke::new(W, col));
}

/// A MIDI (DIN) socket: a ring open at the bottom into a notch, five pins along the top.
pub fn midi_socket(p: &Painter, c: Pos2, col: Color32) {
    let r = 6.3;
    // the ring, from the notch's right edge (bottom right) round the top to its left edge
    let a0 = 62f32.to_radians();
    let span = 304f32.to_radians();
    let n = 32;
    let mut pts: Vec<Pos2> = (0..=n).map(|k| {
        let a = a0 - span * k as f32 / n as f32;
        pos2(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect();
    // the notch: up from the ring's left end, over a small arch, down to its right end
    let top = c.y + 5.4;
    let w = 2.9;
    for k in 0..=10 {
        let a = std::f32::consts::PI * (1.0 - k as f32 / 10.0);
        pts.push(pos2(c.x + w * a.cos(), top - 2.6 * a.sin()));
    }
    pts.push(pts[0]);
    p.add(egui::Shape::line(pts, Stroke::new(W, col)));
    // the pins: on an arc above the centre
    for k in 0..5 {
        let a = (200.0f32 + 35.0 * k as f32).to_radians();
        p.circle_filled(pos2(c.x + 3.7 * a.cos(), c.y + 0.4 + 3.7 * a.sin()), 0.85, col);
    }
}

/// MIDI In (›socket) or MIDI Out (socket›).
pub fn midi(p: &Painter, c: Pos2, out: bool, col: Color32) {
    if out {
        midi_socket(p, pos2(c.x - 2.5, c.y), col);
        chevron(p, pos2(c.x + 7.5, c.y), col);
    } else {
        chevron(p, pos2(c.x - 7.5, c.y), col);
        midi_socket(p, pos2(c.x + 2.5, c.y), col);
    }
}

/// Pattern Browser: a square behind a square of a 4 × 3 grid.
pub fn browser(p: &Painter, c: Pos2, col: Color32, bg: Color32) {
    let back = Rect::from_min_size(pos2(c.x - 6.8, c.y - 6.8), vec2(10.5, 10.5));
    p.rect_stroke(back, 2.2, Stroke::new(W, col), egui::StrokeKind::Middle);
    let front = Rect::from_min_size(pos2(c.x - 4.2, c.y - 4.2), vec2(11.0, 11.0));
    p.rect_filled(front.expand(1.2), 2.8, bg);
    p.rect_stroke(front, 2.2, Stroke::new(W, col), egui::StrokeKind::Middle);
    for i in 0..4 {
        for j in 0..4 {
            let q = pos2(front.min.x + 1.75 + i as f32 * 2.0, front.min.y + 1.75 + j as f32 * 2.0);
            p.rect_filled(Rect::from_min_size(q, vec2(1.5, 1.5)), 0.0, col);
        }
    }
}

/// The local Inspector: two switches, the top one off (knob left), the bottom one on.
pub fn inspector(p: &Painter, c: Pos2, col: Color32, bg: Color32) {
    let top = Rect::from_center_size(pos2(c.x, c.y - 3.6), vec2(13.0, 6.2));
    p.rect_stroke(top, 3.1, Stroke::new(W, col), egui::StrokeKind::Middle);
    p.circle_filled(pos2(top.min.x + 3.3, top.center().y), 2.0, col);
    let bot = Rect::from_center_size(pos2(c.x, c.y + 3.6), vec2(13.6, 6.8));
    p.rect_filled(bot, 3.4, col);
    p.circle_filled(pos2(bot.max.x - 3.4, bot.center().y), 2.1, bg);
}

/// Mono Mode: an × above a small square.
pub fn mono(p: &Painter, c: Pos2, col: Color32) {
    let (x0, x1, y0, y1) = (c.x - 2.6, c.x + 2.6, c.y - 7.0, c.y - 1.8);
    line(p, pos2(x0, y0), pos2(x1, y1), col);
    line(p, pos2(x0, y1), pos2(x1, y0), col);
    p.rect_stroke(Rect::from_min_size(pos2(c.x - 2.6, c.y + 1.6), vec2(5.2, 5.2)), 0.6, Stroke::new(W, col), egui::StrokeKind::Middle);
}

/// Live Pattern Recording: › and a circled R.
pub fn live(p: &Painter, c: Pos2, col: Color32) {
    chevron(p, pos2(c.x - 7.5, c.y), col);
    let o = pos2(c.x + 2.5, c.y);
    p.circle_stroke(o, 6.3, Stroke::new(W, col));
    p.text(pos2(o.x + 0.2, o.y + 0.3), egui::Align2::CENTER_CENTER, "R", egui::FontId::proportional(9.0), col);
}

/// Preview: a speaker and a dot.
pub fn preview(p: &Painter, c: Pos2, col: Color32) {
    let x = c.x - 2.0;
    let pts = vec![pos2(x - 5.0, c.y - 2.4), pos2(x - 2.2, c.y - 2.4), pos2(x + 1.8, c.y - 6.0), pos2(x + 1.8, c.y + 6.0), pos2(x - 2.2, c.y + 2.4), pos2(x - 5.0, c.y + 2.4)];
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    p.circle_filled(pos2(x + 5.2, c.y), 1.4, col);
}

/// Vertical Auto Zoom: ↕ between two lines.
pub fn auto_zoom(p: &Painter, c: Pos2, col: Color32) {
    line(p, pos2(c.x - 6.0, c.y - 7.0), pos2(c.x + 6.0, c.y - 7.0), col);
    line(p, pos2(c.x - 6.0, c.y + 7.0), pos2(c.x + 6.0, c.y + 7.0), col);
    line(p, pos2(c.x, c.y - 4.6), pos2(c.x, c.y + 4.6), col);
    p.line(vec![pos2(c.x - 2.4, c.y - 2.4), pos2(c.x, c.y - 4.8), pos2(c.x + 2.4, c.y - 2.4)], Stroke::new(W, col));
    p.line(vec![pos2(c.x - 2.4, c.y + 2.4), pos2(c.x, c.y + 4.8), pos2(c.x + 2.4, c.y + 2.4)], Stroke::new(W, col));
}

/// A double arrow, vertical (↕, the zoom slider's) or horizontal (↔, before the step widths).
pub fn double_arrow(p: &Painter, c: Pos2, vertical: bool, col: Color32) {
    let f = |dx: f32, dy: f32| if vertical { pos2(c.x + dy, c.y + dx) } else { pos2(c.x + dx, c.y + dy) };
    line(p, f(-6.5, 0.0), f(6.5, 0.0), col);
    p.line(vec![f(-3.5, -3.3), f(-6.8, 0.0), f(-3.5, 3.3)], Stroke::new(W, col));
    p.line(vec![f(3.5, -3.3), f(6.8, 0.0), f(3.5, 3.3)], Stroke::new(W, col));
}

/// Step Width: the minimum (a narrow upright box) or the maximum (a wide one).
pub fn step_width(p: &Painter, c: Pos2, wide: bool, col: Color32) {
    let s = if wide { vec2(17.0, 12.0) } else { vec2(9.5, 14.0) };
    p.rect_stroke(Rect::from_center_size(c, s), 2.0, Stroke::new(W, col), egui::StrokeKind::Middle);
}

/// Rotate: ◂▯ (left) or ▯▸ (right).
pub fn rotate(p: &Painter, c: Pos2, right: bool, col: Color32) {
    let s = if right { -1.0 } else { 1.0 };
    let box_c = pos2(c.x + s * 1.9, c.y);
    p.rect_stroke(Rect::from_center_size(box_c, vec2(4.4, 9.4)), 1.6, Stroke::new(1.0, col), egui::StrokeKind::Middle);
    let tx = c.x - s * 2.3;
    p.add(egui::Shape::convex_polygon(vec![pos2(tx - s * 2.8, c.y), pos2(tx, c.y - 2.8), pos2(tx, c.y + 2.8)], col, Stroke::NONE));
}

/// Increment (⌃) / Decrement (⌄): a wide chevron.
pub fn caret(p: &Painter, c: Pos2, up: bool, col: Color32) {
    let d = if up { -1.0 } else { 1.0 };
    p.line(vec![pos2(c.x - 4.6, c.y - d * 2.3), pos2(c.x, c.y + d * 2.3), pos2(c.x + 4.6, c.y - d * 2.3)], Stroke::new(W, col));
}

/// Randomize: a die showing five.
pub fn dice(p: &Painter, c: Pos2, col: Color32) {
    let r = Rect::from_center_size(c, vec2(12.5, 12.5));
    p.rect_stroke(r, 2.4, Stroke::new(W * 1.15, col), egui::StrokeKind::Middle);
    for (dx, dy) in [(-3.1, -3.1), (3.1, -3.1), (0.0, 0.0), (-3.1, 3.1), (3.1, 3.1)] {
        p.circle_filled(pos2(c.x + dx, c.y + dy), 1.05, col);
    }
}

/// Reset (clear): a square with an × in it.
pub fn clear(p: &Painter, c: Pos2, col: Color32) {
    let r = Rect::from_center_size(c, vec2(11.5, 11.5));
    p.rect_stroke(r, 1.4, Stroke::new(W, col), egui::StrokeKind::Middle);
    line(p, pos2(c.x - 2.8, c.y - 2.8), pos2(c.x + 2.8, c.y + 2.8), col);
    line(p, pos2(c.x - 2.8, c.y + 2.8), pos2(c.x + 2.8, c.y - 2.8), col);
}

/// A drum row's kit piece, by the drum's name: kick, snare, tom, hi-hat, cymbal, clap, bell,
/// shaker; else a note (♩). Drawn in `col` inside the row's round icon (radius about 14).
pub fn kit_piece(p: &Painter, c: Pos2, name: &str, col: Color32) {
    let n = name.to_lowercase();
    let st = Stroke::new(1.3, col);
    let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
    if has(&["kick", "bass drum", "bd"]) {
        // the front skin: two rings
        p.circle_stroke(c, 6.5, st);
        p.circle_stroke(c, 2.2, st);
    } else if has(&["hat", "hh"]) {
        // two cymbals on a stand
        for dy in [-3.6, -1.2] {
            p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 7.0, c.y + dy + 1.0), pos2(c.x, c.y + dy - 1.0), pos2(c.x + 7.0, c.y + dy + 1.0)], col, Stroke::NONE));
        }
        p.line_segment([pos2(c.x, c.y - 4.0), pos2(c.x, c.y + 7.0)], st);
    } else if has(&["crash", "ride", "china", "splash", "cymbal", "metal"]) {
        p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 7.5, c.y - 1.0), pos2(c.x, c.y - 3.6), pos2(c.x + 7.5, c.y - 1.0)], col, Stroke::NONE));
        p.line_segment([pos2(c.x, c.y - 3.0), pos2(c.x, c.y + 7.0)], st);
    } else if has(&["snare", "rim", "stick", "tom", "perc", "conga", "bongo", "timbale"]) {
        // a drum from the side: top ellipse and the shell; toms deeper than a snare
        let h = if has(&["tom", "conga", "bongo"]) { 8.0 } else { 5.0 };
        let top = c.y - h / 2.0;
        let rx = if has(&["bongo"]) { 5.0 } else { 7.0 };
        let ell: Vec<Pos2> = (0..=24).map(|k| {
            let a = std::f32::consts::TAU * k as f32 / 24.0;
            pos2(c.x + rx * a.cos(), top + 2.0 * a.sin())
        }).collect();
        p.add(egui::Shape::closed_line(ell, st));
        let bottom: Vec<Pos2> = (0..=12).map(|k| {
            let a = std::f32::consts::PI * k as f32 / 12.0;
            pos2(c.x + rx * a.cos(), top + h + 2.0 * a.sin())
        }).collect();
        p.add(egui::Shape::line(bottom, st));
        p.line_segment([pos2(c.x - rx, top), pos2(c.x - rx, top + h)], st);
        p.line_segment([pos2(c.x + rx, top), pos2(c.x + rx, top + h)], st);
    } else if has(&["clap"]) {
        crate::glyphs::paint(p, Rect::from_center_size(c, vec2(16.0, 16.0)), crate::glyphs::ph_fill::HANDS_CLAPPING, 15.0, col);
    } else if has(&["bell", "agogo", "triangle", "chime"]) {
        crate::glyphs::paint(p, Rect::from_center_size(c, vec2(16.0, 16.0)), crate::glyphs::ph_fill::BELL_SIMPLE, 14.0, col);
    } else if has(&["shak", "maraca", "cabasa", "tamb", "clave", "block", "guiro"]) {
        p.circle_filled(pos2(c.x - 1.5, c.y - 2.5), 4.0, col);
        p.line_segment([pos2(c.x + 0.5, c.y), pos2(c.x + 4.5, c.y + 6.0)], Stroke::new(2.0, col));
    } else {
        p.text(c, egui::Align2::CENTER_CENTER, "♩", crate::look::regular(15.0), col);
    }
}
