//! The Beat Mapping global track as Logic's: the upper part a ruler with the LCD's divisions
//! at the zoom; a click there makes a line at that position, and dragging the line's bottom
//! onto a note start (or transient) of a selected region maps that ruler position to it (a
//! tempo change, everything keeping its time; ⌃-drag: any position, no note needed). The
//! lower part shows the selected regions' note starts, and audio regions' transients (blue)
//! after Analyze Transients, with − / + in the header for fewer or more. Mappings: click
//! selects, Delete or an Eraser double-click removes, a click on the header selects them all.
//! The title's menu: Beats from Region, Analyze Transients, Protect MIDI, Protect Flex, Move
//! Selection with First Transient to Nearest Beat.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::PPQ;
use lg_core::timeline::{self, Map};

use crate::app::App;
use crate::edit::Tool;
use crate::look;

pub const COMMANDS: &[&str] = &["Move Selection with First Transient to Nearest Beat", "Analyze Transients", "Protect MIDI", "Protect Flex"];

const LINE: Color32 = Color32::from_rgb(0xe0, 0x9a, 0x3c);
const TRANSIENT: Color32 = Color32::from_rgb(0x4a, 0x8c, 0xe6);

pub struct State {
    pub protect_midi: bool,
    pub protect_flex: bool,
    /// Transients found: file → (sensitivity, seconds in the file).
    pub transients: HashMap<PathBuf, (f32, Vec<f64>)>,
    pub sensitivity: f32,
    /// A line made by a click on the ruler (tick), being dragged to a note.
    pub line: Option<u32>,
    /// Selected mappings (ticks).
    pub sel: Vec<u32>,
}

impl Default for State {
    fn default() -> Self {
        State { protect_midi: true, protect_flex: true, transients: HashMap::new(), sensitivity: 0.5, line: None, sel: Vec::new() }
    }
}

