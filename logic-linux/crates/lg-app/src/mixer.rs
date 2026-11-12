//! The Mixer (X): one channel strip per track, aux, VCA and the Stereo Out, as Logic's: the
//! Setting button, the gain reduction meter, the EQ thumbnail, MIDI FX, the input / instrument
//! slot, the Audio FX (click: the plug-in's window, hover: bypass, the arrow: the plug-in menu,
//! drag: move, Option-drag: copy), the sends, the output, the group, the automation mode, the
//! pan knob, the VCA slot, volume and peak readouts, the fader with its meter, Mute / Solo and
//! the name (click: select, Shift / ⌘-click: add). Faders drag (Option-click / double-click:
//! 0 dB), knobs drag up and down. The bar: Edit / Options / View, Sends on Faders, the filter
//! buttons (Audio … MIDI, in All), Single | Tracks | All. Edits go on the Mixer Undo list;
//! groups follow (mixstate).

/// Low Latency Monitoring's orange text on bypassed slots.
const LL_ORANGE: Color32 = Color32::from_rgb(0xf0, 0x9a, 0x2c);

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

const KINDS: [(Kind, &str); 8] = [(Kind::Audio, "Audio"), (Kind::Inst, "Inst"), (Kind::Aux, "Aux"), (Kind::Bus, "Bus"), (Kind::Input, "Input"), (Kind::Output, "Output"), (Kind::MasterVca, "Master/VCA"), (Kind::Midi, "MIDI")];
const VIEWS: [(MixerView, &str); 3] = [(MixerView::Single, "Single"), (MixerView::Tracks, "Tracks"), (MixerView::All, "All")];

fn sof_text(app: &App) -> String {
    match app.sends_on_faders {
        Some(b) => format!("Sends on Faders: Bus {}", b + 1),
        None => "Sends on Faders: Off".into(),
    }
}

fn sof_items(app: &mut App, ui: &mut egui::Ui) {
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
}

fn toggle_kind(app: &mut App, ui: &egui::Ui, k: Kind) {
    // (Option-click: only this type; again: all of them)
    if ui.input(|i| i.modifiers.alt) {
        crate::mixext::solo_kind(app, k);
    } else {
        crate::mixercmd::toggle_kind(app, k);
    }
}

