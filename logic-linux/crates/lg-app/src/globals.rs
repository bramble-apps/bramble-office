//! Logic's global tracks as a set: which show (Track ▸ Global Tracks ▸ Show X Track, the
//! headers' shortcut menu, the Configure Global Tracks dialog with Enable All / Disable All and
//! Show Single Track), Single Global Track mode (⌃⌥⌘G; ' shows the Marker track only; Cycle
//! Through Single Global Tracks), their order (drag a header up or down) and heights (drag a
//! track's lower divider; below the last header: the whole area), Protect buttons, and
//! selecting / deselecting them. tracks.rs draws each track's lane.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

pub const KINDS: [&str; 7] = ["Arrangement", "Marker", "Movie", "Tempo", "Beat Mapping", "Signature", "Chord"];
pub const ARRANGEMENT: usize = 0;
pub const MARKER: usize = 1;
pub const MOVIE: usize = 2;
pub const TEMPO: usize = 3;
pub const BEAT_MAPPING: usize = 4;
pub const SIGNATURE: usize = 5;
pub const CHORD: usize = 6;

const DEFAULT_H: [f32; 7] = [28.0, 28.0, 60.0, 64.0, 44.0, 28.0, 28.0];
const MIN_H: f32 = 22.0;
const MAX_H: f32 = 240.0;

pub const COMMANDS: &[&str] = &[
    "Configure Global Tracks…",
    "Show Single Global Track On/Off",
    "Show Single Global Track",
    "Show/Hide Arrangement Track",
    "Show/Hide Marker Track",
    "Show/Hide Movie Track",
    "Show/Hide Signature Track",
    "Show/Hide Tempo Track",
    "Show/Hide Chord Track",
    "Show/Hide Beat Mapping Track",
    "Show/Hide Arrangement Track Only",
    "Show/Hide Marker Track Only",
    "Show/Hide Movie Track Only",
    "Show/Hide Signature Track Only",
    "Show/Hide Tempo Track Only",
    "Show/Hide Chord Track Only",
    "Show/Hide Beat Mapping Track Only",
    "Cycle Through Single Global Tracks",
    "Hide All Global Tracks",
    "Show All Global Tracks",
    "Deselect Global Tracks",
    "Global Track Protect Buttons",
];

#[derive(Clone, Debug)]
enum GDrag {
    Height { k: usize, h0: f32 },
    All { h0: [f32; 7] },
    Order { k: usize },
}

#[derive(Clone, Debug)]
pub struct State {
    shown: [bool; 7],
    /// The tracks' order (kinds, top to bottom).
    pub order: [usize; 7],
    pub heights: [f32; 7],
    /// Single Global Track: only `single_k` shows.
    pub single: bool,
    pub single_k: usize,
    pub config_open: bool,
    /// Protect buttons shown in the headers, and which tracks are protected.
    pub protect_buttons: bool,
    pub protect: [bool; 7],
    /// The selected global track (its header highlighted).
    pub sel: Option<usize>,
    drag: Option<GDrag>,
}

impl Default for State {
    fn default() -> Self {
        // Logic's default: Arrangement, Marker, Tempo and Signature
        State { shown: [true, true, false, true, false, true, false], order: [0, 1, 2, 3, 4, 5, 6], heights: DEFAULT_H, single: false, single_k: MARKER, config_open: false, protect_buttons: false, protect: [false; 7], sel: None, drag: None }
    }
}

/// Whether global track `k` is enabled (Movie: the movie's own switch).
pub fn shown(app: &App, k: usize) -> bool {
    if k == MOVIE {
        app.hub.movie.track
    } else {
        app.gt.shown[k]
    }
}

fn set_shown(app: &mut App, k: usize, on: bool) {
    if k == MOVIE {
        app.hub.movie.track = on;
    } else {
        app.gt.shown[k] = on;
    }
}

