//! The Region inspector as Logic 13's (General / Quantize / Fades / Details groups with More
//! areas, a name field, colour swatch, value boxes with units and pop-up arrows, Position /
//! Length and Lock Time Position), editing every selected region (relative; Option-Shift:
//! absolute; an asterisk where they differ), Audio / MIDI Defaults with nothing selected, and
//! the Region Inspector Float. Also the region edits that sit with it: Cut / Insert Time
//! (sections between the locators), Length Double / Halve / Change…, Repeat Multiple…, Paste at
//! Original Position, Split's Keep / Shorten / Split dialog, Color Regions by Track Color and
//! Lock / Unlock SMPTE Position. The parameters as played are lg-core regparams.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_audio::fade::FadeType;
use lg_core::project::{RegionProps, GATE_LEGATO};
use lg_core::regparams as rp;
use lg_core::TrackKind;

use crate::app::App;
use crate::look;

/// Logic command names this file runs.
pub const COMMANDS: &[&str] = &[
    "Cut Section Between Locators (Global)",
    "Cut Section Between Locators (Selection)",
    "Cut Section Between Locators",
    "Copy Section Between Locators (Global)",
    "Copy Section Between Locators (Selection)",
    "Copy Section Between Locators",
    "Insert Section at Playhead (Global)",
    "Insert Section at Playhead (Selection)",
    "Insert Section at Playhead",
    "Insert Silence Between Locators (Global)",
    "Insert Silence Between Locators (Selection)",
    "Insert Silence at Locators",
    "Repeat Section Between Locators (Global)",
    "Repeat Section Between Locators (Selection)",
    "Repeat Section Between Locators",
    "Delete Section Between Locators (Global)",
    "Delete Section Between Locators (Selection)",
    "Double Region/Cell/Event/Chord Length",
    "Halve Region/Cell/Event/Chord Length",
    "Change Region/Cell/Event Length…",
    "Double",
    "Halve",
    "Change…",
    "Repeat Regions/Events Multiple Times…",
    "Multiple…",
    "Paste at Original Position",
    "Color Regions/Cells by Track Color",
    "Region Inspector Float",
    "Lock SMPTE Position",
    "Unlock SMPTE Position",
    "Lock/Unlock SMPTE Position",
    "SMPTE-Locked Regions",
    "Gain Tool",
    "Set Gain Tool",
    "Toggle Region Solo",
    "Marquee Ruler",
    "Split Regions at Marquee Selection",
    "Set Locators by Selection and Enable Cycle",
];

pub const ROW: f32 = 29.0;
/// Labels and values (Logic 13's inspector, measured: 11 pt).
const FONT: f32 = 11.0;

/// What Split does with notes across the cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitNotes {
    Keep,
    Shorten,
    Split,
}

#[derive(Default)]
pub struct State {
    /// Cut / Copy Section's clipboard.
    pub section: Option<rp::Section>,
    /// Groups closed (General, Quantize, Fades, Details) and the More areas open (General's,
    /// Quantize's).
    pub closed: [bool; 4],
    pub more: [bool; 2],
    /// The name field being typed.
    pub naming: Option<String>,
    /// MIDI and Audio Defaults (nothing selected): new recorded regions take them.
    pub defaults_midi: Option<RegionProps>,
    pub defaults_audio: Option<RegionProps>,
    /// Region Inspector Float shown; its height last frame.
    pub float: bool,
    float_h: f32,
    /// Split's dialog: the cuts waiting for Keep / Shorten / Split; the answer for the rerun.
    pub split_ask: Option<Vec<u32>>,
    pub split_mode: Option<SplitNotes>,
    /// Change Length… (factor), Repeat Multiple… (copies, adjustment, as aliases).
    pub length_sheet: Option<f32>,
    pub repeat_sheet: Option<(u32, usize, bool)>,
    /// A "(Selection)" section edit waiting for "move global events too?".
    pub ask_global: Option<String>,
    /// Limit Dragging to One Direction: the drag's axis once chosen (true: horizontal).
    pub axis: Option<bool>,
    /// Region Solo on: the soloed regions (only they play).
    pub region_solo: bool,
    pub soloed: Vec<(usize, usize)>,
    /// A Gain tool drag: the regions and their gains when it began, the dB moved.
    gain: Option<(Vec<(usize, usize, f32)>, f32)>,
    /// View ▸ Marquee Ruler shown; its drag's start tick.
    pub marquee_ruler: bool,
    ruler_from: Option<u32>,
    /// Drawing in the Region Inspector Float (its header has no disclosure).
    in_float: bool,
}

// ---------------------------------------------------------------- the parameters

#[derive(Clone, Copy, Debug, PartialEq)]
enum P {
    Gain,
    Delay,
    Transpose,
    FineTune,
    Velocity,
    Dynamics,
    Gate,
    QSwing,
    QStrength,
    QRange,
    QFlam,
    FadeIn,
    InCurve,
    FadeOut,
    OutCurve,
}

/// (min, max, step) of a parameter (Delay in ticks).
fn range(p: P) -> (f32, f32, f32) {
    match p {
        P::Gain => (-96.0, 30.0, 0.1),
        P::Delay => (-3840.0, 3840.0, 1.0),
        P::Transpose => (-96.0, 96.0, 1.0),
        P::FineTune => (-50.0, 50.0, 1.0),
        P::Velocity => (-99.0, 99.0, 1.0),
        P::Dynamics => (0.0, 300.0, 1.0),
        P::Gate => (0.0, 201.0, 1.0),
        P::QSwing | P::QStrength => (0.0, 100.0, 1.0),
        P::QRange => (-960.0, 960.0, 1.0),
        P::QFlam => (-480.0, 480.0, 1.0),
        P::FadeIn | P::FadeOut => (0.0, 60000.0, 1.0),
        P::InCurve | P::OutCurve => (-99.0, 99.0, 1.0),
    }
}

/// What the inspector edits: a region, or the defaults for new MIDI / audio regions.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tg {
    Region(usize, usize),
    Defaults(bool),
}

fn props(app: &App, tg: Tg) -> Option<RegionProps> {
    match tg {
        Tg::Region(t, r) => app.project.as_ref()?.tracks.get(t)?.regions.get(r).map(|x| x.props),
        Tg::Defaults(a) => Some(if a { app.rp.defaults_audio } else { app.rp.defaults_midi }.unwrap_or_default()),
    }
}

fn with_props(app: &mut App, tg: Tg, f: impl FnOnce(&mut RegionProps)) {
    match tg {
        Tg::Region(t, r) => {
            if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                f(&mut x.props);
            }
        }
        Tg::Defaults(a) => {
            let slot = if a { &mut app.rp.defaults_audio } else { &mut app.rp.defaults_midi };
            let mut v = slot.unwrap_or_default();
            f(&mut v);
            *slot = Some(v);
        }
    }
}

fn is_audio(app: &App, tg: Tg) -> bool {
    match tg {
        Tg::Region(t, r) => app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|x| x.audio.is_some()),
        Tg::Defaults(a) => a,
    }
}

fn get(app: &App, tg: Tg, p: P) -> f32 {
    let pr = props(app, tg).unwrap_or_default();
    let x = match tg {
        Tg::Region(t, r) if is_audio(app, tg) => Some(crate::audioedit::extras(app, t, r)),
        _ => None,
    };
    let fades = x.as_ref().map(|x| x.fades).unwrap_or_default();
    match p {
        P::Gain => x.map_or(0.0, |x| x.gain_db),
        P::Delay => pr.delay as f32,
        P::Transpose => pr.transpose as f32,
        P::FineTune => pr.fine_tune as f32,
        P::Velocity => pr.velocity as f32,
        P::Dynamics => pr.dynamics as f32,
        P::Gate => if pr.gate_time == GATE_LEGATO { 201.0 } else { pr.gate_time as f32 },
        P::QSwing => pr.swing as f32,
        P::QStrength => pr.q_strength as f32,
        P::QRange => pr.q_range as f32,
        P::QFlam => pr.q_flam as f32,
        P::FadeIn => fades.in_ms as f32,
        P::InCurve => fades.in_curve as f32,
        P::FadeOut => fades.out_ms as f32,
        P::OutCurve => fades.out_curve as f32,
    }
}

fn put(app: &mut App, tg: Tg, p: P, v: f32) {
    let (lo, hi, _) = range(p);
    let v = v.clamp(lo, hi);
    match p {
        P::Gain | P::FadeIn | P::InCurve | P::FadeOut | P::OutCurve => {
            let Tg::Region(t, r) = tg else { return };
            if !is_audio(app, tg) {
                return;
            }
            let mut x = crate::audioedit::extras(app, t, r);
            match p {
                P::Gain => x.gain_db = (v * 10.0).round() / 10.0,
                P::FadeIn => x.fades.in_ms = v as u16,
                P::InCurve => x.fades.in_curve = v as i8,
                P::FadeOut => x.fades.out_ms = v as u16,
                _ => x.fades.out_curve = v as i8,
            }
            crate::audioedit::set_extras(app, t, r, x);
        }
        _ => with_props(app, tg, |pr| match p {
            P::Delay => pr.delay = v.round() as i32,
            P::Transpose => pr.transpose = v as i8,
            P::FineTune => pr.fine_tune = v as i8,
            P::Velocity => pr.velocity = v as i8,
            P::Dynamics => pr.dynamics = v as u16,
            P::Gate => pr.gate_time = if v >= 201.0 { GATE_LEGATO } else { v as u16 },
            P::QSwing => pr.swing = v as u8,
            P::QStrength => pr.q_strength = v as u8,
            P::QRange => pr.q_range = v as i16,
            _ => pr.q_flam = v as i16,
        }),
    }
}

/// The regions an inspector edit changes: the selection (the inspector's region first).
fn targets(app: &App, tg: Tg) -> Vec<Tg> {
    match tg {
        Tg::Defaults(_) => vec![tg],
        Tg::Region(t, r) => {
            let mut v = vec![tg];
            v.extend(app.region_selection().into_iter().filter(|&x| x != (t, r)).map(|(t, r)| Tg::Region(t, r)));
            v
        }
    }
}

/// Changing `p` from `old` to `new` on every selected region: the differences kept (Logic),
/// or with Option-Shift every region set to `new`.
fn change(app: &mut App, tg: Tg, p: P, old: f32, new: f32, absolute: bool) {
    let all = targets(app, tg);
    crate::audioedit::checkpoint(app, "Region Parameter");
    for x in all {
        let v = if absolute || x == tg { new } else { get(app, x, p) + (new - old) };
        put(app, x, p, v);
    }
    app.after_edit();
}

