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
use lg_synth::alchemy_extra::FX_SLOTS;
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
    // checks (headless): LG_ALCH_EDIT=1 opens source A's page and its edit window once
    // (LG_ALCH_EDIT=page: the source page only)
    static EDIT_ONCE: std::sync::Once = std::sync::Once::new();
    if std::env::var("LG_ALCH_EDIT").is_ok() {
        EDIT_ONCE.call_once(|| {
            st.view = 0;
            st.page = 1;
            st.ed.open = std::env::var("LG_ALCH_EDIT").as_deref() != Ok("page");
            st.ed.sel = vec![(0, 0)];
        });
    }
    let mut live = instwin::with_inst(app, t, |a: &mut lg_synth::alchemy::Alchemy| Live { levels: a.mg.levels, perform: a.mg.perform, envf: a.mg.envf, time: 0.0 }).unwrap_or_default();
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
    // the source edit window's zones, the newest zone's play position and the notes played
    let mut zdoc = instwin::track_id(app, t).and_then(|id| app.inst.alch_zones.get(&id).cloned()).unwrap_or_default();
    let zsrc = st.ed.src;
    let zpos = instwin::with_inst(app, t, |a: &mut lg_synth::alchemy::Alchemy| a.zone_position(zsrc)).flatten();
    let notes = crate::alchemyedit::take_notes();
    let size = match st.view {
        1 => vec2(W, 258.0),
        2 => vec2(W, 520.0),
        _ => vec2(W, 738.0),
    };
    let mut ext: Vec<u32> = (0..5).map(|k| am::CTRL_CC + k).collect();
    ext.extend([lg_synth::alchemy::MONO_MODE, lg_synth::alchemy::MONO_BEND, lg_synth::alchemy_arp::OVERLAP_LEGACY]);
    let actions = synthui::window(app, ctx, t, "Alchemy", size, SKIN, &ext, |p| body(p, &mut st, &live, &fx_params, audio_name.as_deref(), &presets, &cur, &mut zdoc, zpos, &notes));
    let zdirty = std::mem::take(&mut st.ed.dirty);
    ctx.data_mut(|d| d.insert_temp(st_id, st));
    if zdirty {
        instwin::checkpoint_once(app, ctx, t, "Edit Zones");
        store_zones(app, t, zdoc.clone());
    }
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
            v if v.starts_with('z') => zone_action(app, ctx, st_id, t, v, arg, &mut zdoc),
            "arp-import" => arp_import(app, t, arg),
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

/// Initialize Preset: a basic sawtooth (source A, wide unison on), AHDSR 1 and Velocity on
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
    // Initialize Preset turns wide unison mode on (loaded presets keep the older mode)
    put(lg_synth::alchemy::VOICE + 2, 1.0);
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

/// The arpeggiator's File > Import MIDI: a monophonic file onto the pattern's sequences.
fn arp_import(app: &mut App, t: usize, arg: &str) {
    use lg_synth::alchemy_arp as aa;
    let Ok(pi) = arg.parse::<usize>() else { return };
    let Some(path) = kn_ui::files::FileDialog::new().set_title("Import MIDI File").add_filter("MIDI", &["mid", "midi", "MID"]).pick_file() else { return };
    let Some(imp) = std::fs::read(&path).ok().and_then(|d| lg_core::midi::read(&d)) else {
        app.tell(format!("Couldn't read {}", path.display()));
        return;
    };
    let mut notes: Vec<(u32, u8, u8)> = imp.tracks.iter().flat_map(|(_, _, n)| n.iter().map(|n| (n.pos, n.pitch, n.velocity))).collect();
    notes.sort();
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let rate = params.iter().position(|p| p.id == aa::RATE).map(|k| vals[k]).unwrap_or(5.0);
    let beats = aa::RATE_BEATS[(rate.round().max(0.0) as usize).min(aa::RATE_BEATS.len() - 1)];
    let pairs: Vec<(u32, f32)> = aa::import_midi(&notes, 960, beats).into_iter().map(|(s, k, v)| if s == usize::MAX { (aa::pat(pi, 0), v) } else { (aa::step(pi, s, k), v) }).collect();
    instwin::checkpoint(app, t, "Import MIDI to Arpeggiator");
    instwin::set_params(app, t, &pairs);
}

#[allow(clippy::too_many_arguments)]
fn body(p: &mut Pane, st: &mut St, live: &Live, fx: &[Option<(Vec<lg_dsp::Param>, Vec<f32>)>], audio: Option<&str>, presets: &[String], cur: &str, zdoc: &mut lg_synth::alchemy_zones::ZoneDoc, zpos: Option<(usize, usize, f64)>, notes: &[u8]) {
    let ix = Ix::new(p.params);
    st.levels = live.levels;
    st.perform = live.perform;
    name_bar(p, &ix, st, presets, cur);
    // the source edit window covers everything below the Name bar (Advanced view)
    if st.view == 0 && st.ed.open {
        let mut ed = std::mem::take(&mut st.ed);
        crate::alchemyedit::body(p, &ix, st, &mut ed, zdoc, zpos, notes);
        st.ed = ed;
        return;
    }
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
                    morph_page(p, &ix, st);
                }
                s => {
                    nav(p, st);
                    source_filters(p, &ix, st, (s - 1) as u32);
                    source_page(p, &ix, st, (s - 1) as u32, audio, zdoc, zpos);
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
            // blue: the source is on; green: it's being morphed; none: off
            let on = p.get(277 + k as u32 - 1) >= 0.5;
            let morphing = p.get(lg_synth::alchemy::MORPH) >= 2.5;
            // yellow: soloed
            let solo_on = soloed(p) == Some(k) && on;
            if on {
                p.ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(0.0, 8.0), vec2(2.0, 21.0)), 0.0, if solo_on { rgb(0xe8c547) } else if morphing { rgb(0x5ac46a) } else { BLUE });
            }
        }
        p.ui.painter().text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, *n, look::regular(11.5), if lit { BLUE } else if resp.hovered() { Color32::WHITE } else { p.skin.text });
        if resp.clicked() {
            st.page = k as u8;
            // Option-click: view and solo the source (again: the other sources back as they were)
            if (1..=4).contains(&k) && p.ui.input(|i| i.modifiers.alt) {
                solo(p, k);
            }
        }
    }
}

/// The source soloed (1–4) by an Option-click, if any.
pub(crate) fn soloed(p: &Pane) -> Option<usize> {
    p.ui.data(|d| d.get_temp::<(usize, [f32; 4])>(egui::Id::unique(("alch-solo", p.t)))).map(|x| x.0)
}

/// Solos source `k` (1–4) — only it on — or, when it's soloed, puts the sources back.
pub(crate) fn solo(p: &mut Pane, k: usize) {
    let sid = egui::Id::unique(("alch-solo", p.t));
    let saved: Option<(usize, [f32; 4])> = p.ui.data(|d| d.get_temp(sid));
    match saved {
        Some((who, prev)) if who == k => {
            for j in 0..4u32 {
                p.put(277 + j, prev[j as usize]);
            }
            p.ui.data_mut(|d| d.remove::<(usize, [f32; 4])>(sid));
        }
        _ => {
            let prev = saved.map(|x| x.1).unwrap_or([0, 1, 2, 3].map(|j| p.get(277 + j)));
            for j in 0..4u32 {
                p.put(277 + j, if j as usize == k - 1 { 1.0 } else { 0.0 });
            }
            p.ui.data_mut(|d| d.insert_temp(sid, (k, prev)));
        }
    }
}

const ELEMENTS: [&str; 6] = ["VA", "Additive", "Spectral", "Granular", "Formant", "Sampler"];

fn element(p: &Pane, ix: &Ix, s: u32) -> usize {
    if g(p, ix, lg_synth::alchemy_zones::SAMP_ON + s) >= 0.5 {
        5
    } else if g(p, ix, 313 + s) >= 0.5 { 3 } else if g(p, ix, 309 + s) >= 0.5 { 2 } else if g(p, ix, 293 + s) >= 0.5 { 1 } else { 0 }
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
            // Option-click solos the source
            if p.ui.input(|i| i.modifiers.alt) {
                solo(p, s as usize + 1);
            } else {
                p.put(277 + s, if on { 0.0 } else { 1.0 });
            }
        }
        let el = element(p, ix, s);
        let name = if !on { "Off".to_string() } else if el == 0 { lg_synth::alchemy::VA_SHAPES[(g(p, ix, 301 + s).round().max(0.0) as usize).min(3)].to_string() } else { ELEMENTS[el].to_string() };
        let nr = p.rect(bx + 32.0, by, 150.0, 20.0);
        rack::field_btn(p, nr, &name, ("src-name", s), false);
        // the source's output level (to the main filters or straight to the effects)
        rack::level_fill(p, nr, st.levels[s as usize]);
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
        if !lit {
            // a filter that's off: its controls dimmed, as Logic shows them
            p.ui.painter().rect_filled(p.rect(592.0, y + 26.0, 230.0, 62.0), 0.0, rgb(0x1c2027).gamma_multiply(0.55));
        }
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
    // Voice Priority, and Glide Rate | Time (a joined pill)
    p.menu(852.0, 248.0, 86.0, 20.0, lg_synth::alchemy::VOICE);
    p.text(852.0, 276.0, Align2::LEFT_CENTER, "Priority", 10.0, p.skin.dim);
    let gm = g(p, ix, lg_synth::alchemy::VOICE + 1).round() as usize;
    for (k, l) in ["Rate", "Time"].iter().enumerate() {
        if rack::flat(p, p.rect(1066.0 + k as f32 * 34.0, 254.0, 34.0, 20.0), l, gm == k, true, ("glide-mode", k)) {
            p.put(lg_synth::alchemy::VOICE + 1, k as f32);
        }
    }
}

