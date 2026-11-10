//! Smart Controls (B), as Logic's: the selected track's screen controls (its instrument's main
//! parameters as knobs), its name and patch at the left, and the Channel EQ's curve at the right
//! (a click opens it). Tracks playing through the SoundFont get the channel's controls (volume,
//! pan and the EQ's low, mid and high gains), as Logic's default mapping for a strip.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;
use crate::widgets;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(t) = app.selected_track.filter(|t| app.project.as_ref().is_some_and(|p| *t < p.tracks.len())) else {
        p.text(area.center(), Align2::CENTER_CENTER, "Select a track to see its Smart Controls", look::regular(13.0), look::text_dim());
        return;
    };
    let name = app.project.as_ref().map(|p| p.tracks[t].name.clone()).unwrap_or_default();
    let instrument = app.instrument_for(t).unwrap_or_default();
    // the header: track name, instrument
    let head = Rect::from_min_size(area.min, vec2(area.width(), 34.0));
    p.text(pos2(head.min.x + 14.0, head.center().y), Align2::LEFT_CENTER, &name, look::bold(13.0), look::text());
    let nw = p.layout_no_wrap(name.clone(), look::bold(13.0), look::text()).size().x;
    p.text(pos2(head.min.x + 24.0 + nw, head.center().y), Align2::LEFT_CENTER, &instrument, look::regular(12.0), look::text_dim());
    p.hline(head.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    let body = Rect::from_min_max(pos2(area.min.x, head.max.y + 1.0), area.max);
    // the EQ thumbnail on the right
    let eq_w = (body.width() * 0.28).clamp(180.0, 320.0);
    let eq_r = Rect::from_min_max(pos2(body.max.x - eq_w - 12.0, body.min.y + 14.0), pos2(body.max.x - 12.0, body.max.y - 14.0));
    eq_thumb(app, ui, eq_r, t);
    let knobs_area = Rect::from_min_max(pos2(body.min.x + 12.0, body.min.y + 10.0), pos2(eq_r.min.x - 16.0, body.max.y - 10.0));
    let params = app.engine.lock().ok().and_then(|e| e.instrument_params(t));
    match params {
        Some((params, values)) => {
            // Logic shows the patch's mapped controls; ours: the instrument's first parameters
            // that are knobs (menus and switches stay in the plug-in window)
            let shown: Vec<(lg_dsp::Param, f32)> = params.into_iter().zip(values).filter(|(p, _)| !matches!(p.unit, lg_dsp::Unit::Menu | lg_dsp::Unit::OnOff)).take(12).collect();
            let per_row = 6usize;
            let rows = shown.len().div_ceil(per_row).max(1);
            let kw = (knobs_area.width() / per_row as f32).min(96.0);
            let kh = (knobs_area.height() / rows as f32).min(110.0);
            for (k, (prm, mut v)) in shown.into_iter().enumerate() {
                let r = Rect::from_min_size(knobs_area.min + vec2((k % per_row) as f32 * kw, (k / per_row) as f32 * kh), vec2(kw, kh));
                let unit = prm.unit;
                let fmt = move |v: f32| format_value(v, unit);
                if widgets::knob(ui, r.shrink(6.0), ui.scope_id().with(("sc", t, prm.id)), &mut v, prm.min, prm.max, prm.default, prm.name, &fmt) {
                    if let Ok(mut e) = app.engine.lock() {
                        e.set_instrument_param(t, prm.id, v);
                    }
                    app.dirty = true;
                }
            }
        }
        None => {
            // the channel's controls
            let (vol, pan) = app.project.as_ref().map(|p| (p.tracks[t].volume_db, p.tracks[t].pan)).unwrap_or((0.0, 0.0));
            let mut eq = app.eqs.get(&t).copied().unwrap_or_default();
            let kw = (knobs_area.width() / 5.0).min(96.0);
            let kh = knobs_area.height().min(110.0);
            let at = |k: usize| Rect::from_min_size(knobs_area.min + vec2(k as f32 * kw, 0.0), vec2(kw, kh)).shrink(6.0);
            let mut v = vol;
            if widgets::knob(ui, at(0), ui.scope_id().with(("sc-vol", t)), &mut v, -60.0, 6.0, 0.0, "Volume", &|v| format!("{v:.1} dB")) {
                if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    tr.volume_db = v;
                }
                app.sync_mix();
            }
            let mut v = pan * 64.0;
            if widgets::knob(ui, at(1), ui.scope_id().with(("sc-pan", t)), &mut v, -64.0, 63.0, 0.0, "Pan", &|v| format!("{v:+.0}")) {
                if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    tr.pan = v / 64.0;
                }
                app.sync_mix();
            }
            let mut eq_changed = false;
            for (k, (label, band)) in [("Low", 1usize), ("Mid", 3), ("High", 6)].into_iter().enumerate() {
                let mut g = eq.bands[band].gain;
                if widgets::knob(ui, at(2 + k), ui.scope_id().with(("sc-eq", t, band)), &mut g, -24.0, 24.0, 0.0, label, &|v| format!("{v:+.1} dB")) {
                    eq.bands[band].gain = g;
                    eq.bands[band].on = true;
                    eq_changed = true;
                }
            }
            if eq_changed {
                app.eqs.insert(t, eq);
                app.load_engine();
                app.dirty = true;
            }
            p.text(pos2(knobs_area.min.x, knobs_area.max.y - 6.0), Align2::LEFT_BOTTOM, "Playing through the SoundFont: these are the channel's controls", look::regular(11.0), look::text_dim());
        }
    }
}

