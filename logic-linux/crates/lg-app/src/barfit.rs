//! Bars that lay out by their real width, as Logic's local menu bars and the control bar do:
//! the full set of controls when they fit; as the bar narrows, controls leave in a fixed order
//! (Logic's: the right-hand end first) and turn up in the bar's "…" menu, which appears only
//! when something had to leave.
//!
//! `fit` is the layout rule (pure, tested); `tracks_bar` is the Tracks area's local menu bar,
//! Logic 13's: ↑ (out of the folder), Edit / Functions / View, Live Loops grid | Tracks, Flex,
//! and at the right Automation, Catch, the Left-click and Command-click tools, Snap, Drag,
//! Waveform Zoom, Vertical and Horizontal Auto Zoom and the two zoom sliders.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::glyphs::{self, ph, ph_fill};
use crate::look;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
#[allow(dead_code)]
pub const COMMANDS: &[&str] = &["Horizontal Auto Zoom", "Vertical Auto Zoom", "Catch", "Waveform Zoom"];

/// The bars' own state (what the Tracks area's buttons turn on).
pub struct State {
    /// Catch (the Tracks area's Catch button): the view follows the playhead while playing.
    pub catch: bool,
    pub v_auto: bool,
    pub h_auto: bool,
    /// The waveform zoom the Waveform Zoom button turns back on.
    pub wave_level: f32,
    /// What the auto zooms last fitted: the track count and area height, the project's end.
    v_seen: (usize, i32),
    h_seen: u32,
}

impl Default for State {
    fn default() -> State {
        State { catch: true, v_auto: false, h_auto: false, wave_level: 2.0, v_seen: (usize::MAX, 0), h_seen: u32::MAX }
    }
}

/// A control in a bar: its width (with the gap before it) and when it leaves: higher `drop`
/// leaves earlier; among equals the right-most leaves first. `drop == 0` never leaves.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub w: f32,
    pub drop: u8,
}

/// Which controls are shown in `avail` points; when any must leave, `more_w` is kept for the
/// "…" button. Controls leave one at a time in their order until the rest fit.
pub fn fit(slots: &[Slot], avail: f32, more_w: f32) -> Vec<bool> {
    let mut shown = vec![true; slots.len()];
    let total = |shown: &[bool]| slots.iter().zip(shown).filter(|(_, s)| **s).map(|(x, _)| x.w).sum::<f32>();
    if total(&shown) <= avail {
        return shown;
    }
    let mut order: Vec<usize> = (0..slots.len()).filter(|&i| slots[i].drop > 0).collect();
    order.sort_by(|&a, &b| slots[b].drop.cmp(&slots[a].drop).then(b.cmp(&a)));
    for i in order {
        shown[i] = false;
        if total(&shown) + more_w <= avail {
            break;
        }
    }
    shown
}

/// Whether any control left the bar (the "…" button shows then).
pub fn any_hidden(shown: &[bool]) -> bool {
    shown.iter().any(|s| !s)
}

// ---------------------------------------------------------------------------------------------
// drawing: Logic's local-bar controls (24 pt tall, 6 pt corners, grey capsules, blue when on)

const BTN_H: f32 = 24.0;
const ICON_W: f32 = 36.0;
pub(crate) const GAP: f32 = 8.0;

pub(crate) fn cell(cy: f32, x: f32, w: f32) -> Rect {
    Rect::from_min_size(pos2(x, cy - BTN_H / 2.0), vec2(w, BTN_H))
}

/// A square-ish icon button; `on`: Logic's blue.
pub(crate) fn icon_button(ui: &mut egui::Ui, r: Rect, id: egui::Id, on: bool, enabled: bool, tip: &str, icon: &str) -> egui::Response {
    let resp = ui.interact(r, id, if enabled { Sense::click_and_drag() } else { Sense::hover() }).on_hover_text(tip);
    let p = ui.painter();
    let fill = if on { look::accent() } else if resp.hovered() && enabled { look::button_on() } else { look::group_bg() };
    p.rect_filled(r, 6.0, fill);
    let col = if on { Color32::WHITE } else if enabled { look::text() } else { look::text_dim().gamma_multiply(0.6) };
    glyphs::paint(p, r, icon, 16.0, col);
    resp
}

pub(crate) fn chevron(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    glyphs::paint(p, Rect::from_center_size(c, vec2(12.0, 12.0)), ph::CARET_DOWN, 10.0, col);
}

/// A text pop-up button ("Smart ⌄"); returns its response for the menu.
pub(crate) fn text_popup(ui: &mut egui::Ui, r: Rect, id: egui::Id, text: &str, tip: &str) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click()).on_hover_text(tip);
    let p = ui.painter();
    p.rect_filled(r, 6.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
    p.text(pos2(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, text, look::regular(13.0), look::text());
    chevron(p, pos2(r.max.x - 11.0, r.center().y), look::text());
    resp
}

pub(crate) fn text_w(ui: &egui::Ui, text: &str) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), look::regular(13.0), Color32::WHITE).size().x)
}

