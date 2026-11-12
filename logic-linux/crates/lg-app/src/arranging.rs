//! Arranging as Logic does (lg-core regions, timeline): the Tracks area's Snap and Drag modes
//! (the "…" menu) applied to region drags, regions dragged across tracks, aliases (MIDI) and
//! clones (audio) with the Region Alias commands, Mute Notes, the Piano Roll's MIDI Draw lanes
//! for controllers / pitch bend / aftertouch / program changes, the Tempo track (points, lines,
//! curves, sets), the Signature track (time and key changes), the Tempo List, Tempo
//! Operations, Adjust Tempo using Region Length and Locators and Beats from Region (beat
//! mapping).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::regions::{self, DragMode, SnapMode};
use lg_core::timeline::{self, CurveType, Map, Operation};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Make Alias",
    "Re-Assign Alias",
    "Convert Alias to a Copy",
    "Convert Alias to a Region Copy",
    "Select Original Region",
    "Select Aliases of Region",
    "Select All Aliases of Region",
    "Select Orphan Aliases",
    "Delete Orphan Aliases",
    "Repeat Regions/Events as Alias",
    "Mute Notes",
    "Show Tempo List",
    "Open Tempo List…",
    "Show/Hide Tempo List",
    "Tempo Operations…",
    "Open Tempo Operations…",
    "Adjust Tempo using Region Length and Locators",
    "Create Time Signature Change",
    "Create Key Signature Change",
    "Create Tempo Change",
    "Beats from Region",
];

#[derive(Clone, Debug)]
pub struct OpsDialog {
    op: usize,
    from: u32,
    to: u32,
    start: f64,
    end: f64,
    kind: usize,
    curvature: i8,
    density: u32,
    keep: bool,
    percent: f64,
    new_end: u32,
    random: f64,
    tenths: bool,
}

#[derive(Clone, Debug)]
pub struct SigDialog {
    bar: u32,
    key: bool,
    num: u8,
    den: u8,
    grouping: String,
    key_name: String,
    /// Editing the one at this bar (None: a new one).
    editing: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TDrag {
    Point { from: u32, y0: f32, bpm0: f64, axis: Option<bool>, copy: bool, at: u32 },
    Line { k: usize, y0: f32, orig: (f64, f64) },
    Curve { at: u32, y0: f32, c0: i8 },
}

#[derive(Default)]
pub struct State {
    pub snap: SnapMode,
    pub snap_relative: bool,
    pub drag: DragMode,
    /// Selected tempo points (ticks).
    pub tempo_sel: Vec<u32>,
    tdrag: Option<TDrag>,
    /// Tempo points snap to (ticks; 0: free).
    pub tempo_snap: u32,
    pub tempo_list: bool,
    pub curve_events: bool,
    pub granularity: u32,
    pub ops: Option<OpsDialog>,
    pub sig: Option<SigDialog>,
    pub fit: Option<(u32, u32, f64)>,
    set_rename: Option<String>,
    /// The Piano Roll's MIDI Draw lane: (status kind, controller number); None: velocity.
    pub lane: Option<(u8, u8)>,
    /// A region drag: the length when it started (resize) and where it started (move).
    pub drag_origin: Option<(usize, u32)>,
}

pub fn drag_mode_name(app: &App) -> &'static str {
    app.arr.drag.name()
}

// ---------------------------------------------------------------------------------------------
// snap and drag modes

fn sig(app: &App) -> (u8, u8) {
    app.project.as_ref().map_or((4, 4), |p| p.signature)
}

/// The Snap grid in ticks at this zoom.
pub fn snap_grid(app: &App, px_per_tick: f32) -> f32 {
    regions::snap_grid(app.arr.snap, sig(app), lg_core::events::PPQ / 4, app.tempo(), px_per_tick as f64, 8.0) as f32
}

/// Where a dragged region starts: `new` unsnapped, `start` where it was; Shift: no snapping.
pub fn snap_region(app: &App, new: f32, start: u32, px_per_tick: f32, free: bool) -> u32 {
    if free || !app.snap_on {
        return new.max(0.0) as u32;
    }
    let g = snap_grid(app, px_per_tick) as f64;
    regions::snap_move(start, new as f64 - start as f64, g, app.arr.snap_relative)
}

/// A snapped tick (edges, tempo points).
pub fn snap_tick(app: &App, tick: f32, px_per_tick: f32) -> f32 {
    if !app.snap_on {
        return tick.max(0.0);
    }
    let g = snap_grid(app, px_per_tick);
    ((tick / g).round() * g).max(0.0)
}

pub fn ms_per_tick(app: &App) -> f64 {
    60_000.0 / app.tempo().max(1.0) / lg_core::events::PPQ as f64
}

/// Region `r` dragged onto track `to`: moved there when the kinds fit (MIDI on instrument
/// tracks, audio on audio tracks). Returns its index there.
pub fn move_to_track(app: &mut App, from: usize, r: usize, to: usize) -> Option<usize> {
    let p = app.project.as_mut()?;
    if from == to || to >= p.tracks.len() {
        return None;
    }
    let audio = p.tracks.get(from)?.regions.get(r)?.audio.is_some();
    let fits = (p.tracks[to].kind == lg_core::TrackKind::Audio) == audio && p.tracks[to].extra.stack.is_none();
    if !fits {
        return None;
    }
    let rg = p.tracks[from].regions.remove(r);
    p.tracks[to].regions.push(rg);
    let k = p.tracks[to].regions.len() - 1;
    app.selected_region = Some((to, k));
    app.selected_track = Some(to);
    app.selected_regions.retain(|x| *x != (from, r));
    Some(k)
}

/// A region move or copy ended on track `t`: the Drag mode's effect on the others.
pub fn after_move(app: &mut App, t: usize, r: usize) {
    let mode = app.arr.drag;
    let mpt = ms_per_tick(app);
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    let k = regions::after_move(&mut tr.regions, r, mode, mpt);
    if app.selected_region == Some((t, r)) {
        app.selected_region = Some((t, k));
    }
}

/// A region's right edge drag ended (it was `old` ticks long).
pub fn after_resize(app: &mut App, t: usize, r: usize, old: u32) {
    let mode = app.arr.drag;
    let mpt = ms_per_tick(app);
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    let Some(len) = tr.regions.get(r).map(|x| x.length) else { return };
    let k = regions::after_resize(&mut tr.regions, r, true, len as i64 - old as i64, mode, mpt);
    if app.selected_region == Some((t, r)) {
        app.selected_region = Some((t, k));
    }
}

/// Option-Shift-drag: an alias (MIDI) or a clone (audio) of region `r`, placed where it is;
/// returns its index.
pub fn alias_for_drag(app: &mut App, t: usize, r: usize) -> Option<usize> {
    let p = app.project.as_mut()?;
    let mut next = regions::max_id(&p.tracks);
    let tr = p.tracks.get_mut(t)?;
    let start = tr.regions.get(r)?.start;
    let a = regions::make_alias(tr.regions.get_mut(r)?, start, &mut next);
    tr.regions.push(a);
    Some(tr.regions.len() - 1)
}

