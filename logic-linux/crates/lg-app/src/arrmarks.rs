//! The Arrangement track's markers as Logic edits them (lg-core arrmarks): click selects,
//! Shift-click extends (always a contiguous run), Option-click moves the playhead there, a drag
//! moves the section with its content (dropped between two: the later ones make room; onto
//! another: the two swap; Option: a copy; Command onto another: replaces it), the boundary
//! between two is a Junction (both resize), outer edges trim, a click on the name offers the
//! section names and Rename, a double-click elsewhere renames, Option-double-click makes a
//! marker there and opens the Marker List. Delete clears a section's content first, then the
//! marker (closing the gap); ⌘T splits at the playhead. The header's shortcut menu suspends
//! the content connection.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke};
use lg_core::arrmarks as am;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &["Cut Sections of Selected Arrangement Markers"];

/// The names the name menu offers.
pub const NAMES: [&str; 10] = ["Intro", "Verse", "Pre-Chorus", "Chorus", "Post-Chorus", "Bridge", "Breakdown", "Solo", "Interlude", "Outro"];

const EDGE: f32 = 5.0;

#[derive(Clone, Debug)]
enum Drag {
    Move { grab: f32 },
    Junction { k: usize },
    Trim { k: usize, left: bool },
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// Selected sections: a contiguous run (first, last).
    pub sel: Option<(usize, usize)>,
    /// The last click was on the Arrangement track (Delete and ⌘T act on its markers).
    pub focus: bool,
    /// Suspend Content Connection: markers change without their content.
    pub suspended: bool,
    drag: Option<Drag>,
    /// Renaming: (section, text).
    pub renaming: Option<(usize, String)>,
    /// The headers' shortcut menu was opened over the Arrangement header.
    pub menu_here: bool,
}

impl State {
    pub fn range(&self) -> Option<std::ops::Range<usize>> {
        self.sel.map(|(a, b)| a..b + 1)
    }
}

/// Runs `f` on the project with the app's markers and arrangement in it, as one undo step.
pub fn edit<R>(app: &mut App, what: &str, f: impl FnOnce(&mut lg_core::Project) -> R) -> Option<R> {
    app.project.as_ref()?;
    app.checkpoint(what);
    let markers = app.markers.clone();
    let arrangement = app.arrangement.clone();
    let p = app.project.as_mut()?;
    p.markers = markers;
    p.arrangement = arrangement;
    let r = f(p);
    app.markers = p.markers.clone();
    app.arrangement = p.arrangement.clone();
    app.after_edit();
    Some(r)
}

fn connected(app: &App) -> bool {
    !app.arrm.suspended
}

/// The Arrangement track's name in its header (Logic: "Arrangement (suspended)").
pub fn header_name(app: &App) -> &'static str {
    if app.arrm.suspended {
        "Arrangement (suspended)"
    } else {
        "Arrangement"
    }
}

/// The header shortcut menu's items for the Arrangement track.
pub fn header_items(app: &mut App, ui: &mut egui::Ui) {
    if !app.arrm.menu_here {
        return;
    }
    if ui.add(egui::Button::selectable(app.arrm.suspended, "Suspend Content Connection")).clicked() {
        app.arrm.suspended = !app.arrm.suspended;
        ui.close();
    }
    ui.separator();
}

/// Delete (twice), ⌘T and Cut Sections of Selected Arrangement Markers on the selected markers.
pub fn command(app: &mut App, name: &str) -> bool {
    let Some(r) = app.arrm.range().filter(|_| app.arrm.focus) else { return false };
    if r.end > app.arrangement.len() {
        app.arrm.sel = None;
        return false;
    }
    let c = connected(app);
    match name {
        "Delete" => {
            let has = app.project.as_ref().is_some_and(|p| {
                let mut q = p.clone();
                q.markers = app.markers.clone();
                q.arrangement = app.arrangement.clone();
                am::has_content(&q, r.clone())
            });
            if has && c {
                edit(app, "Delete Arrangement Content", |p| am::clear_content(p, r));
            } else {
                edit(app, "Delete Arrangement Marker", |p| am::delete(p, r, c));
                app.arrm.sel = None;
            }
            true
        }
        "Cut Sections of Selected Arrangement Markers" => {
            edit(app, "Cut Sections", |p| am::delete(p, r, c));
            app.arrm.sel = None;
            true
        }
        "Split Regions/Events at Playhead Position" | "Split by Playhead" => {
            let at = app.position() as u32;
            if let Some(Some(k)) = edit(app, "Split Arrangement Marker", |p| am::split(p, at)) {
                app.arrm.sel = Some((k, k));
            }
            true
        }
        _ => false,
    }
}