fn format_value(v: f32, unit: lg_dsp::Unit) -> String {
    use lg_dsp::Unit::*;
    match unit {
        Db => format!("{v:.1} dB"),
        Hz => if v >= 1000.0 { format!("{:.2} kHz", v / 1000.0) } else { format!("{v:.0} Hz") },
        Ms => format!("{v:.0} ms"),
        Seconds => format!("{v:.2} s"),
        Percent => format!("{v:.0} %"),
        Ratio => format!("{v:.1} : 1"),
        Semitones => format!("{v:+.0} st"),
        Cents => format!("{v:+.0} ct"),
        Octaves => format!("{v:.2} oct"),
        Pan => format!("{:+.0}", v * 64.0),
        _ => format!("{v:.2}"),
    }
}

/// The Channel EQ's curve in a dark box (a click opens the EQ).
fn eq_thumb(app: &mut App, ui: &mut egui::Ui, r: Rect, t: usize) {
    let p = ui.painter();
    p.rect_filled(r, 6.0, look::lcd_bg());
    let e = app.eqs.get(&t).copied();
    for k in 1..4 {
        let y = r.min.y + r.height() * k as f32 / 4.0;
        p.hline(r.x_range(), y, Stroke::new(1.0, Color32::from_white_alpha(14)));
    }
    let curve = e.unwrap_or_default();
    let pts: Vec<egui::Pos2> = (0..=80).map(|k| {
        let f = 20.0 * 1000f32.powf(k as f32 / 80.0);
        pos2(r.min.x + k as f32 / 80.0 * r.width(), r.center().y - crate::eq::response(&curve, f).clamp(-24.0, 24.0) / 24.0 * (r.height() / 2.0 - 6.0))
    }).collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.6, Color32::from_rgb(0x9c, 0xe0, 0xff))));
    p.text(r.left_top() + vec2(8.0, 6.0), Align2::LEFT_TOP, "EQ", look::semibold(11.0), look::lcd_dim());
    if ui.interact(r, ui.scope_id().with(("sc-eqopen", t)), Sense::click()).on_hover_text("Open the Channel EQ").clicked() {
        app.eqs.entry(t).or_default();
        app.eq_window = Some(t);
        app.load_engine();
    }
}
