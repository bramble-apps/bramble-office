//! Retro Synth's window, laid out as Logic's: the Oscillator section at the upper left (engine
//! tabs Analog / Sync / Table / FM that switch the controls under them), the Filter section in
//! the middle (on button, type menu, a display whose Cutoff strip drags horizontally and
//! Resonance strip vertically, the filter modulation knobs), Amp and Effect at the upper right;
//! below, the two envelopes with draggable handles, and the Settings area: the label toggles
//! between the modulation controls (LFO / Vibrato tabs, Glide / Autobend) and the global and
//! controller settings (Transpose, Tune, Bend, Polyphony, Voice mode, controller targets that
//! depend on the engine). The wavetable menu of the Table engine, the audio-file wavetables and
//! the extended parameters (MIDI Mono Mode, Mono Mode Bend Range, Audio File Analysis) are here.

use egui::{pos2, vec2, Align2, Color32, Sense, Stroke};
use lg_dsp::Plugin;
use lg_synth::retro::{self as r, RetroSynth};

use crate::app::App;
use crate::instwin;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x2b2b2c, 0x2b2b2c, 0xe8913a).silver().texts(0xc9c9c9, 0x8c8c8c);
const ORANGE: Color32 = rgb(0xec8f35);

/// The filter menu: label, mode (1 LP, 2 HP, 3 BP, 4 BR, 5 Peak), variant, slope.
const FILTER_LIST: &[(&str, u8, u8, u8)] = &[
    ("LP 24 dB Lush", 1, 0, 3),
    ("LP 24 dB Lush (Fat)", 1, 4, 3),
    ("LP 12 dB Creamy", 1, 1, 1),
    ("LP 24 dB Creamy", 1, 1, 3),
    ("LP 12 dB Edgy", 1, 2, 1),
    ("LP 24 dB Edgy", 1, 2, 3),
    ("LP 12 dB Gritty", 1, 3, 1),
    ("LP 12 dB Sharp", 1, 5, 1),
    ("HP 6 dB", 2, 1, 0),
    ("HP 12 dB Creamy", 2, 1, 1),
    ("HP 12 dB Sharp", 2, 5, 1),
    ("HP 24 dB Lush", 2, 0, 3),
    ("BP 6 dB", 3, 1, 0),
    ("BP 12 dB", 3, 1, 1),
    ("BP 12 dB Gritty", 3, 3, 1),
    ("BP 12 dB Sharp", 3, 5, 1),
    ("BR 6 dB", 4, 0, 1),
    ("Peak", 5, 0, 1),
];

/// The Extended Parameters, and the engine values Logic's panel has no control for (filter
/// drive, effect intensity, the FM engine's extra shapes, legato glide).
pub const EXT: &[u32] = &[r::MPE_MODE, r::MPE_BEND, r::ANALYSIS_SENS, 41, 44, 23, r::POLY_LFO, 28, 30, r::FM_INHARMONIC, r::FM_SHAPE];

struct Info {
    custom: Option<(usize, String)>,
    user_names: Vec<String>,
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let info = Info {
        custom: instwin::with_inst(app, t, |s: &mut RetroSynth| s.custom_frames().map(|f| (f.len(), s.custom_name.clone()))).flatten(),
        user_names: r::wavetable::list(),
    };
    let actions = synthui::window(app, ctx, t, "Retro Synth", vec2(W, H), SKIN, EXT, |p| body(p, &info));
    // a file dropped on the Shape 1 knob of the Table engine makes a custom wavetable
    let dropped = std::mem::take(&mut app.inst.dropped);
    if let (Some(f), Some(pos)) = (dropped.first(), app.inst.drop_pos) {
        let rc: Option<egui::Rect> = ctx.data(|d| d.get_temp(egui::Id::unique(("retro-shape1", t))));
        if rc.is_some_and(|rc| rc.contains(pos)) {
            create_from(app, t, f);
        }
    }
    if actions.is_empty() {
        return;
    }
    let src = instwin::with_inst(app, t, |s: &mut RetroSynth| (s.get(r::TABLE_SOURCE).round() as i32, s.get(r::USER_TABLE) as usize, s.custom_name.clone()));
    for a in actions {
        let Some((kind, arg)) = a.split_once(':') else { continue };
        match kind {
            "app" => {
                instwin::checkpoint(app, t, "Wavetable");
                if let Ok(i) = arg.parse::<usize>() {
                    instwin::set_params(app, t, &[(r::WAVETABLE, i as f32), (r::TABLE_SOURCE, 0.0)]);
                }
            }
            "custom" => {
                instwin::checkpoint(app, t, "Wavetable");
                instwin::set_params(app, t, &[(r::TABLE_SOURCE, 1.0)]);
            }
            "user" => {
                if let Some(i) = info.user_names.iter().position(|n| n == arg) {
                    instwin::checkpoint(app, t, "Wavetable");
                    instwin::set_params(app, t, &[(r::USER_TABLE, i as f32), (r::TABLE_SOURCE, 2.0)]);
                }
            }
            "create" => {
                if let Some(f) = kn_ui::files::FileDialog::new().set_title("Create Wavetable from Audiofile").add_filter("Audio", &["wav", "aif", "aiff", "WAV", "AIF", "AIFF"]).pick_file() {
                    create_from(app, t, &f);
                }
            }
            "rev" => {
                let on = instwin::with_inst(app, t, |s: &mut RetroSynth| s.get(r::TABLE_REVERSE) >= 0.5).unwrap_or(false);
                instwin::checkpoint(app, t, "Reverse Wavetable Order");
                instwin::set_params(app, t, &[(r::TABLE_REVERSE, if on { 0.0 } else { 1.0 })]);
            }
            "save" => {
                let name = arg.trim().to_string();
                match instwin::with_inst(app, t, |s: &mut RetroSynth| s.save_custom(&name)) {
                    Some(Ok(())) => app.tell(format!("Saved the wavetable “{name}”")),
                    Some(Err(e)) => app.tell(format!("Couldn't save the wavetable: {e}")),
                    None => {}
                }
            }
            "delete" => {
                let name = match &src {
                    Some((2, i, _)) => info.user_names.get(*i).cloned(),
                    Some((_, _, n)) if !n.is_empty() => Some(n.clone()),
                    _ => None,
                };
                if let Some(n) = name {
                    let ok = instwin::with_inst(app, t, |s: &mut RetroSynth| s.delete_user(&n)).unwrap_or(false);
                    if ok {
                        instwin::checkpoint(app, t, "Delete Wavetable");
                        instwin::set_params(app, t, &[(r::TABLE_SOURCE, 0.0)]);
                        app.tell(format!("Deleted the wavetable “{n}”"));
                    }
                } else {
                    app.tell("No user wavetable is loaded");
                }
            }
            _ => {}
        }
    }
}

/// Builds the custom wavetable from an audio file (the Wavetable menu, or a file dropped on the
/// Shape 1 knob).
fn create_from(app: &mut App, t: usize, f: &std::path::Path) {
    instwin::checkpoint(app, t, "Create Wavetable");
    match instwin::with_inst(app, t, |s: &mut RetroSynth| s.wavetable_from_file(f)) {
        Some(Ok(n)) => {
            instwin::set_params(app, t, &[(r::TABLE_SOURCE, 1.0), (19, 2.0)]);
            app.tell(format!("Created a wavetable with {n} entries"));
        }
        Some(Err(e)) => app.tell(format!("Couldn't create a wavetable: {e}")),
        None => {}
    }
}

// ---------------------------------------------------------------------------------------------
// The panel: two wooden trays (the sound sections above, modulation and envelopes below) with
// the Settings button under the lower one's right corner, as Logic's.

const W: f32 = 990.0;
const H: f32 = 650.0;
const WOOD: Color32 = rgb(0x6a3f22);
const WOOD_DARK: Color32 = rgb(0x3e2412);
const CHARCOAL: Color32 = rgb(0x2b2b2c);
const TAB_BG: Color32 = rgb(0x1f1f20);
const LABEL: Color32 = rgb(0xc9c9c9);
const DIM: Color32 = rgb(0x8c8c8c);
const SECTION: Color32 = rgb(0xa9a9a9);
const LCD: Color32 = rgb(0x1d150c);
const LCD_FIELD: Color32 = rgb(0x2c1f10);
const LCD_LINE: Color32 = rgb(0xcf7f2c);