/// The Mixer bar: Edit / Options / View, then Sends on Faders, Single | Tracks | All, the
/// channel-type filters and the strip width, as many as fit (the rest in "…", Logic's order).
fn bar_controls(app: &mut App, ui: &mut egui::Ui, bar: Rect) {
    use crate::barfit::{cell, fit, more_button, text_popup, text_w, Slot, GAP, MORE_W};
    let id = ui.scope_id().with("mixer-bar");
    let cy = bar.center().y;
    let mut end = bar.min.x + 10.0;
    ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(bar.min.x + 14.0, bar.min.y + 3.0), pos2(bar.min.x + 320.0, bar.max.y - 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "mixer-Edit"), ("Options", "mixer-Options"), ("View", "mixer-View")]);
        end = ui.min_rect().max.x - 8.0;
    });
    let sof = sof_text(app);
    let kinds_w: Vec<f32> = KINDS.iter().map(|(_, n)| text_w(ui, n) + 14.0).collect();
    let widths = [GAP + text_w(ui, &sof) + 32.0, GAP + 186.0, GAP + kinds_w.iter().sum::<f32>() + 2.0 * KINDS.len() as f32, GAP + 36.0];
    let shown = fit(&widths.iter().map(|&w| Slot { w, drop: 1 }).collect::<Vec<_>>(), bar.max.x - 10.0 - end, MORE_W + GAP);
    let mut x = end;
    let p = ui.painter().clone();
    if shown[0] {
        let r = cell(cy, x + GAP, widths[0] - GAP);
        let resp = text_popup(ui, r, id.with("sof"), &sof, "Sends on Faders");
        egui::Popup::menu(&resp).show(|ui| sof_items(app, ui));
        x += widths[0];
    }
    if shown[1] {
        let seg = cell(cy, x + GAP, 186.0);
        p.rect_filled(seg, 6.0, look::group_bg());
        for (k, (v, name)) in VIEWS.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(seg.min.x + k as f32 * 62.0, seg.min.y), vec2(62.0, seg.height()));
            let on = app.mixer_view == v;
            if on {
                p.rect_filled(r, 6.0, look::accent());
            }
            p.text(r.center(), Align2::CENTER_CENTER, name, look::regular(13.0), if on { Color32::WHITE } else { look::text() });
            if ui.interact(r, id.with(("mixview", k)), Sense::click()).clicked() {
                app.mixer_view = v;
            }
        }
        x += widths[1];
    }
    if shown[2] {
        let mut kx = x + GAP;
        for (i, (k, label)) in KINDS.into_iter().enumerate() {
            let r = cell(cy, kx, kinds_w[i]);
            kx += kinds_w[i] + 2.0;
            let on = !app.mix.hidden.contains(&k);
            let resp = ui.interact(r, id.with(("kind", i)), Sense::click()).on_hover_text("Click: show or hide; Option-click: only this type");
            p.rect_filled(r, 6.0, if on { look::button_on() } else { look::group_bg() });
            p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(12.0), if on { Color32::WHITE } else { look::text_dim() });
            if resp.clicked() {
                toggle_kind(app, ui, k);
            }
        }
        x += widths[2];
    }
    if shown[3] {
        let r = cell(cy, x + GAP, 36.0);
        let narrow = app.mix.narrow;
        if crate::barfit::icon_button(ui, r, id.with("width"), narrow, true, if narrow { "Wide Channel Strips" } else { "Narrow Channel Strips" }, crate::glyphs::ph::ARROWS_IN_LINE_HORIZONTAL).clicked() {
            app.mix.narrow = !narrow;
        }
    }
    if shown.iter().any(|s| !s) {
        let mr = cell(cy, bar.max.x - 10.0 - MORE_W, MORE_W);
        let resp = more_button(ui, mr, id.with("more"));
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            if !shown[0] {
                ui.menu_button("Sends on Faders", |ui| sof_items(app, ui));
            }
            if !shown[1] {
                ui.separator();
                for (v, name) in VIEWS {
                    if ui.add(egui::Button::selectable(app.mixer_view == v, name)).clicked() {
                        app.mixer_view = v;
                        ui.close();
                    }
                }
            }
            if !shown[2] {
                ui.separator();
                for (k, label) in KINDS {
                    if ui.add(egui::Button::selectable(!app.mix.hidden.contains(&k), label)).clicked() {
                        toggle_kind(app, ui, k);
                    }
                }
            }
            if !shown[3] {
                ui.separator();
                for (n, label) in [(true, "Narrow Channel Strips"), (false, "Wide Channel Strips")] {
                    if ui.add(egui::Button::selectable(app.mix.narrow == n, label)).clicked() {
                        app.mix.narrow = n;
                        ui.close();
                    }
                }
            }
        });
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(proj) = app.project.clone() else { return };
    app.mix.slot_rects.clear();
    // Logic's Mixer bar: Edit / Options / View, Sends on Faders, the filters, Single | Tracks | All
    // (40 pt as Logic's local bars; laid out by its width: what doesn't fit is in "…", in the
    // order of Logic's Mixer "…" menu; barfit.rs)
    let bar = Rect::from_min_size(area.min, vec2(area.width(), 40.0));
    p.rect_filled(bar, 0.0, look::header_bg());
    p.hline(bar.x_range(), bar.max.y, Stroke::new(1.0, look::line()));
    bar_controls(app, ui, bar);
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
    // (Follow Hide, Follow Track Stacks, Channels with Sends only…: mixext.rs)
    crate::mixext::filter_strips(app, &mut strips);
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
    // (an External MIDI track: a MIDI channel strip)
    if t.extra.set.external.is_some() && !crate::utilwin::ext_instrument(app, i) {
        crate::mixwin::midi_strip(app, ui, r, i);
        return;
    }
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
    // control surface bars: a bar in the surface's colour on the strips it controls
    if crate::mixext::comp(app, "Control Surface Bars") {
        if let Some(c) = crate::surfaces::controlled_tracks(app).get(&i) {
            let c = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8);
            ui.painter().rect_filled(Rect::from_min_size(pos2(r.min.x + 2.0, r.max.y - 4.0), vec2(r.width() - 4.0, 3.0)), 1.0, c);
        }
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
    // (the search field at the menu's top: mixext.rs)
    if let Some(n) = crate::mixext::plugin_search(ui) {
        picked = Some(Some(n));
    }
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
    // Option-click: the Legacy submenu (Logic's older plug-ins: legacy.rs in lg-fx)
    if ui.input(|i| i.modifiers.alt) || ui.data(|d| d.get_temp::<bool>(egui::Id::unique("fx-menu-legacy"))).unwrap_or(false) {
        ui.data_mut(|d| d.insert_temp(egui::Id::unique("fx-menu-legacy"), true));
        ui.separator();
        ui.menu_button("Legacy", |ui| {
            for n in lg_fx::legacy::NAMES {
                if ui.add(egui::Button::selectable(current == Some(*n), *n)).clicked() {
                    picked = Some(Some(n.to_string()));
                    ui.close();
                }
            }
        });
    }
    if picked.is_some() {
        ui.data_mut(|d| d.remove::<bool>(egui::Id::unique("fx-menu-legacy")));
    }
    picked
}

