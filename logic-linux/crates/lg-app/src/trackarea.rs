//! The Tracks area's track list as Logic's: rows of their own heights (Zoom Vertical, Individual
//! Track Zoom, Zoom Focused Track), track stacks (Create Track Stack…, Folder and Summing, open
//! and closed, Flatten), packed folders (Pack Folder, Go Into Folder, Unpack), hidden tracks
//! (Hide Selected Track, Toggle Hide View), track alternatives (the header's pop-up), track
//! reorder by dragging a header, multi-track selection (Shift: range, Command: toggle) and the
//! Transposition global track. The model's part is lg_core::arrange.
//!
//! App state kept by track index (Track inspector values, automation modes, sends, EQs…)
//! follows the tracks through any reorder, delete or undo: `sync` maps the indexes by the
//! tracks' ids after each edit.

use std::collections::{HashMap, HashSet};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::arrange::{self, StackKind};
use lg_core::TrackKind;

use crate::app::App;
use crate::look;

/// Logic's track heights: the area's zoom and each track's own (Individual Track Zoom).
pub const ZOOM_STEPS: [f32; 8] = [0.85, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0];

#[derive(Default)]
pub struct State {
    /// The tracks' ids as last seen (index -> id), for `sync`.
    ids: Vec<u64>,
    next_id: u64,
    /// The selected tracks (Shift-click a range, Command-click one more); `App::selected_track`
    /// is the focused one.
    pub selected: Vec<usize>,
    /// Hide View off: hidden tracks show (dimmed).
    pub show_hidden: bool,
    /// The packed folder entered (Go Into Folder): its track id.
    pub inside: Option<u64>,
    /// The area's vertical zoom (0: 1).
    pub v_zoom: f32,
    /// Zoom Focused Track: the selected track drawn taller.
    pub zoom_focused: bool,
    pub show_transposition: bool,
    /// Track header components: Track Alternatives (the pop-up on every track).
    pub show_alternatives: bool,
    /// The Create Track Stack… sheet, with its choice.
    pub stack_dialog: Option<StackKind>,
    /// A header being dragged to a new place: the track, where the press was (y).
    pub reorder: Option<(usize, f32)>,
    /// The alternative being renamed: (track, text).
    pub renaming_alt: Option<(usize, String)>,
    /// The automation lane each track shows when it is a plug-in parameter's: (slot, id).
    pub shown_plugin: HashMap<usize, (u32, u32)>,
    /// Region and automation edits (regionops.rs, automation.rs).
    pub regions: crate::regionops::State,
    pub auto: crate::automation::State,
}

/// A track's row: which track, where it starts (from the first row's top, unscrolled), how
/// tall it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub t: usize,
    pub y: f32,
    pub h: f32,
}

/// Forget the last project's state (a project was opened).
pub fn reset(app: &mut App) {
    let keep = (app.ta.v_zoom, app.ta.show_alternatives);
    app.ta = State { v_zoom: keep.0, show_alternatives: keep.1, ..Default::default() };
}

/// The packed folder entered, as a track index.
pub fn inside(app: &App) -> Option<usize> {
    let id = app.ta.inside?;
    app.project.as_ref()?.tracks.iter().position(|t| t.extra.id == id)
}

pub fn row_h(app: &App, t: usize) -> f32 {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return crate::tracks::ROW_H };
    let z = if app.ta.v_zoom > 0.0 { app.ta.v_zoom } else { 1.0 };
    let mut h = crate::tracks::ROW_H * z * tr.zoom();
    if app.ta.zoom_focused && app.selected_track == Some(t) {
        h = h.max(crate::tracks::ROW_H * 2.5);
    }
    h.clamp(44.0, 420.0) + crate::autoext::sub_h(app, t)
}

/// The rows the Tracks area shows, top to bottom.
pub fn rows(app: &App) -> Vec<Row> {
    let Some(p) = &app.project else { return Vec::new() };
    let mut y = 0.0;
    arrange::visible(&p.tracks, !app.ta.show_hidden, inside(app))
        .into_iter()
        .map(|t| {
            let h = row_h(app, t);
            let r = Row { t, y, h };
            y += h;
            r
        })
        .collect()
}

pub fn total_h(rows: &[Row]) -> f32 {
    rows.last().map_or(0.0, |r| r.y + r.h)
}

/// The row at `y` (from the first row's top, unscrolled).
pub fn row_at(rows: &[Row], y: f32) -> Option<Row> {
    rows.iter().copied().find(|r| y >= r.y && y < r.y + r.h)
}

/// Where track `t`'s row is on screen, with the rows starting at `rows_top` (scrolled), and
/// its height (None: not shown). For views lined up with the tracks (the Live Loops grid).
pub fn row_span(app: &App, t: usize, rows_top: f32) -> Option<(f32, f32)> {
    rows(app).into_iter().find(|r| r.t == t).map(|r| (rows_top + r.y - app.scroll_y, r.h))
}

fn remap_map<V>(m: &mut HashMap<usize, V>, map: &[Option<usize>]) {
    let old = std::mem::take(m);
    for (k, v) in old {
        if let Some(Some(n)) = map.get(k) {
            m.insert(*n, v);
        }
    }
}