/// The "…" menu's Snap and Drag submenus (their items share names with other menus).
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    match it.label.as_str() {
        "Snap" if !it.children.is_empty() => {
            ui.menu_button("Snap", |ui| {
                ui.set_min_width(240.0);
                if ui.button(if app.snap_on { "✓ Snap to Grid" } else { "Snap to Grid" }).clicked() {
                    app.snap_on = !app.snap_on;
                    ui.close();
                }
                ui.separator();
                for m in SnapMode::ALL {
                    let on = app.arr.snap == m;
                    if ui.button(if on { format!("✓ {}", m.name()) } else { m.name().to_string() }).clicked() {
                        app.arr.snap = m;
                        ui.close();
                    }
                }
                ui.separator();
                for (rel, n) in [(false, "Snap Regions to Absolute Value"), (true, "Snap Regions to Relative Value")] {
                    if ui.button(if app.arr.snap_relative == rel { format!("✓ {n}") } else { n.to_string() }).clicked() {
                        app.arr.snap_relative = rel;
                        ui.close();
                    }
                }
                // the rest of Logic's Snap menu, as before
                let rest: Vec<crate::menus::Item> = it.children.iter().skip_while(|c| c.label != "Snap Flexed Audio Regions by First Downbeat").cloned().collect();
                if !rest.is_empty() {
                    ui.separator();
                    crate::menus::show_items(app, ui, &rest);
                }
            });
            true
        }
        "Drag" if !it.children.is_empty() => {
            ui.menu_button("Drag", |ui| {
                for m in DragMode::ALL {
                    let on = app.arr.drag == m;
                    if ui.button(if on { format!("✓ {}", m.name()) } else { m.name().to_string() }).clicked() {
                        app.arr.drag = m;
                        ui.close();
                    }
                }
            });
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// commands

pub fn enabled(app: &App, label: &str) -> Option<bool> {
    let p = app.project.as_ref()?;
    let sel = app.selected_region.and_then(|(t, r)| p.tracks.get(t)?.regions.get(r));
    Some(match label {
        "Make Alias" | "Repeat Regions/Events as Alias" => sel.is_some_and(|r| r.audio.is_none()),
        "Convert Alias to a Copy" | "Convert Alias to a Region Copy" | "Select Original Region" => sel.is_some_and(regions::is_alias),
        "Select Aliases of Region" | "Select All Aliases of Region" => sel.is_some_and(|r| r.ext.id != 0),
        "Re-Assign Alias" => app.selected_regions.len() == 2,
        "Adjust Tempo using Region Length and Locators" => sel.is_some(),
        "Beats from Region" => sel.is_some_and(|r| r.audio.is_none() && !r.notes.is_empty()),
        "Mute Notes" => !app.selected_notes.is_empty(),
        _ => return None,
    })
}

fn selection(app: &App) -> Vec<(usize, usize)> {
    let mut v = app.selected_regions.clone();
    if let Some(s) = app.selected_region {
        if !v.contains(&s) {
            v.push(s);
        }
    }
    v
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Make Alias" | "Repeat Regions/Events as Alias" => {
            let sel = selection(app);
            if sel.is_empty() {
                return true;
            }
            app.checkpoint(name);
            let at = if name == "Make Alias" { Some(app.position() as u32) } else { None };
            let first = sel.iter().filter_map(|&(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r).map(|x| x.start)).min().unwrap_or(0);
            let dest_track = app.selected_track;
            let mut made = Vec::new();
            if let Some(p) = app.project.as_mut() {
                let mut next = regions::max_id(&p.tracks);
                let first_t = sel.iter().map(|x| x.0).min().unwrap_or(0);
                for &(t, r) in &sel {
                    let Some(orig) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) else { continue };
                    if orig.audio.is_some() && at.is_some() {
                        continue;
                    }
                    // Make Alias: at the playhead, the first on the selected track (relative
                    // track and time kept); Repeat as Alias: right after the original
                    let (start, to_t) = match at {
                        Some(a) => (a + orig.start - first, dest_track.map_or(t, |d| d + t - first_t)),
                        None => (orig.start + orig.length, t),
                    };
                    let a = regions::make_alias(orig, start, &mut next);
                    let dest = to_t.min(p.tracks.len() - 1);
                    if let Some(tr) = p.tracks.get_mut(dest) {
                        tr.regions.push(a);
                        made.push((dest, tr.regions.len() - 1));
                    }
                }
            }
            if let Some(&last) = made.last() {
                app.selected_region = Some(last);
                app.selected_regions = made;
            }
            app.load_engine();
        }
        "Convert Alias to a Copy" | "Convert Alias to a Region Copy" => {
            app.checkpoint("Convert Alias to a Copy");
            for (t, r) in selection(app) {
                if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    regions::convert_to_copy(rg);
                }
            }
            app.load_engine();
        }
        "Select Original Region" => {
            let id = app.selected_region.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r).map(|x| x.ext.alias_of));
            if let Some(o) = id.and_then(|id| regions::original_of(&app.project.as_ref()?.tracks, id)) {
                app.selected_region = Some(o);
                app.selected_regions = vec![o];
                app.selected_track = Some(o.0);
            } else {
                app.tell("The original of this alias was deleted (an orphan alias)");
            }
        }
        "Select Aliases of Region" | "Select All Aliases of Region" => {
            let id = app.selected_region.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r).map(|x| x.ext.id)).unwrap_or(0);
            let all = app.project.as_ref().map(|p| regions::aliases_of(&p.tracks, id)).unwrap_or_default();
            if let Some(&f) = all.first() {
                app.selected_region = Some(f);
            }
            app.selected_regions = all;
        }
        "Re-Assign Alias" => {
            // the alias of the two follows the other from now on
            let sel = app.selected_regions.clone();
            if sel.len() == 2 {
                app.checkpoint(name);
                if let Some(p) = app.project.as_mut() {
                    let (a, b) = (sel[0], sel[1]);
                    let is_a = p.tracks[a.0].regions.get(a.1).is_some_and(regions::is_alias);
                    let (alias, orig) = if is_a { (a, b) } else { (b, a) };
                    let mut next = regions::max_id(&p.tracks);
                    if let Some(id) = p.tracks.get_mut(orig.0).and_then(|t| t.regions.get_mut(orig.1)).map(|o| regions::ensure_id(o, &mut next)) {
                        if let Some(al) = p.tracks.get_mut(alias.0).and_then(|t| t.regions.get_mut(alias.1)) {
                            al.ext.alias_of = id;
                            al.ext.id = 0;
                        }
                    }
                }
                app.load_engine();
            }
        }
        "Select Orphan Aliases" | "Delete Orphan Aliases" => {
            let orphans: Vec<(usize, usize)> = app.project.as_ref().map(|p| {
                p.tracks.iter().enumerate().flat_map(|(t, tr)| tr.regions.iter().enumerate().filter(|(_, r)| regions::is_alias(r) && regions::original_of(&p.tracks, r.ext.alias_of).is_none()).map(move |(k, _)| (t, k))).collect()
            }).unwrap_or_default();
            if name.starts_with("Delete") {
                if !orphans.is_empty() {
                    app.checkpoint(name);
                    if let Some(p) = app.project.as_mut() {
                        for &(t, k) in orphans.iter().rev() {
                            p.tracks[t].regions.remove(k);
                        }
                    }
                    app.selected_region = None;
                    app.selected_regions.clear();
                    app.after_edit();
                }
            } else {
                app.selected_regions = orphans;
            }
        }
        "Mute Notes" => mute_notes(app),
        "Show Tempo List" | "Open Tempo List…" | "Show/Hide Tempo List" => app.arr.tempo_list = !app.arr.tempo_list || name != "Show/Hide Tempo List",
        "Tempo Operations…" | "Open Tempo Operations…" => open_ops(app),
        "Adjust Tempo using Region Length and Locators" => {
            let Some((t, r)) = app.selected_region else { return true };
            let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return true };
            // the region's own length in seconds (audio: its file window; MIDI: at the tempo now)
            let secs = match &rg.audio {
                Some(a) => a.seconds.min(Map::of(app.project.as_ref().unwrap()).secs_at((rg.start + rg.length) as f64) - Map::of(app.project.as_ref().unwrap()).secs_at(rg.start as f64)).max(0.001),
                None => Map::of(app.project.as_ref().unwrap()).secs_at((rg.start + rg.length) as f64) - Map::of(app.project.as_ref().unwrap()).secs_at(rg.start as f64),
            };
            let (a, b) = app.cycle;
            if b > a {
                app.arr.fit = Some((a, b, secs));
            } else {
                app.tell("Set the locators to the length the region should have");
            }
        }
        "Create Time Signature Change" | "Create Key Signature Change" => {
            let Some(p) = &app.project else { return true };
            let m = Map::of(p);
            let (bar, _, s) = m.bar_of(app.position() as u32);
            let key = name.contains("Key");
            app.arr.sig = Some(SigDialog { bar, key, num: s.0, den: s.1, grouping: String::new(), key_name: timeline::key_at(p, bar), editing: None });
        }
        "Create Tempo Change" => {
            let at = app.position() as u32;
            let bpm = app.tempo();
            app.checkpoint(name);
            if let Some(p) = app.project.as_mut() {
                timeline::set_point(p, at, bpm);
            }
            app.after_edit();
        }
        "Beats from Region" => {
            let Some((t, r)) = app.selected_region else { return true };
            let Some(ticks) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|rg| {
                let mut v: Vec<u32> = rg.notes.iter().map(|n| n.pos).collect();
                v.sort();
                v.dedup_by(|a, b| a.abs_diff(*b) < 60);
                v
            }) else { return true };
            app.checkpoint("Beat Mapping");
            let n = app.project.as_mut().map_or(0, |p| timeline::beats_from_notes(p, &ticks, lg_core::events::PPQ));
            if n < ticks.len() {
                app.tell("Beat mapping stopped: the tempo needed would be too low or too high");
            }
            app.after_edit();
        }
        _ => return false,
    }
    true
}

/// Mute Notes: the selected notes are kept but not played (toggles).
pub fn mute_notes(app: &mut App) {
    let Some((t, r)) = app.selected_region else { return };
    let sel = app.selected_notes.clone();
    if sel.is_empty() {
        return;
    }
    app.checkpoint("Mute Notes");
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        let all_muted = sel.iter().all(|&i| rg.notes.get(i).is_some_and(|n| n.muted));
        for &i in &sel {
            if let Some(n) = rg.notes.get_mut(i) {
                n.muted = !all_muted;
            }
        }
    }
    app.load_engine();
}

fn open_ops(app: &mut App) {
    let (a, b) = app.cycle;
    let (a, b) = if b > a { (a, b) } else { (0, 4 * app.bar_ticks()) };
    let (sa, sb) = app.project.as_ref().map_or((120.0, 120.0), |p| (p.tempo_at(a), p.tempo_at(b)));
    app.arr.ops = Some(OpsDialog { op: 0, from: a, to: b, start: sa, end: sb, kind: 0, curvature: 0, density: 4, keep: true, percent: 0.0, new_end: b, random: 5.0, tenths: false });
}

// ---------------------------------------------------------------------------------------------
// the Tempo track

/// The Tempo track's value range: the points' range with room around it.
fn tempo_range(p: &lg_core::Project) -> (f64, f64) {
    let lo = p.tempo_map.iter().map(|x| x.1).fold(p.tempo, f64::min);
    let hi = p.tempo_map.iter().map(|x| x.1).fold(p.tempo, f64::max);
    (((lo - 20.0) / 10.0).floor().max(1.0) * 10.0, ((hi + 20.0) / 10.0).ceil() * 10.0)
}

