//! Alchemy's window, laid out as Logic's: the Name bar (Browse / Simple / Advanced, the preset
//! field with ‹ ›, File: Initialize Preset / Refresh Library, Save, Save As, Limiter, Vol); in
//! Advanced view the Global page (sources A–D with Vol / Tune / Pan / F1/F2, the two main filters
//! with Par/Ser, Master, Voices), each source's page (element and its controls, Import Audio…),
//! the modulation section (alchemyrack) and, at the bottom in every view, the Perform section
//! (Transform pad, eight knobs, two XY pads, the envelope knobs) sharing its place with the
//! Arpeggiator (mode, octaves, rate, length, swing, latch, 16-step sequencer) and Effects (four
//! slots of Studio's effects with their controls). Browse view lists the saved presets with a
//! search field; Simple view is the Name bar and the Perform section.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::alchemy_extra::{self as ax, FX, FX_PARAM, FX_SLOTS, FX_TYPES};
use lg_synth::alchemy_mod as am;

use crate::alchemyrack::{self as rack, g, Ix, Live, St, BLUE, ORANGE};
use crate::app::App;
use crate::instwin;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x1c2027, 0x23272e, 0x46a6d6);
const W: f32 = 1146.0;

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let st_id = egui::Id::unique(("alchemy-st", t));
    let mut st: St = ctx.data(|d| d.get_temp(st_id)).unwrap_or_default();
    let mut live = instwin::with_inst(app, t, |a: &mut lg_synth::alchemy::Alchemy| Live { levels: a.mg.levels, envf: a.mg.envf, time: 0.0 }).unwrap_or_default();
    live.time = ctx.input(|i| i.time);
    // the effects' own parameters, read from the running instrument
    let fx_params: Vec<Option<(Vec<lg_dsp::Param>, Vec<f32>)>> = (0..FX_SLOTS).map(|s| instwin::with_inst(app, t, |a: &mut lg_synth::alchemy::Alchemy| a.extra.fx[s].as_ref().map(|f| {
        let p = f.params().to_vec();
        let v = p.iter().map(|x| f.get(x.id)).collect();
        (p, v)
    })).flatten()).collect();
    let audio_name = instwin::track_id(app, t).and_then(|id| app.inst.alch_audio.get(&id)).and_then(|(p, _)| p.file_name()).map(|s| s.to_string_lossy().into_owned());
    let presets: Vec<String> = crate::pluginwin::saved_settings("Alchemy").into_iter().map(|(n, _)| n).collect();
    let cur = synthui::setting_name(app, t);
    let dropped = std::mem::take(&mut app.inst.dropped);
    let size = match st.view {
        1 => vec2(W, 258.0),
        2 => vec2(W, 520.0),
        _ => vec2(W, 738.0),
    };
    let ext: Vec<u32> = (0..5).map(|k| am::CTRL_CC + k).collect();
    let actions = synthui::window(app, ctx, t, "Alchemy", size, SKIN, &ext, |p| body(p, &mut st, &live, &fx_params, audio_name.as_deref(), &presets, &cur));
    ctx.data_mut(|d| d.insert_temp(st_id, st));
    let mut file = dropped.first().cloned();
    for a in actions {
        let (verb, arg) = a.split_once(':').unwrap_or((a.as_str(), ""));
        match verb {
            "import" => file = kn_ui::files::FileDialog::new().set_title("Import Audio").add_filter("Audio", &["wav", "aif", "aiff", "caf", "flac", "WAV", "AIF", "AIFF"]).pick_file(),
            "init" => initialize(app, t),
            "refresh" => app.tell(format!("{} Alchemy presets", crate::pluginwin::saved_settings("Alchemy").len())),
            "step" => synthui::step_setting(app, t, "Alchemy", arg.parse().unwrap_or(1)),
            "load" => {
                if let Some((n, path)) = crate::pluginwin::saved_settings("Alchemy").into_iter().find(|(n, _)| n == arg)
                    && let Some(pairs) = crate::pluginwin::read_setting(&path)
                {
                    synthui::apply_setting(app, t, &n, &pairs);
                }
            }
            "save" => {
                if let Some((params, vals)) = instwin::values(app, t)
                    && crate::pluginwin::write_setting("Alchemy", arg, &params, &vals).is_some()
                {
                    if let Some(id) = instwin::track_id(app, t) {
                        app.inst.synth_setting.insert(id, arg.to_string());
                    }
                    app.tell(format!("Saved the preset “{arg}”"));
                }
            }
            "seq-import" => seq_import(app, t, arg),
            _ => {}
        }
    }
    if let Some(f) = file {
        match lg_synth::wav::read_audio(&f) {
            Ok(a) => {
                instwin::checkpoint(app, t, "Import Audio");
                if let Some(id) = instwin::track_id(app, t) {
                    app.inst.alch_audio.insert(id, (f.clone(), std::sync::Arc::new(a)));
                }
                // the source shown plays it (its Granular element on)
                let st: St = ctx.data(|d| d.get_temp(st_id)).unwrap_or_default();
                let src = (st.page.clamp(1, 4) - 1) as u32;
                instwin::set_params(app, t, &[(277 + src, 1.0), (313 + src, 1.0)]);
                instwin::bump(app, t);
            }
            Err(e) => app.tell(format!("Couldn't import {}: {e}", f.display())),
        }
    }
}