/// Upper tray: the sections' left edges (Oscillator, Filter, Amp, Effect) and its right edge.
const UX: [f32; 5] = [11.0, 449.0, 714.0, 825.0, 979.0];
/// Lower tray: Glide, LFO / Vibrato, Filter Env, Amp Env.
const LX: [f32; 5] = [11.0, 186.0, 449.0, 714.0, 979.0];
const UTOP: f32 = 13.0;
const UBOT: f32 = 348.0;
const LTOP: f32 = 372.0;
const LBOT: f32 = 608.0;

fn txt(p: &Pane, x: f32, y: f32, s: &str, size: f32, col: Color32) {
    p.text(x, y, Align2::CENTER_CENTER, s, size, col);
}

fn bold(p: &Pane, x: f32, y: f32, s: &str, size: f32, col: Color32) {
    p.ui.painter().text(p.o + vec2(x, y), Align2::CENTER_CENTER, s, look::semibold(size), col);
}

fn trays(p: &Pane, global: bool) {
    let pt = p.painter();
    pt.rect_filled(p.rect(0.0, 0.0, W, H), 8.0, rgb(0x161616));
    for (y0, y1) in [(UTOP - 9.0, UBOT + 9.0), (LTOP - 9.0, LBOT + 9.0)] {
        let r = p.rect(UX[0] - 9.0, y0, UX[4] - UX[0] + 18.0, y1 - y0);
        pt.rect_filled(r, 9.0, WOOD);
        // the grain: a few darker streaks along the tray
        for k in 0..5 {
            let y = r.min.y + 2.0 + k as f32 * 1.6;
            pt.line_segment([pos2(r.min.x + 10.0, y), pos2(r.max.x - 10.0, y)], Stroke::new(0.6, WOOD_DARK.gamma_multiply(0.5)));
            let y = r.max.y - 2.0 - k as f32 * 1.6;
            pt.line_segment([pos2(r.min.x + 10.0, y), pos2(r.max.x - 10.0, y)], Stroke::new(0.6, WOOD_DARK.gamma_multiply(0.5)));
        }
        pt.rect_stroke(r, 9.0, Stroke::new(1.0, WOOD_DARK), egui::StrokeKind::Inside);
        let inner = r.shrink(9.0);
        pt.rect_filled(inner, 4.0, CHARCOAL);
        pt.rect_stroke(inner, 4.0, Stroke::new(1.0, rgb(0x141414)), egui::StrokeKind::Outside);
    }
    // (the settings view has no section lines in the lower tray)
    let lower: &[f32] = if global { &[] } else { &LX[1..4] };
    for (xs, y0, y1) in [(&UX[1..4], UTOP, UBOT), (lower, LTOP, LBOT)] {
        for &x in xs {
            pt.line_segment([p.o + vec2(x, y0), p.o + vec2(x, y1)], Stroke::new(1.5, rgb(0x1a1a1a)));
            pt.line_segment([p.o + vec2(x + 1.5, y0), p.o + vec2(x + 1.5, y1)], Stroke::new(0.5, rgb(0x3a3a3a)));
        }
    }
    for (x, label) in [(230.0, "OSCILLATOR"), (581.0, "FILTER"), (770.0, "AMP"), (902.0, "EFFECT")] {
        p.ui.painter().text(p.o + vec2(x, 326.0), Align2::CENTER_CENTER, label, look::regular(25.0), SECTION);
    }
}

fn body(p: &mut Pane, info: &Info) {
    let st_id = egui::Id::unique(("retro-settings", p.t));
    let mut global: bool = p.ui.ctx().data(|d| d.get_temp(st_id)).unwrap_or(false);
    trays(p, global);
    let engine = p.geti(19);
    oscillator(p, engine, info);
    filter(p);
    amp_effect(p);
    if global {
        global_settings(p, engine);
    } else {
        glide(p);
        modulation(p);
        env(p, LX[2], [56, 57, 58, 59], 60, false);
        env(p, LX[3], [61, 62, 63, 64], 65, true);
    }
    // the Settings button under the lower tray's right corner switches the lower section
    let r = p.rect(894.0, 622.0, 85.0, 22.0);
    let resp = p.ui.interact(r, p.id("settings", 0), Sense::click()).on_hover_text("Switch between modulation and the global and controller settings");
    let pt = p.painter();
    pt.rect_filled(r, 4.0, if global || resp.hovered() { rgb(0x3a3a3a) } else { rgb(0x262626) });
    pt.rect_stroke(r, 4.0, Stroke::new(1.0, rgb(0x0e0e0e)), egui::StrokeKind::Outside);
    pt.text(r.center(), Align2::CENTER_CENTER, "SETTINGS", look::regular(12.5), if global { ORANGE } else { LABEL });
    if resp.clicked() {
        global = !global;
        p.ui.ctx().data_mut(|d| d.insert_temp(st_id, global));
    }
}

/// A tab bar: names across `x0..x1`, the current one lit orange.
fn tab_bar(p: &mut Pane, x0: f32, x1: f32, y: f32, h: f32, names: &[&str], cur: usize, salt: &str, size: f32) -> Option<usize> {
    let pt = p.painter();
    let bar = p.rect(x0, y, x1 - x0, h);
    pt.rect_filled(bar, 3.0, TAB_BG);
    let w = (x1 - x0) / names.len() as f32;
    let mut pick = None;
    for (k, n) in names.iter().enumerate() {
        let r = p.rect(x0 + w * k as f32, y, w, h);
        let resp = p.ui.interact(r.shrink(1.0), p.id(salt, k as u32), Sense::click());
        if k == cur {
            pt.rect_filled(r.shrink(2.0), 3.0, rgb(0x2e2925));
        } else if resp.hovered() {
            pt.rect_filled(r.shrink(2.0), 3.0, rgb(0x262626));
        }
        if k > 0 {
            pt.line_segment([pos2(r.min.x, r.min.y + 3.0), pos2(r.min.x, r.max.y - 3.0)], Stroke::new(1.0, rgb(0x343434)));
        }
        pt.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(size), if k == cur { ORANGE } else { rgb(0xb4b4b4) });
        if resp.clicked() {
            pick = Some(k);
        }
    }
    pick
}

/// A round power button (lit orange when on).
fn power(p: &mut Pane, x: f32, y: f32, s: f32, id: u32) {
    let on = p.on(id);
    let r = p.rect(x, y, s, s);
    let resp = p.ui.interact(r, p.id("power", id), Sense::click()).on_hover_text(p.prm(id).name);
    let pt = p.painter();
    pt.rect_filled(r, 4.0, if resp.hovered() { rgb(0x2a2a2a) } else { TAB_BG });
    crate::glyphs::paint(&pt, r, crate::glyphs::ph::POWER, s * 0.62, if on { ORANGE } else { rgb(0x8a8a8a) });
    if resp.clicked() {
        p.put(id, if on { 0.0 } else { 1.0 });
    }
}

/// A dark box with a label in it that opens a parameter's menu (the filter and effect types).
fn menu_box(p: &mut Pane, r: egui::Rect, text: &str, size: f32, salt: &str) -> egui::Response {
    let resp = p.ui.interact(r, p.id(salt, 0), Sense::click());
    let pt = p.painter();
    pt.rect_filled(r, 3.0, if resp.hovered() { rgb(0x262626) } else { TAB_BG });
    pt.with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, text, look::regular(size), rgb(0xb8b8b8));
    resp
}

