//! The Mixer (X): one channel strip per track and the Stereo Out, as Logic's: the settings and
//! EQ buttons, the instrument slot, Audio FX, the pan knob, volume and peak readouts, the fader
//! with its meter, Mute / Solo and the name at the bottom. Faders drag (Option-click / double-
//! click: 0 dB), the pan knob drags up and down.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::TrackKind;

use crate::app::App;
use crate::look;
use crate::tracks::{db_to_frac, frac_to_db};

const STRIP_W: f32 = 92.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(proj) = app.project.clone() else { return };
    let meters: Vec<f32> = app.engine.lock().map(|e| e.meters.clone()).unwrap_or_default();
    let master = app.engine.lock().map(|e| e.master_meter).unwrap_or(0.0);
    let mut x = area.min.x + 8.0;
    for (i, t) in proj.tracks.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, area.min.y + 6.0), vec2(STRIP_W, area.height() - 10.0));
        if r.min.x > area.max.x {
            break;
        }
        let changed = strip(app, ui, r, i, &t.name, Some(t.kind), t.volume_db, t.pan, t.mute, t.solo, meters.get(i).copied().unwrap_or(0.0), app.selected_track == Some(i));
        if let Some((v, pan, m, s)) = changed {
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)) {
                tr.volume_db = v;
                tr.pan = pan;
                tr.mute = m;
                tr.solo = s;
            }
            app.sync_mix();
        }
        x += STRIP_W + 2.0;
    }
    // the Stereo Out at the right
    let r = Rect::from_min_size(pos2(area.max.x - STRIP_W - 8.0, area.min.y + 6.0), vec2(STRIP_W, area.height() - 10.0));
    if r.min.x > x {
        let master_db = 20.0 * app.engine.lock().map(|e| e.master).unwrap_or(1.0).max(1e-5).log10();
        if let Some((v, ..)) = strip(app, ui, r, usize::MAX, "Stereo Out", None, master_db, 0.0, false, false, master, false) {
            if let Ok(mut e) = app.engine.lock() {
                e.master = crate::app::db_to_gain(v);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn strip(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize, name: &str, kind: Option<TrackKind>, vol: f32, pan: f32, mute: bool, solo: bool, meter: f32, selected: bool) -> Option<(f32, f32, bool, bool)> {
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
    slot(&p, y, "Setting", None);
    y += 24.0;
    // the EQ slot: a click opens the Channel EQ; with one on the strip, its curve
    let eq_r = Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
    match (i != usize::MAX).then(|| app.eqs.get(&i).copied()).flatten() {
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
    if i != usize::MAX && ui.interact(eq_r, ui.scope_id().with(("eqslot", i)), Sense::click()).clicked() {
        app.eqs.entry(i).or_default();
        app.eq_window = Some(i);
        app.load_engine();
    }
    y += 28.0;
    match kind {
        Some(TrackKind::Instrument) => slot(&p, y, "Sampler", Some(Color32::from_rgb(0x2f, 0x8f, 0x3a))),
        Some(TrackKind::Drummer) => slot(&p, y, "Drum Kit", Some(Color32::from_rgb(0x9a, 0x7a, 0x1e))),
        Some(TrackKind::Audio) => slot(&p, y, "Input 1", None),
        None => slot(&p, y, "Audio FX", None),
    }
    y += 24.0;
    slot(&p, y, "Audio FX", None);
    y += 24.0;
    slot(&p, y, "Stereo Out", None);
    y += 34.0;
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
    if kind.is_some() {
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
