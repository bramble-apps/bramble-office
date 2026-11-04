//! Scrolling as the suite's Linux apps do it (Letters' page canvas, pg-app canvas.rs, commits
//! 6093b41 + 1efd17f), for the editor and the shell's list and sidebar: a wheel notch scrolls 100 pt
//! like GTK and eases in over a few frames (a turn the other way drops what's left), trackpad moves
//! apply as they come, and after the fingers lift a GTK glide keeps going and slows down (stopped
//! by touching again, a press, a turn, or the system's own momentum). Plus the GNOME-style overlay
//! scrollbar: drag the thumb, click the track to page.

use egui::{Color32, Rect, Sense, Vec2, pos2, vec2};

/// One vertical scroll position with its wheel easing and glide.
#[derive(Clone, Debug)]
pub struct Scroller {
    /// points from the top
    pub offset: f32,
    wheel_pending: f32,
    kinetic: f32,
    last_finger: f64,
    lifted: bool,
    os_momentum: f64,
    sb_drag: Option<(f32, f32)>,
    /// the largest offset, from the last clamp (input() stops there itself)
    max: Option<f32>,
    /// Shift and the wheel belong to something sideways under the pointer (a PDF's pages)
    pub skip_shift: bool,
}

impl Default for Scroller {
    fn default() -> Self {
        // (no system momentum seen yet: long ago, so the first glide isn't taken for one)
        Scroller { offset: 0.0, wheel_pending: 0.0, kinetic: 0.0, last_finger: f64::NEG_INFINITY, lifted: false, os_momentum: f64::NEG_INFINITY, sb_drag: None, max: None, skip_shift: false }
    }
}