/// Logic's vertical slider with the glossy round thumb; `ticks` draws the scale beside it.
/// `down`: the value grows downward (Logic's Mix: Osc 1 at the top).
fn vslider(p: &mut Pane, cx: f32, top: f32, bot: f32, id: u32, ticks: usize, down: bool) {
    let pr = p.prm(id);
    let v = p.get(id);
    let f = ((v - pr.min) / (pr.max - pr.min).max(1e-9)).clamp(0.0, 1.0);
    let (f, sign) = if down { (1.0 - f, -1.0) } else { (f, 1.0) };
    let r = p.rect(cx - 14.0, top - 12.0, 28.0, bot - top + 24.0);
    let resp = p.ui.interact(r, p.id("vs", id), Sense::click_and_drag()).on_hover_text(format!("{}: {}", pr.name, p.shown(id, v)));
    let pt = p.painter();
    let track = p.rect(cx - 3.5, top, 7.0, bot - top);
    for k in 0..ticks {
        let y = top + (bot - top) * k as f32 / (ticks - 1).max(1) as f32;
        for side in [-1.0f32, 1.0] {
            pt.line_segment([p.o + vec2(cx + side * 9.0, y), p.o + vec2(cx + side * 24.0, y)], Stroke::new(1.5, rgb(0x6a6a6a)));
        }
    }
    pt.rect_filled(track, 3.5, rgb(0x0f0f0f));
    pt.rect_stroke(track, 3.5, Stroke::new(1.0, rgb(0x3c3c3c)), egui::StrokeKind::Outside);
    let c = p.o + vec2(cx, bot - (bot - top) * f);
    thumb(&pt, c, resp.hovered() || resp.dragged());
    if resp.double_clicked() || (resp.clicked() && p.ui.input(|i| i.modifiers.alt)) {
        p.put(id, pr.default);
    } else if resp.dragged() {
        let fine = if p.ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
        p.put(id, (v - sign * resp.drag_delta().y / (bot - top) * (pr.max - pr.min) * fine).clamp(pr.min, pr.max));
    }
}

fn thumb(pt: &egui::Painter, c: egui::Pos2, hot: bool) {
    pt.circle_filled(c + vec2(0.0, 1.5), 12.0, Color32::from_black_alpha(110));
    pt.circle_filled(c, 12.0, rgb(0x5d6f8e));
    for k in 0..5 {
        let t = k as f32 / 4.0;
        let g = look::mix(rgb(0x7f93b5), rgb(0xd7e2f2), t);
        pt.circle_filled(c - vec2(0.5, 1.5) * (3.0 * t), 10.5 * (1.0 - 0.5 * t), g);
    }
    pt.circle_stroke(c, 12.0, Stroke::new(1.0, rgb(0x2a3340)));
    if hot {
        pt.circle_filled(c, 12.0, Color32::from_white_alpha(25));
    }
}

/// A value field inside an orange display ("Cutoff:  0.484"): drag up or down to change it,
/// Option-click or double-click for the default.
fn lcd_field(p: &mut Pane, x: f32, y: f32, label_w: f32, w: f32, id: u32, label: &str, shown: &str, per_px: f32) {
    let pr = p.prm(id);
    let v = p.get(id);
    let pt = p.painter();
    pt.text(p.o + vec2(x, y + 8.0), Align2::LEFT_CENTER, label, look::regular(12.5), LCD_LINE.gamma_multiply(0.85));
    let r = p.rect(x + label_w, y, w, 16.0);
    let resp = p.ui.interact(r, p.id("lcd", id), Sense::click_and_drag()).on_hover_text(pr.name);
    pt.rect_filled(r, 8.0, if resp.hovered() || resp.dragged() { rgb(0x3a2914) } else { LCD_FIELD });
    pt.text(r.center(), Align2::CENTER_CENTER, shown, look::regular(12.5), rgb(0xe9952f));
    if resp.double_clicked() || (resp.clicked() && p.ui.input(|i| i.modifiers.alt)) {
        p.put(id, pr.default);
    } else if resp.dragged() {
        let fine = if p.ui.input(|i| i.modifiers.shift) { 0.2 } else { 1.0 };
        p.put(id, (v - resp.drag_delta().y * per_px * fine).clamp(pr.min, pr.max));
        p.ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
}

fn lcd(p: &Pane, x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
    let r = p.rect(x, y, w, h);
    let pt = p.painter();
    pt.rect_filled(r.expand(3.0), 5.0, rgb(0x121212));
    pt.rect_filled(r, 3.0, LCD);
    pt.rect_filled(egui::Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + h * 0.35)), 3.0, Color32::from_white_alpha(4));
    r
}

// ---------------------------------------------------------------------------------------------
// Oscillator section

/// Draws a small waveform icon (0 noise, 1 rect, 2 saw, 3 tri) centred at c.
fn wave_icon(pt: &egui::Painter, c: egui::Pos2, kind: usize, col: Color32) {
    let (w, h) = (12.0, 8.0);
    let o = c - vec2(w / 2.0, h / 2.0);
    let pts: Vec<egui::Pos2> = match kind {
        0 => [0.0, 0.7, 0.2, 0.9, 0.1, 0.6, 0.3, 1.0, 0.0, 0.5].iter().enumerate().map(|(k, y)| o + vec2(k as f32 * w / 9.0, h * (1.0 - y))).collect(),
        1 => vec![o + vec2(0.0, h), o + vec2(0.0, 0.0), o + vec2(w / 2.0, 0.0), o + vec2(w / 2.0, h), o + vec2(w, h), o + vec2(w, 0.0)],
        2 => vec![o + vec2(0.0, 0.0), o + vec2(w * 0.25, 0.0), o + vec2(w, h)],
        _ => vec![o + vec2(0.0, h), o + vec2(w / 2.0, 0.0), o + vec2(w, h)],
    };
    pt.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
}

/// Logic's Shape knob: one turn through the waveforms and the pulse width. From the lower left:
/// noise, triangle, sawtooth, then the rectangle at the upper left, its pulse narrowing along
/// the arc on the right down to a thin pulse at the lower right. Option-click or double-click:
/// the default (sawtooth).
fn shape4(p: &mut Pane, cx: f32, cy: f32, wave_id: u32, noise_id: u32, pw_id: u32, name: &str) {
    let d = 62.0;
    let noise = p.on(noise_id);
    let w = p.get(wave_id).round() as i32;
    let pw = p.get(pw_id);
    let u = if noise {
        0.05
    } else {
        match w {
            0 => 0.15,
            1 => 0.25,
            _ => 0.3 + 0.7 * ((0.5 - pw) / 0.45).clamp(0.0, 1.0),
        }
    };
    let text = |u: f32| -> String {
        if u < 0.1 {
            "Noise".into()
        } else if u < 0.2 {
            "Triangle".into()
        } else if u < 0.3 {
            "Sawtooth".into()
        } else {
            format!("Pulse {:.0} %", 100.0 * (0.5 - 0.45 * ((u - 0.3) / 0.7)))
        }
    };
    let rc = p.rect(cx - d / 2.0 - 4.0, cy - d / 2.0 - 4.0, d + 8.0, d + 8.0);
    let rid = p.id("shape4", wave_id);
    let resp = p.ui.interact(rc, rid, Sense::click_and_drag()).on_hover_text(format!("{name}: {}", text(u)));
    let acc_id = p.id("shape4acc", wave_id);
    let mut acc: f32 = p.ui.data(|dd| dd.get_temp(acc_id)).unwrap_or(u);
    if resp.drag_started() {
        acc = u;
    }
    let mut nu = u;
    if resp.dragged() {
        let fine = if p.ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
        acc = (acc - resp.drag_delta().y / 160.0 * fine).clamp(0.0, 1.0);
        nu = acc;
        p.ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        egui::Tooltip::always_open(p.ui.ctx().clone(), p.ui.layer_id(), rid.with("tip"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(text(nu));
        });
    }
    if resp.double_clicked() || (resp.clicked() && p.ui.input(|i| i.modifiers.alt)) {
        nu = 0.25;
    }
    p.ui.data_mut(|dd| dd.insert_temp(acc_id, acc));
    if (nu - u).abs() > 1e-4 {
        if nu < 0.1 {
            p.put(noise_id, 1.0);
        } else {
            p.put(noise_id, 0.0);
            if nu < 0.2 {
                p.put(wave_id, 0.0);
            } else if nu < 0.3 {
                p.put(wave_id, 1.0);
            } else {
                p.put(wave_id, 2.0);
                p.put(pw_id, 0.5 - 0.45 * ((nu - 0.3) / 0.7));
            }
        }
    }
    let c = p.o + vec2(cx, cy);
    let pt = p.painter();
    let deg = |a: f32| a.to_radians();
    // the arc on the right: the pulse width's travel
    let arc: Vec<egui::Pos2> = (0..=24).map(|k| {
        let a = deg(-62.0 + 104.0 * k as f32 / 24.0);
        c + vec2(a.cos(), a.sin()) * (d / 2.0 + 13.0)
    }).collect();
    pt.add(egui::Shape::line(arc, Stroke::new(1.2, rgb(0x7a7a7a))));
    // the icons at their places
    for (k, a) in [(0usize, 140.0f32), (3, 168.0), (2, 196.0), (1, 228.0)] {
        let ic = c + vec2(deg(a).cos(), deg(a).sin()) * (d / 2.0 + 15.0);
        wave_icon(&pt, ic, k, DIM);
    }
    // the thin pulse at the lower right
    let ic = c + vec2(deg(50.0).cos(), deg(50.0).sin()) * (d / 2.0 + 15.0);
    let o = ic - vec2(6.0, 4.0);
    pt.add(egui::Shape::line(vec![o + vec2(0.0, 0.0), o + vec2(0.0, 8.0), o + vec2(3.0, 8.0), o + vec2(3.0, 0.0), o + vec2(12.0, 0.0), o + vec2(12.0, 8.0)], Stroke::new(1.3, DIM)));
    // the knob: skirt and cap as the other knobs (no ticks), its pointer at the shape's angle
    synthui::silver_body(&pt, c, d, resp.hovered() || resp.dragged(), Some(deg(135.0 + 270.0 * nu)), false);
}

