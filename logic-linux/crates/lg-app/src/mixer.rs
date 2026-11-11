//! The Mixer (X): one channel strip per track, aux, VCA and the Stereo Out, as Logic's: the
//! Setting button, the gain reduction meter, the EQ thumbnail, MIDI FX, the input / instrument
//! slot, the Audio FX (click: the plug-in's window, hover: bypass, the arrow: the plug-in menu,
//! drag: move, Option-drag: copy), the sends, the output, the group, the automation mode, the
//! pan knob, the VCA slot, volume and peak readouts, the fader with its meter, Mute / Solo and
//! the name (click: select, Shift / ⌘-click: add). Faders drag (Option-click / double-click:
//! 0 dB), knobs drag up and down. The bar: Edit / Options / View, Sends on Faders, the filter
//! buttons (Audio … MIDI, in All), Single | Tracks | All. Edits go on the Mixer Undo list;
//! groups follow (mixstate).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::TrackKind;

use crate::app::{App, MixerView};
use crate::audio::{FxStrip, Out};
use crate::look;
use crate::mixstate::{self, Input, Kind, MASTER, VCA};
use crate::tracks::{db_to_frac, frac_to_db};

const STRIP_W: f32 = 92.0;
const NARROW_W: f32 = 66.0;
/// Strip indexes from here up are aux strips (AUX + the aux's index).
pub const AUX: usize = 1_000_000;

pub fn fx_strip(s: usize) -> FxStrip {
    if s == MASTER {
        FxStrip::Master
    } else if s >= AUX {
        FxStrip::Aux(s - AUX)
    } else {
        FxStrip::Track(s)
    }
}

fn shown(app: &App, part: &str) -> bool {
    !app.mix.hidden_parts.contains(part)
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(proj) = app.project.clone() else { return };
    app.mix.slot_rects.clear();
    // Logic's Mixer bar: Edit / Options / View, Sends on Faders, the filters, Single | Tracks | All
    let bar = Rect::from_min_size(area.min, vec2(area.width(), 30.0));
    p.rect_filled(bar, 0.0, look::header_bg());
    p.hline(bar.x_range(), bar.max.y, Stroke::new(1.0, look::line()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "mixer-Edit"), ("Options", "mixer-Options"), ("View", "mixer-View")]);
        let sof = match app.sends_on_faders { Some(b) => format!("Sends on Faders: Bus {}", b + 1), None => "Sends on Faders: Off".into() };
        ui.menu_button(sof, |ui| {
            if ui.add(egui::Button::selectable(app.sends_on_faders.is_none(), "Off")).clicked() {
                app.sends_on_faders = None;
                ui.close();
            }
            let mut buses: Vec<usize> = app.auxes.iter().map(|a| a.bus).collect();
            buses.sort();
            buses.dedup();
            for b in buses {
                if ui.add(egui::Button::selectable(app.sends_on_faders == Some(b), format!("Bus {}", b + 1))).clicked() {
                    app.sends_on_faders = Some(b);
                    ui.close();
                }
            }
        });
        if app.mixer_view == MixerView::All {
            ui.add_space(10.0);
            for (k, label) in [(Kind::Audio, "Audio"), (Kind::Inst, "Inst"), (Kind::Aux, "Aux"), (Kind::Bus, "Bus"), (Kind::Input, "Input"), (Kind::Output, "Output"), (Kind::MasterVca, "Master/VCA"), (Kind::Midi, "MIDI")] {
                let on = !app.mix.hidden.contains(&k);
                let b = egui::Button::new(egui::RichText::new(label).size(11.0).color(if on { Color32::WHITE } else { look::text_dim() })).fill(if on { look::button_on() } else { look::group_bg() }).corner_radius(4.0);
                if ui.add(b).clicked() {
                    crate::mixercmd::toggle_kind(app, k);
                }
            }
        }
    });
    // the view switch at the right
    let seg = Rect::from_min_size(pos2(bar.max.x - 200.0, bar.min.y + 4.0), vec2(190.0, 22.0));
    p.rect_filled(seg, 6.0, look::group_bg());
    for (k, (v, name)) in [(MixerView::Single, "Single"), (MixerView::Tracks, "Tracks"), (MixerView::All, "All")].into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(seg.min.x + 2.0 + k as f32 * 62.0, seg.min.y + 2.0), vec2(62.0, 18.0));
        let on = app.mixer_view == v;
        if on {
            p.rect_filled(r, 5.0, look::accent());
        }
        p.text(r.center(), Align2::CENTER_CENTER, name, look::regular(12.0), if on { Color32::WHITE } else { look::text() });
        if ui.interact(r, ui.scope_id().with(("mixview", k)), Sense::click()).clicked() {
            app.mixer_view = v;
        }
    }
    let body = Rect::from_min_max(pos2(area.min.x, bar.max.y + 1.0), area.max);
    // which strips: Single = the selected track and what it feeds; Tracks = the tracks, the
    // auxes and VCAs in use; All = every channel strip; then the filters
    let tracks: Vec<usize> = match app.mixer_view {
        MixerView::Single => app.selected_track.into_iter().filter(|t| *t < proj.tracks.len()).collect(),
        _ => (0..proj.tracks.len()).collect(),
    };
    let feeds: Vec<usize> = tracks.iter().flat_map(|t| app.sends.get(t).into_iter().flatten().map(|s| s.bus).chain(match app.mix.outputs.get(t) { Some(Out::Bus(b)) => Some(*b), _ => None })).collect();
    let auxes: Vec<usize> = match app.mixer_view {
        MixerView::Single => (0..app.auxes.len()).filter(|a| feeds.contains(&app.auxes[*a].bus)).collect(),
        MixerView::Tracks => (0..app.auxes.len()).filter(|a| mixstate::bus_used(app, app.auxes[*a].bus)).collect(),
        MixerView::All => (0..app.auxes.len()).collect(),
    };
    let mut strips: Vec<usize> = tracks;
    strips.extend(auxes.into_iter().map(|a| AUX + a));
    if app.mixer_view != MixerView::Single {
        strips.extend((0..app.mix.vcas.len()).map(|v| VCA + v));
    }
    strips.retain(|s| !app.mix.hidden.contains(&mixstate::kind_of(app, *s)));
    let w = if app.mix.narrow { NARROW_W } else { STRIP_W };
    // the strips scroll sideways (the wheel, a trackpad), the Stereo Out stays at the right
    let room = body.max.x - w - 16.0;
    let fits = (((room - body.min.x - 8.0) / (w + 2.0)).floor() as usize).max(1);
    if ui.rect_contains_pointer(body) {
        let d = ui.input(|i| i.smooth_scroll_delta);
        let dx = if d.x.abs() > d.y.abs() { d.x } else { d.y };
        if dx != 0.0 {
            app.mix.scroll -= dx / (w + 2.0);
        }
    }
    app.mix.scroll = app.mix.scroll.clamp(0.0, strips.len().saturating_sub(fits) as f32);
    let skip = app.mix.scroll.round() as usize;
    let mut x = body.min.x + 8.0;
    let mut last_kind = None;
    for &s in strips.iter().skip(skip) {
        if x + w > room {
            break;
        }
        let k = (s >= AUX) as u8 + (s >= VCA) as u8;
        if last_kind.is_some_and(|l| l != k) {
            x += 8.0;
            p.vline(x - 5.0, body.min.y + 8.0..=body.max.y - 8.0, Stroke::new(1.0, look::line()));
        }
        last_kind = Some(k);
        let r = Rect::from_min_size(pos2(x, body.min.y + 6.0), vec2(w, body.height() - 10.0));
        if s >= VCA {
            vca_strip(app, ui, r, s - VCA);
        } else if s >= AUX {
            aux_strip(app, ui, r, s - AUX);
        } else {
            track_strip(app, ui, r, s);
        }
        x += w + 2.0;
    }
    // the Stereo Out at the right
    let r = Rect::from_min_size(pos2(body.max.x - w - 8.0, body.min.y + 6.0), vec2(w, body.height() - 10.0));
    if r.min.x > x - 2.0 && !app.mix.hidden.contains(&Kind::MasterVca) {
        output_strip(app, ui, r);
    }
    // a plug-in slot dropped: moved there, or copied with Option (Logic's Option-drag)
    if let Some((from, k)) = app.mix.slot_drag {
        if let Some(pp) = ui.ctx().pointer_latest_pos() {
            ui.painter().text(pp + vec2(10.0, 0.0), Align2::LEFT_CENTER, mixstate::inserts(app, from).get(k).map(|u| u.name.clone()).unwrap_or_default(), look::regular(11.0), look::text());
        }
        if ui.input(|i| i.pointer.any_released()) {
            let copy = ui.input(|i| i.modifiers.alt);
            let at = ui.ctx().pointer_latest_pos().and_then(|pp| app.mix.slot_rects.iter().find(|(r, ..)| r.contains(pp)).map(|(_, s, k)| (*s, *k)));
            app.mix.slot_drag = None;
            if let Some((to, tk)) = at {
                move_plugin(app, from, k, to, tk, copy);
            }
        }
    }
}