fn select(app: &mut App, k: usize, shift: bool) {
    app.arrm.focus = true;
    app.selected_region = None;
    app.arrm.sel = match (shift, app.arrm.sel) {
        (true, Some((a, b))) => Some((a.min(k), b.max(k))),
        _ => Some((k, k)),
    };
}

/// Where a drop at `tick` goes: onto section `j` (its middle half) or before index `b`.
pub fn drop_target(secs: &[am::Section], tick: f32) -> (Option<usize>, usize) {
    for (j, s) in secs.iter().enumerate() {
        let (a, e) = (s.0 as f32, (s.0 + s.1) as f32);
        if tick >= a && tick < e {
            let q = (tick - a) / (e - a).max(1.0);
            if (0.25..0.75).contains(&q) {
                return (Some(j), j);
            }
            return (None, if q < 0.25 { j } else { j + 1 });
        }
    }
    (None, if secs.first().is_some_and(|s| tick < s.0 as f32) { 0 } else { secs.len() })
}

/// Draws the Arrangement lane and handles its gestures.
pub fn lane(app: &mut App, ui: &mut egui::Ui, head: Rect, row: Rect, lane: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    // the header menu's Arrangement items show when it opens over this header
    if ui.input(|i| i.pointer.secondary_pressed()) {
        app.arrm.menu_here = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| head.contains(h));
    }
    let (alt, cmd, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.command, i.modifiers.shift));
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    if pressed && !ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| lane.contains(h)) {
        app.arrm.focus = false;
    }
    let lp = ui.painter_at(lane);
    let bar_t = app.bar_ticks() as f32;
    let ppt = (x_of(bar_t) - x_of(0.0)) / bar_t;
    let secs = app.arrangement.clone();
    if app.arrm.sel.is_some_and(|(_, b)| b >= secs.len()) {
        app.arrm.sel = None;
    }
    let bg = ui.interact(lane, ui.scope_id().with("arr-lane"), Sense::click());
    if bg.clicked() {
        app.arrm.sel = None;
        app.arrm.focus = true;
    }
    let sel = app.arrm.range();
    for (k, (start, len, name)) in secs.iter().enumerate() {
        let r = Rect::from_min_max(pos2(x_of(*start as f32), row.min.y + 1.0), pos2(x_of((start + len) as f32), row.max.y - 1.0));
        if r.max.x < lane.min.x || r.min.x > lane.max.x {
            continue;
        }
        let selected = sel.as_ref().is_some_and(|s| s.contains(&k));
        // Logic 13: grey sections end to end, the name centred, a thin divider between
        let fill = if selected { Color32::from_gray(0x5a) } else { Color32::from_gray(0x36) };
        lp.rect_filled(r, 0.0, fill);
        lp.vline(r.min.x, r.y_range(), Stroke::new(1.0, Color32::from_gray(0x1e)));
        // Logic 13: ≈11 pt, light grey, centred
        let font = look::regular(11.0);
        let tw = ui.fonts_mut(|f| f.layout_no_wrap(name.clone(), font.clone(), Color32::WHITE).size().x);
        let name_r = Rect::from_center_size(r.center(), vec2(tw + 8.0, r.height())).intersect(r);
        lp.with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, name, font, if selected { Color32::WHITE } else { Color32::from_gray(0xdc) });
        let body = ui.interact(r, ui.scope_id().with(("arr", k)), Sense::click_and_drag());
        let shared_left = k > 0 && secs[k - 1].0 + secs[k - 1].1 == *start;
        let shared_right = secs.get(k + 1).is_some_and(|n| n.0 == start + len);
        let left = Rect::from_min_max(pos2(r.min.x - 1.0, r.min.y), pos2(r.min.x + EDGE, r.max.y));
        let right = Rect::from_min_max(pos2(r.max.x - EDGE, r.min.y), pos2(r.max.x + 1.0, r.max.y));
        let lr = ui.interact(left, ui.scope_id().with(("arr-l", k)), Sense::drag());
        let rr = ui.interact(right, ui.scope_id().with(("arr-r", k)), Sense::drag());
        // the Junction tool between two, the Trim tool at the outer edges
        if (lr.hovered() && !shared_left && k == 0) || (rr.hovered() && !shared_right && k + 1 == secs.len()) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        } else if rr.hovered() && shared_right {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
        }
        if rr.drag_started() {
            if shared_right {
                app.checkpoint("Resize Arrangement Markers");
                app.arrm.drag = Some(Drag::Junction { k });
            } else if k + 1 == secs.len() {
                app.checkpoint("Resize Arrangement Marker");
                app.arrm.drag = Some(Drag::Trim { k, left: false });
            }
        }
        if lr.drag_started() && k == 0 {
            app.checkpoint("Resize Arrangement Marker");
            app.arrm.drag = Some(Drag::Trim { k, left: true });
        }
        if body.drag_started() {
            if !sel.as_ref().is_some_and(|s| s.contains(&k)) {
                select(app, k, false);
            }
            let grab = body.interact_pointer_pos().map_or(0.0, |p| tick_of(p.x) - *start as f32);
            app.arrm.drag = Some(Drag::Move { grab });
        }
        if body.clicked() {
            let on_name = body.interact_pointer_pos().is_some_and(|p| name_r.contains(p));
            if alt {
                app.locate(*start as f64);
            } else {
                select(app, k, shift);
                if on_name && !shift && !cmd {
                    ui.data_mut(|d| d.insert_temp(ui.scope_id().with("arr-names"), k));
                }
            }
        }
        if body.double_clicked() {
            ui.data_mut(|d| d.remove::<usize>(ui.scope_id().with("arr-names")));
            if alt {
                // a marker there (snapped to the bar) and the Marker List
                let t = body.interact_pointer_pos().map_or(*start as f32, |p| tick_of(p.x));
                let at = crate::arranging::snap_tick(app, t, ppt).max(0.0) as u32;
                crate::markers::create_at(app, at);
                app.mk.list_open = true;
            } else {
                app.arrm.renaming = Some((k, name.clone()));
            }
        }
    }
    // the name menu: the section names and Rename
    let mid = ui.scope_id().with("arr-names");
    if let Some(k) = ui.data(|d| d.get_temp::<usize>(mid)) {
        if let Some(s) = secs.get(k) {
            let r = Rect::from_min_max(pos2(x_of(s.0 as f32), row.max.y), pos2(x_of((s.0 + s.1) as f32), row.max.y + 1.0));
            let mut close = false;
            let area = egui::Area::new(mid.with("area")).order(egui::Order::Foreground).fixed_pos(pos2(r.center().x - 60.0, r.min.y));
            let resp = area.show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(120.0);
                    for n in NAMES {
                        if ui.add(egui::Button::selectable(s.2 == n, n)).clicked() {
                            let n = n.to_string();
                            edit(app, "Rename Arrangement Marker", |p| p.arrangement[k].2 = n);
                            close = true;
                        }
                    }
                    ui.separator();
                    if ui.button("Rename").clicked() {
                        app.arrm.renaming = Some((k, s.2.clone()));
                        close = true;
                    }
                });
            });
            let away = ui.input(|i| i.pointer.primary_pressed()) && !resp.response.contains_pointer();
            if close || away || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                ui.data_mut(|d| d.remove::<usize>(mid));
            }
        } else {
            ui.data_mut(|d| d.remove::<usize>(mid));
        }
    }
    drag(app, ui, &lp, row, x_of, tick_of, ppt, alt, cmd);
    rename_field(app, ui, row, x_of);
}