fn remap_pair<V>(m: &mut HashMap<(usize, usize), V>, map: &[Option<usize>]) {
    let old = std::mem::take(m);
    for ((t, r), v) in old {
        if let Some(Some(n)) = map.get(t) {
            m.insert((*n, r), v);
        }
    }
}

fn remap_set(s: &mut HashSet<usize>, map: &[Option<usize>]) {
    *s = s.iter().filter_map(|k| map.get(*k).copied().flatten()).collect();
}

fn remap_one(v: &mut Option<usize>, map: &[Option<usize>]) {
    *v = v.and_then(|k| map.get(k).copied().flatten());
}

/// After an edit (and each frame): tracks get ids, and what the app keeps by track index
/// follows the tracks to where they are now.
pub fn sync(app: &mut App) {
    let Some(p) = app.project.as_mut() else { return };
    let cur: Vec<u64> = p.tracks.iter().map(|t| t.extra.id).collect();
    if cur == app.ta.ids && !cur.contains(&0) {
        return;
    }
    let fresh = app.ta.ids.is_empty() || cur.iter().all(|&i| i == 0);
    let map = arrange::remap(&app.ta.ids, &p.tracks);
    let mut next = app.ta.next_id;
    arrange::assign_ids(&mut p.tracks, &mut next);
    app.ta.next_id = next;
    app.ta.ids = p.tracks.iter().map(|t| t.extra.id).collect();
    if fresh || map.iter().enumerate().all(|(k, m)| *m == Some(k)) {
        return;
    }
    remap_map(&mut app.track_props, &map);
    remap_map(&mut app.instrument_choice, &map);
    remap_map(&mut app.track_notes, &map);
    remap_map(&mut app.auto_mode, &map);
    remap_map(&mut app.auto_param, &map);
    remap_map(&mut app.sends, &map);
    remap_map(&mut app.eqs, &map);
    remap_map(&mut app.plugin_compare, &map);
    remap_map(&mut app.ta.shown_plugin, &map);
    crate::autoext::remap(app, &map);
    remap_pair(&mut app.players, &map);
    remap_pair(&mut app.drum_params, &map);
    remap_pair(&mut app.spx.region_chords, &map);
    remap_pair(&mut app.spx.patterns, &map);
    remap_pair(&mut app.spx.hands, &map);
    remap_pair(&mut app.audio_edit.regions, &map);
    remap_map(&mut app.flex.tracks, &map);
    remap_set(&mut app.armed, &map);
    remap_set(&mut app.monitoring, &map);
    remap_set(&mut app.plugin_bypass, &map);
    remap_one(&mut app.eq_window, &map);
    remap_one(&mut app.plugin_window, &map);
    remap_one(&mut app.selected_track, &map);
    app.selected_region = app.selected_region.and_then(|(t, r)| map.get(t).copied().flatten().map(|n| (n, r)));
    app.selected_regions = app.selected_regions.iter().filter_map(|(t, r)| map.get(*t).copied().flatten().map(|n| (n, *r))).collect();
    app.ta.selected = app.ta.selected.iter().filter_map(|k| map.get(*k).copied().flatten()).collect();
}

/// The tracks a track command works on: the selected ones, else the focused one.
pub fn selection(app: &App) -> Vec<usize> {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let mut v: Vec<usize> = app.ta.selected.iter().copied().filter(|&t| t < n).collect();
    // (the multi-selection counts while it holds the focused track)
    if let Some(t) = app.selected_track.filter(|&t| t < n) {
        if !v.contains(&t) {
            v = vec![t];
        }
    }
    v.sort_unstable();
    v
}

/// A header clicked: Shift extends the selection to it, Command adds or removes it.
pub fn click_track(app: &mut App, t: usize, shift: bool, command: bool) {
    if shift {
        if let Some(a) = app.selected_track {
            let rows = rows(app);
            let (pa, pb) = (rows.iter().position(|r| r.t == a), rows.iter().position(|r| r.t == t));
            if let (Some(pa), Some(pb)) = (pa, pb) {
                app.ta.selected = rows[pa.min(pb)..=pa.max(pb)].iter().map(|r| r.t).collect();
                return;
            }
        }
    } else if command {
        if let Some(k) = app.ta.selected.iter().position(|&x| x == t) {
            app.ta.selected.remove(k);
        } else {
            if app.ta.selected.is_empty() {
                app.ta.selected.extend(app.selected_track);
            }
            app.ta.selected.push(t);
        }
        app.selected_track = Some(t);
        return;
    }
    app.ta.selected = vec![t];
    app.selected_track = Some(t);
}

/// The new order (new index -> old index) applied to what the undo list keeps beside the
/// project (each track's sound).
fn reorder_programs(app: &mut App, order: &[Option<usize>]) {
    let old = std::mem::take(&mut app.programs);
    app.programs = order.iter().map(|o| o.and_then(|k| old.get(k).copied()).unwrap_or(0)).collect();
}

fn stack_name(app: &App, kind: StackKind) -> String {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.iter().filter(|t| t.extra.stack.is_some_and(|s| s.kind == kind)).count()) + 1;
    match kind {
        StackKind::Summing => format!("Sum {n}"),
        StackKind::Folder => format!("Track Stack {n}"),
        StackKind::PackedFolder => "Folder".to_string(),
    }
}

