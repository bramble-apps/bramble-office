//! The Piano Roll's tools as Logic's: the pointer takes the tool's shape over the grid (Pencil,
//! Eraser, Scissors, Join, Brush, Quantize, Velocity, Mute, Solo… each with its hotspot where
//! Logic puts it); the Brush paints along the drag at the Time Quantize steps, following the
//! pointer's pitch (Shift: the pitch it started on); the Eraser deletes the notes it's dragged
//! over; the Pointer's drag on the background is a rubber band.

use egui::{pos2, vec2, Color32, CursorIcon, Pos2, Rect};

use crate::edit::Tool;

/// The pointer over the grid with `tool`: a system cursor where one matches Logic's, else the
/// tool's glyph drawn at the pointer (the system cursor hidden), its tip on the pointer.
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
    let fam = crate::glyphs::family();
    if !ctx.fonts(|f| f.families().contains(&fam)) {
        return;
    }
    ctx.set_cursor_icon(CursorIcon::None);
    let shape = shape(tool);
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("roll-tool-cursor")));
    let galley = p.layout_no_wrap(shape.glyph.to_string(), egui::FontId::new(CURSOR_SIZE, fam), Color32::WHITE);
    let pos = at - tip_offset(ink(&galley), shape);
    // (the Eraser: the line under it in the font, left of the tip once turned, left out)
    let p = if tool == Tool::Eraser { p.with_clip_rect(Rect::everything_right_of(at.x - 1.5)) } else { p };
    // (a dark outline under the white glyph, as macOS draws tool cursors)
    for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
        p.add(egui::epaint::TextShape::new(pos + d, galley.clone(), Color32::BLACK).with_override_text_color(Color32::BLACK).with_angle(shape.angle));
    }
    p.add(egui::epaint::TextShape::new(pos, galley, Color32::WHITE).with_angle(shape.angle));
}

/// The tool cursors' glyph size (points).
pub const CURSOR_SIZE: f32 = 18.0;

/// How a tool's cursor is drawn: its glyph, turned by `angle` (radians, clockwise), with the
/// hotspot at `tip` (a fraction of the glyph's drawn shape, before turning).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub glyph: &'static str,
    pub angle: f32,
    pub tip: (f32, f32),
}

/// A tool's cursor. The pointer-like tools point to the top left as a mouse pointer does, the
/// click landing on their tip: the Pencil, Brush and Eraser (drawn tip down-left: turned a
/// quarter right), the Finger (pointing up: turned 45° left), the Scissors (blades to the right:
/// turned 135° left; the point between the blade tips) and the Join tool's drop (point up:
/// turned 45° left). The Finger stays an upright pointing hand (macOS's, as Logic uses it),
/// its fingertip the hotspot. The others (Solo, Mute, Quantize, Velocity…) upright, centred.
pub fn shape(tool: Tool) -> Shape {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    let glyph = crate::glyphs::tool(tool);
    // (the tips measured on the icon font's outlines)
    let (angle, tip) = match tool {
        Tool::Pencil => (FRAC_PI_2, (0.02, 0.98)),
        Tool::Brush => (FRAC_PI_2, (0.01, 0.99)),
        Tool::Eraser => (FRAC_PI_2, (0.04, 0.84)),
        Tool::Finger => (0.0, (0.44, 0.0)),
        Tool::Scissors => (-3.0 * FRAC_PI_4, (1.0, 0.5)),
        Tool::Glue => (-FRAC_PI_4, (0.51, 0.0)),
        _ => (0.0, (0.5, 0.5)),
    };
    Shape { glyph, angle, tip }
}

/// The drawn shape of a laid-out glyph (from the galley's origin).
fn ink(galley: &egui::Galley) -> Rect {
    let r = galley.rows.iter().flat_map(|row| row.row.glyphs.iter().map(move |g| (row.pos, g))).fold(Rect::NOTHING, |acc, (rp, g)| {
        if g.uv_rect.is_nothing() { acc } else { acc.union(Rect::from_min_size(rp + g.pos.to_vec2() + g.uv_rect.offset, g.uv_rect.size)) }
    });
    if r.is_positive() { r } else { Rect::from_min_size(Pos2::ZERO, galley.size()) }
}

/// Where the tip lands from the galley's origin once turned (the galley is drawn at the
/// pointer minus this, so the tip is on the pointer).
pub fn tip_offset(ink: Rect, s: Shape) -> egui::Vec2 {
    let local = ink.min.to_vec2() + vec2(s.tip.0 * ink.width(), s.tip.1 * ink.height());
    egui::emath::Rot2::from_angle(s.angle) * local
}