/// Initialize Preset: a basic sawtooth (source A, wide unison off), AHDSR 1 and Velocity on
/// Master Vol, Ctrl C (CC 74 Brightness) on Filter 1 Cutoff; everything else at its default.
pub fn init_pairs(list: &[lg_dsp::Param]) -> Vec<(u32, f32)> {
    use am::*;
    let mut v: Vec<(u32, f32)> = list.iter().map(|p| (p.id, p.default)).collect();
    let mut put = |id: u32, x: f32| {
        if let Some(e) = v.iter_mut().find(|e| e.0 == id) {
            e.1 = x;
        }
    };
    // source A: one saw; B off; the Perform knobs neutral
    put(278, 0.0);
    put(301, 2.0);
    put(8, 1.0);
    put(9, 0.0);
    put(10, 0.0);
    put(4, 0.5);
    put(5, 0.0);
    put(85, 0.75);
    put(ahd(0, AH_A), 0.0);
    put(ahd(0, AH_D), 0.0);
    put(ahd(0, AH_S), 1.0);
    put(ahd(0, AH_R), 0.3);
    for (n, (tgt, src, d)) in [(94u32, code(K_AHD, 0), 1.0f32), (94, code(K_NOTE, 0), 0.5), (85, code(K_MIDI, 4), 0.5)].into_iter().enumerate() {
        put(route(n, RT_TARGET), tgt as f32);
        put(route(n, RT_SRC), src);
        put(route(n, RT_ON), 1.0);
        put(route(n, RT_DEPTH), d);
    }
    v
}

fn initialize(app: &mut App, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    // only what differs (a full list of thousands of values would be slow to keep)
    let pairs: Vec<(u32, f32)> = init_pairs(&params).into_iter().zip(&vals).filter(|((_, v), cur)| (*v - **cur).abs() > 1e-6).map(|(p, _)| p).collect();
    instwin::checkpoint(app, t, "Initialize Preset");
    instwin::set_params(app, t, &pairs);
    if let Some(id) = instwin::track_id(app, t) {
        app.inst.synth_setting.insert(id, "Default".into());
    }
}

/// The sequencer's File > Import Velocity / Note / Groove from a short MIDI file.
fn seq_import(app: &mut App, t: usize, arg: &str) {
    let mut it = arg.split(':');
    let (Some(Ok(mode)), Some(Ok(i))) = (it.next().map(|x| x.parse::<u8>()), it.next().map(|x| x.parse::<usize>())) else { return };
    let Some(path) = kn_ui::files::FileDialog::new().set_title("Import MIDI File").add_filter("MIDI", &["mid", "midi", "MID"]).pick_file() else { return };
    let Some(imp) = std::fs::read(&path).ok().and_then(|d| lg_core::midi::read(&d)) else {
        app.tell(format!("Couldn't read {}", path.display()));
        return;
    };
    let mut notes: Vec<(u32, u8, u8)> = imp.tracks.iter().flat_map(|(_, _, n)| n.iter().map(|n| (n.pos, n.pitch, n.velocity))).collect();
    notes.sort();
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let rate = params.iter().position(|p| p.id == am::seq(i, am::SQ_RATE)).map(|k| vals[k]).unwrap_or(5.0);
    let beats = am::SEQ_RATE_BEATS[(rate.round().max(0.0) as usize).min(am::SEQ_RATE_BEATS.len() - 1)];
    let pairs: Vec<(u32, f32)> = am::seq_import(&notes, 960, beats, mode).into_iter().map(|(f, v)| (am::seq(i, f), v)).collect();
    instwin::checkpoint(app, t, "Import to Sequencer");
    instwin::set_params(app, t, &pairs);
}

fn body(p: &mut Pane, st: &mut St, live: &Live, fx: &[Option<(Vec<lg_dsp::Param>, Vec<f32>)>], audio: Option<&str>, presets: &[String], cur: &str) {
    let ix = Ix::new(p.params);
    name_bar(p, &ix, st, presets, cur);
    match st.view {
        1 => perform(p, &ix, st, fx, 46.0),
        2 => {
            browse(p, st, presets, cur);
            perform(p, &ix, st, fx, 308.0);
        }
        _ => {
            match st.page {
                0 => global(p, &ix, st),
                5 => {
                    nav(p, st);
                    p.text(300.0, 160.0, Align2::LEFT_CENTER, "Morph: the sources' morphing isn't in Studio yet.", 12.0, p.skin.dim);
                }
                s => {
                    nav(p, st);
                    source_filters(p, &ix, st, (s - 1) as u32);
                    let (params, vals) = (p.params, p.vals);
                    let get = |id: u32| ix_get(&ix, vals, id);
                    let r = p.rect(84.0, 48.0, 700.0, 230.0);
                    let mut set = Vec::new();
                    let mut import = false;
                    let mut child = p.ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
                    egui::ScrollArea::vertical().id_salt(("alch-src-page", p.t)).show(&mut child, |ui| {
                        sources_page(ui, p.t, (s - 1) as u32, params, &get, &mut set, &mut import, audio);
                    });
                    p.set.extend(set);
                    if import {
                        p.actions.push("import".into());
                    }
                }
            }
            rack::section(p, &ix, st, live, 5.0, 290.0, W - 10.0, 234.0);
            perform(p, &ix, st, fx, 528.0);
        }
    }
}