/// Moves (or copies) insert `k` of strip `from` to insert place `tk` of strip `to`.
pub fn move_plugin(app: &mut App, from: usize, k: usize, to: usize, tk: usize, copy: bool) {
    let Some(u) = mixstate::inserts(app, from).get(k).cloned() else { return };
    if from == to && !copy && (tk == k || tk == k + 1) {
        return;
    }
    mixstate::checkpoint(app, if copy { "Copy Plug-in" } else { "Move Plug-in" });
    let mut tk = tk;
    if !copy {
        mixstate::edit_inserts(app, from, |v| {
            v.remove(k);
        });
        if from == to && tk > k {
            tk -= 1;
        }
    }
    mixstate::edit_inserts(app, to, |v| {
        let at = tk.min(v.len());
        v.insert(at, u);
    });
    app.load_engine();
}

/// An aux strip (its bus in, its effects, its fader).
pub fn aux_strip(app: &mut App, ui: &mut egui::Ui, r: Rect, a: usize) {
    let Some(aux) = app.auxes.get(a).cloned() else { return };
    let meter = app.engine.lock().map(|e| e.aux_meters.get(a).copied().unwrap_or(0.0)).unwrap_or(0.0);
    let sel = app.mix.selected.contains(&(AUX + a));
    if let Some(v) = strip(app, ui, r, AUX + a, &aux.name, None, (aux.volume_db, aux.pan, aux.mute, aux.solo), meter, sel) {
        let old = (aux.volume_db, aux.pan, aux.mute, aux.solo);
        mixstate::set_values(app, AUX + a, v);
        mixstate::follow_group(app, AUX + a, old, v);
        app.dirty = true;
        app.load_engine();
    }
}

/// A VCA fader strip.
pub fn vca_strip(app: &mut App, ui: &mut egui::Ui, r: Rect, v: usize) {
    let Some(x) = app.mix.vcas.get(v).cloned() else { return };
    let sel = app.mix.selected.contains(&(VCA + v));
    if let Some(nv) = strip(app, ui, r, VCA + v, &x.name, None, (x.volume_db, 0.0, x.mute, x.solo), 0.0, sel) {
        mixstate::set_values(app, VCA + v, nv);
        app.dirty = true;
        app.load_engine();
    }
}

/// A track's channel strip (the Mixer's, and the Inspector's left one).
pub fn track_strip(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize) {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(i)).cloned() else { return };
    let meter = app.engine.lock().map(|e| e.meters.get(i).copied().unwrap_or(0.0)).unwrap_or(0.0);
    let sel = app.selected_track == Some(i) || app.mix.selected.contains(&i);
    // Sends on Faders: the fader is the send's level to that bus
    if let Some(b) = app.sends_on_faders {
        let k = app.sends.get(&i).and_then(|v| v.iter().position(|s| s.bus == b));
        let level = k.and_then(|k| app.sends.get(&i).map(|v| v[k].level_db)).unwrap_or(-90.0);
        if let Some((v, ..)) = strip(app, ui, r, i, &t.name, Some(t.kind), (level, t.pan, t.mute, t.solo), meter, sel) {
            if let Some(k) = k {
                if let Some(x) = app.sends.get_mut(&i).and_then(|l| l.get_mut(k)) {
                    x.level_db = v;
                }
                app.dirty = true;
                app.load_engine();
            }
        }
        return;
    }
    if let Some((v, pan, m, s)) = strip(app, ui, r, i, &t.name, Some(t.kind), (t.volume_db, t.pan, t.mute, t.solo), meter, sel) {
        let old = (t.volume_db, t.pan, t.mute, t.solo);
        mixstate::set_values(app, i, (v, pan, m, s));
        mixstate::follow_group(app, i, old, (v, pan, m, s));
        if v != t.volume_db {
            crate::automation::record(app, i, crate::automation::VOLUME, crate::automation::db_to_logic(v));
        }
        if pan != t.pan {
            crate::automation::record(app, i, crate::automation::PAN, 64.0 + pan * 63.0);
        }
        app.dirty = true;
        app.sync_mix();
    }
}