/// The global tracks drawn, top to bottom: (kind, height).
pub fn rows(app: &App) -> Vec<(usize, f32)> {
    let g = &app.gt;
    g.order.iter().copied().filter(|&k| if g.single { k == g.single_k } else { shown(app, k) }).map(|k| (k, g.heights[k])).collect()
}

/// The height of the global tracks (with the Transposition track below them).
pub fn total_h(app: &App, transposition_h: f32) -> f32 {
    rows(app).iter().map(|r| r.1).sum::<f32>() + if app.ta.show_transposition && !app.gt.single { transposition_h } else { 0.0 }
}

fn kind_named(s: &str) -> Option<usize> {
    KINDS.iter().position(|k| *k == s)
}

pub fn command(app: &mut App, name: &str) -> bool {
    let g = &mut app.gt;
    match name {
        "Configure Global Tracks…" => g.config_open = true,
        "Show Global Tracks" => app.global_tracks = !app.global_tracks,
        "Show Single Global Track On/Off" | "Show Single Global Track" | "Single Global Track" => {
            g.single = !g.single;
            if g.single {
                // the selected track, else the first shown one
                let first = g.order.iter().copied().find(|&k| g.shown[k]);
                g.single_k = g.sel.or(first).unwrap_or(MARKER);
                app.global_tracks = true;
            }
        }
        "Cycle Through Single Global Tracks" => {
            let order = g.order;
            let i = order.iter().position(|&k| k == g.single_k).unwrap_or(0);
            g.single_k = order[(i + 1) % order.len()];
            g.single = true;
            app.global_tracks = true;
        }
        "Hide All Global Tracks" => app.global_tracks = false,
        "Show All Global Tracks" => {
            g.single = false;
            for k in 0..KINDS.len() {
                set_shown(app, k, true);
            }
            app.global_tracks = true;
        }
        "Deselect Global Tracks" => g.sel = None,
        "Global Track Protect Buttons" => g.protect_buttons = !g.protect_buttons,
        n => {
            let (only, track) = if let Some(t) = n.strip_prefix("Show/Hide ").and_then(|t| t.strip_suffix(" Track Only")) {
                (true, t)
            } else if let Some(t) = n.strip_prefix("Show/Hide ").and_then(|t| t.strip_suffix(" Track")).or_else(|| n.strip_prefix("Show ").and_then(|t| t.strip_suffix(" Track"))) {
                (false, t)
            } else {
                return false;
            };
            let Some(k) = kind_named(track) else { return false };
            if only {
                // that track alone; again: back to all of them
                if app.global_tracks && g.single && g.single_k == k {
                    g.single = false;
                } else {
                    g.single = true;
                    g.single_k = k;
                    app.global_tracks = true;
                }
            } else {
                let on = !(app.global_tracks && shown(app, k));
                set_shown(app, k, on);
                if on {
                    app.global_tracks = true;
                    app.gt.single = false;
                }
            }
        }
    }
    true
}

/// Menu checks: Track ▸ Global Tracks ▸ …
pub fn checked(app: &App, label: &str) -> Option<bool> {
    match label {
        "Show Global Tracks" => Some(app.global_tracks),
        "Show Single Global Track" | "Single Global Track" => Some(app.gt.single),
        "Global Track Protect Buttons" => Some(app.gt.protect_buttons),
        l => {
            let t = l.strip_prefix("Show ").and_then(|t| t.strip_suffix(" Track"))?;
            let k = kind_named(t)?;
            Some(app.global_tracks && shown(app, k))
        }
    }
}

