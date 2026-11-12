//! Logic's key focus: the area clicked last (the Tracks area, the editor below it, the Mixer,
//! the Browser, the Event List, the Step Editor, a window…) has it, shown by a blue frame, and
//! Delete, Select All, Copy / Cut / Paste, the arrow keys and every other key command act on
//! that area only, wherever the pointer is. ⇥ / ⇧⇥ (Cycle Through Window Views) move it to
//! the next / previous area shown. Menus and popovers don't take it; nor do the control bar,
//! the toolbar or the inspector (Logic's inspector follows the area with key focus).

use egui::{Color32, Pos2, Rect, Stroke, StrokeKind};

use crate::app::{App, Bottom};

pub const COMMANDS: &[&str] = &["Cycle Through Window Views", "Cycle Through Window Views (counter-clockwise)"];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Area {
    #[default]
    Tracks,
    /// The editor below the tracks: the Piano Roll, Score, Step Sequencer or an audio editor.
    Editor,
    Mixer,
    Browser,
    EventList,
    StepEditor,
    /// Somewhere with its own keys (a plug-in window, Smart Controls, Project Audio): where the
    /// press was, for `clicked_in`.
    Other,
}

#[derive(Clone, Debug, Default)]
pub struct KeyFocus {
    pub area: Area,
    /// Where the press that gave `Other` the focus was, and its layer (a window's).
    pub press: Option<Pos2>,
    pub layer: Option<egui::LayerId>,
    /// The Tracks area (headers, ruler, lanes, global tracks), as last drawn.
    pub tracks_rect: Option<Rect>,
    /// ⇥ moved the focus: egui's own Tab (widget focus) is undone at the frame's end, so the
    /// keys after it still reach the area.
    pub tabbed: bool,
}

/// Logic's key-focus frame: 2 pt of translucent blue inside the area's edge.
pub fn frame_color() -> Color32 {
    Color32::from_rgba_unmultiplied(106, 140, 184, 194)
}

/// A press this frame moves the focus to the area under it (with last frame's layout; this
/// runs before the areas draw).
pub fn press(app: &mut App, ctx: &egui::Context) {
    let Some(pos) = ctx.input(|i| (i.pointer.primary_pressed() || i.pointer.secondary_pressed()).then(|| i.pointer.interact_pos()).flatten()) else { return };
    let layer = ctx.layer_id_at(pos).map(|l| l.order);
    if let Some(a) = area_at(app, ctx, pos, layer) {
        app.kf.area = a;
        app.kf.press = (a == Area::Other).then_some(pos);
        app.kf.layer = ctx.layer_id_at(pos);
        if a != Area::Tracks {
            leave_tracks(app);
        }
    }
}

/// The Tracks area's own focused parts (markers, global tracks, the Live Loops grid) give it up
/// with the area.
fn leave_tracks(app: &mut App) {
    app.mk.focus = false;
    app.arrm.focus = false;
    app.gc.focus = crate::globalclip::Focus::None;
    app.live_loops.focused = false;
}

fn area_at(app: &App, ctx: &egui::Context, pos: Pos2, layer: Option<egui::Order>) -> Option<Area> {
    if crate::stepedit::rect(ctx).is_some_and(|r| r.contains(pos)) {
        return Some(Area::StepEditor);
    }
    match layer {
        // menus, popovers, tooltips: the focus stays
        Some(egui::Order::Foreground | egui::Order::Tooltip | egui::Order::Debug) => return None,
        // a window (plug-in, Event Float, Marker List…): its own keys
        Some(egui::Order::Middle) => return Some(Area::Other),
        _ => {}
    }
    if crate::eventedit::shown_rect(app, ctx).is_some_and(|r| r.contains(pos)) {
        return Some(Area::EventList);
    }
    if app.show_browser && app.nav.browser_rect.contains(pos) {
        return Some(Area::Browser);
    }
    if app.bottom != Bottom::None && app.editor_rect.contains(pos) {
        return Some(match app.bottom {
            Bottom::Mixer => Area::Mixer,
            Bottom::PianoRoll => Area::Editor,
            _ => Area::Other,
        });
    }
    if app.pa.rect.is_some_and(|r| r.contains(pos)) || app.sc.rect.is_some_and(|r| r.contains(pos)) {
        return Some(Area::Other);
    }
    if app.kf.tracks_rect.is_some_and(|r| r.contains(pos)) {
        return Some(Area::Tracks);
    }
    None
}

fn shown(app: &App, a: Area) -> bool {
    match a {
        Area::Tracks | Area::Other => true,
        Area::Editor => app.bottom == Bottom::PianoRoll,
        Area::Mixer => app.bottom == Bottom::Mixer,
        Area::Browser => app.show_browser,
        Area::EventList => crate::eventedit::shown_rect(app, &app.ctx).is_some(),
        Area::StepEditor => crate::stepedit::rect(&app.ctx).is_some(),
    }
}

/// The area with key focus (the Tracks area when the one clicked last has been closed).
pub fn area(app: &App) -> Area {
    if shown(app, app.kf.area) { app.kf.area } else { Area::Tracks }
}

/// The editor below the tracks has key focus.
pub fn editor(app: &App) -> bool {
    area(app) == Area::Editor
}

/// The Tracks area has key focus.
pub fn tracks(app: &App) -> bool {
    area(app) == Area::Tracks
}

/// The last press went into `r` (an area with its own keys: a window, Smart Controls…).
pub fn clicked_in(app: &App, r: Option<Rect>) -> bool {
    area(app) == Area::Other && app.kf.press.zip(r).is_some_and(|(p, r)| r.contains(p))
}

