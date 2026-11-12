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

const SKIN: Skin = Skin::new(0x3a3d42, 0x2e3135, 0x7fb0e0);
const BLUE: Color32 = rgb(0x5aa0e8);
const ORANGE: Color32 = rgb(0xe8a04a);

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

pub const EXT: &[u32] = &[r::MPE_MODE, r::MPE_BEND, r::ANALYSIS_SENS, 60, 65];

struct Info {
    custom: Option<(usize, String)>,
    user_names: Vec<String>,
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let info = Info {
        custom: instwin::with_inst(app, t, |s: &mut RetroSynth| s.custom_frames().map(|f| (f.len(), s.custom_name.clone()))).flatten(),
        user_names: r::wavetable::list(),
    };
    let actions = synthui::window(app, ctx, t, "Retro Synth", vec2(900.0, 470.0), SKIN, EXT, |p| body(p, &info));
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

fn kn(p: &mut Pane, x: f32, y: f32, d: f32, id: u32, label: &str) {
    p.knob(x, y, d, id, label);
}

fn knob_rect(p: &Pane, x: f32, y: f32, d: f32) -> egui::Rect {
    p.rect(x - d / 2.0 - 2.0, y, d + 4.0, d + 18.0)
}

fn body(p: &mut Pane, info: &Info) {
    let engine = p.geti(19);
    oscillator(p, engine, info);
    filter(p);
    amp_effect(p);
    envelopes(p);
    settings(p, engine);
}

// ---------------------------------------------------------------------------------------------
// Oscillator section

/// A four-position knob: Noise, Rect, Saw, Tri (a click or a vertical drag steps it).
fn shape4(p: &mut Pane, x: f32, y: f32, d: f32, wave_id: u32, noise_id: u32, label: &str) {
    const NAMES: [&str; 4] = ["Noise", "Rect", "Saw", "Tri"];
    let noise = p.on(noise_id);
    let w = p.get(wave_id).round() as i32;
    let pos = if noise { 0 } else { match w { 0 => 3, 1 => 2, _ => 1 } };
    let rc = p.rect(x - d / 2.0, y, d, d);
    let rid = p.id("shape4", wave_id);
    let resp = p.ui.interact(rc, rid, Sense::click_and_drag());
    let acc_id = p.id("shape4acc", wave_id);
    let mut acc: f32 = p.ui.data(|dd| dd.get_temp(acc_id)).unwrap_or(pos as f32);
    if resp.drag_started() {
        acc = pos as f32;
    }
    let mut np = pos;
    if resp.dragged() {
        acc = (acc - resp.drag_delta().y / 18.0).clamp(0.0, 3.0);
        np = acc.round() as i32;
    } else if resp.clicked() {
        np = (pos + 1) % 4;
    }
    p.ui.data_mut(|dd| dd.insert_temp(acc_id, acc));
    if np != pos {
        match np {
            0 => p.put(noise_id, 1.0),
            n => {
                p.put(noise_id, 0.0);
                p.put(wave_id, match n { 1 => 2.0, 2 => 1.0, _ => 0.0 });
            }
        }
    }
    let pt = p.painter();
    let c = rc.center();
    pt.circle_filled(c, d / 2.0, Color32::from_gray(0x1c));
    pt.circle_stroke(c, d / 2.0, Stroke::new(1.5, Color32::from_gray(0x50)));
    let a = std::f32::consts::PI * (0.75 + 1.5 * np as f32 / 3.0);
    pt.line_segment([c, c + vec2(a.cos(), a.sin()) * (d / 2.0 - 4.0)], Stroke::new(2.5, p.skin.accent));
    p.text(x, y + d + 8.0, Align2::CENTER_CENTER, &format!("{label}: {}", NAMES[np as usize]), 10.0, p.skin.dim);
}

fn oscillator(p: &mut Pane, engine: usize, info: &Info) {
    p.group(10.0, 10.0, 350.0, 222.0, "OSCILLATOR");
    p.seg(20.0, 28.0, 330.0, 20.0, 19, r::ENGINES, false);
    let d = 44.0;
    match engine {
        0 => {
            shape4(p, 52.0, 62.0, d, 28, r::OSC1_NOISE, "Shape 1");
            shape4(p, 142.0, 62.0, d, 30, r::OSC2_NOISE, "Shape 2");
            kn(p, 232.0, 62.0, d, 29, "Pulse 1");
            kn(p, 312.0, 62.0, d, 31, "Pulse 2");
            kn(p, 52.0, 146.0, d, 25, "Shape Mod");
            kn(p, 132.0, 146.0, d, 26, "Mix");
            kn(p, 212.0, 146.0, d, 27, "Sine");
            kn(p, 292.0, 146.0, d, 32, "Semitone");
            p.label(222.0, 224.0, "Detune");
            p.field(246.0, 216.0, 60.0, 14.0, 33, 1.0);
        }
        1 => {
            shape4(p, 52.0, 62.0, d, 28, r::OSC1_NOISE, "Shape 1");
            shape4(p, 142.0, 62.0, d, 30, r::OSC2_NOISE, "Shape 2");
            kn(p, 232.0, 62.0, d, r::SYNC_AMOUNT, "Sync");
            kn(p, 312.0, 62.0, d, 25, "Shape Mod");
            kn(p, 52.0, 146.0, d, 26, "Mix");
            kn(p, 132.0, 146.0, d, 27, "Sine");
            kn(p, 212.0, 146.0, d, 32, "Semitone");
            kn(p, 292.0, 146.0, d, 33, "Detune");
        }
        2 => {
            wavetable_menu(p, info);
            let rc = knob_rect(p, 52.0, 86.0, d);
            p.ui.ctx().data_mut(|dd| dd.insert_temp(egui::Id::unique(("retro-shape1", p.t)), rc));
            kn(p, 52.0, 86.0, d, 28, "Shape 1");
            kn(p, 132.0, 86.0, d, 30, "Shape 2");
            let r_btn = p.rect(210.0, 92.0, 66.0, 22.0);
            let formant = p.on(r::TABLE_MODE);
            if p.button(r_btn, if formant { "Formant" } else { "Shape" }, formant, "tblmode") {
                p.put(r::TABLE_MODE, if formant { 0.0 } else { 1.0 });
            }
            p.label(243.0, 124.0, "Shape / Formant");
            kn(p, 312.0, 86.0, d, 25, if formant { "Formant Mod" } else { "Shape Mod" });
            kn(p, 52.0, 164.0, d, 26, "Mix");
            kn(p, 132.0, 164.0, d, 27, "Sine");
            kn(p, 212.0, 164.0, d, 32, "Semitone");
            kn(p, 292.0, 164.0, d, 33, "Detune");
        }
        _ => {
            p.seg(20.0, 56.0, 250.0, 18.0, r::FM_MODE, &["FM", "FM + Harm.", "Harmonic"], false);
            p.label(310.0, 65.0, "Modulation target");
            kn(p, 52.0, 84.0, d, 28, "Carrier");
            kn(p, 132.0, 84.0, d, 30, "Modulator");
            kn(p, 212.0, 84.0, d, r::FM_HARMONIC, "Harmonic");
            kn(p, 292.0, 84.0, d, r::FM_INHARMONIC, "Inharmonic");
            kn(p, 52.0, 156.0, d, r::FM_SHAPE, "FM Shape");
            kn(p, 132.0, 156.0, d, r::FM_MIX, "FM Mix");
            kn(p, 212.0, 156.0, d, 26, "FM");
            kn(p, 292.0, 156.0, d, 25, "Modulation");
            p.label(40.0, 224.0, "Semi");
            p.field(60.0, 216.0, 60.0, 14.0, 32, 1.0);
            p.label(150.0, 224.0, "Detune");
            p.field(172.0, 216.0, 60.0, 14.0, 33, 1.0);
        }
    }
}

/// The Wavetable pop-up of the Table engine.
fn wavetable_menu(p: &mut Pane, info: &Info) {
    let src = p.geti(r::TABLE_SOURCE);
    let app_i = p.geti(r::WAVETABLE).min(r::APP_TABLES.len() - 1);
    let name = match src {
        1 => info.custom.as_ref().map(|(_, n)| format!("Custom: {n}")).unwrap_or_else(|| "Custom Wavetable".into()),
        2 => info.user_names.get(p.get(r::USER_TABLE) as usize).cloned().unwrap_or_else(|| "User".into()),
        _ => r::APP_TABLES[app_i].to_string(),
    };
    let rc = p.rect(20.0, 56.0, 240.0, 22.0);
    let rid = p.id("wtmenu", 0);
    let resp = p.ui.interact(rc, rid, Sense::click());
    let pt = p.painter();
    pt.rect_filled(rc, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    pt.with_clip_rect(rc).text(pos2(rc.min.x + 8.0, rc.center().y), Align2::LEFT_CENTER, format!("Wavetable: {name}"), look::regular(10.5), p.skin.text);
    crate::glyphs::paint(&pt, egui::Rect::from_center_size(pos2(rc.max.x - 10.0, rc.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, p.skin.dim);
    if let Some((n, _)) = &info.custom {
        p.text(270.0, 67.0, Align2::LEFT_CENTER, &format!("{n} entries"), 10.0, p.skin.dim);
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

fn filter(p: &mut Pane) {
    p.group(370.0, 10.0, 300.0, 222.0, "FILTER");
    // the on button at the top left of the section
    p.toggle(380.0, 28.0, 40.0, 20.0, 34, "On");
    let label = filter_label(p);
    let rc = p.rect(428.0, 28.0, 232.0, 20.0);
    let rid = p.id("ftype", 0);
    let resp = p.ui.interact(rc, rid, Sense::click());
    let pt = p.painter();
    pt.rect_filled(rc, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    pt.text(pos2(rc.min.x + 8.0, rc.center().y), Align2::LEFT_CENTER, &label, look::regular(10.5), p.skin.text);
    crate::glyphs::paint(&pt, egui::Rect::from_center_size(pos2(rc.max.x - 10.0, rc.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, p.skin.dim);
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
    // the response display; the Resonance strip drags vertically, the Cutoff strip horizontally
    let disp = p.display(380.0, 56.0, 248.0, 74.0);
    let (cut, res) = (p.get(36), p.get(37));
    let slope_n = [1.0f32, 2.0, 3.0, 4.0][p.geti(r::FILTER_SLOPE).min(3)];
    let kind = if mode == 0 { match p.geti(35) { 3 => 3, 4 => 2, 5 => 5, 6 => 4, _ => 1 } } else { mode };
    let (br, pg, pbw) = (p.get(r::BR_AMOUNT), p.get(r::PEAK_GAIN), p.get(r::PEAK_BW));
    let pts: Vec<(f32, f32)> = (0..=100)
        .map(|k| {
            let x = k as f32 / 100.0;
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
            (x, (y * 0.62).clamp(0.0, 1.0))
        })
        .collect();
    p.curve(disp, &pts, if p.on(34) { p.skin.accent } else { Color32::from_gray(0x60) });
    // Resonance: dragged vertically
    let rs = p.rect(634.0, 56.0, 26.0, 74.0);
    let rid = p.id("fres", 0);
    let rresp = p.ui.interact(rs, rid, Sense::drag()).on_hover_text("Resonance: drag vertically");
    {
        let pt = p.painter();
        pt.rect_filled(rs, 4.0, Color32::from_gray(0x16));
        let h = rs.height() * res;
        pt.rect_filled(egui::Rect::from_min_max(pos2(rs.min.x + 2.0, rs.max.y - h), pos2(rs.max.x - 2.0, rs.max.y)), 3.0, ORANGE);
        pt.text(rs.center_top() + vec2(0.0, 7.0), Align2::CENTER_CENTER, "Reso", look::regular(9.5), Color32::from_gray(0xd0));
    }
    if rresp.dragged() {
        p.put(37, (res - rresp.drag_delta().y / 80.0).clamp(0.0, 1.0));
    }
    // Cutoff: dragged horizontally
    let cs = p.rect(380.0, 134.0, 248.0, 18.0);
    let cid = p.id("fcut", 0);
    let cresp = p.ui.interact(cs, cid, Sense::drag()).on_hover_text("Cutoff: drag horizontally");
    {
        let pt = p.painter();
        pt.rect_filled(cs, 4.0, Color32::from_gray(0x16));
        pt.rect_filled(egui::Rect::from_min_max(cs.min + vec2(2.0, 2.0), pos2(cs.min.x + 2.0 + (cs.width() - 4.0) * cut, cs.max.y - 2.0)), 3.0, BLUE);
        pt.text(cs.center(), Align2::CENTER_CENTER, format!("Cutoff  {}", hz_text(20.0 * 2f32.powf(cut * 10.0))), look::regular(10.0), Color32::from_gray(0xf0));
    }
    if cresp.dragged() {
        p.put(36, (cut + cresp.drag_delta().x / 220.0).clamp(0.0, 1.0));
    }
    // the modulation knobs
    kn(p, 402.0, 160.0, 38.0, 40, "Cutoff Env");
    // the filter LFO knob is bipolar, centre off (the old unipolar value folds into the new one)
    {
        let rc = knob_rect(p, 448.0, 160.0, 38.0);
        let id = p.id("lfoknob", 0);
        let mut v = (p.get(39) + p.get(r::CUTOFF_LFO2)).clamp(-1.0, 1.0);
        if crate::widgets::knob(p.ui, rc, id, &mut v, -1.0, 1.0, 0.0, "Cutoff by LFO", &|v| format!("{:.0} %", v * 100.0)) {
            p.put(39, 0.0);
            p.put(r::CUTOFF_LFO2, v);
        }
    }
    p.label(448.0, 206.0, "LFO");
    kn(p, 494.0, 160.0, 38.0, r::FILTER_FM, "Filter FM");
    match mode {
        4 => kn(p, 540.0, 160.0, 38.0, r::BR_AMOUNT, "Amount"),
        5 => {
            kn(p, 540.0, 160.0, 38.0, r::PEAK_GAIN, "Gain");
            kn(p, 586.0, 160.0, 38.0, r::PEAK_BW, "BW");
        }
        _ => {
            kn(p, 540.0, 160.0, 38.0, 41, "Drive");
            kn(p, 586.0, 160.0, 38.0, 38, "Key");
        }
    }
    p.label(520.0, 222.0, "Filter FM: static (left)  off  envelope (right)");
}

fn hz_text(hz: f32) -> String {
    if hz >= 1000.0 { format!("{:.2} kHz", hz / 1000.0) } else { format!("{:.0} Hz", hz) }
}

// ---------------------------------------------------------------------------------------------
// Amp and Effect

fn amp_effect(p: &mut Pane) {
    p.group(680.0, 10.0, 210.0, 222.0, "AMP / EFFECT");
    kn(p, 716.0, 34.0, 44.0, 4, "Volume");
    p.text(790.0, 44.0, Align2::LEFT_CENTER, "Effect", 11.0, p.skin.dim);
    p.toggle(836.0, 34.0, 44.0, 20.0, 42, "On");
    p.menu(780.0, 62.0, 100.0, 20.0, 43);
    kn(p, 712.0, 132.0, 40.0, 44, "Intensity");
    kn(p, 772.0, 132.0, 40.0, 45, "Rate");
    kn(p, 832.0, 132.0, 40.0, r::EFFECT_MIX, "Mix");
}

// ---------------------------------------------------------------------------------------------
// Envelopes

fn envelopes(p: &mut Pane) {
    p.group(10.0, 240.0, 350.0, 222.0, "ENVELOPES");
    env(p, 20.0, 262.0, 330.0, 90.0, [56, 57, 58, 59], "Filter Env", false);
    env(p, 20.0, 362.0, 330.0, 90.0, [61, 62, 63, 64], "Amp Env", true);
}

fn time_text(s: f32) -> String {
    if s < 0.001 { format!("{:.2} ms", s * 1000.0) } else if s < 1.0 { format!("{:.0} ms", s * 1000.0) } else { format!("{:.2} s", s) }
}

/// An envelope display: the shape drawn from its times, with a handle each for Attack (the peak,
/// dragged horizontally), Decay (end of the slope, horizontally), Sustain (on the plateau,
/// vertically) and Release (the end, horizontally).
fn env(p: &mut Pane, x: f32, y: f32, w: f32, h: f32, ids: [u32; 4], title: &str, amp: bool) {
    let r = p.display(x, y, w, h);
    let [a, d, s, rel] = ids.map(|i| p.get(i));
    let seg = w * 0.26;
    let tx = |t: f32| (t / 10.0).clamp(0.0, 1.0).sqrt() * seg;
    let (top, bot) = (r.min.y + 18.0, r.max.y - 8.0);
    let yy = |l: f32| bot - (bot - top) * l;
    let x0 = r.min.x + 10.0;
    let xa = x0 + tx(a);
    let xd = xa + tx(d);
    let xs = xd + w * 0.16;
    let xr = xs + tx(rel);
    let line = [pos2(x0, bot), pos2(xa, top), pos2(xd, yy(s)), pos2(xs, yy(s)), pos2(xr, bot)];
    {
        let pt = p.painter().with_clip_rect(r);
        pt.add(egui::Shape::line(line.to_vec(), Stroke::new(1.8, if amp { BLUE } else { ORANGE })));
        pt.text(r.min + vec2(8.0, 8.0), Align2::LEFT_CENTER, title, look::semibold(10.0), Color32::from_gray(0xc0));
        let sync = amp && a < 0.0005;
        pt.text(
            pos2(r.max.x - 8.0, r.min.y + 8.0),
            Align2::RIGHT_CENTER,
            format!("A {}  D {}  S {:.0}%  R {}{}", time_text(a), time_text(d), s * 100.0, time_text(rel), if sync { "  synced" } else { "" }),
            look::regular(9.5),
            if sync { Color32::from_rgb(0xf0, 0xc8, 0x70) } else { Color32::from_gray(0x90) },
        );
    }
    let secs = |dx: f32| 10.0 * (dx / seg).clamp(0.0, 1.0).powi(2);
    let handles: [(usize, egui::Pos2); 4] = [(0, line[1]), (1, line[2]), (2, pos2((xd + xs) / 2.0, yy(s))), (3, line[4])];
    for (k, hp) in handles {
        let hr = egui::Rect::from_center_size(hp, vec2(14.0, 14.0));
        let hid = p.id("envh", ids[k.min(3)] * 10 + k as u32);
        let resp = p.ui.interact(hr, hid, Sense::drag());
        let pt = p.painter();
        pt.circle_filled(hp, 4.5, if resp.hovered() || resp.dragged() { Color32::WHITE } else { Color32::from_gray(0xc8) });
        if resp.dragged()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            match k {
                0 => p.put(ids[0], secs(pp.x - x0)),
                1 => p.put(ids[1], secs(pp.x - xa)),
                2 => p.put(ids[2], ((bot - pp.y) / (bot - top)).clamp(0.0, 1.0)),
                _ => p.put(ids[3], secs(pp.x - xs)),
            }
            p.ui.ctx().set_cursor_icon(if k == 2 { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Settings: modulation, or global and controller settings

fn settings(p: &mut Pane, engine: usize) {
    p.group(370.0, 240.0, 520.0, 222.0, "");
    let st_id = egui::Id::unique(("retro-settings", p.t));
    let mut global: bool = p.ui.ctx().data(|d| d.get_temp(st_id)).unwrap_or(false);
    // the Settings label switches the area
    let lr = p.rect(380.0, 246.0, 150.0, 20.0);
    let lid = p.id("settingslabel", 0);
    let lresp = p.ui.interact(lr, lid, Sense::click()).on_hover_text("Click to switch between modulation and global settings");
    {
        let pt = p.painter();
        pt.rect_filled(lr, 4.0, if lresp.hovered() { Color32::from_gray(0x44) } else { Color32::from_gray(0x38) });
        pt.text(lr.center(), Align2::CENTER_CENTER, format!("Settings: {}", if global { "Global" } else { "Modulation" }), look::semibold(10.5), Color32::from_gray(0xe0));
    }
    if lresp.clicked() {
        global = !global;
        p.ui.ctx().data_mut(|d| d.insert_temp(st_id, global));
    }
    if global {
        global_settings(p, engine);
    } else {
        modulation(p);
    }
}

fn modulation(p: &mut Pane) {
    // LFO / Vibrato tabs
    let tab_id = egui::Id::unique(("retro-modtab", p.t));
    let mut tab: usize = p.ui.ctx().data(|d| d.get_temp(tab_id)).unwrap_or(0);
    for (k, n) in ["LFO", "Vibrato"].iter().enumerate() {
        let rc = p.rect(540.0 + k as f32 * 66.0, 246.0, 62.0, 20.0);
        if p.button(rc, n, tab == k, ("modtab", k)) {
            tab = k;
        }
    }
    p.ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
    let (wave, sync, rate, div, via, via_amt, speed) = if tab == 0 {
        (46, 47, 48, r::LFO_DIV, 49, 50, r::LFO_VIA_SPEED)
    } else {
        (51, 52, 53, r::VIB_DIV, 54, 55, r::VIB_VIA_SPEED)
    };
    // the waveform display and its menu
    let disp = p.display(380.0, 274.0, 120.0, 56.0);
    let wv = p.geti(wave);
    let pts: Vec<(f32, f32)> = (0..=60)
        .map(|k| {
            let ph = k as f32 / 60.0 * 2.0 % 1.0;
            let v = match wv {
                0 => (ph * std::f32::consts::TAU).sin(),
                1 => 1.0 - 4.0 * (ph - 0.5).abs(),
                2 => 2.0 * ph - 1.0,
                3 => 1.0 - 2.0 * ph,
                4 => if ph < 0.5 { 1.0 } else { -1.0 },
                5 => if (k / 10) % 2 == 0 { 0.6 } else { -0.4 },
                _ => ((k as f32 * 0.37).sin() + (k as f32 * 0.11).cos()) * 0.45,
            };
            (k as f32 / 60.0, 0.5 + 0.4 * v)
        })
        .collect();
    p.curve(disp, &pts, p.skin.accent);
    p.menu(380.0, 336.0, 120.0, 20.0, wave);
    // rate: Hz, or a tempo division when Sync is on
    let sync_on = p.on(sync);
    p.toggle(380.0, 362.0, 56.0, 20.0, sync, "Sync");
    if sync_on {
        p.menu(442.0, 362.0, 58.0, 20.0, div);
        p.text(548.0, 300.0, Align2::CENTER_CENTER, r::SYNC_DIVS[p.geti(div).min(7)], 12.0, p.skin.text);
        p.label(548.0, 326.0, "Rate (tempo)");
    } else {
        kn(p, 548.0, 280.0, 40.0, rate, "Rate");
    }
    if tab == 1 {
        kn(p, 618.0, 280.0, 40.0, 20, "Depth");
    }
    // Via: the controller that raises the level, or sets the speed when Sync is off
    p.text(380.0, 402.0, Align2::LEFT_CENTER, "Via", 10.5, p.skin.dim);
    p.menu(404.0, 392.0, 150.0, 20.0, via);
    p.text(380.0, 428.0, Align2::LEFT_CENTER, "Amt", 10.0, p.skin.dim);
    p.hfader(404.0, 418.0, 150.0, 18.0, via_amt);
    let want = if sync_on { 0.0 } else { 1.0 };
    if (p.get(speed) - want).abs() > 0.5 {
        p.put(speed, want);
    }
    p.text(404.0, 448.0, Align2::LEFT_CENTER, if sync_on { "Via sets the level" } else { "Via sets the speed (fastest at the Amount)" }, 9.5, p.skin.dim);

    // Glide / Autobend
    p.group(660.0, 270.0, 220.0, 184.0, "GLIDE / AUTOBEND");
    p.toggle(672.0, 290.0, 44.0, 20.0, r::GLIDE_ON, "On");
    p.seg(724.0, 290.0, 146.0, 20.0, 22, &["Glide", "Autobend"], false);
    p.menu(672.0, 318.0, 198.0, 20.0, r::GLIDE_TARGET);
    p.label(771.0, 346.0, "Mode");
    let amt_label = if p.geti(22) == 1 { "Depth" } else { "Amount" };
    kn(p, 706.0, 356.0, 38.0, 21, amt_label);
    kn(p, 776.0, 356.0, 38.0, 24, "Time");
    p.seg(826.0, 362.0, 48.0, 36.0, 23, &["Always", "Legato"], true);
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
    let rc = p.rect(x, y, w, 18.0);
    let rid = p.id("tmenu", id);
    let resp = p.ui.interact(rc, rid, Sense::click());
    let pt = p.painter();
    pt.rect_filled(rc, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    pt.with_clip_rect(rc).text(pos2(rc.min.x + 6.0, rc.center().y), Align2::LEFT_CENTER, target_name(engine, cur), look::regular(10.0), p.skin.text);
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

fn global_settings(p: &mut Pane, engine: usize) {
    let col = |p: &mut Pane, y: f32, label: &str| p.text(384.0, y + 9.0, Align2::LEFT_CENTER, label, 10.5, p.skin.dim);
    col(p, 274.0, "Transpose");
    let tr = p.get(2).round() as i32;
    let rc = p.rect(470.0, 274.0, 80.0, 18.0);
    let tid = p.id("transpose", 0);
    let resp = p.ui.interact(rc, tid, Sense::click());
    {
        let pt = p.painter();
        pt.rect_filled(rc, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
        pt.text(pos2(rc.min.x + 6.0, rc.center().y), Align2::LEFT_CENTER, format!("{tr:+} st"), look::regular(10.5), p.skin.text);
    }
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for v in (-24..=24).rev() {
                if ui.add(egui::Button::selectable(v == tr, format!("{v:+}"))).clicked() {
                    pick = Some(v);
                    ui.close();
                }
            }
        });
    });
    if let Some(v) = pick {
        p.put(2, v as f32);
    }
    col(p, 298.0, "Tune");
    p.field(470.0, 298.0, 80.0, 18.0, 3, 1.0);
    col(p, 322.0, "Bend up / down");
    p.field(470.0, 322.0, 38.0, 18.0, 6, 1.0);
    // Bend down follows the up range until it is set on its own
    let down = p.get(r::BEND_DOWN);
    {
        let rc = p.rect(512.0, 322.0, 38.0, 18.0);
        let bid = p.id("benddown", 0);
        let resp = p.ui.interact(rc, bid, Sense::click_and_drag());
        let shown = if down < 0.0 { p.get(6) } else { down };
        let pt = p.painter();
        pt.rect_filled(rc, 4.0, Color32::from_gray(0x22));
        pt.text(rc.center(), Align2::CENTER_CENTER, format!("{shown:.0}"), look::regular(10.5), p.skin.text);
        if resp.dragged() {
            p.put(r::BEND_DOWN, (shown - resp.drag_delta().y / 8.0).round().clamp(0.0, 24.0));
        }
    }
    col(p, 346.0, "Polyphony");
    p.field(470.0, 346.0, 38.0, 18.0, 0, 1.0);
    p.seg(512.0, 346.0, 100.0, 18.0, 5, &["Poly", "Mono", "Legato"], false);
    col(p, 370.0, "Voice mode");
    p.seg(470.0, 370.0, 120.0, 18.0, r::VOICE_MODE, &["Single", "Double"], false);
    col(p, 394.0, "Unison");
    p.field(470.0, 394.0, 38.0, 18.0, 1, 1.0);
    kn(p, 548.0, 396.0, 30.0, 9, "Detune");
    kn(p, 600.0, 396.0, 30.0, 8, "Spread");
    p.text(384.0, 450.0, Align2::LEFT_CENTER, "Detune and spread apply to the voice pairs in Double mode", 9.5, p.skin.dim);
    // the controller targets for this engine
    p.text(664.0, 256.0, Align2::LEFT_CENTER, "CONTROLLERS", 10.0, p.skin.dim);
    for (k, (label, tgt, amt)) in [("Velocity", 10u32, 11u32), ("Mod Wheel", 12, 13), ("Aftertouch", 14, 15), ("Ctrl X", 17, 18)].into_iter().enumerate() {
        let y = 270.0 + k as f32 * 44.0;
        p.text(664.0, y + 8.0, Align2::LEFT_CENTER, label, 10.5, p.skin.dim);
        p.hfader(738.0, y, 140.0, 16.0, amt);
        target_menu(p, 664.0, y + 20.0, 214.0, tgt, engine);
    }
    p.text(664.0, 452.0, Align2::LEFT_CENTER, "Ctrl X source CC", 9.5, p.skin.dim);
    p.field(762.0, 444.0, 50.0, 16.0, 16, 1.0);
}