/// The scale at the Tempo track's header: the range's ends and middle.
pub fn tempo_scale(app: &App, p: &egui::Painter, row: Rect, lanes_x: f32) {
    let Some(pr) = &app.project else { return };
    let (lo, hi) = tempo_range(pr);
    for bpm in [lo, (lo + hi) / 2.0, hi] {
        let y = row.max.y - 6.0 - ((bpm - lo) / (hi - lo)) as f32 * (row.height() - 12.0);
        p.text(pos2(lanes_x - 34.0, y), Align2::RIGHT_CENTER, format!("{bpm:.0}"), look::regular(9.5), look::text_dim());
        p.hline(lanes_x..=row.max.x, y, Stroke::new(1.0, look::grid_line()));
    }
}

/// The Tempo track: the tempo line with its points and curve points; click to add a point,
/// drag points (up / down: tempo, Shift finer; left / right: position; Option: a copy), drag a
/// line to change both its points, drag a curve point to bend the segment, double-click a
/// point or curve point to delete it; right-click for the tempo menu.
pub fn tempo_lane(app: &mut App, ui: &mut egui::Ui, row: Rect, lane: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(pr) = app.project.clone() else { return };
    let lp = ui.painter_at(lane);
    let (lo, hi) = tempo_range(&pr);
    let y_of = |bpm: f64| row.max.y - 6.0 - ((bpm - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 * (row.height() - 12.0);
    let bpm_of = |y: f32| lo + ((row.max.y - 6.0 - y) / (row.height() - 12.0)).clamp(0.0, 1.0) as f64 * (hi - lo);
    let m = Map::of(&pr);
    // Logic 13: the tempo the project follows is a blue line over a dark blue fill
    let green = Color32::from_rgb(0x4a, 0x82, 0xd4);
    // the line, sampled so curves show
    let mut pts = Vec::new();
    let mut x = lane.min.x;
    while x <= lane.max.x + 3.0 {
        let t = tick_of(x).max(0.0);
        pts.push(pos2(x, y_of(m.bpm_at(t as f64))));
        x += 3.0;
    }
    // (steps drawn square)
    let mut line = Vec::new();
    for w in pts.windows(2) {
        line.push(w[0]);
        if (w[1].y - w[0].y).abs() > 0.5 && !m.curves.iter().any(|c| c.1 != 0 && { let t = tick_of(w[0].x) as u32; m.points.iter().position(|p| p.0 == c.0).is_some_and(|k| t >= c.0 && m.points.get(k + 1).is_none_or(|n| t < n.0)) }) {
            line.push(pos2(w[1].x, w[0].y));
        }
    }
    if let Some(l) = pts.last() {
        line.push(*l);
    }
    for w in line.windows(2) {
        if w[1].x > w[0].x {
            lp.rect_filled(Rect::from_min_max(pos2(w[0].x, w[0].y.min(w[1].y)), pos2(w[1].x, row.max.y)), 0.0, Color32::from_rgb(0x1a, 0x2c, 0x48));
        }
    }
    lp.add(egui::Shape::line(line, Stroke::new(1.2, green)));
    // points and curve points
    let mut hits: Vec<(Rect, usize, bool)> = Vec::new();
    for (k, &(t, b)) in m.points.iter().enumerate() {
        let c = pos2(x_of(t as f32), y_of(b));
        let sel = app.arr.tempo_sel.contains(&t);
        if lane.expand(4.0).contains(c) {
            if sel {
                lp.circle_filled(c, 3.5, green);
            } else {
                lp.circle_stroke(c, 2.5, Stroke::new(1.2, green));
            }
            // each point's tempo beside it (Logic: "120", "127")
            let label = if (b - b.round()).abs() < 0.005 { format!("{b:.0}") } else { format!("{b:.2}") };
            lp.text(c + vec2(5.0, 2.0), Align2::LEFT_TOP, label, look::regular(10.5), green);
        }
        hits.push((Rect::from_center_size(c, vec2(10.0, 10.0)), k, false));
        if let Some(&(t1, b1)) = m.points.get(k + 1) {
            let mid_t = (t + t1) / 2;
            let cc = pos2(x_of(mid_t as f32), y_of(m.bpm_at(mid_t as f64)));
            if (b1 - b).abs() > 0.01 && (x_of(t1 as f32) - x_of(t as f32)) > 24.0 {
                lp.circle_stroke(cc, 3.0, Stroke::new(1.0, green.gamma_multiply(0.8)));
                hits.push((Rect::from_center_size(cc, vec2(9.0, 9.0)), k, true));
            }
        }
    }
    let resp = ui.interact(lane, ui.scope_id().with("tempo-lane"), Sense::click_and_drag()).on_hover_text("Tempo: click to add a point; drag points, lines and curve points");
    let px_per_tick = (x_of(1000.0) - x_of(0.0)) / 1000.0;
    let hit = |pp: egui::Pos2| hits.iter().find(|h| h.0.contains(pp)).copied();
    let snap_t = |app: &App, t: f32| -> u32 {
        if app.arr.tempo_snap > 0 {
            let g = app.arr.tempo_snap as f32;
            ((t / g).round() * g).max(0.0) as u32
        } else {
            snap_tick(app, t, px_per_tick) as u32
        }
    };
    // a click: select a point (or the line to its right), add one, delete with a double-click
    if resp.double_clicked() {
        if let Some((_, k, curve)) = resp.interact_pointer_pos().and_then(hit) {
            let t = m.points[k].0;
            app.checkpoint(if curve { "Delete Tempo Curve" } else { "Delete Tempo Point" });
            if let Some(p) = app.project.as_mut() {
                if curve { timeline::set_curve(p, t, 0) } else { timeline::delete_point(p, t); }
            }
            app.arr.tempo_sel.clear();
            app.after_edit();
        }
    } else if resp.clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            let shift = ui.input(|i| i.modifiers.shift);
            match hit(pp) {
                Some((_, k, false)) => {
                    let t = m.points[k].0;
                    if shift { app.arr.tempo_sel.push(t) } else { app.arr.tempo_sel = vec![t] }
                }
                _ => {
                    let t = snap_t(app, tick_of(pp.x));
                    // on the line: selects the point to its left; elsewhere a new point
                    let at_line = (y_of(m.bpm_at(t as f64)) - pp.y).abs() < 4.0;
                    let left = m.points.iter().rev().find(|p| p.0 <= t).map(|p| p.0);
                    if at_line && !ui.input(|i| i.modifiers.command) {
                        if let Some(l) = left {
                            app.arr.tempo_sel = vec![l];
                        }
                    } else {
                        app.checkpoint("Create Tempo Point");
                        let bpm = (bpm_of(pp.y) * 100.0).round() / 100.0;
                        if let Some(p) = app.project.as_mut() {
                            timeline::set_point(p, t, bpm);
                        }
                        app.arr.tempo_sel = vec![t];
                        app.after_edit();
                    }
                }
            }
        }
    }
    if resp.drag_started() {
        if let Some(pp) = ui.input(|i| i.pointer.press_origin()) {
            let copy = ui.input(|i| i.modifiers.alt);
            app.arr.tdrag = match hit(pp) {
                Some((_, k, true)) => Some(TDrag::Curve { at: m.points[k].0, y0: pp.y, c0: m.curves.iter().find(|c| c.0 == m.points[k].0).map_or(0, |c| c.1) }),
                Some((_, k, false)) => Some(TDrag::Point { from: m.points[k].0, y0: pp.y, bpm0: m.points[k].1, axis: None, copy, at: m.points[k].0 }),
                None => {
                    let t = tick_of(pp.x).max(0.0);
                    let k = m.points.iter().rposition(|p| p.0 as f32 <= t).unwrap_or(0);
                    ((y_of(m.bpm_at(t as f64)) - pp.y).abs() < 6.0).then(|| TDrag::Line { k, y0: pp.y, orig: (m.points[k].1, m.points.get(k + 1).map_or(m.points[k].1, |p| p.1)) })
                }
            };
            if let Some(d) = app.arr.tdrag {
                app.checkpoint(match d {
                    TDrag::Point { copy: true, .. } => "Copy Tempo Point",
                    TDrag::Point { .. } => "Move Tempo Point",
                    TDrag::Line { .. } => "Change Tempo",
                    TDrag::Curve { .. } => "Create Tempo Curve",
                });
                if let TDrag::Point { from, .. } = d {
                    app.arr.tempo_sel = vec![from];
                }
            }
        }
    }
    if let (true, Some(pp), Some(d)) = (resp.dragged(), resp.interact_pointer_pos(), app.arr.tdrag) {
        let total = ui.input(|i| i.pointer.press_origin()).map_or(vec2(0.0, 0.0), |o| pp - o);
        match d {
            TDrag::Point { from, y0, bpm0, axis, copy, at } => {
                // one axis per drag: the one moved first
                let axis = axis.or_else(|| (total.length() > 4.0).then_some(total.x.abs() > total.y.abs()));
                let mut new_at = at;
                let snapped = snap_t(app, tick_of(pp.x)).max(1);
                if let Some(p) = app.project.as_mut() {
                    match axis {
                        Some(true) if from > 0 => {
                            let t = snapped;
                            if t != at {
                                if copy && at == from {
                                    timeline::copy_point(p, from, t);
                                } else {
                                    timeline::move_point(p, at, t);
                                }
                                new_at = t;
                            }
                        }
                        Some(false) => {
                            let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                            let per_px = (hi - lo) / (row.height() - 12.0) as f64 * fine;
                            let bpm = (bpm0 + (y0 - pp.y) as f64 * per_px).clamp(5.0, 990.0);
                            let bpm = if fine < 1.0 { (bpm * 100.0).round() / 100.0 } else { bpm.round() };
                            timeline::set_point(p, at, bpm);
                        }
                        _ => {}
                    }
                }
                app.arr.tdrag = Some(TDrag::Point { from, y0, bpm0, axis, copy, at: new_at });
                app.arr.tempo_sel = vec![new_at];
                let bpm = app.project.as_ref().map_or(0.0, |p| Map::of(p).bpm_at(new_at as f64));
                app.help_tag = Some(vec![("Tempo:".into(), format!("{bpm:.2}")), ("Position:".into(), crate::app::bbdt(new_at, sig(app)))]);
            }
            TDrag::Line { k, y0, orig } => {
                let per_px = (hi - lo) / (row.height() - 12.0) as f64;
                let d = (y0 - pp.y) as f64 * per_px;
                if let Some(p) = app.project.as_mut() {
                    let pts = Map::of(p).points;
                    if let Some(&(t0, _)) = pts.get(k) {
                        timeline::set_point(p, t0, (orig.0 + d).round());
                    }
                    if let Some(&(t1, _)) = pts.get(k + 1) {
                        timeline::set_point(p, t1, (orig.1 + d).round());
                    }
                }
                app.help_tag = Some(vec![("Tempo:".into(), format!("{:+.0}", d))]);
            }
            TDrag::Curve { at, y0, c0 } => {
                let c = (c0 as f32 + (pp.y - y0) * 1.5).clamp(-99.0, 99.0) as i8;
                if let Some(p) = app.project.as_mut() {
                    timeline::set_curve(p, at, c);
                }
                app.help_tag = Some(vec![("Curve:".into(), format!("{c}"))]);
            }
        }
        sync_tempo(app);
    }
    if resp.drag_stopped() {
        app.arr.tdrag = None;
        app.after_edit();
    }
    // Delete removes the selected points (not the first)
    if !app.arr.tempo_sel.is_empty() && app.gc.focus == crate::globalclip::Focus::Tempo && crate::focus::tracks(app) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
        app.checkpoint("Delete Tempo Point");
        let sel = std::mem::take(&mut app.arr.tempo_sel);
        if let Some(p) = app.project.as_mut() {
            for t in sel {
                timeline::delete_point(p, t);
            }
        }
        app.after_edit();
    }
    resp.context_menu(|ui| tempo_menu(app, ui));
}