fn oscillator(p: &mut Pane, engine: usize, info: &Info) {
    if let Some(k) = tab_bar(p, UX[0] + 12.0, UX[1] - 10.0, UTOP + 10.0, 27.0, &["ANALOG", "SYNC", "TABLE", "FM"], engine, "engine", 19.0) {
        p.put(19, k as f32);
    }
    let (lx, mx1, mx2, sx) = (80.0, 205.0, 299.0, 401.0);
    let mid = |p: &mut Pane, x: f32, y: f32, id: u32, top: &str, second: Option<&str>| {
        if let Some(s) = second {
            txt(p, x, y - 63.0, top, 13.0, LABEL);
            txt(p, x, y - 43.0, s, 13.0, LABEL);
        } else {
            txt(p, x, y - 43.0, top, 13.0, LABEL);
        }
        p.knob_at(x, y, 52.0, id);
    };
    let row1 = 147.0;
    let row2 = 268.0;
    match engine {
        0 | 1 => {
            bold(p, lx, 72.0, "Shape 1", 14.5, LABEL);
            shape4(p, lx, 137.0, 28, r::OSC1_NOISE, 29, "Shape 1");
            if engine == 0 {
                bold(p, lx, 199.0, "Shape 2", 14.5, LABEL);
                shape4(p, lx, 263.0, 30, r::OSC2_NOISE, 31, "Shape 2");
            } else {
                bold(p, lx, 199.0, "Sync", 14.5, LABEL);
                p.knob_at(lx, 263.0, 62.0, r::SYNC_AMOUNT);
            }
            mid(p, mx1, row1, 25, "Shape", Some("Modulation"));
            txt(p, mx1 - 28.0, 187.0, "LFO", 11.0, DIM);
            txt(p, mx1 + 24.0, 187.0, "Filter Env", 11.0, DIM);
            mid(p, mx2, row1, 20, "Vibrato", None);
        }
        2 => {
            bold(p, lx, 72.0, "Shape", 14.5, LABEL);
            let rc = p.rect(lx - 33.0, 104.0, 66.0, 66.0);
            p.ui.ctx().data_mut(|dd| dd.insert_temp(egui::Id::unique(("retro-shape1", p.t)), rc));
            p.knob_at(lx, 137.0, 62.0, 28);
            bold(p, lx, 199.0, "Shape 2", 14.5, LABEL);
            p.knob_at(lx, 263.0, 62.0, 30);
            wavetable_menu(p, info, 150.0, 56.0, 200.0);
            let formant = p.on(r::TABLE_MODE);
            let rb = p.rect(150.0, 80.0, 90.0, 16.0);
            if p.button(rb, if formant { "Formant" } else { "Shape" }, formant, "tblmode") {
                p.put(r::TABLE_MODE, if formant { 0.0 } else { 1.0 });
            }
            p.knob_at(mx1, row1 + 8.0, 52.0, 25);
            txt(p, mx1, 196.0, if formant { "Formant Mod" } else { "Shape Mod" }, 12.0, LABEL);
            p.knob_at(mx2, row1 + 8.0, 52.0, 20);
            txt(p, mx2, 196.0, "Vibrato", 12.0, LABEL);
        }
        _ => {
            bold(p, lx, 72.0, "FM", 14.5, LABEL);
            p.knob_at(lx, 137.0, 62.0, 26);
            bold(p, lx, 199.0, "Harmonic", 14.5, LABEL);
            p.knob_at(lx, 263.0, 62.0, r::FM_HARMONIC);
            p.seg(150.0, 56.0, 200.0, 16.0, r::FM_MODE, &["FM", "FM + Harm.", "Harmonic"], false);
            p.knob_at(mx1, row1 + 8.0, 52.0, 25);
            txt(p, mx1, 196.0, "FM Modulation", 12.0, LABEL);
            p.knob_at(mx2, row1 + 8.0, 52.0, 20);
            txt(p, mx2, 196.0, "Vibrato", 12.0, LABEL);
        }
    }
    mid(p, mx1, row2, 32, "Semitones", None);
    mid(p, mx2, row2, 33, "Cents", None);
    // the Mix slider (FM: the FM / sine mix, Table and the others: oscillator 1 / 2)
    bold(p, sx, 72.0, "Mix", 14.5, LABEL);
    let (top_l, bot_l, mix_id) = match engine {
        3 => ("Carrier", "FM", r::FM_MIX),
        _ => ("Osc 1", "Osc 2", 26),
    };
    txt(p, sx, 104.0, top_l, 13.0, LABEL);
    vslider(p, sx, 125.0, 281.0, mix_id, 6, true);
    txt(p, sx, 297.0, bot_l, 13.0, LABEL);
}