/// The VA Wave pop-up (its last entry turns wide unison mode on) and its ‹ › arrows.
fn va_wave(p: &mut Pane, ix: &Ix, s: u32) {
    use lg_synth::alchemy::{VA_SHAPES, VOICE};
    let cur = g(p, ix, 301 + s).round().clamp(0.0, (VA_SHAPES.len() - 1) as f32) as usize;
    let wide = g(p, ix, VOICE + 2) >= 0.5;
    let resp = rack::field_btn(p, p.rect(124.0, 88.0, 100.0, 20.0), VA_SHAPES[cur], ("va-wave", s), true);
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        for (k, n) in VA_SHAPES.iter().enumerate() {
            if ui.add(egui::Button::selectable(k == cur, *n)).clicked() {
                pick = Some((301 + s, k as f32));
            }
        }
        ui.separator();
        if ui.add(egui::Button::selectable(wide, "Wide Unison")).clicked() {
            pick = Some((VOICE + 2, if wide { 0.0 } else { 1.0 }));
        }
    });
    if let Some((id, v)) = pick {
        p.put(id, v);
    }
    let d = rack::arrows(p, p.rect(226.0, 88.0, 30.0, 20.0), ("va-wave-ar", s));
    if d != 0 {
        p.put(301 + s, (cur as i32 + d).clamp(0, VA_SHAPES.len() as i32 - 1) as f32);
    }
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
    if st.lower == 1 {
        p.ui.painter().rect_filled(p.rect(80.0, y0, W - 88.0, 200.0), 0.0, rgb(0x23272e));
        arp_pane(p, ix, st, 86.0, y0);
        return;
    }
    if st.lower == 2 {
        p.ui.painter().rect_filled(p.rect(80.0, y0, W - 88.0, 200.0), 0.0, rgb(0x23272e));
        effects_pane(p, ix, st, 86.0, y0);
        let _ = fx;
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
    // Control-click: the snapshot commands; the Action button (gear) below has them too
    let mut cmd = None;
    let (upd, can_paste) = (g(p, ix, lg_synth::alchemy::SNAP_UPDATE) >= 0.5, st.snap_clip.is_some());
    resp.context_menu(|ui| snapshot_menu(ui, upd, can_paste, &mut cmd));
    let gear = p.rect(84.0, y0 + 164.0, 46.0, 22.0);
    let gresp = p.ui.interact(gear, egui::Id::unique(("alch-tgear", p.t)), Sense::click());
    p.ui.painter().rect_filled(gear, 3.0, if gresp.hovered() { rgb(0x4a4f58) } else { rgb(0x3a3e45) });
    crate::glyphs::paint(p.ui.painter(), gear, crate::glyphs::ph_fill::GEAR, 13.0, Color32::WHITE);
    egui::Popup::menu(&gresp).show(|ui| snapshot_menu(ui, upd, can_paste, &mut cmd));
    if let Some(c) = cmd {
        snapshot_command(p, ix, st, c);
    }
    // Octave, Rate, ModWheel, Snap Vol (the selected snapshot's)
    {
        use lg_synth::alchemy::{nearest_snapshot, SNAPVOL, SNAP_OCTAVE, SNAP_RATE, SNAP_WHEEL};
        let sel = nearest_snapshot(g(p, ix, 17), g(p, ix, 18)) as u32;
        p.menu(150.0, y0 + 164.0, 80.0, 22.0, SNAP_OCTAVE);
        p.field(244.0, y0 + 164.0, 66.0, 22.0, SNAP_RATE, 0.01);
        p.menu(324.0, y0 + 164.0, 104.0, 22.0, SNAP_WHEEL);
        p.field(442.0, y0 + 164.0, 66.0, 22.0, SNAPVOL + sel, 0.1);
        for (x, l) in [(190.0, "Octave"), (277.0, "Rate"), (376.0, "ModWheel"), (475.0, "Snap Vol")] {
            p.text(x, y0 + 196.0, Align2::CENTER_CENTER, l, 10.0, rgb(0x5a5d63));
        }
    }
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
        perform_menu(p, ix, st, kr, &[k as usize]);
        p.text(x, y + 48.0, Align2::CENTER_CENTER, name, 10.0, dark_text);
    }
    // the XY pads
    for (k, (ix_, iy)) in [(9u32, 10u32), (11, 12)].iter().enumerate() {
        let x = 790.0 + k as f32 * 120.0;
        let xn = p.prm(*ix_).name;
        let yn = p.prm(*iy).name;
        p.text(x, y0 + 12.0, Align2::LEFT_CENTER, &format!("X{}: {xn}", k + 1), 10.5, dark_text);
        let pr = p.xy(x, y0 + 22.0, 110.0, 110.0, *ix_, *iy);
        perform_menu(p, ix, st, pr, &[*ix_ as usize - 1, *iy as usize - 1]);
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
        perform_menu(p, ix, st, kr, &[*id as usize - 1]);
        label(p, x, y + 48.0, n);
    }
    let _ = ORANGE;
}




/// Control-click on a Perform knob, envelope knob or XY pad (its two axes): Copy Setting to all
/// Snapshots and Lock Control, per control.
fn perform_menu(p: &mut Pane, ix: &Ix, st: &St, r: Rect, ks: &[usize]) {
    use lg_synth::alchemy::{snapv, SNAP_LOCK};
    let resp = p.ui.interact(r, egui::Id::unique(("alch-perf-menu", p.t, ks[0])), Sense::click());
    let axis = |i: usize| if ks.len() > 1 { [" X", " Y"][i] } else { "" };
    let locks: Vec<bool> = ks.iter().map(|&k| g(p, ix, SNAP_LOCK + k as u32) >= 0.5).collect();
    let mut act: Option<(usize, bool)> = None;
    resp.context_menu(|ui| {
        for (i, _) in ks.iter().enumerate() {
            if ui.button(format!("Copy{} Setting to all Snapshots", axis(i))).clicked() {
                act = Some((i, false));
                ui.close();
            }
        }
        ui.separator();
        for (i, _) in ks.iter().enumerate() {
            if ui.add(egui::Button::selectable(locks[i], format!("Lock Control{}", axis(i)))).clicked() {
                act = Some((i, true));
                ui.close();
            }
        }
    });
    match act {
        Some((i, false)) => {
            let k = ks[i];
            for n in 0..8 {
                p.put(snapv(n, k), st.perform[k]);
            }
        }
        Some((i, true)) => p.put(SNAP_LOCK + ks[i] as u32, if locks[i] { 0.0 } else { 1.0 }),
        None => {}
    }
}

/// The Transform pad's snapshot commands.
#[derive(Clone, Copy)]
enum SnapCmd {
    Copy,
    Paste,
    CopyFirstToAll,
    Randomize,
    Update,
    StoreCurrent,
    StoreTo(Option<usize>),
    Swap(usize),
}

fn snapshot_menu(ui: &mut egui::Ui, update: bool, can_paste: bool, cmd: &mut Option<SnapCmd>) {
    let mut pick = |ui: &mut egui::Ui, b: egui::Response, c: SnapCmd| {
        if b.clicked() {
            *cmd = Some(c);
            ui.close();
        }
    };
    let b = ui.button("Copy Snapshot");
    pick(ui, b, SnapCmd::Copy);
    let b = ui.add_enabled(can_paste, egui::Button::new("Paste Snapshot"));
    pick(ui, b, SnapCmd::Paste);
    let b = ui.button("Copy 1 To All");
    pick(ui, b, SnapCmd::CopyFirstToAll);
    let b = ui.button("Randomize Snapshots");
    pick(ui, b, SnapCmd::Randomize);
    let b = ui.add(egui::Button::selectable(update, "Snapshot Update"));
    pick(ui, b, SnapCmd::Update);
    ui.separator();
    let b = ui.button("Store Current Snapshot");
    pick(ui, b, SnapCmd::StoreCurrent);
    ui.menu_button("Store Snapshot", |ui| {
        for k in 0..8 {
            let b = ui.button((k + 1).to_string());
            pick(ui, b, SnapCmd::StoreTo(Some(k)));
        }
        let b = ui.button("All");
        pick(ui, b, SnapCmd::StoreTo(None));
    });
    ui.menu_button("Swap Current Snapshot", |ui| {
        for k in 0..8 {
            let b = ui.button((k + 1).to_string());
            pick(ui, b, SnapCmd::Swap(k));
        }
    });
}

/// A snapshot's 16 Perform values and its Snap Vol.
fn snapshot_values(p: &Pane, ix: &Ix, k: usize) -> [f32; 17] {
    use lg_synth::alchemy::{snapv, SNAPVOL};
    let mut v = [0.0; 17];
    for i in 0..16 {
        v[i] = g(p, ix, snapv(k, i));
    }
    v[16] = g(p, ix, SNAPVOL + k as u32);
    v
}

fn put_snapshot(p: &mut Pane, k: usize, v: &[f32; 17]) {
    use lg_synth::alchemy::{snapv, SNAPVOL};
    for i in 0..16 {
        p.put(snapv(k, i), v[i]);
    }
    p.put(SNAPVOL + k as u32, v[16]);
}