/// The Stereo Out strip (the master level and the master chain).
pub fn output_strip(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let master = app.engine.lock().map(|e| e.surround.as_ref().map_or(e.master_meter, |s| s.meters.iter().copied().fold(0.0, f32::max))).unwrap_or(0.0);
    let master_db = 20.0 * app.engine.lock().map(|e| e.master).unwrap_or(1.0).max(1e-5).log10();
    // (a surround project: the surround master channel strip)
    let name = if crate::surround::active(app) { "Surround" } else { "Stereo Out" };
    if let Some((v, ..)) = strip(app, ui, r, MASTER, name, None, (master_db, 0.0, false, false), master, app.mix.selected.contains(&MASTER)) {
        if let Ok(mut e) = app.engine.lock() {
            e.master = crate::app::db_to_gain(v);
        }
    }
    if crate::surround::active(app) {
        crate::surround_ui::bounce_button(app, ui, r);
    }
}

fn effect_names() -> &'static std::collections::HashSet<&'static str> {
    static S: std::sync::OnceLock<std::collections::HashSet<&'static str>> = std::sync::OnceLock::new();
    S.get_or_init(|| crate::units::effect_names().into_iter().collect())
}

/// Logic's plug-in menu (data/context-menus.txt mixer-audiofx-slot): the categories and
/// plug-ins, each with Stereo / Dual Mono; the ones Studio has are enabled. Returns the pick
/// (Some(None): No Plug-in).
fn plugin_menu(ui: &mut egui::Ui, current: Option<&str>) -> Option<Option<String>> {
    let mut picked = None;
    ui.set_min_width(200.0);
    if current.is_some() && ui.button("No Plug-in").clicked() {
        picked = Some(None);
        ui.close();
    }
    fn walk(ui: &mut egui::Ui, items: &[crate::menus::Item], current: Option<&str>, picked: &mut Option<Option<String>>) {
        for it in items {
            if it.separator {
                ui.separator();
                continue;
            }
            if it.label == "Audio Units" {
                // the hosted CLAP / VST3 / LV2 effects, by manufacturer (hosting.rs)
                ui.menu_button("Audio Units", |ui| {
                    if let Some(n) = crate::hosting::menu_items(ui, lg_host::Kind::Effect, current) {
                        *picked = Some(Some(n));
                    }
                });
                continue;
            }
            let have = effect_names().contains(it.label.as_str());
            let is_plugin = it.children.iter().any(|c| c.label == "Stereo" || c.label == "Dual Mono" || c.label == "Mono");
            if is_plugin {
                let label = if current == Some(it.label.as_str()) { format!("✓ {}", it.label) } else { it.label.clone() };
                ui.add_enabled_ui(have, |ui| {
                    ui.menu_button(label, |ui| {
                        if ui.button("Stereo").clicked() {
                            *picked = Some(Some(it.label.clone()));
                            ui.close();
                        }
                        ui.add_enabled(false, egui::Button::new("Dual Mono"));
                    });
                });
            } else if !it.children.is_empty() {
                ui.menu_button(&it.label, |ui| walk(ui, &it.children, current, picked));
            } else {
                ui.add_enabled(false, egui::Button::new(&it.label));
            }
        }
    }
    walk(ui, crate::menus::context("mixer-audiofx-slot"), current, &mut picked);
    picked
}