/// Create Track Stack (Folder or Summing) or Pack Folder from the selected tracks.
pub fn create_stack(app: &mut App, kind: StackKind, tracks: Vec<usize>) -> bool {
    if tracks.is_empty() {
        return false;
    }
    let name = stack_name(app, kind);
    app.checkpoint(match kind {
        StackKind::PackedFolder => "Pack Folder",
        _ => "Create Track Stack",
    });
    let Some(p) = app.project.as_mut() else { return false };
    let Some((main, order)) = arrange::create_stack(&mut p.tracks, &tracks, kind, &name) else { return false };
    reorder_programs(app, &order);
    // (the ids follow by themselves; the index the sync has to know: the main track is new)
    app.after_edit();
    app.selected_track = Some(main);
    app.ta.selected = vec![main];
    true
}

/// The stack main track a command works on: the focused track if it is one, else its stack.
fn focused_stack(app: &App) -> Option<usize> {
    let t = app.selected_track?;
    let p = app.project.as_ref()?;
    if p.tracks.get(t)?.extra.stack.is_some() {
        return Some(t);
    }
    arrange::parent(&p.tracks, t)
}

fn flatten(app: &mut App, i: usize, what: &str) -> bool {
    app.checkpoint(what);
    let Some(p) = app.project.as_mut() else { return false };
    let n = p.tracks.len();
    if arrange::flatten(&mut p.tracks, i).is_none() {
        return false;
    }
    let order: Vec<Option<usize>> = (0..n).filter(|&k| k != i).map(Some).collect();
    reorder_programs(app, &order);
    if inside(app).is_none() {
        app.ta.inside = None;
    }
    app.after_edit();
    true
}

/// Moves a track (with its subtracks) before visible row `before_row` (rows.len(): the end).
pub fn move_track(app: &mut App, t: usize, before_row: usize) {
    let rows = rows(app);
    let Some(p) = app.project.as_ref() else { return };
    let before = rows.get(before_row).map_or(p.tracks.len(), |r| r.t);
    let mut tracks = p.tracks.clone();
    let Some(order) = arrange::move_group(&mut tracks, t, before) else { return };
    app.checkpoint("Move Track");
    if let Some(p) = app.project.as_mut() {
        p.tracks = tracks;
    }
    let order: Vec<Option<usize>> = order.into_iter().map(Some).collect();
    reorder_programs(app, &order);
    app.after_edit();
}