fn snapshot_command(p: &mut Pane, ix: &Ix, st: &mut St, c: SnapCmd) {
    use lg_synth::alchemy::{nearest_snapshot, SNAP_UPDATE};
    let sel = nearest_snapshot(g(p, ix, 17), g(p, ix, 18));
    match c {
        SnapCmd::Copy => st.snap_clip = Some(snapshot_values(p, ix, sel)),
        SnapCmd::Paste => {
            if let Some(v) = st.snap_clip {
                put_snapshot(p, sel, &v);
            }
        }
        SnapCmd::CopyFirstToAll => {
            let v = snapshot_values(p, ix, 0);
            for k in 1..8 {
                put_snapshot(p, k, &v);
            }
        }
        SnapCmd::Randomize => {
            // every snapshot but the selected one
            let mut seed = (p.ui.input(|i| i.time) * 1000.0) as u32 | 1;
            for k in (0..8).filter(|&k| k != sel) {
                let mut v = snapshot_values(p, ix, k);
                for x in v.iter_mut().take(16) {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    *x = (seed % 1000) as f32 / 999.0;
                }
                put_snapshot(p, k, &v);
            }
        }
        SnapCmd::Update => {
            let on = g(p, ix, SNAP_UPDATE) >= 0.5;
            p.put(SNAP_UPDATE, if on { 0.0 } else { 1.0 });
        }
        SnapCmd::StoreCurrent => {
            let mut v = snapshot_values(p, ix, sel);
            v[..16].copy_from_slice(&st.perform);
            put_snapshot(p, sel, &v);
        }
        SnapCmd::StoreTo(to) => {
            let v = snapshot_values(p, ix, sel);
            for k in (0..8).filter(|&k| to.is_none_or(|t| t == k)) {
                put_snapshot(p, k, &v);
            }
        }
        SnapCmd::Swap(k) => {
            let (a, b) = (snapshot_values(p, ix, sel), snapshot_values(p, ix, k));
            put_snapshot(p, sel, &b);
            put_snapshot(p, k, &a);
        }
    }
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

/// A source's subpage: the element buttons (VA, Additive, Spectral, Granular) and Formant,
/// Import Audio…, and the chosen element's controls.
fn source_page(p: &mut Pane, ix: &Ix, st: &mut St, s: u32, audio: Option<&str>, zdoc: &lg_synth::alchemy_zones::ZoneDoc, zpos: Option<(usize, usize, f64)>) {
    use lg_synth::alchemy_zones as az;
    use lg_synth::alchemy_partials::{self as ap, id};
    let su = s as usize;
    p.text(84.0, 66.0, Align2::LEFT_CENTER, &format!("SOURCE {}", ["A", "B", "C", "D"][su]), 12.5, p.skin.text);
    let playing = element(p, ix, s);
    let view = match st.elem[su] {
        0 => playing,
        v => (v - 1) as usize,
    };
    let names = ["VA", "ADDITIVE", "SPECTRAL", "GRANULAR", "FORMANT", "SAMPLER"];
    let flags = [297 + s, 293 + s, 309 + s, 313 + s, id(su, ap::FM_ON), az::SAMP_ON + s];
    for (k, n) in names.iter().enumerate() {
        let r = p.rect(170.0 + k as f32 * 64.0, 56.0, 62.0, 20.0);
        if rack::flat(p, r, n, view == k, true, ("elem", s, k)) {
            st.elem[su] = k as u8 + 1;
        }
        let on = g(p, ix, flags[k]) >= 0.5;
        p.ui.painter().circle_filled(r.right_top() + vec2(-5.0, 5.0), 2.5, if on { ORANGE } else { rgb(0x44484f) });
    }
    source_select(p, ix, st, s, audio);
    p.menu(560.0, 88.0, 90.0, 20.0, lg_synth::alchemy::KEYSCALE + s);
    p.text(560.0, 116.0, Align2::LEFT_CENTER, "Keyscale", 10.0, p.skin.dim);
    rack::knob(p, ix, st, 700.0, 82.0, 30.0, lg_synth::alchemy::WAIT + s, "Wait");
    // Edit opens the source edit window; Stereo: a circle mono, a double circle stereo
    if rack::flat(p, p.rect(616.0, 118.0, 40.0, 20.0), "Edit", false, true, ("src-edit", s)) {
        st.ed.open = true;
        st.ed.src = su;
        st.ed.sel.clear();
        st.ed.group = 0;
        if let Some(gr) = zdoc.src[su].groups.first()
            && !gr.zones.is_empty()
        {
            st.ed.sel = vec![(0, 0)];
        }
    }
    let stereo = g(p, ix, az::STEREO + s) >= 0.5;
    let sr = p.rect(744.0, 118.0, 30.0, 20.0);
    let sresp = p.ui.interact(sr, egui::Id::unique(("alch-stereo", p.t, s)), Sense::click()).on_hover_text(if stereo { "Stereo" } else { "Mono" });
    p.ui.painter().rect_filled(sr, 3.0, rack::BTN);
    let c = sr.center();
    if stereo {
        p.ui.painter().circle_stroke(c - vec2(3.5, 0.0), 5.0, Stroke::new(1.2, p.skin.text));
        p.ui.painter().circle_stroke(c + vec2(3.5, 0.0), 5.0, Stroke::new(1.2, p.skin.text));
    } else {
        p.ui.painter().circle_stroke(c, 5.0, Stroke::new(1.2, p.skin.text));
    }
    if sresp.clicked() {
        p.put(az::STEREO + s, if stereo { 0.0 } else { 1.0 });
    }
    // the element's own On (the engine plays one element: Granular, else Spectral, else Additive, else VA)
    let on_id = flags[view];
    let on = g(p, ix, on_id) >= 0.5;
    if rack::flat(p, p.rect(84.0, 88.0, 30.0, 20.0), "On", on, true, ("elem-on", s, view)) {
        p.put(on_id, if on { 0.0 } else { 1.0 });
        // the Sampler and Granular elements are mutually exclusive
        if !on && view == 5 {
            p.put(313 + s, 0.0);
        } else if !on && view == 3 {
            p.put(az::SAMP_ON + s, 0.0);
        }
    }
    let k = |p: &mut Pane, st: &mut St, x: f32, y: f32, d: f32, pid: u32, l: &str| rack::knob(p, ix, st, x, y, d, pid, l);
    match view {
        0 => {
            va_wave(p, ix, s);
            for (j, (pid, l)) in [(204 + s, "Volume"), (208 + s, "Symmetry"), (212 + s, "Phase"), (216 + s, "Detune"), (250 + s, "Sync")].iter().enumerate() {
                k(p, st, 120.0 + j as f32 * 60.0, 124.0, 38.0, *pid, l);
            }
            p.knob_menu(450.0, 124.0, 38.0, 220 + s, "Num Osc");
            let nz = g(p, ix, 305 + s) >= 0.5;
            if rack::flat(p, p.rect(500.0, 132.0, 50.0, 20.0), "Noise", nz, true, ("va-noise", s)) {
                p.put(305 + s, if nz { 0.0 } else { 1.0 });
            }
            // the noise oscillator: its waveform (‹ ›), Vol, Low Cut and High Cut
            let n0 = lg_synth::alchemy::NOISE + s;
            rack::menu_arrows(p, ix, 556.0, 132.0, 80.0, n0);
            for (j, (pid, l)) in [(n0 + 4, "Vol"), (n0 + 8, "Low Cut"), (n0 + 12, "High Cut")].iter().enumerate() {
                k(p, st, 540.0 + j as f32 * 64.0, 170.0, 30.0, *pid, l);
            }
            if !nz {
                // the noise oscillator off: its controls dimmed
                p.ui.painter().rect_filled(p.rect(554.0, 128.0, 116.0, 28.0), 0.0, rgb(0x1c2027).gamma_multiply(0.55));
                p.ui.painter().rect_filled(p.rect(510.0, 166.0, 200.0, 62.0), 0.0, rgb(0x1c2027).gamma_multiply(0.55));
            }
        }
        1 => {
            let cx = g(p, ix, id(su, ap::A_COMPLEX)) >= 0.5;
            for (j, l) in ["Sine", "Complex"].iter().enumerate() {
                if rack::flat(p, p.rect(124.0 + j as f32 * 56.0, 88.0, 54.0, 20.0), l, cx == (j == 1), true, ("add-cx", s, j)) {
                    p.put(id(su, ap::A_COMPLEX), j as f32);
                }
            }
            if cx {
                rack::menu_arrows(p, ix, 240.0, 88.0, 90.0, id(su, ap::A_SHAPE));
            }
            for (j, (f, l)) in [(ap::A_VOL, "Volume"), (ap::A_PVAR, "PVar"), (ap::A_SYM, "Sym")].iter().enumerate() {
                k(p, st, 420.0 + j as f32 * 56.0, 82.0, 32.0, id(su, *f), l);
            }
            k(p, st, 600.0, 82.0, 32.0, id(su, ap::A_NUM), "");
            p.text(600.0, 128.0, Align2::CENTER_CENTER, &format!("{} Partials", g(p, ix, id(su, ap::A_NUM)).round()), 10.0, p.skin.dim);
            for u in 0..3usize {
                let b = [ap::U1, ap::U2, ap::U3][u];
                let types = [ap::U1_TYPES, ap::U2_TYPES, ap::U3_TYPES][u];
                unit_strip(p, ix, st, 84.0 + u as f32 * 236.0, 150.0, su, b, types, u);
            }
        }
        2 => {
            let nm = g(p, ix, id(su, ap::SP_MODE)) >= 0.5;
            for (j, l) in ["Pitch", "Noise"].iter().enumerate() {
                if rack::flat(p, p.rect(124.0 + j as f32 * 50.0, 88.0, 48.0, 20.0), l, nm == (j == 1), true, ("sp-mode", s, j)) {
                    p.put(id(su, ap::SP_MODE), j as f32);
                }
            }
            for (j, (f, l)) in [(ap::SP_VOL, "Volume"), (ap::SP_LO, "Low Cut"), (ap::SP_HI, "High Cut")].iter().enumerate() {
                k(p, st, 300.0 + j as f32 * 60.0, 82.0, 32.0, id(su, *f), l);
            }
            for u in 0..2usize {
                spec_strip(p, ix, st, 84.0 + u as f32 * 350.0, 150.0, su, [ap::SFX1, ap::SFX2][u], u);
            }
        }
        3 => {
            use lg_synth::alchemy_gran::{self as gr, id as gid};
            let sync = g(p, ix, gid(su, gr::SYNC)) >= 0.5;
            if rack::flat(p, p.rect(124.0, 88.0, 50.0, 20.0), "Sync", sync, true, ("gran-sync", s)) {
                p.put(gid(su, gr::SYNC), if sync { 0.0 } else { 1.0 });
            }
            rack::menu_arrows(p, ix, 190.0, 88.0, 100.0, gid(su, gr::SHAPE));
            p.text(190.0, 114.0, Align2::LEFT_CENTER, "Grain Shape", 10.0, p.skin.dim);
            // Sync: Formant Shift in place of Size and Density
            let mut row: Vec<(u32, &str)> = vec![(35 + s, "Position"), (39 + s, "Speed")];
            if sync {
                row.push((gid(su, gr::FORMANT), "Formant"));
            } else {
                row.extend([(gid(su, gr::SIZE), "Size"), (gid(su, gr::DENSITY), "Density")]);
            }
            row.extend([(gid(su, gr::RTIME), "RTime"), (gid(su, gr::RPAN), "RPan"), (gid(su, gr::VOL), "Volume")]);
            for (j, (pid, l)) in row.iter().enumerate() {
                k(p, st, 110.0 + j as f32 * 62.0, 124.0, 36.0, *pid, l);
            }
            let taps = [(gid(su, gr::TAPS), "Num Taps"), (gid(su, gr::INTERVAL), "Tap Interval"), (gid(su, gr::DELAY), "Tap Delay"), (gid(su, gr::SPACING), "Tap Spacing"), (gid(su, gr::STOFFSET), "StOffset")];
            for (j, (pid, l)) in taps.iter().enumerate() {
                k(p, st, 110.0 + j as f32 * 70.0, 196.0, 36.0, *pid, l);
            }
            // Num Taps in whole taps; Tap Interval in semitones unless Shift is held
            let nt = p.get(gid(su, gr::TAPS));
            if nt.fract() != 0.0 {
                p.put(gid(su, gr::TAPS), nt.round());
            }
            let iv = p.get(gid(su, gr::INTERVAL));
            if iv.fract() != 0.0 && !p.ui.input(|i| i.modifiers.shift) && p.set.iter().any(|x| x.0 == gid(su, gr::INTERVAL)) {
                p.put(gid(su, gr::INTERVAL), iv.round());
            }
        }
        5 => sampler_view(p, ix, st, su, zdoc, zpos),
        _ => {
            for (j, (f, l)) in [(ap::FM_SHIFT, "Shift"), (ap::FM_SIZE, "Size"), (ap::FM_CENTER, "Center"), (ap::FM_SELECT, "Select")].iter().enumerate() {
                k(p, st, 150.0 + j as f32 * 64.0, 88.0, 38.0, id(su, *f), l);
            }
            for u in 0..4u32 {
                rack::menu_arrows(p, ix, 84.0 + u as f32 * 176.0, 170.0, 140.0, id(su, ap::FM_TYPE + u));
                p.text(84.0 + u as f32 * 176.0, 200.0, Align2::LEFT_CENTER, &format!("Unit {}", u + 1), 10.0, p.skin.dim);
            }
        }
    }
}

/// An additive effect unit: On, its type (choosing one sets its knobs), four knobs.
fn unit_strip(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, s: usize, b: u32, types: &[&str], u: usize) {
    use lg_synth::alchemy_partials::{self as ap, id};
    p.ui.painter().rect_filled(p.rect(x, y, 230.0, 126.0), 4.0, rgb(0x20242a));
    let on = g(p, ix, id(s, b)) >= 0.5;
    if rack::flat(p, p.rect(x + 6.0, y + 6.0, 28.0, 20.0), "On", on, true, ("unit-on", s, u)) {
        p.put(id(s, b), if on { 0.0 } else { 1.0 });
    }
    let ty = g(p, ix, id(s, b + 1)).round().max(0.0) as usize;
    let resp = rack::field_btn(p, p.rect(x + 40.0, y + 6.0, 150.0, 20.0), types.get(ty).copied().unwrap_or(""), ("unit-ty", s, u), true);
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        for (k, n) in types.iter().enumerate() {
            if ui.add(egui::Button::selectable(k == ty, *n)).clicked() {
                pick = Some(k);
            }
        }
    });
    if let Some(k) = pick {
        p.put(id(s, b + 1), k as f32);
        for (j, v) in ap::unit_defaults(u, k).iter().enumerate() {
            p.put(id(s, b + 2 + j as u32), *v);
        }
    }
    for (j, n) in ap::unit_knobs(u, ty).iter().enumerate() {
        if !n.is_empty() {
            // the label drawn here: long names don't fit the knob's own slot
            let kx = x + 30.0 + j as f32 * 56.0;
            rack::knob(p, ix, st, kx, y + 40.0, 32.0, id(s, b + 2 + j as u32), "");
            p.text(kx, y + 88.0, Align2::CENTER_CENTER, n, 10.0, p.skin.dim);
        }
    }
}