/// The Piano Roll's time axis: the grid's left edge (right of the keyboard), the scroll (bars),
/// the zoom (a bar's width) and the ticks in a bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub left: f32,
    pub scroll: f32,
    pub bar_w: f32,
    pub bar_t: f32,
}

impl Axis {
    pub fn x_of(self, tick: f32) -> f32 {
        self.left + (tick / self.bar_t - self.scroll) * self.bar_w
    }

    pub fn tick_at(self, x: f32) -> f32 {
        ((x - self.left) / self.bar_w + self.scroll) * self.bar_t
    }

    /// The tick a click at `x` puts a new note on with the grid `g` (None: no snap): the grid
    /// line at or before the pointer (Logic's Pencil). `px` is a physical pixel in points: a
    /// line drawn in the pointer's pixel counts as at the pointer (lines are drawn in the
    /// pixel their position falls in), so clicking on a line lands on it.
    pub fn snap_floor(self, x: f32, g: Option<f32>, px: f32) -> f32 {
        match g.filter(|g| *g > 0.0) {
            Some(g) => {
                let t = self.tick_at(x + px * 0.99);
                ((t / g).floor() * g).max(0.0)
            }
            None => self.tick_at(x).max(0.0),
        }
    }
}

/// Where the primary button went down on `resp` (kept until the next press: a click's or a
/// drag's start is where it was pressed, not where the pointer is when egui reports it).
pub fn press_pos(ui: &egui::Ui, resp: &egui::Response) -> Option<Pos2> {
    let id = resp.id.with("press-pos");
    if let Some(o) = ui.input(|i| i.pointer.press_origin()).filter(|_| resp.is_pointer_button_down_on()) {
        ui.data_mut(|d| d.insert_temp(id, o));
        return Some(o);
    }
    ui.data(|d| d.get_temp::<Pos2>(id)).or_else(|| resp.interact_pointer_pos())
}

/// A Tool menu's rows as Logic's (the Tool Menu figure in Logic's guide): a check for the
/// current tool, the tool's icon (the one its pointer uses), its name and its key at the
/// right, in 22 pt rows 24 pt apart, the hovered one in the selection blue. The tool picked.
pub fn menu_rows(ui: &mut egui::Ui, tools: &[Tool], cur: Tool) -> Option<Tool> {
    let font = crate::look::regular(13.0);
    let margin = ui.style().spacing.menu_margin.left as f32;
    let widest = tools.iter().map(|t| ui.fonts_mut(|f| f.layout_no_wrap(t.name().into(), font.clone(), Color32::WHITE).size().x)).fold(0.0, f32::max);
    let key_w = tools.iter().map(|t| ui.fonts_mut(|f| f.layout_no_wrap(t.key().into(), font.clone(), Color32::WHITE).size().x)).fold(0.0, f32::max);
    // (Logic: the name 56 pt in from the menu's edge; keys ending 16 pt in from the right)
    let w = MENU_TEXT_X - margin + widest + 28.0 + key_w + 16.0 - margin;
    let mut picked = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for &t in tools {
            let (row, resp) = ui.allocate_exact_size(vec2(w, MENU_ROW), egui::Sense::click());
            let edge = row.min.x - margin;
            let hl = Rect::from_min_max(pos2(edge + 4.0, row.min.y + 1.0), pos2(row.max.x + margin - 4.0, row.max.y - 1.0));
            let hot = resp.hovered();
            let p = ui.painter();
            if hot {
                p.rect_filled(hl, 5.0, ui.visuals().selection.bg_fill);
            }
            let c = if hot { Color32::WHITE } else { crate::look::text() };
            if t == cur {
                crate::glyphs::paint(p, Rect::from_center_size(pos2(edge + 16.0, row.center().y), vec2(14.0, 14.0)), crate::glyphs::ph::CHECK, 12.0, c);
            }
            crate::glyphs::paint(p, Rect::from_center_size(pos2(edge + 38.0, row.center().y), vec2(16.0, 16.0)), crate::glyphs::tool(t), MENU_ICON, c);
            p.text(pos2(edge + MENU_TEXT_X, row.center().y), egui::Align2::LEFT_CENTER, t.name(), font.clone(), c);
            let kc = if hot { Color32::WHITE } else { crate::look::text_dim() };
            p.text(pos2(row.max.x + margin - 16.0, row.center().y), egui::Align2::RIGHT_CENTER, t.key(), font.clone(), kc);
            if resp.clicked() {
                picked = Some(t);
            }
        }
    });
    picked
}