/// The headers' shortcut menu (Logic's): Deselect Global Tracks, Configure Global Tracks…,
/// Single Global Track, then each track with a check (and Studio's Transposition).
pub fn header_menu(app: &mut App, resp: &egui::Response) {
    resp.context_menu(|ui| {
        if ui.add_enabled(app.gt.sel.is_some(), egui::Button::new("Deselect Global Tracks")).clicked() {
            app.gt.sel = None;
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::new("Configure Global Tracks…").shortcut_text("⌥G")).clicked() {
            app.gt.config_open = true;
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::selectable(app.gt.single, "Single Global Track").shortcut_text("⌃⌥⌘G")).clicked() {
            command(app, "Show Single Global Track On/Off");
            ui.close();
        }
        ui.separator();
        for (k, n) in KINDS.iter().enumerate() {
            if ui.add(egui::Button::selectable(shown(app, k), *n)).clicked() {
                let on = !shown(app, k);
                set_shown(app, k, on);
                app.gt.single = false;
                ui.close();
            }
        }
        ui.separator();
        let on = app.ta.show_transposition;
        if ui.add(egui::Button::selectable(on, "Transposition")).clicked() {
            app.ta.show_transposition = !on;
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::selectable(app.gt.protect_buttons, "Global Track Protect Buttons")).clicked() {
            app.gt.protect_buttons = !app.gt.protect_buttons;
            ui.close();
        }
    });
}

/// One global track's header: click selects it, the Protect button, its lower divider resizes
/// it, a drag up or down moves it.
pub fn header(app: &mut App, ui: &mut egui::Ui, k: usize, row: Rect, lanes_x: f32) {
    let p = ui.painter();
    let head = Rect::from_min_max(row.min, pos2(lanes_x, row.max.y));
    if app.gt.sel == Some(k) {
        p.rect_filled(head, 0.0, look::accent().gamma_multiply(0.25));
    }
    // the divider at the bottom (the last track: just above it)
    let div = Rect::from_min_max(pos2(head.min.x, row.max.y - 4.0), pos2(head.max.x - 30.0, row.max.y + 1.0));
    let dr = ui.interact(div, ui.scope_id().with(("gt-div", k)), Sense::drag());
    if dr.hovered() || dr.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if dr.drag_started() {
        app.gt.drag = Some(GDrag::Height { k, h0: app.gt.heights[k] });
    }
    if let (Some(GDrag::Height { k: dk, h0 }), true) = (app.gt.drag.clone(), dr.dragged()) {
        if dk == k {
            let d = ui.input(|i| i.pointer.press_origin().zip(i.pointer.hover_pos()).map_or(0.0, |(o, h)| h.y - o.y));
            app.gt.heights[k] = (h0 + d).clamp(MIN_H, MAX_H);
        }
    }
    // the name: click selects, drag reorders
    let name = Rect::from_min_max(pos2(head.min.x, row.min.y), pos2(head.max.x - 30.0, row.max.y - 4.0));
    let nr = ui.interact(name, ui.scope_id().with(("gt-name", k)), Sense::click_and_drag());
    if nr.clicked() {
        app.gt.sel = Some(k);
    }
    if nr.drag_started() {
        app.gt.drag = Some(GDrag::Order { k });
    }
    if let Some(GDrag::Order { k: dk }) = app.gt.drag.clone() {
        if dk == k && nr.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
    }
    // the Protect button
    if app.gt.protect_buttons {
        let b = Rect::from_center_size(pos2(head.max.x - 44.0, row.min.y + 14.0), vec2(16.0, 16.0));
        let on = app.gt.protect[k];
        let br = ui.interact(b, ui.scope_id().with(("gt-protect", k)), Sense::click()).on_hover_text(if on { "Protected: click to allow edits" } else { "Protect this global track from edits" });
        crate::glyphs::paint(p, b, if on { crate::glyphs::ph_fill::LOCK_SIMPLE } else { crate::glyphs::ph::LOCK_SIMPLE_OPEN }, 13.0, if on { look::accent() } else { look::text_dim() });
        if br.clicked() {
            app.gt.protect[k] = !on;
        }
    }
}