/// A source's filter module: filters 1–3 (buttons with their on light), On, type ‹ ›, the
/// type's three knobs, Ser / Par, and the send (F1/F2) to the main filters.
fn source_filters(p: &mut Pane, ix: &Ix, st: &mut St, s: u32) {
    use lg_synth::alchemy::{ROUTE_F2, SRC_FILT};
    let x0 = 800.0;
    p.ui.painter().line_segment([p.o + vec2(x0 - 10.0, 48.0), p.o + vec2(x0 - 10.0, 282.0)], Stroke::new(1.0, rgb(0x30353d)));
    p.text(x0, 66.0, Align2::LEFT_CENTER, "FILTER", 12.5, p.skin.text);
    let b = SRC_FILT + s * 16;
    let sel_id = egui::Id::unique(("alch-sfsel", p.t, s));
    let mut sel: u32 = p.ui.data(|d| d.get_temp(sel_id)).unwrap_or(0);
    for k in 0..3u32 {
        let r = p.rect(x0 + 60.0 + k as f32 * 34.0, 56.0, 32.0, 20.0);
        if rack::flat(p, r, &(k + 1).to_string(), sel == k, true, ("sfsel", s, k)) {
            sel = k;
        }
        // the light: that filter is on
        let on = g(p, ix, b + k) >= 0.5;
        p.ui.painter().circle_filled(r.right_top() + vec2(-5.0, 5.0), 2.5, if on { ORANGE } else { rgb(0x44484f) });
    }
    p.ui.data_mut(|d| d.insert_temp(sel_id, sel));
    let on = g(p, ix, b + sel) >= 0.5;
    if rack::flat(p, p.rect(x0, 90.0, 28.0, 20.0), "On", on, true, ("sfon", s, sel)) {
        p.put(b + sel, if on { 0.0 } else { 1.0 });
    }
    let ty_id = if sel == 0 { 333 + s } else { b + 2 + sel };
    rack::menu_arrows(p, ix, x0 + 32.0, 90.0, 140.0, ty_id);
    let t = g(p, ix, ty_id).round().max(0.0) as usize;
    let names = lg_synth::alchemy_filter::knob_names(t);
    for (j, id) in [51 + sel * 8 + s, 55 + sel * 8 + s, b + 5 + sel].iter().enumerate() {
        if j == 2 && !lg_synth::alchemy_filter::has_drive(t) {
            continue;
        }
        rack::knob(p, ix, st, x0 + 24.0 + j as f32 * 56.0, 122.0, 38.0, *id, names[j]);
    }
    let par = g(p, ix, b + 8) >= 0.5;
    for (k, l) in ["Ser", "Par"].iter().enumerate() {
        if rack::flat(p, p.rect(x0 + k as f32 * 36.0, 200.0, 34.0, 20.0), l, par == (k == 1), true, ("sfpar", s, k)) {
            p.put(b + 8, k as f32);
        }
    }
    rack::knob(p, ix, st, x0 + 250.0, 186.0, 38.0, ROUTE_F2 + s, "F1/F2");
}

fn ix_get(ix: &Ix, vals: &[f32], id: u32) -> f32 {
    ix.0.get(&id).map(|&i| vals[i]).unwrap_or(0.0)
}

// ---------------------------------------------------------------- Name bar