/// A spectral effect unit: On, type, Mix and three knobs named by the type.
fn spec_strip(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, s: usize, b: u32, u: usize) {
    use lg_synth::alchemy_partials::{self as ap, id};
    p.ui.painter().rect_filled(p.rect(x, y, 344.0, 126.0), 4.0, rgb(0x20242a));
    let on = g(p, ix, id(s, b)) >= 0.5;
    if rack::flat(p, p.rect(x + 6.0, y + 6.0, 28.0, 20.0), "On", on, true, ("sfx-on", s, u)) {
        p.put(id(s, b), if on { 0.0 } else { 1.0 });
    }
    rack::menu_arrows(p, ix, x + 40.0, y + 6.0, 130.0, id(s, b + 1));
    let ty = g(p, ix, id(s, b + 1)).round().max(0.0) as usize;
    rack::knob(p, ix, st, x + 34.0, y + 40.0, 32.0, id(s, b + 2), "Mix");
    for (j, n) in ap::spec_knobs(ty).iter().enumerate() {
        if !n.is_empty() {
            rack::knob(p, ix, st, x + 110.0 + j as f32 * 70.0, y + 40.0, 32.0, id(s, b + 3 + j as u32), n);
        }
    }
}

/// Studio's arpeggiator patterns (File > Preset): (name, steps, per step: velocity, tune, tie).
fn arp_presets() -> Vec<(&'static str, usize, Vec<(f32, f32, f32)>)> {
    vec![
        ("Straight 16", 16, vec![(1.0, 0.0, 0.0); 16]),
        ("Accents", 16, (0..16).map(|k| (if k % 4 == 0 { 1.0 } else { 0.6 }, 0.0, 0.0)).collect()),
        ("Offbeat", 8, (0..8).map(|k| (if k % 2 == 1 { 1.0 } else { 0.0 }, 0.0, 0.0)).collect()),
        ("Gallop", 12, (0..12).map(|k| (if k % 3 == 1 { 0.0 } else { 0.9 }, 0.0, 0.0)).collect()),
        ("Octave Bounce", 8, (0..8).map(|k| (0.9, if k % 2 == 1 { 12.0 } else { 0.0 }, 0.0)).collect()),
        ("Long Short", 8, (0..8).map(|k| (0.9, 0.0, if k % 4 == 0 { 1.0 } else { 0.0 })).collect()),
    ]
}

/// The Arpeggiator pane: All / A–D, Sync, Key Trigger, Cycle Reset, the nine knobs, File, and
/// the sequencer (Step mode: one sequence as bars with Target and Value Snap; Multi mode: On,
/// Tie, Tune and Pan rows), its end marker and scroll bar.
fn arp_pane(p: &mut Pane, ix: &Ix, st: &mut St, x0: f32, y0: f32) {
    use lg_synth::alchemy_arp as aa;
    rack::flat(p, p.rect(x0, y0 + 6.0, 30.0, 20.0), "All", true, true, "arp-all");
    for (k, l) in ["A", "B", "C", "D"].iter().enumerate() {
        rack::flat(p, p.rect(x0 + 34.0 + k as f32 * 22.0, y0 + 6.0, 20.0, 20.0), l, false, false, ("arp-src", k));
    }
    let sync = g(p, ix, aa::SYNC) >= 0.5;
    if rack::flat(p, p.rect(x0 + 130.0, y0 + 6.0, 44.0, 20.0), "Sync", sync, true, "arp-sync") {
        p.put(aa::SYNC, if sync { 0.0 } else { 1.0 });
    }
    p.text(x0 + 182.0, y0 + 16.0, Align2::LEFT_CENTER, "Key Trigger", 10.5, p.skin.dim);
    p.menu(x0 + 248.0, y0 + 6.0, 90.0, 20.0, aa::KEY_TRIG);
    let cr = g(p, ix, aa::CYCLE_RESET) >= 0.5;
    if rack::flat(p, p.rect(x0 + 344.0, y0 + 6.0, 80.0, 20.0), "Cycle Reset", cr, true, "arp-cr") {
        p.put(aa::CYCLE_RESET, if cr { 0.0 } else { 1.0 });
    }
    // the nine knobs
    let knobs: [(u32, &str); 9] = [(aa::MODE, "Mode"), (if sync { aa::RATE } else { aa::HZ }, "Rate"), (aa::OCTAVE, "Octave"), (aa::LENGTH, "Length"), (aa::PATTERN, "Pattern"), (aa::SWING, "Swing"), (aa::NOTE_VEL, "Note Vel"), (aa::SPLIT, "Split"), (aa::LATCH, "Latch")];
    for (k, (id, l)) in knobs.iter().enumerate() {
        rack::knob(p, ix, st, x0 + 470.0 + k as f32 * 64.0, y0 + 2.0, 30.0, *id, l);
    }
    let pi = (g(p, ix, aa::PATTERN).round().clamp(1.0, 16.0) as usize) - 1;
    // edit mode, target, snap, File
    let y = y0 + 54.0;
    p.seg(x0, y, 100.0, 20.0, aa::EDIT, aa::EDIT_MODES, false);
    let multi = g(p, ix, aa::EDIT) >= 0.5;
    if !multi {
        p.text(x0 + 110.0, y + 10.0, Align2::LEFT_CENTER, "Target", 10.5, p.skin.dim);
        p.menu(x0 + 150.0, y, 90.0, 20.0, aa::TARGET);
        p.text(x0 + 250.0, y + 10.0, Align2::LEFT_CENTER, "Value Snap", 10.5, p.skin.dim);
        rack::menu_arrows(p, ix, x0 + 312.0, y, 50.0, aa::SNAP);
    }
    arp_file(p, ix, rect_at(p, x0 + 400.0, y, 48.0, 20.0), pi);
    p.text(x0 + 460.0, y + 10.0, Align2::LEFT_CENTER, &format!("Pattern {}", pi + 1), 10.5, p.skin.dim);
    // the steps
    let steps = (g(p, ix, aa::pat(pi, 0)).round().max(1.0) as usize).min(aa::STEPS);
    let cols = 32usize;
    let total = (steps + 4).min(aa::STEPS).max(cols);
    st.arp_scroll = st.arp_scroll.min(total.saturating_sub(cols));
    let first = st.arp_scroll;
    let area = p.rect(x0, y0 + 80.0, W - 100.0, 112.0);
    let cw = area.width() / cols as f32;
    // the end marker (drag: steps)
    let mk = Rect::from_min_size(area.min, vec2(area.width(), 8.0));
    p.ui.painter().rect_filled(mk, 2.0, rgb(0x15181d));
    let ex = (mk.min.x + (steps as f32 - first as f32) * cw).clamp(mk.min.x, mk.max.x);
    p.ui.painter().rect_filled(Rect::from_min_max(mk.min, pos2(ex, mk.max.y)), 2.0, rgb(0x6a6f78));
    let resp = p.ui.interact(mk, egui::Id::unique(("arp-end", p.t)), Sense::click_and_drag());
    if (resp.dragged() || resp.clicked())
        && let Some(q) = resp.interact_pointer_pos()
    {
        p.put(aa::pat(pi, 0), (first as f32 + (q.x - mk.min.x) / cw).round().clamp(1.0, aa::STEPS as f32));
    }
    let body = Rect::from_min_max(pos2(area.min.x, area.min.y + 12.0), pos2(area.max.x, area.max.y - 22.0));
    p.ui.painter().rect_filled(body, 0.0, rgb(0x0f1216));
    let alt = p.ui.input(|i| i.modifiers.alt);
    if !multi {
        let seq = g(p, ix, aa::TARGET).round().max(0.0) as usize;
        let (lo, hi, _) = aa::seq_range(seq);
        let snap = aa::SNAP_N[(g(p, ix, aa::SNAP).round().max(0.0) as usize).min(aa::SNAP_N.len() - 1)];
        let resp = p.ui.interact(body, egui::Id::unique(("arp-bars", p.t)), Sense::click_and_drag());
        if let Some(q) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked())
        {
            let k = first + ((q.x - body.min.x) / cw).floor().max(0.0) as usize;
            if k < aa::STEPS {
                let mut f = ((body.max.y - q.y) / body.height()).clamp(0.0, 1.0);
                if snap > 0 {
                    f = (f * snap as f32).round() / snap as f32;
                }
                // Option-click: zero (a rest for Velocity)
                let v = if alt && resp.clicked() { if lo < 0.0 { 0.0 } else { lo } } else { lo + f * (hi - lo) };
                p.put(aa::step(pi, seq, k), if seq == aa::S_TUNE { v.round() } else { v });
            }
        }
        let pc = p.ui.painter().with_clip_rect(body);
        for c in 0..cols {
            let k = first + c;
            if k >= aa::STEPS {
                break;
            }
            let cx = body.min.x + c as f32 * cw;
            let v = g(p, ix, aa::step(pi, seq, k));
            let col = if k < steps { ORANGE } else { ORANGE.gamma_multiply(0.3) };
            let f = (v - lo) / (hi - lo);
            let zero = if lo < 0.0 { 0.5 } else { 0.0 };
            let (ya, yb) = (body.max.y - f * body.height(), body.max.y - zero * body.height());
            if (ya - yb).abs() > 0.5 {
                pc.rect_filled(Rect::from_min_max(pos2(cx + 2.0, ya.min(yb)), pos2(cx + cw - 2.0, ya.max(yb))), 1.0, col);
            }
        }
    } else {
        // Multi: On / Tie buttons, Tune fields, Pan knobs per step
        // rows: On, Tie, Tune (small), Pan (the rest)
        let rh = 15.0;
        for c in 0..cols {
            let k = first + c;
            if k >= aa::STEPS {
                break;
            }
            let cx = body.min.x + c as f32 * cw;
            let vel = g(p, ix, aa::step(pi, aa::S_VEL, k));
            let r0 = Rect::from_min_size(pos2(cx + 2.0, body.min.y + 2.0), vec2(cw - 4.0, rh - 4.0));
            let on = vel > 0.0;
            let resp = p.ui.interact(r0, egui::Id::unique(("arp-m-on", p.t, k)), Sense::click());
            p.ui.painter().rect_filled(r0, 2.0, if on { ORANGE } else { rgb(0x2a2e35) });
            if resp.clicked() {
                // back on: full velocity
                p.put(aa::step(pi, aa::S_VEL, k), if on { 0.0 } else { 1.0 });
            }
            let r1 = r0.translate(vec2(0.0, rh));
            let tie = g(p, ix, aa::step(pi, aa::S_TIE, k)) >= 0.5;
            let resp = p.ui.interact(r1, egui::Id::unique(("arp-m-tie", p.t, k)), Sense::click());
            p.ui.painter().rect_filled(r1, 2.0, if tie { BLUE } else { rgb(0x2a2e35) });
            p.ui.painter().text(r1.center(), Align2::CENTER_CENTER, "⁀", look::regular(10.0), p.skin.text);
            if resp.clicked() {
                p.put(aa::step(pi, aa::S_TIE, k), if tie { 0.0 } else { 1.0 });
            }
            let r2 = r1.translate(vec2(0.0, rh));
            let tune = g(p, ix, aa::step(pi, aa::S_TUNE, k));
            let resp = p.ui.interact(r2, egui::Id::unique(("arp-m-tune", p.t, k)), Sense::click_and_drag());
            p.ui.painter().rect_filled(r2, 2.0, rgb(0x1a1d22));
            p.ui.painter().text(r2.center(), Align2::CENTER_CENTER, format!("{tune:+.0}"), look::regular(9.5), p.skin.text);
            if resp.dragged() {
                let acc_id = egui::Id::unique(("arp-m-tune-acc", p.t, k));
                let mut acc: f32 = p.ui.data(|d| d.get_temp(acc_id)).unwrap_or(tune);
                acc = (acc - resp.drag_delta().y / 6.0).clamp(-24.0, 24.0);
                p.ui.data_mut(|d| d.insert_temp(acc_id, acc));
                p.put(aa::step(pi, aa::S_TUNE, k), acc.round());
            }
            if resp.drag_stopped() {
                p.ui.data_mut(|d| d.remove::<f32>(egui::Id::unique(("arp-m-tune-acc", p.t, k))));
            }
            if resp.double_clicked() {
                p.put(aa::step(pi, aa::S_TUNE, k), 0.0);
            }
            let pr = Rect::from_min_max(pos2(cx + 2.0, r2.max.y + 3.0), pos2(cx + cw - 2.0, body.max.y - 1.0));
            let mut pan = g(p, ix, aa::step(pi, aa::S_PAN, k));
            let d = pr.height().min(pr.width());
            let kr = Rect::from_min_size(pos2(pr.center().x - d / 2.0 - 2.0, pr.min.y - 2.0), vec2(d + 4.0, d + 18.0));
            if crate::widgets::knob(p.ui, kr, egui::Id::unique(("arp-m-pan", p.t, k)), &mut pan, -1.0, 1.0, 0.0, "", &|v| format!("{:.0}", v * 100.0)) {
                p.put(aa::step(pi, aa::S_PAN, k), pan);
            }
        }
    }
    // ties under the steps (Step mode) and the step numbers
    for c in 0..cols {
        let k = first + c;
        if k >= aa::STEPS {
            break;
        }
        let cx = body.min.x + c as f32 * cw;
        let tr = Rect::from_min_size(pos2(cx + 2.0, body.max.y + 2.0), vec2(cw - 4.0, 8.0));
        if !multi {
            let tie = g(p, ix, aa::step(pi, aa::S_TIE, k)) >= 0.5;
            p.ui.painter().rect_filled(tr, 2.0, if tie { BLUE } else { rgb(0x2a2e35) });
            if p.ui.interact(tr, egui::Id::unique(("arp-tie", p.t, k)), Sense::click()).on_hover_text("Tie to the next step").clicked() {
                p.put(aa::step(pi, aa::S_TIE, k), if tie { 0.0 } else { 1.0 });
            }
        }
        p.text(cx + cw / 2.0 - p.o.x, body.max.y + 16.0 - p.o.y, Align2::CENTER_CENTER, &(k + 1).to_string(), 8.5, p.skin.dim);
    }
    if total > cols {
        let sb = Rect::from_min_size(pos2(area.min.x, area.max.y - 4.0), vec2(area.width(), 4.0));
        p.ui.painter().rect_filled(sb, 2.0, rgb(0x15181d));
        let th = Rect::from_min_size(pos2(sb.min.x + first as f32 / total as f32 * sb.width(), sb.min.y), vec2(cols as f32 / total as f32 * sb.width(), 4.0));
        p.ui.painter().rect_filled(th, 2.0, rgb(0x6a6f78));
        let resp = p.ui.interact(sb.expand2(vec2(0.0, 4.0)), egui::Id::unique(("arp-sb", p.t)), Sense::click_and_drag());
        if let Some(q) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked())
        {
            st.arp_scroll = (((q.x - sb.min.x) / sb.width() * total as f32) as usize).saturating_sub(cols / 2).min(total - cols);
        }
    }
}