/// After all rows: a header dropped elsewhere moves its track there; the strip below the last
/// header resizes the whole area; protected tracks take no edits.
pub fn after_rows(app: &mut App, ui: &mut egui::Ui, placed: &[(usize, Rect)], lanes_x: f32) {
    let Some(last) = placed.last().map(|p| p.1) else { return };
    // reorder: where the pointer is, the dragged track goes
    if let Some(GDrag::Order { k }) = app.gt.drag.clone() {
        let down = ui.input(|i| i.pointer.primary_down());
        if let Some(h) = ui.input(|i| i.pointer.hover_pos()) {
            let to = placed.iter().find(|(_, r)| h.y < r.center().y).or(placed.last()).map(|p| p.0);
            if let Some(t) = to {
                ui.painter().hline(placed[0].1.min.x..=lanes_x, placed.iter().find(|p| p.0 == t).map_or(last.max.y, |p| if h.y < p.1.center().y { p.1.min.y } else { p.1.max.y }), Stroke::new(2.0, look::accent()));
                if !down {
                    move_before(app, k, t, h.y >= last.center().y && t == placed.last().unwrap().0);
                }
            }
        }
        if !down {
            app.gt.drag = None;
        }
    }
    // the whole area, from just below the last header's divider
    let strip = Rect::from_min_max(pos2(last.min.x, last.max.y + 1.0), pos2(lanes_x - 30.0, last.max.y + 5.0));
    let sr = ui.interact(strip, ui.scope_id().with("gt-all"), Sense::drag());
    if sr.hovered() || sr.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if sr.drag_started() {
        app.gt.drag = Some(GDrag::All { h0: app.gt.heights });
    }
    if let (Some(GDrag::All { h0 }), true) = (app.gt.drag.clone(), sr.dragged()) {
        let total: f32 = placed.iter().map(|p| h0[p.0]).sum();
        let d = ui.input(|i| i.pointer.press_origin().zip(i.pointer.hover_pos()).map_or(0.0, |(o, h)| h.y - o.y));
        let f = ((total + d) / total.max(1.0)).max(0.2);
        for (k, _) in placed {
            app.gt.heights[*k] = (h0[*k] * f).clamp(MIN_H, MAX_H);
        }
    }
    if (sr.drag_stopped() || ui.input(|i| !i.pointer.primary_down())) && !matches!(app.gt.drag, Some(GDrag::Order { .. })) {
        app.gt.drag = None;
    }
    // protected tracks: the lane takes the pointer (nothing under it is edited)
    for (k, r) in placed {
        if app.gt.protect[*k] {
            let lane = Rect::from_min_max(pos2(lanes_x, r.min.y), r.max);
            let resp = ui.interact(lane, ui.scope_id().with(("gt-protected", *k)), Sense::click_and_drag());
            ui.painter().rect_filled(lane, 0.0, Color32::from_black_alpha(40));
            if resp.clicked() || resp.drag_started() {
                app.tell(format!("The {} track is protected", KINDS[*k]));
            }
        }
    }
}

fn move_before(app: &mut App, k: usize, target: usize, after: bool) {
    if k == target {
        return;
    }
    let mut o: Vec<usize> = app.gt.order.iter().copied().filter(|&x| x != k).collect();
    let i = o.iter().position(|&x| x == target).unwrap_or(o.len());
    o.insert(if after { i + 1 } else { i }, k);
    app.gt.order.copy_from_slice(&o);
}

/// Configure Global Tracks: a checkbox per track, Enable All, Disable All, Show Single Track.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.gt.config_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Global Tracks Configuration").id(egui::Id::unique("gt-config")).open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        for &k in &app.gt.order.clone() {
            let mut on = shown(app, k);
            if ui.checkbox(&mut on, KINDS[k]).changed() {
                set_shown(app, k, on);
                if on {
                    app.global_tracks = true;
                }
            }
        }
        let mut tr = app.ta.show_transposition;
        if ui.checkbox(&mut tr, "Transposition").changed() {
            app.ta.show_transposition = tr;
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Enable All").clicked() {
                for k in 0..KINDS.len() {
                    set_shown(app, k, true);
                }
                app.global_tracks = true;
            }
            if ui.button("Disable All").clicked() {
                for k in 0..KINDS.len() {
                    set_shown(app, k, false);
                }
            }
        });
        ui.checkbox(&mut app.gt.single, "Show Single Track");
        ui.checkbox(&mut app.gt.protect_buttons, "Protect buttons");
    });
    app.gt.config_open = open;
}