fn name_bar(p: &mut Pane, ix: &Ix, st: &mut St, presets: &[String], cur: &str) {
    let pt = p.painter();
    pt.rect_filled(p.rect(0.0, 0.0, W, 40.0), 0.0, rgb(0x55585e));
    for (k, n) in ["BROWSE", "SIMPLE", "ADVANCED"].iter().enumerate() {
        let view = [2u8, 1, 0][k];
        let r = p.rect(8.0 + k as f32 * 90.0, 7.0, 89.0, 27.0);
        let resp = p.ui.interact(r, egui::Id::unique(("alch-view", p.t, k)), Sense::click());
        let lit = st.view == view;
        pt.rect_filled(r, 0.0, if lit { BLUE } else if resp.hovered() { rgb(0x4d5056) } else { rgb(0x43464c) });
        pt.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(11.5), Color32::WHITE);
        if resp.clicked() {
            st.view = view;
        }
    }
    // the preset field, ‹ ›
    let fr = p.rect(326.0, 7.0, 355.0, 27.0);
    let resp = p.ui.interact(fr, egui::Id::unique(("alch-preset", p.t)), Sense::click());
    pt.rect_filled(fr, 0.0, rgb(0x43464c));
    pt.rect_filled(Rect::from_min_size(fr.min, vec2(28.0, 27.0)), 0.0, rgb(0x3a3d43));
    crate::glyphs::paint(&pt, Rect::from_min_size(fr.min, vec2(28.0, 27.0)), crate::glyphs::ph::CARET_DOWN, 11.0, Color32::WHITE);
    let shown = if cur == "Factory Default" { "Default" } else { cur };
    pt.text(pos2(fr.min.x + 28.0 + (fr.width() - 28.0) / 2.0, fr.center().y), Align2::CENTER_CENTER, shown, look::regular(12.0), Color32::WHITE);
    egui::Popup::menu(&resp).show(|ui| {
        if presets.is_empty() {
            ui.label("No saved presets");
        }
        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
            for n in presets {
                if ui.add(egui::Button::selectable(n == cur, n.as_str())).clicked() {
                    p.actions.push(format!("load:{n}"));
                }
            }
        });
    });
    let ar = p.rect(683.0, 7.0, 60.0, 27.0);
    pt.rect_filled(ar, 0.0, rgb(0x43464c));
    let d = rack::arrows(p, ar, "alch-preset-step");
    if d != 0 {
        p.actions.push(format!("step:{d}"));
    }
    // File / Save / Save As
    let fb = p.rect(750.0, 7.0, 45.0, 27.0);
    let resp = p.ui.interact(fb, egui::Id::unique(("alch-namefile", p.t)), Sense::click());
    pt.rect_filled(fb, 0.0, if resp.hovered() { rgb(0x4d5056) } else { rgb(0x43464c) });
    pt.text(fb.center(), Align2::CENTER_CENTER, "File", look::regular(11.5), Color32::WHITE);
    egui::Popup::menu(&resp).show(|ui| {
        ui.add_enabled(false, egui::Button::new("Save Consolidated"));
        if ui.button("Initialize Preset").clicked() {
            p.actions.push("init".into());
        }
        if ui.button("Refresh Library").clicked() {
            p.actions.push("refresh".into());
        }
    });
    let saved = presets.iter().any(|n| n == cur);
    let sb = p.rect(796.0, 7.0, 52.0, 27.0);
    let sresp = p.ui.interact(sb, egui::Id::unique(("alch-namesave", p.t)), Sense::click());
    pt.rect_filled(sb, 0.0, if sresp.hovered() { rgb(0x4d5056) } else { rgb(0x43464c) });
    pt.text(sb.center(), Align2::CENTER_CENTER, "Save", look::regular(11.5), Color32::WHITE);
    if sresp.clicked() && saved {
        p.actions.push(format!("save:{cur}"));
    }
    let ab = p.rect(849.0, 7.0, 68.0, 27.0);
    let aresp = p.ui.interact(ab, egui::Id::unique(("alch-namesaveas", p.t)), Sense::click());
    pt.rect_filled(ab, 0.0, if aresp.hovered() { rgb(0x4d5056) } else { rgb(0x43464c) });
    pt.text(ab.center(), Align2::CENTER_CENTER, "Save As", look::regular(11.5), Color32::WHITE);
    // Save on a preset that isn't one of the user's opens Save As
    let save_as = if sresp.clicked() && !saved { Some(&sresp) } else { None };
    let mut name = std::mem::take(&mut st.save_name);
    for resp in [Some(&aresp), save_as].into_iter().flatten() {
        egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Preset name").desired_width(170.0));
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || enter) && !name.trim().is_empty() {
                p.actions.push(format!("save:{}", name.trim()));
                name.clear();
                ui.close();
            }
        });
    }
    st.save_name = name;
    // Limiter, Vol
    let lim = g(p, ix, lg_synth::alchemy::LIMITER) >= 0.5;
    if rack::flat(p, p.rect(990.0, 9.0, 54.0, 22.0), "Limiter", lim, true, "limiter") {
        p.put(lg_synth::alchemy::LIMITER, if lim { 0.0 } else { 1.0 });
    }
    p.text(1062.0, 20.0, Align2::LEFT_CENTER, "Vol", 11.0, Color32::WHITE);
    let mut v = g(p, ix, 99);
    let kr = p.rect(1086.0, 2.0, 36.0, 36.0 + 18.0);
    if crate::widgets::knob(p.ui, kr, egui::Id::unique(("alch-outvol", p.t)), &mut v, -40.0, 12.0, 0.0, "", &|v| format!("{v:.1} dB")) {
        p.put(99, v);
    }
}

fn browse(p: &mut Pane, st: &mut St, presets: &[String], cur: &str) {
    p.ui.painter().rect_filled(p.rect(0.0, 40.0, W, 262.0), 0.0, rgb(0x1c2027));
    let sr = p.rect(16.0, 52.0, 300.0, 22.0);
    let mut q = std::mem::take(&mut st.search);
    p.ui.put(sr, egui::TextEdit::singleline(&mut q).hint_text("Search presets"));
    let list: Vec<&String> = presets.iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q.to_lowercase())).collect();
    st.search = q;
    let r = p.rect(16.0, 82.0, W - 32.0, 212.0);
    let mut child = p.ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
    egui::ScrollArea::vertical().id_salt(("alch-browse", p.t)).show(&mut child, |ui| {
        if list.is_empty() {
            ui.label(egui::RichText::new("No presets: save one with Save As").color(look::text_dim()));
        }
        for n in list {
            if ui.add(egui::Button::selectable(n == cur, n.as_str())).clicked() {
                p.actions.push(format!("load:{n}"));
            }
        }
    });
}

// ---------------------------------------------------------------- Global page

fn nav(p: &mut Pane, st: &mut St) {
    p.ui.painter().rect_filled(p.rect(0.0, 40.0, W, 248.0), 0.0, rgb(0x1c2027));
    p.ui.painter().rect_filled(p.rect(4.0, 48.0, 64.0, 232.0), 0.0, rgb(0x16191e));
    for (k, n) in ["GLOBAL", "A", "B", "C", "D", "MORPH"].iter().enumerate() {
        let r = p.rect(4.0, 50.0 + k as f32 * 38.5, 64.0, 37.0);
        let resp = p.ui.interact(r, egui::Id::unique(("alch-nav", p.t, k)), Sense::click());
        let lit = st.page as usize == k;
        if (1..=4).contains(&k) {
            p.ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(0.0, 8.0), vec2(2.0, 21.0)), 0.0, BLUE);
        }
        p.ui.painter().text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, *n, look::regular(11.5), if lit { BLUE } else if resp.hovered() { Color32::WHITE } else { p.skin.text });
        if resp.clicked() {
            st.page = k as u8;
        }
    }
}

const ELEMENTS: [&str; 4] = ["VA", "Additive", "Spectral", "Granular"];

fn element(p: &Pane, ix: &Ix, s: u32) -> usize {
    if g(p, ix, 313 + s) >= 0.5 { 3 } else if g(p, ix, 309 + s) >= 0.5 { 2 } else if g(p, ix, 293 + s) >= 0.5 { 1 } else { 0 }
}

