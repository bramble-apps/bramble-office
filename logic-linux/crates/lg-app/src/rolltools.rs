//! The Piano Roll's tools as Logic's: the pointer takes the tool's shape over the grid (Pencil,
//! Eraser, Scissors, Join, Brush, Quantize, Velocity, Mute, Solo… each with its hotspot where
//! Logic puts it); the Brush paints along the drag at the Time Quantize steps, following the
//! pointer's pitch (Shift: the pitch it started on); the Eraser deletes the notes it's dragged
//! over; the Pointer's drag on the background is a rubber band.

use egui::{pos2, vec2, Color32, CursorIcon, Pos2, Rect};

use crate::edit::Tool;

/// The pointer over the grid with `tool`: a system cursor where one matches Logic's, else the
/// tool's glyph drawn at the pointer (the system cursor hidden).
pub fn cursor(ctx: &egui::Context, tool: Tool, at: Pos2) {
    // (a cursor already set this frame, e.g. a note's end: keep it)
    let current = ctx.output(|o| o.cursor_icon);
    if !matches!(current, CursorIcon::Default | CursorIcon::PointingHand) {
        return;
    }
    let sys = match tool {
        Tool::Pointer => Some(CursorIcon::Default),
        Tool::Marquee => Some(CursorIcon::Crosshair),
        Tool::Zoom => Some(CursorIcon::ZoomIn),
        Tool::Text => Some(CursorIcon::Text),
        _ => None,
    };
    if let Some(c) = sys {
        ctx.set_cursor_icon(c);
        return;
    }
    ctx.set_cursor_icon(CursorIcon::None);
    let (glyph, hot) = glyph(tool);
    let size = 18.0;
    // the glyph's box placed so its hotspot (a fraction of the box) is on the pointer
    let r = Rect::from_min_size(at - vec2(hot.0 * size, hot.1 * size), vec2(size, size));
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("roll-tool-cursor")));
    // (a dark outline under the white glyph, as macOS draws tool cursors)
    for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
        crate::glyphs::paint(&p, r.translate(d), glyph, size, Color32::BLACK);
    }
    crate::glyphs::paint(&p, r, glyph, size, Color32::WHITE);
}

/// A tool's cursor glyph and its hotspot (fractions of the glyph's box from its top left).
pub fn glyph(tool: Tool) -> (&'static str, (f32, f32)) {
    let g = crate::glyphs::tool(tool);
    let hot = match tool {
        // drawn tip down-left: the tip
        Tool::Pencil | Tool::Eraser | Tool::Brush => (0.1, 0.9),
        // a pointing hand: the fingertip
        Tool::Finger => (0.38, 0.08),
        // centred tools (Scissors' blades, Join, Quantize, Velocity, Mute, Solo…): the middle
        _ => (0.5, 0.5),
    };
    (g, hot)
}

/// The Brush's steps from the last painted one to the pointer's: (tick, row) for each step
/// crossed, the pitch following the pointer (rows interpolated over a fast drag).
pub fn brush_steps(last: Option<(u32, f32)>, at: u32, row: f32, step: u32) -> Vec<(u32, f32)> {
    let step = step.max(1);
    let at = at / step * step;
    let Some((l, lr)) = last else { return vec![(at, row)] };
    if at == l {
        return Vec::new();
    }
    let n = (at as i64 - l as i64).unsigned_abs() / step as u64;
    let dir: i64 = if at > l { step as i64 } else { -(step as i64) };
    (1..=n).map(|k| {
        let t = (l as i64 + dir * k as i64) as u32;
        (t, lr + (row - lr) * k as f32 / n as f32)
    }).collect()
}

/// The notes a rubber band from `a` to `b` touches (each note's rect given).
pub fn band(a: Pos2, b: Pos2, notes: &[(usize, Rect)]) -> Vec<usize> {
    let r = Rect::from_two_pos(a, b);
    notes.iter().filter(|(_, n)| n.intersects(r)).map(|(k, _)| *k).collect()
}

/// The note under `p` (the topmost: the last drawn).
pub fn hit(p: Pos2, notes: &[(usize, Rect)]) -> Option<usize> {
    notes.iter().rev().find(|(_, r)| r.contains(p)).map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brush_follows_the_pitch_across_steps() {
        // first press: the step under the pointer
        assert_eq!(brush_steps(None, 250, 60.0, 240), vec![(240, 60.0)]);
        // the same step again: nothing
        assert!(brush_steps(Some((240, 60.0)), 300, 62.0, 240).is_empty());
        // three steps on in one move, four rows up: each step gets its row
        let v = brush_steps(Some((240, 60.0)), 960, 64.0, 240);
        assert_eq!(v.iter().map(|x| x.0).collect::<Vec<_>>(), vec![480, 720, 960]);
        assert_eq!(v.iter().map(|x| x.1.round() as i32).collect::<Vec<_>>(), vec![61, 63, 64]);
        // backwards too
        assert_eq!(brush_steps(Some((960, 64.0)), 500, 64.0, 240).iter().map(|x| x.0).collect::<Vec<_>>(), vec![720, 480]);
    }

    #[test]
    fn band_and_hit() {
        let n = vec![(0, Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0))), (1, Rect::from_min_size(pos2(50.0, 50.0), vec2(10.0, 10.0)))];
        assert_eq!(band(pos2(5.0, 5.0), pos2(20.0, 20.0), &n), vec![0]);
        assert_eq!(band(pos2(70.0, 70.0), pos2(0.0, 0.0), &n), vec![0, 1]);
        assert_eq!(hit(pos2(55.0, 55.0), &n), Some(1));
        assert_eq!(hit(pos2(30.0, 30.0), &n), None);
    }

    #[test]
    fn tip_tools_put_the_hotspot_at_the_tip() {
        assert_eq!(glyph(Tool::Pencil).1, (0.1, 0.9));
        assert_eq!(glyph(Tool::Scissors).1, (0.5, 0.5));
    }
}