fn rect_at(p: &Pane, x: f32, y: f32, w: f32, h: f32) -> Rect {
    p.rect(x, y, w, h)
}

/// The arpeggiator's File menu for the shown pattern.
fn arp_file(p: &mut Pane, ix: &Ix, r: Rect, pi: usize) {
    use lg_synth::alchemy_arp as aa;
    let resp = p.ui.interact(r, egui::Id::unique(("arp-file", p.t)), Sense::click());
    p.ui.painter().rect_filled(r, 3.0, if resp.hovered() { rgb(0x4a4f58) } else { rack::BTN });
    p.ui.painter().text(r.center(), Align2::CENTER_CENTER, "File", look::regular(11.0), p.skin.text);
    let fields = |p: &Pane| -> Vec<(u32, f32)> {
        let mut v = vec![(0u32, g(p, ix, aa::pat(pi, 0)))];
        for s in 0..aa::SEQS.len() {
            for k in 0..aa::STEPS {
                v.push((aa::SEQ0 + (s * aa::STEPS + k) as u32, g(p, ix, aa::step(pi, s, k))));
            }
        }
        v
    };
    let mut act = 0u8;
    let mut load: Option<Vec<(u32, f32)>> = None;
    let files: Vec<(String, std::path::PathBuf)> = rack::mod_dir("arp").and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().map(|e| e.path()).filter(|x| x.extension().is_some_and(|e| e == "arp")).map(|x| (x.file_stem().unwrap_or_default().to_string_lossy().into_owned(), x)).collect();
    let name_id = egui::Id::unique(("arp-save-name", p.t));
    let mut name: String = p.ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.menu_button("Preset", |ui| {
            for (n, steps, list) in arp_presets() {
                if ui.button(n).clicked() {
                    let mut v = vec![(0u32, steps as f32)];
                    for (k, (vel, tune, tie)) in list.iter().enumerate() {
                        v.push((aa::SEQ0 + (aa::S_VEL * aa::STEPS + k) as u32, *vel));
                        v.push((aa::SEQ0 + (aa::S_TUNE * aa::STEPS + k) as u32, *tune));
                        v.push((aa::SEQ0 + (aa::S_TIE * aa::STEPS + k) as u32, *tie));
                    }
                    load = Some(v);
                    ui.close();
                }
            }
            if !files.is_empty() {
                ui.separator();
            }
            for (n, path) in &files {
                if ui.button(n).clicked() {
                    load = Some(rack::read_fields(path));
                    ui.close();
                }
            }
        });
        ui.menu_button("Save…", |ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Name").desired_width(150.0));
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || enter) && !name.trim().is_empty() {
                act = 1;
                ui.close();
            }
        });
        ui.separator();
        for (k, n) in ["Copy", "Paste", "Clear", "Randomize"].iter().enumerate() {
            if ui.button(*n).clicked() {
                act = 2 + k as u8;
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Import MIDI…").clicked() {
            p.actions.push(format!("arp-import:{pi}"));
            ui.close();
        }
    });
    let clip = egui::Id::unique("alchemy-arp-clipboard");
    match act {
        1 => {
            if let Some(dir) = rack::mod_dir("arp") {
                let _ = std::fs::create_dir_all(&dir);
                let body: String = fields(p).iter().map(|(f, v)| format!("{f} {v}\n")).collect();
                let _ = std::fs::write(dir.join(format!("{}.arp", name.trim().replace('/', "-"))), body);
            }
            name.clear();
        }
        2 => {
            let v = fields(p);
            p.ui.data_mut(|d| d.insert_temp(clip, v));
        }
        3 => load = p.ui.data(|d| d.get_temp::<Vec<(u32, f32)>>(clip)),
        4 => {
            let mut v = vec![(0u32, 16.0)];
            for s in 0..aa::SEQS.len() {
                let d = aa::seq_range(s).2;
                for k in 0..aa::STEPS {
                    v.push((aa::SEQ0 + (s * aa::STEPS + k) as u32, d));
                }
            }
            load = Some(v);
        }
        5 => {
            // random values for the sequence shown
            let seq = g(p, ix, aa::TARGET).round().max(0.0) as usize;
            let (lo, hi, _) = aa::seq_range(seq);
            let seed = (p.ui.input(|i| i.time) * 1000.0) as u32;
            let steps = g(p, ix, aa::pat(pi, 0)).round() as usize;
            let mut x = seed | 1;
            let mut v = Vec::new();
            for k in 0..steps.min(aa::STEPS) {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let f = (x & 0xFFFF) as f32 / 65535.0;
                let val = lo + f * (hi - lo);
                v.push((aa::SEQ0 + (seq * aa::STEPS + k) as u32, if seq == aa::S_TUNE || seq == aa::S_TIE { val.round() } else { val }));
            }
            load = Some(v);
        }
        _ => {}
    }
    p.ui.data_mut(|d| d.insert_temp(name_id, name));
    if let Some(v) = load {
        for (f, val) in v {
            p.put(aa::pat(pi, f), val);
        }
    }
}