fn sync_tempo(app: &App) {
    let Some(p) = &app.project else { return };
    let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    e.tempo = p.tempo;
    e.tempo_map = p.tempo_map.clone();
    e.timeline = Map::of(p);
}

/// The tempo point menu (the Tempo track's shortcut menu).
fn tempo_menu(app: &mut App, ui: &mut egui::Ui) {
    ui.set_min_width(240.0);
    let sel = app.arr.tempo_sel.clone();
    let (from, to) = (sel.iter().copied().min().unwrap_or(0), sel.iter().copied().max().unwrap_or(0));
    if ui.add_enabled(sel.len() == 1 && sel[0] > 0, egui::Button::new("Extend Left")).clicked() {
        app.checkpoint("Extend Left");
        if let Some(p) = app.project.as_mut() {
            timeline::extend_left(p, sel[0]);
        }
        app.arr.tempo_sel.clear();
        app.after_edit();
        ui.close();
    }
    if ui.add_enabled(sel.len() >= 2, egui::Button::new("Replace Selected Points by Average Tempo")).clicked() {
        app.checkpoint("Replace by Average");
        if let Some(p) = app.project.as_mut() {
            timeline::replace_by_average(p, from, to);
        }
        app.after_edit();
        ui.close();
    }
    if ui.add_enabled(sel.len() >= 2, egui::Button::new("Apply Smoothing to Selected Points")).clicked() {
        app.checkpoint("Apply Smoothing");
        if let Some(p) = app.project.as_mut() {
            timeline::smooth(p, from, to + 1);
        }
        app.after_edit();
        ui.close();
    }
    if ui.add_enabled(sel.len() >= 2, egui::Button::new("Set Time For Last Selected Tempo Event…")).clicked() {
        crate::globalclip::command(app, "Tempo Track: Set Time For Last Selected Tempo");
        ui.close();
    }
    ui.separator();
    ui.menu_button("Snap", |ui| {
        for (n, v) in [("Off (Snap setting)", 0u32), ("Bar", app.bar_ticks()), ("Beat", 960), ("1/8", 480), ("1/16", 240)] {
            if ui.button(if app.arr.tempo_snap == v { format!("✓ {n}") } else { n.to_string() }).clicked() {
                app.arr.tempo_snap = v;
                ui.close();
            }
        }
    });
    ui.menu_button("Tempo Set", |ui| set_menu(app, ui));
    ui.separator();
    if ui.button("Show Tempo List").clicked() {
        app.arr.tempo_list = true;
        ui.close();
    }
    if ui.button("Tempo Operations…").clicked() {
        open_ops(app);
        ui.close();
    }
}

fn set_menu(app: &mut App, ui: &mut egui::Ui) {
    let Some(p) = app.project.as_ref() else { return };
    let names = timeline::set_names(p);
    let cur = p.global.tempo_set;
    for (k, n) in names.iter().enumerate() {
        if ui.button(if k == cur { format!("✓ {n}") } else { n.clone() }).clicked() {
            app.checkpoint("Choose Tempo Set");
            if let Some(p) = app.project.as_mut() {
                timeline::switch_set(p, k);
            }
            app.after_edit();
            ui.close();
        }
    }
    ui.separator();
    if ui.add_enabled(names.len() < 9, egui::Button::new("New Set")).clicked() {
        app.checkpoint("New Tempo Set");
        if let Some(p) = app.project.as_mut() {
            let n = format!("Tempo Set {}", names.len() + 1);
            timeline::new_set(p, &n);
        }
        app.after_edit();
        ui.close();
    }
    if ui.button("Rename Set…").clicked() {
        app.arr.set_rename = Some(names.get(cur).cloned().unwrap_or_default());
        app.arr.tempo_list = true;
        ui.close();
    }
    if ui.add_enabled(names.len() > 1, egui::Button::new("Delete Set")).clicked() {
        app.checkpoint("Delete Tempo Set");
        if let Some(p) = app.project.as_mut() {
            timeline::delete_set(p);
        }
        app.after_edit();
        ui.close();
    }
}

// ---------------------------------------------------------------------------------------------
// the Signature track

/// The Signature track: time signatures (and key changes) at their bars; double-click to
/// add or edit one (the dialog), drag one to another bar, right-click to delete it.
pub fn signature_lane(app: &mut App, ui: &mut egui::Ui, row: Rect, lane: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(pr) = app.project.clone() else { return };
    let lp = ui.painter_at(lane);
    let m = Map::of(&pr);
    let mut items: Vec<(u32, String, bool)> = vec![(0, format!("{}/{}", pr.signature.0, pr.signature.1), false)];
    items.extend(pr.global.signatures.iter().map(|s| (s.0, format!("{}/{}", s.1, s.2), false)));
    items.push((0, pr.key.clone(), true));
    items.extend(pr.global.keys.iter().map(|k| (k.0, k.1.clone(), true)));
    let top = row.min.y + 2.0;
    let half = (row.height() - 4.0) / 2.0;
    for (k, (bar, text, key)) in items.iter().enumerate() {
        let x = x_of(m.bar_start(*bar) as f32);
        let y = if *key { top + half } else { top };
        let r = Rect::from_min_size(pos2(x + 1.0, y), vec2((text.len() as f32 * 7.0 + 10.0).max(28.0), half - 1.0));
        lp.rect_filled(r, 3.0, if *key { look::group_bg() } else { look::accent().gamma_multiply(0.25) });
        lp.text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, text, look::regular(10.5), look::text());
        if *bar == 0 {
            continue;
        }
        let resp = ui.interact(r.intersect(lane), ui.scope_id().with(("sig", k)), Sense::click_and_drag());
        if crate::globalclip::sig_item(app, ui, &resp, *bar, *key, &lp, r) {
            continue;
        }
        if resp.double_clicked() {
            let s = m.sig_of_bar(*bar);
            app.arr.sig = Some(SigDialog { bar: *bar, key: *key, num: s.0, den: s.1, grouping: String::new(), key_name: text.clone(), editing: Some(*bar) });
        }
        if resp.drag_started() {
            app.checkpoint(if *key { "Move Key Signature" } else { "Move Time Signature" });
        }
        if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let to = m.bar_of(tick_of(pp.x).max(0.0) as u32).0.max(1);
            if to != *bar {
                if let Some(p) = app.project.as_mut() {
                    if *key {
                        if let Some(kc) = p.global.keys.iter_mut().find(|kc| kc.0 == *bar) {
                            kc.0 = to;
                        }
                        p.global.keys.sort_by_key(|kc| kc.0);
                    } else {
                        timeline::move_signature(p, *bar, to);
                    }
                }
                sync_tempo(app);
            }
            app.help_tag = Some(vec![("Bar:".into(), format!("{}", to + 1))]);
        }
        if resp.drag_stopped() {
            app.after_edit();
        }
        let (b, isk) = (*bar, *key);
        resp.context_menu(|ui| {
            if ui.button("Delete").clicked() {
                app.checkpoint("Delete Signature");
                if let Some(p) = app.project.as_mut() {
                    if isk { p.global.keys.retain(|kc| kc.0 != b); } else { timeline::delete_signature(p, b); }
                }
                app.after_edit();
                ui.close();
            }
        });
    }
    let resp = ui.interact(lane, ui.scope_id().with("sig-lane"), Sense::click());
    if resp.double_clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            let (bar, _, s) = m.bar_of(tick_of(pp.x).max(0.0) as u32);
            let key = pp.y > top + half;
            app.arr.sig = Some(SigDialog { bar: bar.max(1), key, num: s.0, den: s.1, grouping: String::new(), key_name: timeline::key_at(&pr, bar), editing: None });
        }
    }
    resp.on_hover_text("Signatures: double-click to add one at that bar (upper half: time, lower: key); drag to move; right-click to delete").context_menu(|ui| {
        if ui.button("Create Time Signature Change").clicked() {
            command(app, "Create Time Signature Change");
            ui.close();
        }
        if ui.button("Create Key Signature Change").clicked() {
            command(app, "Create Key Signature Change");
            ui.close();
        }
    });
}

