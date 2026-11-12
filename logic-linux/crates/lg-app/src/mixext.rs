//! The Mixer's channel strips as Logic's guide describes them, on top of mixer.rs: the Channel
//! Mode button (input format), the panners (Pan / Balance with typed values, Stereo Pan with
//! its handles, the Binaural Panner), Mute and Solo (slide over them, Command-click all,
//! Option-click exclusive solo, Control-click solo-safe, the flashing states), the held peak
//! level display with clipping colours, segmented meters, fine and typed fader values, the
//! sends' modes (Post Pan, Independent Pan, Copy Fader / Pan to Send, Low Latency Safe, Select
//! Channels with Same Destination), renaming strips, strip colours, Record Enable / Input
//! Monitoring / Dim / Bounce buttons, several groups per strip, the automation mode menu, the
//! Mixer's view options (Follow Hide, Follow Track Stacks, Autoscroll to Selection, Channels
//! with Sends only, Signal Flow Channel Strips), dragging strips to reorder, and the commands
//! around them. The windows (Binaural Panner, I/O Labels, channel strip settings, MIDI channel
//! strips, Configure Channel Strip Components) are in mixwin.rs.

use std::collections::{HashMap, HashSet};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::trackmix::{self, InputFormat, PanMode, SendSet};
use lg_core::TrackKind;

use crate::app::App;
use crate::audio::Out;
use crate::glyphs::{self, ph};
use crate::look;
use crate::mixer::AUX;
use crate::mixstate::{self, Kind, MASTER, VCA};

pub use crate::mixwin::{apply_strip_setting, io_label, strip_setting_text, Io};

include!("mixext_commands.rs");

pub struct State {
    /// The master's Dim button.
    pub dim: bool,
    /// The strip whose Binaural Panner window is open.
    pub bin_window: Option<usize>,
    pub bin_extended: bool,
    /// A value being typed: the strip and the text (pan, volume, name).
    pub pan_edit: Option<(usize, String)>,
    pub vol_edit: Option<(usize, String)>,
    pub renaming: Option<(usize, String)>,
    pub notes_edit: Option<usize>,
    /// Slide activation over Mute (0) or Solo (1) buttons: the state being spread.
    pub swipe: Option<(u8, bool)>,
    /// A send's destination flashing (Shift-click the send): the bus and when it began.
    pub flash: Option<(usize, f64)>,
    /// Groups a strip is in beside its first (strip → group indexes), and the groups last
    /// assigned (Option-click a Group slot applies them).
    pub more_groups: HashMap<usize, Vec<usize>>,
    pub last_groups: Vec<usize>,
    pub group_default: Option<[u8; 4]>,
    pub follow_hide: bool,
    pub follow_stacks: bool,
    pub autoscroll: bool,
    pub sends_only: bool,
    pub signal_flow: bool,
    pub same_strip_tracks: bool,
    pub long_faders: bool,
    /// Stacks closed in the Mixer alone (Follow Track Stacks off): main track ids.
    pub closed_stacks: HashSet<u64>,
    last_selected: Option<usize>,
    /// This frame's strips (their rectangles), for dragging a strip to a new place.
    pub strip_rects: Vec<(Rect, usize)>,
    pub reorder: Option<usize>,
    pub win: crate::mixwin::State,
    /// The filter kinds before an Option-click showed one alone.
    pub solo_kind: Option<HashSet<Kind>>,
}

impl Default for State {
    fn default() -> Self {
        State { dim: false, bin_window: None, bin_extended: false, pan_edit: None, vol_edit: None, renaming: None, notes_edit: None, swipe: None, flash: None, more_groups: HashMap::new(), last_groups: Vec::new(), group_default: None, follow_hide: true, follow_stacks: true, autoscroll: true, sends_only: false, signal_flow: true, same_strip_tracks: false, long_faders: false, closed_stacks: HashSet::new(), last_selected: None, strip_rects: Vec::new(), reorder: None, win: Default::default(), solo_kind: None }
    }
}

/// Channel strip components that are off until chosen (the others are on until hidden).
const DEFAULT_OFF: [&str; 4] = ["Type and Number Label", "Track Number", "Track Notes", "Audio Device Controls"];

/// A component's key by its menu label.
pub fn comp_key(label: &str) -> Option<&'static str> {
    Some(match label {
        "Audio Device Controls" => "Audio Device Controls",
        "Type and Number Label" => "Type and Number Label",
        "Setting Menu" => "Setting Menu",
        "Gain Reduction Meter" => "Gain Reduction Meter",
        "EQ Thumbnail" => "EQ Thumbnail",
        "MIDI Effects" => "MIDI Effects",
        "Input / Instrument" => "Input / Instrument",
        "Audio Effects" => "Audio Effects",
        "Sends" => "Sends",
        "Output" => "Output",
        "Group" => "Group",
        "Automation" => "part:Automation",
        "Icon" | "Track Icon" => "Track Icon",
        "Pan Knob" => "Pan Knob",
        "VCA" => "VCA",
        "Track Color" => "Track Color",
        "Track Stacks" => "Track Stacks",
        "Track Name" => "part:Track Name",
        "Track Number" => "Track Number",
        "Control Surface Bars" => "Control Surface Bars",
        "Track Notes" | "Notes" => "Track Notes",
        _ => return None,
    })
}

/// Whether a channel strip component shows.
pub fn comp(app: &App, key: &str) -> bool {
    if DEFAULT_OFF.contains(&key) { app.mix.hidden_parts.contains(&format!("on:{key}")) } else { !app.mix.hidden_parts.contains(key) }
}

pub fn set_comp(app: &mut App, key: &str, on: bool) {
    if DEFAULT_OFF.contains(&key) {
        let k = format!("on:{key}");
        if on {
            app.mix.hidden_parts.insert(k);
        } else {
            app.mix.hidden_parts.remove(&k);
        }
    } else if on {
        app.mix.hidden_parts.remove(key);
    } else {
        app.mix.hidden_parts.insert(key.to_string());
    }
}

fn is_track(s: usize) -> bool {
    s < AUX
}

// ---------------------------------------------------------------------------------------------
// the Channel Mode button and the panners

/// The Channel Mode button: one circle (mono), two interlocked (stereo), two with the left or
/// right filled (Left / Right), five (Surround). A click switches mono and stereo; a long
/// click (or right-click) shows all the input formats.
pub fn channel_mode_button(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, s: usize) {
    let fmt = crate::tm::strip(app, s).format;
    let resp = ui.interact(r, ui.scope_id().with(("chmode", s)), Sense::click()).on_hover_text(format!("Input format: {} (click: mono / stereo; hold: all formats)", fmt.name()));
    let c = r.center();
    let col = if resp.hovered() { look::text() } else { look::text_dim() };
    let ring = |at: egui::Pos2, filled: bool| {
        if filled {
            p.circle_filled(at, 4.0, col);
        } else {
            p.circle_stroke(at, 4.0, Stroke::new(1.2, col));
        }
    };
    match fmt {
        InputFormat::Mono => ring(c, false),
        InputFormat::Stereo => {
            ring(c - vec2(2.5, 0.0), false);
            ring(c + vec2(2.5, 0.0), false);
        }
        InputFormat::Left => {
            ring(c - vec2(2.5, 0.0), true);
            ring(c + vec2(2.5, 0.0), false);
        }
        InputFormat::Right => {
            ring(c - vec2(2.5, 0.0), false);
            ring(c + vec2(2.5, 0.0), true);
        }
        InputFormat::Surround => {
            // a square with dots at the ear-level speakers (L C R above, Ls Rs below)
            let sq = Rect::from_center_size(c, vec2(13.0, 13.0));
            p.rect_stroke(sq, 1.5, Stroke::new(1.1, col), egui::StrokeKind::Inside);
            for d in [vec2(-3.5, -3.5), vec2(0.0, -4.0), vec2(3.5, -3.5), vec2(-3.5, 3.5), vec2(3.5, 3.5)] {
                p.circle_filled(c + d, 1.1, col);
            }
        }
    }
    let long = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.press_start_time().is_some_and(|t| i.time - t > 0.45));
    if long || resp.secondary_clicked() {
        egui::Popup::open_id(ui.ctx(), resp.id.with("menu"));
    }
    egui::Popup::menu(&resp).id(resp.id.with("menu")).open_memory(None).show(|ui| {
        for f in InputFormat::ALL {
            if f == InputFormat::Surround && !crate::surround::active(app) {
                continue;
            }
            if ui.add(egui::Button::selectable(fmt == f, f.name())).clicked() {
                set_format(app, s, f);
                ui.close();
            }
        }
    });
    if resp.clicked() && !long {
        set_format(app, s, if fmt == InputFormat::Mono { InputFormat::Stereo } else { InputFormat::Mono });
    }
}

pub fn set_format(app: &mut App, s: usize, f: InputFormat) {
    mixstate::checkpoint(app, "Channel Strip Format");
    crate::tm::edit_strip(app, s, |x| {
        x.format = f;
        // (Stereo Pan is only on stereo strips)
        if !f.is_stereo() && x.pan_mode == PanMode::StereoPan {
            x.pan_mode = PanMode::Balance;
        }
    });
}

fn knob_angle(x: f32) -> f32 {
    std::f32::consts::PI * (1.5 + 0.75 * x)
}

/// The send the Pan knob drives now: with Sends on Faders, the strip's send to that bus when
/// it is in Independent Pan mode.
fn independent_send(app: &App, s: usize) -> Option<usize> {
    let b = app.sends_on_faders?;
    let k = app.sends.get(&s)?.iter().position(|x| x.bus == b)?;
    crate::tm::strip(app, s).send(k).independent_pan.map(|_| k)
}