/// The Beat Mapping track: the beat mappings made (Beats from Region) as lines from their
/// ruler position.
pub fn beat_mapping_lane(app: &App, p: &egui::Painter, lane: Rect, x_of: &dyn Fn(f32) -> f32) {
    let Some(proj) = app.project.as_ref() else { return };
    p.text(pos2(lane.min.x + 6.0, lane.min.y + 3.0), Align2::LEFT_TOP, "", look::regular(10.0), look::text_dim());
    for &(t, _) in &proj.global.beat_maps {
        let x = x_of(t as f32);
        p.vline(x, lane.y_range(), Stroke::new(1.0, Color32::from_rgb(0xe0, 0x9a, 0x3c)));
        p.circle_filled(pos2(x, lane.min.y + 6.0), 3.0, Color32::from_rgb(0xe0, 0x9a, 0x3c));
    }
}

// ---- signature sets ------------------------------------------------------------------------------

/// The Signature List's Signature Set pop-up: the sets (up to nine), New Set, Rename Set…,
/// Delete Set.
pub fn sig_set_popup(app: &mut App, ui: &mut egui::Ui) {
    let Some(p) = app.project.as_ref() else { return };
    let g = &p.global;
    let names: Vec<String> = if g.sig_sets.is_empty() { vec!["Signature Set 1".into()] } else { g.sig_sets.iter().map(|s| s.name.clone()).collect() };
    let cur = g.sig_set.min(names.len() - 1);
    ui.label(egui::RichText::new("Signature Set:").color(look::text_dim()));
    let id = ui.scope_id().with("sig-set-rename");
    if let Some(mut n) = ui.data(|d| d.get_temp::<String>(id)) {
        let te = ui.add(egui::TextEdit::singleline(&mut n).desired_width(120.0));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(id));
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && !n.trim().is_empty() {
                app.checkpoint("Rename Signature Set");
                if let Some(p) = app.project.as_mut() {
                    lg_core::timeline::rename_sig_set(&mut p.global, n.trim());
                }
            }
        } else {
            ui.data_mut(|d| d.insert_temp(id, n));
        }
        return;
    }
    let mut action = None;
    egui::ComboBox::from_id_salt("sig-set").selected_text(names[cur].clone()).width(130.0).show_ui(ui, |ui| {
        for (k, n) in names.iter().enumerate() {
            if ui.selectable_label(k == cur, n).clicked() {
                action = Some(k as i32);
            }
        }
        ui.separator();
        if ui.add_enabled(names.len() < lg_core::timeline::MAX_SIG_SETS, egui::Button::new("New Set")).clicked() {
            action = Some(-1);
        }
        if ui.button("Rename Set…").clicked() {
            action = Some(-2);
        }
        if ui.add_enabled(names.len() > 1, egui::Button::new("Delete Set")).clicked() {
            action = Some(-3);
        }
    });
    let Some(a) = action else { return };
    if a == -2 {
        ui.data_mut(|d| d.insert_temp(id, names[cur].clone()));
        return;
    }
    if a >= 0 && a as usize == cur {
        return;
    }
    app.checkpoint(match a {
        -1 => "New Signature Set",
        -3 => "Delete Signature Set",
        _ => "Signature Set",
    });
    if let Some(p) = app.project.as_mut() {
        let g = &mut p.global;
        match a {
            -1 => {
                lg_core::timeline::new_sig_set(g, None);
            }
            -3 => {
                lg_core::timeline::delete_sig_set(g);
            }
            k => lg_core::timeline::switch_sig_set(g, k as usize),
        }
    }
    app.after_edit();
}