/// Bar lines for the Tracks area when the signature changes: (tick, bar number).
pub fn bar_lines(app: &App, from: u32, to: u32) -> Option<Vec<(u32, u32)>> {
    let p = app.project.as_ref()?;
    if p.global.signatures.is_empty() {
        return None;
    }
    Some(Map::of(p).bars(from, to).into_iter().map(|b| (b.0, b.1)).collect())
}

// ---------------------------------------------------------------------------------------------
// windows: Tempo List, Tempo Operations, the signature dialog, Adjust Tempo

const KEYS: [&str; 24] = ["C major", "G major", "D major", "A major", "E major", "B major", "F# major", "Db major", "Ab major", "Eb major", "Bb major", "F major", "A minor", "E minor", "B minor", "F# minor", "C# minor", "G# minor", "Eb minor", "Bb minor", "F minor", "C minor", "G minor", "D minor"];

pub fn windows(app: &mut App, ctx: &egui::Context) {
    // LG_SIGNATURE="bar num den" / LG_TEMPO="tick bpm curve;…" (checks): once, into the project
    static ONCE: std::sync::Once = std::sync::Once::new();
    if app.project.is_some() {
        ONCE.call_once(|| {
            if let Some(p) = app.project.as_mut() {
                if let Ok(s) = std::env::var("LG_SIGNATURE") {
                    let n: Vec<u32> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if let [b, a, d] = n[..] {
                        timeline::set_signature(p, b.saturating_sub(1), (a as u8, d as u8));
                    }
                }
                if let Ok(s) = std::env::var("LG_TEMPO") {
                    for part in s.split(';') {
                        let n: Vec<f64> = part.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                        if n.len() >= 2 {
                            timeline::set_point(p, n[0] as u32, n[1]);
                            if n.len() > 2 {
                                timeline::set_curve(p, n[0] as u32, n[2] as i8);
                            }
                        }
                    }
                }
            }
            if std::env::var("LG_SIGNATURE").is_ok() || std::env::var("LG_TEMPO").is_ok() {
                app.after_edit();
            }
        });
    }
    tempo_list_window(app, ctx);
    ops_window(app, ctx);
    sig_window(app, ctx);
    if let Some((a, b, secs)) = app.arr.fit {
        let mut open = true;
        let mut pick: Option<bool> = None;
        egui::Window::new("Adjust Tempo").id(egui::Id::unique("fit-tempo")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("The region ({secs:.3} s) should last from the left to the right locator: change the tempo of the whole project, or only create a tempo change for the region's section?"));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    app.arr.fit = None;
                }
                if ui.button("Create").clicked() {
                    pick = Some(false);
                }
                if ui.button("Globally").clicked() {
                    pick = Some(true);
                }
            });
        });
        if let Some(g) = pick {
            app.checkpoint("Adjust Tempo using Region Length and Locators");
            if let Some(p) = app.project.as_mut() {
                timeline::fit_region(p, a, b, secs, g);
            }
            app.arr.fit = None;
            app.after_edit();
        }
        if !open {
            app.arr.fit = None;
        }
    }
}

/// The Tempo List (also the List Editors' Tempo tab).
pub fn tempo_list(app: &mut App, ui: &mut egui::Ui) {
    let Some(p) = app.project.clone() else { return };
    let m = Map::of(&p);
    ui.horizontal(|ui| {
        if ui.button("Add Tempo").on_hover_text("A tempo event at the playhead, at the current tempo").clicked() {
            command(app, "Create Tempo Change");
        }
        let names = timeline::set_names(&p);
        ui.menu_button(format!("{} ▾", names.get(p.global.tempo_set).cloned().unwrap_or_default()), |ui| set_menu(app, ui));
        let mut ce = app.arr.curve_events;
        if ui.checkbox(&mut ce, "Additional Info").on_hover_text("Shows the events tempo curves stand for").changed() {
            app.arr.curve_events = ce;
        }
        let g = if app.arr.granularity == 0 { 240 } else { app.arr.granularity };
        egui::ComboBox::from_id_salt("tempo-gran").selected_text(format!("Granularity 1/{}", 3840 / g)).show_ui(ui, |ui| {
            for d in [4u32, 8, 16, 32] {
                if ui.selectable_label(g == 3840 / d, format!("1/{d}")).clicked() {
                    app.arr.granularity = 3840 / d;
                }
            }
        });
    });
    if let Some(mut name) = app.arr.set_rename.clone() {
        ui.horizontal(|ui| {
            ui.label("Set name:");
            let te = ui.text_edit_singleline(&mut name);
            crate::widgets::focus_once(ui, &te);
            app.arr.set_rename = Some(name.clone());
            if ui.button("OK").clicked() || te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let n = name.clone();
                app.checkpoint("Rename Tempo Set");
                if let Some(p) = app.project.as_mut() {
                    let k = p.global.tempo_set;
                    timeline::rename_set(p, k, &n);
                }
                app.arr.set_rename = None;
            }
        });
    }
    ui.separator();
    let gran = if app.arr.granularity == 0 { 240 } else { app.arr.granularity };
    let rows = if app.arr.curve_events { m.expanded(gran) } else { m.points.iter().map(|p| (p.0, p.1, false)).collect() };
    let mut change: Option<(u32, Option<u32>, Option<f64>, bool)> = None;
    egui::ScrollArea::vertical().id_salt("tempo-list").show(ui, |ui| {
        egui::Grid::new("tempo-list-grid").striped(true).num_columns(4).show(ui, |ui| {
            ui.label(egui::RichText::new("Position").strong());
            ui.label(egui::RichText::new("Time").strong());
            ui.label(egui::RichText::new("Tempo").strong());
            ui.label("");
            ui.end_row();
            for (t, bpm, derived) in rows {
                let sel = app.arr.tempo_sel.contains(&t);
                if derived {
                    ui.label(egui::RichText::new(crate::app::bbdt(t, p.signature)).weak());
                    ui.label(egui::RichText::new(format!("{:.3} s", m.secs_at(t as f64))).weak());
                    ui.label(egui::RichText::new(format!("{bpm:.4}")).weak());
                    ui.label("");
                    ui.end_row();
                    continue;
                }
                // the position: drag (1/16 steps) or pickup to the playhead
                let mut pos = t as f64 / 240.0;
                let pr = ui.add_enabled(t > 0, egui::DragValue::new(&mut pos).speed(0.2).custom_formatter(|v, _| crate::app::bbdt((v * 240.0).round() as u32, p.signature)));
                if pr.changed() {
                    change = Some((t, Some((pos.max(0.0) * 240.0).round() as u32), None, pr.drag_started()));
                }
                if pr.clicked() {
                    app.arr.tempo_sel = vec![t];
                }
                ui.label(format!("{:.3} s", m.secs_at(t as f64)));
                let mut b = bpm;
                let br = ui.add(egui::DragValue::new(&mut b).speed(0.2).range(5.0..=990.0).max_decimals(4));
                if br.changed() {
                    change = Some((t, None, Some(b), br.drag_started()));
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(t > 0, egui::Button::new("⌫").small()).on_hover_text("Delete").clicked() {
                        app.checkpoint("Delete Tempo Point");
                        if let Some(pp) = app.project.as_mut() {
                            timeline::delete_point(pp, t);
                        }
                        app.after_edit();
                    }
                    if ui.add_enabled(t > 0, egui::Button::new("⇥").small()).on_hover_text("Move Region/Event to Playhead Position (Pickup Clock)").clicked() {
                        change = Some((t, Some(app.position() as u32), None, true));
                    }
                    if sel {
                        ui.label("●");
                    }
                });
                ui.end_row();
            }
        });
    });
    if let Some((t, to, bpm, _start)) = change {
        app.checkpoint("Change Tempo");
        if let Some(pp) = app.project.as_mut() {
            if let Some(b) = bpm {
                timeline::set_point(pp, t, b);
            }
            if let Some(to) = to {
                timeline::move_point(pp, t, to);
            }
        }
        app.after_edit();
    }
}