#[allow(clippy::too_many_arguments)]
fn drag(app: &mut App, ui: &mut egui::Ui, lp: &egui::Painter, row: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32, ppt: f32, alt: bool, cmd: bool) {
    let Some(d) = app.arrm.drag.clone() else { return };
    let Some(pp) = ui.input(|i| i.pointer.latest_pos()) else { return };
    let down = ui.input(|i| i.pointer.primary_down());
    let t = crate::arranging::snap_tick(app, tick_of(pp.x), ppt).max(0.0) as u32;
    let min = app.bar_ticks();
    match d {
        Drag::Junction { k } => {
            if let Some(p) = app.project.as_mut() {
                p.arrangement = app.arrangement.clone();
                am::junction(p, k, t, min);
                app.arrangement = p.arrangement.clone();
            }
        }
        Drag::Trim { k, left } => {
            if let Some(p) = app.project.as_mut() {
                p.arrangement = app.arrangement.clone();
                am::trim(p, k, left, t, min);
                app.arrangement = p.arrangement.clone();
            }
        }
        Drag::Move { grab } => {
            let Some(r) = app.arrm.range() else {
                app.arrm.drag = None;
                return;
            };
            let at = tick_of(pp.x) - grab + grab.min(0.0);
            let (onto, before) = drop_target(&app.arrangement, tick_of(pp.x));
            let onto = onto.filter(|j| !r.contains(j));
            // the drop's feedback: a line between sections, or the target outlined
            if let Some(j) = onto {
                let s = &app.arrangement[j];
                lp.rect_stroke(Rect::from_min_max(pos2(x_of(s.0 as f32), row.min.y + 1.0), pos2(x_of((s.0 + s.1) as f32), row.max.y - 1.0)), 0.0, Stroke::new(2.0, look::accent()), egui::StrokeKind::Inside);
            } else {
                let x = app.arrangement.get(before).map_or_else(|| app.arrangement.last().map_or(x_of(0.0), |s| x_of((s.0 + s.1) as f32)), |s| x_of(s.0 as f32));
                lp.vline(x, row.y_range(), Stroke::new(2.0, look::accent()));
            }
            let _ = at;
            if !down {
                let c = connected(app);
                let n = r.end - r.start;
                match onto {
                    Some(j) if cmd && n == 1 => {
                        edit(app, if alt { "Copy Arrangement Marker" } else { "Replace Arrangement Marker" }, |p| am::replace(p, r.start, j, alt, c));
                        let j = if !alt && r.start < j { j - 1 } else { j };
                        app.arrm.sel = Some((j, j));
                    }
                    Some(j) if !alt && n == 1 => {
                        edit(app, "Swap Arrangement Markers", |p| am::swap(p, r.start, j, c));
                        app.arrm.sel = Some((j, j));
                    }
                    _ => {
                        if alt {
                            if let Some(k) = edit(app, "Copy Arrangement Marker", |p| am::copy_block(p, r.clone(), before, c)) {
                                app.arrm.sel = Some((k, k + n - 1));
                            }
                        } else if !(before >= r.start && before <= r.end) {
                            if let Some(k) = edit(app, "Move Arrangement Marker", |p| am::move_block(p, r.clone(), before, c)) {
                                app.arrm.sel = Some((k, k + n - 1));
                            }
                        }
                    }
                }
            }
        }
    }
    if !down {
        app.arrm.drag = None;
        app.dirty = true;
    }
}

