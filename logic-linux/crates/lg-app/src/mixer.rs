//! The Mixer (X): one channel strip per track and the Stereo Out, as Logic's: the settings and
//! EQ buttons, the instrument slot, Audio FX, the pan knob, volume and peak readouts, the fader
//! with its meter, Mute / Solo and the name at the bottom. Faders drag (Option-click / double-
//! click: 0 dB), the pan knob drags up and down.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::TrackKind;

use crate::app::{App, MixerView};
use crate::look;
use crate::tracks::{db_to_frac, frac_to_db};

const STRIP_W: f32 = 92.0;
/// Strip indexes from here up are aux strips (AUX + the aux's index).
pub const AUX: usize = 1_000_000;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(proj) = app.project.clone() else { return };
    // Logic's Mixer bar: Edit / Options / View, Sends on Faders, Single | Tracks | All
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
            let buses: Vec<usize> = app.auxes.iter().map(|a| a.bus).collect();
            for b in buses {
                if ui.add(egui::Button::selectable(app.sends_on_faders == Some(b), format!("Bus {}", b + 1))).clicked() {
                    app.sends_on_faders = Some(b);
                    ui.close();
                }
            }
        });
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
    // which strips: Single = the selected track and what it feeds; Tracks = the tracks, their
    // auxes and the output; All = every channel strip
    let tracks: Vec<usize> = match app.mixer_view {
        MixerView::Single => app.selected_track.into_iter().filter(|t| *t < proj.tracks.len()).collect(),
        _ => (0..proj.tracks.len()).collect(),
    };
    let auxes: Vec<usize> = match app.mixer_view {
        MixerView::Single => {
            let used: Vec<usize> = tracks.iter().flat_map(|t| app.sends.get(t).into_iter().flatten().map(|s| s.bus)).collect();
            (0..app.auxes.len()).filter(|a| used.contains(&app.auxes[*a].bus)).collect()
        }
        _ => (0..app.auxes.len()).collect(),
    };
    let mut x = body.min.x + 8.0;
    let room = body.max.x - STRIP_W - 16.0;
    for i in tracks {
        if x + STRIP_W > room {
            break;
        }
        let r = Rect::from_min_size(pos2(x, body.min.y + 6.0), vec2(STRIP_W, body.height() - 10.0));
        track_strip(app, ui, r, i);
        x += STRIP_W + 2.0;
    }
    if !auxes.is_empty() {
        x += 8.0;
        p.vline(x - 5.0, body.min.y + 8.0..=body.max.y - 8.0, Stroke::new(1.0, look::line()));
    }
    for a in auxes {
        if x + STRIP_W > room {
            break;
        }
        let r = Rect::from_min_size(pos2(x, body.min.y + 6.0), vec2(STRIP_W, body.height() - 10.0));
        aux_strip(app, ui, r, a);
        x += STRIP_W + 2.0;
    }
    // the Stereo Out at the right
    let r = Rect::from_min_size(pos2(body.max.x - STRIP_W - 8.0, body.min.y + 6.0), vec2(STRIP_W, body.height() - 10.0));
    if r.min.x > x - 2.0 {
        output_strip(app, ui, r);
    }
}

/// An aux strip (its bus in, its effects, its fader).
pub fn aux_strip(app: &mut App, ui: &mut egui::Ui, r: Rect, a: usize) {
    let Some(aux) = app.auxes.get(a).cloned() else { return };
    let meter = app.engine.lock().map(|e| e.aux_meters.get(a).copied().unwrap_or(0.0)).unwrap_or(0.0);
    if let Some((v, pan, m, s)) = strip(app, ui, r, AUX + a, &aux.name, None, aux.volume_db, aux.pan, aux.mute, aux.solo, meter, false) {
        if let Some(x) = app.auxes.get_mut(a) {
            x.volume_db = v;
            x.pan = pan;
            x.mute = m;
            x.solo = s;
        }
        app.dirty = true;
        app.load_engine();
    }
}