/// The strip's panner in the Pan knob's place. True when `pan` (the Balance / Pan value) changed.
pub fn pan_control(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, y: f32, s: usize, pan: &mut f32) -> bool {
    let set = crate::tm::strip(app, s);
    let id = ui.scope_id().with(("panner", s));
    let kc = pos2(r.center().x, y + 16.0);
    let kr = Rect::from_center_size(kc, vec2(34.0, 34.0));
    let stereo = set.format.is_stereo();
    if comp(app, "Input / Instrument") && s < VCA {
        channel_mode_button(app, ui, p, Rect::from_min_size(pos2(r.min.x + 4.0, y + 8.0), vec2(18.0, 16.0)), s);
    }
    let mut changed = false;
    let (alt, cmd) = ui.input(|i| (i.modifiers.alt, i.modifiers.command));
    // typing a value (double-click the knob): -64 … +63
    if let Some((_, text)) = app.tm.mx.pan_edit.as_mut().filter(|x| x.0 == s) {
        let te = ui.put(Rect::from_center_size(kc, vec2(44.0, 20.0)), egui::TextEdit::singleline(text).font(look::regular(11.0)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            let v = text.trim().trim_start_matches('+').parse::<f32>().ok();
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            app.tm.mx.pan_edit = None;
            if let (Some(v), false) = (v, cancel) {
                mixstate::checkpoint(app, "Pan");
                *pan = (v.clamp(-64.0, 63.0) / if v < 0.0 { 64.0 } else { 63.0 }).clamp(-1.0, 1.0);
                return true;
            }
        }
        return false;
    }
    let resp = match set.pan_mode {
        PanMode::Balance => {
            // (a send in Independent Pan mode shown on the faders: the knob is that send's pan)
            let ind = independent_send(app, s);
            let shown = ind.and_then(|k| set.send(k).independent_pan).unwrap_or(*pan);
            let resp = ui.interact(kr, id, Sense::click_and_drag()).on_hover_text(format!("{} {:+.0}", if ind.is_some() { "Send Pan" } else if stereo { "Balance" } else { "Pan" }, shown * if shown < 0.0 { 64.0 } else { 63.0 }));
            p.circle_filled(kc, 15.0, Color32::from_gray(0x55));
            p.circle_stroke(kc, 15.0, Stroke::new(1.0, if ind.is_some() { Color32::from_rgb(0x9c, 0xc8, 0xf0) } else { Color32::from_gray(0x22) }));
            let ang = knob_angle(shown);
            p.line_segment([kc, kc + vec2(ang.cos(), ang.sin()) * 11.0], Stroke::new(2.0, Color32::WHITE));
            if resp.drag_started() {
                mixstate::checkpoint(app, "Pan");
            }
            let mut new = None;
            if resp.dragged() {
                // (sideways or up and down)
                let d = resp.drag_delta();
                new = Some((shown + (d.x - d.y) / 120.0).clamp(-1.0, 1.0));
            }
            if resp.clicked() && alt {
                mixstate::checkpoint(app, "Pan");
                new = Some(0.0);
            }
            if resp.double_clicked() {
                app.tm.mx.pan_edit = Some((s, format!("{:+.0}", shown * if shown < 0.0 { 64.0 } else { 63.0 })));
            }
            if let Some(v) = new {
                match ind {
                    Some(k) => crate::tm::edit_strip(app, s, |x| x.send_mut(k).independent_pan = Some(v)),
                    None => {
                        *pan = v;
                        changed = true;
                    }
                }
            }
            resp
        }
        PanMode::StereoPan => {
            let sp = set.stereo_pan;
            let resp = ui.interact(kr, id, Sense::click_and_drag()).on_hover_text(format!("Stereo Pan: L {:+.0}  R {:+.0}{}", sp.left * 64.0, sp.right * 64.0, if sp.swapped() { "  (swapped)" } else { "" }));
            p.circle_filled(kc, 9.0, Color32::from_gray(0x55));
            // the ring between the two sides: green, orange when left and right are swapped
            let ring = if sp.swapped() { Color32::from_rgb(0xe8, 0x9a, 0x2c) } else { Color32::from_rgb(0x4c, 0xd9, 0x64) };
            let (a0, a1) = (knob_angle(sp.left.min(sp.right)), knob_angle(sp.left.max(sp.right)));
            let n = 24;
            let pts: Vec<egui::Pos2> = (0..=n).map(|k| {
                let a = a0 + (a1 - a0) * k as f32 / n as f32;
                kc + vec2(a.cos(), a.sin()) * 14.0
            }).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.5, ring)));
            let handle = |x: f32| kc + vec2(knob_angle(x).cos(), knob_angle(x).sin()) * 14.0;
            p.circle_filled(handle(sp.left), 3.2, Color32::WHITE);
            p.circle_filled(handle(sp.right), 3.2, Color32::WHITE);
            // what the press took hold of: a handle (that side alone), the centre, or the ring
            if resp.drag_started() {
                mixstate::checkpoint(app, "Stereo Pan");
                let o = ui.input(|i| i.pointer.press_origin()).unwrap_or(kc);
                let part: u8 = if o.distance(handle(sp.left)) < 6.0 { 1 } else if o.distance(handle(sp.right)) < 6.0 { 2 } else if o.distance(kc) < 8.0 { 3 } else { 4 };
                ui.data_mut(|d| d.insert_temp(id, part));
            }
            if resp.dragged() {
                let part: u8 = ui.data(|d| d.get_temp(id)).unwrap_or(4);
                let d = resp.drag_delta();
                crate::tm::edit_strip(app, s, |x| {
                    let sp = &mut x.stereo_pan;
                    match part {
                        1 => sp.left = (sp.left + (d.x - d.y) / 90.0).clamp(-1.0, 1.0),
                        2 => sp.right = (sp.right + (d.x - d.y) / 90.0).clamp(-1.0, 1.0),
                        // the centre, dragged up or down: the spread's middle moves
                        3 => sp.set(sp.center() - d.y / 90.0, sp.spread()),
                        // the ring, up or down: wider or narrower
                        _ => sp.set(sp.center(), sp.spread() - d.y / 60.0),
                    }
                });
            }
            if resp.clicked() && cmd {
                mixstate::checkpoint(app, "Stereo Pan");
                crate::tm::edit_strip(app, s, |x| x.stereo_pan.swap());
            } else if resp.clicked() && alt {
                mixstate::checkpoint(app, "Stereo Pan");
                crate::tm::edit_strip(app, s, |x| x.stereo_pan = Default::default());
            }
            resp
        }
        PanMode::Binaural => {
            let b = set.binaural;
            let pad = Rect::from_center_size(kc, vec2(32.0, 32.0));
            let resp = ui.interact(pad, id, Sense::click_and_drag()).on_hover_text(format!("Binaural Panner: {:+.0}°, {:.2} m (double-click: the window)", b.angle, b.distance));
            p.circle_filled(kc, 16.0, look::lcd_bg());
            p.circle_stroke(kc, 16.0, Stroke::new(1.0, Color32::from_gray(0x55)));
            p.circle_filled(kc, 1.5, Color32::from_rgb(0x4c, 0xd9, 0x64));
            let (px, py) = b.puck();
            p.circle_filled(kc + vec2(px, -py) * 14.0, 3.5, Color32::from_rgb(0x9c, 0xc8, 0xf0));
            if resp.drag_started() {
                mixstate::checkpoint(app, "Binaural Pan");
            }
            if resp.dragged() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    let v = (pp - kc) / 14.0;
                    crate::tm::edit_strip(app, s, |x| x.binaural.set_puck(v.x, -v.y));
                }
            }
            if resp.clicked() && alt {
                mixstate::checkpoint(app, "Binaural Pan");
                crate::tm::edit_strip(app, s, |x| x.binaural.reset_position());
            }
            if resp.double_clicked() {
                app.tm.mx.bin_window = Some(s);
            }
            resp
        }
    };
    // Control-click: the panner (Logic's mixer-pan menu)
    resp.context_menu(|ui| {
        let cur = set.pan_mode;
        let mut pick = None;
        if ui.add_enabled(stereo, egui::Button::selectable(cur == PanMode::StereoPan, "Stereo Pan")).clicked() {
            pick = Some(PanMode::StereoPan);
        }
        if ui.add(egui::Button::selectable(cur == PanMode::Balance, if stereo { "Balance" } else { "Pan" })).clicked() {
            pick = Some(PanMode::Balance);
        }
        if ui.add(egui::Button::selectable(cur == PanMode::Binaural, "Binaural Panner")).clicked() {
            pick = Some(PanMode::Binaural);
        }
        if let Some(m) = pick {
            mixstate::checkpoint(app, "Panner");
            crate::tm::edit_strip(app, s, |x| x.pan_mode = m);
            ui.close();
        }
    });
    changed
}

// ---------------------------------------------------------------------------------------------
// Mute and Solo

/// Mute and Solo with Logic's gestures. True when this strip's own state changed (the caller
/// stores `mute` / `solo`); the gestures over all strips are applied here.
#[allow(clippy::too_many_arguments)]
pub fn ms_buttons(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, mr: Rect, sr: Rect, s: usize, mute: &mut bool, solo: &mut bool) -> bool {
    let id = ui.scope_id().with(("ms2", s));
    let (alt, cmd) = ui.input(|i| (i.modifiers.alt, i.modifiers.command));
    let down = ui.input(|i| i.pointer.primary_down());
    let pointer = ui.ctx().pointer_latest_pos();
    let blink = (ui.input(|i| i.time) * 2.5).fract() < 0.5;
    let set = crate::tm::strip(app, s);
    let (any_solo, flow) = crate::trackhdr::solo_state(app);
    let external = is_track(s) && crate::tm::set(app, s).is_some_and(|x| x.external.is_some());
    // (a soloed track's destination aux, and tracks feeding a soloed aux, are in its signal flow)
    let soloed_tracks: Vec<usize> = app.project.as_ref().map(|pr| (0..pr.tracks.len()).filter(|t| pr.tracks[*t].solo).collect()).unwrap_or_default();
    let fed_by_solo = s >= AUX && s < VCA && app.auxes.get(s - AUX).is_some_and(|a| soloed_tracks.iter().any(|t| matches!(app.mix.outputs.get(t), Some(Out::Bus(b)) if *b == a.bus) || app.sends.get(t).is_some_and(|v| v.iter().any(|x| x.bus == a.bus))));
    let in_flow = (is_track(s) && flow.contains(&s)) || fed_by_solo;
    let mut changed = false;
    let blue = Color32::from_rgb(0x4f, 0xa8, 0xe8);
    let yellow = Color32::from_rgb(0xe8, 0xc8, 0x3c);
    for (which, rr) in [(0u8, mr), (1u8, sr)] {
        let on = if which == 0 { *mute } else { *solo };
        let silenced = which == 0 && any_solo && !*solo && !*mute && !set.solo_safe && !in_flow && !external && s < VCA;
        let flowing = which == 1 && any_solo && !*solo && in_flow;
        let col = if which == 0 { blue } else { yellow };
        p.rect_filled(rr, 4.0, if on { col } else if (silenced || flowing) && blink { col.gamma_multiply(0.45) } else { look::group_bg() });
        p.text(rr.center(), Align2::CENTER_CENTER, if which == 0 { "M" } else { "S" }, look::bold(11.0), if on { Color32::BLACK } else { look::text() });
        if which == 1 && set.solo_safe {
            // solo-safe: a red slash through the S
            p.line_segment([rr.left_bottom() + vec2(4.0, -3.0), rr.right_top() + vec2(-4.0, 3.0)], Stroke::new(1.6, look::record_red()));
        }
        let resp = ui.interact(rr, id.with(which), Sense::click_and_drag());
        if which == 1 && resp.secondary_clicked() {
            let safe = set.solo_safe;
            crate::tm::edit_strip(app, s, |x| x.solo_safe = !safe);
            app.load_engine();
            continue;
        }
        let mut new = None;
        if resp.drag_started() {
            app.tm.mx.swipe = Some((which, !on));
            new = Some(!on);
        } else if resp.clicked() {
            new = Some(!on);
        } else if let (Some((k, state)), true, Some(pp)) = (app.tm.mx.swipe, down, pointer) {
            // sliding sideways over the buttons: each takes the first one's new state
            if k == which && rr.expand2(vec2(6.0, 2.0)).contains(pp) && on != state {
                new = Some(state);
            }
        }
        let Some(state) = new else { continue };
        mixstate::checkpoint(app, if which == 0 { "Mute" } else { "Solo" });
        if which == 0 && cmd {
            // Command-click: every Mute button in the same state follows (the outputs too)
            for o in mixstate::all_strips(app) {
                if let Some(v) = mixstate::values(app, o) {
                    if v.2 == on {
                        mixstate::set_values(app, o, (v.0, v.1, state, v.3));
                    }
                }
            }
            *mute = state;
            changed = true;
        } else if which == 1 && alt {
            // Option-click: this strip alone; on a soloed one: every solo off
            for o in mixstate::all_strips(app) {
                if let Some(v) = mixstate::values(app, o) {
                    mixstate::set_values(app, o, (v.0, v.1, v.2, !on && o == s));
                }
            }
            *solo = !on;
            changed = true;
        } else {
            if which == 0 {
                *mute = state;
            } else {
                *solo = state;
            }
            changed = true;
        }
    }
    changed
}