fn global(p: &mut Pane, ix: &Ix, st: &mut St) {
    nav(p, st);
    let pt = p.painter();
    let line = Stroke::new(1.0, rgb(0x30353d));
    pt.line_segment([p.o + vec2(548.0, 48.0), p.o + vec2(548.0, 282.0)], line);
    pt.line_segment([p.o + vec2(838.0, 48.0), p.o + vec2(838.0, 282.0)], line);
    pt.line_segment([p.o + vec2(838.0, 172.0), p.o + vec2(W - 4.0, 172.0)], line);
    let title = |x: f32, y: f32, s: &str| pt.text(p.o + vec2(x, y), Align2::LEFT_CENTER, s, look::regular(12.5), p.skin.text);
    title(84.0, 66.0, "SOURCES");
    title(560.0, 66.0, "FILTER");
    title(852.0, 66.0, "MASTER");
    title(852.0, 192.0, "VOICES");
    // the four sources
    for s in 0..4u32 {
        let bx = 84.0 + (s % 2) as f32 * 231.0;
        let by = 82.0 + (s / 2) as f32 * 102.0;
        let on = g(p, ix, 277 + s) >= 0.5;
        if rack::flat(p, p.rect(bx + 8.0, by, 20.0, 20.0), ["A", "B", "C", "D"][s as usize], on, true, ("src-on", s)) {
            p.put(277 + s, if on { 0.0 } else { 1.0 });
        }
        let el = element(p, ix, s);
        let name = if !on { "Off".to_string() } else if el == 0 { lg_synth::alchemy::VA_SHAPES[(g(p, ix, 301 + s).round().max(0.0) as usize).min(3)].to_string() } else { ELEMENTS[el].to_string() };
        rack::field_btn(p, p.rect(bx + 32.0, by, 150.0, 20.0), &name, ("src-name", s), false);
        let d = rack::arrows(p, p.rect(bx + 184.0, by, 30.0, 20.0), ("src-ar", s));
        if d != 0 && el == 0 {
            let v = (g(p, ix, 301 + s) + d as f32).rem_euclid(4.0);
            p.put(301 + s, v);
        }
        for (k, (id, l)) in [(19 + s, "Vol"), (27 + s, "Tune"), (23 + s, "Pan"), (lg_synth::alchemy::ROUTE_F2 + s, "F1/F2")].iter().enumerate() {
            rack::knob(p, ix, st, bx + 28.0 + k as f32 * 50.0, by + 30.0, 38.0, *id, l);
        }
    }
    // the main filters
    for (k, (on, ty, ids)) in [(364u32, 365u32, [85u32, 86, 87]), (367, 368, [89, 90, 91])].iter().enumerate() {
        let y = 82.0 + k as f32 * 102.0;
        let lit = g(p, ix, *on) >= 0.5;
        if rack::flat(p, p.rect(560.0, y, 28.0, 20.0), "On", lit, true, ("f-on", k)) {
            p.put(*on, if lit { 0.0 } else { 1.0 });
        }
        rack::menu_arrows(p, ix, 592.0, y, 110.0, *ty);
        // the knobs take the type's names (Clean filters have no Drive)
        let t = g(p, ix, *ty).round().max(0.0) as usize;
        let names = lg_synth::alchemy_filter::knob_names(t);
        for (j, id) in ids.iter().enumerate() {
            if j == 2 && !lg_synth::alchemy_filter::has_drive(t) {
                continue;
            }
            rack::knob(p, ix, st, 594.0 + j as f32 * 50.0, y + 30.0, 38.0, *id, names[j]);
        }
        rack::knob(p, ix, st, 790.0, y + 30.0, 38.0, [88u32, 92][k], "FX Main");
    }
    rack::knob(p, ix, st, 742.0, 150.0, 28.0, 93, "Par/Ser");
    // master
    for (k, (id, l, x)) in [(94u32, "Vol", 884.0f32), (95, "Pan", 934.0), (96, "Coarse", 1050.0), (97, "Fine", 1100.0)].iter().enumerate() {
        let _ = k;
        rack::knob(p, ix, st, *x, 80.0, 38.0, *id, l);
    }
    // voices
    rack::flat(p, p.rect(936.0, 182.0, 30.0, 20.0), "All", true, true, "v-all");
    for (k, l) in ["A", "B", "C", "D"].iter().enumerate() {
        rack::flat(p, p.rect(976.0 + k as f32 * 21.0, 182.0, 20.0, 20.0), l, false, false, ("v-src", k));
    }
    p.menu(852.0, 210.0, 86.0, 20.0, 371);
    p.text(852.0, 238.0, Align2::LEFT_CENTER, "Mode", 10.0, p.skin.dim);
    // Num: the voices (whole numbers)
    let num = g(p, ix, 376).round();
    let nr = p.rect(946.0, 210.0, 48.0, 20.0);
    let resp = rack::field_btn(p, nr, &format!("{num:.0}"), "num", true);
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for n in 1..=32 {
                if ui.add(egui::Button::selectable(n as f32 == num, n.to_string())).clicked() {
                    pick = Some(n as f32);
                }
            }
        });
    });
    if let Some(n) = pick {
        p.put(376, n);
    }
    p.text(946.0, 238.0, Align2::LEFT_CENTER, "Num", 10.0, p.skin.dim);
    p.field(946.0, 248.0, 38.0, 20.0, 391, 1.0);
    p.field(988.0, 248.0, 38.0, 20.0, 396, 1.0);
    p.text(946.0, 276.0, Align2::LEFT_CENTER, "Up-Bend-Down", 10.0, p.skin.dim);
    rack::knob(p, ix, st, 1100.0, 196.0, 34.0, 98, "Glide");
}