/// The track commands (Track menu, track header menu, key commands). False: not one of these.
pub fn command(app: &mut App, name: &str) -> bool {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let sel = selection(app);
    match name {
        // ---- track stacks and folders
        "Create Track Stack…" => {
            if sel.is_empty() {
                return false;
            }
            app.ta.stack_dialog = Some(StackKind::Folder);
        }
        "Create Folder Stack" => return create_stack(app, StackKind::Folder, sel),
        "Create Summing Stack" => return create_stack(app, StackKind::Summing, sel),
        "Pack Folder" => {
            // the selected regions' tracks (Logic), else the selected tracks
            let mut tracks: Vec<usize> = app.region_selection().into_iter().map(|(t, _)| t).collect();
            if tracks.is_empty() {
                tracks = sel;
            }
            tracks.sort_unstable();
            tracks.dedup();
            return create_stack(app, StackKind::PackedFolder, tracks);
        }
        "Flatten Stack" => {
            let Some(i) = focused_stack(app).filter(|&i| app.project.as_ref().is_some_and(|p| p.tracks[i].extra.stack.is_some_and(|s| s.kind != StackKind::PackedFolder))) else { return false };
            return flatten(app, i, "Flatten Stack");
        }
        "Unpack Folder to New Tracks" | "Unpack Folder to Existing Tracks" => {
            let folder = inside(app).or_else(|| focused_stack(app)).or_else(|| app.selected_region.map(|x| x.0));
            let Some(i) = folder.filter(|&i| app.project.as_ref().is_some_and(|p| p.tracks.get(i).is_some_and(|t| t.extra.stack.is_some_and(|s| s.kind == StackKind::PackedFolder)))) else { return false };
            app.ta.inside = None;
            return flatten(app, i, "Unpack Folder");
        }
        "Convert Folder Stack to Summing Stack" => {
            let Some(i) = focused_stack(app) else { return false };
            if !app.project.as_ref().is_some_and(|p| p.tracks[i].extra.stack.is_some_and(|s| s.kind == StackKind::Folder)) {
                return false;
            }
            app.checkpoint("Convert to Summing Stack");
            let name = stack_name(app, StackKind::Summing);
            if let Some(t) = app.project.as_mut().map(|p| &mut p.tracks[i]) {
                if let Some(s) = t.extra.stack.as_mut() {
                    s.kind = StackKind::Summing;
                }
                if t.name.starts_with("Track Stack") {
                    t.name = name;
                }
            }
            app.after_edit();
        }
        "Open Track Stack" | "Close Track Stack" | "Open/Close Track Stack" => {
            let Some(i) = focused_stack(app) else { return false };
            if let Some(s) = app.project.as_mut().and_then(|p| p.tracks[i].extra.stack.as_mut()) {
                s.open = match name {
                    "Open Track Stack" => true,
                    "Close Track Stack" => false,
                    _ => !s.open,
                };
            }
            if name != "Open Track Stack" {
                app.selected_track = Some(i);
            }
        }
        "Open/Close All Track Stacks" => {
            let Some(p) = app.project.as_mut() else { return false };
            let open = !p.tracks.iter().any(|t| t.extra.stack.is_some_and(|s| s.open && s.kind != StackKind::PackedFolder));
            for t in &mut p.tracks {
                if let Some(s) = t.extra.stack.as_mut().filter(|s| s.kind != StackKind::PackedFolder) {
                    s.open = open;
                }
            }
        }
        "Go Into Folder or Region" => {
            let folder = app.selected_region.map(|x| x.0).into_iter().chain(app.selected_track).find(|&t| app.project.as_ref().is_some_and(|p| p.tracks.get(t).is_some_and(|tr| tr.extra.stack.is_some_and(|s| s.kind == StackKind::PackedFolder))));
            let Some(f) = folder else { return false };
            app.ta.inside = app.project.as_ref().map(|p| p.tracks[f].extra.id);
            app.selected_track = app.project.as_ref().and_then(|p| (arrange::members(&p.tracks, f).start < p.tracks.len()).then_some(f + 1));
            app.scroll_y = 0.0;
        }
        "Go Out of Folder or Region" => {
            let Some(f) = inside(app) else { return false };
            let up = app.project.as_ref().and_then(|p| arrange::parent(&p.tracks, f)).filter(|&u| app.project.as_ref().is_some_and(|p| p.tracks[u].extra.stack.is_some_and(|s| s.kind == StackKind::PackedFolder)));
            app.ta.inside = up.and_then(|u| app.project.as_ref().map(|p| p.tracks[u].extra.id));
            app.selected_track = Some(f);
        }
        "Select Summing Stack Tracks" | "Select Folder Stack Tracks" | "Select Folder Tracks" | "Summing Stack" | "Folder Stack" | "Folder" => {
            let kind = if name.contains("Summing") { StackKind::Summing } else if name.contains("Stack") { StackKind::Folder } else { StackKind::PackedFolder };
            let v: Vec<usize> = app.project.as_ref().map_or(Vec::new(), |p| (0..n).filter(|&i| p.tracks[i].extra.stack.is_some_and(|s| s.kind == kind)).collect());
            if v.is_empty() {
                return true;
            }
            app.selected_track = v.first().copied();
            app.ta.selected = v;
        }
        // ---- hide
        "Hide Selected Track" | "Hide Unselected Track" => {
            let unselected = name.contains("Unselected");
            if let Some(p) = app.project.as_mut() {
                for (i, t) in p.tracks.iter_mut().enumerate() {
                    if sel.contains(&i) != unselected {
                        t.extra.hidden = true;
                    }
                }
            }
            app.ta.show_hidden = false;
            app.dirty = true;
        }
        "Unhide All Tracks" => {
            if let Some(p) = app.project.as_mut() {
                for t in &mut p.tracks {
                    t.extra.hidden = false;
                }
            }
            app.dirty = true;
        }
        "Toggle Hide View" => app.ta.show_hidden = !app.ta.show_hidden,
        // ---- track zoom
        "Individual Track Zoom In" | "Individual Track Zoom Out" | "Toggle Individual Track Zoom" | "Reset Individual Track Zoom" => {
            if sel.is_empty() {
                return false;
            }
            // (groups linking Track Zoom: their tracks zoom with the selected ones)
            let mut sel = sel.clone();
            for k in sel.clone() {
                sel.extend(crate::mixstate::linked(app, k, (2, 0x02)));
            }
            sel.sort();
            sel.dedup();
            if let Some(p) = app.project.as_mut() {
                for &i in &sel {
                    let t = &mut p.tracks[i];
                    let z = t.zoom();
                    t.extra.height = match name {
                        "Individual Track Zoom In" => ZOOM_STEPS.iter().copied().find(|&s| s > z + 0.01).unwrap_or(z),
                        "Individual Track Zoom Out" => ZOOM_STEPS.iter().rev().copied().find(|&s| s < z - 0.01).unwrap_or(z),
                        "Toggle Individual Track Zoom" => if (z - 1.0).abs() < 0.01 { 2.0 } else { 1.0 },
                        _ => 1.0,
                    };
                }
            }
        }
        "Individual Track Zoom Reset for All Tracks" => {
            if let Some(p) = app.project.as_mut() {
                for t in &mut p.tracks {
                    t.extra.height = 1.0;
                }
            }
        }
        "Zoom Vertical In" | "Zoom Vertical Out" => {
            let z = if app.ta.v_zoom > 0.0 { app.ta.v_zoom } else { 1.0 };
            app.ta.v_zoom = if name.ends_with("In") { ZOOM_STEPS.iter().copied().find(|&s| s > z + 0.01).unwrap_or(z) } else { ZOOM_STEPS.iter().rev().copied().find(|&s| s < z - 0.01).unwrap_or(z) };
        }
        "Toggle Zoom Focused Track or Row" | "Zoom Focused Track" => app.ta.zoom_focused = !app.ta.zoom_focused,
        // ---- track alternatives
        "New Track Alternative" | "Duplicate Track Alternative" => {
            let Some(t) = app.selected_track.filter(|&t| t < n) else { return false };
            app.checkpoint(if name.starts_with("New") { "New Track Alternative" } else { "Duplicate Track Alternative" });
            if let Some(tr) = app.project.as_mut().map(|p| &mut p.tracks[t]) {
                arrange::new_alternative(tr, name.starts_with("Duplicate"));
            }
            app.ta.show_alternatives = true;
            app.selected_region = None;
            app.selected_regions.clear();
            app.after_edit();
        }
        "Delete Inactive Track Alternatives" => {
            let Some(t) = app.selected_track.filter(|&t| t < n) else { return false };
            app.checkpoint("Delete Inactive Track Alternatives");
            if let Some(tr) = app.project.as_mut().map(|p| &mut p.tracks[t]) {
                tr.extra.alternatives.clear();
            }
            app.after_edit();
        }
        "Show Track Alternatives" | "Track Alternatives" => app.ta.show_alternatives = !app.ta.show_alternatives,
        "Show Transposition Track" => app.ta.show_transposition = !app.ta.show_transposition,
        _ => return false,
    }
    true
}