fn differs(app: &App, tg: Tg, f: &dyn Fn(&App, Tg) -> String) -> bool {
    let a = f(app, tg);
    targets(app, tg).into_iter().any(|x| f(app, x) != a)
}

fn ms_per_tick(app: &App) -> f32 {
    (60_000.0 / app.tempo().max(1.0) / lg_core::events::PPQ as f64) as f32
}

// ---------------------------------------------------------------- drawing

/// Logic 13's inspector card (measured on Apple's screenshot: window 39, card 28, rows split
/// by a 42 hairline, value boxes 20, check boxes 50 grey).
fn card_bg() -> Color32 {
    look::mix(look::pane_bg(), Color32::BLACK, 0.25)
}

fn row_line(p: &egui::Painter, x: egui::Rangef, y: f32) {
    p.hline(x, y, Stroke::new(1.0, look::mix(card_bg(), Color32::WHITE, 0.06)));
}

fn label(p: &egui::Painter, area: Rect, y: f32, indent: bool, text: &str) {
    p.text(pos2(area.min.x + if indent { 18.0 } else { 7.0 }, y + ROW / 2.0), Align2::LEFT_CENTER, text, look::regular(FONT), look::text());
}

/// A value box (Logic 13's dark rounded field: the number, its unit dimmed), with an optional
/// up / down pop-up arrow beside it. Returns the new value when dragged, typed or picked.
#[allow(clippy::too_many_arguments)]
fn value_box(ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, v: f32, p: P, unit: &str, shown: &dyn Fn(f32) -> String, menu: &[(String, f32)], star: bool, scale: f32) -> Option<f32> {
    let has_menu = !menu.is_empty();
    let right = area.max.x - if has_menu { 26.0 } else { 5.0 };
    let w = if unit.len() > 2 { 82.0 } else { 64.0 };
    let b = Rect::from_min_max(pos2(right - w, y + 4.5), pos2(right, y + ROW - 4.5));
    let pnt = ui.painter().clone();
    pnt.rect_filled(b, 5.0, look::lcd_bg());
    let uw = if unit.is_empty() { 0.0 } else { pnt.layout_no_wrap(unit.to_string(), look::regular(FONT), look::text_dim()).size().x + 4.0 };
    if !unit.is_empty() {
        pnt.text(pos2(b.max.x - 6.0, b.center().y), Align2::RIGHT_CENTER, unit, look::regular(FONT), look::text_dim());
    }
    if star {
        pnt.text(pos2(b.min.x - 6.0, b.center().y), Align2::RIGHT_CENTER, "*", look::regular(FONT), look::text_dim());
    }
    let (lo, hi, step) = range(p);
    let mut val = v * scale;
    let num = Rect::from_min_max(b.min, pos2(b.max.x - uw - 2.0, b.max.y));
    let mut out = None;
    if crate::widgets::value_slot(ui, num, id, &mut val, lo * scale, hi * scale, if p == P::Dynamics || p == P::Gate || p == P::QStrength { 100.0 * scale } else if p == P::QSwing { 50.0 } else { 0.0 }, step * if scale != 1.0 { 1.0 } else { 1.0 }, shown) {
        out = Some(val / scale);
    }
    if has_menu {
        let cr = Rect::from_center_size(pos2(area.max.x - 14.0, b.center().y), vec2(16.0, 20.0));
        let resp = ui.interact(cr, id.with("menu"), Sense::click());
        crate::glyphs::paint(&pnt, cr, crate::glyphs::ph::CARET_UP_DOWN, 12.0, if resp.hovered() { look::text() } else { look::text_dim() });
        egui::Popup::menu(&resp).show(|ui| {
            for (name, x) in menu {
                if ui.button(name).clicked() {
                    out = Some(*x);
                    ui.close();
                }
            }
        });
    }
    out
}

/// A group header ("General", "Quantize"…): dim, with the chevron; true when clicked.
fn group_header(ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, name: &str, open: bool, indent: bool) -> bool {
    let r = Rect::from_min_max(pos2(area.min.x, y), pos2(area.max.x, y + ROW));
    let resp = ui.interact(r, id, Sense::click());
    let p = ui.painter();
    p.text(pos2(area.min.x + if indent { 18.0 } else { 7.0 }, r.center().y), Align2::LEFT_CENTER, name, look::regular(FONT), look::text_dim());
    let cr = Rect::from_center_size(pos2(area.max.x - 14.0, r.center().y), vec2(14.0, 14.0));
    crate::glyphs::paint(p, cr, if open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 12.0, if resp.hovered() { look::text() } else { look::text_dim() });
    resp.clicked()
}

fn check_row(app: &mut App, ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, tg: Tg, indent: bool, name: &str, get: fn(&App, Tg) -> bool, set: fn(&mut App, Tg, bool)) {
    label(ui.painter(), area, y, indent, name);
    let mut on = get(app, tg);
    let star = differs(app, tg, &|a, x| get(a, x).to_string());
    if star {
        ui.painter().text(pos2(area.max.x - 24.0, y + ROW / 2.0), Align2::RIGHT_CENTER, "*", look::regular(FONT), look::text_dim());
    }
    // Logic 13's box: a rounded grey square, blue with a white tick when on
    let r = Rect::from_min_max(pos2(area.max.x - 60.0, y + 2.0), pos2(area.max.x - 3.0, y + ROW - 2.0));
    let resp = ui.interact(r, id, Sense::click());
    let b = Rect::from_center_size(pos2(area.max.x - 12.0, r.center().y), vec2(14.0, 14.0));
    let pnt = ui.painter();
    pnt.rect_filled(b, 4.0, if on { Color32::from_rgb(0x3d, 0x8b, 0xf7) } else { look::mix(look::pane_bg(), Color32::WHITE, if resp.hovered() { 0.14 } else { 0.08 }) });
    if on {
        pnt.line_segment([b.center() + vec2(-3.5, 0.0), b.center() + vec2(-1.0, 3.0)], Stroke::new(1.8, Color32::WHITE));
        pnt.line_segment([b.center() + vec2(-1.0, 3.0), b.center() + vec2(4.0, -3.5)], Stroke::new(1.8, Color32::WHITE));
    }
    if resp.clicked() {
        on = !on;
        crate::audioedit::checkpoint(app, "Region Parameter");
        for x in targets(app, tg) {
            set(app, x, on);
        }
        app.after_edit();
    }
}

#[allow(clippy::too_many_arguments)]
fn num_row(app: &mut App, ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, tg: Tg, indent: bool, name: &str, p: P, unit: &str, shown: &dyn Fn(f32) -> String, menu: &[(String, f32)], scale: f32) {
    label(ui.painter(), area, y, indent, name);
    let v = get(app, tg, p);
    let star = differs(app, tg, &|a, x| format!("{}", get(a, x, p)));
    if let Some(nv) = value_box(ui, area, y, id, v, p, unit, shown, menu, star, scale) {
        let abs = ui.input(|i| i.modifiers.alt && i.modifiers.shift);
        change(app, tg, p, v, nv, abs);
    }
}

fn choice_row(app: &mut App, ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, tg: Tg, indent: bool, name: &str, cur: usize, names: &[&str]) -> Option<usize> {
    label(ui.painter(), area, y, indent, name);
    let r = Rect::from_min_max(pos2(area.max.x - 150.0, y + 3.0), pos2(area.max.x - 3.0, y + ROW - 3.0));
    let _ = (app, tg);
    crate::widgets::choice_slot(ui, r, id, cur, names)
}

fn set_choice(app: &mut App, tg: Tg, f: impl Fn(&mut App, Tg)) {
    crate::audioedit::checkpoint(app, "Region Parameter");
    for x in targets(app, tg) {
        f(app, x);
    }
    app.after_edit();
}

fn signed(v: f32) -> String {
    format!("{:.0}", v)
}

/// Position / Length: bar, beat, division, tick boxes.
fn bbdt_row(app: &mut App, ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, t: usize, r: usize, length: bool) {
    label(ui.painter(), area, y, false, if length { "Length" } else { "Position" });
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|x| (x.start, x.length)) else { return };
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let beat = lg_core::timeline::Map::beat_len(sig).max(1);
    let bar = beat * sig.0.max(1) as u32;
    let div = 240u32.min(beat);
    let v = if length { rg.1 } else { rg.0 };
    let base = if length { 0 } else { 1 };
    let parts = [v / bar + base, v % bar / beat + base, v % beat / div + base, v % div + base];
    let w = 4.0 * 34.0;
    let b = Rect::from_min_max(pos2(area.max.x - 5.0 - w, y + 4.5), pos2(area.max.x - 5.0, y + ROW - 4.5));
    ui.painter().rect_filled(b, 5.0, look::lcd_bg());
    let mut newv = None;
    for (k, part) in parts.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(b.min.x + k as f32 * 34.0, b.min.y), vec2(34.0, b.height()));
        let mut x = *part as f32;
        let max = [9999.0, sig.0 as f32, (beat / div) as f32, div as f32][k] + base as f32 - 1.0;
        let max = if k == 0 { 9999.0 } else { max };
        if crate::widgets::value_slot(ui, cell.shrink2(vec2(2.0, 0.0)), id.with(k), &mut x, base as f32 - if k == 0 { 0.0 } else { 1.0 }, max + 1.0, *part as f32, 1.0, &|v| format!("{v:.0}")) {
            let d = (x as i64 - *part as i64) * [bar, beat, div, 1][k] as i64;
            newv = Some((v as i64 + d).max(if length { 1 } else { 0 }) as u32);
        }
    }
    if let Some(nv) = newv {
        crate::audioedit::checkpoint(app, if length { "Region Length" } else { "Move Region" });
        let d = nv as i64 - v as i64;
        for (t, r) in sel_or(app, t, r) {
            let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) else { continue };
            if length {
                rp::set_length(x, (x.length as i64 + d).max(1) as u32, false);
            } else if !x.props.locked {
                let to = (x.start as i64 + d).max(0);
                let dd = to - x.start as i64;
                x.start = to as u32;
                for n in &mut x.notes {
                    n.pos = (n.pos as i64 + dd).max(0) as u32;
                }
            }
        }
        app.after_edit();
    }
}