// ---------------------------------------------------------------- Perform / Arp / Effects

fn perform(p: &mut Pane, ix: &Ix, st: &mut St, fx: &[Option<(Vec<lg_dsp::Param>, Vec<f32>)>], y0: f32) {
    let light = rgb(0xc9cacd);
    let dark_text = rgb(0x2a2c30);
    p.ui.painter().rect_filled(p.rect(0.0, y0 - 4.0, W, 212.0), 0.0, light);
    p.ui.painter().rect_filled(p.rect(4.0, y0, 64.0, 204.0), 0.0, rgb(0x9a9ca1));
    for (k, n) in ["PERFORM", "ARP", "EFFECTS"].iter().enumerate() {
        let r = p.rect(4.0, y0 + k as f32 * 38.0, 64.0, 37.0);
        let resp = p.ui.interact(r, egui::Id::unique(("alch-lower", p.t, k)), Sense::click());
        let lit = st.lower as usize == k;
        if lit {
            p.ui.painter().rect_filled(r, 0.0, light);
        }
        p.ui.painter().text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, *n, look::regular(11.5), if lit { rgb(0x5a5d63) } else { Color32::WHITE });
        if resp.clicked() {
            st.lower = k as u8;
        }
    }
    if st.lower != 0 {
        let r = p.rect(80.0, y0, W - 88.0, 200.0);
        p.ui.painter().rect_filled(r, 0.0, rgb(0x23272e));
        let (vals, ixr) = (p.vals, ix);
        let get = |id: u32| ix_get(ixr, vals, id);
        let mut set = Vec::new();
        let mut child = p.ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(6.0)).layout(egui::Layout::top_down(egui::Align::Min)));
        egui::ScrollArea::vertical().id_salt(("alch-lower-pane", p.t, st.lower)).show(&mut child, |ui| {
            if st.lower == 1 {
                arp_page(ui, &get, &mut set);
            } else {
                fx_page(ui, &get, fx, &mut set);
            }
        });
        p.set.extend(set);
        return;
    }
    // the Transform pad: eight snapshots, the framing box follows the pad position
    let pad = p.rect(84.0, y0 + 4.0, 425.0, 152.0);
    let pt = p.painter();
    pt.rect_filled(pad, 2.0, rgb(0x1a1d22));
    let (cw, chh) = (pad.width() / 4.0, pad.height() / 2.0);
    for k in 0..8 {
        let c = Rect::from_min_size(pad.min + vec2((k % 4) as f32 * cw, (k / 4) as f32 * chh), vec2(cw, chh));
        pt.rect_stroke(c.shrink(1.5), 0.0, Stroke::new(1.0, rgb(0x2c3036)), egui::StrokeKind::Inside);
        pt.text(c.center(), Align2::CENTER_CENTER, format!("Snapshot {}", k + 1), look::regular(11.5), Color32::WHITE);
    }
    let (px, py) = (g(p, ix, 17), g(p, ix, 18));
    let fc = pos2(pad.min.x + px * pad.width(), pad.max.y - py * pad.height());
    let fr = Rect::from_center_size(fc, vec2(cw - 8.0, chh - 8.0));
    for (a, b, c) in [(fr.left_top(), vec2(12.0, 0.0), vec2(0.0, 12.0)), (fr.right_top(), vec2(-12.0, 0.0), vec2(0.0, 12.0)), (fr.left_bottom(), vec2(12.0, 0.0), vec2(0.0, -12.0)), (fr.right_bottom(), vec2(-12.0, 0.0), vec2(0.0, -12.0))] {
        pt.line_segment([a, a + b], Stroke::new(2.0, BLUE));
        pt.line_segment([a, a + c], Stroke::new(2.0, BLUE));
    }
    let resp = p.ui.interact(pad, egui::Id::unique(("alch-tpad", p.t)), Sense::click_and_drag());
    if let Some(q) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let (mut fx_, mut fy) = (((q.x - pad.min.x) / pad.width()).clamp(0.0, 1.0), ((pad.max.y - q.y) / pad.height()).clamp(0.0, 1.0));
        if resp.clicked() {
            // a click selects that snapshot (the box centred on it)
            fx_ = ((fx_ * 4.0).floor().min(3.0) + 0.5) / 4.0;
            fy = ((fy * 2.0).floor().min(1.0) + 0.5) / 2.0;
        }
        p.put(17, fx_);
        p.put(18, fy);
    }
    // the eight knobs, labelled by what they control
    let label = |p: &Pane, x: f32, y: f32, s: &str| p.text(x, y, Align2::CENTER_CENTER, s, 11.0, dark_text);
    for k in 0..8u32 {
        let x = 553.0 + (k % 4) as f32 * 66.0;
        let y = y0 + 12.0 + (k / 4) as f32 * 82.0;
        let name = p.prm(k + 1).name;
        let mut v = g(p, ix, k + 1);
        let kr = p.rect(x - 19.0, y, 38.0, 38.0 + 18.0);
        if crate::widgets::knob(p.ui, kr, egui::Id::unique(("alch-perf", p.t, k)), &mut v, 0.0, 1.0, p.prm(k + 1).default, "", &|v| format!("{:.0} %", v * 100.0)) {
            p.put(k + 1, v);
        }
        p.text(x, y + 48.0, Align2::CENTER_CENTER, name, 10.0, dark_text);
    }
    // the XY pads
    for (k, (ix_, iy)) in [(9u32, 10u32), (11, 12)].iter().enumerate() {
        let x = 790.0 + k as f32 * 120.0;
        let xn = p.prm(*ix_).name;
        let yn = p.prm(*iy).name;
        p.text(x, y0 + 12.0, Align2::LEFT_CENTER, &format!("X{}: {xn}", k + 1), 10.5, dark_text);
        p.xy(x, y0 + 22.0, 110.0, 110.0, *ix_, *iy);
        p.text(x + 55.0, y0 + 144.0, Align2::CENTER_CENTER, &format!("Y{}: {yn}", k + 1), 10.5, dark_text);
    }
    // the envelope knobs
    for (k, (id, n)) in [(13u32, "Attack"), (14, "Decay"), (15, "Sustain"), (16, "Release")].iter().enumerate() {
        let x = 1060.0 + (k % 2) as f32 * 52.0;
        let y = y0 + 12.0 + (k / 2) as f32 * 82.0;
        let mut v = g(p, ix, *id);
        let kr = p.rect(x - 19.0, y, 38.0, 38.0 + 18.0);
        if crate::widgets::knob(p.ui, kr, egui::Id::unique(("alch-penv", p.t, k)), &mut v, 0.0, 1.0, p.prm(*id).default, "", &|v| format!("{:.0} %", v * 100.0)) {
            p.put(*id, v);
        }
        label(p, x, y + 48.0, n);
    }
    let _ = ORANGE;
}