/// A zoom slider as Logic's (measured for the Score's, scoretools.rs): a darker pill 75 × 24 pt
/// with the icon at 10 pt, a 48 pt groove (blue up to the knob) and a 7.5 pt round knob.
fn slider(ui: &mut egui::Ui, r: Rect, id: egui::Id, v: &mut f32, icon: &str, tip: &str) -> bool {
    let p = ui.painter().clone();
    let y = r.center().y;
    p.rect_filled(r, 6.0, look::mix(look::header_bg(), Color32::BLACK, 0.2));
    glyphs::paint(&p, Rect::from_center_size(pos2(r.min.x + 10.0, y), vec2(14.0, 14.0)), icon, 12.0, look::text());
    let groove = Rect::from_min_max(pos2(r.min.x + 22.0, y - 2.0), pos2(r.max.x - 5.0, y + 2.0));
    let resp = ui.interact(groove.expand2(vec2(6.0, 8.0)), id, Sense::click_and_drag()).on_hover_text(tip);
    let mut changed = false;
    if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        *v = ((pp.x - groove.min.x) / groove.width().max(1.0)).clamp(0.0, 1.0);
        changed = true;
    }
    let kx = groove.min.x + groove.width() * v.clamp(0.0, 1.0);
    p.rect_filled(groove, 2.0, Color32::from_gray(0x31));
    p.rect_filled(Rect::from_min_max(groove.min, pos2(kx, groove.max.y)), 2.0, Color32::from_rgb(0x06, 0x65, 0xad));
    p.circle_filled(pos2(kx, y), 7.5, Color32::from_gray(0xde));
    p.circle_stroke(pos2(kx, y), 7.5, Stroke::new(0.5, Color32::from_black_alpha(60)));
    changed
}

/// The "…" button (Logic's capsule of three dots and a chevron); returns its response.
pub(crate) fn more_button(ui: &mut egui::Ui, r: Rect, id: egui::Id) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click()).on_hover_text("More");
    let p = ui.painter();
    p.rect_filled(r, 6.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
    glyphs::paint(p, Rect::from_center_size(pos2(r.min.x + 22.0, r.center().y), vec2(20.0, 20.0)), ph::DOTS_THREE, 17.0, look::text());
    chevron(p, pos2(r.max.x - 14.0, r.center().y), look::text());
    resp
}

/// The "…" button's width.
pub const MORE_W: f32 = 62.0;

// ---------------------------------------------------------------------------------------------
// the zooms (sliders and auto zoom)

const BAR_W: (f32, f32) = (8.0, 1200.0);

fn h_value(app: &App) -> f32 {
    ((app.bar_w / BAR_W.0).ln() / (BAR_W.1 / BAR_W.0).ln()).clamp(0.0, 1.0)
}

fn set_h(app: &mut App, v: f32) {
    app.bar_w = BAR_W.0 * (BAR_W.1 / BAR_W.0).powf(v.clamp(0.0, 1.0));
}

const V_RANGE: (f32, f32) = (0.85, 4.0);

fn v_value(app: &App) -> f32 {
    let z = if app.ta.v_zoom > 0.0 { app.ta.v_zoom } else { 1.0 };
    ((z / V_RANGE.0).ln() / (V_RANGE.1 / V_RANGE.0).ln()).clamp(0.0, 1.0)
}

fn set_v(app: &mut App, v: f32) {
    app.ta.v_zoom = V_RANGE.0 * (V_RANGE.1 / V_RANGE.0).powf(v.clamp(0.0, 1.0));
}

/// The vertical zoom that fits `n` default rows into `height` (Vertical Auto Zoom).
pub fn v_fit(n: usize, height: f32) -> f32 {
    if n == 0 {
        return 1.0;
    }
    (height / n as f32 / crate::tracks::ROW_H).clamp(V_RANGE.0, V_RANGE.1)
}

/// The bar width that fits `bars` bars (with one spare) into `width` (Horizontal Auto Zoom).
pub fn h_fit(bars: f32, width: f32) -> f32 {
    (width / (bars.max(4.0) + 1.0)).clamp(BAR_W.0, BAR_W.1)
}

/// Vertical / Horizontal Auto Zoom: refit when tracks come or go (or the area changes size),
/// and when the project's content grows or shrinks (tracks.rs calls this every frame).
pub fn auto_zoom(app: &mut App, lanes_h: f32, lanes_w: f32) {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    if app.fit.v_auto {
        let seen = (n, lanes_h as i32);
        if app.fit.v_seen != seen {
            app.fit.v_seen = seen;
            app.ta.v_zoom = v_fit(n, lanes_h);
        }
    }
    if app.fit.h_auto {
        let end = app.project.as_ref().map_or(0, |p| p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(0));
        if app.fit.h_seen != end {
            app.fit.h_seen = end;
            app.bar_w = h_fit(end as f32 / app.bar_ticks().max(1) as f32, lanes_w);
            app.scroll_x = 0.0;
        }
    }
}