fn tempo_list_window(app: &mut App, ctx: &egui::Context) {
    if !app.arr.tempo_list {
        return;
    }
    let mut open = true;
    egui::Window::new("Tempo List").id(egui::Id::unique("tempo-list-window")).open(&mut open).default_size(vec2(460.0, 380.0)).show(ctx, |ui| tempo_list(app, ui));
    if !open {
        app.arr.tempo_list = false;
    }
}

const OPS: [&str; 7] = ["Create Tempo Curve", "Create Constant Tempo", "Scale Existing Tempo Changes", "Stretch Existing Tempo Changes", "Thin Out Existing Tempo Changes", "Randomize Tempo", "Round Existing Tempo Changes"];

fn op_of(d: &OpsDialog) -> Operation {
    match d.op {
        0 => Operation::Curve { start: d.start, end: d.end, kind: [CurveType::Linear, CurveType::Curved, CurveType::SCurve][d.kind.min(2)], curvature: d.curvature, density: d.density, keep: d.keep },
        1 => Operation::Constant { bpm: Some(d.start) },
        2 => Operation::Scale { percent: d.percent },
        3 => Operation::Stretch { new_end: d.new_end },
        4 => Operation::ThinOut { density: d.density },
        5 => Operation::Randomize { bpm: d.random, density: d.density, seed: (d.from as u64) << 8 | d.density as u64 },
        _ => Operation::Round { tenths: d.tenths },
    }
}

fn ops_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.arr.ops.clone() else { return };
    let Some(p) = app.project.clone() else { return };
    let mut open = true;
    let mut apply = false;
    let sig = p.signature;
    egui::Window::new("Tempo Operations").id(egui::Id::unique("tempo-ops")).open(&mut open).default_size(vec2(520.0, 420.0)).show(ctx, |ui| {
        egui::ComboBox::from_label("Operation").selected_text(OPS[d.op]).show_ui(ui, |ui| {
            for (k, n) in OPS.iter().enumerate() {
                ui.selectable_value(&mut d.op, k, *n);
            }
        });
        let bar = lg_core::project::bar_ticks(sig) as f64;
        ui.horizontal(|ui| {
            let mut a = d.from as f64 / bar + 1.0;
            let mut b = d.to as f64 / bar + 1.0;
            ui.label("Position");
            if ui.add(egui::DragValue::new(&mut a).speed(0.25).range(1.0..=9999.0).max_decimals(2)).changed() {
                d.from = ((a - 1.0) * bar).round() as u32;
            }
            ui.label("to");
            if ui.add(egui::DragValue::new(&mut b).speed(0.25).range(1.0..=9999.0).max_decimals(2)).changed() {
                d.to = ((b - 1.0) * bar).round() as u32;
            }
            let m = Map::of(&p);
            ui.label(format!("Time {:.3} – {:.3} s", m.secs_at(d.from as f64), m.secs_at(d.to as f64)));
        });
        match d.op {
            0 => {
                ui.horizontal(|ui| {
                    ui.label("Tempo");
                    ui.add(egui::DragValue::new(&mut d.start).range(5.0..=990.0).speed(0.5));
                    ui.label("to");
                    ui.add(egui::DragValue::new(&mut d.end).range(5.0..=990.0).speed(0.5));
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Curve Type").selected_text(["Linear", "Curved", "S-Curve"][d.kind.min(2)]).show_ui(ui, |ui| {
                        for (k, n) in ["Linear", "Curved", "S-Curve"].iter().enumerate() {
                            ui.selectable_value(&mut d.kind, k, *n);
                        }
                    });
                    ui.label("Curvature");
                    ui.add(egui::DragValue::new(&mut d.curvature).range(-99..=99));
                });
                ui.horizontal(|ui| {
                    ui.label("Density (events per bar)");
                    ui.add(egui::DragValue::new(&mut d.density).range(1..=32));
                });
                ui.checkbox(&mut d.keep, "Continue with new Tempo");
            }
            1 => {
                ui.horizontal(|ui| {
                    ui.label("Tempo");
                    ui.add(egui::DragValue::new(&mut d.start).range(5.0..=990.0).speed(0.5));
                    if ui.button("Average").clicked() {
                        let m = Map::of(&p);
                        let secs = m.secs_at(d.to as f64) - m.secs_at(d.from as f64);
                        d.start = (d.to.saturating_sub(d.from)) as f64 / 960.0 / secs.max(1e-9) * 60.0;
                    }
                });
            }
            2 => {
                ui.add(egui::Slider::new(&mut d.percent, -90.0..=200.0).text("Scale %"));
            }
            3 => {
                let mut b = d.new_end as f64 / bar + 1.0;
                ui.horizontal(|ui| {
                    ui.label("New end (bar)");
                    if ui.add(egui::DragValue::new(&mut b).speed(0.25).range(1.0..=9999.0)).changed() {
                        d.new_end = ((b - 1.0) * bar).round() as u32;
                    }
                    let pct = (d.new_end.saturating_sub(d.from)) as f64 / (d.to.saturating_sub(d.from)).max(1) as f64 * 100.0;
                    ui.label(format!("Stretch {pct:.1} %"));
                });
            }
            4 => {
                ui.horizontal(|ui| {
                    ui.label("Density (events per bar)");
                    ui.add(egui::DragValue::new(&mut d.density).range(1..=32));
                });
            }
            5 => {
                ui.add(egui::Slider::new(&mut d.random, 0.0..=40.0).text("Randomize (bpm)"));
                ui.horizontal(|ui| {
                    ui.label("Density (events per bar)");
                    ui.add(egui::DragValue::new(&mut d.density).range(1..=32));
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.tenths, false, "Whole bpm");
                    ui.radio_value(&mut d.tenths, true, "1/10 bpm");
                });
            }
        }
        // the result in red over the tempo now
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
        let pa = ui.painter_at(r);
        pa.rect_filled(r, 4.0, look::lcd_bg());
        let mut q = p.clone();
        timeline::operate(&mut q, d.from, d.to, op_of(&d));
        let (m0, m1) = (Map::of(&p), Map::of(&q));
        let span = (d.to.saturating_sub(d.from)).max(1) as f32;
        let (a, b) = (d.from as f32 - span * 0.25, d.to as f32 + span * 0.25);
        let lo = (0..=100).map(|i| a + (b - a) * i as f32 / 100.0).flat_map(|t| [m0.bpm_at(t.max(0.0) as f64), m1.bpm_at(t.max(0.0) as f64)]).fold(f64::MAX, f64::min) - 5.0;
        let hi = (0..=100).map(|i| a + (b - a) * i as f32 / 100.0).flat_map(|t| [m0.bpm_at(t.max(0.0) as f64), m1.bpm_at(t.max(0.0) as f64)]).fold(f64::MIN, f64::max) + 5.0;
        for (m, c) in [(&m0, look::lcd_dim()), (&m1, Color32::from_rgb(0xe8, 0x4c, 0x3c))] {
            let pts: Vec<egui::Pos2> = (0..=200).map(|i| {
                let t = a + (b - a) * i as f32 / 200.0;
                pos2(r.min.x + r.width() * i as f32 / 200.0, r.max.y - 6.0 - ((m.bpm_at(t.max(0.0) as f64) - lo) / (hi - lo)) as f32 * (r.height() - 12.0))
            }).collect();
            pa.add(egui::Shape::line(pts, Stroke::new(1.5, c)));
        }
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() {
                apply = true;
            }
        });
    });
    if apply {
        app.checkpoint(OPS[d.op]);
        if let Some(pp) = app.project.as_mut() {
            timeline::operate(pp, d.from, d.to, op_of(&d));
        }
        app.after_edit();
    }
    app.arr.ops = open.then_some(d);
}

fn sig_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.arr.sig.clone() else { return };
    let mut open = true;
    let mut ok = false;
    egui::Window::new(if d.key { "Key Signature" } else { "Time Signature" }).id(egui::Id::unique("sig-dialog")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Bar");
            let mut b = d.bar + 1;
            if ui.add(egui::DragValue::new(&mut b).range(1..=9999)).changed() {
                d.bar = b - 1;
            }
        });
        if d.key {
            egui::ComboBox::from_label("Key").selected_text(&d.key_name).show_ui(ui, |ui| {
                for k in KEYS {
                    ui.selectable_value(&mut d.key_name, k.to_string(), k);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut d.num).range(1..=99));
                ui.label("/");
                egui::ComboBox::from_id_salt("sig-den").selected_text(format!("{}", d.den)).show_ui(ui, |ui| {
                    for v in [1u8, 2, 4, 8, 16, 32] {
                        ui.selectable_value(&mut d.den, v, format!("{v}"));
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Beat Grouping");
                ui.add(egui::TextEdit::singleline(&mut d.grouping).desired_width(80.0).hint_text("e.g. 223"));
            });
            // a grouping sets the numerator (2+2+3 = 7)
            let sum: u32 = d.grouping.chars().filter_map(|c| c.to_digit(10)).sum();
            if sum > 0 {
                d.num = sum.min(99) as u8;
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                app.arr.sig = None;
            }
            if ui.button("OK").clicked() {
                ok = true;
            }
        });
    });
    if ok {
        app.checkpoint(if d.key { "Key Signature" } else { "Time Signature" });
        if let Some(p) = app.project.as_mut() {
            if let Some(old) = d.editing.filter(|&b| b != d.bar) {
                if d.key { p.global.keys.retain(|k| k.0 != old); } else { timeline::delete_signature(p, old); }
            }
            if d.key { timeline::set_key(p, d.bar, &d.key_name) } else { timeline::set_signature(p, d.bar, (d.num, d.den)) }
        }
        app.arr.sig = None;
        app.after_edit();
        return;
    }
    if app.arr.sig.is_some() {
        app.arr.sig = open.then_some(d);
    }
}

