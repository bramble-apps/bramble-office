//! The Project End marker in Studio (lg-core projend): playback stops there as Logic's does
//! (Cycle loops first), recording past it moves it later, and the ruler shows it as Logic's
//! small grey triangle (the Project Start's mirror) that the Trim pointer drags, with a help
//! tag giving the project start and end.

use egui::{pos2, CursorIcon, Rect, Sense, Stroke};

use crate::app::App;

/// The project end, ticks from bar 1 (Logic's 128 bars without a project).
pub fn end(app: &App) -> u32 {
    app.project.as_ref().map_or(lg_core::projend::DEFAULT_BARS * 3840, lg_core::projend::end)
}

/// Each frame: the engine knows the end; a stop at the end acts as the Stop button (Stop Button
/// Options: Stop and Go to Left Locator / Last Locate Position); recording moves the end on.
pub fn frame(app: &mut App) {
    let end = end(app);
    // (Live Loops: playing cells go on at the end, so the song does too)
    let want = (!app.live_loops.any_active()).then_some(end);
    let (ended, pos) = {
        let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
        if e.end != want {
            e.end = want;
        }
        (std::mem::take(&mut e.ended), e.pos)
    };
    if ended {
        after_auto_stop(app);
    }
    if app.recording() && pos >= end as f64 {
        if let Some(p) = app.project.as_mut() {
            if lg_core::projend::extend_for_recording(p, pos as u32) {
                app.dirty = true;
            }
        }
    }
}

/// Logic stops at the end as if Stop were pressed while playing: the Stop mode's locate.
fn after_auto_stop(app: &mut App) {
    match app.stopbtn.mode {
        crate::stopbutton::Mode::Stop => {}
        crate::stopbutton::Mode::LeftLocator => {
            let c = app.cycle.0 as f64;
            app.locate(c);
        }
        crate::stopbutton::Mode::LastLocate => {
            let at = app.stopbtn.last_pos;
            app.locate(at);
        }
    }
}

/// The marker's shape: a 7 × 11 pt triangle pointing back into the project, its tip 1 pt after
/// the end (Logic's Project Start triangle at 2x, mirrored). `cy` is its centre line.
pub fn marker(x: f32, cy: f32) -> [egui::Pos2; 3] {
    [pos2(x + 8.0, cy - 5.5), pos2(x + 1.0, cy), pos2(x + 8.0, cy + 5.5)]
}

/// The ruler's end marker; dragging it (Trim pointer) moves the end by bars, one undo step.
pub fn ruler(app: &mut App, ui: &mut egui::Ui, upper: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(sig) = app.project.as_ref().map(|p| p.signature) else { return };
    let x = x_of(end(app) as f32);
    if x < upper.min.x - 10.0 || x > upper.max.x + 10.0 {
        return;
    }
    let cy = upper.min.y + 7.5;
    let p = ui.painter_at(upper);
    p.add(egui::Shape::convex_polygon(marker(x, cy).to_vec(), egui::Color32::from_gray(0x54), Stroke::NONE));
    let hit = Rect::from_min_max(pos2(x - 2.0, upper.min.y), pos2(x + 10.0, upper.max.y));
    let resp = ui.interact(hit, ui.scope_id().with("project-end"), Sense::drag()).on_hover_text("Project End");
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if resp.drag_started() {
        app.checkpoint("Project End");
    }
    if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
        let t = tick_of(pp.x);
        if let Some(pr) = app.project.as_mut() {
            let e = lg_core::projend::snap_end(pr, t);
            pr.end = Some(e);
            app.help_tag = Some(vec![("Project".into(), String::new()), ("Start:".into(), crate::app::bbdt(0, sig)), ("End:".into(), crate::app::bbdt(e, sig))]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_points_back_into_the_project() {
        let m = marker(100.0, 10.0);
        // the tip is the leftmost point, 1 pt after the end; the flat side 8 pt after
        assert_eq!(m[1], pos2(101.0, 10.0));
        assert!(m[0].x == 108.0 && m[2].x == 108.0);
        assert_eq!(m[2].y - m[0].y, 11.0);
    }

    #[test]
    fn playback_stops_at_the_project_end() {
        let mut e = crate::audio::Engine::new();
        e.end = Some(3840);
        e.pos = 3800.0;
        e.playing = true;
        let mut buf = vec![0f32; 2 * 512];
        for _ in 0..200 {
            e.render(&mut buf);
        }
        assert!(!e.playing);
        assert!(e.ended);
        assert_eq!(e.pos, 3840.0);
    }

    #[test]
    fn cycle_loops_past_the_end() {
        let mut e = crate::audio::Engine::new();
        e.end = Some(3840);
        e.cycle = Some((0, 3840 * 2));
        e.cycle_on = true;
        e.pos = 3800.0;
        e.playing = true;
        let mut buf = vec![0f32; 2 * 512];
        for _ in 0..200 {
            e.render(&mut buf);
        }
        assert!(e.playing && !e.ended);
    }
}