fn bus_menu(ui: &mut egui::Ui, app: &App, current: Option<usize>) -> Option<usize> {
    // (with a search field, I/O labels and the aux strips' names: mixext.rs)
    crate::mixext::bus_menu(ui, app, current)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn strip(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize, name: &str, kind: Option<TrackKind>, vals: (f32, f32, bool, bool), meter: f32, selected: bool) -> Option<(f32, f32, bool, bool)> {
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, if selected { look::track_selected() } else { look::header_bg() });
    crate::mixext::strip_bg(app, ui, r, i);
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
    y = crate::mixext::top_rows(app, ui, &p, r, y, i);
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
            // (the saved settings, Next / Previous, Save as, performances: mixwin.rs)
            crate::mixwin::setting_menu(app, ui, i);
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
        let gresp = ui.interact(gm.expand2(vec2(0.0, 2.0)), id.with("gr"), Sense::click()).on_hover_text(format!("Gain Reduction: {gr:.1} dB"));
        crate::mixext::gr_clicked(app, ui, &gresp, i);
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
        let eresp = ui.interact(eq_r, id.with("eqslot"), Sense::click()).on_hover_text("Channel EQ (Shift-click: Linear Phase EQ; Command-click: remove)");
        crate::mixext::eq_clicked(app, ui, &eresp, i);
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
                crate::glyphs::paint(&p, ir, crate::glyphs::ph::INTERSECT, 14.0, look::text_dim());
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
            // (Low Latency Monitoring's bypass: grey with orange text; it shows once the engine runs)
            let ll = !u.bypass && app.engine.lock().ok().is_some_and(|e| e.ext.ll_bypassed.get(i).and_then(|v| v.get(k).copied()).unwrap_or(false));
            let fill = if u.bypass || ll { Color32::from_gray(0x55) } else { Color32::from_rgb(0x1f, 0x5f, 0xc8) };
            p.rect_filled(sr, 4.0, fill);
            // a side chain: a left-pointing arrow after the name (Logic's ◂), its source in the tip
            let side = app.side.by_slot.get(&(i, k)).copied();
            let shown_name = if side.is_some() { format!("{} ◂", u.name) } else { u.name.clone() };
            p.with_clip_rect(sr.shrink(2.0)).text(sr.center(), Align2::CENTER_CENTER, &shown_name, look::regular(11.0), if ll { LL_ORANGE } else if u.bypass { look::text_dim() } else { Color32::WHITE });
            let hover = ui.rect_contains_pointer(sr);
            let bypass_r = Rect::from_min_size(sr.min, vec2(14.0, sr.height()));
            let arrow_r = Rect::from_min_max(pos2(sr.max.x - 14.0, sr.min.y), sr.max);
            let body_r = Rect::from_min_max(pos2(bypass_r.max.x, sr.min.y), pos2(arrow_r.min.x, sr.max.y));
            if hover {
                // the power button and the menu arrow appear on hover, as Logic's
                p.rect_filled(bypass_r.shrink(1.0), 3.0, fill);
                crate::glyphs::paint(&p, bypass_r, crate::glyphs::ph::POWER, 11.0, if u.bypass { look::text_dim() } else { Color32::WHITE });
                p.rect_filled(arrow_r.shrink(1.0), 3.0, fill);
                crate::glyphs::paint(&p, arrow_r, crate::glyphs::ph::CARET_DOWN, 10.0, Color32::WHITE);
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
            let body = ui.interact(body_r, id.with(("fxslot", k)), Sense::click_and_drag()).on_hover_text(match side {
                Some(c) => format!("{}\nSide Chain: {}", crate::hosting::slot_tip(app, i, k, &u.name), crate::sidechain::label_of(app, c)),
                None => crate::hosting::slot_tip(app, i, k, &u.name),
            });
            if body.clicked() {
                if ui.input(|i| i.modifiers.command) {
                    // Command-click: the plug-in goes
                    crate::mixext::remove_insert(app, i, k);
                } else if ui.input(|i| i.modifiers.alt) {
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
                    Some(n) => {
                        // Channel EQ and Linear Phase EQ share their parameters: swapping one
                        // for the other keeps the setting
                        let eqs = ["Channel EQ", "Linear Phase EQ"];
                        let keep = (eqs.contains(&v[k].name.as_str()) && eqs.contains(&n.as_str())).then(|| (v[k].params.clone(), v[k].preset.clone()));
                        v[k] = mixstate::new_plugin(&n);
                        if let Some((params, preset)) = keep {
                            v[k].params = params;
                            v[k].preset = preset;
                        }
                    }
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
            // (a send Low Latency Monitoring turns off: grey with orange text)
            let ll = app.engine.lock().ok().is_some_and(|e| e.ext.low_latency && e.ext.tracks.get(i).is_some_and(|s| s.low_latency && !s.sends.get(k).copied().unwrap_or_default().low_latency_safe));
            p.rect_filled(sr, 4.0, if ll { Color32::from_gray(0x55) } else if on_faders { Color32::from_rgb(0x3c, 0x7c, 0xb0) } else { Color32::from_rgb(0x2a, 0x5a, 0x86) });
            p.with_clip_rect(sr.shrink(2.0)).text(sr.center(), Align2::CENTER_CENTER, crate::mixext::send_label(app, snd.bus), look::regular(11.0), if ll { LL_ORANGE } else { Color32::WHITE });
            let kc = pos2(r.max.x - 16.0, y + 10.0);
            let kr = ui.interact(Rect::from_center_size(kc, vec2(18.0, 18.0)), id.with(("send", k)), Sense::click_and_drag()).on_hover_text(format!("Send to Bus {}: {:.1} dB{}", snd.bus + 1, snd.level_db, if snd.pre { " (pre fader)" } else { "" }));
            p.circle_filled(kc, 8.0, Color32::from_gray(0x55));
            let f = db_to_frac(snd.level_db);
            let ang = std::f32::consts::PI * (0.75 + 1.5 * f);
            p.line_segment([kc, kc + vec2(ang.cos(), ang.sin()) * 6.5], Stroke::new(1.6, Color32::WHITE));
            crate::mixext::send_ring(app, &p, kc, i, k, snd.pre);
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
            // (Shift-click: its destination flashes; Command-click: the send goes)
            if crate::mixext::send_click(app, ui, &sresp, i, k) {
                y += 22.0;
                continue;
            }
            egui::Popup::menu(&sresp).show(|ui| {
                ui.set_min_width(160.0);
                let mut edit: Option<Box<dyn FnOnce(&mut Vec<crate::app::SendSlot>)>> = None;
                // (Independent Pan, Post Pan / Post Fader / Pre Fader, Copy Fader to Send…)
                crate::mixext::send_menu(app, ui, i, k);
                if ui.button("No Send").clicked() {
                    mixstate::checkpoint(app, "Send");
                    crate::mixext::remove_send(app, i, k);
                    ui.close();
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
                    crate::mixext::add_send_selected(app, i, b);
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
                    crate::mixext::set_output_selected(app, i, v);
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
            let label = crate::mixext::group_label(app, i).unwrap_or_else(|| "Group".into());
            slot(&p, y, &label, g.map(|_| crate::mixext::group_color(app)));
            let resp = ui.interact(slot_rect(y), id.with("group"), Sense::click());
            // (Option-click: the groups assigned last)
            let resp = if resp.clicked() && ui.input(|x| x.modifiers.alt) {
                mixstate::checkpoint(app, "Group");
                let last = app.tm.mx.last_groups.clone();
                crate::mixext::set_groups(app, i, &last);
                ui.interact(Rect::NOTHING, id.with("group-none"), Sense::hover())
            } else {
                resp
            };
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
                    if ui.add(egui::Button::selectable(crate::mixext::groups_of(app, i).contains(&k), grp.label(k))).clicked() {
                        pick = Some(Some(k));
                    }
                }
                let n = app.mix.groups.len();
                // (a project has 64 groups at most)
                if n < 64 && ui.button(format!("Group {}: (new)", n + 1)).clicked() {
                    pick = Some(Some(n));
                }
                if let Some(choice) = pick {
                    mixstate::checkpoint(app, "Group");
                    match choice {
                        Some(k) => {
                            if k == app.mix.groups.len() {
                                let mut ng = mixstate::Group::new();
                                if let Some(m) = app.tm.mx.group_default {
                                    ng.mask = m;
                                }
                                app.mix.groups.push(ng);
                                app.mix.group_window = Some(k);
                            }
                            // (Shift: one more group; a group the strip is in: out of it)
                            crate::mixext::choose_group(app, i, k, ui.input(|x| x.modifiers.shift));
                        }
                        None => crate::mixext::set_groups(app, i, &[]),
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
            let label = if is_track { crate::mixext::auto_mode_label(app, i) } else { format!("{mode:?}") };
            slot(&p, y, &label, on.then(|| Color32::from_rgb(0x3a, 0x6e, 0x3a)));
            let aresp = ui.interact(slot_rect(y), id.with("automode"), Sense::click());
            crate::mixext::auto_mode_menu(app, &aresp, i);
            y += 26.0;
        }
    }
    // the pan knob (a Surround Panner / 3D Object Panner on strips routed to surround)
    if !is_vca && !is_master && matches!(mixstate::output_of(app, i), Out::Surround | Out::Object) {
        crate::surround_ui::pan_control(app, ui, &p, r, y, i);
        y += 40.0;
    } else if shown(app, "Pan Knob") && !is_vca && !is_master {
        // (Pan / Balance, Stereo Pan, the Binaural Panner, the Channel Mode button: mixext.rs)
        if crate::mixext::pan_control(app, ui, &p, r, y, i, &mut pan) {
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
    // (the held peak, its clipping colours, a click resets; typing a fader value: mixext.rs)
    if !is_vca {
        crate::mixext::peak_display(app, ui, &p, rpk, i);
    }
    if crate::mixext::typed_volume(app, ui, rv, i, &mut vol) {
        out = Some(());
    }
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
        crate::mixext::meter(app, &p, meter_r.shrink2(vec2(1.0, 0.0)), i, meter);
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
            if ui.input(|i| i.modifiers.shift) {
                // Shift: finer steps
                vol = crate::mixext::fine_fader(vol, fresp.drag_delta().y, fh);
            } else {
                let f = ((track.max.y - fresp.interact_pointer_pos().map_or(fy, |pp| pp.y)) / fh).clamp(0.0, 1.0);
                vol = frac_to_db(f).min(6.0);
            }
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
        // (slide over them, Command-click all, Option-click exclusive, solo-safe: mixext.rs)
        if crate::mixext::ms_buttons(app, ui, &p, mr, sr, i, &mut mute, &mut solo) {
            out = Some(());
        }
    }
    crate::mixext::strip_buttons(app, ui, &p, r, by, mr, sr, i, kind);
    // the name: click selects (Shift / ⌘: adds), right-click: Logic's strip name menu
    if shown(app, "part:Track Name") {
        let nr = Rect::from_min_size(pos2(r.min.x + 4.0, r.max.y - 26.0), vec2(r.width() - 8.0, 20.0));
        let c = if is_vca { Color32::from_rgb(0xe0, 0xa8, 0x60) } else if aux.is_some() { Color32::from_rgb(0x5a, 0x9a, 0xd8) } else { kind.map_or(look::text_dim(), look::region_color) };
        // (the colour, two lines, renaming by double-click, dragging to reorder: mixext.rs)
        let nresp = crate::mixext::name_field(app, ui, &p, nr, i, name, c);
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
            for c in ["Assign Channel Strip Color…", "Rename Channel Strip…"] {
                if ui.button(c).clicked() {
                    app.mix.selected = [i].into_iter().collect();
                    crate::mixext::command(app, &ui.ctx().clone(), c);
                    ui.close();
                }
            }
            ui.separator();
            if let Some(c) = crate::menus::context("mixer-strip-name").iter().find(|x| x.label == "Channel Strip Components").cloned() {
                crate::mixext::components_menu(app, ui, &c);
            }
            if ui.button("Configure Channel Strip Components…").clicked() {
                app.tm.mx.win.components = true;
                ui.close();
            }
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
    let mut more: Option<&str> = None;
    egui::Window::new("Group Settings").open(&mut open).collapsible(false).resizable(false).default_width(300.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("group-pick").selected_text(&labels[g]).show_ui(ui, |ui| {
                for (k, l) in labels.iter().enumerate() {
                    if ui.selectable_label(k == g, l).clicked() {
                        pick = Some(k);
                    }
                }
            });
            // the More menu at the upper right: the group commands
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("···", |ui| {
                    for c in ["Copy Group Settings", "Paste Group Settings", "Add Selected Channel Strips to Selected Groups", "Remove Selected Channel Strips from Selected Groups", "Select Members of Selected Groups", "Delete Unused Groups"] {
                        if ui.button(c).clicked() {
                            more = Some(c);
                            ui.close();
                        }
                    }
                });
            });
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
    if let Some(c) = more {
        crate::mixercmd::run(app, ctx, c);
    }
    if !open {
        app.mix.group_window = None;
    }
}