// ---------------------------------------------------------------------------------------------
// levels

fn held_peak(app: &App, s: usize) -> f32 {
    let e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    if s == MASTER {
        e.ext.master_peak
    } else if s >= AUX {
        e.ext.aux_peaks.get(s - AUX).copied().unwrap_or(0.0)
    } else {
        e.ext.peaks.get(s).copied().unwrap_or(0.0)
    }
}

pub fn reset_peaks(app: &App) {
    app.engine.lock().unwrap_or_else(|e| e.into_inner()).ext.reset_peaks();
}

/// The peak level display: the highest level reached since the last reset; over 0 dB it lights
/// red on the output strip (clipping) and orange on the others. A click resets all of them.
pub fn peak_display(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, s: usize) {
    let peak = held_peak(app, s);
    let db = if peak <= 1e-4 { -90.0 } else { 20.0 * peak.log10() };
    let over = db > 0.0;
    let col = if over && s == MASTER { look::record_red() } else if over { Color32::from_rgb(0xf0, 0x9a, 0x2c) } else { Color32::from_rgb(0x4c, 0xd9, 0x64) };
    if over {
        p.rect_filled(r, 3.0, col.gamma_multiply(0.25));
    }
    p.text(r.center(), Align2::CENTER_CENTER, if db <= -89.0 { "-∞".into() } else { format!("{db:.1}") }, look::regular(10.5), col);
    if ui.interact(r, ui.scope_id().with(("peak", s)), Sense::click()).on_hover_text("Peak level (click: reset all)").clicked() {
        reset_peaks(app);
    }
}

/// The level meter in segments: green, then yellow and amber towards 0 dB, red at the top;
/// one column for mono, left or right strips, two for stereo.
pub fn meter(app: &App, p: &egui::Painter, r: Rect, s: usize, level: f32) {
    // (a surround strip: one column per channel of the project's surround format)
    let f = crate::tm::strip(app, s).format;
    let cols = if f == InputFormat::Surround { app.sur.format.n() } else { f.meter_columns().min(2) };
    let lv = level.clamp(0.0, 1.2).sqrt().min(1.0);
    let seg = 3.0;
    let n = (r.height() / seg).floor() as usize;
    let w = (r.width() - (cols as f32 - 1.0)) / cols as f32;
    for c in 0..cols {
        let x0 = r.min.x + c as f32 * (w + 1.0);
        for k in 0..n {
            let f = (k as f32 + 0.5) / n as f32;
            if f > lv {
                break;
            }
            // the segment's level: the height is the amplitude's square root
            let db = 40.0 * f.max(1e-3).log10();
            let col = if db > -1.0 { look::record_red() } else if db > -6.0 { Color32::from_rgb(0xf0, 0x9a, 0x2c) } else if db > -14.0 { Color32::from_rgb(0xe8, 0xd0, 0x3c) } else { Color32::from_rgb(0x4c, 0xd9, 0x64) };
            let yb = r.max.y - k as f32 * seg;
            p.rect_filled(Rect::from_min_max(pos2(x0, yb - seg + 1.0), pos2(x0 + w, yb)), 0.0, col);
        }
    }
}

/// Typing a fader value (double-click the volume field). True when `vol` changed.
pub fn typed_volume(app: &mut App, ui: &mut egui::Ui, r: Rect, s: usize, vol: &mut f32) -> bool {
    if let Some((_, text)) = app.tm.mx.vol_edit.as_mut().filter(|x| x.0 == s) {
        let te = ui.put(r.expand2(vec2(6.0, 1.0)), egui::TextEdit::singleline(text).font(look::regular(10.5)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            let t = text.trim().to_lowercase();
            let v = if t.contains('∞') || t == "-inf" { Some(-96.0) } else { t.trim_end_matches("db").trim().parse::<f32>().ok() };
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            app.tm.mx.vol_edit = None;
            if let (Some(v), false) = (v, cancel) {
                mixstate::checkpoint(app, "Volume");
                *vol = v.clamp(-96.0, 6.0);
                return true;
            }
        }
        return false;
    }
    if ui.interact(r, ui.scope_id().with(("voledit", s)), Sense::click()).double_clicked() {
        app.tm.mx.vol_edit = Some((s, if *vol <= -90.0 { "-∞".into() } else { format!("{vol:.1}") }));
    }
    false
}

/// The fader dragged with Shift: finer steps (a tenth of the plain drag).
pub fn fine_fader(vol: f32, dy: f32, height: f32) -> f32 {
    let f = crate::tracks::db_to_frac(vol) - dy / height * 0.1;
    crate::tracks::frac_to_db(f.clamp(0.0, 1.0)).min(6.0)
}

// ---------------------------------------------------------------------------------------------
// sends

/// The strips sending to `bus` (their sends or their output).
pub fn senders(app: &App, bus: usize) -> Vec<usize> {
    let mut v: Vec<usize> = app.sends.iter().filter(|(_, l)| l.iter().any(|x| x.bus == bus)).map(|(s, _)| *s).collect();
    v.sort();
    v
}

pub fn send_label(app: &App, bus: usize) -> String {
    io_label(app, Io::Bus(bus), false)
}

/// The send knob's ring: green for Post Pan, blue for Post Fader (a Pre Fader send has none);
/// an Independent Pan send shows a small pan mark.
pub fn send_ring(app: &App, p: &egui::Painter, c: egui::Pos2, s: usize, k: usize, pre: bool) {
    let set = crate::tm::strip(app, s).send(k);
    if set.bypass {
        p.circle_stroke(c, 9.0, Stroke::new(1.2, look::text_dim()));
        return;
    }
    if !pre {
        p.circle_stroke(c, 9.0, Stroke::new(1.4, if set.post_pan { Color32::from_rgb(0x4c, 0xd9, 0x64) } else { Color32::from_rgb(0x4f, 0xa8, 0xe8) }));
    }
    if let Some(pan) = set.independent_pan {
        p.circle_filled(c + vec2(pan * 7.0, -11.0), 1.5, Color32::WHITE);
    }
}

/// A click on a send slot with a modifier: Shift flashes its destination's strips, Command
/// removes the send. True when the click was used (the menu stays closed).
pub fn send_click(app: &mut App, ui: &egui::Ui, resp: &egui::Response, s: usize, k: usize) -> bool {
    if resp.hovered() && ui.input(|i| i.modifiers.command) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
    }
    if !resp.clicked() {
        return false;
    }
    let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
    if shift {
        if let Some(b) = app.sends.get(&s).and_then(|v| v.get(k)).map(|x| x.bus) {
            app.tm.mx.flash = Some((b, ui.input(|i| i.time)));
        }
        return true;
    }
    if cmd {
        mixstate::checkpoint(app, "Remove Send");
        remove_send(app, s, k);
        return true;
    }
    false
}

/// Removes send `k` of strip `s` with its settings.
pub fn remove_send(app: &mut App, s: usize, k: usize) {
    if let Some(v) = app.sends.get_mut(&s) {
        if k < v.len() {
            v.remove(k);
        }
    }
    crate::tm::edit_strip(app, s, |x| {
        if k < x.sends.len() {
            x.sends.remove(k);
        }
    });
    app.dirty = true;
    app.load_engine();
}

/// Whether an aux strip flashes now (its bus is a Shift-clicked send's destination): three
/// flashes.
pub fn flashing(app: &App, ui: &egui::Ui, s: usize) -> bool {
    let Some((bus, t0)) = app.tm.mx.flash else { return false };
    let dt = ui.input(|i| i.time) - t0;
    s >= AUX && s < VCA && app.auxes.get(s - AUX).is_some_and(|a| a.bus == bus) && dt < 0.9 && (dt * 3.34).fract() < 0.5
}

