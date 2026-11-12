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
    // (Live Loops: at the end the playhead stops while playing cells go on; once they've all
    // stopped, the song stops there too)
    let cells = app.live_loops.any_active();
    let (ended, pos) = {
        let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
        e.end = Some(end);
        e.start = start(app) as f64;
        e.cells_run = cells;
        if e.past_end && (!cells || !e.playing) {
            e.past_end = false;
            e.pos = end as f64;
            if e.playing {
                e.playing = false;
                e.all_off();
                e.ended = true;
            }
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

/// Where the Project Start is kept with the project's settings (its .logicx place isn't known
/// yet; the end is in the file).
const START_KEY: &str = "State / Project / Start";

/// A project opened: its Project Start.
pub fn after_open(app: &mut App) {
    let s = app.proj.0.get(START_KEY).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0).min(0);
    if let Some(p) = app.project.as_mut() {
        p.start = s;
    }
}

/// The project saved: its Project Start beside it.
pub fn store(app: &mut App) {
    match start(app) {
        0 => {
            app.proj.0.remove(START_KEY);
        }
        s => {
            app.proj.0.insert(START_KEY.into(), s.to_string());
        }
    }
}

/// The Project Start (ticks from bar 1; 0 or earlier).
pub fn start(app: &App) -> i32 {
    app.project.as_ref().map_or(0, |p| p.start)
}

/// How far left the Tracks area scrolls (bars): to the Project Start.
pub fn min_scroll(app: &App) -> f32 {
    start(app) as f32 / app.bar_ticks().max(1) as f32
}

/// The Project Start's triangle: Logic's at 2x, 7 × 11 pt, flat side 8 pt before the start,
/// tip 1 pt before.
pub fn start_marker(x: f32, cy: f32) -> [egui::Pos2; 3] {
    [pos2(x - 8.0, cy - 5.5), pos2(x - 1.0, cy), pos2(x - 8.0, cy + 5.5)]
}

/// The ruler's Project Start and End markers; dragging one (Trim pointer) moves it by bars, one
/// undo step, a help tag giving both. The start moves right only back to bar 1.
pub fn ruler(app: &mut App, ui: &mut egui::Ui, upper: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(sig) = app.project.as_ref().map(|p| p.signature) else { return };
    let cy = upper.min.y + 7.5;
    let p = ui.painter_at(upper);
    let grey = egui::Color32::from_gray(0x54);
    let xs = x_of(start(app) as f32);
    let xe = x_of(end(app) as f32);
    p.add(egui::Shape::convex_polygon(start_marker(xs, cy).to_vec(), grey, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(marker(xe, cy).to_vec(), grey, Stroke::NONE));
    let div = app.nav.division;
    let tag = |pr: &lg_core::Project| -> Vec<(String, String)> {
        vec![
            ("Project".into(), String::new()),
            ("Start:".into(), crate::lcdcustom::bars_text(pr.start as f64, sig, div)),
            ("End:".into(), crate::lcdcustom::bars_text(lg_core::projend::end(pr) as f64, sig, div)),
        ]
    };
    for (is_start, x) in [(true, xs), (false, xe)] {
        if x < upper.min.x - 10.0 || x > upper.max.x + 10.0 {
            continue;
        }
        let hit = if is_start { Rect::from_min_max(pos2(x - 10.0, upper.min.y), pos2(x + 1.0, upper.max.y)) } else { Rect::from_min_max(pos2(x - 1.0, upper.min.y), pos2(x + 10.0, upper.max.y)) };
        let name = if is_start { "Project Start" } else { "Project End" };
        let resp = ui.interact(hit, ui.scope_id().with(name), Sense::drag()).on_hover_text(name);
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if resp.drag_started() {
            app.checkpoint(name);
        }
        if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let t = tick_of(pp.x);
            if let Some(pr) = app.project.as_mut() {
                if is_start {
                    pr.start = lg_core::projend::snap_start(pr, t);
                } else {
                    pr.end = Some(lg_core::projend::snap_end(pr, t));
                }
                app.help_tag = Some(tag(pr));
            }
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
    fn cells_run_on_past_the_end_with_the_playhead_held() {
        let mut e = crate::audio::Engine::new();
        e.end = Some(3840);
        e.cells_run = true;
        e.pos = 3800.0;
        e.playing = true;
        let mut buf = vec![0f32; 2 * 512];
        for _ in 0..200 {
            e.render(&mut buf);
        }
        assert!(e.playing && e.past_end && !e.ended);
        assert!(e.pos > 3840.0);
        e.locate(0.0);
        assert!(!e.past_end);
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