fn sel_or(app: &App, t: usize, r: usize) -> Vec<(usize, usize)> {
    let mut v = app.region_selection();
    if !v.contains(&(t, r)) {
        v.push((t, r));
    }
    v
}

fn color_of(k: u8) -> Color32 {
    let c = lg_core::trackmix::palette(k);
    Color32::from_rgb(c[0], c[1], c[2])
}

/// A region's colour in the Tracks area: its own, else its track's channel strip colour, else
/// the track type's; with Settings ▸ View ▸ Region Color "As Track Color" always the track's.
pub fn region_color(app: &App, region: &lg_core::Region, kind: TrackKind) -> Color32 {
    let track = app.project.as_ref().and_then(|p| p.tracks.iter().enumerate().find(|(_, t)| t.regions.iter().any(|r| std::ptr::eq(r, region))).map(|(i, _)| i));
    let strip = track.and_then(|t| crate::tm::set(app, t)).and_then(|s| s.strip.color);
    let as_track = app.logic_settings.get("View / Tracks / Region Color:") == "As Track Color";
    match (region.props.color.filter(|_| !as_track), strip) {
        (Some(k), _) | (None, Some(k)) => color_of(k),
        _ => look::region_color(kind),
    }
}

fn color_row(app: &mut App, ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, t: usize, r: usize) {
    label(ui.painter(), area, y, false, "Color");
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
    let kind = app.project.as_ref().map_or(TrackKind::Instrument, |p| p.tracks[t].kind);
    let b = Rect::from_min_max(pos2(area.max.x - 5.0 - 64.0, y + 4.0), pos2(area.max.x - 5.0, y + ROW - 4.0));
    let resp = ui.interact(b, id, Sense::click());
    ui.painter().rect_filled(b, 5.0, region_color(app, &rg, kind));
    if resp.hovered() {
        ui.painter().rect_stroke(b, 5.0, Stroke::new(1.0, look::text_dim()), egui::StrokeKind::Inside);
    }
    let mut pick: Option<Option<u8>> = None;
    egui::Popup::menu(&resp).show(|ui| {
        let (cols, rows) = (lg_core::trackmix::PALETTE_COLS, lg_core::trackmix::PALETTE_ROWS);
        let cell = 18.0;
        let (gr, _) = ui.allocate_exact_size(vec2(cols as f32 * cell, rows as f32 * cell), Sense::hover());
        for k in 0..(cols * rows) as u8 {
            let cr = Rect::from_min_size(gr.min + vec2((k as usize % cols) as f32 * cell, (k as usize / cols) as f32 * cell), vec2(cell, cell)).shrink(1.5);
            ui.painter().rect_filled(cr, 3.0, color_of(k));
            if rg.props.color == Some(k) {
                ui.painter().rect_stroke(cr.expand(1.0), 4.0, Stroke::new(2.0, look::text()), egui::StrokeKind::Outside);
            }
            if ui.interact(cr, ui.scope_id().with(("rpal", k)), Sense::click()).clicked() {
                pick = Some(Some(k));
                ui.close();
            }
        }
        if ui.button("Track Color").clicked() {
            pick = Some(None);
            ui.close();
        }
    });
    if let Some(c) = pick {
        color_regions(app, &sel_or(app, t, r), c);
    }
}

/// The regions coloured (None: back to the track's colour). Colouring every region of one
/// track, with nothing else selected, also colours the track (Logic).
pub fn color_regions(app: &mut App, sel: &[(usize, usize)], c: Option<u8>) {
    app.checkpoint("Assign Region Color");
    for &(t, r) in sel {
        if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            x.props.color = c;
        }
    }
    let tracks: std::collections::BTreeSet<usize> = sel.iter().map(|x| x.0).collect();
    if let (Some(c), Some(&t)) = (c, tracks.iter().next()) {
        let n = app.project.as_ref().map_or(0, |p| p.tracks[t].regions.len());
        if tracks.len() == 1 && n > 0 && sel.len() == n {
            if let Some(s) = crate::tm::set_mut(app, t) {
                s.strip.color = Some(c);
            }
        }
    }
    app.after_edit();
}

/// The inspector's Region part: header (name field, count, or MIDI / Audio Defaults) and,
/// when open, its groups. Returns the y under it.
pub fn inspector(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32) -> f32 {
    let sel = app.region_selection();
    let tg = match app.selected_region.filter(|x| app.project.as_ref().is_some_and(|p| p.tracks.get(x.0).is_some_and(|t| x.1 < t.regions.len()))) {
        Some((t, r)) => Tg::Region(t, r),
        None => Tg::Defaults(app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).is_some_and(|t| t.kind == TrackKind::Audio)),
    };
    let hr = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), 32.0));
    let (lab, val) = match tg {
        Tg::Defaults(a) => (if a { "Audio Defaults" } else { "MIDI Defaults" }, String::new()),
        Tg::Region(t, r) if sel.len() > 1 => ("Region:", { let _ = (t, r); format!("{} selected", sel.len()) }),
        Tg::Region(t, r) => ("Region:", app.project.as_ref().map(|p| p.tracks[t].regions[r].name.clone()).unwrap_or_default()),
    };
    let id = ui.scope_id().with("rp-insp");
    // the name: a click types a new one
    let lw = ui.painter().layout_no_wrap(lab.to_string(), look::bold(13.0), look::text()).size().x;
    let name_r = Rect::from_min_max(pos2(hr.min.x + 14.0 + lw, hr.min.y + 4.0), pos2(hr.max.x - 30.0, hr.max.y - 4.0));
    if let (Tg::Region(t, r), Some(text)) = (tg, app.rp.naming.as_mut()) {
        let te = ui.put(name_r, egui::TextEdit::singleline(text).font(look::regular(13.0)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            let name = app.rp.naming.take().unwrap_or_default();
            app.checkpoint("Rename Region");
            for (t, r) in sel_or(app, t, r) {
                if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    x.name = name.clone();
                }
            }
            app.after_edit();
        }
        y += 33.0;
    } else if app.rp.in_float {
        // the float's title: "Region: name", no disclosure (a click on the name renames)
        let p = ui.painter();
        p.text(pos2(hr.min.x + 13.0, hr.center().y), Align2::LEFT_CENTER, lab, look::bold(13.0), look::text());
        p.text(pos2(hr.min.x + 17.0 + lw, hr.center().y), Align2::LEFT_CENTER, &val, look::regular(13.0), look::text_dim());
        if ui.interact(name_r, id.with("fname"), Sense::click()).clicked() && matches!(tg, Tg::Region(..)) && sel.len() <= 1 {
            app.rp.naming = Some(val.clone());
        }
        y += 33.0;
    } else {
        if crate::widgets::disclosure(ui, hr, id.with("hdr"), lab, &val, app.inspector_region_open) {
            let over_name = ui.ctx().pointer_latest_pos().is_some_and(|p| name_r.contains(p) && p.x < name_r.min.x + 6.0 + ui.painter().layout_no_wrap(val.clone(), look::regular(13.0), look::text()).size().x);
            if over_name && matches!(tg, Tg::Region(..)) && sel.len() <= 1 {
                app.rp.naming = Some(val.clone());
            } else {
                app.inspector_region_open = !app.inspector_region_open;
            }
        }
        y += 33.0;
    }
    if !app.inspector_region_open {
        return y;
    }
    y = body(app, ui, area, y + 4.0, tg, id);
    y
}