fn toggle_auto(app: &mut App, vertical: bool) {
    if vertical {
        app.fit.v_auto = !app.fit.v_auto;
        app.fit.v_seen = (usize::MAX, 0);
    } else {
        app.fit.h_auto = !app.fit.h_auto;
        app.fit.h_seen = u32::MAX;
    }
}

fn toggle_wave(app: &mut App) {
    if app.keyx.wave_zoom > 1.0 {
        app.fit.wave_level = app.keyx.wave_zoom;
        app.keyx.wave_zoom = 1.0;
    } else {
        app.keyx.wave_zoom = app.fit.wave_level.max(1.4);
    }
}

/// Menu commands with these names (the "…" menu, key commands).
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Horizontal Auto Zoom" => toggle_auto(app, false),
        "Vertical Auto Zoom" => toggle_auto(app, true),
        "Catch" => app.fit.catch = !app.fit.catch,
        "Waveform Zoom" => toggle_wave(app),
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------------------------------------
// the Tracks area's local menu bar

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackCtl {
    Automation,
    Catch,
    Tools,
    Snap,
    Drag,
    WaveZoom,
    AutoZoom,
    VZoom,
    HZoom,
}

/// The right-hand controls in Logic's order, left to right; they leave from the right.
pub const TRACK_RIGHT: [TrackCtl; 9] = [TrackCtl::Automation, TrackCtl::Catch, TrackCtl::Tools, TrackCtl::Snap, TrackCtl::Drag, TrackCtl::WaveZoom, TrackCtl::AutoZoom, TrackCtl::VZoom, TrackCtl::HZoom];

const SLIDER_W: f32 = 75.0;

fn snap_text(app: &App) -> String {
    app.arr.snap.name().to_string()
}

fn drag_text(app: &App) -> String {
    app.arr.drag.name().to_string()
}

/// Each right-hand control's width (with the gap before it).
fn track_slot_w(app: &App, ui: &egui::Ui, c: TrackCtl) -> f32 {
    GAP + match c {
        TrackCtl::Automation | TrackCtl::Catch | TrackCtl::WaveZoom => ICON_W,
        TrackCtl::Tools => 2.0 * 48.0,
        TrackCtl::Snap => ICON_W + 2.0 + text_w(ui, &snap_text(app)) + 32.0,
        TrackCtl::Drag => text_w(ui, &drag_text(app)) + 32.0,
        TrackCtl::AutoZoom => 2.0 * ICON_W,
        TrackCtl::VZoom | TrackCtl::HZoom => SLIDER_W,
    }
}

/// The menus' capsule width (Edit / Functions / View) as Logic's, for tests.
const MENUS_W: f32 = 225.0;
const VIEW_W: f32 = GAP + 62.0;
const FLEX_W: f32 = GAP + ICON_W;

/// What shows in the room after the menus: the view buttons, Flex, then the right-hand
/// controls. The right-hand ones leave first (from the right end), then Flex, then the view
/// buttons (pure: for tests).
pub fn tracks_layout(room: f32, right_w: &[f32]) -> (bool, bool, Vec<bool>) {
    let mut slots = vec![Slot { w: VIEW_W, drop: 1 }, Slot { w: FLEX_W, drop: 1 }];
    slots.extend(right_w.iter().map(|&w| Slot { w, drop: 2 }));
    // (16 pt between the left and right groups)
    let v = fit(&slots, room - 16.0, MORE_W + GAP);
    let mut right = v[2..].to_vec();
    // (Automation and Catch leave with the tool menus, as Logic's bar shows nothing but "…" at
    // the right once the tools have gone: ui-tracks-with-pianoroll.png, a 630 pt Tracks area)
    if !right[2] {
        right[0] = false;
        right[1] = false;
    }
    (v[0], v[1], right)
}