/// The send slot's menu top: Independent Pan, then Post Pan / Post Fader / Pre Fader, Copy
/// Fader to Send, Copy Pan to Send, Low Latency Safe, Select Channels with Same Destination.
pub fn send_menu(app: &mut App, ui: &mut egui::Ui, s: usize, k: usize) {
    let Some(snd) = app.sends.get(&s).and_then(|v| v.get(k)).copied() else { return };
    let set = crate::tm::strip(app, s).send(k);
    let edit = |app: &mut App, f: &dyn Fn(&mut SendSet)| {
        mixstate::checkpoint(app, "Send");
        crate::tm::edit_strip(app, s, |x| f(x.send_mut(k)));
        app.load_engine();
    };
    let on = set.independent_pan.is_some();
    if ui.add(egui::Button::selectable(on, "Independent Pan")).clicked() {
        let pan = mixstate::values(app, s).map_or(0.0, |v| v.1);
        edit(app, &|x| x.independent_pan = if on { None } else { Some(pan) });
        ui.close();
    }
    ui.separator();
    let mode = if snd.pre { 2 } else if set.post_pan { 0 } else { 1 };
    for (m, label) in [(0, "Post Pan"), (1, "Post Fader"), (2, "Pre Fader")] {
        if ui.add(egui::Button::selectable(mode == m, label)).clicked() {
            edit(app, &|x| x.post_pan = m == 0);
            if let Some(x) = app.sends.get_mut(&s).and_then(|v| v.get_mut(k)) {
                x.pre = m == 2;
            }
            app.load_engine();
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Copy Fader to Send").clicked() {
        mixstate::checkpoint(app, "Copy Fader to Send");
        let vol = mixstate::values(app, s).map_or(0.0, |v| v.0);
        if let Some(x) = app.sends.get_mut(&s).and_then(|v| v.get_mut(k)) {
            x.level_db = vol.min(6.0);
        }
        app.load_engine();
        ui.close();
    }
    // (only an Independent Pan send has a pan of its own to copy to)
    if ui.add_enabled(on, egui::Button::new("Copy Pan to Send")).clicked() {
        let pan = mixstate::values(app, s).map_or(0.0, |v| v.1);
        edit(app, &|x| x.independent_pan = Some(pan));
        ui.close();
    }
    ui.separator();
    if ui.add(egui::Button::selectable(set.low_latency_safe, "Low Latency Safe")).clicked() {
        edit(app, &|x| x.low_latency_safe = !set.low_latency_safe);
        ui.close();
    }
    if ui.add(egui::Button::selectable(set.bypass, "Bypass Send")).clicked() {
        edit(app, &|x| x.bypass = !set.bypass);
        ui.close();
    }
    let same = senders(app, snd.bus);
    if same.len() > 1 {
        ui.separator();
        if ui.button("Select Channels with Same Destination").clicked() {
            app.mix.selected = same.into_iter().collect();
            ui.close();
        }
    }
    ui.separator();
}

/// The bus menu with a search field at its top: bus numbers, their I/O labels and the names of
/// the aux strips they feed all match.
pub fn bus_menu(ui: &mut egui::Ui, app: &App, current: Option<usize>) -> Option<usize> {
    let mut out = None;
    let id = egui::Id::unique("bus-menu-search");
    let mut q: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search").desired_width(170.0));
    ui.data_mut(|d| d.insert_temp(id, q.clone()));
    let ql = q.to_lowercase();
    let label = |b: usize| {
        let long = io_label(app, Io::Bus(b), true);
        match app.auxes.iter().find(|a| a.bus == b) {
            Some(a) => format!("{long} → {}", a.name),
            None => long,
        }
    };
    let mut row = |ui: &mut egui::Ui, b: usize| {
        if ui.add(egui::Button::selectable(current == Some(b), label(b))).clicked() {
            out = Some(b);
            ui.close();
        }
    };
    if !ql.is_empty() {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for b in (0..256).filter(|&b| label(b).to_lowercase().contains(&ql) || (b + 1).to_string() == ql) {
                row(ui, b);
            }
        });
        return out;
    }
    for b in 0..32 {
        row(ui, b);
    }
    ui.separator();
    for (lo, hi) in [(32usize, 64usize), (64, 96), (96, 128), (128, 160), (160, 192), (192, 224), (224, 256)] {
        ui.menu_button(format!("{} - {}", lo + 1, hi), |ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for b in lo..hi {
                    row(ui, b);
                }
            });
        });
    }
    out
}

/// A search field at the top of a plug-in menu: the effects Studio has that match; the pick.
pub fn plugin_search(ui: &mut egui::Ui) -> Option<String> {
    let id = egui::Id::unique("plugin-menu-search");
    let mut q: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search").desired_width(180.0));
    ui.data_mut(|d| d.insert_temp(id, q.clone()));
    let mut pick = None;
    if !q.trim().is_empty() {
        let ql = q.to_lowercase();
        let mut names: Vec<&str> = crate::units::effect_names().into_iter().filter(|n| n.to_lowercase().contains(&ql)).collect();
        names.sort();
        names.dedup();
        for n in names.into_iter().take(16) {
            if ui.button(n).clicked() {
                pick = Some(n.to_string());
                ui.data_mut(|d| d.insert_temp(id, String::new()));
                ui.close();
            }
        }
        ui.separator();
    }
    pick
}

// ---------------------------------------------------------------------------------------------
// the Gain Reduction meter, the EQ display, the plug-in slots, sends and outputs of a selection

fn topmost(app: &App, s: usize, names: &[&str]) -> Option<usize> {
    mixstate::inserts(app, s).iter().position(|u| names.contains(&u.name.as_str()))
}

/// A click on the Gain Reduction meter: a Compressor goes into the first free slot (with
/// Option: into the first slot, the others moving down), or the topmost Compressor's window
/// opens or closes; Command-click removes the topmost Compressor.
pub fn gr_clicked(app: &mut App, ui: &egui::Ui, resp: &egui::Response, s: usize) {
    if !resp.clicked() {
        return;
    }
    let (alt, cmd) = ui.input(|i| (i.modifiers.alt, i.modifiers.command));
    let at = topmost(app, s, &["Compressor"]);
    if cmd {
        if let Some(k) = at {
            remove_insert(app, s, k);
        }
        return;
    }
    match at {
        Some(k) if !alt => app.mix.fx_window = if app.mix.fx_window == Some((s, k)) { None } else { Some((s, k)) },
        _ => {
            mixstate::checkpoint(app, "Insert Plug-in");
            let mut slot = 0;
            mixstate::edit_inserts(app, s, |v| {
                slot = if alt { 0 } else { v.len() };
                v.insert(slot, mixstate::new_plugin("Compressor"));
            });
            app.mix.fx_window = Some((s, slot));
            app.load_engine();
        }
    }
}

/// The EQ display's clicks: a click inserts the Channel EQ or opens (closes) its window;
/// Shift-click inserts a Linear Phase EQ into the first free Audio FX slot (Option-Shift:
/// the first slot); Command-click removes the topmost Channel EQ or Linear Phase EQ.
pub fn eq_clicked(app: &mut App, ui: &egui::Ui, resp: &egui::Response, s: usize) {
    if !resp.clicked() && !resp.double_clicked() {
        return;
    }
    let (alt, cmd, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.command, i.modifiers.shift));
    if cmd {
        if app.eqs.contains_key(&s) {
            mixstate::checkpoint(app, "Remove Channel EQ");
            app.eqs.remove(&s);
            if app.eq_window == Some(s) {
                app.eq_window = None;
            }
            app.load_engine();
        } else if let Some(k) = topmost(app, s, &["Linear Phase EQ"]) {
            remove_insert(app, s, k);
        }
        return;
    }
    if shift {
        match topmost(app, s, &["Linear Phase EQ"]) {
            Some(k) if !alt => app.mix.fx_window = if app.mix.fx_window == Some((s, k)) { None } else { Some((s, k)) },
            _ => {
                mixstate::checkpoint(app, "Insert Plug-in");
                let mut slot = 0;
                mixstate::edit_inserts(app, s, |v| {
                    slot = if alt { 0 } else { v.len() };
                    v.insert(slot, mixstate::new_plugin("Linear Phase EQ"));
                });
                app.mix.fx_window = Some((s, slot));
                app.load_engine();
            }
        }
        return;
    }
    if !app.eqs.contains_key(&s) {
        mixstate::checkpoint(app, "Insert Channel EQ");
        app.eqs.entry(s).or_default();
        app.eq_window = Some(s);
        app.load_engine();
    } else {
        app.eq_window = if app.eq_window == Some(s) && !resp.double_clicked() { None } else { Some(s) };
    }
}

/// Removes Audio FX `k` of strip `s` (Command-click a slot).
pub fn remove_insert(app: &mut App, s: usize, k: usize) {
    mixstate::checkpoint(app, "Remove Plug-in");
    mixstate::edit_inserts(app, s, |v| {
        if k < v.len() {
            v.remove(k);
        }
    });
    if app.mix.fx_window.is_some_and(|w| w.0 == s) {
        app.mix.fx_window = None;
    }
    app.load_engine();
}

/// The strips an edit on `s` reaches: every selected strip when `s` is one of them.
fn with_selection(app: &App, s: usize) -> Vec<usize> {
    if app.mix.selected.contains(&s) { app.mix.selected.iter().copied().filter(|x| *x < VCA).collect() } else { vec![s] }
}

/// A new send to `bus` on `s` — and on the other selected strips when `s` is selected (up to
/// 12 sends a strip); the bus gets its aux.
pub fn add_send_selected(app: &mut App, s: usize, bus: usize) {
    for x in with_selection(app, s) {
        let n = app.sends.get(&x).map_or(0, |v| v.len());
        let own_bus = x >= AUX && app.auxes.get(x - AUX).is_some_and(|a| a.bus == bus);
        if n < 12 && !own_bus && !app.sends.get(&x).is_some_and(|v| v.iter().any(|y| y.bus == bus)) {
            app.add_send(x, bus);
        }
    }
}