/// The Signature List (the List Editors' Signature tab): time and key signatures.
pub fn signature_list(app: &mut App, ui: &mut egui::Ui) {
    let Some(p) = app.project.clone() else { return };
    let m = Map::of(&p);
    ui.horizontal(|ui| {
        if ui.button("Add Time Signature").clicked() {
            command(app, "Create Time Signature Change");
        }
        if ui.button("Add Key Signature").clicked() {
            command(app, "Create Key Signature Change");
        }
        crate::globals::sig_set_popup(app, ui);
    });
    let Some(p) = app.project.clone() else { return };
    let mut rows: Vec<(u32, String, bool)> = vec![(0, format!("Time Signature {}/{}", p.signature.0, p.signature.1), false), (0, format!("Key Signature {}", p.key), true)];
    rows.extend(p.global.signatures.iter().map(|s| (s.0, format!("Time Signature {}/{}", s.1, s.2), false)));
    rows.extend(p.global.keys.iter().map(|k| (k.0, format!("Key Signature {}", k.1), true)));
    rows.sort_by_key(|r| (r.0, r.2));
    egui::Grid::new("sig-list").striped(true).num_columns(3).show(ui, |ui| {
        for (bar, text, key) in rows {
            ui.label(crate::app::bbdt(m.bar_start(bar), m.sig_of_bar(bar)));
            if ui.add(egui::Label::new(&text).sense(Sense::click())).double_clicked() {
                let s = m.sig_of_bar(bar);
                app.arr.sig = Some(SigDialog { bar, key, num: s.0, den: s.1, grouping: String::new(), key_name: timeline::key_at(&p, bar), editing: Some(bar) });
            }
            if bar > 0 && ui.small_button("⌫").on_hover_text("Delete").clicked() {
                app.checkpoint("Delete Signature");
                if let Some(pp) = app.project.as_mut() {
                    if key { pp.global.keys.retain(|k| k.0 != bar); } else { timeline::delete_signature(pp, bar); }
                }
                app.after_edit();
            } else if bar == 0 {
                ui.label("");
            }
            ui.end_row();
        }
    });
}

// ---------------------------------------------------------------------------------------------
// the Piano Roll's MIDI Draw lanes

const LANES: [(&str, u8, u8); 11] = [
    ("Modulation", 0xB0, 1),
    ("Breath", 0xB0, 2),
    ("Volume", 0xB0, 7),
    ("Pan", 0xB0, 10),
    ("Expression", 0xB0, 11),
    ("Sustain", 0xB0, 64),
    ("Cutoff", 0xB0, 74),
    ("Pitch Bend", 0xE0, 0),
    ("Aftertouch", 0xD0, 0),
    ("Poly Aftertouch", 0xA0, 60),
    ("Program Change", 0xC0, 0),
];

pub fn lane_name(l: Option<(u8, u8)>) -> String {
    match l {
        None => "Note Velocity".into(),
        Some((k, n)) => LANES.iter().find(|x| x.1 == k && (k != 0xB0 || x.2 == n)).map_or(format!("CC {n}"), |x| x.0.to_string()),
    }
}

/// The lane chooser (MIDI Draw pop-up).
pub fn lane_menu(app: &mut App, ui: &mut egui::Ui) {
    if ui.button("Note Velocity").clicked() {
        app.arr.lane = None;
        ui.close();
    }
    ui.separator();
    for (n, k, c) in LANES {
        if ui.button(n).clicked() {
            app.arr.lane = Some((k, c));
            ui.close();
        }
    }
    ui.menu_button("Other Controller", |ui| {
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for c in 0..128u8 {
                let l = lg_dsp::midifx::cc_label(c);
                if ui.button(if l.is_empty() { format!("{c}") } else { format!("{c} {l}") }).clicked() {
                    app.arr.lane = Some((0xB0, c));
                    ui.close();
                }
            }
        });
    });
}

/// A controller / pitch bend / pressure / program lane for the region: its events as a line
/// of stems; drag across to draw events (on the Division grid), Option-drag to erase them.
#[allow(clippy::too_many_arguments)]
pub fn event_lane(app: &mut App, ui: &mut egui::Ui, lane: Rect, ti: usize, ri: usize, x_of: &dyn Fn(f32) -> f32, tick_at: &dyn Fn(f32) -> f32, left: f32) {
    let Some((kind, num)) = app.arr.lane else { return };
    let p = ui.painter_at(Rect::from_min_max(pos2(left, lane.min.y), lane.max));
    p.rect_filled(Rect::from_min_max(pos2(left, lane.min.y), lane.max), 0.0, look::pane_bg());
    p.hline(left..=lane.max.x, lane.min.y, Stroke::new(1.0, look::line()));
    let title = Rect::from_min_size(pos2(left + 4.0, lane.min.y + 2.0), vec2(90.0, 16.0));
    p.text(pos2(left + 6.0, lane.min.y + 10.0), Align2::LEFT_CENTER, format!("{} ▾", lane_name(app.arr.lane)), look::semibold(10.5), look::text());
    let tr = ui.interact(title, ui.scope_id().with("lane-title"), Sense::click());
    egui::Popup::menu(&tr).show(|ui| lane_menu(app, ui));
    let inner = lane.shrink2(vec2(0.0, 6.0));
    let y_of = |v: f32| inner.max.y - inner.height() * v / 127.0;
    for v in [32.0f32, 64.0, 96.0] {
        p.hline(lane.x_range(), y_of(v), Stroke::new(1.0, look::grid_line()));
    }
    let Some(region) = app.project.as_ref().and_then(|pr| pr.tracks.get(ti)?.regions.get(ri)).cloned() else { return };
    let evs: Vec<&lg_core::events::Control> = region.ext.events.iter().filter(|c| regions::on_lane(c, kind, num)).collect();
    let c = Color32::from_rgb(0x5a, 0xc8, 0xfa);
    let mut last: Option<egui::Pos2> = None;
    for e in &evs {
        let x = x_of((region.start + e.pos) as f32);
        let y = y_of(regions::lane_value(e));
        if let Some(l) = last {
            // a controller holds its value to the next event
            p.line_segment([l, pos2(x, l.y)], Stroke::new(1.0, c.gamma_multiply(0.6)));
        }
        p.vline(x, y..=inner.max.y, Stroke::new(1.5, c));
        p.circle_filled(pos2(x, y), 3.0, c);
        last = Some(pos2(x, y));
    }
    if let Some(l) = last {
        p.line_segment([l, pos2(lane.max.x, l.y)], Stroke::new(1.0, c.gamma_multiply(0.6)));
    }
    let resp = ui.interact(lane, ui.scope_id().with(("event-lane", kind, num)), Sense::click_and_drag());
    if resp.drag_started() || resp.clicked() {
        app.checkpoint(if ui.input(|i| i.modifiers.alt) { "Delete Events" } else { "Draw Events" });
    }
    if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        let erase = ui.input(|i| i.modifiers.alt);
        let v = ((inner.max.y - pp.y) / inner.height()).clamp(0.0, 1.0) * 127.0;
        let prev = pp.x - ui.input(|i| i.pointer.delta().x);
        let (a, b) = (tick_at(pp.x.min(prev)), tick_at(pp.x.max(prev)));
        let grid = 60.0; // 1/64 notes
        if let Some(rg) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(ti)?.regions.get_mut(ri)) {
            let (s, len) = (rg.start as f32, rg.length as f32);
            let ra = ((a - s) / grid).floor() * grid;
            let rb = ((b - s) / grid).ceil() * grid;
            rg.ext.events.retain(|e| !(regions::on_lane(e, kind, num) && (e.pos as f32) >= ra && (e.pos as f32) <= rb.max(ra)));
            if !erase {
                let at = ((tick_at(pp.x) - s) / grid).round() * grid;
                if at >= 0.0 && at < len {
                    rg.ext.events.push(regions::lane_event(kind, num, 0, at as u32, v));
                    rg.ext.events.sort_by_key(|e| e.pos);
                }
            }
        }
        let shown = if kind & 0xF0 == 0xE0 { format!("{}", (v * 128.0).round() as i32 - 8192) } else { format!("{}", v.round()) };
        app.help_tag = Some(vec![(format!("{}:", lane_name(app.arr.lane)), if erase { "erase".into() } else { shown })]);
    }
    if resp.drag_stopped() || resp.clicked() {
        app.load_engine();
    }
}

// ---------------------------------------------------------------------------------------------
// what the Logic file can't hold yet: kept by Studio next to its hosted plug-ins' settings

fn extras_path(app: &App) -> Option<std::path::PathBuf> {
    let side = crate::hosting::sidecar_path(&app.file.as_ref()?.0)?;
    Some(side.parent()?.parent()?.join("project-extras").join(side.file_name()?))
}