pub fn tracks_bar(app: &mut App, ui: &mut egui::Ui, local: Rect) {
    let id = ui.scope_id().with("tracks-bar");
    let cy = local.center().y;
    let mut x = local.min.x + 9.0;
    // ↑: out of the packed folder (Logic's, dimmed at the top level)
    let inside = crate::trackarea::inside(app).is_some();
    if icon_button(ui, cell(cy, x, ICON_W), id.with("up"), false, inside, "Go Out of Folder", ph::ARROW_UP).clicked() {
        crate::trackarea::command(app, "Go Out of Folder or Region");
    }
    x += ICON_W + 7.0;
    // Edit / Functions / View
    let menus = Rect::from_min_max(pos2(x + 4.0, local.min.y + 3.0), pos2(x + MENUS_W + 40.0, local.max.y - 3.0));
    let mut end = x + MENUS_W;
    ui.scope_builder(egui::UiBuilder::new().max_rect(menus).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "tracks-Edit"), ("Functions", "tracks-Functions"), ("View", "tracks-View")]);
        end = ui.min_rect().max.x - 12.0 + 4.0;
    });
    let widths: Vec<f32> = TRACK_RIGHT.iter().map(|&c| track_slot_w(app, ui, c)).collect();
    let (view_on, flex_on_bar, shown) = tracks_layout(local.max.x - 10.0 - end, &widths);
    x = end + GAP;
    // Live Loops grid | Tracks (one capsule), then Flex
    if view_on {
        let pair = cell(cy, x, 62.0);
        ui.painter().rect_filled(pair, 6.0, look::group_bg());
        let (grid_on, tracks_on) = crate::liveloops::shown(app);
        for (i, (on, tip)) in [(grid_on, "Live Loops Grid (L)"), (tracks_on, "Tracks View")].into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(pair.min.x + i as f32 * 31.0, pair.min.y), vec2(31.0, BTN_H));
            let resp = ui.interact(r, id.with(("view", i)), Sense::click()).on_hover_text(tip);
            let p = ui.painter();
            if on {
                p.rect_filled(r, 6.0, look::accent());
            } else if resp.hovered() {
                p.rect_filled(r, 6.0, look::button_on());
            }
            let glyph = match (i, on) {
                (0, true) => ph_fill::DOTS_NINE,
                (0, false) => ph::DOTS_NINE,
                (_, true) => ph_fill::ROWS,
                _ => ph::ROWS,
            };
            glyphs::paint(p, r, glyph, 16.0, if on { Color32::WHITE } else { look::text() });
            if resp.clicked() {
                crate::liveloops::command(app, if i == 0 { "Show/Hide Live Loops Grid" } else { "Show/Hide Tracks area" });
            }
        }
        x = pair.max.x + GAP;
    }
    if flex_on_bar {
        // (Logic's Flex icon, drawn: cbicons.rs)
        let fr = cell(cy, x, ICON_W);
        let on = app.flex.show;
        if icon_button(ui, fr, id.with("flex"), on, true, "Show/Hide Flex (⌘F)", "").clicked() {
            crate::flex::command(app, ui.ctx(), "Show/Hide Flex Pitch/Time");
        }
        crate::cbicons::flex(ui.painter(), fr, if on { Color32::WHITE } else { look::text() });
    }

    // ---- the right-hand controls, as many as fit (placed from the right)
    let mut rx = local.max.x - 10.0;
    if any_hidden(&shown) || !view_on || !flex_on_bar {
        let mr = cell(cy, rx - MORE_W, MORE_W);
        let resp = more_button(ui, mr, id.with("more"));
        egui::Popup::menu(&resp).show(|ui| more_menu(app, ui, view_on, flex_on_bar, &shown));
        rx = mr.min.x - GAP;
    }
    let mut place = vec![None; TRACK_RIGHT.len()];
    for i in (0..TRACK_RIGHT.len()).rev() {
        if shown[i] {
            let w = widths[i] - GAP;
            place[i] = Some(cell(cy, rx - w, w));
            rx -= widths[i];
        }
    }
    for (i, c) in TRACK_RIGHT.iter().enumerate() {
        let Some(r) = place[i] else { continue };
        track_control(app, ui, id, *c, r);
    }
}