/// The Effects pane: the rack buttons (Main, A–D) with the rack's File menu, the rack's slots
/// (On, the effect pop-up, drag a name to move it), and each effect's controls left to right with
/// its own File menu.
fn effects_pane(p: &mut Pane, ix: &Ix, st: &mut St, x0: f32, y0: f32) {
    use lg_synth::alchemy_fx as fx;
    let rk = st.fx_rack.min(fx::RACKS - 1);
    for (k, n) in fx::RACK_NAMES.iter().enumerate() {
        let w = if k == 0 { 42.0 } else { 26.0 };
        let x = if k == 0 { x0 } else { x0 + 46.0 + (k - 1) as f32 * 28.0 };
        if rack::flat(p, p.rect(x, y0 + 6.0, w, 20.0), n, rk == k, true, ("fx-rack", k)) {
            st.fx_rack = k;
            st.fx_scroll = 0.0;
        }
    }
    rack_file(p, ix, p.rect(x0 + 162.0, y0 + 6.0, 48.0, 20.0), rk);
    // the slots: the effects in order, then one empty slot
    let types: Vec<usize> = (0..fx::SLOTS).map(|s| g(p, ix, fx::id(rk, s, 0)).round().max(0.0) as usize).collect();
    let last = types.iter().rposition(|&t| t != 0).map(|i| i + 1).unwrap_or(0);
    let shown = (last + 1).min(fx::SLOTS);
    let mut drop_on: Option<usize> = None;
    let mut rows = Vec::new();
    for s in 0..shown {
        let y = y0 + 32.0 + s as f32 * 21.0;
        let on = g(p, ix, fx::id(rk, s, 1)) >= 0.5;
        if rack::flat(p, p.rect(x0, y, 28.0, 19.0), "On", on && types[s] != 0, types[s] != 0, ("fx-on", rk, s)) {
            p.put(fx::id(rk, s, 1), if on { 0.0 } else { 1.0 });
        }
        let r = p.rect(x0 + 32.0, y, 178.0, 19.0);
        rows.push(r);
        // one control: click for the effect menu, drag the name to move the effect
        let resp = p.ui.interact(r, egui::Id::unique(("fx-ty", p.t, rk, s)), Sense::click_and_drag());
        p.ui.painter().rect_filled(r, 3.0, if resp.hovered() { rgb(0x23272e) } else { rack::FIELD });
        p.ui.painter().with_clip_rect(r).text(pos2(r.center().x - 6.0, r.center().y), Align2::CENTER_CENTER, if types[s] == 0 { "" } else { fx::TYPES[types[s]] }, look::regular(11.0), p.skin.text);
        crate::glyphs::paint(p.ui.painter(), Rect::from_center_size(pos2(r.max.x - 9.0, r.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, p.skin.dim);
        let dresp = resp.clone();
        if dresp.drag_started() && types[s] != 0 {
            st.fx_drag = Some(s);
        }
        if dresp.drag_stopped() {
            if let Some(q) = p.ui.input(|i| i.pointer.interact_pos()) {
                drop_on = Some((((q.y - p.o.y - y0 - 32.0) / 21.0).floor().max(0.0) as usize).min(shown.saturating_sub(1)));
            }
        }
        let mut pick = None;
        egui::Popup::menu(&resp).show(|ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (k, n) in fx::TYPES.iter().enumerate() {
                    // Band Reject needs a Bandpass Filter before it
                    let ok = k != 12 || types[..s].contains(&11);
                    if ui.add_enabled(ok, egui::Button::selectable(k == types[s], *n)).clicked() {
                        pick = Some(k);
                    }
                }
            });
        });
        if let Some(k) = pick {
            // a new effect replaces the slot's one, with its own defaults; None keeps the slot
            p.put(fx::id(rk, s, 0), k as f32);
            p.put(fx::id(rk, s, 1), 1.0);
            for (i, sp) in fx::knobs(k).iter().enumerate() {
                p.put(fx::id(rk, s, 2 + i as u32), fx::unscale(sp, sp.def));
            }
        }
    }
    if let (Some(from), Some(to)) = (st.fx_drag, drop_on) {
        if from != to {
            // moving an effect shifts the others to keep their order
            let slot = |s: usize| -> Vec<f32> { (0..24u32).map(|f| g(p, ix, fx::id(rk, s, f))).collect() };
            let mut all: Vec<Vec<f32>> = (0..fx::SLOTS).map(slot).collect();
            let item = all.remove(from);
            all.insert(to, item);
            for (s, v) in all.iter().enumerate() {
                for (f, x) in v.iter().enumerate() {
                    p.put(fx::id(rk, s, f as u32), *x);
                }
            }
        }
        st.fx_drag = None;
    } else if drop_on.is_some() {
        st.fx_drag = None;
    }
    // the effects' controls, left to right
    let area = p.rect(x0 + 222.0, y0 + 4.0, W - x0 - 230.0, 192.0);
    if p.ui.rect_contains_pointer(area) {
        let d = p.ui.input(|i| i.smooth_scroll_delta);
        st.fx_scroll = (st.fx_scroll - d.x - d.y).max(0.0);
    }
    let mut x = area.min.x - p.o.x - st.fx_scroll;
    let clip = p.ui.clip_rect();
    p.ui.set_clip_rect(area.intersect(clip));
    for s in 0..fx::SLOTS {
        let ty = types[s];
        if ty == 0 {
            continue;
        }
        let ks = fx::knobs(ty);
        let cols = ks.len().div_ceil(3).max(3);
        let w = cols as f32 * 52.0 + 12.0;
        let y = area.min.y - p.o.y;
        p.ui.painter().rect_filled(p.rect(x, y, w, 188.0), 4.0, rgb(0x1c2026));
        p.text(x + 8.0, y + 12.0, Align2::LEFT_CENTER, fx::TYPES[ty], 11.0, p.skin.text);
        effect_file(p, ix, p.rect(x + w - 46.0, y + 3.0, 40.0, 18.0), rk, s, ty);
        for (i, sp) in ks.iter().enumerate() {
            let (cx, cy) = (x + 32.0 + (i / 3) as f32 * 52.0, y + 26.0 + (i % 3) as f32 * 54.0);
            let pid = fx::id(rk, s, 2 + i as u32);
            match sp.kind {
                2 => {
                    let on = g(p, ix, pid) >= 0.5;
                    if rack::flat(p, p.rect(cx - 22.0, cy + 8.0, 44.0, 18.0), sp.name, on, true, ("fx-sw", rk, s, i)) {
                        p.put(pid, if on { 0.0 } else { 1.0 });
                    }
                }
                3 => {
                    let cur = fx::scale(sp, g(p, ix, pid)) as usize;
                    let resp = rack::field_btn(p, p.rect(cx - 24.0, cy + 8.0, 48.0, 18.0), sp.choices.get(cur).copied().unwrap_or(""), ("fx-menu", rk, s, i), false);
                    p.text(cx, cy + 36.0, Align2::CENTER_CENTER, sp.name, 9.5, p.skin.dim);
                    let mut pick = None;
                    egui::Popup::menu(&resp).show(|ui| {
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (k, n) in sp.choices.iter().enumerate() {
                                if ui.add(egui::Button::selectable(k == cur, *n)).clicked() {
                                    pick = Some(k);
                                }
                            }
                        });
                    });
                    if let Some(k) = pick {
                        p.put(pid, fx::unscale(sp, k as f32));
                    }
                }
                _ => {
                    p.knob(cx, cy, 26.0, pid, "");
                    p.text(cx, cy + 36.0, Align2::CENTER_CENTER, sp.name, 9.5, p.skin.dim);
                }
            }
        }
        x += w + 6.0;
    }
    p.ui.set_clip_rect(clip);
}

fn fx_dir(ext: &str) -> Option<std::path::PathBuf> {
    rack::mod_dir(ext)
}

/// An effect's File menu: Presets (Studio's and saved), Save…, Copy, Paste, Clear.
fn effect_file(p: &mut Pane, ix: &Ix, r: Rect, rk: usize, s: usize, ty: usize) {
    use lg_synth::alchemy_fx as fx;
    let resp = p.ui.interact(r, egui::Id::unique(("fx-file", p.t, rk, s)), Sense::click());
    p.ui.painter().rect_filled(r, 3.0, if resp.hovered() { rgb(0x4a4f58) } else { rack::BTN });
    p.ui.painter().text(r.center(), Align2::CENTER_CENTER, "File", look::regular(10.0), p.skin.text);
    let ks = fx::knobs(ty);
    let cur: Vec<f32> = (0..ks.len()).map(|i| g(p, ix, fx::id(rk, s, 2 + i as u32))).collect();
    let files: Vec<(String, std::path::PathBuf)> = fx_dir("afx").and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().map(|e| e.path()).filter(|x| x.extension().is_some_and(|e| e == "afx") && x.file_stem().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{} - ", fx::TYPES[ty])))).map(|x| (x.file_stem().unwrap_or_default().to_string_lossy().into_owned(), x)).collect();
    let name_id = egui::Id::unique(("fx-save-name", p.t));
    let mut name: String = p.ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
    let mut load: Option<Vec<f32>> = None;
    let mut act = 0u8;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.menu_button("Presets", |ui| {
            for (n, list) in fx::presets(ty) {
                if ui.button(n).clicked() {
                    let mut v: Vec<f32> = ks.iter().map(|sp| fx::unscale(sp, sp.def)).collect();
                    for (i, real) in list {
                        if let Some(sp) = ks.get(i) {
                            v[i] = fx::unscale(sp, real);
                        }
                    }
                    load = Some(v);
                    ui.close();
                }
            }
            if !files.is_empty() {
                ui.separator();
            }
            for (n, path) in &files {
                if ui.button(n.split(" - ").nth(1).unwrap_or(n)).clicked() {
                    load = Some(rack::read_fields(path).into_iter().map(|(_, v)| v).collect());
                    ui.close();
                }
            }
        });
        ui.menu_button("Save…", |ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Name").desired_width(150.0));
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || enter) && !name.trim().is_empty() {
                act = 1;
                ui.close();
            }
        });
        for (k, n) in ["Copy", "Paste", "Clear"].iter().enumerate() {
            if ui.button(*n).clicked() {
                act = 2 + k as u8;
                ui.close();
            }
        }
    });
    let clip = egui::Id::unique("alchemy-fx-clipboard");
    match act {
        1 => {
            if let Some(dir) = fx_dir("afx") {
                let _ = std::fs::create_dir_all(&dir);
                let body: String = cur.iter().enumerate().map(|(i, v)| format!("{i} {v}\n")).collect();
                let _ = std::fs::write(dir.join(format!("{} - {}.afx", fx::TYPES[ty], name.trim().replace('/', "-"))), body);
            }
            name.clear();
        }
        2 => {
            p.ui.data_mut(|d| d.insert_temp(clip, (ty, cur.clone())));
        }
        3 => {
            if let Some((t2, v)) = p.ui.data(|d| d.get_temp::<(usize, Vec<f32>)>(clip))
                && t2 == ty
            {
                load = Some(v);
            }
        }
        4 => load = Some(ks.iter().map(|sp| fx::unscale(sp, sp.def)).collect()),
        _ => {}
    }
    p.ui.data_mut(|d| d.insert_temp(name_id, name));
    if let Some(v) = load {
        for (i, x) in v.iter().enumerate().take(fx::KNOBS) {
            p.put(fx::id(rk, s, 2 + i as u32), *x);
        }
    }
}

/// A rack's File menu: Load / Save (.afr), Copy, Paste, Clear (removes its effects), Randomize.
fn rack_file(p: &mut Pane, ix: &Ix, r: Rect, rk: usize) {
    use lg_synth::alchemy_fx as fx;
    let resp = p.ui.interact(r, egui::Id::unique(("fx-rackfile", p.t)), Sense::click());
    p.ui.painter().rect_filled(r, 3.0, if resp.hovered() { rgb(0x4a4f58) } else { rack::BTN });
    p.ui.painter().text(r.center(), Align2::CENTER_CENTER, "File", look::regular(11.0), p.skin.text);
    let all: Vec<(u32, f32)> = (0..fx::SLOTS).flat_map(|s| (0..24u32).map(move |f| (s as u32 * 24 + f, 0.0))).map(|(k, _)| (k, g(p, ix, fx::id(rk, 0, 0) + k))).collect();
    let files: Vec<(String, std::path::PathBuf)> = fx_dir("afr").and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().map(|e| e.path()).filter(|x| x.extension().is_some_and(|e| e == "afr")).map(|x| (x.file_stem().unwrap_or_default().to_string_lossy().into_owned(), x)).collect();
    let name_id = egui::Id::unique(("fx-rack-save-name", p.t));
    let mut name: String = p.ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
    let mut load: Option<Vec<(u32, f32)>> = None;
    let mut act = 0u8;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.menu_button("Load", |ui| {
            if files.is_empty() {
                ui.label("No saved racks");
            }
            for (n, path) in &files {
                if ui.button(n).clicked() {
                    load = Some(rack::read_fields(path));
                    ui.close();
                }
            }
        });
        ui.menu_button("Save…", |ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Name").desired_width(150.0));
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || enter) && !name.trim().is_empty() {
                act = 1;
                ui.close();
            }
        });
        for (k, n) in ["Copy", "Paste", "Clear", "Randomize"].iter().enumerate() {
            if ui.button(*n).clicked() {
                act = 2 + k as u8;
                ui.close();
            }
        }
    });
    let clip = egui::Id::unique("alchemy-rack-clipboard");
    match act {
        1 => {
            if let Some(dir) = fx_dir("afr") {
                let _ = std::fs::create_dir_all(&dir);
                let body: String = all.iter().map(|(f, v)| format!("{f} {v}\n")).collect();
                let _ = std::fs::write(dir.join(format!("{}.afr", name.trim().replace('/', "-"))), body);
            }
            name.clear();
        }
        2 => {
            p.ui.data_mut(|d| d.insert_temp(clip, all.clone()));
        }
        3 => load = p.ui.data(|d| d.get_temp::<Vec<(u32, f32)>>(clip)),
        4 => {
            for s in 0..fx::SLOTS {
                p.put(fx::id(rk, s, 0), 0.0);
            }
        }
        5 => {
            let mut x = ((p.ui.input(|i| i.time) * 1000.0) as u32) | 1;
            for s in 0..fx::SLOTS {
                let ty = g(p, ix, fx::id(rk, s, 0)).round() as usize;
                for i in 0..fx::knobs(ty).len() {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    p.put(fx::id(rk, s, 2 + i as u32), (x & 0xFFFF) as f32 / 65535.0);
                }
            }
        }
        _ => {}
    }
    p.ui.data_mut(|d| d.insert_temp(name_id, name));
    if let Some(v) = load {
        for (f, x) in v {
            p.put(fx::id(rk, 0, 0) + f, x);
        }
    }
}