/// The groups, in a rounded card as Logic 13's.
fn body(app: &mut App, ui: &mut egui::Ui, area: Rect, y0: f32, tg: Tg, id: egui::Id) -> f32 {
    let card = Rect::from_min_max(pos2(area.min.x + 8.0, y0), pos2(area.max.x - 8.0, y0 + app.rp.float_h.max(ROW)));
    let inner = card;
    let bg_layer = ui.painter().clone();
    let bg_idx = bg_layer.add(egui::Shape::Noop);
    let audio = is_audio(app, tg);
    let region = match tg {
        Tg::Region(t, r) => Some((t, r)),
        _ => None,
    };
    let x = inner.x_range();
    let mut y = y0;
    let p = ui.painter().clone();
    // ---- General
    if group_header(ui, inner, y, id.with("g0"), "General", !app.rp.closed[0], false) {
        app.rp.closed[0] = !app.rp.closed[0];
    }
    y += ROW;
    if !app.rp.closed[0] {
        let mut rows = 0;
        let line = |y: f32| row_line(&p, x, y);
        if let Some((t, r)) = region {
            line(y);
            color_row(app, ui, inner, y, id.with("color"), t, r);
            y += ROW;
            line(y);
            check_row(app, ui, inner, y, id.with("mute"), tg, false, "Mute", |a, tg| match tg {
                Tg::Region(t, r) => a.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|x| x.muted),
                _ => false,
            }, |a, tg, on| {
                if let Tg::Region(t, r) = tg {
                    if let Some(x) = a.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                        x.muted = on;
                    }
                }
            });
            y += ROW;
            rows += 2;
        }
        line(y);
        check_row(app, ui, inner, y, id.with("loop"), tg, false, "Loop", |a, tg| props(a, tg).is_some_and(|p| p.looped), |a, tg, on| with_props(a, tg, |p| p.looped = on));
        y += ROW;
        if audio && region.is_some() {
            line(y);
            num_row(app, ui, inner, y, id.with("gain"), tg, false, "Gain", P::Gain, "dB", &|v| format!("{v:.1}"), &[], 1.0);
            y += ROW;
        }
        // Smart Tempo (tempomode.rs) for an audio region
        if let (true, Some((t, r))) = (audio, region) {
            line(y);
            let cur = crate::tempomode::smart_tempo(app, t, r);
            if let Some(i) = choice_row(app, ui, inner, y, id.with("smart-tempo"), tg, false, "Smart Tempo", cur, &crate::tempomode::SMART) {
                crate::flex::checkpoint(app, "Smart Tempo");
                crate::tempomode::set_smart_tempo(app, t, r, i);
            }
            y += ROW;
        }
        // Delay: ticks or ms (the unit's pop-up switches)
        let in_ms = props(app, tg).is_some_and(|p| p.delay_ms);
        let k = if in_ms { ms_per_tick(app) } else { 1.0 };
        let mut menu: Vec<(String, f32)> = Vec::new();
        for (n, ticks) in [("-1/16 Note", -240.0), ("-1/32 Note", -120.0), ("-1/64 Note", -60.0), ("0", 0.0), ("+1/64 Note", 60.0), ("+1/32 Note", 120.0), ("+1/16 Note", 240.0)] {
            menu.push((n.to_string(), ticks));
        }
        for ms in [-20.0f32, -10.0, 10.0, 20.0] {
            menu.push((format!("{ms:+.0} ms"), ms / ms_per_tick(app)));
        }
        menu.push((if in_ms { "Show in Ticks".into() } else { "Show in Milliseconds".into() }, f32::NAN));
        line(y);
        label(&p, inner, y, false, "Delay");
        let v = get(app, tg, P::Delay);
        let star = differs(app, tg, &|a, x| format!("{}", get(a, x, P::Delay)));
        let shown = move |v: f32| if in_ms { format!("{v:.1}") } else { format!("{v:.0}") };
        if let Some(nv) = value_box(ui, inner, y, id.with("delay"), v, P::Delay, if in_ms { "ms" } else { "tk" }, &shown, &menu, star, k) {
            if nv.is_nan() {
                set_choice(app, tg, |a, x| with_props(a, x, |p| p.delay_ms = !in_ms));
            } else {
                let abs = ui.input(|i| i.modifiers.alt && i.modifiers.shift);
                change(app, tg, P::Delay, v, nv, abs);
            }
        }
        y += ROW;
        line(y);
        let oct: Vec<(String, f32)> = [36, 24, 12, 0, -12, -24, -36].iter().map(|o| (format!("{:+} (Octave {:+})", o, o / 12), *o as f32)).collect();
        num_row(app, ui, inner, y, id.with("tr"), tg, false, "Transpose", P::Transpose, "st", &signed, &oct, 1.0);
        y += ROW;
        if audio {
            line(y);
            let ft: Vec<(String, f32)> = (-5..=5).rev().map(|k| (format!("{:+}", k * 10), (k * 10) as f32)).collect();
            num_row(app, ui, inner, y, id.with("ft"), tg, false, "Fine Tune", P::FineTune, "ct", &signed, &ft, 1.0);
            y += ROW;
            if let Some((t, r)) = region {
                line(y);
                check_row(app, ui, inner, y, id.with("flex"), tg, false, "Flex", |a, tg| match tg {
                    Tg::Region(t, r) => crate::audioedit::extras(a, t, r).follow,
                    _ => false,
                }, |a, tg, on| {
                    if let Tg::Region(t, r) = tg {
                        let mut x = crate::audioedit::extras(a, t, r);
                        x.follow = on;
                        crate::audioedit::set_extras(a, t, r, x);
                    }
                });
                y += ROW;
                let _ = (t, r);
            }
        } else {
            line(y);
            num_row(app, ui, inner, y, id.with("vel"), tg, false, "Velocity", P::Velocity, "", &signed, &[], 1.0);
            y += ROW;
            line(y);
            num_row(app, ui, inner, y, id.with("dyn"), tg, false, "Dynamics", P::Dynamics, "%", &|v| if v == 0.0 { "Fixed".into() } else { format!("{v:.0}") }, &[("Fixed".into(), 0.0), ("100".into(), 100.0)], 1.0);
            y += ROW;
            line(y);
            num_row(app, ui, inner, y, id.with("gate"), tg, false, "Gate Time", P::Gate, "%", &|v| if v == 0.0 { "Fixed".into() } else if v >= 201.0 { "Legato".into() } else { format!("{v:.0}") }, &[("Fixed".into(), 0.0), ("100".into(), 100.0), ("Legato".into(), 201.0)], 1.0);
            y += ROW;
            line(y);
            let ps = props(app, tg).map_or(0, |p| p.pitch_source as usize);
            if let Some(i) = choice_row(app, ui, inner, y, id.with("ps"), tg, false, "Pitch Source", ps, &crate::globals::PITCH_SOURCES) {
                set_choice(app, tg, |a, x| with_props(a, x, |p| p.pitch_source = i as u8));
            }
            y += ROW;
        }
        // More
        line(y);
        if group_header(ui, inner, y, id.with("more0"), "More", app.rp.more[0], true) {
            app.rp.more[0] = !app.rp.more[0];
        }
        y += ROW;
        if app.rp.more[0] {
            if audio {
                if region.is_some() {
                    line(y);
                    check_row(app, ui, inner, y, id.with("rev"), tg, true, "Reverse", |a, tg| match tg {
                        Tg::Region(t, r) => crate::audioedit::extras(a, t, r).reverse,
                        _ => false,
                    }, |a, tg, on| {
                        if let Tg::Region(t, r) = tg {
                            let mut x = crate::audioedit::extras(a, t, r);
                            // (Reverse isn't available with Flex on)
                            if !x.follow {
                                x.reverse = on;
                            }
                            crate::audioedit::set_extras(a, t, r, x);
                        }
                    });
                    y += ROW;
                }
            } else {
                line(y);
                check_row(app, ui, inner, y, id.with("clip"), tg, true, "Clip Length", |a, tg| props(a, tg).is_some_and(|p| p.clip_length), |a, tg, on| with_props(a, tg, |p| p.clip_length = on));
                y += ROW;
            }
        }
        let _ = rows;
    }
    // ---- Quantize
    row_line(&p, x, y);
    if group_header(ui, inner, y, id.with("g1"), "Quantize", !app.rp.closed[1], false) {
        app.rp.closed[1] = !app.rp.closed[1];
    }
    y += ROW;
    if !app.rp.closed[1] {
        row_line(&p, x, y);
        let cur = crate::inspector::QUANTIZE.iter().position(|q| Some(q.1) == props(app, tg).map(|p| p.quantize)).unwrap_or(0);
        let names: Vec<&str> = crate::inspector::QUANTIZE.iter().map(|q| q.0).collect();
        if let Some(i) = choice_row(app, ui, inner, y, id.with("q"), tg, false, "Quantize", cur, &names) {
            set_choice(app, tg, |a, x| with_props(a, x, |p| p.quantize = crate::inspector::QUANTIZE[i].1));
        }
        y += ROW;
        row_line(&p, x, y);
        num_row(app, ui, inner, y, id.with("sw"), tg, false, "Q-Swing", P::QSwing, "%", &signed, &[], 1.0);
        y += ROW;
        row_line(&p, x, y);
        if group_header(ui, inner, y, id.with("more1"), "More", app.rp.more[1], true) {
            app.rp.more[1] = !app.rp.more[1];
        }
        y += ROW;
        if app.rp.more[1] {
            row_line(&p, x, y);
            num_row(app, ui, inner, y, id.with("qs"), tg, true, "Q-Strength", P::QStrength, "%", &signed, &[], 1.0);
            y += ROW;
            row_line(&p, x, y);
            let qr: Vec<(String, f32)> = [-60, -30, -15, 0, 15, 30, 60].iter().map(|v| (format!("{v}"), *v as f32)).collect();
            num_row(app, ui, inner, y, id.with("qr"), tg, true, "Q-Range", P::QRange, "", &signed, &qr, 1.0);
            y += ROW;
            if !audio {
                row_line(&p, x, y);
                num_row(app, ui, inner, y, id.with("qf"), tg, true, "Q-Flam", P::QFlam, "", &signed, &[], 1.0);
                y += ROW;
            }
        }
    }
    // ---- Fades (audio regions)
    if audio && region.is_some() {
        row_line(&p, x, y);
        if group_header(ui, inner, y, id.with("g2"), "Fades", !app.rp.closed[2], false) {
            app.rp.closed[2] = !app.rp.closed[2];
        }
        y += ROW;
        if !app.rp.closed[2] {
            for (k, (name, pp, unit)) in [("Fade-In", P::FadeIn, "ms"), ("Curve", P::InCurve, ""), ("Fade-Out", P::FadeOut, "ms")].into_iter().enumerate() {
                row_line(&p, x, y);
                num_row(app, ui, inner, y, id.with(("fd", k)), tg, false, name, pp, unit, &signed, &[], 1.0);
                y += ROW;
            }
            row_line(&p, x, y);
            let (t, r) = region.unwrap_or((0, 0));
            let cur = FadeType::ALL.iter().position(|f| *f == crate::audioedit::extras(app, t, r).fades.out_type).unwrap_or(0);
            if let Some(i) = choice_row(app, ui, inner, y, id.with("ftype"), tg, false, "Type", cur, &["Out", "X", "EqP", "X S"]) {
                set_choice(app, tg, |a, x| {
                    if let Tg::Region(t, r) = x {
                        if is_audio(a, x) {
                            let mut e = crate::audioedit::extras(a, t, r);
                            e.fades.out_type = FadeType::ALL[i];
                            crate::audioedit::set_extras(a, t, r, e);
                        }
                    }
                });
            }
            y += ROW;
            row_line(&p, x, y);
            num_row(app, ui, inner, y, id.with("fdoc"), tg, false, "Curve", P::OutCurve, "", &signed, &[], 1.0);
            y += ROW;
        }
    }
    // ---- Details
    if let Some((t, r)) = region {
        row_line(&p, x, y);
        if group_header(ui, inner, y, id.with("g3"), "Details", !app.rp.closed[3], false) {
            app.rp.closed[3] = !app.rp.closed[3];
        }
        y += ROW;
        if !app.rp.closed[3] {
            row_line(&p, x, y);
            bbdt_row(app, ui, inner, y, id.with("pos"), t, r, false);
            y += ROW;
            row_line(&p, x, y);
            bbdt_row(app, ui, inner, y, id.with("len"), t, r, true);
            y += ROW;
            row_line(&p, x, y);
            check_row(app, ui, inner, y, id.with("lock"), tg, false, "Lock Time Position", |a, tg| props(a, tg).is_some_and(|p| p.locked), |a, tg, on| with_props(a, tg, |p| p.locked = on));
            y += ROW;
        }
    }
    app.rp.float_h = y - y0;
    let card = Rect::from_min_max(card.min, pos2(card.max.x, y));
    bg_layer.set(bg_idx, egui::Shape::rect_filled(card, 10.0, card_bg()));
    y + 6.0
}

// ---------------------------------------------------------------- commands

fn frames_fn(app: &App) -> impl Fn(u32, u32) -> u32 + use<> {
    let (map, rate) = match &app.project {
        Some(p) => (lg_core::timeline::Map::of(p), p.sample_rate.max(1) as f64),
        None => (lg_core::timeline::Map::tempo(120.0), 44100.0),
    };
    move |a, b| ((map.secs_at(b as f64) - map.secs_at(a as f64)) * rate).round().max(0.0) as u32
}