fn row(ui: &mut egui::Ui, label: &str, pid: u32, min: f32, max: f32, get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.5).color(look::text_dim()));
        let mut v = get(pid);
        if ui.add(egui::Slider::new(&mut v, min..=max)).changed() {
            set.push((pid, v));
        }
    });
}

fn menu(ui: &mut egui::Ui, label: &str, pid: u32, choices: &[&str], get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.5).color(look::text_dim()));
        let cur = get(pid).round().max(0.0) as usize;
        ui.menu_button(format!("{} ⌄", choices.get(cur).copied().unwrap_or("")), |ui| {
            for (k, c) in choices.iter().enumerate() {
                if ui.add(egui::Button::selectable(k == cur, *c)).clicked() {
                    set.push((pid, k as f32));
                    ui.close();
                }
            }
        });
    });
}

fn check(ui: &mut egui::Ui, label: &str, pid: u32, get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    let mut on = get(pid) >= 0.5;
    if ui.checkbox(&mut on, label).changed() {
        set.push((pid, if on { 1.0 } else { 0.0 }));
    }
}

fn arp_page(ui: &mut egui::Ui, get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    let rates: Vec<&str> = lg_synth::arp::ARP_RATES.iter().map(|r| r.0).collect();
    ui.horizontal(|ui| {
        check(ui, "Arpeggiator On", ax::ARP, get, set);
        check(ui, "Latch", ax::ARP + 6, get, set);
        check(ui, "Step Velocity", ax::ARP + 8, get, set);
    });
    menu(ui, "Mode", ax::ARP + 1, lg_synth::arp::ARP_MODES, get, set);
    menu(ui, "Rate", ax::ARP + 3, &rates, get, set);
    row(ui, "Octaves", ax::ARP + 2, 1.0, 4.0, get, set);
    row(ui, "Note Length", ax::ARP + 4, 0.05, 1.0, get, set);
    row(ui, "Swing", ax::ARP + 5, 0.0, 1.0, get, set);
    row(ui, "Steps", ax::ARP + 7, 1.0, 16.0, get, set);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Sequencer: click a step to turn it on or off, drag its bar for the velocity; the lower strip is its length").size(11.0).color(look::text_dim()));
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 6.0, look::lane_bg());
    let steps = get(ax::ARP + 7).round().clamp(1.0, 16.0) as usize;
    let w = r.width() / 16.0;
    for k in 0..16 {
        let cell = Rect::from_min_size(pos2(r.min.x + k as f32 * w, r.min.y), vec2(w, r.height()));
        let bar = Rect::from_min_max(cell.min + vec2(3.0, 4.0), pos2(cell.max.x - 3.0, cell.max.y - 30.0));
        let len_r = Rect::from_min_max(pos2(cell.min.x + 3.0, cell.max.y - 24.0), cell.max - vec2(3.0, 4.0));
        let on = get(ax::STEP_ON + k as u32) >= 0.5;
        let vel = get(ax::STEP_VEL + k as u32);
        let len = get(ax::STEP_LEN + k as u32);
        let used = k < steps;
        let col = if !used { Color32::from_gray(0x40) } else if on { look::accent() } else { Color32::from_gray(0x60) };
        p.rect_filled(Rect::from_min_max(pos2(bar.min.x, bar.max.y - bar.height() * vel), bar.max), 3.0, col);
        p.rect_stroke(bar, 3.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
        p.rect_filled(Rect::from_min_size(len_r.min, vec2(len_r.width() * len, len_r.height())), 2.0, col.gamma_multiply(0.7));
        let resp = ui.interact(bar, ui.scope_id().with(("step", k)), Sense::click_and_drag());
        if resp.clicked() {
            set.push((ax::STEP_ON + k as u32, if on { 0.0 } else { 1.0 }));
        }
        if resp.dragged()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            set.push((ax::STEP_VEL + k as u32, ((bar.max.y - pp.y) / bar.height()).clamp(0.0, 1.0)));
        }
        let lresp = ui.interact(len_r, ui.scope_id().with(("steplen", k)), Sense::click_and_drag());
        if (lresp.dragged() || lresp.clicked())
            && let Some(pp) = lresp.interact_pointer_pos()
        {
            set.push((ax::STEP_LEN + k as u32, ((pp.x - len_r.min.x) / len_r.width()).clamp(0.05, 1.0)));
        }
    }
}