fn bus_menu(ui: &mut egui::Ui, app: &App, current: Option<usize>) -> Option<usize> {
    let mut out = None;
    let mut row = |ui: &mut egui::Ui, b: usize| {
        let used = app.auxes.iter().find(|a| a.bus == b).map(|a| a.name.clone());
        let label = match used { Some(n) => format!("Bus {} → {n}", b + 1), None => format!("Bus {}", b + 1) };
        if ui.add(egui::Button::selectable(current == Some(b), label)).clicked() {
            out = Some(b);
            ui.close();
        }
    };
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn strip(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize, name: &str, kind: Option<TrackKind>, vals: (f32, f32, bool, bool), meter: f32, selected: bool) -> Option<(f32, f32, bool, bool)> {
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, if selected { look::track_selected() } else { look::header_bg() });
    let mut out = None;
    let (mut vol, mut pan, mut mute, mut solo) = vals;
    let is_vca = i >= VCA && i != MASTER;
    let is_master = i == MASTER;
    let aux = (i >= AUX && !is_vca && !is_master).then(|| i - AUX);
    let is_track = i < AUX;
    let id = ui.scope_id().with(("strip", i));
    let slot = |p: &egui::Painter, y: f32, text: &str, fill: Option<Color32>| {
        let s = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
        p.rect_filled(s, 4.0, fill.unwrap_or(look::group_bg()));
        p.with_clip_rect(s.shrink(2.0)).text(s.center(), Align2::CENTER_CENTER, text, look::regular(11.0), if fill.is_some() { Color32::WHITE } else { look::text_dim() });
    };
    let slot_rect = |y: f32| Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
    let mut y = r.min.y + 8.0;
    // Setting: the channel strip setting menu
    if shown(app, "Setting Menu") {
        slot(&p, y, "Setting", None);
        let resp = ui.interact(slot_rect(y), id.with("setting"), Sense::click());
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            for c in ["Copy Channel Strip Setting", "Paste Channel Strip Setting", "Paste Channel Strip Setting Plug-ins Only", "Paste Channel Strip Setting Sends Only", "Reset Channel Strip Setting", "Remove Bypassed Plug-ins"] {
                if ui.button(c).clicked() {
                    app.mix.selected = [i].into_iter().collect();
                    crate::mixercmd::run(app, &ui.ctx().clone(), c);
                    ui.close();
                }
            }
        });
        y += 24.0;
    }
    // the gain reduction meter (the strip's compressors and limiters)
    if shown(app, "Gain Reduction Meter") && !is_vca {
        let gr = app.engine.lock().map(|e| match fx_strip(i) {
            FxStrip::Track(t) => e.gr.get(t).copied().unwrap_or(0.0),
            FxStrip::Aux(a) => e.aux_gr.get(a).copied().unwrap_or(0.0),
            FxStrip::Master => e.master_gr,
        }).unwrap_or(0.0);
        let gm = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 7.0));
        p.rect_filled(gm, 2.0, look::lcd_bg());
        if gr > 0.05 {
            // right to left, 0 to 30 dB (Logic's meter fills from the right)
            let f = (gr / 30.0).sqrt().clamp(0.0, 1.0);
            p.rect_filled(Rect::from_min_max(pos2(gm.max.x - f * gm.width(), gm.min.y + 1.0), pos2(gm.max.x, gm.max.y - 1.0)), 1.0, Color32::from_rgb(0xf0, 0xb0, 0x30));
        }
        ui.interact(gm, id.with("gr"), Sense::hover()).on_hover_text(format!("Gain Reduction: {gr:.1} dB"));
        y += 11.0;
    }
    // the EQ slot: a click opens the Channel EQ; with one on the strip, its curve
    if shown(app, "EQ Thumbnail") && is_track {
        let eq_r = slot_rect(y);
        match app.eqs.get(&i).copied() {
            Some(e) => {
                p.rect_filled(eq_r, 4.0, look::lcd_bg());
                let pts: Vec<egui::Pos2> = (0..=40).map(|k| {
                    let f = 20.0 * 1000f32.powf(k as f32 / 40.0);
                    pos2(eq_r.min.x + k as f32 / 40.0 * eq_r.width(), eq_r.center().y - crate::eq::response(&e, f).clamp(-18.0, 18.0) / 18.0 * 8.0)
                }).collect();
                p.add(egui::Shape::line(pts, Stroke::new(1.2, Color32::from_rgb(0x9c, 0xe0, 0xff))));
            }
            None => slot(&p, y, "EQ", None),
        }
        if ui.interact(eq_r, id.with("eqslot"), Sense::click()).on_hover_text("Channel EQ").clicked() {
            if !app.eqs.contains_key(&i) {
                mixstate::checkpoint(app, "Insert Channel EQ");
            }
            app.eqs.entry(i).or_default();
            app.eq_window = Some(i);
            app.load_engine();
        }
        y += 26.0;
    } else if shown(app, "EQ Thumbnail") {
        y += 26.0;
    }
    let instrument = is_track.then(|| app.project.as_ref().and_then(|pr| pr.tracks.get(i)).and_then(|t| t.plugins.iter().find(|u| u.slot == 2)).map(|u| u.name.clone())).flatten();
    let inst_track = matches!(kind, Some(TrackKind::Instrument) | Some(TrackKind::Drummer));
    if inst_track && shown(app, "MIDI Effects") {
        y = crate::midifx::strip_slots(app, ui, &p, r, y, i);
    }
    // the input / instrument slot
    if shown(app, "Input / Instrument") && !is_vca {
        let ir = slot_rect(y);
        match kind {
            Some(TrackKind::Instrument) => slot(&p, y, instrument.as_deref().unwrap_or("Sampler"), Some(Color32::from_rgb(0x2f, 0x8f, 0x3a))),
            Some(TrackKind::Drummer) => slot(&p, y, instrument.as_deref().unwrap_or("Drum Kit"), Some(Color32::from_rgb(0x9a, 0x7a, 0x1e))),
            // (the recording's input unless the strip is set to none or a bus)
            Some(TrackKind::Audio) => slot(&p, y, &app.mix.inputs.get(&i).map_or_else(|| crate::recording::input_label(app, i).to_string(), |x| mixstate::in_label(*x)), None),
            None if aux.is_some() => {
                let bus = app.auxes.get(aux.unwrap()).map_or(0, |a| a.bus);
                slot(&p, y, &format!("Bus {}", bus + 1), None);
            }
            None => {
                // (the output's stereo link)
                p.rect_filled(ir, 4.0, look::group_bg());
                p.circle_stroke(ir.center() - vec2(3.0, 0.0), 4.0, Stroke::new(1.2, look::text_dim()));
                p.circle_stroke(ir.center() + vec2(3.0, 0.0), 4.0, Stroke::new(1.2, look::text_dim()));
            }
        }
        let resp = ui.interact(ir, id.with("input"), Sense::click());
        if inst_track {
            crate::instwin::slot_menu(app, &resp, i);
            if resp.on_hover_text("Open the instrument's window (right-click: choose the instrument)").clicked() {
                app.plugin_window = Some(i);
            }
        } else if kind == Some(TrackKind::Audio) {
            egui::Popup::menu(&resp).show(|ui| {
                // one input slot for the strip and for recording: the sound card's two inputs
                // (recording::InputSel) or none or a bus (mixstate::Input)
                ui.set_min_width(160.0);
                let rec = app.rec.inputs.get(&i).copied().unwrap_or_default();
                let own = app.mix.inputs.get(&i).copied();
                let mut pick: Option<Result<crate::recording::InputSel, Input>> = None;
                if ui.add(egui::Button::selectable(own == Some(Input::None), "No Input")).clicked() {
                    pick = Some(Err(Input::None));
                }
                ui.separator();
                ui.menu_button("Input", |ui| {
                    for s in crate::recording::InputSel::ALL {
                        if ui.add(egui::Button::selectable(own.is_none() && rec == s, s.label())).clicked() {
                            pick = Some(Ok(s));
                        }
                    }
                    ui.separator();
                    // (the sound card is opened as one stereo pair)
                    for k in 3..=8 {
                        ui.add_enabled(false, egui::Button::new(format!("Input {k}")));
                    }
                });
                ui.menu_button("Bus", |ui| {
                    if let Some(b) = bus_menu(ui, app, match own { Some(Input::Bus(b)) => Some(b), _ => None }) {
                        pick = Some(Err(Input::Bus(b)));
                    }
                });
                if let Some(v) = pick {
                    mixstate::checkpoint(app, "Input");
                    match v {
                        Ok(s) => {
                            app.rec.inputs.insert(i, s);
                            app.mix.inputs.remove(&i);
                        }
                        Err(x) => {
                            app.mix.inputs.insert(i, x);
                        }
                    }
                    app.dirty = true;
                    ui.close();
                }
            });
        } else if let Some(a) = aux {
            egui::Popup::menu(&resp).show(|ui| {
                let cur = app.auxes.get(a).map(|x| x.bus);
                ui.label(egui::RichText::new("Bus").color(look::text_dim()));
                if let Some(b) = bus_menu(ui, app, cur) {
                    mixstate::checkpoint(app, "Input");
                    if let Some(x) = app.auxes.get_mut(a) {
                        x.bus = b;
                    }
                    app.dirty = true;
                    app.load_engine();
                }
            });
        }
        y += 24.0;
    }
    // the Audio FX (Logic's blue slots), then an empty one
    if shown(app, "Audio Effects") && !is_vca {
        let chain = mixstate::inserts(app, i);
        let max_slots = if r.height() > 560.0 { 6 } else if r.height() > 420.0 { 4 } else { 2 };
        for (k, u) in chain.iter().enumerate().take(max_slots) {
            let sr = slot_rect(y);
            app.mix.slot_rects.push((sr, i, k));
            let fill = if u.bypass { Color32::from_gray(0x55) } else { Color32::from_rgb(0x1f, 0x5f, 0xc8) };
            p.rect_filled(sr, 4.0, fill);
            p.with_clip_rect(sr.shrink(2.0)).text(sr.center(), Align2::CENTER_CENTER, &u.name, look::regular(11.0), if u.bypass { look::text_dim() } else { Color32::WHITE });
            let hover = ui.rect_contains_pointer(sr);
            let bypass_r = Rect::from_min_size(sr.min, vec2(14.0, sr.height()));
            let arrow_r = Rect::from_min_max(pos2(sr.max.x - 14.0, sr.min.y), sr.max);
            let body_r = Rect::from_min_max(pos2(bypass_r.max.x, sr.min.y), pos2(arrow_r.min.x, sr.max.y));
            if hover {
                // the power button and the menu arrow appear on hover, as Logic's
                p.rect_filled(bypass_r.shrink(1.0), 3.0, fill);
                p.circle_stroke(bypass_r.center(), 4.5, Stroke::new(1.3, if u.bypass { look::text_dim() } else { Color32::WHITE }));
                p.rect_filled(arrow_r.shrink(1.0), 3.0, fill);
                p.text(arrow_r.center(), Align2::CENTER_CENTER, "⌄", look::regular(10.0), Color32::WHITE);
            }
            let toggle_bypass = |app: &mut App| {
                mixstate::checkpoint(app, "Bypass Plug-in");
                mixstate::edit_inserts(app, i, |v| {
                    if let Some(x) = v.get_mut(k) {
                        x.bypass = !x.bypass;
                    }
                });
                app.load_engine();
            };
            if ui.interact(bypass_r, id.with(("bypass", k)), Sense::click()).on_hover_text("Bypass").clicked() {
                toggle_bypass(app);
            }
            let body = ui.interact(body_r, id.with(("fxslot", k)), Sense::click_and_drag()).on_hover_text(crate::hosting::slot_tip(app, i, k, &u.name));
            if body.clicked() {
                if ui.input(|i| i.modifiers.alt) {
                    // Option-click: bypass (Logic's shortcut)
                    toggle_bypass(app);
                } else {
                    app.mix.fx_window = Some((i, k));
                }
            }
            if body.drag_started() {
                app.mix.slot_drag = Some((i, k));
            }
            let arrow = ui.interact(arrow_r, id.with(("fxmenu", k)), Sense::click());
            let mut picked = None;
            let sur_menu = is_master && crate::surround::active(app);
            egui::Popup::menu(&arrow).show(|ui| picked = if sur_menu { crate::surround_ui::master_plugin_menu(app, ui, Some(&u.name)) } else { plugin_menu(ui, Some(&u.name)) });
            body.context_menu(|ui| {
                if let Some(x) = if sur_menu { crate::surround_ui::master_plugin_menu(app, ui, Some(&u.name)) } else { plugin_menu(ui, Some(&u.name)) } {
                    picked = Some(x);
                }
            });
            if let Some(choice) = picked {
                mixstate::checkpoint(app, if choice.is_some() { "Insert Plug-in" } else { "Remove Plug-in" });
                let inserted = choice.is_some();
                mixstate::edit_inserts(app, i, |v| match choice {
                    Some(n) => v[k] = mixstate::new_plugin(&n),
                    None => {
                        v.remove(k);
                    }
                });
                if app.mix.fx_window == Some((i, k)) {
                    app.mix.fx_window = None;
                }
                if inserted && app.logic_settings.on(crate::prefs::OPEN_ON_INSERT) {
                    app.mix.fx_window = Some((i, k));
                }
                app.load_engine();
            }
            y += 22.0;
        }
        let er = slot_rect(y);
        app.mix.slot_rects.push((er, i, chain.len()));
        slot(&p, y, "Audio FX", None);
        let eresp = ui.interact(er, id.with("fxadd"), Sense::click());
        let mut picked = None;
        let sur_menu = is_master && crate::surround::active(app);
        egui::Popup::menu(&eresp).show(|ui| picked = if sur_menu { crate::surround_ui::master_plugin_menu(app, ui, None) } else { plugin_menu(ui, None) });
        if let Some(Some(n)) = picked {
            mixstate::checkpoint(app, "Insert Plug-in");
            mixstate::edit_inserts(app, i, |v| v.push(mixstate::new_plugin(&n)));
            // (Settings ▸ View ▸ Mixer ▸ Open plug-in window on insertion)
            if app.logic_settings.on(crate::prefs::OPEN_ON_INSERT) {
                app.mix.fx_window = Some((i, mixstate::inserts(app, i).len() - 1));
            }
            app.load_engine();
        }
        y += 24.0;
    }
    // the sends: each "Bus N" with its level knob; the empty slot below adds one
    if shown(app, "Sends") && (is_track || aux.is_some()) {
        let list = app.sends.get(&i).cloned().unwrap_or_default();
        for (k, snd) in list.iter().enumerate() {
            let sr = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 34.0, 20.0));
            let on_faders = app.sends_on_faders == Some(snd.bus);
            p.rect_filled(sr, 4.0, if on_faders { Color32::from_rgb(0x3c, 0x7c, 0xb0) } else { Color32::from_rgb(0x2a, 0x5a, 0x86) });
            p.with_clip_rect(sr.shrink(2.0)).text(sr.center(), Align2::CENTER_CENTER, format!("Bus {}", snd.bus + 1), look::regular(11.0), Color32::WHITE);
            let kc = pos2(r.max.x - 16.0, y + 10.0);
            let kr = ui.interact(Rect::from_center_size(kc, vec2(18.0, 18.0)), id.with(("send", k)), Sense::click_and_drag()).on_hover_text(format!("Send to Bus {}: {:.1} dB{}", snd.bus + 1, snd.level_db, if snd.pre { " (pre fader)" } else { "" }));
            p.circle_filled(kc, 8.0, Color32::from_gray(0x55));
            let f = db_to_frac(snd.level_db);
            let ang = std::f32::consts::PI * (0.75 + 1.5 * f);
            p.line_segment([kc, kc + vec2(ang.cos(), ang.sin()) * 6.5], Stroke::new(1.6, Color32::WHITE));
            let mut changed = None;
            if kr.drag_started() || kr.double_clicked() {
                mixstate::checkpoint(app, "Send Level");
            }
            if kr.dragged() {
                let nf = (f - kr.drag_delta().y / 120.0).clamp(0.0, 1.0);
                changed = Some(frac_to_db(nf).min(6.0));
            }
            if kr.double_clicked() {
                changed = Some(0.0);
            }
            if let Some(db) = changed {
                // a group linking this send moves its members' sends by as much
                let others: Vec<usize> = mixstate::active_group(app, i).filter(|g| app.mix.groups[*g].links_send(k)).map(|g| app.mix.group_of.iter().filter(|(s, x)| **x == g && **s != i).map(|(s, _)| *s).collect()).unwrap_or_default();
                let d = db - snd.level_db;
                if let Some(x) = app.sends.get_mut(&i).and_then(|v| v.get_mut(k)) {
                    x.level_db = db;
                }
                for o in others {
                    if let Some(x) = app.sends.get_mut(&o).and_then(|v| v.get_mut(k)) {
                        x.level_db = (x.level_db + d).clamp(-90.0, 6.0);
                    }
                }
                app.dirty = true;
                app.load_engine();
            }
            let sresp = ui.interact(sr, id.with(("sendslot", k)), Sense::click());
            egui::Popup::menu(&sresp).show(|ui| {
                ui.set_min_width(160.0);
                let mut edit: Option<Box<dyn FnOnce(&mut Vec<crate::app::SendSlot>)>> = None;
                if ui.add(egui::Button::selectable(!snd.pre, "Post Fader")).clicked() {
                    edit = Some(Box::new(move |v| v[k].pre = false));
                }
                if ui.add(egui::Button::selectable(snd.pre, "Pre Fader")).clicked() {
                    edit = Some(Box::new(move |v| v[k].pre = true));
                }
                ui.separator();
                if ui.button("No Send").clicked() {
                    edit = Some(Box::new(move |v| {
                        v.remove(k);
                    }));
                }
                ui.separator();
                ui.menu_button("Bus", |ui| {
                    if let Some(b) = bus_menu(ui, app, Some(snd.bus)) {
                        edit = Some(Box::new(move |v| v[k].bus = b));
                    }
                });
                if let Some(f) = edit {
                    mixstate::checkpoint(app, "Send");
                    if let Some(v) = app.sends.get_mut(&i) {
                        f(v);
                    }
                    // a bus without an aux gets one, as Logic makes it
                    let buses: Vec<usize> = app.sends.get(&i).map(|v| v.iter().map(|s| s.bus).collect()).unwrap_or_default();
                    for b in buses {
                        if !app.auxes.iter().any(|a| a.bus == b) {
                            mixstate::new_aux(app, Some(b));
                        }
                    }
                    app.dirty = true;
                    app.load_engine();
                    ui.close();
                }
            });
            y += 22.0;
        }
        let er = slot_rect(y);
        slot(&p, y, "Sends", None);
        let eresp = ui.interact(er, id.with("sendadd"), Sense::click());
        egui::Popup::menu(&eresp).show(|ui| {
            ui.set_min_width(160.0);
            ui.label(egui::RichText::new("Bus").color(look::text_dim()));
            if let Some(b) = bus_menu(ui, app, None) {
                // an aux can't send into the bus it takes in
                if aux.is_none_or(|a| app.auxes.get(a).is_none_or(|x| x.bus != b)) {
                    mixstate::checkpoint(app, "Send");
                    app.add_send(i, b);
                }
            }
        });
        y += 24.0;
    }
    // the output
    if shown(app, "Output") && !is_vca {
        if is_master {
            slot(&p, y, "Mastering", None);
            let resp = ui.interact(slot_rect(y), id.with("mastering"), Sense::click()).on_hover_text("Mastering Assistant");
            if resp.clicked() {
                crate::mixercmd::run(app, &ui.ctx().clone(), "Mastering Assistant…");
            }
        } else {
            let o = mixstate::output_of(app, i);
            slot(&p, y, &mixstate::out_label(o), None);
            let resp = ui.interact(slot_rect(y), id.with("output"), Sense::click());
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(170.0);
                let mut pick = None;
                if ui.add(egui::Button::selectable(o == Out::Stereo, "Stereo Output")).clicked() {
                    pick = Some(Out::Stereo);
                }
                ui.separator();
                if ui.add(egui::Button::selectable(o == Out::Off, "No Output")).clicked() {
                    pick = Some(Out::Off);
                }
                ui.separator();
                ui.menu_button("Output", |ui| {
                    if ui.add(egui::Button::selectable(o == Out::Stereo, "Stereo Output")).clicked() {
                        pick = Some(Out::Stereo);
                    }
                    if ui.add(egui::Button::selectable(o == Out::Surround, "Surround")).clicked() {
                        pick = Some(Out::Surround);
                    }
                });
                // (Spatial Audio: the 3D Object Panner, and Pan back to a bed track)
                crate::surround_ui::output_items(app, ui, o, &mut pick);
                ui.menu_button("Bus", |ui| {
                    if let Some(b) = bus_menu(ui, app, match o { Out::Bus(b) => Some(b), _ => None }) {
                        // an aux can't feed its own input
                        if aux.is_none_or(|a| app.auxes.get(a).is_none_or(|x| x.bus != b)) {
                            pick = Some(Out::Bus(b));
                        }
                    }
                });
                if let Some(v) = pick {
                    crate::surround::set_output(app, i, v);
                    ui.close();
                }
            });
        }
        y += 24.0;
    }
    // (a short Mixer leaves out the group and automation rows so the fader keeps its room)
    if r.height() > 470.0 {
        // the group slot
        if shown(app, "Group") && !is_master {
            let g = app.mix.group_of.get(&i).copied();
            let label = g.map_or("Group".into(), |g| app.mix.groups.get(g).map(|x| x.label(g)).unwrap_or_default());
            slot(&p, y, &label, g.map(|_| Color32::from_rgb(0x6a, 0x4a, 0x9a)));
            let resp = ui.interact(slot_rect(y), id.with("group"), Sense::click());
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(180.0);
                if ui.button("Open Group Settings…").clicked() {
                    app.mix.group_window = g.or((!app.mix.groups.is_empty()).then_some(0));
                    ui.close();
                }
                ui.separator();
                let mut pick: Option<Option<usize>> = None;
                if ui.add(egui::Button::selectable(g.is_none(), "No Group")).clicked() {
                    pick = Some(None);
                }
                ui.separator();
                for (k, grp) in app.mix.groups.iter().enumerate() {
                    if ui.add(egui::Button::selectable(g == Some(k), grp.label(k))).clicked() {
                        pick = Some(Some(k));
                    }
                }
                let n = app.mix.groups.len();
                if ui.button(format!("Group {}: (new)", n + 1)).clicked() {
                    pick = Some(Some(n));
                }
                if let Some(choice) = pick {
                    mixstate::checkpoint(app, "Group");
                    match choice {
                        Some(k) => {
                            if k == app.mix.groups.len() {
                                app.mix.groups.push(mixstate::Group::new());
                                app.mix.group_window = Some(k);
                            }
                            app.mix.group_of.insert(i, k);
                        }
                        None => {
                            app.mix.group_of.remove(&i);
                        }
                    }
                    app.dirty = true;
                    ui.close();
                }
            });
            y += 22.0;
        }
        if shown(app, "part:Automation") {
            let mode = if is_track { app.auto_mode.get(&i).copied().unwrap_or_default() } else { Default::default() };
            let on = !matches!(mode, crate::automation::AutoMode::Off);
            slot(&p, y, &format!("{mode:?}"), on.then(|| Color32::from_rgb(0x3a, 0x6e, 0x3a)));
            y += 26.0;
        }
    }
    // the pan knob (a Surround Panner / 3D Object Panner on strips routed to surround)
    if !is_vca && !is_master && matches!(mixstate::output_of(app, i), Out::Surround | Out::Object) {
        crate::surround_ui::pan_control(app, ui, &p, r, y, i);
        y += 40.0;
    } else if shown(app, "Pan Knob") && !is_vca && !is_master {
        let kc = pos2(r.center().x, y + 16.0);
        let kr = Rect::from_center_size(kc, vec2(34.0, 34.0));
        let kresp = ui.interact(kr, id.with("pan"), Sense::click_and_drag()).on_hover_text(format!("Pan {:+.0}", pan * 64.0));
        p.circle_filled(kc, 15.0, Color32::from_gray(0x55));
        p.circle_stroke(kc, 15.0, Stroke::new(1.0, Color32::from_gray(0x22)));
        let ang = std::f32::consts::PI * (0.5 + 0.75 * pan) + std::f32::consts::PI;
        let tip = kc + vec2(ang.cos(), ang.sin()) * 11.0;
        p.line_segment([kc, tip], Stroke::new(2.0, Color32::WHITE));
        if kresp.drag_started() || kresp.double_clicked() {
            mixstate::checkpoint(app, "Pan");
        }
        if kresp.dragged() {
            pan = (pan - kresp.drag_delta().y / 120.0).clamp(-1.0, 1.0);
            out = Some(());
        }
        if kresp.double_clicked() || (kresp.clicked() && ui.input(|i| i.modifiers.alt)) {
            pan = 0.0;
            out = Some(());
        }
        y += 40.0;
    } else if !is_vca {
        y += 40.0;
    }
    // the VCA slot (shown once there are VCAs)
    if !app.mix.vcas.is_empty() && shown(app, "VCA") && i < VCA {
        let v = app.mix.vca_of.get(&i).copied();
        slot(&p, y, &v.and_then(|v| app.mix.vcas.get(v)).map_or("VCA".into(), |x| x.name.clone()), v.map(|_| Color32::from_rgb(0x8a, 0x5a, 0x2a)));
        let resp = ui.interact(slot_rect(y), id.with("vca"), Sense::click());
        egui::Popup::menu(&resp).show(|ui| {
            let mut pick: Option<Option<usize>> = None;
            if ui.add(egui::Button::selectable(v.is_none(), "No VCA")).clicked() {
                pick = Some(None);
            }
            ui.separator();
            for (k, x) in app.mix.vcas.iter().enumerate() {
                if ui.add(egui::Button::selectable(v == Some(k), &x.name)).clicked() {
                    pick = Some(Some(k));
                }
            }
            if ui.button("New VCA").clicked() {
                let n = mixstate::new_vca(app, &[]);
                pick = Some(Some(n));
            }
            if let Some(c) = pick {
                mixstate::checkpoint(app, "VCA");
                match c {
                    Some(k) => app.mix.vca_of.insert(i, k),
                    None => app.mix.vca_of.remove(&i),
                };
                app.load_engine();
                ui.close();
            }
        });
        y += 24.0;
    }
    // readouts: volume, peak
    let rv = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(36.0, 18.0));
    let rpk = Rect::from_min_size(pos2(r.max.x - 44.0, y), vec2(36.0, 18.0));
    p.rect_filled(rv, 3.0, look::lcd_bg());
    p.rect_filled(rpk, 3.0, look::lcd_bg());
    p.text(rv.center(), Align2::CENTER_CENTER, if vol <= -90.0 { "-∞".into() } else { format!("{vol:.1}") }, look::regular(10.5), look::lcd_text());
    let peak_db = if meter <= 1e-4 { -90.0 } else { 20.0 * meter.log10() };
    p.text(rpk.center(), Align2::CENTER_CENTER, if peak_db <= -89.0 { "-∞".into() } else { format!("{peak_db:.1}") }, look::regular(10.5), Color32::from_rgb(0x4c, 0xd9, 0x64));
    y += 26.0;
    // the fader and meter (Sends on Faders: the send's level, greyed where there's none)
    let bottom_room = 62.0;
    let fh = (r.max.y - bottom_room - y).max(40.0);
    let sof = app.sends_on_faders.filter(|_| is_track || aux.is_some());
    let has_send = sof.is_some_and(|b| app.sends.get(&i).is_some_and(|v| v.iter().any(|s| s.bus == b)));
    let fader_live = sof.is_none() || has_send;
    let track = Rect::from_min_size(pos2(r.center().x - 14.0, y), vec2(6.0, fh));
    p.rect_filled(track, 3.0, look::lcd_bg());
    let meter_r = Rect::from_min_size(pos2(r.center().x + 8.0, y), vec2(8.0, fh));
    p.rect_filled(meter_r, 2.0, look::lcd_bg());
    let lv = meter.clamp(0.0, 1.0).sqrt();
    let surround_master = is_master && crate::surround::active(app);
    if surround_master {
        crate::surround_ui::master_meters(app, ui, &p, Rect::from_min_max(pos2(meter_r.min.x, y), pos2(r.max.x - 6.0, y + fh)));
    } else if lv > 0.01 && !is_vca {
        let mr = Rect::from_min_max(pos2(meter_r.min.x + 1.0, meter_r.max.y - lv * fh), pos2(meter_r.max.x - 1.0, meter_r.max.y));
        p.rect_filled(mr, 1.0, if meter > 0.98 { look::record_red() } else { Color32::from_rgb(0x4c, 0xd9, 0x64) });
    }
    // the scale between fader and meter, as Logic's: 0 at the top mark, down to 60
    if r.width() >= 80.0 && !surround_master {
        for db in [0.0f32, 3.0, 6.0, 9.0, 12.0, 15.0, 18.0, 21.0, 24.0, 30.0, 35.0, 40.0, 45.0, 50.0, 60.0] {
            let yy = track.max.y - db_to_frac(-db) * fh;
            p.hline(meter_r.max.x + 2.0..=meter_r.max.x + 5.0, yy, Stroke::new(1.0, look::text_dim()));
            p.text(pos2(meter_r.max.x + 7.0, yy), Align2::LEFT_CENTER, format!("{db:.0}"), look::regular(8.0), look::text_dim());
        }
    }
    let fy = track.max.y - db_to_frac(vol) * fh;
    let cap = Rect::from_center_size(pos2(track.center().x, fy), vec2(26.0, 14.0));
    let cap_fill = if !fader_live { Color32::from_gray(0x60) } else if sof.is_some() { Color32::from_rgb(0x9c, 0xc8, 0xf0) } else if is_vca { Color32::from_rgb(0xe0, 0xa8, 0x60) } else { Color32::from_gray(0xc8) };
    p.rect_filled(cap, 3.0, cap_fill);
    p.hline(cap.x_range(), cap.center().y, Stroke::new(1.0, Color32::from_gray(0x40)));
    if fader_live {
        let fresp = ui.interact(Rect::from_min_max(pos2(track.min.x - 12.0, y - 8.0), pos2(track.max.x + 12.0, y + fh + 8.0)), id.with("fader"), Sense::click_and_drag()).on_hover_text(if sof.is_some() { "Send level (double-click: 0 dB)" } else { "Volume (double-click: 0 dB)" });
        if (fresp.drag_started() || fresp.double_clicked()) && !is_master {
            mixstate::checkpoint(app, if sof.is_some() { "Send Level" } else { "Volume" });
        }
        if fresp.dragged() {
            let f = ((track.max.y - fresp.interact_pointer_pos().map_or(fy, |pp| pp.y)) / fh).clamp(0.0, 1.0);
            vol = frac_to_db(f).min(6.0);
            out = Some(());
        }
        if fresp.double_clicked() || (fresp.clicked() && ui.input(|i| i.modifiers.alt)) {
            vol = 0.0;
            out = Some(());
        }
    }
    // Mute, Solo
    let by = r.max.y - bottom_room + 8.0;
    let mr = Rect::from_min_size(pos2(r.center().x - 30.0, by), vec2(26.0, 20.0));
    let sr = Rect::from_min_size(pos2(r.center().x + 4.0, by), vec2(26.0, 20.0));
    if kind.is_some() || aux.is_some() || is_vca {
        for (rr, label, on, col, which) in [(mr, "M", mute, Color32::from_rgb(0x4f, 0xa8, 0xe8), 0), (sr, "S", solo, Color32::from_rgb(0xe8, 0xc8, 0x3c), 1)] {
            p.rect_filled(rr, 4.0, if on { col } else { look::group_bg() });
            p.text(rr.center(), Align2::CENTER_CENTER, label, look::bold(11.0), if on { Color32::BLACK } else { look::text() });
            if ui.interact(rr, id.with(("ms", which)), Sense::click()).clicked() {
                mixstate::checkpoint(app, if which == 0 { "Mute" } else { "Solo" });
                if which == 0 {
                    mute = !mute;
                } else {
                    solo = !solo;
                }
                out = Some(());
            }
        }
    }
    // the name: click selects (Shift / ⌘: adds), right-click: Logic's strip name menu
    if shown(app, "part:Track Name") {
        let nr = Rect::from_min_size(pos2(r.min.x + 4.0, r.max.y - 26.0), vec2(r.width() - 8.0, 20.0));
        let c = if is_vca { Color32::from_rgb(0xe0, 0xa8, 0x60) } else if aux.is_some() { Color32::from_rgb(0x5a, 0x9a, 0xd8) } else { kind.map_or(look::text_dim(), look::region_color) };
        p.rect_filled(nr, 4.0, c.gamma_multiply(0.35));
        p.with_clip_rect(nr).text(nr.center(), Align2::CENTER_CENTER, name, look::regular(11.0), look::text());
        let nresp = ui.interact(nr, id.with("stripname"), Sense::click());
        if nresp.clicked() {
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            mixstate::select(app, i, add);
        }
        nresp.context_menu(|ui| {
            ui.set_min_width(200.0);
            if ui.button("Create New VCA").clicked() {
                if !app.mix.selected.contains(&i) {
                    app.mix.selected = [i].into_iter().collect();
                }
                crate::mixercmd::run(app, &ui.ctx().clone(), "Create New VCA");
                ui.close();
            }
            ui.add_enabled(false, egui::Button::new("Assign Channel Strip Color…"));
            ui.add_enabled(false, egui::Button::new("Rename Channel Strip…"));
            ui.separator();
            let comps = crate::menus::context("mixer-strip-name").iter().find(|x| x.label == "Channel Strip Components").map_or(&[][..], |x| x.children.as_slice());
            ui.menu_button("Channel Strip Components", |ui| crate::menus::show_items(app, ui, comps));
        });
    }
    out.map(|_| (vol, pan, mute, solo))
}