fn track_control(app: &mut App, ui: &mut egui::Ui, id: egui::Id, c: TrackCtl, r: Rect) {
    match c {
        TrackCtl::Automation => {
            if icon_button(ui, r, id.with("auto"), app.show_automation, true, "Show/Hide Automation (A)", br_automation()).clicked() {
                app.command(ui.ctx(), "Show/Hide Automation");
            }
        }
        TrackCtl::Catch => {
            if icon_button(ui, r, id.with("catch"), app.fit.catch, true, "Catch Playhead", ph::ARROW_LINE_RIGHT).clicked() {
                command(app, "Catch");
                app.catch_off = false;
            }
        }
        TrackCtl::Tools => {
            let p = ui.painter().clone();
            p.rect_filled(r, 6.0, look::group_bg());
            for (k, cmd) in [false, true].into_iter().enumerate() {
                let cr = Rect::from_min_size(pos2(r.min.x + k as f32 * 48.0, r.min.y), vec2(48.0, BTN_H));
                let tip = if cmd { "Command-click Tool" } else { "Left-click Tool" };
                let resp = ui.interact(cr, id.with(("tool", k)), Sense::click()).on_hover_text(tip);
                if resp.hovered() {
                    p.rect_filled(cr, 6.0, look::button_on());
                }
                let cur = if cmd { app.cmd_tool } else { app.tool };
                glyphs::paint(&p, Rect::from_center_size(pos2(cr.min.x + 18.0, cr.center().y), vec2(20.0, 20.0)), glyphs::tool(cur), 15.0, look::text());
                chevron(&p, pos2(cr.max.x - 12.0, cr.center().y), look::text());
                if k == 1 {
                    p.vline(cr.min.x, (cr.center().y - 7.0)..=(cr.center().y + 7.0), Stroke::new(1.0, look::line()));
                }
                egui::Popup::menu(&resp).show(|ui| tool_items(app, ui, cmd));
            }
        }
        TrackCtl::Snap => {
            let on_r = Rect::from_min_size(r.min, vec2(ICON_W, BTN_H));
            if icon_button(ui, on_r, id.with("snap-on"), app.snap_on, true, "Snap to Grid (⌘G)", ph::MAGNET).clicked() {
                app.snap_on = !app.snap_on;
            }
            let pr = Rect::from_min_max(pos2(on_r.max.x + 2.0, r.min.y), r.max);
            let resp = text_popup(ui, pr, id.with("snap"), &snap_text(app), "Snap");
            egui::Popup::menu(&resp).show(|ui| crate::arranging::snap_items(app, ui, snap_rest()));
        }
        TrackCtl::Drag => {
            let resp = text_popup(ui, r, id.with("drag"), &drag_text(app), "Drag");
            egui::Popup::menu(&resp).show(|ui| crate::arranging::drag_items(app, ui));
        }
        TrackCtl::WaveZoom => {
            let resp = icon_button(ui, r, id.with("wz"), app.keyx.wave_zoom > 1.0, true, "Waveform Zoom (drag up or down to zoom)", ph::WAVEFORM);
            if resp.dragged() {
                app.keyx.wave_zoom = (app.keyx.wave_zoom * (1.0 - resp.drag_delta().y * 0.02)).clamp(1.0, 16.0);
                if app.keyx.wave_zoom > 1.0 {
                    app.fit.wave_level = app.keyx.wave_zoom;
                }
            } else if resp.clicked() {
                toggle_wave(app);
            }
        }
        TrackCtl::AutoZoom => {
            let a = Rect::from_min_size(r.min, vec2(ICON_W, BTN_H));
            let b = Rect::from_min_size(pos2(r.min.x + ICON_W, r.min.y), vec2(ICON_W, BTN_H));
            ui.painter().rect_filled(r, 6.0, look::group_bg());
            if icon_button(ui, a, id.with("vauto"), app.fit.v_auto, true, "Vertical Auto Zoom", crate::glyphs::tb::ARROW_AUTOFIT_HEIGHT).clicked() {
                toggle_auto(app, true);
            }
            if icon_button(ui, b, id.with("hauto"), app.fit.h_auto, true, "Horizontal Auto Zoom", crate::glyphs::tb::ARROW_AUTOFIT_WIDTH).clicked() {
                toggle_auto(app, false);
            }
        }
        TrackCtl::VZoom => {
            let mut v = v_value(app);
            if slider(ui, r, id.with("vz"), &mut v, ph::ARROWS_OUT_LINE_VERTICAL, "Vertical Zoom") {
                set_v(app, v);
                app.fit.v_auto = false;
            }
        }
        TrackCtl::HZoom => {
            let mut v = h_value(app);
            if slider(ui, r, id.with("hz"), &mut v, ph::ARROWS_OUT_LINE_HORIZONTAL, "Horizontal Zoom") {
                set_h(app, v);
                app.fit.h_auto = false;
            }
        }
    }
}

/// Logic's automation icon (a curve through points).
fn br_automation() -> &'static str {
    ph::LINE_SEGMENTS
}

/// The Snap menu's items after the modes (from the research's "…" menu).
fn snap_rest() -> &'static [crate::menus::Item] {
    crate::menus::local("tracks-more").iter().find(|i| i.label == "Snap").map_or(&[], |s| s.children.as_slice())
}

fn tool_items(app: &mut App, ui: &mut egui::Ui, cmd: bool) {
    let cur = if cmd { app.cmd_tool } else { app.tool };
    for t in crate::edit::Tool::TRACKS {
        if ui.add(egui::Button::selectable(cur == t, t.name()).shortcut_text(t.key())).clicked() {
            if cmd {
                app.cmd_tool = t;
            } else {
                app.tool = t;
            }
            ui.close();
        }
    }
}