/// A switch to inactive alternative `k` of track `t`.
fn switch_alternative(app: &mut App, t: usize, k: usize) {
    app.checkpoint("Switch Track Alternative");
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        arrange::switch_alternative(tr, k);
    }
    app.selected_region = None;
    app.selected_regions.clear();
    app.after_edit();
}

/// Menu items that show a tick (Hide View, Zoom Focused Track…).
pub fn checked(app: &App, label: &str) -> Option<bool> {
    Some(match label {
        "Toggle Hide View" => !app.ta.show_hidden,
        "Zoom Focused Track" => app.ta.zoom_focused,
        "Show Track Alternatives" | "Track Alternatives" => app.ta.show_alternatives,
        _ => return None,
    })
}

/// Commands that need something to work on.
pub fn enabled(app: &App, label: &str) -> Option<bool> {
    let p = app.project.as_ref()?;
    let stack_of = || focused_stack(app).and_then(|i| p.tracks[i].extra.stack);
    Some(match label {
        "Flatten Stack" => stack_of().is_some_and(|s| s.kind != StackKind::PackedFolder),
        "Convert Folder Stack to Summing Stack" => stack_of().is_some_and(|s| s.kind == StackKind::Folder),
        "Unpack Folder to New Tracks" | "Unpack Folder to Existing Tracks" => inside(app).is_some() || stack_of().is_some_and(|s| s.kind == StackKind::PackedFolder) || app.selected_region.is_some_and(|(t, _)| p.tracks.get(t).is_some_and(|x| x.extra.stack.is_some_and(|s| s.kind == StackKind::PackedFolder))),
        "Unhide All Tracks" => p.tracks.iter().any(|t| t.extra.hidden),
        "Reset Individual Track Zoom" => app.selected_track.and_then(|t| p.tracks.get(t)).is_some_and(|t| (t.zoom() - 1.0).abs() > 0.01),
        "Summing Stack" | "Folder Stack" | "Folder" => p.tracks.iter().any(|t| t.extra.stack.is_some_and(|s| s.kind == match label { "Summing Stack" => StackKind::Summing, "Folder Stack" => StackKind::Folder, _ => StackKind::PackedFolder })),
        _ => return None,
    })
}

/// The Create Track Stack… sheet (Logic's: Folder stack or Summing stack, and what each does).
pub fn dialogs(app: &mut App, ctx: &egui::Context) {
    let Some(mut kind) = app.ta.stack_dialog else { return };
    let mut open = true;
    let mut create = false;
    // (a sheet, as Logic's: no title bar, the question as its heading, Details under a disclosure)
    egui::Window::new("Create Track Stack").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, vec2(0.0, 90.0)).open(&mut open).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Which kind of track stack do you want to create?").font(look::bold(13.0)));
        ui.add_space(6.0);
        ui.radio_value(&mut kind, StackKind::Folder, "Folder stack");
        ui.radio_value(&mut kind, StackKind::Summing, "Summing stack");
        ui.add_space(4.0);
        egui::CollapsingHeader::new("Details").id_salt("stack-details").default_open(true).show(ui, |ui| {
            ui.label(egui::RichText::new(match kind {
                StackKind::Folder | StackKind::PackedFolder => "Groups the tracks under a main track whose fader, mute and solo act on all of them. Use it to tidy up the Tracks area.",
                StackKind::Summing => "The tracks play into the main track's aux channel and are mixed there as one sound, so effects on the main track process them all.",
            }).color(look::text_dim()).size(11.5));
        });
        ui.add_space(10.0);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(egui::RichText::new("Create").color(egui::Color32::WHITE)).fill(look::accent()).min_size(vec2(76.0, 24.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            if ui.add(egui::Button::new("Cancel").min_size(vec2(76.0, 24.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.ta.stack_dialog = None;
            }
        });
    });
    if app.ta.stack_dialog.is_none() {
        return;
    }
    app.ta.stack_dialog = if open { Some(kind) } else { None };
    if create {
        app.ta.stack_dialog = None;
        let sel = selection(app);
        create_stack(app, kind, sel);
    }
}