/// The Group Settings window (Logic's: the group's name, Enabled, and what it links).
pub fn group_window(app: &mut App, ctx: &egui::Context) {
    let Some(g) = app.mix.group_window else { return };
    if g >= app.mix.groups.len() {
        app.mix.group_window = None;
        return;
    }
    let mut open = true;
    let before = app.mix.groups[g].clone();
    let mut grp = before.clone();
    let labels: Vec<String> = app.mix.groups.iter().enumerate().map(|(k, x)| x.label(k)).collect();
    let mut pick = None;
    egui::Window::new("Group Settings").open(&mut open).collapsible(false).resizable(false).default_width(300.0).show(ctx, |ui| {
        egui::ComboBox::from_id_salt("group-pick").selected_text(&labels[g]).show_ui(ui, |ui| {
            for (k, l) in labels.iter().enumerate() {
                if ui.selectable_label(k == g, l).clicked() {
                    pick = Some(k);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut grp.name);
        });
        ui.checkbox(&mut grp.on, "Enabled");
        ui.separator();
        ui.label(egui::RichText::new("Settings").strong());
        for (label, l) in [("Editing (Selection)", mixstate::L_SELECT), ("Track Zoom", (2usize, 0x02u8)), ("Hide Track (H)", mixstate::L_HIDE), ("Record Enable", mixstate::L_RECORD), ("Input Monitoring", mixstate::L_INPUT)] {
            let mut on = grp.links(l);
            if ui.checkbox(&mut on, label).changed() {
                grp.set(l, on);
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Channel Strip").strong());
        for (label, l) in [("Volume", mixstate::L_VOLUME), ("Mute", mixstate::L_MUTE), ("Pan", mixstate::L_PAN), ("Solo", mixstate::L_SOLO), ("Automation Mode", mixstate::L_AUTOMATION), ("Color", mixstate::L_COLOR)] {
            let mut on = grp.links(l);
            if ui.checkbox(&mut on, label).changed() {
                grp.set(l, on);
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Sends:");
            for k in 0..8 {
                let mut on = grp.links_send(k);
                if ui.checkbox(&mut on, format!("{}", k + 1)).changed() {
                    if on { grp.mask[1] |= 1 << k } else { grp.mask[1] &= !(1 << k) }
                }
            }
        });
    });
    if grp != before {
        mixstate::checkpoint(app, "Group Settings");
        app.mix.groups[g] = grp;
        app.dirty = true;
    }
    if let Some(k) = pick {
        app.mix.group_window = Some(k);
    }
    if !open {
        app.mix.group_window = None;
    }
}