/// The "…" menu: the controls that left the bar, in the bar's order, as menu items.
fn more_menu(app: &mut App, ui: &mut egui::Ui, view_on: bool, flex_on: bool, shown: &[bool]) {
    ui.set_min_width(220.0);
    if !view_on {
        let (grid_on, tracks_on) = crate::liveloops::shown(app);
        for (on, label, cmd) in [(grid_on, "Live Loops Grid", "Show/Hide Live Loops Grid"), (tracks_on, "Tracks", "Show/Hide Tracks area")] {
            if ui.add(egui::Button::selectable(on, label)).clicked() {
                crate::liveloops::command(app, cmd);
                ui.close();
            }
        }
    }
    if !flex_on && ui.add(egui::Button::selectable(app.flex.show, "Show Flex")).clicked() {
        crate::flex::command(app, ui.ctx(), "Show/Hide Flex Pitch/Time");
        ui.close();
    }
    if !view_on || !flex_on {
        ui.separator();
    }
    for (i, c) in TRACK_RIGHT.iter().enumerate() {
        if shown[i] {
            continue;
        }
        match c {
            TrackCtl::Automation => {
                if ui.add(egui::Button::selectable(app.show_automation, "Show Automation").shortcut_text("A")).clicked() {
                    app.command(ui.ctx(), "Show/Hide Automation");
                    ui.close();
                }
            }
            TrackCtl::Catch => {
                if ui.add(egui::Button::selectable(app.fit.catch, "Catch Playhead")).clicked() {
                    command(app, "Catch");
                    ui.close();
                }
            }
            TrackCtl::Tools => {
                ui.menu_button("Left-click Tool", |ui| tool_items(app, ui, false));
                ui.menu_button("Command-click Tool", |ui| tool_items(app, ui, true));
            }
            TrackCtl::Snap => {
                ui.menu_button("Snap", |ui| crate::arranging::snap_items(app, ui, snap_rest()));
            }
            TrackCtl::Drag => {
                ui.menu_button("Drag", |ui| crate::arranging::drag_items(app, ui));
            }
            TrackCtl::WaveZoom => {
                ui.menu_button("Waveform Zoom", |ui| {
                    if ui.add(egui::Button::selectable(app.keyx.wave_zoom > 1.0, "Waveform Zoom")).clicked() {
                        toggle_wave(app);
                    }
                    ui.separator();
                    let mut z = app.keyx.wave_zoom;
                    if ui.add(egui::Slider::new(&mut z, 1.0..=16.0).logarithmic(true).show_value(false)).changed() {
                        app.keyx.wave_zoom = z;
                    }
                });
            }
            TrackCtl::AutoZoom => {
                if ui.add(egui::Button::selectable(app.fit.h_auto, "Horizontal Auto Zoom")).clicked() {
                    toggle_auto(app, false);
                    ui.close();
                }
                if ui.add(egui::Button::selectable(app.fit.v_auto, "Vertical Auto Zoom")).clicked() {
                    toggle_auto(app, true);
                    ui.close();
                }
            }
            TrackCtl::VZoom => {
                ui.menu_button("Vertical Zoom", |ui| {
                    let mut v = v_value(app);
                    if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                        set_v(app, v);
                        app.fit.v_auto = false;
                    }
                });
            }
            TrackCtl::HZoom => {
                ui.menu_button("Horizontal Zoom", |ui| {
                    let mut v = h_value(app);
                    if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                        set_h(app, v);
                        app.fit.h_auto = false;
                    }
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the Piano Roll's local menu bar

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RollCtl {
    Inspector,
    MidiDraw,
    Collapse,
    Scale,
    Articulation,
    Tool,
    Snap,
    HZoom,
}

/// The Piano Roll's controls after its menus, left to right; they leave from the right.
pub const ROLL_RIGHT: [RollCtl; 8] = [RollCtl::Collapse, RollCtl::MidiDraw, RollCtl::Scale, RollCtl::Inspector, RollCtl::Articulation, RollCtl::Tool, RollCtl::Snap, RollCtl::HZoom];

fn roll_slot_w(app: &App, ui: &egui::Ui, c: RollCtl) -> f32 {
    GAP + match c {
        RollCtl::Inspector | RollCtl::MidiDraw | RollCtl::Collapse | RollCtl::Scale => ICON_W,
        RollCtl::Articulation => {
            let has = app.selected_region.is_some_and(|(t, _)| app.artic.tracks.contains_key(&t));
            if has { text_w(ui, "Articulation") + 40.0 } else { return 0.0 }
        }
        RollCtl::Tool => 48.0,
        RollCtl::Snap => text_w(ui, &format!("Snap: {}", crate::rollsnap::current_name(ui.ctx()))) + 32.0,
        RollCtl::HZoom => SLIDER_W,
    }
}

/// The Piano Roll's bar: Edit / Functions / View, then its buttons, as many as fit.
pub fn roll_bar(app: &mut App, ui: &mut egui::Ui, local: Rect) {
    let id = ui.scope_id().with("roll-bar");
    let cy = local.center().y;
    let x = local.min.x + 10.0;
    let menus = Rect::from_min_max(pos2(x + 4.0, local.min.y + 3.0), pos2(x + MENUS_W + 40.0, local.max.y - 3.0));
    let mut end = x + MENUS_W;
    ui.scope_builder(egui::UiBuilder::new().max_rect(menus).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "pianoroll-Edit"), ("Functions", "pianoroll-Functions"), ("View", "pianoroll-View")]);
        end = ui.min_rect().max.x - 8.0;
    });
    let widths: Vec<f32> = ROLL_RIGHT.iter().map(|&c| roll_slot_w(app, ui, c)).collect();
    // (the buttons follow the menus; the "…" sits at the right end)
    let avail = local.max.x - 10.0 - end;
    let shown = fit(&widths.iter().map(|&w| Slot { w, drop: 1 }).collect::<Vec<_>>(), avail, MORE_W + GAP);
    let mut bx = end;
    for (i, c) in ROLL_RIGHT.iter().enumerate() {
        if !shown[i] || widths[i] == 0.0 {
            continue;
        }
        let w = widths[i] - GAP;
        let r = cell(cy, bx + GAP, w);
        bx += widths[i];
        roll_control(app, ui, id, *c, r);
    }
    if any_hidden(&shown) {
        let mr = cell(cy, local.max.x - 10.0 - MORE_W, MORE_W);
        let resp = more_button(ui, mr, id.with("more"));
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            for (i, c) in ROLL_RIGHT.iter().enumerate() {
                if !shown[i] && widths[i] > 0.0 {
                    roll_menu_item(app, ui, *c);
                }
            }
        });
    }
}