/// What the stacks do to the sound: a stack's main track fader, pan, mute and solo act on its
/// subtracks (Folder stacks as Logic's VCA-like folder; Summing stacks approximate the aux they
/// sum into — its plug-ins aren't run yet). Applied to the engine's tracks by index.
pub fn stack_mix(app: &App, tracks: &mut [crate::audio::PlayTrack]) {
    let Some(p) = &app.project else { return };
    for i in 0..p.tracks.len().min(tracks.len()) {
        let anc = arrange::ancestors(&p.tracks, i);
        for a in anc {
            let m = &p.tracks[a];
            tracks[i].volume *= crate::app::db_to_gain(m.volume_db);
            if m.extra.stack.is_some_and(|s| s.kind == StackKind::Summing) {
                tracks[i].pan = (tracks[i].pan + m.pan).clamp(-1.0, 1.0);
            }
            tracks[i].mute |= m.mute;
            tracks[i].solo |= m.solo;
        }
    }
}

/// The Transposition track's shift for a note on track `t` at `tick` (none on drum tracks or
/// tracks set to ignore transposition).
pub fn transposition(app: &App, t: usize, tick: u32) -> i16 {
    let Some(p) = &app.project else { return 0 };
    if p.transposition.is_empty() || p.tracks.get(t).is_none_or(|x| x.kind != TrackKind::Instrument) || app.track_props.get(&t).is_some_and(|tp| tp.no_transpose) {
        return 0;
    }
    arrange::transposition_at(&p.transposition, tick) as i16
}

// ---------------------------------------------------------------------------------------------
// drawing

/// The header's extras: indentation by depth, the stack's disclosure triangle, the track
/// alternatives pop-up, the hidden mark. Returns how far the icon moves right.
pub fn header_extras(app: &mut App, ui: &mut egui::Ui, row: Rect, t: usize) -> f32 {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)).cloned() else { return 0.0 };
    let p = ui.painter().clone();
    let indent = tr.extra.depth as f32 * 14.0;
    if let Some(s) = tr.extra.stack {
        // the disclosure triangle (packed folders: double-click goes in)
        let c = pos2(row.min.x + 32.0 + indent, row.min.y + 15.0);
        let r = Rect::from_center_size(c, vec2(14.0, 14.0));
        let resp = ui.interact(r, ui.scope_id().with(("stack-open", t)), Sense::click()).on_hover_text(if s.kind == StackKind::PackedFolder { "Go into the folder" } else if s.open { "Close Track Stack" } else { "Open Track Stack" });
        let col = if resp.hovered() { look::text() } else { look::text_dim() };
        let open = s.open && s.kind != StackKind::PackedFolder;
        let tri = if open { vec![c + vec2(-4.5, -2.5), c + vec2(4.5, -2.5), c + vec2(0.0, 3.5)] } else { vec![c + vec2(-2.5, -4.5), c + vec2(3.5, 0.0), c + vec2(-2.5, 4.5)] };
        p.add(egui::Shape::convex_polygon(tri, col, Stroke::NONE));
        if resp.clicked() {
            if s.kind == StackKind::PackedFolder {
                app.selected_track = Some(t);
                app.ta.inside = Some(tr.extra.id);
                app.scroll_y = 0.0;
            } else if let Some(st) = app.project.as_mut().and_then(|p| p.tracks[t].extra.stack.as_mut()) {
                st.open = !st.open;
            }
        }
    }
    if tr.extra.hidden {
        p.text(pos2(row.max.x - 44.0, row.min.y + 15.0), Align2::CENTER_CENTER, "H", look::bold(10.0), Color32::from_rgb(0x4f, 0xa8, 0xe8));
    }
    // track alternatives: the pop-up right of the name (Logic: "<name> ⌄")
    if app.ta.show_alternatives || !tr.extra.alternatives.is_empty() {
        let label = if tr.extra.alt_name.is_empty() { "A".to_string() } else { tr.extra.alt_name.clone() };
        let ar = Rect::from_min_size(pos2(row.max.x - 70.0, row.min.y + 6.0), vec2(40.0, 17.0));
        let resp = ui.interact(ar, ui.scope_id().with(("track-alt", t)), Sense::click()).on_hover_text("Track Alternatives");
        p.rect_filled(ar, 4.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
        p.text(ar.center(), Align2::CENTER_CENTER, format!("{} ⌄", crate::tracks::ellipsize(&label, 4)), look::regular(10.5), look::text());
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            let cur = label.clone();
            let _ = ui.add(egui::Button::selectable(true, cur));
            for (k, a) in tr.extra.alternatives.iter().enumerate() {
                if ui.add(egui::Button::selectable(false, &a.name)).clicked() {
                    switch_alternative(app, t, k);
                    ui.close();
                }
            }
            ui.separator();
            if ui.add(egui::Button::new("New").shortcut_text("⌃⌥⌘N")).clicked() {
                app.selected_track = Some(t);
                command(app, "New Track Alternative");
                ui.close();
            }
            if ui.add(egui::Button::new("Duplicate").shortcut_text("⌃⌥⌘D")).clicked() {
                app.selected_track = Some(t);
                command(app, "Duplicate Track Alternative");
                ui.close();
            }
            if ui.button("Rename").clicked() {
                app.ta.renaming_alt = Some((t, label.clone()));
                ui.close();
            }
            if ui.add_enabled(!tr.extra.alternatives.is_empty(), egui::Button::new("Delete Inactive").shortcut_text("⌃⌥⌘⌦")).clicked() {
                app.selected_track = Some(t);
                command(app, "Delete Inactive Track Alternatives");
                ui.close();
            }
        });
        if let Some((rt, text)) = app.ta.renaming_alt.as_mut().filter(|x| x.0 == t) {
            let _ = rt;
            let te = ui.put(ar.expand2(vec2(30.0, 1.0)), egui::TextEdit::singleline(text).font(look::regular(11.0)));
            crate::widgets::focus_once(ui, &te);
            if te.lost_focus() {
                let name = text.trim().to_string();
                app.ta.renaming_alt = None;
                if !name.is_empty() {
                    app.checkpoint("Rename Track Alternative");
                    if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        x.extra.alt_name = name;
                    }
                }
            }
        }
    }
    indent
}