/// The output chosen on `s`: a bus chosen on one of several selected strips goes to all.
pub fn set_output_selected(app: &mut App, s: usize, out: Out) {
    let targets = if matches!(out, Out::Bus(_)) { with_selection(app, s) } else { vec![s] };
    for x in targets {
        let own_bus = matches!(out, Out::Bus(b) if x >= AUX && app.auxes.get(x - AUX).is_some_and(|a| a.bus == b));
        if !own_bus {
            crate::surround::set_output(app, x, out);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// groups: several per strip

/// Every group strip `s` is in (its first, then the others).
pub fn groups_of(app: &App, s: usize) -> Vec<usize> {
    let mut v: Vec<usize> = app.mix.group_of.get(&s).copied().into_iter().collect();
    for g in app.tm.mx.more_groups.get(&s).into_iter().flatten() {
        if !v.contains(g) {
            v.push(*g);
        }
    }
    v
}

/// Choosing group `g` in the Group slot's menu: a group the strip is already in is left;
/// with Shift the group is added to the others; else it becomes the strip's only group.
pub fn choose_group(app: &mut App, s: usize, g: usize, shift: bool) {
    let cur = groups_of(app, s);
    let mut new: Vec<usize> = if cur.contains(&g) { cur.into_iter().filter(|x| *x != g).collect() } else if shift { cur.into_iter().chain([g]).collect() } else { vec![g] };
    new.dedup();
    set_groups(app, s, &new);
    app.tm.mx.last_groups = new;
}

pub fn set_groups(app: &mut App, s: usize, groups: &[usize]) {
    match groups.first() {
        Some(g) => {
            app.mix.group_of.insert(s, *g);
        }
        None => {
            app.mix.group_of.remove(&s);
        }
    }
    if groups.len() > 1 {
        app.tm.mx.more_groups.insert(s, groups[1..].to_vec());
    } else {
        app.tm.mx.more_groups.remove(&s);
    }
    app.dirty = true;
}

/// The Group slot's text: the group's name, or the numbers of all the strip's groups.
pub fn group_label(app: &App, s: usize) -> Option<String> {
    let gs = groups_of(app, s);
    match gs.len() {
        0 => None,
        1 => app.mix.groups.get(gs[0]).map(|x| x.label(gs[0])),
        _ => Some(gs.iter().map(|g| (g + 1).to_string()).collect::<Vec<_>>().join(", ")),
    }
}

/// The Group slot's colour: yellow while groups are active, light grey when they are off.
pub fn group_color(app: &App) -> Color32 {
    if app.mix.groups_enabled { Color32::from_rgb(0xc8, 0xa8, 0x28) } else { Color32::from_gray(0x8a) }
}

/// The other strips linked to `s` through any of its active groups that follow `link`.
pub fn linked(app: &App, s: usize, link: (usize, u8)) -> Vec<usize> {
    if !app.mix.groups_enabled {
        return Vec::new();
    }
    let mut v = Vec::new();
    for g in groups_of(app, s) {
        if !app.mix.groups.get(g).is_some_and(|x| x.on && x.links(link)) {
            continue;
        }
        let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
        for o in (0..n).chain((0..app.auxes.len()).map(|a| AUX + a)) {
            if o != s && groups_of(app, o).contains(&g) && !v.contains(&o) {
                v.push(o);
            }
        }
    }
    v.sort();
    v
}

/// The automation mode chosen on a strip or header: all the selected ones take it, and a
/// group linking Automation Mode takes its members along.
pub fn set_auto_mode(app: &mut App, s: usize, mode: crate::automation::AutoMode) {
    let mut targets: Vec<usize> = if app.mix.selected.contains(&s) || app.ta.selected.contains(&s) { app.mix.selected.iter().copied().chain(app.ta.selected.iter().copied()).collect() } else { vec![s] };
    for t in targets.clone() {
        targets.extend(linked(app, t, mixstate::L_AUTOMATION));
    }
    targets.sort();
    targets.dedup();
    for t in targets.into_iter().filter(|t| is_track(*t)) {
        app.auto_mode.insert(t, mode);
    }
}

/// The Automation Mode pop-up on a channel strip.
pub fn auto_mode_menu(app: &mut App, resp: &egui::Response, s: usize) {
    if !is_track(s) {
        return;
    }
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(150.0);
        let cur = app.auto_mode.get(&s).copied().unwrap_or_default();
        for m in crate::automation::AutoMode::ALL {
            if ui.add(egui::Button::selectable(cur == m, m.name())).clicked() {
                set_auto_mode(app, s, m);
                ui.close();
            }
        }
        ui.separator();
        let set = crate::tm::set(app, s).cloned().unwrap_or_default();
        if ui.add(egui::Button::selectable(set.auto_trim, "Trim")).clicked() {
            if let Some(x) = crate::tm::set_mut(app, s) {
                x.auto_trim = !set.auto_trim;
                x.auto_relative = false;
            }
            ui.close();
        }
        if ui.add(egui::Button::selectable(set.auto_relative, "Relative")).clicked() {
            if let Some(x) = crate::tm::set_mut(app, s) {
                x.auto_relative = !set.auto_relative;
                x.auto_trim = false;
            }
            ui.close();
        }
    });
}

/// The automation mode's name as the strip and header show it: T-Touch, R-Latch…
pub fn auto_mode_label(app: &App, t: usize) -> String {
    let m = app.auto_mode.get(&t).copied().unwrap_or_default();
    use crate::automation::AutoMode as A;
    let set = crate::tm::set(app, t);
    let trim = set.is_some_and(|s| s.auto_trim) && matches!(m, A::Touch | A::Latch);
    let rel = set.is_some_and(|s| s.auto_relative) && matches!(m, A::Touch | A::Latch | A::Write);
    format!("{}{}", if trim { "T-" } else if rel { "R-" } else { "" }, m.name())
}

// ---------------------------------------------------------------------------------------------
// the name, colour, buttons

/// The strip's name field: its colour, the name in two lines (cut in the middle when long),
/// the track number, a stack's disclosure triangle. Double-click renames (all selected
/// strips; a trailing number counts up from left to right); dragging it sideways moves the
/// strip (and its track).
pub fn name_field(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, nr: Rect, s: usize, name: &str, base: Color32) -> egui::Response {
    let id = ui.scope_id().with(("stripname2", s));
    let color = if comp(app, "Track Color") { crate::trackhdr::strip_color(app, s).unwrap_or(base) } else { base };
    let flash = flashing(app, ui, s);
    p.rect_filled(nr, 4.0, if flash { Color32::WHITE } else { color.gamma_multiply(0.35) });
    if let Some((_, text)) = app.tm.mx.renaming.as_mut().filter(|x| x.0 == s) {
        let te = ui.put(nr, egui::TextEdit::singleline(text).font(look::regular(11.0)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            let new = text.trim().to_string();
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            app.tm.mx.renaming = None;
            if !cancel && !new.is_empty() {
                rename_strips(app, s, &new);
            }
        }
        return ui.interact(Rect::NOTHING, id, Sense::hover());
    }
    let narrow = nr.width() < 70.0;
    let per_line = if narrow { 11 } else { 14 };
    let mut text_r = nr;
    // the track number at the left, a stack's triangle at the right
    if comp(app, "Track Number") && is_track(s) {
        p.text(pos2(nr.min.x + 3.0, nr.center().y), Align2::LEFT_CENTER, format!("{}", s + 1), look::regular(9.0), look::text_dim());
        text_r.min.x += 12.0;
    }
    let stack = is_track(s).then(|| app.project.as_ref().and_then(|pr| pr.tracks.get(s)).and_then(|t| t.extra.stack.map(|st| (st, t.extra.id)))).flatten();
    if let (Some((st, tid)), true) = (stack, comp(app, "Track Stacks")) {
        let tr = Rect::from_min_size(pos2(nr.max.x - 13.0, nr.min.y), vec2(13.0, nr.height()));
        text_r.max.x -= 12.0;
        let open = stack_open(app, st.open, tid);
        glyphs::paint(p, tr, if open { ph::CARET_DOWN } else { ph::CARET_RIGHT }, 9.0, look::text());
        if ui.interact(tr, id.with("stack"), Sense::click()).on_hover_text("Show or hide the subtracks").clicked() {
            toggle_stack(app, s, tid, !open);
        }
    }
    let ink = if flash { Color32::BLACK } else { look::text() };
    let (a, b) = trackmix::two_lines(name, per_line);
    if b.is_empty() {
        p.with_clip_rect(text_r).text(text_r.center(), Align2::CENTER_CENTER, a, look::regular(11.0), ink);
    } else {
        p.with_clip_rect(text_r).text(text_r.center() - vec2(0.0, 5.0), Align2::CENTER_CENTER, a, look::regular(9.0), ink);
        p.with_clip_rect(text_r).text(text_r.center() + vec2(0.0, 4.5), Align2::CENTER_CENTER, b, look::regular(9.0), ink);
    }
    let resp = ui.interact(text_r, id, Sense::click_and_drag());
    if resp.double_clicked() && s != MASTER {
        app.tm.mx.renaming = Some((s, name.to_string()));
    }
    // dragging the name sideways: the strip moves to where it is dropped (Tracks view)
    if resp.drag_started() && is_track(s) {
        app.tm.mx.reorder = Some(s);
    }
    if resp.dragged() && app.tm.mx.reorder == Some(s) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(pp) = ui.ctx().pointer_latest_pos() {
            ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("strip-drag"))).text(pp + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, name, look::regular(11.0), look::text());
        }
    }
    if resp.drag_stopped() {
        if let (Some(from), Some(pp)) = (app.tm.mx.reorder.take(), ui.ctx().pointer_latest_pos()) {
            let hit = app.tm.mx.strip_rects.iter().find(|(r, _)| r.x_range().contains(pp.x)).map(|(r, t)| (*t, pp.x > r.center().x));
            if let Some((to, after)) = hit.filter(|(to, _)| is_track(*to) && *to != from) {
                move_strip(app, from, to, after);
            }
        }
    }
    resp
}

fn stack_open(app: &App, track_open: bool, id: u64) -> bool {
    if app.tm.mx.follow_stacks { track_open } else { !app.tm.mx.closed_stacks.contains(&id) }
}

fn toggle_stack(app: &mut App, t: usize, id: u64, open: bool) {
    if app.tm.mx.follow_stacks {
        if let Some(st) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|x| x.extra.stack.as_mut()) {
            st.open = open;
        }
    } else if open {
        app.tm.mx.closed_stacks.remove(&id);
    } else {
        app.tm.mx.closed_stacks.insert(id);
    }
}

/// Moves strip `from` (a track) before or after the track `to`; several selected strips move
/// together to the new place.
pub fn move_strip(app: &mut App, from: usize, to: usize, after: bool) {
    let mut moving: Vec<usize> = app.mix.selected.iter().copied().filter(|s| is_track(*s)).collect();
    if !moving.contains(&from) {
        moving = vec![from];
    }
    moving.sort();
    if moving.contains(&to) {
        return;
    }
    let Some(p) = app.project.as_ref() else { return };
    // (strips inside and outside a stack don't move together)
    let depth = p.tracks[from].extra.depth;
    moving.retain(|t| p.tracks[*t].extra.depth == depth);
    let to_id = p.tracks[to].extra.id;
    app.checkpoint("Move Channel Strip");
    let Some(p) = app.project.as_mut() else { return };
    let mut taken = Vec::new();
    for &t in moving.iter().rev() {
        let g = lg_core::arrange::group(&p.tracks, t);
        let part: Vec<lg_core::Track> = p.tracks.drain(g).collect();
        taken.insert(0, part);
    }
    let Some(at) = p.tracks.iter().position(|t| t.extra.id == to_id) else { return };
    let at = if after { lg_core::arrange::group(&p.tracks, at).end } else { at };
    let flat: Vec<lg_core::Track> = taken.into_iter().flatten().collect();
    for (k, t) in flat.into_iter().enumerate() {
        p.tracks.insert(at + k, t);
    }
    app.mix.selected.clear();
    app.after_edit();
}

/// A strip (and every selected one) renamed: a trailing number counts up from left to right;
/// a track's strip renames its track.
pub fn rename_strips(app: &mut App, s: usize, new: &str) {
    let mut sel: Vec<usize> = app.mix.selected.iter().copied().filter(|x| *x != MASTER).collect();
    if !sel.contains(&s) {
        sel = vec![s];
    }
    // left to right: the tracks, the auxes, the VCAs (the Mixer's order)
    sel.sort();
    let names = trackmix::numbered_names(new, sel.len());
    app.checkpoint("Rename Channel Strip");
    for (x, name) in sel.into_iter().zip(names) {
        if x >= VCA {
            if let Some(v) = app.mix.vcas.get_mut(x - VCA) {
                v.name = name;
            }
        } else if x >= AUX {
            if let Some(a) = app.auxes.get_mut(x - AUX) {
                a.name = name;
            }
        } else if let Some(t) = app.project.as_mut().and_then(|p| p.tracks.get_mut(x)) {
            t.name = name;
            t.extra.set.renamed = true;
        }
    }
    app.dirty = true;
}