/// A section edit with the audio regions' extras kept: each region tagged, its extras' file
/// offset carried in `audio_offset`, and the extras rebuilt from the tags afterwards (cut
/// pieces keep the gain, reverse and flex settings; a piece cut on its left loses the fade-in,
/// one cut on its right the fade-out).
fn with_audio_kept(app: &mut App, what: &str, f: impl FnOnce(&mut lg_core::Project, &dyn Fn(u32, u32) -> u32)) {
    crate::audioedit::checkpoint(app, what);
    let frames = frames_fn(app);
    let mut stash = Vec::new();
    let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    for t in 0..n_tracks {
        let n = app.project.as_ref().map_or(0, |p| p.tracks[t].regions.len());
        for r in 0..n {
            let audio = app.project.as_ref().is_some_and(|p| p.tracks[t].regions[r].audio.is_some());
            let x = if audio { Some(crate::audioedit::extras(app, t, r)) } else { None };
            let (start, len) = app.project.as_ref().map(|p| (p.tracks[t].regions[r].start, p.tracks[t].regions[r].length)).unwrap_or_default();
            let tag = stash.len() as u32 + 1;
            if let Some(rg) = app.project.as_mut().map(|p| &mut p.tracks[t].regions[r]) {
                rg.props.tag = tag;
                if let Some(x) = &x {
                    rg.props.audio_offset = x.offset as u32;
                }
            }
            stash.push((x, start, len));
        }
    }
    if let Some(p) = app.project.as_mut() {
        f(p, &frames);
        rp::sort_regions(p);
    }
    let keys: Vec<(usize, usize)> = app.audio_edit.regions.keys().copied().collect();
    for k in keys {
        app.audio_edit.regions.remove(&k);
    }
    let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    for t in 0..n_tracks {
        let n = app.project.as_ref().map_or(0, |p| p.tracks[t].regions.len());
        for r in 0..n {
            let Some(rg) = app.project.as_mut().map(|p| &mut p.tracks[t].regions[r]) else { continue };
            let tag = std::mem::take(&mut rg.props.tag) as usize;
            let (off, len) = (rg.props.audio_offset, rg.length);
            if let Some((Some(x), _, olen)) = stash.get(tag.wrapping_sub(1)).cloned() {
                let mut x = x;
                if off as usize != x.offset {
                    x.fades.in_ms = 0;
                }
                if len < olen && off as usize == x.offset || (off as usize != x.offset && len < olen) {
                    x.fades.out_ms = if off as usize == x.offset { 0 } else { x.fades.out_ms };
                }
                x.offset = off as usize;
                crate::audioedit::set_extras(app, t, r, x);
            }
        }
    }
    app.selected_regions.clear();
    app.selected_region = None;
    app.after_edit();
}

fn locators(app: &App) -> Option<(u32, u32)> {
    let (a, b) = app.cycle;
    (b > a).then_some((a, b))
}

fn selected_tracks(app: &App) -> Vec<usize> {
    let mut v: Vec<usize> = app.region_selection().into_iter().map(|x| x.0).collect();
    v.dedup();
    v
}

fn section(app: &mut App, name: &str, global_events: bool) {
    let selection = name.ends_with("(Selection)");
    let only = if selection { Some(selected_tracks(app)) } else { None };
    let Some((a, b)) = locators(app) else {
        app.tell("Set the locators first");
        return;
    };
    let pos = app.position() as u32;
    let only_ref = only.as_deref();
    let base = name.split(" (").next().unwrap_or(name);
    match base {
        "Cut Section Between Locators" | "Delete Section Between Locators" => {
            let mut sec = None;
            with_audio_kept(app, base, |p, fr| sec = Some(rp::cut_section(p, a, b, only_ref, global_events, fr)));
            if base.starts_with("Cut") {
                app.rp.section = sec;
            }
        }
        "Copy Section Between Locators" => {
            if let Some(p) = &app.project {
                let fr = frames_fn(app);
                app.rp.section = Some(rp::copy_section(p, a, b, only_ref, &fr));
            }
        }
        "Insert Section at Playhead" => {
            let Some(sec) = app.rp.section.clone() else { return };
            with_audio_kept(app, base, |p, fr| rp::insert_section(p, pos, &sec, global_events, fr));
        }
        "Insert Silence Between Locators" | "Insert Silence at Locators" => with_audio_kept(app, "Insert Silence", |p, fr| rp::insert_silence(p, a, b, only_ref, global_events, fr)),
        "Repeat Section Between Locators" => with_audio_kept(app, base, |p, fr| rp::repeat_section(p, a, b, only_ref, global_events, fr)),
        _ => {}
    }
}

fn length_edit(app: &mut App, what: &str, f: impl Fn(&mut lg_core::Region)) {
    let sel = app.region_selection();
    if sel.is_empty() {
        return;
    }
    app.checkpoint(what);
    for (t, r) in sel {
        if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            f(x);
        }
    }
    app.after_edit();
}

/// Repeat Regions: `copies` after each selected region, each at the end of the one before
/// (Adjustment rounds the step up to a bar / beat), as copies or aliases.
pub fn repeat(app: &mut App, copies: u32, adjust: usize, aliases: bool) {
    let sel = app.region_selection();
    if sel.is_empty() || copies == 0 {
        return;
    }
    app.checkpoint(if aliases { "Repeat Regions as Aliases" } else { "Repeat Regions" });
    let bar = app.bar_ticks().max(1);
    let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32);
    let grid = [0, bar, beat, 240][adjust.min(3)];
    let mut new_sel = Vec::new();
    for (t, r) in sel {
        let Some((start, len)) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|x| (x.start, x.length)) else { continue };
        let step = if grid == 0 { len } else { len.div_ceil(grid) * grid };
        for k in 1..=copies {
            let idx = if aliases && app.project.as_ref().is_some_and(|p| p.tracks[t].regions[r].audio.is_none()) {
                crate::arranging::alias_for_drag(app, t, r)
            } else {
                app.project.as_mut().map(|p| {
                    let mut c = p.tracks[t].regions[r].clone();
                    c.origin = c.source.or(c.origin);
                    c.source = None;
                    c.props.locked = false;
                    p.tracks[t].regions.push(c);
                    p.tracks[t].regions.len() - 1
                })
            };
            if let Some(i) = idx {
                if let Some(x) = app.project.as_mut().map(|p| &mut p.tracks[t].regions[i]) {
                    let d = start as i64 + (k * step) as i64 - x.start as i64;
                    x.start = (x.start as i64 + d) as u32;
                    for n in &mut x.notes {
                        n.pos = (n.pos as i64 + d).max(0) as u32;
                    }
                }
                new_sel.push((t, i));
            }
        }
    }
    app.selected_regions = new_sel;
    app.after_edit();
}

/// Split: what to do with notes across a cut (None: the dialog asked, wait for it). Notes
/// reaching more than a 1/16 note past a cut ask Keep / Shorten / Split; otherwise Shorten.
pub fn split_mode(app: &mut App, sel: &[(usize, usize)], cuts: &[u32]) -> Option<SplitNotes> {
    if let Some(m) = app.rp.split_mode.take() {
        return Some(m);
    }
    if app.automated {
        return Some(SplitNotes::Shorten);
    }
    let long = app.project.as_ref().is_some_and(|p| {
        sel.iter().any(|&(t, r)| p.tracks.get(t).and_then(|tr| tr.regions.get(r)).is_some_and(|rg| rg.notes.iter().any(|n| cuts.iter().any(|&c| n.pos < c && n.pos + n.len > c + 240 && c > rg.start && c < rg.start + rg.length))))
    });
    if long {
        app.rp.split_ask = Some(cuts.to_vec());
        return None;
    }
    Some(SplitNotes::Shorten)
}

/// The notes of the two halves of a split at `c` (left `a`, right `b`, already filtered by
/// start).
pub fn split_notes(a: &mut lg_core::Region, b: &mut lg_core::Region, c: u32, mode: SplitNotes) {
    if mode == SplitNotes::Keep {
        return;
    }
    for n in a.notes.iter_mut().filter(|n| n.pos + n.len > c) {
        if mode == SplitNotes::Split {
            b.notes.push(lg_core::events::Note { pos: c, len: n.pos + n.len - c, ..*n });
        }
        n.len = (c - n.pos).max(1);
    }
    b.notes.sort_by_key(|n| n.pos);
}

/// Join: parameters normalized first; a single looped region joins with its loops.
pub fn before_join(app: &mut App, sel: &[(usize, usize)]) {
    if let [(t, r)] = sel {
        let Some(p) = app.project.as_mut() else { return };
        let tr = &mut p.tracks[*t];
        let rg = &tr.regions[*r];
        if rg.props.looped {
            let end = rg.start + rg.length;
            let mut until = tr.regions.iter().map(|o| o.start).filter(|&s| s >= end).min().unwrap_or(end + 16 * 3840);
            if rg.props.loop_to > end {
                until = until.min(rg.props.loop_to);
            }
            rp::unloop(&mut tr.regions[*r], until);
        }
        return;
    }
    if let Some(p) = app.project.as_mut() {
        for &(t, r) in sel {
            if let Some(x) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) {
                rp::normalize_for_join(x);
            }
        }
    }
}

/// A region drag's start with Control (steps of one division, over the Snap value) or
/// Control-Shift (single ticks); None: the usual snapping.
pub fn drag_snap(app: &App, ui: &egui::Ui, raw: f32, start: u32) -> Option<u32> {
    let m = ui.input(|i| i.modifiers);
    if !m.ctrl {
        return None;
    }
    if m.shift {
        return Some(raw.max(0.0).round() as u32);
    }
    let div = (lg_core::events::PPQ / 4) as f64;
    let _ = app;
    Some(lg_core::regions::snap_move(start, raw as f64 - start as f64, div, true))
}