fn rename_field(app: &mut App, ui: &mut egui::Ui, row: Rect, x_of: &dyn Fn(f32) -> f32) {
    let Some((k, mut text)) = app.arrm.renaming.clone() else { return };
    let Some(s) = app.arrangement.get(k).cloned() else {
        app.arrm.renaming = None;
        return;
    };
    let er = Rect::from_min_size(pos2(x_of(s.0 as f32) + 2.0, row.min.y + 2.0), vec2(150.0, row.height() - 4.0));
    let te = ui.put(er, egui::TextEdit::singleline(&mut text).font(look::regular(12.0)));
    crate::widgets::focus_once(ui, &te);
    if te.lost_focus() {
        app.arrm.renaming = None;
        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && s.2 != text && !text.trim().is_empty() {
            edit(app, "Rename Arrangement Marker", |p| p.arrangement[k].2 = text);
        }
    } else {
        app.arrm.renaming = Some((k, text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_onto_or_between_sections() {
        let secs: Vec<am::Section> = vec![(0, 100, "A".into()), (100, 100, "B".into())];
        assert_eq!(drop_target(&secs, 50.0), (Some(0), 0));
        assert_eq!(drop_target(&secs, 10.0), (None, 0));
        assert_eq!(drop_target(&secs, 90.0), (None, 1));
        assert_eq!(drop_target(&secs, 195.0), (None, 2));
        assert_eq!(drop_target(&secs, 400.0), (None, 2));
    }

    #[test]
    fn delete_clears_content_then_the_marker() {
        let ctx = egui::Context::default();
        let mut app = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        app.set_project(crate::demo::song(), true);
        let bar = app.bar_ticks();
        app.arrangement = vec![(0, 8 * bar, "Intro".into()), (8 * bar, 8 * bar, "Verse".into())];
        app.arrm.sel = Some((0, 0));
        app.arrm.focus = true;
        let regions_before: usize = app.project.as_ref().unwrap().tracks.iter().map(|t| t.regions.len()).sum();
        assert!(command(&mut app, "Delete"));
        let regions_after: usize = app.project.as_ref().unwrap().tracks.iter().map(|t| t.regions.len()).sum();
        assert_eq!(app.arrangement.len(), 2);
        assert!(regions_after <= regions_before);
        assert!(command(&mut app, "Delete"));
        assert_eq!(app.arrangement.len(), 1);
        assert_eq!(app.arrangement[0].0, 0);
        // undo brings the marker back
        app.undo();
        assert_eq!(app.arrangement.len(), 2);
    }
}