impl Scroller {
    /// Takes this frame's wheel and trackpad input (when the pointer is over the area) and moves
    /// the offset; `page_h` is what a page-sized wheel step scrolls. True while still moving.
    pub fn input(&mut self, ctx: &egui::Context, hovered: bool, page_h: f32) -> bool {
        let (events, now, dt, pressed) = ctx.input(|i| {
            let ev: Vec<(egui::MouseWheelUnit, Vec2, egui::TouchPhase, egui::MouseWheelSource, egui::Modifiers)> = i
                .raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, phase, source, modifiers } => Some((*unit, *delta, *phase, *source, *modifiers)),
                    _ => None,
                })
                .collect();
            let pressed = i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
            (ev, i.time, i.stable_dt.clamp(0.001, 0.05), pressed)
        });
        let (mut fingers, mut lines) = (0.0f32, 0.0f32);
        let (mut touched, mut lifted, mut momentum, mut ctrl) = (false, false, false, false);
        for (unit, delta, phase, source, mods) in events {
            if mods.command || mods.ctrl {
                ctrl = true;
                continue;
            }
            if self.skip_shift && mods.shift {
                continue;
            }
            match phase {
                egui::TouchPhase::Start => touched = true,
                egui::TouchPhase::End | egui::TouchPhase::Cancel => lifted = true,
                egui::TouchPhase::Move => {}
            }
            let d = delta.y;
            match unit {
                egui::MouseWheelUnit::Point if source == egui::MouseWheelSource::Momentum => {
                    momentum = true;
                    fingers += d;
                }
                egui::MouseWheelUnit::Point => fingers += d,
                egui::MouseWheelUnit::Line => lines += d * 100.0,
                egui::MouseWheelUnit::Page => lines += d * page_h,
            }
        }
        if !hovered {
            (fingers, lines, momentum) = (0.0, 0.0, false);
        }
        let mut moving = false;
        if hovered && !ctrl {
            if fingers != 0.0 {
                self.offset -= fingers;
            }
            if lines != 0.0 {
                if lines * self.wheel_pending < 0.0 {
                    self.wheel_pending = 0.0;
                }
                self.wheel_pending += lines;
            }
        }
        if self.wheel_pending.abs() > 0.5 {
            let step = self.wheel_pending * (1.0 - (-22.0 * dt).exp());
            self.offset -= step;
            self.wheel_pending -= step;
            moving = true;
        } else if self.wheel_pending != 0.0 {
            self.offset -= self.wheel_pending;
            self.wheel_pending = 0.0;
        }
        // the glide
        if momentum {
            self.os_momentum = now;
        }
        if touched || pressed || now - self.os_momentum < 0.5 || ctrl {
            self.kinetic = 0.0;
        }
        if fingers != 0.0 && !momentum {
            let v = fingers / dt;
            let fresh = now - self.last_finger > 0.1 || v * self.kinetic < 0.0;
            self.kinetic = if fresh { v } else { self.kinetic * 0.6 + v * 0.4 };
            self.last_finger = now;
            self.lifted = false;
        } else {
            if lifted {
                self.lifted = true;
            }
            // (where the platform says when the fingers lift, glide from then; else after a pause)
            let go = self.lifted || now - self.last_finger > 0.06;
            if go && self.kinetic.abs() > 30.0 {
                self.offset -= self.kinetic * dt;
                self.kinetic *= (-2.6 * dt).exp();
                moving = true;
            } else if go {
                self.kinetic = 0.0;
            }
        }
        // the ends stop it dead (no drawing past them and snapping back): what is pushing that way,
        // wheel or glide, is dropped (GNOME has no rubber band; nor have Letters' pages)
        if self.limit() {
            moving = self.wheel_pending.abs() > 0.5 || self.kinetic.abs() > 30.0;
        }
        if moving {
            ctx.request_repaint();
        }
        moving
    }

    /// Holds the offset in 0..=max (the last known max); true if it was outside.
    fn limit(&mut self) -> bool {
        let max = self.max.unwrap_or(f32::MAX).max(0.0);
        if self.offset < 0.0 {
            self.offset = 0.0;
            // (positive amounts move up)
            self.wheel_pending = self.wheel_pending.min(0.0);
            self.kinetic = self.kinetic.min(0.0);
            return true;
        }
        if self.offset > max {
            self.offset = max;
            self.wheel_pending = self.wheel_pending.max(0.0);
            self.kinetic = self.kinetic.max(0.0);
            return true;
        }
        false
    }

    /// The largest offset now (content height minus the view's); keeps the offset in 0..=max and
    /// remembers it, so input() stops at the ends by itself from the next frame. Call it before
    /// drawing when the content height is known, else after (the remembered one is used meanwhile).
    pub fn clamp(&mut self, max: f32) {
        self.max = Some(max.max(0.0));
        self.limit();
    }

    /// Stops any easing or glide (a jump, a keyboard scroll).
    pub fn stop(&mut self) {
        self.kinetic = 0.0;
        self.wheel_pending = 0.0;
    }

    /// The overlay scrollbar in `track` (its right edge where the thumb sits). True while it has the
    /// pointer (hovered or dragged), so the area under it doesn't take the press.
    pub fn scrollbar(&mut self, ui: &egui::Ui, track: Rect, content_h: f32, view_h: f32, id: egui::Id) -> bool {
        if content_h <= view_h + 1.0 {
            self.sb_drag = None;
            return false;
        }
        let max = content_h - view_h;
        let thumb_h = (track.height() * view_h / content_h).clamp(36.0, track.height());
        let thumb_y = track.min.y + (track.height() - thumb_h) * (self.offset / max).clamp(0.0, 1.0);
        let resp = ui.interact(track, id, Sense::click_and_drag());
        let hot = resp.hovered() || resp.dragged() || self.sb_drag.is_some();
        let w = if hot { 9.0 } else { 5.0 };
        let thumb = Rect::from_min_size(pos2(track.max.x - 2.0 - w, thumb_y), vec2(w, thumb_h));
        let p = ui.painter();
        let light = kn_ui::theme::is_light();
        if hot {
            p.rect_filled(Rect::from_min_max(pos2(track.max.x - 3.0 - w, track.min.y), pos2(track.max.x - 1.0, track.max.y)), 6.0, kn_ui::theme::gray(0x80).gamma_multiply(0.12));
        }
        let ink = if light { Color32::from_black_alpha(if hot { 150 } else { 110 }) } else { Color32::from_white_alpha(if hot { 170 } else { 120 }) };
        p.rect_filled(thumb, w / 2.0, ink);
        if resp.drag_started() {
            if let Some(q) = ui.input(|i| i.pointer.press_origin()) {
                if thumb.expand2(vec2(6.0, 0.0)).contains(q) {
                    self.sb_drag = Some((q.y, self.offset));
                } else {
                    // a drag that starts on the track: the thumb jumps under the pointer first
                    let s = ((q.y - track.min.y - thumb_h / 2.0) / (track.height() - thumb_h)).clamp(0.0, 1.0) * max;
                    self.offset = s;
                    self.sb_drag = Some((q.y, s));
                }
            }
        }
        if let (Some((y0, s0)), Some(q)) = (self.sb_drag, ui.input(|i| i.pointer.latest_pos())) {
            let k = max / (track.height() - thumb_h).max(1.0);
            self.offset = (s0 + (q.y - y0) * k).clamp(0.0, max);
            self.stop();
            ui.ctx().request_repaint();
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.sb_drag = None;
        }
        if resp.clicked() {
            if let Some(q) = resp.interact_pointer_pos() {
                let page = view_h * 0.9;
                if q.y < thumb.min.y {
                    self.offset = (self.offset - page).max(0.0);
                } else if q.y > thumb.max.y {
                    self.offset = (self.offset + page).min(max);
                }
            }
        }
        hot
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ends_stop_dead_without_bouncing() {
        let ctx = egui::Context::default();
        let mut s = Scroller::default();
        s.clamp(200.0);
        let mut t = 0.0;
        for _ in 0..5 {
            t += 1.0 / 60.0;
            frame_max(&ctx, &mut s, t, vec![], 200.0);
        }
        // wheel notches down, far past the end
        let mut seen = Vec::new();
        for k in 0..60 {
            t += 1.0 / 60.0;
            let ev = if k % 4 == 0 { vec![wheel(egui::MouseWheelUnit::Line, -1.0, egui::TouchPhase::Move)] } else { vec![] };
            seen.push(frame_max(&ctx, &mut s, t, ev, 200.0));
        }
        assert!(seen.windows(2).all(|w| w[1] >= w[0]), "monotonic: {seen:?}");
        assert!(seen.iter().all(|o| *o <= 200.0), "never past the end");
        assert_eq!(*seen.last().unwrap(), 200.0);
        let at = seen.iter().position(|o| *o == 200.0).unwrap();
        assert!(seen[at..].iter().all(|o| *o == 200.0), "stays put once there");
        // a trackpad flick up into the top
        let mut seen = Vec::new();
        for _ in 0..6 {
            t += 1.0 / 60.0;
            seen.push(frame_max(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Point, 30.0, egui::TouchPhase::Move)], 200.0));
        }
        t += 1.0 / 60.0;
        seen.push(frame_max(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Point, 0.0, egui::TouchPhase::End)], 200.0));
        for _ in 0..60 {
            t += 1.0 / 60.0;
            seen.push(frame_max(&ctx, &mut s, t, vec![], 200.0));
        }
        assert!(seen.windows(2).all(|w| w[1] <= w[0]), "monotonic: {seen:?}");
        assert!(seen.iter().all(|o| *o >= 0.0));
        assert_eq!(*seen.last().unwrap(), 0.0);
        // the content shrinking below the view: one move to 0 (a caller that clamps after drawing
        // learns the new height a frame late), never back again
        s.offset = 150.0;
        let mut seen = Vec::new();
        for k in 1..6 {
            seen.push(frame_max(&ctx, &mut s, t + 0.02 * k as f64, vec![], 0.0));
        }
        assert_eq!(seen, [150.0, 0.0, 0.0, 0.0, 0.0]);
        // clamped before drawing (the editor): straight to 0
        s.offset = 150.0;
        s.clamp(0.0);
        assert_eq!(s.offset, 0.0);
    }

    /// A frame where the area clamps after drawing (as the shell's list does): what was drawn.
    fn frame_max(ctx: &egui::Context, s: &mut Scroller, t: f64, events: Vec<egui::Event>, max: f32) -> f32 {
        let raw = egui::RawInput { time: Some(t), predicted_dt: 1.0 / 60.0, events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0))), ..Default::default() };
        let mut drawn = 0.0;
        let mut out = ctx.run_ui(raw, |ui| {
            s.input(ui.ctx(), true, 400.0);
            drawn = s.offset;
            s.clamp(max);
        });
        out.textures_delta.clear();
        drawn
    }

    use super::Scroller;

    fn frame(ctx: &egui::Context, s: &mut Scroller, t: f64, events: Vec<egui::Event>) -> f32 {
        let raw = egui::RawInput { time: Some(t), predicted_dt: 1.0 / 60.0, events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            s.input(ui.ctx(), true, 400.0);
            s.clamp(10_000.0);
        });
        out.textures_delta.clear();
        s.offset
    }

    fn wheel(unit: egui::MouseWheelUnit, dy: f32, phase: egui::TouchPhase) -> egui::Event {
        egui::Event::MouseWheel { unit, delta: egui::vec2(0.0, dy), phase, source: egui::MouseWheelSource::Unknown, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn wheel_notch_eases_to_100_points() {
        let ctx = egui::Context::default();
        let mut s = Scroller::default();
        let mut t = 0.0;
        // (egui's stable_dt settles after a few frames)
        for _ in 0..5 {
            t += 1.0 / 60.0;
            frame(&ctx, &mut s, t, vec![]);
        }
        t += 1.0 / 60.0;
        let first = frame(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Line, -1.0, egui::TouchPhase::Move)]);
        assert!(first > 10.0 && first < 60.0, "{first}");
        let mut last = first;
        for _ in 0..40 {
            t += 1.0 / 60.0;
            last = frame(&ctx, &mut s, t, vec![]);
        }
        assert!((last - 100.0).abs() < 0.6, "{last}");
        // a notch the other way drops what's pending and turns at once
        t += 1.0 / 60.0;
        frame(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Line, -1.0, egui::TouchPhase::Move)]);
        t += 1.0 / 60.0;
        let a = frame(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Line, 1.0, egui::TouchPhase::Move)]);
        t += 1.0 / 60.0;
        let b = frame(&ctx, &mut s, t, vec![]);
        assert!(b < a, "turned around: {a} then {b}");
    }

    #[test]
    fn trackpad_moves_then_glides_and_a_touch_stops_it() {
        let ctx = egui::Context::default();
        let mut s = Scroller::default();
        let mut t = 0.0;
        for _ in 0..5 {
            t += 1.0 / 60.0;
            frame(&ctx, &mut s, t, vec![]);
        }
        for _ in 0..6 {
            t += 1.0 / 60.0;
            frame(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Point, -20.0, egui::TouchPhase::Move)]);
        }
        assert!((s.offset - 120.0).abs() < 0.01, "moves apply as they come: {}", s.offset);
        t += 1.0 / 60.0;
        let lift = frame(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Point, 0.0, egui::TouchPhase::End)]);
        let mut prev = lift;
        let mut steps = Vec::new();
        for _ in 0..30 {
            t += 1.0 / 60.0;
            let o = frame(&ctx, &mut s, t, vec![]);
            steps.push(o - prev);
            prev = o;
        }
        assert!(prev > lift + 100.0, "glides on: {lift} -> {prev}");
        assert!(steps.windows(2).all(|w| w[1] <= w[0] + 1e-3), "slows down: {steps:?}");
        t += 1.0 / 60.0;
        let at = frame(&ctx, &mut s, t, vec![wheel(egui::MouseWheelUnit::Point, 0.0, egui::TouchPhase::Start)]);
        t += 1.0 / 60.0;
        let after = frame(&ctx, &mut s, t, vec![]);
        assert!((after - at).abs() < 0.01, "a touch stops the glide");
    }
}