/// The Wavetable pop-up of the Table engine.
fn wavetable_menu(p: &mut Pane, info: &Info, x: f32, y: f32, w: f32) {
    let src = p.geti(r::TABLE_SOURCE);
    let app_i = p.geti(r::WAVETABLE).min(r::APP_TABLES.len() - 1);
    let name = match src {
        1 => info.custom.as_ref().map(|(_, n)| format!("Custom: {n}")).unwrap_or_else(|| "Custom Wavetable".into()),
        2 => info.user_names.get(p.get(r::USER_TABLE) as usize).cloned().unwrap_or_else(|| "User".into()),
        _ => r::APP_TABLES[app_i].to_string(),
    };
    let rc = p.rect(x, y, w, 20.0);
    let rid = p.id("wtmenu", 0);
    let resp = p.ui.interact(rc, rid, Sense::click());
    let pt = p.painter();
    pt.rect_filled(rc, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    pt.with_clip_rect(rc).text(pos2(rc.min.x + 8.0, rc.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), p.skin.text);
    crate::glyphs::paint(&pt, egui::Rect::from_center_size(pos2(rc.max.x - 10.0, rc.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, p.skin.dim);
    if let Some((n, _)) = &info.custom {
        p.text(x + 96.0, y + 32.0, Align2::LEFT_CENTER, &format!("{n} entries"), 10.0, p.skin.dim);
    }
    let name_id = egui::Id::unique(("retro-wt-name", p.t));
    let mut acts: Vec<String> = Vec::new();
    let rev = p.on(r::TABLE_REVERSE);
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(220.0);
        ui.menu_button("App Wavetables", |ui| {
            for (i, n) in r::APP_TABLES.iter().enumerate() {
                if ui.add(egui::Button::selectable(src == 0 && app_i == i, *n)).clicked() {
                    acts.push(format!("app:{i}"));
                    ui.close();
                }
            }
        });
        if ui.add_enabled(info.custom.is_some(), egui::Button::new("Custom Wavetable")).clicked() {
            acts.push("custom:".into());
            ui.close();
        }
        if !info.user_names.is_empty() {
            ui.menu_button("User Wavetables", |ui| {
                for n in &info.user_names {
                    if ui.button(n).clicked() {
                        acts.push(format!("user:{n}"));
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        if ui.add(egui::Button::selectable(rev, "Reverse Wavetable Order")).clicked() {
            acts.push("rev:".into());
            ui.close();
        }
        ui.separator();
        if ui.button("Create Wavetable from Audiofile…").clicked() {
            acts.push("create:".into());
            ui.close();
        }
        ui.add_enabled_ui(info.custom.is_some(), |ui| {
            ui.menu_button("Save Wavetable as…", |ui| {
                let mut name: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
                let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Wavetable name").desired_width(160.0));
                let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Save").clicked() || enter) && !name.trim().is_empty() {
                    acts.push(format!("save:{}", name.trim()));
                    name.clear();
                    ui.close();
                }
                ui.data_mut(|d| d.insert_temp(name_id, name));
            });
        });
        if ui.button("Delete Selected User Wavetable").clicked() {
            acts.push("delete:".into());
            ui.close();
        }
    });
    p.actions.extend(acts);
}

// ---------------------------------------------------------------------------------------------
// Filter section

fn filter_label(p: &Pane) -> String {
    let (m, v, s) = (p.geti(r::FILTER_MODE) as u8, p.geti(r::FILTER_VARIANT) as u8, p.geti(r::FILTER_SLOPE) as u8);
    if m == 0 {
        return r::FILTERS[p.geti(35).min(r::FILTERS.len() - 1)].to_string();
    }
    FILTER_LIST
        .iter()
        .find(|e| e.1 == m && e.2 == v && e.3 == s)
        .map(|e| e.0.to_string())
        .unwrap_or_else(|| format!("{} {}", r::FILTER_MODES[m as usize], r::FILTER_VARIANTS[v as usize]))
}


// ---------------------------------------------------------------------------------------------
// Filter section

fn filter(p: &mut Pane) {
    let x0 = UX[1];
    power(p, x0 + 10.0, UTOP + 10.0, 27.0, 34);
    let label = filter_label(p);
    let resp = menu_box(p, p.rect(x0 + 43.0, UTOP + 10.0, 217.0, 27.0), &label, 19.0, "ftype");
    let mut pick: Option<usize> = None;
    egui::Popup::menu(&resp).show(|ui| {
        let mut last = 0;
        for (i, e) in FILTER_LIST.iter().enumerate() {
            if e.1 != last && last != 0 {
                ui.separator();
            }
            last = e.1;
            if ui.button(e.0).clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        let e = FILTER_LIST[i];
        p.put(r::FILTER_MODE, e.1 as f32);
        p.put(r::FILTER_VARIANT, e.2 as f32);
        p.put(r::FILTER_SLOPE, e.3 as f32);
    }
    let mode = p.geti(r::FILTER_MODE);
    // the response display: drag the dot sideways for Cutoff, up or down for Resonance
    let disp = lcd(p, x0 + 9.0, 62.0, 218.0, 132.0);
    let (cut, res) = (p.get(36), p.get(37));
    let slope_n = [1.0f32, 2.0, 3.0, 4.0][p.geti(r::FILTER_SLOPE).min(3)];
    let kind = if mode == 0 { match p.geti(35) { 3 => 3, 4 => 2, 5 => 5, 6 => 4, _ => 1 } } else { mode };
    let (br, pg, pbw) = (p.get(r::BR_AMOUNT), p.get(r::PEAK_GAIN), p.get(r::PEAK_BW));
    let resp_y = |x: f32| -> f32 {
        let f = 20.0 * 1000f32.powf(x);
        let fc = 20.0 * 2f32.powf(cut * 10.0);
        let rr = f / fc;
        let g = (-((rr.ln()) / 0.18).powi(2)).exp();
        let y = match kind {
            2 => rr.powf(2.0 * slope_n) / (1.0 + rr.powf(2.0 * slope_n)) + 0.3 * res * g,
            3 => {
                let n = slope_n.min(2.0);
                let a = rr.powf(2.0 * n);
                (a / (1.0 + a)) * (1.0 / (1.0 + (rr / 3.0).powf(2.0 * n))) * 2.0 + 0.2 * res * g
            }
            4 => 1.0 - 0.9 * br * (-((rr.ln()) / 0.4).powi(2)).exp(),
            5 => 0.5 + 0.3 * (pg / 24.0) * (-((rr.ln()) / (0.2 + 0.8 * pbw)).powi(2)).exp() * 2.0,
            _ => 1.0 / (1.0 + rr.powf(2.0 * slope_n)).sqrt() + 0.45 * res * g,
        };
        (y * 0.5).clamp(0.0, 1.0)
    };
    // the curve sits above the field row
    let plot = egui::Rect::from_min_max(disp.min + vec2(4.0, 8.0), pos2(disp.max.x - 4.0, disp.max.y - 26.0));
    let pts: Vec<(f32, f32)> = (0..=100).map(|k| (k as f32 / 100.0, resp_y(k as f32 / 100.0))).collect();
    p.curve(plot, &pts, if p.on(34) { LCD_LINE } else { rgb(0x5a4630) });
    let fx = ((cut * 10.0 * 2f32.ln()) / 1000f32.ln()).clamp(0.0, 1.0);
    let dot = pos2(plot.min.x + fx * plot.width(), plot.max.y - resp_y(fx) * plot.height());
    let did = p.id("fdot", 0);
    let dresp = p.ui.interact(plot, did, Sense::drag()).on_hover_text("Drag sideways for Cutoff, up or down for Resonance");
    p.painter().circle_filled(dot, 3.5, rgb(0xf0a040));
    if dresp.dragged() {
        let d = dresp.drag_delta();
        p.put(36, (cut + d.x / plot.width()).clamp(0.0, 1.0));
        p.put(37, (res - d.y / plot.height()).clamp(0.0, 1.0));
    }
    match kind {
        4 => lcd_field(p, disp.min.x - p.o.x + 8.0, 66.0, 60.0, 56.0, r::BR_AMOUNT, "Amount:", &format!("{br:.2}"), 0.005),
        5 => {
            lcd_field(p, disp.min.x - p.o.x + 8.0, 66.0, 40.0, 56.0, r::PEAK_GAIN, "Gain:", &format!("{pg:.1} dB"), 0.1);
            lcd_field(p, disp.min.x - p.o.x + 116.0, 66.0, 30.0, 56.0, r::PEAK_BW, "BW:", &format!("{pbw:.2}"), 0.005);
        }
        _ => {}
    }
    let fy = disp.max.y - p.o.y - 21.0;
    lcd_field(p, disp.min.x - p.o.x + 8.0, fy, 50.0, 66.0, 36, "Cutoff:", &format!("{cut:.3}"), 0.003);
    lcd_field(p, disp.min.x - p.o.x + 132.0, fy, 32.0, 50.0, 37, "Res:", &format!("{res:.2}"), 0.005);
    // Keyscale: the slider at the display's right
    txt(p, x0 + 247.0, 67.0, "Key", 13.0, LABEL);
    vslider(p, x0 + 247.0, 92.0, 190.0, 38, 0, false);
    // the modulation knobs: Filter FM (static at the left, envelope at the right), LFO, Env
    let row = 268.0;
    txt(p, x0 + 45.0, 216.0, "Filter FM", 13.0, LABEL);
    p.knob_at(x0 + 45.0, row, 52.0, r::FILTER_FM);
    txt(p, x0 + 25.0, 307.0, "Static", 11.0, DIM);
    txt(p, x0 + 73.0, 307.0, "Env", 11.0, DIM);
    txt(p, x0 + 134.0, 216.0, "LFO", 13.0, LABEL);
    // the filter LFO knob is bipolar, centre off (the old unipolar value folds into the new one)
    {
        let id = p.id("lfoknob", 0);
        let mut v = (p.get(39) + p.get(r::CUTOFF_LFO2)).clamp(-1.0, 1.0);
        if synthui::silver_knob(p.ui, p.o + vec2(x0 + 134.0, row), 52.0, id, &mut v, -1.0, 1.0, 0.0, "Cutoff by LFO", &|v| format!("{:.0} %", v * 100.0)) {
            p.put(39, 0.0);
            p.put(r::CUTOFF_LFO2, v);
        }
    }
    txt(p, x0 + 222.0, 216.0, "Env", 13.0, LABEL);
    p.knob_at(x0 + 222.0, row, 52.0, 40);
}

// ---------------------------------------------------------------------------------------------
// Amp and Effect

fn amp_effect(p: &mut Pane) {
    let ax = (UX[2] + UX[3]) / 2.0;
    txt(p, ax, 85.0, "Volume", 13.0, LABEL);
    p.knob_at(ax, 137.0, 52.0, 4);
    txt(p, ax, 216.0, "Sine Level", 13.0, LABEL);
    p.knob_at(ax, 268.0, 52.0, 27);
    let x0 = UX[3];
    power(p, x0 + 12.0, UTOP + 10.0, 27.0, 42);
    let cur = p.geti(43).min(r::EFFECTS.len() - 1);
    let resp = menu_box(p, p.rect(x0 + 46.0, UTOP + 10.0, 100.0, 27.0), r::EFFECTS[cur], 19.0, "fxtype");
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        for (i, n) in r::EFFECTS.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == cur, *n)).clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        p.put(43, i as f32);
    }
    let ex = (UX[3] + UX[4]) / 2.0;
    txt(p, ex, 85.0, "Mix", 13.0, LABEL);
    p.knob_at(ex, 137.0, 52.0, r::EFFECT_MIX);
    txt(p, ex, 216.0, "Rate", 13.0, LABEL);
    p.knob_at(ex, 268.0, 52.0, 45);
}

// ---------------------------------------------------------------------------------------------
// Glide / Autobend

fn glide(p: &mut Pane) {
    let x0 = LX[0];
    power(p, x0 + 9.0, LTOP + 6.0, 25.0, r::GLIDE_ON);
    let ty = p.geti(22).min(1);
    let resp = menu_box(p, p.rect(x0 + 41.0, LTOP + 5.0, 125.0, 27.0), r::GLIDE_TYPE[ty], 18.0, "glidetype");
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        for (i, n) in r::GLIDE_TYPE.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == ty, *n)).clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        p.put(22, i as f32);
    }
    txt(p, x0 + 42.0, 426.0, "Mode", 13.0, LABEL);
    let tg = p.geti(r::GLIDE_TARGET).min(r::GLIDE_TARGETS.len() - 1);
    const SHORT: [&str; 5] = ["Osc 1", "Osc 2", "All Osc", "Opposed", "Osc + Filter"];
    let mr = p.rect(x0 + 69.0, 416.0, 98.0, 21.0);
    let mresp = p.ui.interact(mr, p.id("glidetarget", 0), Sense::click()).on_hover_text(r::GLIDE_TARGETS[tg]);
    {
        let pt = p.painter();
        pt.rect_filled(mr, 10.0, if mresp.hovered() { rgb(0x2a2a2a) } else { TAB_BG });
        pt.text(mr.center(), Align2::CENTER_CENTER, SHORT[tg], look::regular(12.5), LABEL);
    }
    let mut pick = None;
    egui::Popup::menu(&mresp).show(|ui| {
        for (i, n) in r::GLIDE_TARGETS.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == tg, *n)).clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        p.put(r::GLIDE_TARGET, i as f32);
    }
    let cx = (LX[0] + LX[1]) / 2.0;
    if ty == 1 {
        txt(p, cx - 36.0, 453.0, "Time", 13.0, LABEL);
        p.knob_at(cx - 36.0, 495.0, 39.0, 24);
        txt(p, cx + 36.0, 453.0, "Depth", 13.0, LABEL);
        p.knob_at(cx + 36.0, 495.0, 39.0, 21);
    } else {
        txt(p, cx, 453.0, "Time", 13.0, LABEL);
        p.knob_at(cx, 495.0, 39.0, 24);
    }
}