/// A stack's main track lane: a closed stack shows its subtracks' regions in one lane (Logic's
/// summary); a packed folder shows its folder region (double-click: go in).
pub fn stack_lane(app: &mut App, ui: &mut egui::Ui, lane: Rect, clip: Rect, t: usize, x_of: &dyn Fn(f32) -> f32) {
    let Some(p) = &app.project else { return };
    let Some(s) = p.tracks.get(t).and_then(|x| x.extra.stack) else { return };
    let m = arrange::members(&p.tracks, t);
    let regions: Vec<(u32, u32, usize)> = m.clone().flat_map(|i| p.tracks[i].regions.iter().filter(|r| !r.muted).map(move |r| (r.start, r.start + r.length, i))).collect();
    let lp = ui.painter_at(clip);
    if s.kind == StackKind::PackedFolder {
        let (Some(a), Some(b)) = (regions.iter().map(|r| r.0).min(), regions.iter().map(|r| r.1).max()) else { return };
        let r = Rect::from_min_max(pos2(x_of(a as f32), lane.min.y + 3.0), pos2(x_of(b as f32), lane.max.y - 3.0));
        let col = Color32::from_rgb(0x8a, 0x8f, 0x9a);
        lp.rect_filled(r, 4.0, col.gamma_multiply(0.85));
        let header = Rect::from_min_size(r.min, vec2(r.width(), 15.0));
        lp.rect_filled(header, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, look::mix(col, Color32::WHITE, 0.3));
        lp.with_clip_rect(header).text(pos2(header.min.x + 4.0, header.center().y), Align2::LEFT_CENTER, &p.tracks[t].name, look::semibold(10.5), Color32::from_rgb(0x15, 0x18, 0x20));
        // the tracks inside, one thin line of regions each
        let n = m.len().max(1) as f32;
        let inner = Rect::from_min_max(pos2(r.min.x, header.max.y + 2.0), pos2(r.max.x, r.max.y - 2.0));
        for (a, b, i) in &regions {
            let k = (i - m.start) as f32;
            let y = inner.min.y + (k + 0.5) / n * inner.height();
            lp.line_segment([pos2(x_of(*a as f32), y), pos2(x_of(*b as f32) - 1.0, y)], Stroke::new((inner.height() / n * 0.6).clamp(1.0, 4.0), look::mix(col, Color32::BLACK, 0.5)));
        }
        let resp = ui.interact(r.intersect(clip), ui.scope_id().with(("folder-region", t)), Sense::click());
        if resp.double_clicked() {
            app.selected_track = Some(t);
            app.ta.inside = Some(p.tracks[t].extra.id);
            app.scroll_y = 0.0;
        } else if resp.clicked() {
            app.selected_track = Some(t);
            app.selected_region = None;
        }
        return;
    }
    if s.open {
        return;
    }
    for (a, b, i) in regions {
        let r = Rect::from_min_max(pos2(x_of(a as f32), lane.min.y + 4.0), pos2(x_of(b as f32).max(x_of(a as f32) + 3.0), lane.max.y - 4.0));
        lp.rect_filled(r, 3.0, look::region_color(p.tracks[i].kind).gamma_multiply(0.55));
    }
}

/// Large icons when the headers are wide (Logic switches past about 340 pt).
pub fn large_icons(app: &App) -> bool {
    app.header_w >= 340.0
}

/// A stack main track's icon (stacked layers), or a folder's.
pub fn stack_icon(p: &egui::Painter, r: Rect, kind: StackKind) {
    use crate::glyphs::ph;
    let c = if kind == StackKind::Summing { Color32::from_rgb(0x6c, 0xc4, 0xd8) } else { Color32::from_rgb(0xa8, 0xae, 0xbc) };
    p.rect_filled(r, 6.0, c.gamma_multiply(0.2));
    let icon = if kind == StackKind::PackedFolder { ph::FOLDER_SIMPLE } else { ph::STACK };
    crate::glyphs::paint(p, r, icon, r.width() * 0.7, c);
}