/// Logic's Tool menu: a row's height (22 pt highlight, 1 pt above and below), the name's
/// inset from the menu's edge and the icons' size.
pub const MENU_ROW: f32 = 24.0;
pub const MENU_TEXT_X: f32 = 56.0;
pub const MENU_ICON: f32 = 15.0;

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
    fn tip_tools_point_to_the_top_left_with_the_hotspot_at_the_tip() {
        // a square glyph box 16 pt wide, its shape from (1, 2) to (17, 18)
        let ink = Rect::from_min_size(pos2(1.0, 2.0), vec2(16.0, 16.0));
        for t in [Tool::Pencil, Tool::Brush, Tool::Eraser, Tool::Scissors, Tool::Glue] {
            let s = shape(t);
            // turned, the tip is the shape's top-left-most point: every corner of the turned
            // shape lies right of or below it
            let rot = egui::emath::Rot2::from_angle(s.angle);
            let tip = tip_offset(ink, s);
            for c in [ink.left_top(), ink.right_top(), ink.left_bottom(), ink.right_bottom()] {
                let q = rot * c.to_vec2() - tip;
                // (within a point: the outlines' corners are rounded; the Eraser's tip is its
                // body's corner, the line under it reaching a little further)
                let slack = if t == Tool::Eraser { 4.0 } else { 1.0 };
                assert!(q.x + q.y >= -slack, "{t:?}: corner {c:?} is above-left of the tip");
            }
        }
        // the Pencil's tip (down-left in the font) goes to the top left
        let s = shape(Tool::Pencil);
        let rot = egui::emath::Rot2::from_angle(s.angle);
        let c = rot * (ink.center().to_vec2()) - tip_offset(ink, s);
        assert!(c.x > 0.0 && c.y > 0.0);
        // the Finger upright, its fingertip the hotspot; the others upright, centred
        assert_eq!(shape(Tool::Finger).angle, 0.0);
        assert_eq!(shape(Tool::Finger).tip.1, 0.0);
        assert_eq!(shape(Tool::Solo).angle, 0.0);
        assert_eq!(shape(Tool::Mute).tip, (0.5, 0.5));
    }

    #[test]
    fn x_and_tick_round_trip_at_any_zoom_and_scroll() {
        for (left, scroll, bar_w) in [(330.0, 0.0, 300.0), (330.0, 1.0, 140.0), (412.5, 7.25, 960.0), (56.0, 123.5, 30.0), (56.0, 0.3, 3000.0)] {
            let a = Axis { left, scroll, bar_w, bar_t: 3840.0 };
            for tick in [0.0, 1.0, 239.0, 240.0, 961.0, 3840.0, 50_000.0] {
                assert!((a.tick_at(a.x_of(tick)) - tick).abs() < 0.05, "{a:?} {tick}");
            }
            for x in [left, left + 0.5, left + 17.0, left + 400.25] {
                assert!((a.x_of(a.tick_at(x)) - x).abs() < 1e-3, "{a:?} {x}");
            }
            // the grid's left edge is the scroll position
            assert!((a.tick_at(left) - scroll * 3840.0).abs() < 0.05);
        }
    }

    #[test]
    fn a_click_lands_on_the_grid_line_at_or_before_the_tip() {
        for (left, scroll, bar_w) in [(330.0, 0.0, 300.0), (330.0, 1.0, 140.0), (412.5, 7.25, 960.0), (330.0, 0.37, 777.0)] {
            let a = Axis { left, scroll, bar_w, bar_t: 3840.0 };
            let g = 240.0;
            let first = (scroll * 3840.0 / g).ceil() as i32;
            for k in first..first + 6 {
                let line = k as f32 * g;
                let x = a.x_of(line);
                // the line is drawn in the pixel its position falls in: a click anywhere in that
                // pixel lands on it, as does one further right in the cell
                let px = x.floor();
                assert_eq!(a.snap_floor(px, Some(g), 1.0), line, "{a:?} line {k} at {x}");
                assert_eq!(a.snap_floor(px + 0.5, Some(g), 1.0), line);
                let next = a.x_of(line + g).floor();
                assert_eq!(a.snap_floor(next - 1.0, Some(g), 1.0), line, "{a:?} end of cell {k}");
                // the pixel left of the line: the cell before
                if k > 0 {
                    assert_eq!(a.snap_floor(px - 1.0, Some(g), 1.0), line - g, "{a:?} before line {k}");
                }
                // HiDPI (2 pixels a point): the half-point pixel holding the line
                let px2 = (x * 2.0).floor() / 2.0;
                assert_eq!(a.snap_floor(px2, Some(g), 0.5), line);
            }
            // no snap: where it is
            assert!((a.snap_floor(a.x_of(1234.0), None, 1.0) - 1234.0).abs() < 0.05);
        }
    }
}