fn roll_toggle(app: &mut App, c: RollCtl) {
    match c {
        RollCtl::Inspector => app.roll_inspector = !app.roll_inspector,
        RollCtl::MidiDraw => app.midi.velocity_lane = !app.midi.velocity_lane,
        RollCtl::Collapse => app.midi.collapse = !app.midi.collapse,
        RollCtl::Scale => app.midi.scale_guides = !app.midi.scale_guides,
        _ => {}
    }
}

fn roll_on(app: &App, c: RollCtl) -> bool {
    match c {
        RollCtl::Inspector => app.roll_inspector,
        RollCtl::MidiDraw => app.midi.velocity_lane,
        RollCtl::Collapse => app.midi.collapse,
        RollCtl::Scale => app.midi.scale_guides,
        _ => false,
    }
}

fn roll_name(c: RollCtl) -> &'static str {
    match c {
        RollCtl::Inspector => "Show/Hide Inspector",
        RollCtl::MidiDraw => "Show/Hide MIDI Draw",
        RollCtl::Collapse => "Collapse Mode",
        RollCtl::Scale => "Show Scale Guides",
        RollCtl::Articulation => "Articulation",
        RollCtl::Tool => "Left-click Tool",
        RollCtl::Snap => "Snap",
        RollCtl::HZoom => "Horizontal Zoom",
    }
}

fn roll_tools(app: &mut App, ui: &mut egui::Ui) {
    for t in crate::edit::Tool::PIANO_ROLL {
        if ui.add(egui::Button::selectable(app.roll_tool == t, t.name()).shortcut_text(t.key())).clicked() {
            app.roll_tool = t;
            ui.close();
        }
    }
}

fn roll_zoom_v(app: &App) -> f32 {
    ((app.roll_bar_w / 30.0).ln() / 100.0f32.ln()).clamp(0.0, 1.0)
}

fn roll_control(app: &mut App, ui: &mut egui::Ui, id: egui::Id, c: RollCtl, r: Rect) {
    match c {
        RollCtl::Inspector | RollCtl::MidiDraw | RollCtl::Collapse | RollCtl::Scale => {
            let icon = match c {
                RollCtl::Inspector => ph::SIDEBAR_SIMPLE,
                RollCtl::MidiDraw => ph::CHART_BAR,
                RollCtl::Collapse => ph::ARROWS_IN_LINE_VERTICAL,
                _ => ph::PIANO_KEYS,
            };
            if icon_button(ui, r, id.with(roll_name(c)), roll_on(app, c), true, roll_name(c), icon).clicked() {
                roll_toggle(app, c);
            }
        }
        RollCtl::Articulation | RollCtl::Snap => {
            ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                ui.visuals_mut().widgets.inactive.weak_bg_fill = look::group_bg();
                ui.visuals_mut().widgets.inactive.corner_radius = egui::CornerRadius::same(6);
                ui.spacing_mut().button_padding = vec2(10.0, 4.0);
                if c == RollCtl::Snap {
                    crate::rollsnap::popup(ui);
                } else {
                    crate::artic::roll_popup(app, ui);
                }
            });
        }
        RollCtl::Tool => {
            let resp = ui.interact(r, id.with("tool"), Sense::click()).on_hover_text("Left-click Tool");
            let p = ui.painter();
            p.rect_filled(r, 6.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
            glyphs::paint(p, Rect::from_center_size(pos2(r.min.x + 18.0, r.center().y), vec2(20.0, 20.0)), glyphs::tool(app.roll_tool), 15.0, look::text());
            chevron(p, pos2(r.max.x - 12.0, r.center().y), look::text());
            egui::Popup::menu(&resp).show(|ui| roll_tools(app, ui));
        }
        RollCtl::HZoom => {
            let mut v = roll_zoom_v(app);
            if slider(ui, r, id.with("hz"), &mut v, ph::ARROWS_OUT_LINE_HORIZONTAL, "Horizontal Zoom") {
                app.roll_bar_w = 30.0 * 100.0f32.powf(v);
            }
        }
    }
}