/// The selected regions' events: (tick, is a transient).
fn events(app: &App) -> Vec<(u32, bool)> {
    let Some(p) = &app.project else { return Vec::new() };
    let mut v = Vec::new();
    for (t, r) in app.region_selection() {
        let Some(rg) = p.tracks.get(t).and_then(|tr| tr.regions.get(r)) else { continue };
        match &rg.audio {
            None => v.extend(rg.notes.iter().filter(|n| n.pos >= rg.start && n.pos < rg.start + rg.length).map(|n| (n.pos, false))),
            Some(a) => {
                let (Some(rt), Some((_, secs))) = (crate::smarttempo::region_time(app, t, r), app.bm.transients.get(&a.file)) else { continue };
                for s in secs {
                    let rel = s - rt.offset_sec;
                    if rel >= 0.0 && rel <= rt.len_sec {
                        v.push((crate::smarttempo::tick_at(app, rt.start_sec + rel).round().max(0.0) as u32, true));
                    }
                }
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Analyze Transients for the selected audio regions' files (at the track's sensitivity).
pub fn analyze_transients(app: &mut App) {
    let Some(p) = &app.project else { return };
    let files: Vec<PathBuf> = app.region_selection().iter().filter_map(|&(t, r)| p.tracks.get(t)?.regions.get(r)?.audio.as_ref().map(|a| a.file.clone())).collect();
    let s = app.bm.sensitivity;
    for f in files {
        if app.bm.transients.get(&f).is_some_and(|x| (x.0 - s).abs() < 1e-3) {
            continue;
        }
        if let Ok(a) = lg_audio::AudioFile::read(&f) {
            let secs = lg_audio::transients::detect(&a.data, a.channels, a.rate, s).into_iter().map(|fr| fr as f64 / a.rate as f64).collect();
            app.bm.transients.insert(f, (s, secs));
        }
    }
}

/// Maps ruler position `tick` to the moment of `at` (a tick in the current map), one undo
/// step; regions keep their time (MIDI ones only with Protect MIDI).
pub fn map(app: &mut App, tick: u32, at: u32) -> bool {
    let Some(p) = app.project.as_mut() else { return false };
    let old = Map::of(p);
    let secs = old.secs_at(at as f64);
    let before = p.clone();
    if !timeline::beat_map(p, tick, secs) {
        *p = before;
        return false;
    }
    let mut after = p.clone();
    timeline::retime(&mut after, &old);
    let protect = app.bm.protect_midi;
    for (tr, tr2) in p.tracks.iter_mut().zip(after.tracks) {
        for (r, r2) in tr.regions.iter_mut().zip(tr2.regions) {
            if r.audio.is_some() || protect {
                *r = r2;
            }
        }
    }
    app.undo.push(("Beat Mapping".into(), before, app.programs.clone()));
    app.redo.clear();
    app.after_edit();
    true
}

/// Removes mappings (and the tempo change each made).
pub fn delete(app: &mut App, ticks: &[u32]) {
    if ticks.is_empty() {
        return;
    }
    app.checkpoint("Delete Beat Mapping");
    if let Some(p) = app.project.as_mut() {
        for &t in ticks {
            p.global.beat_maps.retain(|b| b.0 != t);
            timeline::delete_point(p, t);
        }
    }
    app.bm.sel.clear();
    app.after_edit();
}

/// Move Selection with First Transient to Nearest Beat: the selected audio regions move
/// together so the first one's first transient lands on the nearest beat.
pub fn first_transient_to_beat(app: &mut App) -> bool {
    analyze_transients(app);
    let first = events(app).into_iter().find(|e| e.1).map(|e| e.0);
    let Some(t) = first else {
        app.tell("No transients in the selected audio regions");
        return false;
    };
    let beat = PPQ as i64;
    let target = ((t as i64 + beat / 2) / beat) * beat;
    let d = target - t as i64;
    if d == 0 {
        return true;
    }
    app.checkpoint("Move to Nearest Beat");
    let sel = app.region_selection();
    if let Some(p) = app.project.as_mut() {
        for (tr, r) in sel {
            if let Some(rg) = p.tracks.get_mut(tr).and_then(|x| x.regions.get_mut(r)) {
                lg_core::regions::shift(rg, d);
            }
        }
    }
    app.after_edit();
    true
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Move Selection with First Transient to Nearest Beat" => first_transient_to_beat(app),
        "Analyze Transients" => {
            analyze_transients(app);
            true
        }
        "Protect MIDI" => {
            app.bm.protect_midi = !app.bm.protect_midi;
            true
        }
        "Protect Flex" => {
            app.bm.protect_flex = !app.bm.protect_flex;
            true
        }
        "Delete" if !app.bm.sel.is_empty() => {
            let s = app.bm.sel.clone();
            delete(app, &s);
            true
        }
        "Delete" if app.gt.sel == Some(crate::globals::BEAT_MAPPING) => {
            let all: Vec<u32> = app.project.as_ref().map_or(Vec::new(), |p| p.global.beat_maps.iter().map(|b| b.0).collect());
            if all.is_empty() {
                return false;
            }
            delete(app, &all);
            true
        }
        _ => false,
    }
}

/// The header: the title's menu and the transient − / + buttons.
pub fn header(app: &mut App, ui: &mut egui::Ui, row: Rect, lanes_x: f32) {
    let title = Rect::from_min_size(pos2(row.min.x + 10.0, row.min.y + 2.0), vec2(96.0, 22.0));
    let tr = ui.interact(title, ui.scope_id().with("bm-title"), Sense::click());
    egui::Popup::menu(&tr).show(|ui| {
        ui.set_min_width(280.0);
        let sel_midi = app.selected_region.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r).map(|rg| rg.audio.is_none()));
        if ui.add_enabled(sel_midi == Some(true), egui::Button::new("Beats from Region…")).clicked() {
            crate::arranging::command(app, "Beats from Region");
            ui.close();
        }
        if ui.button("Analyze Transients").clicked() {
            analyze_transients(app);
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::selectable(app.bm.protect_midi, "Protect MIDI")).clicked() {
            app.bm.protect_midi = !app.bm.protect_midi;
            ui.close();
        }
        if ui.add(egui::Button::selectable(app.bm.protect_flex, "Protect Flex")).clicked() {
            app.bm.protect_flex = !app.bm.protect_flex;
            ui.close();
        }
        ui.separator();
        if ui.button("Move Selection with First Transient to Nearest Beat").clicked() {
            first_transient_to_beat(app);
            ui.close();
        }
    });
    // − / + : fewer or more transients
    for (k, (glyph, d)) in [(crate::glyphs::ph::MINUS, -0.1f32), (crate::glyphs::ph::PLUS, 0.1)].into_iter().enumerate() {
        let r = Rect::from_center_size(pos2(lanes_x - 52.0 + k as f32 * 22.0, row.max.y - 14.0), vec2(18.0, 18.0));
        let resp = ui.interact(r, ui.scope_id().with(("bm-thr", k)), Sense::click()).on_hover_text(if d < 0.0 { "Fewer transients" } else { "More transients" });
        crate::glyphs::paint(ui.painter(), r, glyph, 13.0, if resp.hovered() { look::text() } else { look::text_dim() });
        if resp.clicked() {
            app.bm.sensitivity = (app.bm.sensitivity + d).clamp(0.0, 1.0);
            app.bm.transients.clear();
            analyze_transients(app);
        }
    }
}

