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
    // (as Logic's at 2x: two 15 pt toggles with a clear gap between them; pill-shaped knobs
    // inside each toggle with an even margin)
    let (w, h, gap) = (15.0f32, 6.0f32, 2.6f32);
    for k in [0.0f32, 1.0] {
        let cy = r.center().y + (k - 0.5) * (h + W + gap);
        let pill = Rect::from_center_size(pos2(r.center().x, cy), vec2(w, h));
        if k == 0.0 {
            // off: an outline, its knob at the left
            let sw = 1.3;
            p.rect_stroke(pill, h / 2.0, Stroke::new(sw, c), egui::StrokeKind::Middle);
            // (a pill-shaped knob: rounded ends, with an even gap to the outline)
            let kh = h - sw - 1.6;
            let knob = Rect::from_min_size(pos2(pill.min.x + sw / 2.0 + 0.8, cy - kh / 2.0), vec2(6.6, kh));
            p.rect_filled(knob, kh / 2.0, c);
        } else {
            // on: filled, its knob (cut out) at the right
            p.rect_filled(pill.expand(W / 2.0), h / 2.0 + W / 2.0, c);
            let kh = h - 1.4;
            let knob = Rect::from_min_size(pos2(pill.max.x - 0.7 - 6.6, cy - kh / 2.0), vec2(6.6, kh));
            p.rect_filled(knob, kh / 2.0, cell);
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

/// How far the Metronome's arm swings each way (radians): as far as the icon font's metronome
/// leans.
pub const SWING: f32 = 0.733;

/// The Metronome arm's angle (radians from upright, positive leaning right) `beats` beats into
/// playback: one swing per beat, like a pendulum (a cosine), at the end of a swing exactly on each
/// beat (left on even beats, right on odd ones) and upright halfway between. Beats come from the
/// playhead's ticks, so the swing follows every tempo change.
pub fn metronome_angle(beats: f64) -> f32 {
    (-(SWING as f64) * (std::f64::consts::PI * beats).cos()) as f32
}

/// The Metronome as the icon font's (`ph::METRONOME`), drawn so its arm can swing: the tapered
/// body with rounded corners, the band across its foot, and the arm pivoting at the band's middle,
/// `angle` radians from upright (0 = at rest). `size` is the glyph size it stands in for.
pub fn metronome(p: &egui::Painter, r: Rect, size: f32, c: Color32, angle: f32) {
    // (measured from the glyph drawn 380 px tall: its ink box centre is (183, 210.5))
    let k = size / 380.0;
    let ctr = r.center();
    let at = |x: f32, y: f32| ctr + vec2((x - 183.0) * k, (y - 210.5) * k);
    let w = 24.0 * k;
    let s = Stroke::new(w, c);
    // body: the trapezoid, its corners rounded
    let corners = [at(143.0, 79.5), at(223.0, 79.5), at(306.0, 340.5), at(60.0, 340.5)];
    let rad = 9.0 * k;
    let mut pts = Vec::new();
    for i in 0..4 {
        let (prev, v, next) = (corners[(i + 3) % 4], corners[i], corners[(i + 1) % 4]);
        let a = v + (prev - v).normalized() * rad;
        let b = v + (next - v).normalized() * rad;
        for j in 0..=4 {
            let t = j as f32 / 4.0;
            pts.push(pos2(
                (1.0 - t) * (1.0 - t) * a.x + 2.0 * (1.0 - t) * t * v.x + t * t * b.x,
                (1.0 - t) * (1.0 - t) * a.y + 2.0 * (1.0 - t) * t * v.y + t * t * b.y,
            ));
        }
    }
    p.add(egui::Shape::closed_line(pts, s));
    // the band (side to side at the pivot's height)
    let side = |y: f32| 0.317 * (y - 92.0);
    p.line_segment([at(139.0 - side(269.5), 269.5), at(227.0 + side(269.5), 269.5)], s);
    // the arm, with a round end
    let pivot = at(183.0, 269.5);
    let tip = pivot + vec2(angle.sin(), -angle.cos()) * (152.0 * k);
    p.line_segment([pivot, tip], s);
    p.circle_filled(tip, w / 2.0, c);
}

#[cfg(test)]
mod metronome_tests {
    use super::*;

    #[test]
    fn the_arm_ends_each_swing_on_the_beat() {
        // on every beat: at the end of a swing, alternating sides
        for b in 0..8 {
            let a = metronome_angle(b as f64);
            assert!((a.abs() - SWING).abs() < 1e-5, "beat {b}: {a}");
            assert_eq!(a > 0.0, b % 2 == 1, "beat {b} swings to the {} side", if a > 0.0 { "right" } else { "left" });
        }
        // halfway between beats: upright, moving fastest
        for b in 0..8 {
            assert!(metronome_angle(b as f64 + 0.5).abs() < 1e-5);
        }
    }

    #[test]
    fn the_swing_eases_like_a_pendulum() {
        // a quarter of the way through a swing it has covered 1 - cos 45° of the way out
        let a = metronome_angle(0.25);
        assert!((a - (-SWING * std::f32::consts::FRAC_1_SQRT_2)).abs() < 1e-5);
        // slow near the ends, fast in the middle
        let near_end = (metronome_angle(0.1) - metronome_angle(0.0)).abs();
        let middle = (metronome_angle(0.55) - metronome_angle(0.45)).abs();
        assert!(middle > 5.0 * near_end);
        // the same beat position gives the same angle (it depends on beats, not seconds, so a
        // tempo change only changes how fast the beats come)
        assert_eq!(metronome_angle(3.75), metronome_angle(3.75 + 2.0));
        assert!(metronome_angle(-1.0) > 0.0);
    }

    #[test]
    fn beats_from_the_playhead() {
        let ppq = lg_core::events::PPQ as f64;
        assert_eq!(super::beats_at(ppq * 3.0, 4), 3.0);
        assert_eq!(super::beats_at(ppq * 3.0, 8), 6.0);
    }
}

/// Beats `ticks` into the project, counting the signature's beat unit (a quarter in 4/4, an
/// eighth in 6/8), as the metronome clicks.
pub fn beats_at(ticks: f64, denominator: u32) -> f64 {
    ticks / (lg_core::events::PPQ as f64 * 4.0 / denominator.max(1) as f64)
}