fn extras_text(p: &lg_core::Project) -> String {
    let hex = |f: &[f32]| -> String { f.iter().map(|x| format!("{:08x}", x.to_bits())).collect() };
    let mut out = String::from("# Bramble Studio: MIDI FX, tempo curves and sets, signature changes\n");
    for (i, t) in p.tracks.iter().enumerate() {
        for s in &t.extra.midi_fx {
            out += &format!("midifx\t{i}\t{}\t{}\t{}\n", s.bypass as u8, s.name, hex(&s.values));
        }
    }
    for c in &p.global.tempo_curves {
        out += &format!("curve\t{}\t{}\n", c.0, c.1);
    }
    for s in &p.global.signatures {
        out += &format!("sig\t{}\t{}\t{}\n", s.0, s.1, s.2);
    }
    for k in &p.global.keys {
        out += &format!("key\t{}\t{}\n", k.0, k.1);
    }
    for (k, s) in p.global.tempo_sets.iter().enumerate() {
        let pts: Vec<String> = s.map.iter().map(|x| format!("{}:{}", x.0, x.1)).collect();
        let cs: Vec<String> = s.curves.iter().map(|x| format!("{}:{}", x.0, x.1)).collect();
        out += &format!("tset\t{k}\t{}\t{}\t{}\n", s.name, pts.join(","), cs.join(","));
    }
    if !p.global.tempo_sets.is_empty() {
        out += &format!("tcur\t{}\n", p.global.tempo_set);
    }
    out += &crate::markers::extras_text(p);
    out += &crate::globals::extras_text(p);
    out
}

fn apply_extras(p: &mut lg_core::Project, text: &str) -> bool {
    let unhex = |s: &str| -> Vec<f32> { (0..s.len() / 8).filter_map(|i| u32::from_str_radix(&s[i * 8..i * 8 + 8], 16).ok()).map(f32::from_bits).collect() };
    let mut any = false;
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = l.split('\t').collect();
        match w.as_slice() {
            ["midifx", i, b, name, vals] => {
                if let Some(t) = i.parse::<usize>().ok().and_then(|i| p.tracks.get_mut(i)) {
                    t.extra.midi_fx.push(lg_core::regions::MidiFxSlot { name: name.to_string(), values: unhex(vals), bypass: *b == "1" });
                    any = true;
                }
            }
            ["curve", t, c] => {
                if let (Ok(t), Ok(c)) = (t.parse(), c.parse()) {
                    p.global.tempo_curves.push((t, c));
                    any = true;
                }
            }
            ["sig", b, n, d] => {
                if let (Ok(b), Ok(n), Ok(d)) = (b.parse(), n.parse(), d.parse()) {
                    p.global.signatures.push((b, n, d));
                    any = true;
                }
            }
            ["key", b, k] => {
                if let Ok(b) = b.parse() {
                    p.global.keys.push((b, k.to_string()));
                    any = true;
                }
            }
            ["tset", _, name, pts, cs] => {
                let pair = |s: &str| s.split(',').filter_map(|x| x.split_once(':')).filter_map(|(a, b)| Some((a.parse::<u32>().ok()?, b.to_string()))).collect::<Vec<_>>();
                p.global.tempo_sets.push(timeline::TempoSet {
                    name: name.to_string(),
                    map: pair(pts).into_iter().filter_map(|(a, b)| Some((a, b.parse().ok()?))).collect(),
                    curves: pair(cs).into_iter().filter_map(|(a, b)| Some((a, b.parse().ok()?))).collect(),
                });
                any = true;
            }
            ["tcur", k] => p.global.tempo_set = k.parse().unwrap_or(0),
            _ => any |= crate::markers::apply_extras(p, l) || crate::globals::apply_extras(p, l),
        }
    }
    any
}

/// On Save: the extras next to Studio's settings (removed when there are none).
pub fn save_sidecar(app: &App) {
    let (Some(path), Some(p)) = (extras_path(app), app.project.as_ref()) else { return };
    let text = extras_text(p);
    if text.lines().all(|l| l.starts_with('#')) {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, text);
}

/// After a project opens: its extras back.
pub fn after_open(app: &mut App) {
    let Some(path) = extras_path(app) else { return };
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let applied = app.project.as_mut().is_some_and(|p| apply_extras(p, &text));
    if applied {
        app.markers = app.project.as_ref().map(|p| p.markers.clone()).unwrap_or_default();
        app.load_engine();
        app.dirty = false;
    }
}

/// What Save reports left out: these are kept by Studio.
pub fn saved_elsewhere(left: &mut Vec<String>) {
    left.retain(|l| !(l.starts_with("time / key signature changes") || l.starts_with("tempo sets") || l.starts_with("MIDI FX")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_round_trip() {
        let mut p = crate::demo::empty();
        p.tracks[0].extra.midi_fx.push(lg_core::regions::MidiFxSlot { name: "Chord Trigger".into(), values: vec![1.0, 48.5, -1.0], bypass: true });
        p.global.tempo_curves.push((0, 40));
        p.global.signatures.push((4, 7, 8));
        p.global.keys.push((8, "A minor".into()));
        lg_core::timeline::new_set(&mut p, "Fast");
        let text = extras_text(&p);
        let mut q = crate::demo::empty();
        assert!(apply_extras(&mut q, &text));
        assert_eq!(q.tracks[0].extra.midi_fx, p.tracks[0].extra.midi_fx);
        assert_eq!(q.global, p.global);
    }
}

/// The Event List's other events (Control, Pitch Bend, Aftertouch, Poly Aftertouch, Program):
/// a list under the notes with position, status, channel, number and value (drag to edit),
/// delete, and Create for each kind at the playhead. Returns the rect left for the notes.
pub fn event_list_extras(app: &mut App, ui: &mut egui::Ui, body: Rect, t: usize, r: usize) -> Rect {
    let Some(region) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return body };
    if region.audio.is_some() {
        return body;
    }
    let h = if region.ext.events.is_empty() { 34.0 } else { (body.height() * 0.4).clamp(80.0, 260.0) };
    let area = Rect::from_min_max(pos2(body.min.x, body.max.y - h), body.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area.shrink2(vec2(6.0, 2.0))));
    child.painter().hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let mut change: Option<(usize, Option<lg_core::events::Control>)> = None;
    let mut create: Option<(u8, u8)> = None;
    child.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Other events: {}", region.ext.events.len())).strong());
        ui.menu_button("Create ▾", |ui| {
            for (n, k, c) in [("Control (Modulation)", 0xB0u8, 1u8), ("Control (Volume)", 0xB0, 7), ("Control (Sustain)", 0xB0, 64), ("Pitch Bend", 0xE0, 0), ("Aftertouch", 0xD0, 0), ("Poly Aftertouch", 0xA0, 60), ("Program", 0xC0, 0)] {
                if ui.button(n).clicked() {
                    create = Some((k, c));
                    ui.close();
                }
            }
        });
    });
    egui::ScrollArea::vertical().id_salt(("ev-extras", t, r)).auto_shrink([false, false]).show(&mut child, |ui| {
        egui::Grid::new("ev-extras-grid").striped(true).num_columns(6).show(ui, |ui| {
            for (i, e) in region.ext.events.iter().enumerate() {
                ui.label(crate::app::bbdt(region.start + e.pos, sig));
                ui.label(regions::event_kind_name(e.kind));
                ui.label(format!("{}", e.channel + 1));
                let mut ne = *e;
                match e.kind & 0xF0 {
                    0xE0 => {
                        ui.label("");
                        let mut v = regions::bend_value(e) as i32;
                        if ui.add(egui::DragValue::new(&mut v).range(-8192..=8191).speed(16.0)).changed() {
                            ne = regions::bend_event(e.pos, e.channel, v as i16);
                        }
                    }
                    0xC0 | 0xD0 => {
                        ui.label("");
                        let mut v = e.number;
                        if ui.add(egui::DragValue::new(&mut v).range(0..=127)).changed() {
                            ne.number = v;
                        }
                    }
                    _ => {
                        let mut n = e.number;
                        if ui.add(egui::DragValue::new(&mut n).range(0..=127)).changed() {
                            ne.number = n;
                        }
                        let mut v = e.value;
                        if ui.add(egui::DragValue::new(&mut v).range(0..=127)).changed() {
                            ne.value = v;
                        }
                    }
                }
                if ne != *e {
                    change = Some((i, Some(ne)));
                }
                if ui.small_button("⌫").on_hover_text("Delete").clicked() {
                    change = Some((i, None));
                }
                ui.end_row();
            }
        });
    });
    if let Some((i, e)) = change {
        app.checkpoint(if e.is_some() { "Change Event" } else { "Delete Event" });
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            match e {
                Some(e) => rg.ext.events[i] = e,
                None => {
                    rg.ext.events.remove(i);
                }
            }
        }
        app.load_engine();
    }
    if let Some((k, c)) = create {
        let at = (app.position() as u32).saturating_sub(region.start).min(region.length.saturating_sub(1));
        app.checkpoint("Create Event");
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            rg.ext.events.push(regions::lane_event(k, c, 0, at, if k == 0xE0 { 64.0 } else { 64.0 }));
            rg.ext.events.sort_by_key(|e| e.pos);
        }
        app.load_engine();
    }
    Rect::from_min_max(body.min, pos2(body.max.x, area.min.y))
}