/// Extras lines for the signature sets other than the current one.
pub fn extras_text(p: &lg_core::Project) -> String {
    let g = &p.global;
    let mut out = String::new();
    for (k, s) in g.sig_sets.iter().enumerate() {
        let (sigs, keys) = if k == g.sig_set { (Vec::new(), Vec::new()) } else { (s.signatures.iter().map(|x| format!("{}:{}:{}", x.0, x.1, x.2)).collect::<Vec<_>>(), s.keys.iter().map(|x| format!("{}:{}", x.0, x.1)).collect::<Vec<_>>()) };
        out += &format!("sset\t{k}\t{}\t{}\t{}\n", s.name, sigs.join(","), keys.join(","));
    }
    if !g.sig_sets.is_empty() {
        out += &format!("scur\t{}\n", g.sig_set);
    }
    out
}

pub fn apply_extras(p: &mut lg_core::Project, l: &str) -> bool {
    let w: Vec<&str> = l.split('\t').collect();
    match w.as_slice() {
        ["sset", _, name, sigs, keys] => {
            let signatures = sigs.split(',').filter_map(|x| {
                let v: Vec<&str> = x.split(':').collect();
                Some((v.first()?.parse().ok()?, v.get(1)?.parse().ok()?, v.get(2)?.parse().ok()?))
            });
            let keys = keys.split(',').filter_map(|x| x.split_once(':')).filter_map(|(a, b)| Some((a.parse().ok()?, b.to_string())));
            p.global.sig_sets.push(lg_core::timeline::SigSet { name: name.to_string(), signatures: signatures.collect(), keys: keys.collect() });
            true
        }
        ["scur", k] => {
            let g = &mut p.global;
            g.sig_set = k.parse().unwrap_or(0);
            if let Some(s) = g.sig_sets.get_mut(g.sig_set) {
                s.signatures = g.signatures.clone();
                s.keys = g.keys.clone();
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod sig_tests {
    use super::*;

    #[test]
    fn sig_set_extras_round_trip() {
        let mut p = crate::demo::empty();
        p.global.signatures.push((4, 3, 4));
        lg_core::timeline::new_sig_set(&mut p.global, Some("B"));
        p.global.keys.push((2, "E minor".into()));
        // (the current set's copy is brought up to date when the sets are read back)
        p.global.sig_sets[1].keys = p.global.keys.clone();
        let text = extras_text(&p);
        let mut q = crate::demo::empty();
        q.global.keys = p.global.keys.clone();
        for l in text.lines() {
            assert!(apply_extras(&mut q, l));
        }
        assert_eq!(q.global, p.global);
    }

    #[test]
    fn show_hide_single_and_order() {
        let mut app = crate::tm::test_app();
        app.global_tracks = true;
        assert_eq!(rows(&app).iter().map(|r| r.0).collect::<Vec<_>>(), [ARRANGEMENT, MARKER, TEMPO, SIGNATURE]);
        command(&mut app, "Show/Hide Chord Track");
        assert!(rows(&app).iter().any(|r| r.0 == CHORD));
        command(&mut app, "Show/Hide Marker Track Only");
        assert_eq!(rows(&app).iter().map(|r| r.0).collect::<Vec<_>>(), [MARKER]);
        command(&mut app, "Cycle Through Single Global Tracks");
        assert_eq!(rows(&app)[0].0, MOVIE);
        command(&mut app, "Show Single Global Track On/Off");
        assert!(rows(&app).len() > 1);
        move_before(&mut app, SIGNATURE, ARRANGEMENT, false);
        assert_eq!(rows(&app)[0].0, SIGNATURE);
        command(&mut app, "Hide All Global Tracks");
        assert!(!app.global_tracks);
        command(&mut app, "Show All Global Tracks");
        assert_eq!(rows(&app).len(), 7);
        assert_eq!(checked(&app, "Show Tempo Track"), Some(true));
    }
}