/// The strip's background: a click on an empty part selects it (Shift / Command add);
/// Control-click gives the Channel Strip Components. Called before the strip's controls.
pub fn strip_bg(app: &mut App, ui: &mut egui::Ui, r: Rect, s: usize) {
    app.tm.mx.strip_rects.push((r, s));
    let resp = ui.interact(r, ui.scope_id().with(("stripbg", s)), Sense::click());
    if resp.clicked() {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        mixstate::select(app, s, add);
    }
    resp.context_menu(|ui| {
        ui.set_min_width(210.0);
        let comps = crate::menus::context("mixer-strip-name").iter().find(|x| x.label == "Channel Strip Components").cloned();
        if let Some(c) = comps {
            components_menu(app, ui, &c);
        }
        if ui.button("Configure Channel Strip Components…").clicked() {
            app.tm.mx.win.components = true;
            ui.close();
        }
    });
}

/// The rows at the strip's top: the Type and Number label, the Notes area.
pub fn top_rows(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, mut y: f32, s: usize) -> f32 {
    if comp(app, "Type and Number Label") {
        p.text(pos2(r.center().x, y + 6.0), Align2::CENTER_CENTER, type_label(app, s), look::regular(9.5), look::text_dim());
        y += 14.0;
    }
    if comp(app, "Track Notes") && is_track(s) {
        let nr = Rect::from_min_size(pos2(r.min.x + 6.0, y), vec2(r.width() - 12.0, 34.0));
        p.rect_filled(nr, 3.0, look::lcd_bg());
        if app.tm.mx.notes_edit == Some(s) {
            let mut text = app.track_notes.get(&s).cloned().unwrap_or_default();
            let te = ui.put(nr, egui::TextEdit::multiline(&mut text).font(look::regular(9.5)).desired_rows(2));
            crate::widgets::focus_once(ui, &te);
            if te.changed() {
                // (the same notes as the Note Pad's Track tab)
                if text.is_empty() {
                    app.track_notes.remove(&s);
                } else {
                    app.track_notes.insert(s, text);
                }
                app.dirty = true;
            }
            if te.lost_focus() {
                app.tm.mx.notes_edit = None;
            }
        } else {
            let text = app.track_notes.get(&s).cloned().unwrap_or_default();
            let g = p.layout(text, look::regular(9.5), look::text(), nr.width() - 6.0);
            p.with_clip_rect(nr).galley(nr.min + vec2(3.0, 2.0), g, look::text());
            if ui.interact(nr, ui.scope_id().with(("notes", s)), Sense::click()).on_hover_text("Track notes (double-click to edit)").double_clicked() {
                app.tm.mx.notes_edit = Some(s);
            }
        }
        y += 38.0;
    }
    y
}

/// "Audio 1", "Inst 3", "Aux 2", "Stereo Out": the strip's type and number.
pub fn type_label(app: &App, s: usize) -> String {
    if s == MASTER {
        return "Stereo Out".into();
    }
    if s >= VCA {
        return format!("VCA {}", s - VCA + 1);
    }
    if s >= AUX {
        return format!("Aux {}", s - AUX + 1);
    }
    let Some(p) = app.project.as_ref() else { return String::new() };
    let Some(t) = p.tracks.get(s) else { return String::new() };
    if let Some(e) = &t.extra.set.external {
        return format!("MIDI {}", e.channel);
    }
    let audio = t.kind == TrackKind::Audio;
    let n = p.tracks[..=s].iter().filter(|o| (o.kind == TrackKind::Audio) == audio && o.extra.set.external.is_none()).count();
    format!("{} {n}", if audio { "Audio" } else { "Inst" })
}