fn roll_menu_item(app: &mut App, ui: &mut egui::Ui, c: RollCtl) {
    match c {
        RollCtl::Inspector | RollCtl::MidiDraw | RollCtl::Collapse | RollCtl::Scale => {
            if ui.add(egui::Button::selectable(roll_on(app, c), roll_name(c))).clicked() {
                roll_toggle(app, c);
                ui.close();
            }
        }
        RollCtl::Articulation => crate::artic::roll_popup(app, ui),
        RollCtl::Snap => crate::rollsnap::popup(ui),
        RollCtl::Tool => {
            ui.menu_button("Left-click Tool", |ui| roll_tools(app, ui));
        }
        RollCtl::HZoom => {
            ui.menu_button("Horizontal Zoom", |ui| {
                let mut v = roll_zoom_v(app);
                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                    app.roll_bar_w = 30.0 * 100.0f32.powf(v);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(w: f32) -> Slot {
        Slot { w, drop: 1 }
    }

    #[test]
    fn everything_shows_when_it_fits() {
        let v = fit(&[s(40.0), s(40.0), s(100.0)], 180.0, 60.0);
        assert_eq!(v, vec![true, true, true]);
        assert!(!any_hidden(&v));
    }

    #[test]
    fn the_right_end_leaves_first_and_room_is_kept_for_more() {
        // 180 needed, 170 there: the last one leaves, and the "…" (60) needs room too
        let v = fit(&[s(40.0), s(40.0), s(100.0)], 170.0, 60.0);
        assert_eq!(v, vec![true, true, false]);
        // 100 + 60 > 140 room: two leave
        let v = fit(&[s(40.0), s(40.0), s(100.0)], 120.0, 60.0);
        assert_eq!(v, vec![true, false, false]);
    }

    #[test]
    fn drop_rank_beats_position_and_zero_never_leaves() {
        let slots = [Slot { w: 50.0, drop: 3 }, Slot { w: 50.0, drop: 0 }, Slot { w: 50.0, drop: 1 }];
        let v = fit(&slots, 120.0, 10.0);
        assert_eq!(v, vec![false, true, true]);
        let v = fit(&slots, 10.0, 10.0);
        assert_eq!(v, vec![false, true, false]);
    }

    #[test]
    fn tracks_bar_full_set_at_a_wide_window_and_leaves_in_order() {
        let w = [44.0, 44.0, 104.0, 100.0, 120.0, 44.0, 80.0, 83.0, 83.0];
        let full: f32 = w.iter().sum::<f32>() + VIEW_W + FLEX_W + 16.0;
        let (v, f, r) = tracks_layout(full + 1.0, &w);
        assert!(v && f && r.iter().all(|s| *s));
        // a little narrower: the horizontal zoom slider leaves first, then the vertical one
        let (v, f, r) = tracks_layout(full - 20.0, &w);
        assert!(v && f);
        assert_eq!(r, vec![true, true, true, true, true, true, true, false, false]);
        // narrower still: Snap and Drag before the tools, the tools before Catch / Automation
        let (_, _, r) = tracks_layout(VIEW_W + FLEX_W + 16.0 + 44.0 + 44.0 + 104.0 + MORE_W + GAP, &w);
        assert_eq!(r, vec![true, true, true, false, false, false, false, false, false]);
        // the tools gone: Automation and Catch go with them (Logic's 630 pt bar)
        let (_, _, r) = tracks_layout(VIEW_W + FLEX_W + 16.0 + 44.0 + 44.0 + 50.0 + MORE_W + GAP, &w);
        assert!(r.iter().all(|s| !*s));
        // only the view buttons and Flex: everything on the right is in "…"
        let (v, f, r) = tracks_layout(VIEW_W + FLEX_W + 16.0 + MORE_W + GAP, &w);
        assert!(v && f && r.iter().all(|s| !*s));
        // a Tracks area at Logic's minimum: Flex, then the view buttons leave too
        let (v, f, _) = tracks_layout(VIEW_W + 16.0 + MORE_W + GAP, &w);
        assert!(v && !f);
        let (v, f, _) = tracks_layout(40.0, &w);
        assert!(!v && !f);
    }

    #[test]
    fn auto_zoom_fits() {
        // 10 rows of 64 pt into 320 pt: the smallest zoom allowed
        assert_eq!(v_fit(10, 320.0), V_RANGE.0);
        assert!((v_fit(2, 256.0) - 2.0).abs() < 1e-4);
        // 15 bars of content (+1 spare) into 800 pt
        assert!((h_fit(15.0, 800.0) - 50.0).abs() < 1e-4);
    }
}