/// The Transposition global track: a stepped line of semitones; click adds a point (the value
/// there), drag one up or down (or along), Option-click removes it.
pub fn transposition_lane(app: &mut App, ui: &mut egui::Ui, row: Rect, lanes_x: f32) {
    let p = ui.painter_at(row);
    p.hline(row.x_range(), row.max.y, Stroke::new(1.0, look::line()));
    p.text(pos2(row.min.x + 14.0, row.min.y + 14.0), Align2::LEFT_CENTER, "Transposition", look::semibold(12.0), look::text_dim());
    let lane = Rect::from_min_max(pos2(lanes_x, row.min.y), row.max);
    let lp = ui.painter_at(lane);
    let bar_t = app.bar_ticks() as f32;
    let (scroll, bw) = (app.scroll_x, app.bar_w);
    let x_of = |tick: f32| lanes_x + (tick / bar_t - scroll) * bw;
    let tick_of = |x: f32| ((x - lanes_x) / bw + scroll) * bar_t;
    let inner = lane.shrink2(vec2(0.0, 6.0));
    let y_of = |v: f32| inner.center().y - v / 24.0 * inner.height();
    let v_of = |y: f32| ((inner.center().y - y) / inner.height() * 24.0).round().clamp(-12.0, 12.0);
    lp.hline(lane.x_range(), y_of(0.0), Stroke::new(1.0, look::grid_line()));
    let pts = app.project.as_ref().map_or(Vec::new(), |p| p.transposition.clone());
    let col = Color32::from_rgb(0xe8, 0x9a, 0x3c);
    let mut line = vec![pos2(lane.min.x, y_of(0.0))];
    for (tk, v) in &pts {
        let x = x_of(*tk as f32);
        let last = line.last().copied().unwrap_or(pos2(x, y_of(0.0)));
        line.push(pos2(x, last.y));
        line.push(pos2(x, y_of(*v as f32)));
    }
    if let Some(last) = line.last().copied() {
        line.push(pos2(lane.max.x, last.y));
    }
    lp.add(egui::Shape::line(line, Stroke::new(1.6, col)));
    let mut edit: Option<Vec<(u32, i8)>> = None;
    let mut on_point = false;
    for (k, (tk, v)) in pts.iter().enumerate() {
        let c = pos2(x_of(*tk as f32), y_of(*v as f32));
        if !lane.contains(c) {
            continue;
        }
        lp.circle_filled(c, 4.0, col);
        lp.text(c + vec2(6.0, -7.0), Align2::LEFT_CENTER, format!("{v:+}"), look::regular(9.5), look::text_dim());
        let resp = ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), ui.scope_id().with(("transp-pt", k)), Sense::click_and_drag());
        on_point |= resp.hovered() || resp.dragged();
        if resp.clicked() && ui.input(|i| i.modifiers.alt) {
            let mut n = pts.clone();
            n.remove(k);
            app.checkpoint("Delete Transposition Point");
            edit = Some(n);
        } else if resp.drag_started() {
            app.checkpoint("Move Transposition Point");
        } else if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let mut n = pts.clone();
            let beat = bar_t / 4.0;
            let lo = if k > 0 { pts[k - 1].0 + 1 } else { 0 };
            let hi = pts.get(k + 1).map_or(u32::MAX, |x| x.0.saturating_sub(1));
            n[k] = ((((tick_of(pp.x) / beat).round() * beat).max(0.0) as u32).clamp(lo, hi.max(lo)), v_of(pp.y) as i8);
            app.help_tag = Some(vec![("Transposition".into(), format!("{:+}", n[k].1))]);
            edit = Some(n);
        }
    }
    let bg = ui.interact(lane, ui.scope_id().with("transp-lane"), Sense::click()).on_hover_text("Transposition: click to add a point, drag to change it");
    if !on_point && bg.clicked() {
        if let Some(pp) = bg.interact_pointer_pos() {
            app.checkpoint("Create Transposition Point");
            let at = ((tick_of(pp.x) / bar_t).round() * bar_t).max(0.0) as u32;
            let mut n = pts.clone();
            n.retain(|x| x.0 != at);
            n.push((at, v_of(pp.y) as i8));
            n.sort_by_key(|x| x.0);
            edit = Some(n);
        }
    }
    if let Some(n) = edit {
        if let Some(p) = app.project.as_mut() {
            p.transposition = n;
        }
        app.dirty = true;
        app.load_engine();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_maps_follow_their_tracks() {
        let mut m: HashMap<usize, &str> = HashMap::from([(0, "a"), (1, "b"), (2, "c")]);
        // track 0 deleted, 1 and 2 swapped
        remap_map(&mut m, &[None, Some(1), Some(0)]);
        assert_eq!(m, HashMap::from([(1, "b"), (0, "c")]));
        let mut s: HashSet<usize> = HashSet::from([0, 2]);
        remap_set(&mut s, &[None, Some(1), Some(0)]);
        assert_eq!(s, HashSet::from([0]));
    }
}