/// Settings ▸ General ▸ Editing ▸ Limit Dragging to One Direction In: Tracks area: the axis
/// the drag first went along (Some(true) horizontal, Some(false) vertical); None when the
/// setting is off, Shift is held, or the pointer hasn't moved far enough yet.
pub fn drag_axis(app: &mut App, ui: &egui::Ui) -> Option<bool> {
    if !app.logic_settings.on("General / Editing / Tracks area") || ui.input(|i| i.modifiers.shift) {
        return None;
    }
    if app.rp.axis.is_none() {
        let d = ui.input(|i| i.pointer.press_origin().zip(i.pointer.latest_pos()).map(|(a, b)| b - a));
        if let Some(d) = d.filter(|d| d.length() > 4.0) {
            app.rp.axis = Some(d.x.abs() >= d.y.abs());
        }
    }
    app.rp.axis
}

/// Region Solo: whether region `r` of track `t` plays.
pub fn plays(app: &App, t: usize, r: usize) -> bool {
    !app.rp.region_solo || app.rp.soloed.contains(&(t, r))
}

fn audio_targets(app: &App, t: usize, r: usize) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = app.region_selection();
    if !v.contains(&(t, r)) {
        v = vec![(t, r)];
    }
    v.retain(|&(t, r)| app.project.as_ref().is_some_and(|p| p.tracks.get(t).and_then(|x| x.regions.get(r)).is_some_and(|x| x.audio.is_some())));
    v
}

fn gain_tool(app: &App, ui: &egui::Ui) -> bool {
    let t = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
    t == crate::edit::Tool::Gain
}

/// Gain tool over an audio region: its gain in dB and a yellow line across it.
pub fn gain_hover(app: &App, ui: &egui::Ui, rr: Rect, t: usize, r: usize, hovered: bool) {
    if !(hovered || app.rp.gain.as_ref().is_some_and(|g| g.0.iter().any(|x| (x.0, x.1) == (t, r)))) || !gain_tool(app, ui) {
        return;
    }
    if !app.project.as_ref().is_some_and(|p| p.tracks[t].regions[r].audio.is_some()) {
        return;
    }
    let db = crate::audioedit::extras(app, t, r).gain_db;
    // the line: 0 dB at mid height, ±24 dB at the edges
    let y = rr.center().y - (db / 24.0).clamp(-1.0, 1.0) * rr.height() * 0.45;
    let p = ui.painter_at(rr);
    p.hline(rr.x_range(), y, Stroke::new(1.5, Color32::from_rgb(0xf5, 0xc5, 0x18)));
    p.text(pos2(rr.min.x + 6.0, y - 3.0), Align2::LEFT_BOTTOM, format!("{db:+.1} dB"), look::semibold(11.0), Color32::from_rgb(0xf5, 0xc5, 0x18));
}

/// Gain tool press on an audio region: the selected audio regions (or this one) take part.
pub fn gain_start(app: &mut App, t: usize, r: usize) {
    let v = audio_targets(app, t, r);
    if v.is_empty() {
        return;
    }
    crate::audioedit::checkpoint(app, "Region Gain");
    let g = v.iter().map(|&(t, r)| (t, r, crate::audioedit::extras(app, t, r).gain_db)).collect();
    app.rp.gain = Some((g, 0.0));
}

/// While the Gain tool drags: up raises, down lowers (0.1 dB a point, Shift finer); every
/// region by the same amount. True while a Gain drag runs.
pub fn gain_drag(app: &mut App, dy: f32, fine: bool) -> bool {
    let Some((v, mut d)) = app.rp.gain.take() else { return false };
    d -= dy * if fine { 0.02 } else { 0.1 };
    for &(t, r, g0) in &v {
        let mut x = crate::audioedit::extras(app, t, r);
        x.gain_db = (((g0 + d).clamp(-96.0, 30.0)) * 10.0).round() / 10.0;
        crate::audioedit::set_extras(app, t, r, x);
    }
    if let Some(&(t, r, _)) = v.first() {
        app.help_tag = Some(vec![("Gain:".into(), format!("{:+.1} dB", crate::audioedit::extras(app, t, r).gain_db))]);
    }
    app.rp.gain = Some((v, d));
    true
}

pub fn gain_end(app: &mut App) {
    if app.rp.gain.take().is_some() {
        app.load_engine();
    }
}

/// Gain tool click: Option resets the gain to 0 dB.
pub fn gain_click(app: &mut App, ui: &egui::Ui, t: usize, r: usize) -> bool {
    if ui.input(|i| i.modifiers.alt) {
        let v = audio_targets(app, t, r);
        if !v.is_empty() {
            crate::audioedit::checkpoint(app, "Region Gain");
            for (t, r) in v {
                let mut x = crate::audioedit::extras(app, t, r);
                x.gain_db = 0.0;
                crate::audioedit::set_extras(app, t, r, x);
            }
            app.after_edit();
        }
    }
    true
}

/// A marquee edge's tick: Control-Shift ticks, Control the division, else the Snap value
/// (Snap off: free).
pub fn marquee_snap(app: &App, t: f32, div: u32, px_per_tick: f32, m: egui::Modifiers) -> u32 {
    let t = t.max(0.0);
    if m.ctrl && m.shift {
        return t.round() as u32;
    }
    if m.ctrl {
        return ((t / div as f32).round() as u32) * div;
    }
    crate::arranging::snap_tick(app, t, px_per_tick).round() as u32
}

/// A click with the Marquee tool while a marquee is up: true when it was used (Shift: the
/// nearest end moves to the click, Option-Shift the farther one, outside the range it grows
/// and takes in the clicked track; a plain click inside: split at its borders).
pub fn marquee_click(app: &mut App, tick: u32, track: Option<usize>, m: egui::Modifiers) -> bool {
    let Some(mut mq) = app.ta.regions.marquee.clone() else { return false };
    let inside = tick > mq.from && tick < mq.to && track.is_none_or(|t| mq.tracks.contains(&t));
    if m.shift {
        if inside {
            let near_start = tick - mq.from <= mq.to - tick;
            if near_start != m.alt {
                mq.from = tick;
            } else {
                mq.to = tick;
            }
        } else {
            mq.from = mq.from.min(tick);
            mq.to = mq.to.max(tick);
            if let Some(t) = track {
                let (lo, hi) = (mq.tracks.iter().min().copied().unwrap_or(t).min(t), mq.tracks.iter().max().copied().unwrap_or(t).max(t));
                mq.tracks = (lo..=hi).collect();
            }
        }
        app.ta.regions.marquee = Some(mq);
        return true;
    }
    if inside {
        crate::regionops::apply_marquee(app, crate::regionops::Cut::Split);
        return true;
    }
    false
}

/// Shift-click on a track header with a marquee up: the track leaves (or rejoins) it.
pub fn marquee_toggle_track(app: &mut App, t: usize) {
    if let Some(m) = app.ta.regions.marquee.as_mut() {
        if let Some(k) = m.tracks.iter().position(|x| *x == t) {
            m.tracks.remove(k);
        } else {
            m.tracks.push(t);
            m.tracks.sort_unstable();
        }
    }
}

/// The marquee ruler: a thin strip along the bottom of the ruler; dragging in it makes a
/// marquee over every track; its shortcut menu sets the locators or splits at the marquee.
pub fn marquee_ruler(app: &mut App, ui: &mut egui::Ui, ruler: Rect, x0: f32, tick0: f32, px_per_tick: f32) {
    if !app.rp.marquee_ruler {
        return;
    }
    let strip = Rect::from_min_max(pos2(ruler.min.x, ruler.max.y - 7.0), ruler.max);
    let p = ui.painter_at(strip);
    p.rect_filled(strip, 0.0, look::mix(look::pane_bg(), Color32::WHITE, 0.06));
    let x_of = |t: u32| x0 + (t as f32 - tick0) * px_per_tick;
    if let Some(m) = &app.ta.regions.marquee {
        p.rect_filled(Rect::from_min_max(pos2(x_of(m.from), strip.min.y), pos2(x_of(m.to), strip.max.y)), 0.0, Color32::from_white_alpha(150));
    }
    let resp = ui.interact(strip, ui.scope_id().with("marquee-ruler"), Sense::click_and_drag());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let tick_at = |x: f32| ((x - x0) / px_per_tick + tick0).max(0.0);
    let div = (lg_core::events::PPQ / 4) as u32;
    let m = ui.input(|i| i.modifiers);
    if resp.drag_started() {
        app.rp.ruler_from = ui.input(|i| i.pointer.press_origin()).map(|o| marquee_snap(app, tick_at(o.x), div, px_per_tick, m));
    }
    if let (true, Some(from), Some(pp)) = (resp.dragged(), app.rp.ruler_from, resp.interact_pointer_pos()) {
        let now = marquee_snap(app, tick_at(pp.x), div, px_per_tick, m);
        let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
        if now != from {
            app.ta.regions.marquee = Some(crate::regionops::Marquee { from: from.min(now), to: from.max(now), tracks: (0..n).collect() });
        }
    }
    if resp.drag_stopped() {
        app.rp.ruler_from = None;
    }
    let mut run = None;
    resp.context_menu(|ui| {
        for c in ["Set Locators by Selection and Enable Cycle", "Split Regions at Marquee Selection"] {
            if ui.add_enabled(app.ta.regions.marquee.is_some(), egui::Button::new(c)).clicked() {
                run = Some(c);
                ui.close();
            }
        }
    });
    if let Some(c) = run {
        let ctx = ui.ctx().clone();
        command(app, &ctx, c);
    }
}

/// A region locked with Lock Time Position (Lock SMPTE Position) doesn't move.
pub fn locked(rg: &lg_core::Region) -> bool {
    rg.props.locked
}

/// New recorded regions take the Region inspector's defaults.
pub fn apply_defaults(app: &App, rg: &mut lg_core::Region) {
    let d = if rg.audio.is_some() { app.rp.defaults_audio } else { app.rp.defaults_midi };
    if let Some(d) = d {
        let keep = rg.props;
        rg.props = RegionProps { audio_offset: keep.audio_offset, fade_in: keep.fade_in, fade_out: keep.fade_out, color: keep.color, ..d };
    }
}