/// The small buttons beside the fader: Record Enable and Input Monitoring on audio strips
/// (dimmed with No Input); on the output strip Bounce and Dim in the Mute / Solo places.
pub fn strip_buttons(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, by: f32, mr: Rect, sr: Rect, s: usize, kind: Option<TrackKind>) {
    let id = ui.scope_id().with(("stripbtn", s));
    if s == MASTER {
        let resp = ui.interact(mr, id.with("bnc"), Sense::click()).on_hover_text("Bounce the output to an audio file");
        p.rect_filled(mr, 4.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
        p.text(mr.center(), Align2::CENTER_CENTER, "Bnc", look::bold(9.5), look::text());
        if resp.clicked() {
            app.bounce(None);
        }
        let on = app.tm.mx.dim;
        let resp = ui.interact(sr, id.with("dim"), Sense::click()).on_hover_text("Dim: the outputs lowered by the Dim Level (Settings ▸ Audio ▸ General)");
        p.rect_filled(sr, 4.0, if on { Color32::from_rgb(0xe8, 0x9a, 0x2c) } else { look::group_bg() });
        p.text(sr.center(), Align2::CENTER_CENTER, "Dim", look::bold(9.5), if on { Color32::BLACK } else { look::text() });
        if resp.clicked() {
            app.tm.mx.dim = !on;
            crate::mixengine::sync(app);
        }
        return;
    }
    if kind != Some(TrackKind::Audio) || r.width() < 80.0 {
        return;
    }
    let no_input = app.mix.inputs.get(&s) == Some(&crate::mixstate::Input::None);
    let blink = (ui.input(|i| i.time) * 2.5).fract() < 0.5;
    let x = r.min.x + 3.0;
    let rr = Rect::from_min_size(pos2(x, by - 44.0), vec2(15.0, 16.0));
    let ir = Rect::from_min_size(pos2(x, by - 24.0), vec2(15.0, 16.0));
    let armed = app.armed.contains(&s);
    let red = look::record_red();
    let implied = !armed && app.armed.is_empty() && app.selected_track == Some(s);
    p.rect_filled(rr, 3.0, if armed && (blink || app.recording()) { red } else if armed { red.gamma_multiply(0.5) } else { look::group_bg() });
    p.text(rr.center(), Align2::CENTER_CENTER, "R", look::bold(10.0), if no_input { look::text_dim().gamma_multiply(0.5) } else if armed { Color32::WHITE } else if implied { red } else { look::text() });
    if ui.interact(rr, id.with("r"), Sense::click()).on_hover_text("Record Enable").clicked() && !no_input {
        if armed {
            app.armed.remove(&s);
        } else {
            app.armed.insert(s);
            for o in linked(app, s, mixstate::L_RECORD) {
                app.armed.insert(o);
            }
        }
    }
    let mon = app.monitoring.contains(&s);
    let orange = Color32::from_rgb(0xe8, 0x9a, 0x2c);
    p.rect_filled(ir, 3.0, if mon { orange } else { look::group_bg() });
    p.text(ir.center(), Align2::CENTER_CENTER, "I", look::bold(10.0), if no_input { look::text_dim().gamma_multiply(0.5) } else if mon { Color32::WHITE } else { look::text() });
    if ui.interact(ir, id.with("i"), Sense::click()).on_hover_text("Input Monitoring").clicked() && !no_input {
        let on = !mon;
        for o in linked(app, s, mixstate::L_INPUT).into_iter().chain([s]) {
            if on {
                app.monitoring.insert(o);
            } else {
                app.monitoring.remove(&o);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// which strips show

/// The Mixer's view options applied to the strips about to be drawn.
pub fn filter_strips(app: &mut App, strips: &mut Vec<usize>) {
    app.tm.mx.strip_rects.clear();
    let Some(p) = app.project.as_ref() else { return };
    let mx = &app.tm.mx;
    let inside = crate::trackarea::inside(app);
    let sending: HashSet<usize> = app.sends.iter().filter(|(_, v)| !v.is_empty()).map(|(s, _)| *s).collect();
    let send_buses: HashSet<usize> = app.sends.values().flatten().map(|s| s.bus).collect();
    strips.retain(|&s| {
        if s >= VCA {
            return true;
        }
        if s >= AUX {
            let Some(a) = app.auxes.get(s - AUX) else { return false };
            if !mx.signal_flow && app.mixer_view == crate::app::MixerView::Tracks {
                return false;
            }
            return !mx.sends_only || send_buses.contains(&a.bus);
        }
        let Some(t) = p.tracks.get(s) else { return false };
        // Follow Hide: hidden tracks' strips are hidden too (while the Hide view doesn't show them)
        if mx.follow_hide && t.extra.hidden && !app.ta.show_hidden {
            return false;
        }
        // tracks sharing another's channel strip have no strip of their own
        if t.extra.set.same_strip.is_some() && !mx.same_strip_tracks {
            return false;
        }
        // Follow Track Stacks: a closed stack's subtracks
        if lg_core::arrange::ancestors(&p.tracks, s).into_iter().any(|a| p.tracks[a].extra.stack.is_some_and(|st| !stack_open(app, st.open, p.tracks[a].extra.id))) {
            return false;
        }
        // inside a folder: only its tracks
        if let Some(f) = inside {
            if !lg_core::arrange::ancestors(&p.tracks, s).contains(&f) {
                return false;
            }
        }
        !mx.sends_only || sending.contains(&s)
    });
    // Autoscroll to Selection: the strip of the track just selected comes into view
    if app.tm.mx.autoscroll && app.selected_track != app.tm.mx.last_selected {
        if let Some(k) = app.selected_track.and_then(|t| strips.iter().position(|s| *s == t)) {
            let first = app.mix.scroll.round() as usize;
            if k < first {
                app.mix.scroll = k as f32;
            } else if k >= first + 6 {
                app.mix.scroll = (k as f32 - 5.0).max(0.0);
            }
        }
    }
    app.tm.mx.last_selected = app.selected_track;
}

/// Option-click a Channel Strip Type button: only that type shows; again: all.
pub fn solo_kind(app: &mut App, k: Kind) {
    const ALL: [Kind; 8] = [Kind::Audio, Kind::Inst, Kind::Aux, Kind::Bus, Kind::Input, Kind::Output, Kind::MasterVca, Kind::Midi];
    let only: HashSet<Kind> = ALL.into_iter().filter(|x| *x != k).collect();
    if app.mix.hidden == only {
        app.mix.hidden = app.tm.mx.solo_kind.take().unwrap_or_default();
    } else {
        app.tm.mx.solo_kind = Some(std::mem::replace(&mut app.mix.hidden, only));
    }
}

/// Shift-Left / Shift-Right: the strip selection grows that way.
pub fn extend_selection(app: &mut App, right: bool) {
    let all = mixstate::all_strips(app);
    let pos: Vec<usize> = app.mix.selected.iter().filter_map(|s| all.iter().position(|x| x == s)).collect();
    let next = if right { pos.iter().max().map(|k| k + 1) } else { pos.iter().min().and_then(|k| k.checked_sub(1)) };
    match next.and_then(|k| all.get(k)) {
        Some(s) => {
            app.mix.selected.insert(*s);
        }
        None if pos.is_empty() => {
            if let Some(t) = app.selected_track {
                app.mix.selected.insert(t);
            }
        }
        None => {}
    }
}

// ---------------------------------------------------------------------------------------------
// commands

fn targets(app: &App) -> Vec<usize> {
    if !app.mix.selected.is_empty() {
        return app.mix.selected.iter().copied().collect();
    }
    app.selected_track.into_iter().collect()
}

/// Toggle Level of … Channel Strips: the first use takes every strip of that kind fully down
/// (their levels kept); the next brings them back.
pub fn toggle_level(app: &mut App, kind: Kind) {
    let strips: Vec<usize> = mixstate::all_strips(app).into_iter().filter(|s| *s != MASTER && mixstate::kind_of(app, *s) == kind).collect();
    if strips.is_empty() {
        return;
    }
    mixstate::checkpoint(app, "Toggle Level");
    let down = strips.iter().any(|s| crate::tm::strip(app, *s).toggled_level.is_none());
    for s in strips {
        let Some(v) = mixstate::values(app, s) else { continue };
        if down {
            if crate::tm::strip(app, s).toggled_level.is_none() {
                crate::tm::edit_strip(app, s, |x| x.toggled_level = Some(v.0));
                mixstate::set_values(app, s, (-96.0, v.1, v.2, v.3));
            }
        } else if let Some(old) = crate::tm::strip(app, s).toggled_level {
            // (the level it has now becomes the other state)
            crate::tm::edit_strip(app, s, |x| x.toggled_level = None);
            mixstate::set_values(app, s, (old, v.1, v.2, v.3));
        }
    }
    app.load_engine();
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if crate::mixwin::command(app, ctx, name) {
        return true;
    }
    if let Some(n) = name.strip_prefix("Select Members of Group ").and_then(|x| x.parse::<usize>().ok()) {
        let g = n.wrapping_sub(1);
        if g >= app.mix.groups.len() {
            return false;
        }
        let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
        app.mix.selected = (0..n_tracks).chain((0..app.auxes.len()).map(|a| AUX + a)).filter(|s| groups_of(app, *s).contains(&g)).collect();
        return true;
    }
    if let Some(n) = name.strip_prefix("Toggle Hide Group ").and_then(|x| x.parse::<usize>().ok()) {
        let g = n.wrapping_sub(1);
        let Some(p) = app.project.as_ref() else { return false };
        let members: Vec<usize> = (0..p.tracks.len()).filter(|s| groups_of(app, *s).contains(&g)).collect();
        if members.is_empty() {
            return false;
        }
        let hide = !members.iter().all(|t| p.tracks[*t].extra.hidden);
        if let Some(p) = app.project.as_mut() {
            for t in members {
                p.tracks[t].extra.hidden = hide;
            }
        }
        app.tm.ui.hide_button = true;
        return true;
    }
    match name {
        "Toggle Level of Audio Channel Strips" => toggle_level(app, Kind::Audio),
        "Toggle Level of Auxiliary Channel Strips" => toggle_level(app, Kind::Aux),
        "Toggle Level of Output Channel Strips" => toggle_level(app, Kind::Output),
        "Toggle Level of Input Channel Strips" => toggle_level(app, Kind::Input),
        "Toggle Level of Bus Channel Strips" => toggle_level(app, Kind::Bus),
        "Toggle Pre-Fader Metering" => {
            app.mix.pre_fader_meters = !app.mix.pre_fader_meters;
            app.load_engine();
        }
        "Clear Overload Flag in Audio Channel Display" => reset_peaks(app),
        "Toggle Channel Strip Format (mono/stereo)" => {
            for s in targets(app) {
                let f = crate::tm::strip(app, s).format;
                set_format(app, s, if f == InputFormat::Mono { InputFormat::Stereo } else { InputFormat::Mono });
            }
        }
        "Toggle Dim" | "Dim" => {
            app.tm.mx.dim = !app.tm.mx.dim;
            crate::mixengine::sync(app);
        }
        "Toggle Low Latency Monitoring Mode" => {
            app.low_latency = !app.low_latency;
            crate::mixengine::sync(app);
        }
        "Set Plug-in Latency Compensation Mode" => {
            app.mix.compensate = !app.mix.compensate;
            app.load_engine();
            app.tell(if app.mix.compensate { "Plug-in Latency Compensation: All" } else { "Plug-in Latency Compensation: Audio and Software Instrument Tracks" });
        }
        "Select MIDI Channel Strips" => {
            let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
            app.mix.selected = (0..n).filter(|t| mixstate::kind_of(app, *t) == Kind::Midi).collect();
        }
        "Select Summing Stack Channel Strips" => {
            let Some(p) = app.project.as_ref() else { return false };
            app.mix.selected = (0..p.tracks.len()).filter(|t| p.tracks[*t].extra.stack.is_some_and(|s| s.kind == lg_core::arrange::StackKind::Summing)).collect();
        }
        "Select Channel Strips with Same Panner Type" => {
            let Some(&s) = targets(app).first() else { return false };
            let mode = crate::tm::strip(app, s).pan_mode;
            app.mix.selected = mixstate::all_strips(app).into_iter().filter(|x| *x < VCA && crate::tm::strip(app, *x).pan_mode == mode).collect();
        }
        "Follow Hide" => app.tm.mx.follow_hide = !app.tm.mx.follow_hide,
        "Follow Track Stacks" => app.tm.mx.follow_stacks = !app.tm.mx.follow_stacks,
        "Autoscroll to Selection" => app.tm.mx.autoscroll = !app.tm.mx.autoscroll,
        "Channels with Sends only" => app.tm.mx.sends_only = !app.tm.mx.sends_only,
        "Signal Flow Channel Strips" | "Show/Hide Signal Flow Channel Strips" => app.tm.mx.signal_flow = !app.tm.mx.signal_flow,
        "All Tracks with Same Channel Strip/Instrument" | "Show/Hide All Tracks with Same Channel Strip/Instrument" => app.tm.mx.same_strip_tracks = !app.tm.mx.same_strip_tracks,
        "Long Faders" => {
            // (the Mixer grows: the faders get the extra room)
            app.tm.mx.long_faders = !app.tm.mx.long_faders;
            app.mixer_h += if app.tm.mx.long_faders { 140.0 } else { -140.0 };
            app.mixer_h = app.mixer_h.max(220.0);
        }
        "Rename Channel Strip…" => {
            let Some(&s) = targets(app).first() else { return false };
            app.tm.mx.renaming = Some((s, mixstate::strip_name(app, s)));
        }
        "Assign Channel Strip Color…" => {
            let Some(&s) = targets(app).first() else { return false };
            app.tm.ui.colors = Some(crate::trackhdr::ColorFor::Strip(s));
        }
        "Define Group Settings as Default" => {
            let Some(g) = app.mix.group_window.and_then(|g| app.mix.groups.get(g)) else { return false };
            app.tm.mx.group_default = Some(g.mask);
            app.tell("Group settings made the default for new groups");
        }
        "Extend Channel Strip Selection Left" => extend_selection(app, false),
        "Extend Channel Strip Selection Right" => extend_selection(app, true),
        "Remove All Sends" => {
            mixstate::checkpoint(app, "Remove All Sends");
            for s in targets(app) {
                app.sends.remove(&s);
                crate::tm::edit_strip(app, s, |x| x.sends.clear());
            }
            app.load_engine();
        }
        "Remove Bypassed Sends" => {
            mixstate::checkpoint(app, "Remove Bypassed Sends");
            for s in targets(app) {
                let set = crate::tm::strip(app, s);
                let n = app.sends.get(&s).map_or(0, |v| v.len());
                for k in (0..n).rev().filter(|k| set.send(*k).bypass) {
                    remove_send(app, s, k);
                }
            }
        }
        "Remove All Effect Plug-ins" => {
            mixstate::checkpoint(app, "Remove All Effect Plug-ins");
            for s in targets(app) {
                mixstate::edit_inserts(app, s, |v| v.clear());
            }
            app.mix.fx_window = None;
            app.load_engine();
        }
        "Remove Empty Insert Slots" => {
            mixstate::checkpoint(app, "Remove Empty Insert Slots");
            for s in targets(app) {
                mixstate::edit_inserts(app, s, |v| v.retain(|u| !u.name.is_empty()));
            }
            app.load_engine();
        }
        _ => return false,
    }
    true
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    let mx = &app.tm.mx;
    Some(match label {
        "Follow Hide" => mx.follow_hide,
        "Follow Track Stacks" => mx.follow_stacks,
        "Autoscroll to Selection" => mx.autoscroll,
        "Channels with Sends only" => mx.sends_only,
        "Signal Flow Channel Strips" => mx.signal_flow,
        "All Tracks with Same Channel Strip/Instrument" => mx.same_strip_tracks,
        "Long Faders" => mx.long_faders,
        _ => return None,
    })
}

pub fn enabled(app: &App, label: &str) -> Option<bool> {
    let _ = (app, label);
    None
}

/// The Channel Strip Components submenu (its items share names with other menus' items).
pub fn components_menu(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) {
    ui.menu_button(&it.label, |ui| {
        ui.set_min_width(210.0);
        for c in &it.children {
            if c.separator {
                ui.separator();
                continue;
            }
            let Some(key) = comp_key(&c.label) else {
                ui.add_enabled(false, egui::Button::new(&c.label));
                continue;
            };
            let on = comp(app, key);
            if ui.add(egui::Button::new(if on { format!("✓ {}", c.label) } else { c.label.clone() }).shortcut_text(c.shortcut.clone())).clicked() {
                set_comp(app, key, !on);
                ui.close();
            }
        }
    });
}

pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    match it.label.as_str() {
        "Channel Strip Components" if !it.children.is_empty() => {
            components_menu(app, ui, it);
            true
        }
        "MIDI Channel Strip Components" if !it.children.is_empty() => {
            ui.menu_button(&it.label, |ui| {
                for c in &it.children {
                    let key = format!("midi:{}", c.label);
                    // (Assign 3 to 5, Instrument Name, Program and Bank are off until chosen)
                    let default_on = matches!(c.label.as_str(), "Assign 1" | "Assign 2" | "Program" | "Bank");
                    let flipped = app.mix.hidden_parts.contains(&key);
                    let on = default_on != flipped;
                    if ui.button(if on { format!("✓ {}", c.label) } else { c.label.clone() }).clicked() {
                        if flipped {
                            app.mix.hidden_parts.remove(&key);
                        } else {
                            app.mix.hidden_parts.insert(key);
                        }
                        ui.close();
                    }
                }
            });
            true
        }
        _ => false,
    }
}

/// Whether a MIDI channel strip component shows.
pub fn midi_comp(app: &App, label: &str) -> bool {
    let default_on = matches!(label, "Assign 1" | "Assign 2" | "Program" | "Bank");
    default_on != app.mix.hidden_parts.contains(&format!("midi:{label}"))
}

pub fn frame(app: &mut App, ctx: &egui::Context) {
    if !ctx.input(|i| i.pointer.primary_down()) {
        app.tm.mx.swipe = None;
    }
    if let Some((_, t0)) = app.tm.mx.flash {
        if ctx.input(|i| i.time) - t0 > 0.9 {
            app.tm.mx.flash = None;
        } else {
            ctx.request_repaint();
        }
    }
    // Shift-Left / Shift-Right over the Mixer extend the strip selection
    if app.bottom == crate::app::Bottom::Mixer && !ctx.egui_wants_keyboard_input() && !app.mix.selected.is_empty() {
        let (l, r) = ctx.input(|i| (i.modifiers.shift && i.key_pressed(egui::Key::ArrowLeft), i.modifiers.shift && i.key_pressed(egui::Key::ArrowRight)));
        if l || r {
            extend_selection(app, r);
        }
    }
    crate::mixwin::frame(app, ctx);
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    crate::mixwin::windows(app, ctx);
}

pub fn sidecar_lines(app: &App) -> String {
    let mx = &app.tm.mx;
    let mut out = String::new();
    let mut strips: Vec<&usize> = mx.more_groups.keys().collect();
    strips.sort();
    for s in strips {
        out += &format!("groups\t{s}\t{}\n", mx.more_groups[s].iter().map(|g| g.to_string()).collect::<Vec<_>>().join(","));
    }
    let d = State::default();
    let flags = [("follow_hide", mx.follow_hide, d.follow_hide), ("follow_stacks", mx.follow_stacks, d.follow_stacks), ("autoscroll", mx.autoscroll, d.autoscroll), ("sends_only", mx.sends_only, d.sends_only), ("signal_flow", mx.signal_flow, d.signal_flow), ("same_strip_tracks", mx.same_strip_tracks, d.same_strip_tracks)];
    let changed: Vec<String> = flags.iter().filter(|f| f.1 != f.2).map(|f| format!("{}={}", f.0, f.1 as u8)).collect();
    if !changed.is_empty() {
        out += &format!("mixview\t{}\n", changed.join(" "));
    }
    out
}

pub fn apply_sidecar_line(app: &mut App, kind: &str, a: &str, b: &str) -> bool {
    match kind {
        "groups" => {
            if let Ok(s) = a.parse::<usize>() {
                app.tm.mx.more_groups.insert(s, b.split(',').filter_map(|g| g.parse().ok()).collect());
            }
            true
        }
        "mixview" => {
            for w in a.split_whitespace().chain(b.split_whitespace()) {
                let Some((k, v)) = w.split_once('=') else { continue };
                let on = v == "1";
                let mx = &mut app.tm.mx;
                match k {
                    "follow_hide" => mx.follow_hide = on,
                    "follow_stacks" => mx.follow_stacks = on,
                    "autoscroll" => mx.autoscroll = on,
                    "sends_only" => mx.sends_only = on,
                    "signal_flow" => mx.signal_flow = on,
                    "same_strip_tracks" => mx.same_strip_tracks = on,
                    _ => {}
                }
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_default_on_and_off() {
        let mut app = crate::tm::test_app();
        assert!(comp(&app, "Sends") && comp(&app, "Pan Knob") && comp(&app, "Track Color"));
        assert!(!comp(&app, "Track Number") && !comp(&app, "Track Notes") && !comp(&app, "Type and Number Label"));
        set_comp(&mut app, "Track Number", true);
        set_comp(&mut app, "Sends", false);
        assert!(comp(&app, "Track Number") && !comp(&app, "Sends"));
        for l in ["Audio Device Controls", "Type and Number Label", "Setting Menu", "Gain Reduction Meter", "EQ Thumbnail", "MIDI Effects", "Input / Instrument", "Audio Effects", "Sends", "Output", "Group", "Automation", "Icon", "Pan Knob", "VCA", "Track Color", "Track Stacks", "Track Name", "Track Number", "Control Surface Bars", "Track Notes", "Notes"] {
            assert!(comp_key(l).is_some(), "{l}");
        }
        assert!(midi_comp(&app, "Assign 1") && !midi_comp(&app, "Assign 3"));
    }

    #[test]
    fn several_groups_per_strip() {
        let mut app = crate::tm::test_app();
        for _ in 0..3 {
            app.mix.groups.push(mixstate::Group::new());
        }
        choose_group(&mut app, 0, 0, false);
        choose_group(&mut app, 0, 2, true);
        assert_eq!(groups_of(&app, 0), vec![0, 2]);
        assert_eq!(group_label(&app, 0).unwrap(), "1, 3");
        choose_group(&mut app, 1, 2, false);
        // group 3 links volume by default: strip 1 follows strip 0 through it
        assert!(linked(&app, 0, mixstate::L_VOLUME).contains(&1));
        // choosing a group the strip is in takes it out
        choose_group(&mut app, 0, 2, true);
        assert_eq!(groups_of(&app, 0), vec![0]);
        assert!(!linked(&app, 0, mixstate::L_VOLUME).contains(&1));
        choose_group(&mut app, 0, 1, false);
        assert_eq!(groups_of(&app, 0), vec![1], "without Shift: the only group");
        assert_eq!(app.tm.mx.last_groups, vec![1]);
        app.mix.groups_enabled = false;
        assert!(linked(&app, 1, mixstate::L_VOLUME).is_empty());
    }

    #[test]
    fn toggle_level_takes_the_faders_down_and_back() {
        let mut app = crate::tm::test_app();
        let audio: Vec<usize> = mixstate::all_strips(&app).into_iter().filter(|s| *s != MASTER && mixstate::kind_of(&app, *s) == Kind::Inst).collect();
        assert!(!audio.is_empty());
        let before: Vec<f32> = audio.iter().map(|s| mixstate::values(&app, *s).unwrap().0).collect();
        toggle_level(&mut app, Kind::Inst);
        assert!(audio.iter().all(|s| mixstate::values(&app, *s).unwrap().0 <= -90.0));
        toggle_level(&mut app, Kind::Inst);
        let after: Vec<f32> = audio.iter().map(|s| mixstate::values(&app, *s).unwrap().0).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn renaming_selected_strips_counts_up_and_renames_tracks() {
        let mut app = crate::tm::test_app();
        app.mix.selected = [0, 1].into_iter().collect();
        rename_strips(&mut app, 1, "Drums 1");
        let p = app.project.as_ref().unwrap();
        assert_eq!((p.tracks[0].name.as_str(), p.tracks[1].name.as_str()), ("Drums 1", "Drums 2"));
        app.undo();
        assert_ne!(app.project.as_ref().unwrap().tracks[0].name, "Drums 1");
    }

    #[test]
    fn view_options_filter_the_strips() {
        let mut app = crate::tm::test_app();
        let n = app.project.as_ref().unwrap().tracks.len();
        let all: Vec<usize> = (0..n).collect();
        app.project.as_mut().unwrap().tracks[1].extra.hidden = true;
        let mut s = all.clone();
        filter_strips(&mut app, &mut s);
        assert!(!s.contains(&1), "Follow Hide: the hidden track's strip is gone");
        app.tm.mx.follow_hide = false;
        let mut s = all.clone();
        filter_strips(&mut app, &mut s);
        assert!(s.contains(&1));
        // a track sharing a strip has none of its own
        let id = app.project.as_ref().unwrap().tracks[0].extra.id;
        app.project.as_mut().unwrap().tracks[2].extra.set.same_strip = Some(id);
        let mut s = all.clone();
        filter_strips(&mut app, &mut s);
        assert!(!s.contains(&2));
        // Channels with Sends only
        app.tm.mx.sends_only = true;
        app.add_send(0, 3);
        let mut s: Vec<usize> = all.iter().copied().chain((0..app.auxes.len()).map(|a| AUX + a)).collect();
        filter_strips(&mut app, &mut s);
        assert_eq!(s.iter().filter(|x| **x < AUX).copied().collect::<Vec<_>>(), vec![0]);
        assert!(s.iter().any(|x| *x >= AUX), "the aux its bus feeds stays");
    }

    #[test]
    fn option_click_shows_one_type_alone() {
        let mut app = crate::tm::test_app();
        app.mix.hidden.insert(Kind::Midi);
        solo_kind(&mut app, Kind::Aux);
        assert!(!app.mix.hidden.contains(&Kind::Aux) && app.mix.hidden.len() == 7);
        solo_kind(&mut app, Kind::Aux);
        assert_eq!(app.mix.hidden, [Kind::Midi].into_iter().collect());
    }

    #[test]
    fn moving_a_strip_moves_its_track() {
        let mut app = crate::tm::test_app();
        let names: Vec<String> = app.project.as_ref().unwrap().tracks.iter().map(|t| t.name.clone()).collect();
        assert!(names.len() >= 3);
        move_strip(&mut app, 0, 2, true);
        let now: Vec<String> = app.project.as_ref().unwrap().tracks.iter().map(|t| t.name.clone()).collect();
        assert_eq!(now[2], names[0]);
        assert_eq!(now[0], names[1]);
        app.undo();
        let back: Vec<String> = app.project.as_ref().unwrap().tracks.iter().map(|t| t.name.clone()).collect();
        assert_eq!(back, names);
    }

    #[test]
    fn send_modes_reach_the_engine() {
        let mut app = crate::tm::test_app();
        app.add_send(0, 2);
        crate::tm::edit_strip(&mut app, 0, |x| x.send_mut(0).post_pan = true);
        app.load_engine();
        assert!(app.engine.lock().unwrap().ext.tracks[0].sends[0].post_pan);
        remove_send(&mut app, 0, 0);
        assert!(app.sends.get(&0).is_none_or(|v| v.is_empty()));
        assert!(crate::tm::strip(&app, 0).sends.is_empty());
        assert_eq!(fine_fader(0.0, 0.0, 100.0), crate::tracks::frac_to_db(crate::tracks::db_to_frac(0.0)).min(6.0));
        assert!(fine_fader(0.0, -10.0, 100.0) > 0.0 && fine_fader(0.0, -10.0, 100.0) < 1.0, "a fine step");
    }
}