/// A track's channel strip (the Mixer's, and the Inspector's left one).
pub fn track_strip(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize) {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(i)).cloned() else { return };
    let meter = app.engine.lock().map(|e| e.meters.get(i).copied().unwrap_or(0.0)).unwrap_or(0.0);
    if let Some((v, pan, m, s)) = strip(app, ui, r, i, &t.name, Some(t.kind), t.volume_db, t.pan, t.mute, t.solo, meter, app.selected_track == Some(i)) {
        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)) {
            tr.volume_db = v;
            tr.pan = pan;
            tr.mute = m;
            tr.solo = s;
        }
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

/// The Stereo Out strip (the master level).
pub fn output_strip(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let master = app.engine.lock().map(|e| e.master_meter).unwrap_or(0.0);
    let master_db = 20.0 * app.engine.lock().map(|e| e.master).unwrap_or(1.0).max(1e-5).log10();
    if let Some((v, ..)) = strip(app, ui, r, usize::MAX, "Stereo Out", None, master_db, 0.0, false, false, master, false) {
        if let Ok(mut e) = app.engine.lock() {
            e.master = crate::app::db_to_gain(v);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn strip(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize, name: &str, kind: Option<TrackKind>, vol: f32, pan: f32, mute: bool, solo: bool, meter: f32, selected: bool) -> Option<(f32, f32, bool, bool)> {
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, if selected { look::track_selected() } else { look::header_bg() });
    let mut out = None;
    let (mut vol, mut pan, mut mute, mut solo) = (vol, pan, mute, solo);
    let slot = |p: &egui::Painter, y: f32, text: &str, fill: Option<Color32>| {
        let s = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
        p.rect_filled(s, 4.0, fill.unwrap_or(look::group_bg()));
        p.text(s.center(), Align2::CENTER_CENTER, text, look::regular(11.0), if fill.is_some() { Color32::WHITE } else { look::text_dim() });
    };
    let mut y = r.min.y + 8.0;
    let small = r.width() < 80.0;
    slot(&p, y, "Setting", None);
    y += 24.0;
    // the EQ slot: a click opens the Channel EQ; with one on the strip, its curve
    let eq_r = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
    match (i < AUX).then(|| app.eqs.get(&i).copied()).flatten() {
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
    if i < AUX && ui.interact(eq_r, ui.scope_id().with(("eqslot", i)), Sense::click()).on_hover_text("Channel EQ").clicked() {
        app.eqs.entry(i).or_default();
        app.eq_window = Some(i);
        app.load_engine();
    }
    y += 26.0;
    let aux = (i >= AUX && i != usize::MAX).then(|| i - AUX);
    let chain: Vec<String> = if let Some(a) = aux { app.auxes.get(a).map(|x| x.inserts.clone()).unwrap_or_default() } else { (i != usize::MAX).then(|| app.project.as_ref().and_then(|pr| pr.tracks.get(i)).map(|t| t.plugins.iter().map(|u| (u.slot, u.name.clone())).collect::<Vec<_>>())).flatten().unwrap_or_default().into_iter().filter(|(s, _)| *s >= 3).map(|(_, n)| n).filter(|n| n != "Channel EQ").collect() };
    let instrument = (i != usize::MAX).then(|| app.project.as_ref().and_then(|pr| pr.tracks.get(i)).and_then(|t| t.plugins.iter().find(|u| u.slot == 2)).map(|u| u.name.clone())).flatten();
    if matches!(kind, Some(TrackKind::Instrument) | Some(TrackKind::Drummer)) {
        slot(&p, y, "MIDI FX", None);
        y += 22.0;
    }
    if i != usize::MAX && matches!(kind, Some(TrackKind::Instrument) | Some(TrackKind::Drummer)) {
        let ir = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
        if ui.interact(ir, ui.scope_id().with(("instslot", i)), Sense::click()).on_hover_text("Open the instrument's window").clicked() {
            app.plugin_window = Some(i);
        }
    }
    match kind {
        Some(TrackKind::Instrument) => slot(&p, y, instrument.as_deref().unwrap_or("Sampler"), Some(Color32::from_rgb(0x2f, 0x8f, 0x3a))),
        Some(TrackKind::Drummer) => slot(&p, y, instrument.as_deref().unwrap_or("Drum Kit"), Some(Color32::from_rgb(0x9a, 0x7a, 0x1e))),
        Some(TrackKind::Audio) => {
            slot(&p, y, crate::recording::input_label(app, i), None);
            crate::recording::input_slot(app, ui, Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0)), i);
        }
        None if aux.is_some() => {
            let bus = app.auxes.get(aux.unwrap()).map_or(0, |a| a.bus);
            slot(&p, y, &format!("Bus {}", bus + 1), None);
        }
        None => {
            // (the output's stereo link)
            let s = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
            p.rect_filled(s, 4.0, look::group_bg());
            p.circle_stroke(s.center() - vec2(3.0, 0.0), 4.0, Stroke::new(1.2, look::text_dim()));
            p.circle_stroke(s.center() + vec2(3.0, 0.0), 4.0, Stroke::new(1.2, look::text_dim()));
        }
    }
    y += 24.0;
    // the Audio FX (Logic's blue slots), then an empty one
    for n in chain.iter().take(4) {
        slot(&p, y, n, Some(Color32::from_rgb(0x1f, 0x5f, 0xc8)));
        y += 22.0;
    }
    slot(&p, y, "Audio FX", None);
    y += 24.0;
    if kind.is_some() && i < AUX {
        // the sends: each "Bus N" with its level knob; the empty slot below adds one
        let list = app.sends.get(&i).cloned().unwrap_or_default();
        for (k, snd) in list.iter().enumerate() {
            let sr = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 34.0, 20.0));
            p.rect_filled(sr, 4.0, Color32::from_rgb(0x2a, 0x5a, 0x86));
            p.text(sr.center(), Align2::CENTER_CENTER, format!("Bus {}", snd.bus + 1), look::regular(11.0), Color32::WHITE);
            let kc = pos2(r.max.x - 16.0, y + 10.0);
            let kr = ui.interact(Rect::from_center_size(kc, vec2(18.0, 18.0)), ui.scope_id().with(("send", i, k)), Sense::click_and_drag()).on_hover_text(format!("Send to Bus {}: {:.1} dB{}", snd.bus + 1, snd.level_db, if snd.pre { " (pre fader)" } else { "" }));
            p.circle_filled(kc, 8.0, Color32::from_gray(0x55));
            let f = db_to_frac(snd.level_db);
            let ang = std::f32::consts::PI * (0.75 + 1.5 * f);
            p.line_segment([kc, kc + vec2(ang.cos(), ang.sin()) * 6.5], Stroke::new(1.6, Color32::WHITE));
            let mut changed = None;
            if kr.dragged() {
                let nf = (f - kr.drag_delta().y / 120.0).clamp(0.0, 1.0);
                changed = Some(frac_to_db(nf).min(6.0));
            }
            if kr.double_clicked() {
                changed = Some(0.0);
            }
            if let Some(db) = changed {
                if let Some(x) = app.sends.get_mut(&i).and_then(|v| v.get_mut(k)) {
                    x.level_db = db;
                }
                app.dirty = true;
                app.load_engine();
            }
            let sresp = ui.interact(sr, ui.scope_id().with(("sendslot", i, k)), Sense::click());
            egui::Popup::menu(&sresp).show(|ui| {
                if ui.add(egui::Button::selectable(snd.pre, "Pre Fader")).clicked() {
                    if let Some(x) = app.sends.get_mut(&i).and_then(|v| v.get_mut(k)) {
                        x.pre = !x.pre;
                    }
                    app.load_engine();
                    ui.close();
                }
                if ui.button("No Send").clicked() {
                    if let Some(v) = app.sends.get_mut(&i) {
                        v.remove(k);
                    }
                    app.load_engine();
                    ui.close();
                }
            });
            y += 22.0;
        }
        let er = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
        slot(&p, y, "Sends", None);
        let eresp = ui.interact(er, ui.scope_id().with(("sendadd", i)), Sense::click());
        egui::Popup::menu(&eresp).show(|ui| {
            ui.set_min_width(140.0);
            ui.label(egui::RichText::new("Bus").color(look::text_dim()));
            for b in 0..16 {
                let used = app.auxes.iter().find(|a| a.bus == b).map(|a| a.name.clone());
                let label = match used { Some(n) => format!("Bus {} → {n}", b + 1), None => format!("Bus {}", b + 1) };
                if ui.button(label).clicked() {
                    app.add_send(i, b);
                    ui.close();
                }
            }
        });
        y += 24.0;
        slot(&p, y, "Stereo Out", None);
    } else if aux.is_some() {
        slot(&p, y, "Stereo Out", None);
    } else {
        slot(&p, y, "Mastering", None);
    }
    y += 24.0;
    if !small || r.height() > 520.0 {
        slot(&p, y, "Group", None);
        y += 22.0;
        slot(&p, y, "Read", Some(Color32::from_rgb(0x3a, 0x6e, 0x3a)));
        y += 26.0;
    }
    // the pan knob
    let kc = pos2(r.center().x, y + 16.0);
    let kr = Rect::from_center_size(kc, vec2(34.0, 34.0));
    let kresp = ui.interact(kr, ui.scope_id().with(("pan", i)), Sense::click_and_drag()).on_hover_text(format!("Pan {:+.0}", pan * 64.0));
    p.circle_filled(kc, 15.0, Color32::from_gray(0x55));
    p.circle_stroke(kc, 15.0, Stroke::new(1.0, Color32::from_gray(0x22)));
    let ang = std::f32::consts::PI * (0.5 + 0.75 * pan) + std::f32::consts::PI;
    let tip = kc + vec2(ang.cos(), ang.sin()) * 11.0;
    p.line_segment([kc, tip], Stroke::new(2.0, Color32::WHITE));
    if kresp.dragged() {
        pan = (pan - kresp.drag_delta().y / 120.0).clamp(-1.0, 1.0);
        out = Some(());
    }
    if kresp.double_clicked() || (kresp.clicked() && ui.input(|i| i.modifiers.alt)) {
        pan = 0.0;
        out = Some(());
    }
    y += 40.0;
    // readouts: volume, peak
    let rv = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(36.0, 18.0));
    let rpk = Rect::from_min_size(pos2(r.max.x - 44.0, y), vec2(36.0, 18.0));
    p.rect_filled(rv, 3.0, look::lcd_bg());
    p.rect_filled(rpk, 3.0, look::lcd_bg());
    p.text(rv.center(), Align2::CENTER_CENTER, if vol <= -90.0 { "-∞".into() } else { format!("{vol:.1}") }, look::regular(10.5), look::lcd_text());
    let peak_db = if meter <= 1e-4 { -90.0 } else { 20.0 * meter.log10() };
    p.text(rpk.center(), Align2::CENTER_CENTER, if peak_db <= -89.0 { "-∞".into() } else { format!("{peak_db:.1}") }, look::regular(10.5), Color32::from_rgb(0x4c, 0xd9, 0x64));
    y += 26.0;
    // the fader and meter
    let bottom_room = 62.0;
    let fh = (r.max.y - bottom_room - y).max(40.0);
    let track = Rect::from_min_size(pos2(r.center().x - 14.0, y), vec2(6.0, fh));
    p.rect_filled(track, 3.0, look::lcd_bg());
    let meter_r = Rect::from_min_size(pos2(r.center().x + 8.0, y), vec2(8.0, fh));
    p.rect_filled(meter_r, 2.0, look::lcd_bg());
    let lv = meter.clamp(0.0, 1.0).sqrt();
    if lv > 0.01 {
        let mr = Rect::from_min_max(pos2(meter_r.min.x + 1.0, meter_r.max.y - lv * fh), pos2(meter_r.max.x - 1.0, meter_r.max.y));
        p.rect_filled(mr, 1.0, if meter > 0.98 { look::record_red() } else { Color32::from_rgb(0x4c, 0xd9, 0x64) });
    }
    // the scale between fader and meter, as Logic's: 0 at the top mark, down to 60
    if r.width() >= 80.0 {
        for db in [0.0f32, 3.0, 6.0, 9.0, 12.0, 15.0, 18.0, 21.0, 24.0, 30.0, 35.0, 40.0, 45.0, 50.0, 60.0] {
            let yy = track.max.y - db_to_frac(-db) * fh;
            p.hline(meter_r.max.x + 2.0..=meter_r.max.x + 5.0, yy, Stroke::new(1.0, look::text_dim()));
            p.text(pos2(meter_r.max.x + 7.0, yy), Align2::LEFT_CENTER, format!("{db:.0}"), look::regular(8.0), look::text_dim());
        }
    }
    let fy = track.max.y - db_to_frac(vol) * fh;
    let cap = Rect::from_center_size(pos2(track.center().x, fy), vec2(26.0, 14.0));
    let fresp = ui.interact(Rect::from_min_max(pos2(track.min.x - 12.0, y - 8.0), pos2(track.max.x + 12.0, y + fh + 8.0)), ui.scope_id().with(("fader", i)), Sense::click_and_drag()).on_hover_text("Volume (double-click: 0 dB)");
    p.rect_filled(cap, 3.0, Color32::from_gray(0xc8));
    p.hline(cap.x_range(), cap.center().y, Stroke::new(1.0, Color32::from_gray(0x40)));
    if fresp.dragged() {
        let f = ((track.max.y - fresp.interact_pointer_pos().map_or(fy, |pp| pp.y)) / fh).clamp(0.0, 1.0);
        vol = frac_to_db(f).min(6.0);
        out = Some(());
    }
    if fresp.double_clicked() || (fresp.clicked() && ui.input(|i| i.modifiers.alt)) {
        vol = 0.0;
        out = Some(());
    }
    // Mute, Solo
    let by = r.max.y - bottom_room + 8.0;
    let mr = Rect::from_min_size(pos2(r.center().x - 30.0, by), vec2(26.0, 20.0));
    let sr = Rect::from_min_size(pos2(r.center().x + 4.0, by), vec2(26.0, 20.0));
    if kind.is_some() || aux.is_some() {
        for (rr, label, on, col, which) in [(mr, "M", mute, Color32::from_rgb(0x4f, 0xa8, 0xe8), 0), (sr, "S", solo, Color32::from_rgb(0xe8, 0xc8, 0x3c), 1)] {
            p.rect_filled(rr, 4.0, if on { col } else { look::group_bg() });
            p.text(rr.center(), Align2::CENTER_CENTER, label, look::bold(11.0), if on { Color32::BLACK } else { look::text() });
            if ui.interact(rr, ui.scope_id().with(("ms", i, which)), Sense::click()).clicked() {
                if which == 0 {
                    mute = !mute;
                } else {
                    solo = !solo;
                }
                out = Some(());
            }
        }
    }
    // the name
    let nr = Rect::from_min_size(pos2(r.min.x + 4.0, r.max.y - 26.0), vec2(r.width() - 8.0, 20.0));
    let c = kind.map_or(look::text_dim(), look::region_color);
    p.rect_filled(nr, 4.0, c.gamma_multiply(0.35));
    p.with_clip_rect(nr).text(nr.center(), Align2::CENTER_CENTER, name, look::regular(11.0), look::text());
    if ui.interact(nr, ui.scope_id().with(("stripname", i)), Sense::click()).clicked() && i != usize::MAX {
        app.selected_track = Some(i);
    }
    out.map(|_| (vol, pan, mute, solo))
}