// ---------------------------------------------------------------------------------------------
// LFO / Vibrato

fn modulation(p: &mut Pane) {
    let tab_id = egui::Id::unique(("retro-modtab", p.t));
    let mut tab: usize = p.ui.ctx().data(|d| d.get_temp(tab_id)).unwrap_or(0);
    let x0 = LX[1];
    if let Some(k) = tab_bar(p, x0 + 6.0, LX[2] - 6.0, LTOP + 5.0, 27.0, &["LFO", "VIBRATO"], tab, "modtab", 19.0) {
        tab = k;
        p.ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
    }
    let (wave, sync, rate, div, via, via_amt, speed) = if tab == 0 {
        (46, 47, 48, r::LFO_DIV, 49, 50, r::LFO_VIA_SPEED)
    } else {
        (51, 52, 53, r::VIB_DIV, 54, 55, r::VIB_VIA_SPEED)
    };
    // the LEDs beside the tab names (lit while the generator does something)
    for (k, lit) in [(0usize, p.get(25).abs() > 0.001 || p.get(39) > 0.001 || p.get(r::CUTOFF_LFO2).abs() > 0.001), (1, p.get(20) > 0.001 || p.get(55).abs() > 0.001)] {
        let c = p.o + vec2(x0 + 6.0 + (LX[2] - x0 - 12.0) * (k as f32 + 0.5) / 2.0 + 48.0, LTOP + 18.5);
        let pt = p.painter();
        pt.circle_filled(c, 5.0, rgb(0x1a1a1a));
        pt.circle_filled(c, 3.6, if lit { rgb(0xf09040) } else { rgb(0x5a3a20) });
    }
    let disp = lcd(p, x0 + 11.0, 417.0, 223.0, 135.0);
    // the waveform buttons along the display's top
    let wv = p.geti(wave);
    let n = r::LFO_WAVES.len();
    let bw = (disp.width() - 8.0) / n as f32;
    for k in 0..n {
        let br = egui::Rect::from_min_size(pos2(disp.min.x + 4.0 + bw * k as f32, disp.min.y + 4.0), vec2(bw - 3.0, 15.0));
        let resp = p.ui.interact(br, p.id("lfowave", wave * 10 + k as u32), Sense::click()).on_hover_text(r::LFO_WAVES[k]);
        let pt = p.painter();
        pt.rect_filled(br, 2.0, if k == wv { rgb(0x4a3216) } else { LCD_FIELD });
        pt.rect_stroke(br, 2.0, Stroke::new(1.0, if k == wv { LCD_LINE } else { rgb(0x3a2a16) }), egui::StrokeKind::Inside);
        let wpts = wave_points(k, 12);
        let ir = br.shrink2(vec2(5.0, 3.5));
        let v: Vec<egui::Pos2> = wpts.iter().map(|&(x, y)| pos2(ir.min.x + x * ir.width(), ir.max.y - y * ir.height())).collect();
        pt.add(egui::Shape::line(v, Stroke::new(1.0, if k == wv { rgb(0xf0a040) } else { rgb(0x9a6a30) })));
        if resp.clicked() {
            p.put(wave, k as f32);
        }
    }
    let plot = egui::Rect::from_min_max(pos2(disp.min.x + 8.0, disp.min.y + 26.0), pos2(disp.max.x - 8.0, disp.max.y - 26.0));
    p.curve(plot, &wave_points(wv, 80), LCD_LINE);
    // Rate: the strip along the bottom (a tempo division when Sync is on)
    let sync_on = p.on(sync);
    let ry = disp.max.y - 21.0;
    p.text(disp.min.x - p.o.x + 6.0, ry - p.o.y + 8.0, Align2::LEFT_CENTER, "Rate:", 12.5, LCD_LINE.gamma_multiply(0.85));
    let tr = egui::Rect::from_min_size(pos2(disp.min.x + 44.0, ry), vec2(118.0, 16.0));
    let tresp = p.ui.interact(tr, p.id("lforate", rate), Sense::click_and_drag());
    let (frac, shown) = if sync_on {
        let dv = p.geti(div).min(r::SYNC_DIVS.len() - 1);
        (dv as f32 / (r::SYNC_DIVS.len() - 1) as f32, r::SYNC_DIVS[dv].to_string())
    } else {
        let pr = p.prm(rate);
        let v = p.get(rate);
        (((v / pr.min).ln() / (pr.max / pr.min).ln()).clamp(0.0, 1.0), format!("{v:.2} Hz"))
    };
    {
        let pt = p.painter();
        pt.rect_filled(tr, 8.0, LCD_FIELD);
        pt.circle_filled(pos2(tr.min.x + 8.0 + (tr.width() - 16.0) * frac, tr.center().y), 6.5, rgb(0xe8902f));
        pt.text(pos2(disp.max.x - 6.0, ry + 8.0), Align2::RIGHT_CENTER, &shown, look::regular(12.5), rgb(0xe9952f));
    }
    if (tresp.dragged() || tresp.clicked())
        && let Some(pp) = tresp.interact_pointer_pos()
    {
        let f = ((pp.x - tr.min.x - 8.0) / (tr.width() - 16.0)).clamp(0.0, 1.0);
        if sync_on {
            p.put(div, (f * (r::SYNC_DIVS.len() - 1) as f32).round());
        } else {
            let pr = p.prm(rate);
            p.put(rate, pr.min * (pr.max / pr.min).powf(f));
        }
    }
    // the Via amount slider at the display's right, and the Via source below it
    vslider(p, x0 + 251.0, 427.0, 543.0, via_amt, 0, false);
    let vr = p.rect(x0 + 167.0, 563.0, 88.0, 21.0);
    let cur = p.geti(via).min(r::VIA.len() - 1);
    const VIA_SHORT: [&str; 6] = ["Off", "Wheel", "Touch", "Velocity", "Ctrl X", "Wheel+Touch"];
    let vresp = p.ui.interact(vr, p.id("via", via), Sense::click()).on_hover_text(format!("Via: {}", r::VIA[cur]));
    {
        let pt = p.painter();
        pt.rect_filled(vr, 10.0, if vresp.hovered() { rgb(0x2a2a2a) } else { TAB_BG });
        pt.text(vr.center(), Align2::CENTER_CENTER, VIA_SHORT[cur], look::regular(13.0), LABEL);
    }
    let mut pick = None;
    egui::Popup::menu(&vresp).show(|ui| {
        for (i, n) in r::VIA.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == cur, *n)).clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        p.put(via, i as f32);
    }
    // Sync: the rocker switch with Off / On under it
    p.text(x0 + 13.0, 573.0, Align2::LEFT_CENTER, "Sync", 13.0, LABEL);
    let sr = p.rect(x0 + 52.0, 565.0, 52.0, 17.0);
    let sresp = p.ui.interact(sr, p.id("sync", sync), Sense::click());
    {
        let pt = p.painter();
        pt.rect_filled(sr, 8.5, rgb(0x101010));
        let kx = if sync_on { sr.max.x - 10.0 } else { sr.min.x + 10.0 };
        pt.circle_filled(pos2(kx, sr.center().y), 9.0, rgb(0xb8b8b8));
        pt.circle_filled(pos2(kx - 1.0, sr.center().y - 1.5), 6.0, rgb(0xdedede));
        pt.text(p.o + vec2(x0 + 62.0, 591.0), Align2::CENTER_CENTER, "Off", look::regular(11.0), if sync_on { DIM } else { LABEL });
        pt.text(p.o + vec2(x0 + 95.0, 591.0), Align2::CENTER_CENTER, "On", look::regular(11.0), if sync_on { LABEL } else { DIM });
    }
    if sresp.clicked() {
        p.put(sync, if sync_on { 0.0 } else { 1.0 });
    }
    // with Sync off the Via source sets the speed, with Sync on the level
    let want = if sync_on { 0.0 } else { 1.0 };
    if (p.get(speed) - want).abs() > 0.5 {
        p.put(speed, want);
    }
}