pub fn command(app: &mut App, _ctx: &egui::Context, name: &str) -> bool {
    match name {
        n if n.contains("Section Between Locators") || n.starts_with("Insert Section at Playhead") || n.starts_with("Insert Silence") => {
            if n.ends_with("(Selection)") && !app.automated {
                app.rp.ask_global = Some(n.to_string());
            } else {
                section(app, n, !n.ends_with("(Selection)"));
            }
        }
        "Double Region/Cell/Event/Chord Length" | "Double" => length_edit(app, "Double Region Length", rp::double),
        "Halve Region/Cell/Event/Chord Length" | "Halve" => length_edit(app, "Halve Region Length", rp::halve),
        "Change Region/Cell/Event Length…" | "Change…" => {
            if !app.region_selection().is_empty() {
                app.rp.length_sheet = Some(2.0);
            }
        }
        "Repeat Regions/Events Multiple Times…" | "Multiple…" => {
            if !app.region_selection().is_empty() {
                app.rp.repeat_sheet = Some((1, 0, false));
            }
        }
        "Paste at Original Position" => {
            let (Some(t), Some(mut rg)) = (app.selected_track, app.clipboard.clone()) else { return true };
            rg.origin = rg.source.or(rg.origin);
            rg.source = None;
            app.checkpoint("Paste at Original Position");
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                tr.regions.push(rg);
                app.selected_region = Some((t, tr.regions.len() - 1));
                app.selected_regions.clear();
            }
            app.after_edit();
        }
        "Color Regions/Cells by Track Color" => {
            let sel = app.region_selection();
            app.checkpoint("Color Regions by Track Color");
            for (t, r) in sel {
                let c = crate::tm::set(app, t).and_then(|s| s.strip.color);
                if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    x.props.color = c;
                }
            }
            app.after_edit();
        }
        "Region Inspector Float" => app.rp.float = !app.rp.float,
        "Marquee Ruler" => app.rp.marquee_ruler = !app.rp.marquee_ruler,
        "Split Regions at Marquee Selection" => {
            crate::regionops::apply_marquee(app, crate::regionops::Cut::Split);
        }
        "Set Locators by Selection and Enable Cycle" => {
            let Some(m) = app.ta.regions.marquee.clone() else { return true };
            app.set_cycle((m.from, m.to));
            if !app.cycle_on() {
                app.toggle_cycle();
            }
        }
        "Gain Tool" | "Set Gain Tool" => app.tool = crate::edit::Tool::Gain,
        "Toggle Region Solo" => {
            // on: the selected regions solo (only they play); off: everything plays again
            app.rp.region_solo = !app.rp.region_solo;
            app.rp.soloed = if app.rp.region_solo { app.region_selection() } else { Vec::new() };
            app.load_engine();
        }
        "Lock SMPTE Position" | "Unlock SMPTE Position" | "Lock/Unlock SMPTE Position" => {
            let sel = app.region_selection();
            let all = sel.iter().all(|&(t, r)| app.project.as_ref().is_some_and(|p| p.tracks[t].regions[r].props.locked));
            let on = match name {
                "Lock SMPTE Position" => true,
                "Unlock SMPTE Position" => false,
                _ => !all,
            };
            app.checkpoint(if on { "Lock Position" } else { "Unlock Position" });
            for (t, r) in sel {
                if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    x.props.locked = on;
                }
            }
            app.after_edit();
        }
        "SMPTE-Locked Regions" => {
            let v: Vec<(usize, usize)> = app.project.as_ref().map(|p| p.tracks.iter().enumerate().flat_map(|(t, tr)| tr.regions.iter().enumerate().filter(|(_, r)| r.props.locked).map(move |(r, _)| (t, r))).collect()).unwrap_or_default();
            app.selected_region = v.last().copied();
            app.selected_regions = v;
        }
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------- windows

pub fn windows(app: &mut App, ctx: &egui::Context) {
    if app.rp.float {
        let mut open = true;
        let h = app.rp.float_h + 60.0;
        egui::Window::new("Region Inspector").id(egui::Id::unique("rp-float")).title_bar(false).resizable(false).collapsible(false).default_pos(pos2(300.0, 120.0)).frame(egui::Frame::new().fill(look::pane_bg()).corner_radius(12.0).inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 6 }).stroke(Stroke::new(1.0, look::line()))).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(350.0, h), Sense::hover());
            // the close button, as Logic's float
            let c = Rect::from_center_size(r.min + vec2(16.0, 10.0), vec2(13.0, 13.0));
            ui.painter().circle_filled(c.center(), 6.5, Color32::from_rgb(0xff, 0x5f, 0x57));
            if ui.interact(c, ui.scope_id().with("close"), Sense::click()).clicked() {
                open = false;
            }
            let area = Rect::from_min_max(pos2(r.min.x, r.min.y + 26.0), r.max);
            let was = app.inspector_region_open;
            app.inspector_region_open = true;
            app.rp.in_float = true;
            ui.scope_builder(egui::UiBuilder::new().id_salt("rp-float-body").max_rect(area), |ui| {
                inspector(app, ui, area, area.min.y);
            });
            app.inspector_region_open = was;
            app.rp.in_float = false;
        });
        if !open {
            app.rp.float = false;
        }
    }
    if let Some(cuts) = app.rp.split_ask.clone() {
        let mut pick = None;
        egui::Window::new("Split").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -80.0)).show(ctx, |ui| {
            ui.label("Some notes in the regions reach across the split position.\nDo you want to keep, shorten or split them?");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Keep").clicked() {
                    pick = Some(Some(SplitNotes::Keep));
                }
                if ui.button("Shorten").clicked() {
                    pick = Some(Some(SplitNotes::Shorten));
                }
                if ui.button("Split").clicked() {
                    pick = Some(Some(SplitNotes::Split));
                }
                if ui.button("Cancel").clicked() {
                    pick = Some(None);
                }
            });
        });
        if let Some(m) = pick {
            app.rp.split_ask = None;
            if let Some(m) = m {
                app.rp.split_mode = Some(m);
                app.split_at(&cuts);
                app.rp.split_mode = None;
            }
        }
    }
    if let Some(f) = app.rp.length_sheet {
        let mut f = f;
        let mut done = None;
        egui::Window::new("Change Length").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -80.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Length Factor:");
                egui::ComboBox::from_id_salt("rp-lf").selected_text(format!("{f}")).show_ui(ui, |ui| {
                    for v in [0.25f32, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0] {
                        ui.selectable_value(&mut f, v, format!("{v}"));
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.button("OK").clicked() {
                    done = Some(true);
                }
            });
        });
        app.rp.length_sheet = Some(f);
        if let Some(ok) = done {
            app.rp.length_sheet = None;
            if ok {
                length_edit(app, "Change Region Length", |r| rp::change_length(r, f));
            }
        }
    }
    if let Some((mut n, mut adj, mut alias)) = app.rp.repeat_sheet {
        let mut done = None;
        egui::Window::new("Repeat Regions").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -80.0)).show(ctx, |ui| {
            egui::Grid::new("rp-rep").num_columns(2).show(ui, |ui| {
                ui.label("Number of Copies:");
                ui.add(egui::DragValue::new(&mut n).range(1..=999));
                ui.end_row();
                ui.label("Adjustment:");
                let names = ["None", "Bar", "Beat", "Division"];
                egui::ComboBox::from_id_salt("rp-adj").selected_text(names[adj]).show_ui(ui, |ui| {
                    for (i, s) in names.iter().enumerate() {
                        ui.selectable_value(&mut adj, i, *s);
                    }
                });
                ui.end_row();
                ui.label("");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut alias, false, "As Copies");
                    ui.radio_value(&mut alias, true, "As Aliases");
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.button("OK").clicked() {
                    done = Some(true);
                }
            });
        });
        app.rp.repeat_sheet = Some((n, adj, alias));
        if let Some(ok) = done {
            app.rp.repeat_sheet = None;
            if ok {
                repeat(app, n, adj, alias);
            }
        }
    }
    if let Some(cmd) = app.rp.ask_global.clone() {
        let mut pick = None;
        egui::Window::new("Section").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -80.0)).show(ctx, |ui| {
            ui.label("Also move global events (tempo, signature and key changes, markers)?");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    pick = Some(None);
                }
                if ui.button("Don't Move").clicked() {
                    pick = Some(Some(false));
                }
                if ui.button("Move").clicked() {
                    pick = Some(Some(true));
                }
            });
        });
        if let Some(p) = pick {
            app.rp.ask_global = None;
            if let Some(g) = p {
                section(app, &cmd, g);
            }
        }
    }
}

// ---------------------------------------------------------------- saved with the project

/// The parameters Studio adds beyond what Save writes into Logic's file, one line per region
/// that has any (track, region, start, then the values).
pub fn extras_text(p: &lg_core::Project) -> String {
    let mut out = String::from("# Bramble Studio: region parameters\n");
    let d = RegionProps::default();
    for (t, tr) in p.tracks.iter().enumerate() {
        for (r, x) in tr.regions.iter().enumerate() {
            let q = &x.props;
            if (q.q_strength, q.q_range, q.q_flam, q.clip_length, q.pitch_source, q.locked, q.color) == (d.q_strength, d.q_range, d.q_flam, d.clip_length, d.pitch_source, d.locked, d.color)
                && (q.delay_ms, q.dynamics, q.gate_time, q.delay, q.quantize, q.swing) == (d.delay_ms, d.dynamics, d.gate_time, d.delay, d.quantize, d.swing)
            {
                continue;
            }
            out += &format!(
                "rp\t{t}\t{r}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                x.start, q.q_strength, q.q_range, q.q_flam, q.clip_length as u8, q.pitch_source, q.locked as u8, q.color.map_or(-1, |c| c as i32), q.delay_ms as u8, q.dynamics, q.gate_time, q.delay, q.quantize, q.swing
            );
        }
    }
    out
}

pub fn apply_extras(p: &mut lg_core::Project, text: &str) -> bool {
    let mut any = false;
    for l in text.lines().filter(|l| l.starts_with("rp\t")) {
        let w: Vec<i64> = l.split('\t').skip(1).filter_map(|x| x.parse().ok()).collect();
        if w.len() < 16 {
            continue;
        }
        let Some(x) = p.tracks.get_mut(w[0] as usize).and_then(|t| t.regions.get_mut(w[1] as usize)).filter(|x| x.start as i64 == w[2]) else { continue };
        let q = &mut x.props;
        q.q_strength = w[3] as u8;
        q.q_range = w[4] as i16;
        q.q_flam = w[5] as i16;
        q.clip_length = w[6] != 0;
        q.pitch_source = w[7] as u8;
        q.locked = w[8] != 0;
        q.color = (w[9] >= 0).then_some(w[9] as u8);
        q.delay_ms = w[10] != 0;
        q.dynamics = w[11] as u16;
        q.gate_time = w[12] as u16;
        q.delay = w[13] as i32;
        q.quantize = w[14] as u32;
        q.swing = w[15] as u8;
        any = true;
    }
    any
}