fn fx_page(ui: &mut egui::Ui, get: &dyn Fn(u32) -> f32, fx: &[Option<(Vec<lg_dsp::Param>, Vec<f32>)>], set: &mut Vec<(u32, f32)>) {
    egui::ScrollArea::vertical().id_salt("alchemy-fx").show(ui, |ui| {
        for s in 0..FX_SLOTS {
            let b = FX + s as u32 * 10;
            egui::Frame::new().fill(look::group_bg()).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Effect {}", s + 1)).strong());
                    menu(ui, "", b, FX_TYPES, get, set);
                    check(ui, "Bypass", b + 2, get, set);
                    row(ui, "Mix", b + 1, 0.0, 1.0, get, set);
                });
                if let Some(Some((params, vals))) = fx.get(s) {
                    egui::Grid::new(("alch-fx", s)).num_columns(4).show(ui, |ui| {
                        for (i, prm) in params.iter().enumerate().take(24) {
                            ui.label(egui::RichText::new(prm.name).size(11.0).color(look::text_dim()));
                            let mut v = vals[i];
                            let id = FX_PARAM + s as u32 * 100 + prm.id;
                            let ch = match prm.unit {
                                lg_dsp::Unit::OnOff => {
                                    let mut on = v >= 0.5;
                                    let c = ui.checkbox(&mut on, "").changed();
                                    v = if on { 1.0 } else { 0.0 };
                                    c
                                }
                                lg_dsp::Unit::Menu => {
                                    let mut c = false;
                                    ui.menu_button(prm.choices.get(v.round() as usize).copied().unwrap_or(""), |ui| {
                                        for (j, n) in prm.choices.iter().enumerate() {
                                            if ui.button(*n).clicked() {
                                                v = j as f32;
                                                c = true;
                                                ui.close();
                                            }
                                        }
                                    });
                                    c
                                }
                                _ => ui.add(egui::DragValue::new(&mut v).range(prm.min..=prm.max).speed((prm.max - prm.min) / 300.0)).changed(),
                            };
                            if ch {
                                set.push((id, v));
                            }
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                }
            });
        }
    });
}

/// Imported audio into the running Alchemy (mono, tuned by its detected pitch).
pub fn apply(a: &lg_synth::wav::Audio, al: &mut lg_synth::alchemy::Alchemy) {
    let n = a.frames();
    let ch = a.channels.len().max(1) as f32;
    let mono: Vec<f32> = (0..n).map(|i| a.channels.iter().map(|c| c.get(i).copied().unwrap_or(0.0)).sum::<f32>() / ch).collect();
    let data = lg_synth::sampler::SampleData { rate: a.rate as f32, ch: vec![lg_synth::sampler::Pcm::F32(mono.clone())] };
    let hz = lg_synth::sampler_doc::detect_pitch(&data).map(|(k, c)| lg_dsp::midi_hz(k as f32 + c / 100.0)).unwrap_or(261.63);
    al.set_granular_sample(mono, a.rate as f32, hz);
}

fn sources_page(ui: &mut egui::Ui, t: usize, src: u32, params: &[lg_dsp::Param], get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>, import: &mut bool, audio: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("SOURCE {}", ["A", "B", "C", "D"][src as usize])).size(12.5));
        ui.separator();
        if ui.button("Import Audio…").on_hover_text("Or drop an audio file on the window").clicked() {
            *import = true;
        }
        ui.label(egui::RichText::new(audio.unwrap_or("no audio imported")).color(look::text_dim()));
    });
    let letter = ["A", "B", "C", "D"][src as usize];
    ui.horizontal(|ui| {
        check(ui, "On", 277 + src, get, set);
        ui.label(egui::RichText::new("Element").color(look::text_dim()));
        // one element at a time (the engine plays the first on of Granular, Spectral, Additive, VA)
        let cur = if get(313 + src) >= 0.5 { 3 } else if get(309 + src) >= 0.5 { 2 } else if get(293 + src) >= 0.5 { 1 } else { 0 };
        for (k, n) in ["VA", "Additive", "Spectral", "Granular"].iter().enumerate() {
            if ui.add(egui::Button::selectable(cur == k, *n)).clicked() {
                set.push((297 + src, if k == 0 { 1.0 } else { 0.0 }));
                set.push((293 + src, if k == 1 { 1.0 } else { 0.0 }));
                set.push((309 + src, if k == 2 { 1.0 } else { 0.0 }));
                set.push((313 + src, if k == 3 { 1.0 } else { 0.0 }));
            }
        }
    });
    let suffix = format!(" {letter}");
    let own: Vec<lg_dsp::Param> = params.iter().filter(|p| p.id < 1000 && p.name.ends_with(&suffix) && !p.name.contains(" On ") && !p.name.starts_with("Filter ") && !p.name.starts_with("Source On") && !p.name.ends_with(&format!("On{suffix}"))).cloned().collect();
    let vals: Vec<f32> = own.iter().map(|p| get(p.id)).collect();
    set.extend(crate::pluginwin::controls(ui, &own, &vals, &[], &format!("alch-src-{t}-{src}")));
}