/// One cycle of an LFO waveform (x and y as fractions), `n` steps.
fn wave_points(wv: usize, n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|k| {
            let ph = k as f32 / n as f32;
            let v = match wv {
                0 => (ph * std::f32::consts::TAU).sin(),
                1 => 1.0 - 4.0 * (ph - 0.5).abs(),
                2 => 2.0 * ph - 1.0,
                3 => 1.0 - 2.0 * ph,
                4 => if ph < 0.5 { 1.0 } else { -1.0 },
                5 => [0.6, -0.4, 0.9, -0.8, 0.2][((ph * 5.0) as usize).min(4)],
                _ => ((ph * 23.0).sin() + (ph * 7.0).cos()) * 0.45,
            };
            (ph, 0.5 + 0.42 * v)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Envelopes

fn time_text(s: f32) -> String {
    if s < 0.001 { format!("{:.2} ms", s * 1000.0) } else if s < 1.0 { format!("{:.0} ms", s * 1000.0) } else { format!("{:.2} s", s) }
}

/// An envelope display with its A / D values above and S / R below, a handle each for Attack
/// (the peak, dragged sideways), Decay (the end of the slope, sideways), Sustain (the plateau, up
/// or down) and Release (the end, sideways), and the Velocity slider at its right.
fn env(p: &mut Pane, x0: f32, ids: [u32; 4], vel: u32, amp: bool) {
    let disp = lcd(p, x0 + 12.0, 417.0, 217.0, 135.0);
    let [a, d, s, rel] = ids.map(|i| p.get(i));
    let (lx, ly) = (disp.min.x - p.o.x, disp.min.y - p.o.y);
    lcd_field(p, lx + 5.0, ly + 5.0, 18.0, 78.0, ids[0], "A:", &time_text(a), 0.01);
    lcd_field(p, lx + 110.0, ly + 5.0, 18.0, 82.0, ids[1], "D:", &time_text(d), 0.02);
    lcd_field(p, lx + 5.0, disp.max.y - p.o.y - 21.0, 18.0, 78.0, ids[2], "S:", &format!("{s:.2}"), 0.005);
    lcd_field(p, lx + 110.0, disp.max.y - p.o.y - 21.0, 18.0, 82.0, ids[3], "R:", &time_text(rel), 0.02);
    let w = disp.width();
    let seg = w * 0.27;
    let tx = |t: f32| (t / 10.0).clamp(0.0, 1.0).sqrt() * seg;
    let (top, bot) = (disp.min.y + 30.0, disp.max.y - 28.0);
    let yy = |l: f32| bot - (bot - top) * l;
    let x0p = disp.min.x + 10.0;
    let xa = x0p + tx(a);
    let xd = xa + tx(d);
    let xs = xd + w * 0.14;
    let xr = xs + tx(rel);
    let line = [pos2(x0p, bot), pos2(xa, top), pos2(xd, yy(s)), pos2(xs, yy(s)), pos2(xr, bot)];
    p.painter().with_clip_rect(disp).add(egui::Shape::line(line.to_vec(), Stroke::new(1.5, LCD_LINE)));
    let sync = amp && a < 0.0005;
    if sync {
        p.painter().text(pos2(disp.max.x - 6.0, top - 2.0), Align2::RIGHT_CENTER, "synced", look::regular(10.0), rgb(0xf0c870));
    }
    let secs = |dx: f32| 10.0 * (dx / seg).clamp(0.0, 1.0).powi(2);
    let handles: [(usize, egui::Pos2); 4] = [(0, line[1]), (1, line[2]), (2, pos2((xd + xs) / 2.0, yy(s))), (3, line[4])];
    for (k, hp) in handles {
        let hr = egui::Rect::from_center_size(hp, vec2(14.0, 14.0));
        let hid = p.id("envh", ids[k.min(3)] * 10 + k as u32);
        let resp = p.ui.interact(hr, hid, Sense::drag());
        p.painter().circle_filled(hp, 3.5, if resp.hovered() || resp.dragged() { rgb(0xffc070) } else { rgb(0xf0a040) });
        if resp.dragged()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            match k {
                0 => p.put(ids[0], secs(pp.x - x0p)),
                1 => p.put(ids[1], secs(pp.x - xa)),
                2 => p.put(ids[2], ((bot - pp.y) / (bot - top)).clamp(0.0, 1.0)),
                _ => p.put(ids[3], secs(pp.x - xs)),
            }
            p.ui.ctx().set_cursor_icon(if k == 2 { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
        }
    }
    vslider(p, x0 + 248.0, 427.0, 543.0, vel, 0, false);
    txt(p, x0 + 246.0, 574.0, "Vel", 13.0, LABEL);
}

fn target_name(engine: usize, i: usize) -> &'static str {
    match (engine, i) {
        (2, 10) => "Wave Variation (Formant)",
        (3, 10) => "Wave Variation (FM Harmonic)",
        _ => r::MOD_TARGETS.get(i).copied().unwrap_or(""),
    }
}

/// A controller-target menu offering only what the active engine can modulate.
fn target_menu(p: &mut Pane, x: f32, y: f32, w: f32, id: u32, engine: usize) {
    let cur = p.geti(id);
    let rc = p.rect(x, y, w, 20.0);
    let rid = p.id("tmenu", id);
    let resp = p.ui.interact(rc, rid, Sense::click());
    let pt = p.painter();
    pt.rect_filled(rc, 9.0, if resp.hovered() { rgb(0x2a2a2a) } else { TAB_BG });
    pt.with_clip_rect(rc).text(pos2(rc.min.x + 10.0, rc.center().y), Align2::LEFT_CENTER, target_name(engine, cur), look::regular(12.5), LABEL);
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        for i in r::targets_for(engine as u8) {
            if ui.add(egui::Button::selectable(i == cur, target_name(engine, i))).clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        p.put(id, i as f32);
    }
}


// ---------------------------------------------------------------------------------------------
// Settings: the global and controller settings in the lower tray

/// A dark rounded field showing a value; drag up or down to change it (whole steps).
fn step_field(p: &mut Pane, r: egui::Rect, id: u32, text: &str, per_px: f32, lo: f32, hi: f32, value: f32) -> Option<f32> {
    let resp = p.ui.interact(r, p.id("stepf", id), Sense::click_and_drag()).on_hover_text(p.prm(id).name);
    let pt = p.painter();
    pt.rect_filled(r, 10.0, if resp.hovered() || resp.dragged() { rgb(0x2a2a2a) } else { TAB_BG });
    pt.text(r.center(), Align2::CENTER_CENTER, text, look::regular(13.0), LABEL);
    if resp.double_clicked() || (resp.clicked() && p.ui.input(|i| i.modifiers.alt)) {
        return Some(p.prm(id).default);
    }
    if resp.dragged() {
        let acc_id = p.id("stepacc", id);
        let mut acc: f32 = p.ui.data(|d| d.get_temp(acc_id)).unwrap_or(value);
        if resp.drag_started() {
            acc = value;
        }
        acc = (acc - resp.drag_delta().y * per_px).clamp(lo, hi);
        p.ui.data_mut(|d| d.insert_temp(acc_id, acc));
        if acc.round() != value.round() {
            return Some(acc.round());
        }
    }
    None
}

fn global_settings(p: &mut Pane, engine: usize) {
    let lab = |p: &Pane, x: f32, y: f32, s: &str| p.text(x, y, Align2::RIGHT_CENTER, s, 13.0, LABEL);
    // left: tuning and voices
    let (lx, fx) = (128.0, 138.0);
    let rows = [400.0, 432.0, 464.0, 496.0, 528.0, 560.0];
    lab(p, lx, rows[0], "Transpose");
    let tr = p.get(2).round();
    if let Some(v) = step_field(p, p.rect(fx, rows[0] - 10.0, 90.0, 20.0), 2, &format!("{tr:+.0} st"), 0.15, -24.0, 24.0, tr) {
        p.put(2, v);
    }
    lab(p, lx, rows[1], "Tune");
    let tu = p.get(3);
    if let Some(v) = step_field(p, p.rect(fx, rows[1] - 10.0, 90.0, 20.0), 3, &format!("{tu:+.0} ct"), 0.5, -100.0, 100.0, tu) {
        p.put(3, v);
    }
    lab(p, lx, rows[2], "Bend Up / Down");
    let up = p.get(6);
    if let Some(v) = step_field(p, p.rect(fx, rows[2] - 10.0, 43.0, 20.0), 6, &format!("{up:.0}"), 0.15, 0.0, 24.0, up) {
        p.put(6, v);
    }
    // Bend down follows the up range until it is set on its own
    let down = p.get(r::BEND_DOWN);
    let shown = if down < 0.0 { up } else { down };
    if let Some(v) = step_field(p, p.rect(fx + 47.0, rows[2] - 10.0, 43.0, 20.0), r::BEND_DOWN, &format!("{shown:.0}"), 0.15, 0.0, 24.0, shown) {
        p.put(r::BEND_DOWN, v);
    }
    lab(p, lx, rows[3], "Voices");
    let poly = p.get(0);
    if let Some(v) = step_field(p, p.rect(fx, rows[3] - 10.0, 43.0, 20.0), 0, &format!("{poly:.0}"), 0.2, 1.0, 32.0, poly) {
        p.put(0, v);
    }
    p.seg(fx + 50.0, rows[3] - 10.0, 150.0, 20.0, 5, &["Poly", "Mono", "Legato"], false);
    lab(p, lx, rows[4], "Unison");
    let un = p.get(1);
    if let Some(v) = step_field(p, p.rect(fx, rows[4] - 10.0, 43.0, 20.0), 1, &format!("{un:.0}"), 0.1, 1.0, 4.0, un) {
        p.put(1, v);
    }
    p.seg(fx + 50.0, rows[4] - 10.0, 150.0, 20.0, r::VOICE_MODE, &["Single", "Double"], false);
    lab(p, lx, rows[5], "Detune / Spread");
    p.hfader(fx - 4.0, rows[5] - 9.0, 100.0, 18.0, 9);
    p.hfader(fx + 104.0, rows[5] - 9.0, 100.0, 18.0, 8);
    // right: the controller assignments for this engine
    let cx = 420.0;
    p.text(cx, 392.0, Align2::LEFT_CENTER, "Controller", 12.0, DIM);
    p.text(cx + 120.0, 392.0, Align2::LEFT_CENTER, "Target", 12.0, DIM);
    p.text(cx + 370.0, 392.0, Align2::LEFT_CENTER, "Amount", 12.0, DIM);
    for (k, (label, tgt, amt)) in [("Velocity", 10u32, 11u32), ("Mod Wheel", 12, 13), ("Aftertouch", 14, 15), ("Ctrl X", 17, 18)].into_iter().enumerate() {
        let y = 418.0 + k as f32 * 34.0;
        p.text(cx, y, Align2::LEFT_CENTER, label, 13.0, LABEL);
        target_menu(p, cx + 120.0, y - 10.0, 238.0, tgt, engine);
        p.hfader(cx + 366.0, y - 9.0, 170.0, 18.0, amt);
    }
    p.text(cx, 562.0, Align2::LEFT_CENTER, "Ctrl X source CC", 12.0, DIM);
    let cc = p.get(16);
    if let Some(v) = step_field(p, p.rect(cx + 120.0, 552.0, 60.0, 20.0), 16, &format!("{cc:.0}"), 0.3, 1.0, 127.0, cc) {
        p.put(16, v);
    }
}