fn extras_path(app: &App) -> Option<std::path::PathBuf> {
    let side = crate::hosting::sidecar_path(&app.file.as_ref()?.0)?;
    Some(side.parent()?.parent()?.join("region-params").join(side.file_name()?))
}

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

pub fn after_open(app: &mut App) {
    let Some(path) = extras_path(app) else { return };
    let Ok(text) = std::fs::read_to_string(path) else { return };
    if app.project.as_mut().is_some_and(|p| apply_extras(p, &text)) {
        app.load_engine();
        app.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let ctx = egui::Context::default();
        let mut a = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        a.set_project(crate::demo::song(), true);
        a
    }

    #[test]
    fn inspector_edits_every_selected_region_relatively() {
        let mut a = app();
        let regs: Vec<(usize, usize)> = a.project.as_ref().unwrap().tracks.iter().enumerate().filter(|(_, t)| t.kind != TrackKind::Audio && t.regions.len() >= 1).flat_map(|(t, tr)| (0..tr.regions.len()).map(move |r| (t, r))).take(2).collect();
        assert_eq!(regs.len(), 2);
        put(&mut a, Tg::Region(regs[1].0, regs[1].1), P::Transpose, 5.0);
        a.selected_region = Some(regs[0]);
        a.selected_regions = regs.clone();
        let tg = Tg::Region(regs[0].0, regs[0].1);
        assert!(differs(&a, tg, &|a, x| format!("{}", get(a, x, P::Transpose))), "the asterisk");
        change(&mut a, tg, P::Transpose, 0.0, 2.0, false);
        assert_eq!((get(&a, tg, P::Transpose), get(&a, Tg::Region(regs[1].0, regs[1].1), P::Transpose)), (2.0, 7.0), "differences kept");
        change(&mut a, tg, P::Transpose, 2.0, -1.0, true);
        assert_eq!(get(&a, Tg::Region(regs[1].0, regs[1].1), P::Transpose), -1.0, "Option-Shift: the same absolute value");
        a.undo();
        assert_eq!(get(&a, Tg::Region(regs[1].0, regs[1].1), P::Transpose), 7.0);
        // Legato through the Gate Time box
        put(&mut a, tg, P::Gate, 201.0);
        assert_eq!(props(&a, tg).unwrap().gate_time, GATE_LEGATO);
    }

    #[test]
    fn defaults_go_to_new_regions() {
        let mut a = app();
        put(&mut a, Tg::Defaults(false), P::Velocity, 12.0);
        let mut r = a.project.as_ref().unwrap().tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.audio.is_none()).unwrap().clone();
        apply_defaults(&a, &mut r);
        assert_eq!(r.props.velocity, 12);
    }

    #[test]
    fn commands_lock_color_length_repeat() {
        let mut a = app();
        let (t, r) = a.project.as_ref().unwrap().tracks.iter().enumerate().find(|(_, t)| t.kind != TrackKind::Audio && !t.regions.is_empty()).map(|(t, _)| (t, 0)).unwrap();
        a.selected_region = Some((t, r));
        a.selected_regions = vec![(t, r)];
        let ctx = egui::Context::default();
        assert!(command(&mut a, &ctx, "Lock SMPTE Position"));
        assert!(a.project.as_ref().unwrap().tracks[t].regions[r].props.locked);
        let len = a.project.as_ref().unwrap().tracks[t].regions[r].length;
        command(&mut a, &ctx, "Double Region/Cell/Event/Chord Length");
        assert_eq!(a.project.as_ref().unwrap().tracks[t].regions[r].length, len * 2);
        command(&mut a, &ctx, "Halve");
        assert_eq!(a.project.as_ref().unwrap().tracks[t].regions[r].length, len);
        color_regions(&mut a, &[(t, r)], Some(5));
        assert_eq!(a.project.as_ref().unwrap().tracks[t].regions[r].props.color, Some(5));
        let n = a.project.as_ref().unwrap().tracks[t].regions.len();
        repeat(&mut a, 3, 1, false);
        let p = a.project.as_ref().unwrap();
        assert_eq!(p.tracks[t].regions.len(), n + 3);
        let bar = 3840;
        let step = len.div_ceil(bar) * bar;
        assert_eq!(p.tracks[t].regions[n + 2].start, p.tracks[t].regions[r].start + 3 * step);
        assert!(!p.tracks[t].regions[n].props.locked, "copies aren't locked");
    }

    #[test]
    fn section_commands_keep_audio_offsets() {
        let mut a = app();
        let Some((t, r)) = a.project.as_ref().unwrap().tracks.iter().enumerate().find_map(|(t, tr)| tr.regions.iter().position(|x| x.audio.is_some()).map(|r| (t, r))) else { return };
        let (s, len) = a.project.as_ref().map(|p| (p.tracks[t].regions[r].start, p.tracks[t].regions[r].length)).unwrap();
        let mid = s + len / 2;
        a.cycle = (mid, mid + 3840);
        let ctx = egui::Context::default();
        command(&mut a, &ctx, "Insert Silence Between Locators (Global)");
        let p = a.project.as_ref().unwrap();
        let k = p.tracks[t].regions.iter().position(|x| x.start == mid + 3840).expect("the right part moved by the gap");
        assert!(crate::audioedit::extras(&a, t, k).offset > 0, "the right part plays on from the cut");
    }

    fn with_audio(a: &mut App) -> (usize, usize) {
        let p = a.project.as_mut().unwrap();
        let mut rg = p.tracks[0].regions[0].clone();
        rg.notes.clear();
        rg.audio = Some(lg_core::project::AudioRef { file: "/nonexistent/a.wav".into(), seconds: 4.0 });
        rg.start = 0;
        rg.length = 4 * 3840;
        p.tracks[0].regions.push(rg);
        (0, p.tracks[0].regions.len() - 1)
    }

    #[test]
    fn gain_tool_and_region_solo() {
        let mut a = app();
        let (t, r) = with_audio(&mut a);
        a.selected_regions = vec![(t, r)];
        gain_start(&mut a, t, r);
        assert!(gain_drag(&mut a, -30.0, false));
        gain_end(&mut a);
        assert!((crate::audioedit::extras(&a, t, r).gain_db - 3.0).abs() < 0.01, "30 points up: +3 dB");
        let ctx = egui::Context::default();
        command(&mut a, &ctx, "Toggle Region Solo");
        assert!(plays(&a, t, r) && !plays(&a, 0, 0));
        command(&mut a, &ctx, "Toggle Region Solo");
        assert!(plays(&a, 0, 0));
    }

    #[test]
    fn section_cut_keeps_audio_offset_and_fades() {
        let mut a = app();
        let (t, r) = with_audio(&mut a);
        let mut x = crate::audioedit::extras(&a, t, r);
        x.fades.in_ms = 50;
        x.fades.out_ms = 80;
        x.gain_db = -2.0;
        crate::audioedit::set_extras(&mut a, t, r, x);
        a.cycle = (3840, 2 * 3840);
        command(&mut a, &egui::Context::default(), "Cut Section Between Locators (Global)");
        let p = a.project.as_ref().unwrap();
        let left = p.tracks[t].regions.iter().position(|x| x.audio.is_some() && x.start == 0).unwrap();
        let right = p.tracks[t].regions.iter().position(|x| x.audio.is_some() && x.start == 3840).unwrap();
        let (l, rr) = (crate::audioedit::extras(&a, t, left), crate::audioedit::extras(&a, t, right));
        assert_eq!((l.offset, l.fades.in_ms, l.fades.out_ms, l.gain_db), (0, 50, 0, -2.0));
        assert!(rr.offset > 0 && rr.fades.in_ms == 0 && rr.fades.out_ms == 80 && rr.gain_db == -2.0, "{rr:?}");
        assert!(a.rp.section.as_ref().is_some_and(|s| s.len == 3840));
    }

    #[test]
    fn marquee_clicks() {
        let mut a = app();
        a.ta.regions.marquee = Some(crate::regionops::Marquee { from: 3840, to: 3 * 3840, tracks: vec![0] });
        let shift = egui::Modifiers::SHIFT;
        assert!(marquee_click(&mut a, 3840 + 100, Some(0), shift));
        assert_eq!(a.ta.regions.marquee.as_ref().unwrap().from, 3940, "nearest end");
        assert!(marquee_click(&mut a, 3940 + 100, Some(0), shift | egui::Modifiers::ALT));
        assert_eq!(a.ta.regions.marquee.as_ref().unwrap().to, 4040, "Option-Shift: the farther end");
        assert!(marquee_click(&mut a, 8 * 3840, Some(2), shift));
        let m = a.ta.regions.marquee.clone().unwrap();
        assert_eq!((m.from, m.to, m.tracks), (3940, 8 * 3840, vec![0, 1, 2]), "grows in any direction");
        marquee_toggle_track(&mut a, 1);
        assert_eq!(a.ta.regions.marquee.as_ref().unwrap().tracks, vec![0, 2]);
        marquee_toggle_track(&mut a, 1);
        assert_eq!(a.ta.regions.marquee.as_ref().unwrap().tracks, vec![0, 1, 2]);
        let n = a.project.as_ref().unwrap().tracks[0].regions.len();
        assert!(marquee_click(&mut a, 4 * 3840, Some(0), egui::Modifiers::NONE), "a click inside splits");
        assert!(a.project.as_ref().unwrap().tracks[0].regions.len() > n);
        assert_eq!(marquee_snap(&a, 250.0, 240, 0.1, egui::Modifiers::CTRL), 240);
        assert_eq!(marquee_snap(&a, 250.4, 240, 0.1, egui::Modifiers::CTRL | egui::Modifiers::SHIFT), 250);
    }

    #[test]
    fn sidecar_round_trip() {
        let mut p = crate::demo::song();
        let (t, r) = (0..p.tracks.len()).find_map(|t| (!p.tracks[t].regions.is_empty()).then_some((t, 0))).unwrap();
        p.tracks[t].regions[r].props.q_flam = -12;
        p.tracks[t].regions[r].props.color = Some(3);
        p.tracks[t].regions[r].props.locked = true;
        let text = extras_text(&p);
        let mut q = crate::demo::song();
        assert!(apply_extras(&mut q, &text));
        assert_eq!(q.tracks[t].regions[r].props, p.tracks[t].regions[r].props);
    }
}
