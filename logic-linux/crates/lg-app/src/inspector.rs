//! The Inspector (I), as Logic's: "Region:" with the region's playback parameters (Mute, Loop,
//! Quantize, Q-Swing, Transpose, Fine Tune, Velocity, Dynamics, Gate Time, Delay), "Track:" with
//! the track's (Channel, Freeze Mode, Transpose, Velocity, Delay, Key and Velocity Limits, No
//! Transpose), each opening and closing with its header; under them two channel strips: the
//! selected track's and the one it plays into (Stereo Out).

use egui::{pos2, vec2, Align2, Rect, Stroke};

use crate::app::App;
use crate::look;
use crate::widgets;

const ROW: f32 = 22.0;

pub const QUANTIZE: &[(&str, u32)] = &[
    ("Off", 0),
    ("1/1 Note", 3840),
    ("1/2 Note", 1920),
    ("1/4 Note", 960),
    ("1/8 Note", 480),
    ("1/16 Note", 240),
    ("1/32 Note", 120),
    ("1/64 Note", 60),
    ("1/2 Triplet", 1280),
    ("1/4 Triplet", 640),
    ("1/8 Triplet", 320),
    ("1/16 Triplet", 160),
    ("1/32 Triplet", 80),
];

fn label(p: &egui::Painter, area: Rect, y: f32, text: &str) {
    p.text(pos2(area.min.x + 14.0, y + ROW / 2.0), Align2::LEFT_CENTER, text, look::regular(12.0), look::text_dim());
}

fn slot_rect(area: Rect, y: f32) -> Rect {
    Rect::from_min_max(pos2(area.min.x + area.width() * 0.48, y + 2.0), pos2(area.max.x - 8.0, y + ROW - 2.0))
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    let mut y = area.min.y;
    let region_name = app.selected_region.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r)).map(|r| r.name.clone());
    let track_name = app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).map(|t| t.name.clone());
    let hr = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), 32.0));
    if widgets::disclosure(ui, hr, ui.scope_id().with("insp-region"), "Region:", region_name.as_deref().unwrap_or(""), app.inspector_region_open) {
        app.inspector_region_open = !app.inspector_region_open;
    }
    y += 33.0;
    if app.inspector_region_open {
        if let Some((t, r)) = app.selected_region {
            y = region_rows(app, ui, area, y, t, r);
        }
    }
    let hr = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), 32.0));
    if widgets::disclosure(ui, hr, ui.scope_id().with("insp-track"), "Track:", track_name.as_deref().unwrap_or(""), app.inspector_track_open) {
        app.inspector_track_open = !app.inspector_track_open;
    }
    y += 33.0;
    if app.inspector_track_open {
        if let Some(t) = app.selected_track {
            y = track_rows(app, ui, area, y, t);
        }
    }
    // the two channel strips
    let strips = Rect::from_min_max(pos2(area.min.x, y + 6.0), pos2(area.max.x - 1.0, area.max.y - 4.0));
    if strips.height() > 260.0 {
        p.vline(strips.center().x, strips.y_range(), Stroke::new(1.0, look::line()));
        let w = strips.width() / 2.0;
        // (in a scope of their own: the Mixer shows the same strips, with the same ids)
        ui.scope_builder(egui::UiBuilder::new().id_salt("inspector-strips").max_rect(strips), |ui| {
            if let Some(t) = app.selected_track.filter(|t| app.project.as_ref().is_some_and(|p| *t < p.tracks.len())) {
                let left = Rect::from_min_size(strips.min + vec2(4.0, 0.0), vec2(w - 8.0, strips.height()));
                crate::mixer::track_strip(app, ui, left, t);
            }
            let right = Rect::from_min_size(strips.min + vec2(w + 4.0, 0.0), vec2(w - 8.0, strips.height()));
            crate::mixer::output_strip(app, ui, right);
        });
    }
}

type Shown = Box<dyn Fn(f32) -> String>;

fn signed(v: f32) -> String {
    if v == 0.0 { String::new() } else { format!("{v:+.0}") }
}