/// The lane: the ruler, the selected regions' events, the mappings, and the gestures.
pub fn lane(app: &mut App, ui: &mut egui::Ui, lane: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(proj) = app.project.clone() else { return };
    let p = ui.painter_at(lane);
    let ruler = Rect::from_min_max(lane.min, pos2(lane.max.x, lane.min.y + (lane.height() * 0.4).max(14.0)));
    let low = Rect::from_min_max(pos2(lane.min.x, ruler.max.y), lane.max);
    p.rect_filled(ruler, 0.0, Color32::from_gray(0x2a));
    p.hline(lane.x_range(), ruler.max.y, Stroke::new(1.0, look::line()));
    // the ruler's divisions as the LCD shows them at this zoom: bars, beats, divisions
    let m = Map::of(&proj);
    let bar_t = lg_core::project::bar_ticks(proj.signature) as f32;
    let ppt = (x_of(bar_t) - x_of(0.0)) / bar_t;
    let step = [3840.0f32, 960.0, 240.0, 120.0, 60.0].into_iter().filter(|s| s * ppt >= 6.0).last().unwrap_or(bar_t);
    let (a, b) = (tick_of(lane.min.x).max(0.0), tick_of(lane.max.x));
    let mut t = (a / step).floor() * step;
    while t <= b {
        let x = x_of(t);
        let (bar, start, _) = m.bar_of(t as u32);
        let on_bar = start == t as u32;
        let on_beat = (t as u32 - start) % 960 == 0;
        let h = if on_bar { ruler.height() } else if on_beat { ruler.height() * 0.5 } else { ruler.height() * 0.25 };
        p.vline(x, ruler.max.y - h..=ruler.max.y, Stroke::new(1.0, if on_bar { look::text_dim() } else { Color32::from_gray(0x55) }));
        if on_bar && step < bar_t {
            p.text(pos2(x + 2.0, ruler.min.y + 1.0), Align2::LEFT_TOP, format!("{}", bar + 1), look::regular(9.5), look::text_dim());
        }
        t += step;
    }
    // the selected regions' note starts and transients
    let evs = events(app);
    for &(t, tr) in &evs {
        let x = x_of(t as f32);
        if lane.x_range().contains(x) {
            p.vline(x, low.min.y + 3.0..=low.max.y - 2.0, Stroke::new(1.0, if tr { TRANSIENT } else { look::text_dim() }));
        }
    }
    // the mappings
    for &(t, _) in &proj.global.beat_maps {
        let x = x_of(t as f32);
        let sel = app.bm.sel.contains(&t);
        p.vline(x, lane.y_range(), Stroke::new(if sel { 2.0 } else { 1.0 }, LINE));
        p.circle_filled(pos2(x, ruler.min.y + 5.0), 3.0, LINE);
    }
    let protected = app.gt.protect[crate::globals::BEAT_MAPPING];
    let resp = ui.interact(lane, ui.scope_id().with("bm-lane"), Sense::click_and_drag());
    let near_map = |x: f32| proj.global.beat_maps.iter().map(|b| b.0).find(|&t| (x_of(t as f32) - x).abs() < 5.0);
    if protected {
        return;
    }
    if resp.double_clicked() && app.tool == Tool::Eraser {
        if let Some(t) = resp.interact_pointer_pos().and_then(|pp| near_map(pp.x)) {
            delete(app, &[t]);
            return;
        }
    }
    if resp.clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            if let Some(t) = near_map(pp.x) {
                if ui.input(|i| i.modifiers.shift) {
                    app.bm.sel.push(t);
                } else {
                    app.bm.sel = vec![t];
                }
            } else if ruler.contains(pp) {
                // a line at the ruler position (snapped to the division)
                let t = ((tick_of(pp.x) / step).round() * step).max(0.0) as u32;
                app.bm.line = Some(t);
                app.bm.sel.clear();
            } else {
                app.bm.sel.clear();
                app.bm.line = None;
            }
        }
    }
    // the line, and dragging its bottom to a note start
    if let Some(t) = app.bm.line {
        let x = x_of(t as f32);
        p.vline(x, lane.y_range(), Stroke::new(1.0, LINE.gamma_multiply(0.7)));
        if resp.drag_started() {
            if resp.interact_pointer_pos().is_some_and(|pp| (pp.x - x).abs() > 8.0) {
                app.bm.line = None;
            }
        }
        if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let ctrl = ui.input(|i| i.modifiers.ctrl);
            let target = if ctrl { Some(tick_of(pp.x).max(0.0) as u32) } else { evs.iter().map(|e| e.0).min_by_key(|&e| (x_of(e as f32) - pp.x).abs() as u32).filter(|&e| (x_of(e as f32) - pp.x).abs() < 10.0) };
            let bx = target.map_or(pp.x, |e| x_of(e as f32));
            p.line_segment([pos2(x, ruler.max.y), pos2(bx, lane.max.y)], Stroke::new(1.5, LINE));
            ui.data_mut(|d| d.insert_temp(ui.scope_id().with("bm-target"), target));
        }
        if resp.drag_stopped() {
            let target: Option<u32> = ui.data(|d| d.get_temp(ui.scope_id().with("bm-target"))).flatten();
            if let Some(at) = target {
                if map(app, t, at) {
                    app.bm.line = None;
                } else {
                    app.tell("That mapping would need a tempo outside 5–990 bpm");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_a_note_and_deleting_it() {
        let ctx = egui::Context::default();
        let mut app = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        app.set_project(crate::demo::song(), true);
        let old = Map::of(app.project.as_ref().unwrap());
        let tm = app.project.as_ref().unwrap().tempo_map.clone();
        // ruler bar 3 (7680) mapped to where tick 7200 sounds now
        let secs = old.secs_at(7200.0);
        assert!(map(&mut app, 7680, 7200));
        let p = app.project.as_ref().unwrap();
        assert!((Map::of(p).secs_at(7680.0) - secs).abs() < 1e-6);
        assert!(p.global.beat_maps.iter().any(|b| b.0 == 7680));
        delete(&mut app, &[7680]);
        assert!(app.project.as_ref().unwrap().global.beat_maps.is_empty());
        app.undo();
        app.undo();
        assert_eq!(app.project.as_ref().unwrap().tempo_map, tm);
    }
}