/// The Morph page: the four modes, All / Elements, X / Y (Y hidden in the linear modes), the
/// element knobs, Auto Gain, Fixed Pitch, the Morph Linear groups and the morph square.
fn morph_page(p: &mut Pane, ix: &Ix, st: &mut St) {
    use lg_synth::alchemy::{MORPH, MORPH_GROUPS};
    let mode = g(p, ix, MORPH).round() as usize;
    p.text(84.0, 66.0, Align2::LEFT_CENTER, "MORPH", 12.5, p.skin.text);
    for (k, n) in ["Off", "XFade XY", "XFade Linear", "Morph XY", "Morph Linear"].iter().enumerate() {
        if rack::flat(p, p.rect(150.0 + k as f32 * 92.0, 56.0, 90.0, 20.0), n, mode == k, true, ("morph-mode", k)) {
            p.put(MORPH, k as f32);
        }
    }
    let linear = mode == 2 || mode == 4;
    let elements = g(p, ix, MORPH + 1) >= 0.5;
    for (k, n) in ["All", "Elements"].iter().enumerate() {
        if rack::flat(p, p.rect(84.0 + k as f32 * 66.0, 92.0, 64.0, 20.0), n, elements == (k == 1), mode >= 3, ("morph-el", k)) {
            p.put(MORPH + 1, k as f32);
        }
    }
    if mode == 4 {
        for (k, n) in MORPH_GROUPS.iter().enumerate() {
            let cur = g(p, ix, MORPH + 2).round() as usize;
            if rack::flat(p, p.rect(230.0 + k as f32 * 64.0, 92.0, 62.0, 20.0), n, cur == k, true, ("morph-grp", k)) {
                p.put(MORPH + 2, k as f32);
            }
        }
    }
    // X / Y, or the element knobs
    let knobs: Vec<(u32, u32, &str, Color32)> = if elements && mode >= 3 {
        vec![(79, 80, "Additive/VA", BLUE), (81, 82, "Spectral/Granular", ORANGE), (MORPH + 6, MORPH + 7, "Pitch", rgb(0x5ac46a)), (83, 84, "Formant", rgb(0xa070e0))]
    } else {
        vec![(75, 76, "", Color32::WHITE)]
    };
    for (k, (xid, yid, n, _)) in knobs.iter().enumerate() {
        let x = 120.0 + k as f32 * 120.0;
        rack::knob(p, ix, st, x, 128.0, 36.0, *xid, &if n.is_empty() { "X".to_string() } else { format!("{n} X") });
        if !linear {
            rack::knob(p, ix, st, x + 54.0, 128.0, 36.0, *yid, "Y");
        }
    }
    for (k, (id, n)) in [(MORPH + 3, "Auto Gain"), (MORPH + 4, "Fixed Pitch")].iter().enumerate() {
        let on = g(p, ix, *id) >= 0.5;
        if rack::flat(p, p.rect(84.0 + k as f32 * 92.0, 220.0, 88.0, 20.0), n, on, true, ("morph-sw", k)) {
            p.put(*id, if on { 0.0 } else { 1.0 });
        }
    }
    // the morph square: drag a point (with Elements, each element's)
    let sq = p.rect(650.0, 86.0, 136.0, 136.0);
    p.ui.painter().rect_filled(sq, 3.0, rgb(0x12151a));
    for (lx, ly, l) in [(0.0, 0.0, "C"), (1.0, 0.0, "D"), (0.0, 1.0, "A"), (1.0, 1.0, "B")] {
        p.ui.painter().text(pos2(sq.min.x + 8.0 + lx * (sq.width() - 16.0), sq.min.y + 8.0 + (1.0 - ly) * (sq.height() - 16.0)), Align2::CENTER_CENTER, l, look::regular(10.0), p.skin.dim);
    }
    for (k, (xid, yid, _, col)) in knobs.iter().enumerate() {
        let (fx_, fy) = (g(p, ix, *xid), if linear { 0.5 } else { g(p, ix, *yid) });
        let c = pos2(sq.min.x + fx_ * sq.width(), sq.min.y + fy * sq.height());
        p.ui.painter().circle_filled(c, 6.0, *col);
        let resp = p.ui.interact(Rect::from_center_size(c, vec2(16.0, 16.0)), egui::Id::unique(("morph-pt", p.t, k)), Sense::drag());
        if resp.dragged()
            && let Some(q) = resp.interact_pointer_pos()
        {
            p.put(*xid, ((q.x - sq.min.x) / sq.width()).clamp(0.0, 1.0));
            if !linear {
                p.put(*yid, ((q.y - sq.min.y) / sq.height()).clamp(0.0, 1.0));
            }
        }
    }
}

/// A source's Source Select field: the content's name, ‹ › through the VA waves, and the menu:
/// Import Audio…, Load VA ▸, Load Source / Save Source…, Copy, Paste, Clear, Randomize, Swap With.
fn source_select(p: &mut Pane, ix: &Ix, st: &mut St, s: u32, audio: Option<&str>) {
    let su = s as usize;
    let letter = ["A", "B", "C", "D"][su];
    let suffix = format!(" {letter}");
    let el = element(p, ix, s);
    let name = match el {
        0 => lg_synth::alchemy::VA_SHAPES[(g(p, ix, 301 + s).round().max(0.0) as usize).min(3)].to_string(),
        _ => audio.map(|a| a.to_string()).unwrap_or_else(|| ELEMENTS[el].to_string()),
    };
    let r = p.rect(560.0, 56.0, 190.0, 20.0);
    let resp = rack::field_btn(p, r, &name, ("src-select", s), true);
    rack::level_fill(p, r, st.levels[su]);
    let d = rack::arrows(p, p.rect(754.0, 56.0, 30.0, 20.0), ("src-sel-ar", s));
    if d != 0 {
        let v = (g(p, ix, 301 + s) + d as f32).rem_euclid(4.0);
        p.put(301 + s, v);
    }
    // the source's own settings (by name), for Copy / Save / Clear / Swap
    let mine: Vec<(u32, &'static str)> = p.params.iter().filter(|q| q.name.ends_with(&suffix)).map(|q| (q.id, q.name)).collect();
    let files: Vec<(String, std::path::PathBuf)> = rack::mod_dir("src").and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().map(|e| e.path()).filter(|x| x.extension().is_some_and(|e| e == "src")).map(|x| (x.file_stem().unwrap_or_default().to_string_lossy().into_owned(), x)).collect();
    let name_id = egui::Id::unique(("src-save-name", p.t));
    let mut save_name: String = p.ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
    let mut act: Option<(u8, usize)> = None;
    let mut load: Option<Vec<(String, f32)>> = None;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        if ui.button("Import Audio…").clicked() {
            act = Some((0, 0));
            ui.close();
        }
        ui.menu_button("Load VA", |ui| {
            for (k, n) in lg_synth::alchemy::VA_SHAPES.iter().enumerate() {
                if ui.button(*n).clicked() {
                    act = Some((1, k));
                    ui.close();
                }
            }
        });
        ui.menu_button("Load Source", |ui| {
            if files.is_empty() {
                ui.label("No saved sources");
            }
            for (n, path) in &files {
                if ui.button(n).clicked() {
                    load = Some(std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| {
                        let (k, v) = l.rsplit_once('\t')?;
                        Some((k.to_string(), v.parse().ok()?))
                    }).collect());
                    ui.close();
                }
            }
        });
        ui.menu_button("Save Source…", |ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut save_name).hint_text("Name").desired_width(150.0));
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || enter) && !save_name.trim().is_empty() {
                act = Some((2, 0));
                ui.close();
            }
        });
        ui.separator();
        for (k, n) in ["Copy Source", "Paste Source", "Clear Source", "Randomize Source"].iter().enumerate() {
            if ui.button(*n).clicked() {
                act = Some((3 + k as u8, 0));
                ui.close();
            }
        }
        ui.menu_button("Swap With Source", |ui| {
            for (k, l) in ["A", "B", "C", "D"].iter().enumerate() {
                if k != su && ui.button(*l).clicked() {
                    act = Some((7, k));
                    ui.close();
                }
            }
        });
    });
    // settings keyed by their name without the letter, so they move between sources
    let generic = |n: &str| n[..n.len() - suffix.len()].to_string();
    let cur: Vec<(String, f32)> = mine.iter().map(|(id, n)| (generic(n), g(p, ix, *id))).collect();
    let clip = egui::Id::unique("alchemy-source-clipboard");
    match act {
        Some((0, _)) => p.actions.push("import".into()),
        Some((1, k)) => {
            // Load VA: VA on, the other engines off
            p.put(297 + s, 1.0);
            p.put(301 + s, k as f32);
            for f in [293, 309, 313] {
                p.put(f + s, 0.0);
            }
            st.elem[su] = 1;
        }
        Some((2, _)) => {
            if let Some(dir) = rack::mod_dir("src") {
                let _ = std::fs::create_dir_all(&dir);
                let body: String = cur.iter().map(|(n, v)| format!("{n}\t{v}\n")).collect();
                let _ = std::fs::write(dir.join(format!("{}.src", save_name.trim().replace('/', "-"))), body);
            }
            save_name.clear();
        }
        Some((3, _)) => {
            p.ui.data_mut(|d| d.insert_temp(clip, cur.clone()));
        }
        Some((4, _)) => load = p.ui.data(|d| d.get_temp::<Vec<(String, f32)>>(clip)),
        Some((5, _)) => {
            for (id, _) in &mine {
                if let Some(q) = p.params.iter().find(|q| q.id == *id) {
                    let d = q.default;
                    p.put(*id, d);
                }
            }
        }
        Some((6, _)) => {
            // random values for the knobs of the source's elements that are on
            let mut x = ((p.ui.input(|i| i.time) * 1000.0) as u32) | 1;
            for (id, _) in &mine {
                let Some(q) = p.params.iter().find(|q| q.id == *id).cloned() else { continue };
                if matches!(q.unit, lg_dsp::Unit::Menu | lg_dsp::Unit::OnOff) || q.hidden {
                    continue;
                }
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let f = (x & 0xFFFF) as f32 / 65535.0;
                let v = q.default + (q.min + f * (q.max - q.min) - q.default) * 0.4;
                p.put(*id, v);
            }
        }
        Some((7, k)) => {
            let other = format!(" {}", ["A", "B", "C", "D"][k]);
            let theirs: Vec<(u32, String)> = p.params.iter().filter(|q| q.name.ends_with(&other)).map(|q| (q.id, q.name[..q.name.len() - other.len()].to_string())).collect();
            let vals_mine: Vec<(u32, String, f32)> = mine.iter().map(|(id, n)| (*id, generic(n), g(p, ix, *id))).collect();
            let vals_theirs: Vec<(u32, String, f32)> = theirs.iter().map(|(id, n)| (*id, n.clone(), g(p, ix, *id))).collect();
            for (id, n, _) in &vals_mine {
                if let Some(t) = vals_theirs.iter().find(|t| &t.1 == n) {
                    p.put(*id, t.2);
                }
            }
            for (id, n, _) in &vals_theirs {
                if let Some(m) = vals_mine.iter().find(|m| &m.1 == n) {
                    p.put(*id, m.2);
                }
            }
        }
        _ => {}
    }
    p.ui.data_mut(|d| d.insert_temp(name_id, save_name));
    if let Some(list) = load {
        for (id, n) in &mine {
            if let Some((_, v)) = list.iter().find(|(k, _)| *k == generic(n)) {
                p.put(*id, *v);
            }
        }
    }
}