fn region_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, t: usize, r: usize) -> f32 {
    let Some(region) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return y };
    let p = ui.painter().clone();
    let mut props = region.props;
    let mut muted = region.muted;
    let audio = region.audio.is_some();
    let id = ui.scope_id().with(("rgn", t, r));
    let mut changed = false;
    label(&p, area, y, "Mute");
    changed |= widgets::check_slot(ui, slot_rect(area, y), id.with("mute"), &mut muted);
    y += ROW;
    label(&p, area, y, "Loop");
    changed |= widgets::check_slot(ui, slot_rect(area, y), id.with("loop"), &mut props.looped);
    y += ROW;
    label(&p, area, y, "Quantize");
    let cur = QUANTIZE.iter().position(|q| q.1 == props.quantize).unwrap_or(0);
    let names: Vec<&str> = QUANTIZE.iter().map(|q| q.0).collect();
    if let Some(i) = widgets::choice_slot(ui, slot_rect(area, y), id.with("q"), cur, &names) {
        props.quantize = QUANTIZE[i].1;
        changed = true;
    }
    y += ROW;
    let pct_or = |dflt: f32| move |v: f32| if v == 0.0 { "Fixed".to_string() } else if v == dflt { String::new() } else { format!("{v:.0} %") };
    let mut rows: Vec<(&str, f32, f32, f32, f32, Shown)> = vec![
        ("Q-Swing", props.swing as f32, 0.0, 100.0, 50.0, Box::new(|v: f32| format!("{v:.0} %"))),
        ("Transpose", props.transpose as f32, -96.0, 96.0, 0.0, Box::new(signed)),
        ("Fine Tune", props.fine_tune as f32, -50.0, 50.0, 0.0, Box::new(signed)),
    ];
    if !audio {
        rows.push(("Velocity", props.velocity as f32, -99.0, 99.0, 0.0, Box::new(signed)));
        rows.push(("Dynamics", props.dynamics as f32, 0.0, 300.0, 100.0, Box::new(pct_or(100.0))));
        rows.push(("Gate Time", props.gate_time as f32, 0.0, 200.0, 100.0, Box::new(pct_or(100.0))));
    }
    rows.push(("Delay", props.delay as f32, -3840.0, 3840.0, 0.0, Box::new(|v: f32| if v == 0.0 { String::new() } else { format!("{v:+.0} ticks") })));
    for (i, (name, mut v, min, max, def, shown)) in rows.into_iter().enumerate() {
        label(&p, area, y, name);
        let step = if name == "Delay" { 10.0 } else { 1.0 };
        if widgets::value_slot(ui, slot_rect(area, y), id.with(("v", i)), &mut v, min, max, def, step, &*shown) {
            changed = true;
            match name {
                "Q-Swing" => props.swing = v as u8,
                "Transpose" => props.transpose = v as i8,
                "Fine Tune" => props.fine_tune = v as i8,
                "Velocity" => props.velocity = v as i8,
                "Dynamics" => props.dynamics = v as u16,
                "Gate Time" => props.gate_time = v as u16,
                _ => props.delay = v as i32,
            }
        }
        y += ROW;
    }
    if changed {
        app.checkpoint("Region Parameter");
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            rg.props = props;
            rg.muted = muted;
        }
        app.after_edit();
    }
    y += 4.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

fn track_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, t: usize) -> f32 {
    let p = ui.painter().clone();
    let mut tp = app.track_props.get(&t).copied().unwrap_or_default();
    let id = ui.scope_id().with(("trk", t));
    let mut changed = false;
    let value = |p: &egui::Painter, y: f32, text: String| {
        p.text(pos2(slot_rect(area, y).max.x - 4.0, y + ROW / 2.0), Align2::RIGHT_CENTER, text, look::regular(12.0), look::text());
    };
    label(&p, area, y, "Channel");
    let audio = app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Audio);
    value(&p, y, format!("{} {}", if audio { "Audio" } else { "Inst" }, t + 1));
    y += ROW;
    label(&p, area, y, "Freeze Mode");
    value(&p, y, "Pre Fader".into());
    y += ROW;
    let rows: [(&str, f32, f32, f32); 3] = [("Transpose", tp.transpose as f32, -96.0, 96.0), ("Velocity", tp.velocity as f32, -99.0, 99.0), ("Delay", tp.delay as f32, -3840.0, 3840.0)];
    for (i, (name, mut v, min, max)) in rows.into_iter().enumerate() {
        label(&p, area, y, name);
        if widgets::value_slot(ui, slot_rect(area, y), id.with(("v", i)), &mut v, min, max, 0.0, if i == 2 { 10.0 } else { 1.0 }, &signed) {
            changed = true;
            match i {
                0 => tp.transpose = v as i8,
                1 => tp.velocity = v as i8,
                _ => tp.delay = v as i32,
            }
        }
        y += ROW;
    }
    let pair = |ui: &mut egui::Ui, y: f32, key: &str, (lo, hi): (u8, u8), min: f32, max: f32, shown: &dyn Fn(f32) -> String| -> Option<(u8, u8)> {
        let sr = slot_rect(area, y);
        let half = sr.width() / 2.0;
        let (mut a, mut b) = (lo as f32, hi as f32);
        let c1 = widgets::value_slot(ui, Rect::from_min_size(sr.min, vec2(half, sr.height())), id.with((key, 0)), &mut a, min, max, min, 1.0, shown);
        let c2 = widgets::value_slot(ui, Rect::from_min_size(sr.min + vec2(half, 0.0), vec2(half, sr.height())), id.with((key, 1)), &mut b, min, max, max, 1.0, shown);
        (c1 || c2).then_some((a as u8, b as u8))
    };
    label(&p, area, y, "Key Limit");
    if let Some(v) = pair(ui, y, "key", tp.key_limit, 0.0, 127.0, &|v| crate::app::note_name(v as u8)) {
        tp.key_limit = v;
        changed = true;
    }
    y += ROW;
    label(&p, area, y, "Vel Limit");
    if let Some(v) = pair(ui, y, "vel", tp.vel_limit, 1.0, 127.0, &|v| format!("{v:.0}")) {
        tp.vel_limit = v;
        changed = true;
    }
    y += ROW;
    label(&p, area, y, "No Transpose");
    changed |= widgets::check_slot(ui, slot_rect(area, y), id.with("notr"), &mut tp.no_transpose);
    y += ROW;
    if changed {
        app.checkpoint("Track Parameter");
        app.track_props.insert(t, tp);
        app.after_edit();
    }
    y += 4.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}