/// The window `ui` is in was clicked last: its own Delete / Copy / Paste keys apply.
pub fn in_window(app: &App, ui: &egui::Ui) -> bool {
    area(app) == Area::Other && app.kf.layer == Some(ui.layer_id())
}

/// Which group of key commands applies (keymap.rs).
pub fn keymap_focus(app: &App, ctx: &egui::Context) -> crate::keymap::Focus {
    use crate::keymap::Focus;
    match area(app) {
        Area::Editor => crate::score::focus(app, ctx),
        Area::Mixer => Focus::Mixer,
        Area::Browser => Focus::Browser,
        Area::EventList => Focus::EventList,
        Area::StepEditor => Focus::StepEditor,
        Area::Tracks | Area::Other => Focus::Tracks,
    }
}

/// The areas ⇥ steps through, in Logic's order (left to right, top to bottom).
fn ring(app: &App) -> Vec<Area> {
    [Area::Browser, Area::Tracks, Area::Editor, Area::Mixer, Area::EventList, Area::StepEditor].into_iter().filter(|&a| shown(app, a)).collect()
}

/// Cycle Through Window Views: the next (or previous) area shown gets the focus.
pub fn cycle(app: &mut App, forward: bool) {
    let r = ring(app);
    let cur = r.iter().position(|&a| a == area(app)).unwrap_or(0);
    let n = r.len().max(1);
    let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
    if let Some(&a) = r.get(next) {
        app.kf.area = a;
        app.kf.press = None;
        app.kf.layer = None;
        app.kf.tabbed = true;
        if a != Area::Tracks {
            leave_tracks(app);
        }
    }
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Cycle Through Window Views" => cycle(app, true),
        "Cycle Through Window Views (counter-clockwise)" => cycle(app, false),
        _ => return false,
    }
    true
}

/// Delete / Backspace (no modifier): Edit ▸ Delete for the area with key focus, which keeps to
/// that area (never the tracks' regions while an editor has the focus).
pub fn delete_key(app: &mut App, ctx: &egui::Context) {
    match area(app) {
        // (the Live Loops grid's own Delete ran in liveloops.rs)
        Area::Tracks if app.live_loops.focused => {}
        Area::Tracks | Area::Editor | Area::EventList => {
            app.key_command(ctx, "Delete");
        }
        // the Mixer, the Browser, windows: their own keys or nothing
        _ => {}
    }
}

/// The frame on the focused area (drawn over it, after everything else).
pub fn paint(app: &mut App, ctx: &egui::Context, painter: &egui::Painter) {
    if std::mem::take(&mut app.kf.tabbed) {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
    let r = match area(app) {
        Area::Tracks => app.kf.tracks_rect,
        Area::Editor | Area::Mixer => Some(app.editor_rect),
        Area::Browser => Some(app.nav.browser_rect),
        Area::EventList => crate::eventedit::shown_rect(app, ctx),
        // (windows: their own title bar shows it)
        Area::StepEditor | Area::Other => None,
    };
    if let Some(r) = r.filter(|r| r.is_positive()) {
        painter.rect_stroke(r, 0.0, Stroke::new(2.0, frame_color()), StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn app() -> App {
        let mut a = crate::tm::test_app();
        a.kf.tracks_rect = Some(Rect::from_min_size(pos2(0.0, 100.0), vec2(800.0, 300.0)));
        a.editor_rect = Rect::from_min_size(pos2(0.0, 401.0), vec2(800.0, 300.0));
        a
    }

    #[test]
    fn the_area_clicked_last_has_focus() {
        let mut a = app();
        a.bottom = Bottom::PianoRoll;
        let ctx = a.ctx.clone();
        assert_eq!(area_at(&a, &ctx, pos2(10.0, 500.0), None), Some(Area::Editor));
        assert_eq!(area_at(&a, &ctx, pos2(10.0, 200.0), None), Some(Area::Tracks));
        // a menu or popover keeps it; a window takes it
        assert_eq!(area_at(&a, &ctx, pos2(10.0, 500.0), Some(egui::Order::Foreground)), None);
        assert_eq!(area_at(&a, &ctx, pos2(10.0, 500.0), Some(egui::Order::Middle)), Some(Area::Other));
        // the control bar above: no change
        assert_eq!(area_at(&a, &ctx, pos2(10.0, 20.0), None), None);
        a.bottom = Bottom::Mixer;
        assert_eq!(area_at(&a, &ctx, pos2(10.0, 500.0), None), Some(Area::Mixer));
    }

    #[test]
    fn a_closed_area_gives_focus_back_to_the_tracks() {
        let mut a = app();
        a.bottom = Bottom::PianoRoll;
        a.kf.area = Area::Editor;
        assert!(editor(&a));
        a.bottom = Bottom::None;
        assert_eq!(area(&a), Area::Tracks);
    }

    #[test]
    fn tab_cycles_the_shown_areas() {
        let mut a = app();
        a.show_browser = false;
        a.bottom = Bottom::PianoRoll;
        a.kf.area = Area::Tracks;
        assert!(command(&mut a, "Cycle Through Window Views"));
        assert_eq!(area(&a), Area::Editor);
        command(&mut a, "Cycle Through Window Views");
        assert_eq!(area(&a), Area::Tracks);
        command(&mut a, "Cycle Through Window Views (counter-clockwise)");
        assert_eq!(area(&a), Area::Editor);
    }
}