/// Stores the source edit window's zones (the running instrument plays them at once).
fn store_zones(app: &mut App, t: usize, doc: lg_synth::alchemy_zones::ZoneDoc) {
    let Some(id) = instwin::track_id(app, t) else { return };
    let arc = std::sync::Arc::new(doc.clone());
    instwin::with_inst(app, t, |a: &mut lg_synth::alchemy::Alchemy| a.set_zones(arc));
    app.inst.alch_zones.insert(id, doc);
    app.dirty = true;
}

/// Reads audio files for zones: their root key from the filename, else from pitch analysis.
fn zone_files(paths: &[std::path::PathBuf], app: &mut App) -> Vec<lg_synth::alchemy_zones::ImportFile> {
    let mut out = Vec::new();
    for f in paths {
        match lg_synth::wav::read_audio(f) {
            Ok(a) => out.push(zone_file(f, &a)),
            Err(e) => app.tell(format!("Couldn't import {}: {e}", f.display())),
        }
    }
    out
}

pub fn zone_file(f: &std::path::Path, a: &lg_synth::wav::Audio) -> lg_synth::alchemy_zones::ImportFile {
    let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let root = lg_synth::sampler_doc::root_from_name(&name).or_else(|| {
        let ch = a.channels.len().max(1) as f32;
        let mono: Vec<f32> = (0..a.frames()).map(|i| a.channels.iter().map(|c| c.get(i).copied().unwrap_or(0.0)).sum::<f32>() / ch).collect();
        let data = lg_synth::sampler::SampleData { rate: a.rate as f32, ch: vec![lg_synth::sampler::Pcm::F32(mono)] };
        lg_synth::sampler_doc::detect_pitch(&data).map(|(k, c)| (k as f32 + c / 100.0).round().clamp(0.0, 127.0) as u8)
    });
    let audio = std::sync::Arc::new(lg_synth::alchemy_zones::ZAudio { ch: a.channels.clone(), rate: a.rate as f32 });
    lg_synth::alchemy_zones::ImportFile { name, path: Some(f.to_path_buf()), audio, root }
}

fn audio_dialog(title: &str) -> kn_ui::files::FileDialog {
    kn_ui::files::FileDialog::new().set_title(title).add_filter("Audio", &["wav", "aif", "aiff", "WAV", "AIF", "AIFF"])
}

/// The source edit window's file and audition actions.
fn zone_action(app: &mut App, ctx: &egui::Context, st_id: egui::Id, t: usize, verb: &str, arg: &str, doc: &mut lg_synth::alchemy_zones::ZoneDoc) {
    use lg_synth::alchemy_zones as az;
    let a: Vec<usize> = arg.split(':').filter_map(|x| x.parse().ok()).collect();
    let s = a.first().copied().unwrap_or(0).min(3);
    let mut changed = false;
    match verb {
        "zgimport" => {
            let Some(ps) = audio_dialog("Import Audio").pick_files() else { return };
            let files = zone_files(&ps, app);
            if files.is_empty() {
                return;
            }
            let g = doc.src[s].import(files, az::Mapping::Pitch, false);
            // the Sampler element plays the zones (Granular off: the two are exclusive)
            instwin::checkpoint(app, t, "Import Audio");
            instwin::set_params(app, t, &[(277 + s as u32, 1.0), (az::SAMP_ON + s as u32, 1.0), (313 + s as u32, 0.0)]);
            let mut st: St = ctx.data(|d| d.get_temp(st_id)).unwrap_or_default();
            st.ed.group = g;
            st.ed.sel = vec![(g, 0)];
            ctx.data_mut(|d| d.insert_temp(st_id, st));
            return store_zones(app, t, doc.clone());
        }
        "zadd" => {
            let Some(ps) = audio_dialog("Add Zone").pick_files() else { return };
            let files = zone_files(&ps, app);
            let zones = az::map_zones(files, az::Mapping::Pitch);
            let g = a.get(1).copied().filter(|&g| g < doc.src[s].groups.len());
            doc.src[s].paste(zones, g);
            changed = true;
        }
        "zexchange" | "zstep" => {
            let (g, z) = if verb == "zexchange" { (a.get(1).copied().unwrap_or(0), a.get(2).copied().unwrap_or(0)) } else { (0, 0) };
            let Some(old) = doc.src[s].zone(g, z).cloned() else { return };
            let path = if verb == "zexchange" {
                audio_dialog("Exchange Sample").pick_file()
            } else {
                // ‹ ›: the previous / next audio file in the sample's folder
                let d: i64 = arg.split(':').nth(1).and_then(|x| x.parse().ok()).unwrap_or(1);
                old.file.as_ref().and_then(|f| {
                    let dir = f.parent()?;
                    let mut list: Vec<std::path::PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|x| x.extension().is_some_and(|e| ["wav", "aif", "aiff"].contains(&e.to_string_lossy().to_lowercase().as_str()))).collect();
                    list.sort();
                    let i = list.iter().position(|x| x == f)? as i64;
                    list.get((i + d).rem_euclid(list.len() as i64) as usize).cloned()
                })
            };
            let Some(path) = path else { return };
            let Some(f) = zone_files(&[path], app).pop() else { return };
            if let Some(zn) = doc.src[s].zone_mut(g, z) {
                let n = f.audio.frames() as u32;
                zn.name = f.name;
                zn.file = f.path;
                zn.audio = Some(f.audio);
                if let Some(r) = f.root {
                    zn.key = r;
                }
                zn.start = 0;
                zn.end = n;
                zn.set_loop(0, n);
                zn.xfade = zn.xfade.min(n);
            }
            changed = true;
        }
        "zclear" => {
            doc.src[s] = Default::default();
            changed = true;
        }
        "zaud-key" => {
            let (n, v) = (a.get(1).copied().unwrap_or(60) as u8, a.get(2).copied().unwrap_or(100) as u8);
            instwin::with_inst(app, t, |al: &mut lg_synth::alchemy::Alchemy| {
                al.audition_stop();
                al.audition_key(s, n, v)
            });
        }
        "zaud-zone" => {
            let (g, z) = (a.get(1).copied().unwrap_or(0), a.get(2).copied().unwrap_or(0));
            instwin::with_inst(app, t, |al: &mut lg_synth::alchemy::Alchemy| {
                al.audition_stop();
                al.audition_zone(s, g, z)
            });
        }
        "zaud-stop" => {
            instwin::with_inst(app, t, |al: &mut lg_synth::alchemy::Alchemy| al.audition_stop());
        }
        _ => {}
    }
    if changed {
        instwin::checkpoint(app, t, "Edit Zones");
        store_zones(app, t, doc.clone());
    }
}

/// The Sampler element: On (with the source page's On), Volume, Reverse, the Loop Mode with
/// Position / Speed — or Loop Start and Loop End / Length in Start/End and Start/Length — and the
/// sample's waveform with the newest note's play position.
fn sampler_view(p: &mut Pane, ix: &Ix, st: &mut St, s: usize, zdoc: &lg_synth::alchemy_zones::ZoneDoc, zpos: Option<(usize, usize, f64)>) {
    use lg_synth::alchemy_zones as az;
    let su = s as u32;
    if zdoc.src[s].zone_count() == 0 {
        p.text(300.0, 170.0, Align2::CENTER_CENTER, "Import audio in Sampler mode (source pop-up ▸ Import Audio… or Edit ▸ Group +)", 11.0, p.skin.dim);
    }
    let rev = g(p, ix, az::SAMP_REV + su) >= 0.5;
    if rack::flat(p, p.rect(124.0, 88.0, 60.0, 20.0), "Reverse", rev, true, ("samp-rev", s)) {
        p.put(az::SAMP_REV + su, if rev { 0.0 } else { 1.0 });
    }
    rack::menu_arrows(p, ix, 200.0, 88.0, 110.0, az::LOOP_MODE + su);
    p.text(200.0, 114.0, Align2::LEFT_CENTER, "Loop Mode", 10.0, p.skin.dim);
    rack::knob(p, ix, st, 110.0, 124.0, 38.0, az::SAMP_VOL + su, "Volume");
    let mode = g(p, ix, az::LOOP_MODE + su).round() as u8;
    let pair = match mode {
        az::L_STARTEND => [(az::LOOP_START + su, "Loop Start"), (az::LOOP_END + su, "Loop End")],
        az::L_STARTLEN => [(az::LOOP_START + su, "Loop Start"), (az::LOOP_END + su, "Loop Length")],
        _ => [(35 + su, "Position"), (39 + su, "Speed")],
    };
    for (j, (pid, l)) in pair.iter().enumerate() {
        rack::knob(p, ix, st, 180.0 + j as f32 * 64.0, 124.0, 38.0, *pid, l);
    }
    // the waveform of the zone sounding (else the first zone)
    let r = p.rect(330.0, 124.0, 440.0, 96.0);
    p.ui.painter().rect_filled(r, 3.0, rgb(0x15181d));
    let src = &zdoc.src[s];
    let shown = zpos.and_then(|(g, z, at)| src.zone(g, z).map(|zn| (zn, Some(at)))).or_else(|| src.first_zone().map(|z| (z, None)));
    if let Some((z, at)) = shown
        && let Some(a) = z.audio.as_ref()
    {
        let d = &a.ch[0];
        let n = d.len().max(1);
        let cols = r.width() as usize;
        let mid = r.center().y;
        for c in 0..cols {
            let (i0, i1) = (c * n / cols, ((c + 1) * n / cols).max(c * n / cols + 1).min(n));
            let (mut mn, mut mx) = (0.0f32, 0.0f32);
            for &v in d[i0..i1].iter().step_by(((i1 - i0) / 32).max(1)) {
                mn = mn.min(v);
                mx = mx.max(v);
            }
            p.ui.painter().vline(r.min.x + c as f32 + 0.5, (mid - mx * 44.0)..=(mid - mn * 44.0 + 0.5), Stroke::new(1.0, rgb(0x5f86b3)));
        }
        // the progress of the newest note (and the Position when idle)
        let frac = match at {
            Some(f) => f as f32 / n as f32,
            None => g(p, ix, 35 + su),
        };
        let x = r.min.x + frac.clamp(0.0, 1.0) * r.width();
        p.ui.painter().vline(x, r.y_range(), Stroke::new(1.0, Color32::WHITE));
        p.ui.painter().with_clip_rect(r).text(r.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, &z.name, look::regular(10.0), p.skin.dim);
    }
}
