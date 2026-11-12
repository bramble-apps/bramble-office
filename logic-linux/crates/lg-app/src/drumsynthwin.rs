//! Drum Synth's window, laid out as Logic's: the Group and Sound type pop-ups at the top (the
//! Sound pop-up lists the group's types and Back to Default), the instrument's picture (a click
//! plays the sound), the sound type's own controls (up to eight) beside Pitch, Decay and
//! Volume, and Key Tracking with its Root Key and the Mono / Poly / Gate Mode at the bottom.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::drum_synth as ds;

use crate::app::App;
use crate::look;
use crate::synthui::{Pane, Skin};

pub const COMMANDS: &[&str] = &["Back to Default"];

const SKIN: Skin = Skin::new(0x1d1f22, 0x2a2d31, 0xe0a63a).texts(0xe6e6e8, 0x9c9ca2);
const W: f32 = 640.0;
const H: f32 = 330.0;

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let acts = crate::synthui::window(app, ctx, t, "Drum Synth", vec2(W, H), SKIN, &[], body);
    for a in acts {
        if a == "play" {
            let root = crate::instwin::values(app, t).and_then(|(p, v)| p.iter().position(|x| x.id == ds::ROOT).map(|i| v[i])).unwrap_or(60.0);
            crate::keyswin::audition(app, t, root.round() as u8, 110);
        }
    }
}

fn body(pn: &mut Pane) {
    let group = pn.geti(ds::GROUP);
    let sound = pn.geti(ds::SOUND).min(ds::SOUNDS.len() - 1);
    // the pop-ups
    pn.text(20.0, 22.0, Align2::LEFT_CENTER, "Group", 10.5, SKIN.dim);
    pn.menu(64.0, 12.0, 170.0, 22.0, ds::GROUP);
    let sr = pn.rect(W / 2.0 - 20.0, 12.0, 220.0, 22.0);
    let resp = pn.ui.interact(sr, pn.id("ds-sound", 0), Sense::click());
    let p = pn.painter();
    p.rect_filled(sr, 4.0, if resp.hovered() { Color32::from_gray(0x34) } else { Color32::from_gray(0x24) });
    p.text(sr.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, ds::SOUNDS[sound].name, look::regular(12.0), SKIN.text);
    crate::glyphs::paint(&p, Rect::from_center_size(pos2(sr.max.x - 12.0, sr.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_UP_DOWN, 10.0, SKIN.dim);
    let mut pick: Option<Vec<(u32, f32)>> = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(200.0);
        for i in ds::sounds_of(group) {
            if ui.add(egui::Button::selectable(i == sound, ds::SOUNDS[i].name)).clicked() {
                pick = Some(vec![(ds::SOUND, i as f32)]);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Back to Default").clicked() {
            pick = Some(ds::defaults_of(sound));
            ui.close();
        }
    });
    if let Some(v) = pick {
        for (id, x) in v {
            pn.put(id, x);
        }
    }
    // the picture: a click plays the sound
    let ir = pn.rect(20.0, 50.0, 170.0, 170.0);
    let iresp = pn.ui.interact(ir, pn.id("ds-icon", 0), Sense::click()).on_hover_text("Play the sound");
    p.rect_filled(ir, 10.0, if iresp.hovered() { Color32::from_gray(0x34) } else { SKIN.panel });
    picture(&p, ir.shrink(26.0), ds::SOUNDS[sound].model, SKIN.accent);
    if iresp.clicked() {
        pn.actions.push("play".into());
    }
    // the main controls and the sound type's own
    pn.group(206.0, 50.0, W - 226.0, 170.0, "");
    let controls = ds::SOUNDS[sound].controls;
    for (k, &id) in controls.iter().enumerate().take(8) {
        let (cx, cy) = (250.0 + (k % 4) as f32 * 98.0, 58.0 + (k / 4) as f32 * 80.0);
        pn.knob(cx, cy, 46.0, id, "");
        let name = pn.prm(id).name;
        pn.text(cx, cy + 62.0, Align2::CENTER_CENTER, name, 10.5, SKIN.dim);
    }
    pn.group(20.0, 232.0, W - 40.0, 86.0, "");
    for (k, (id, l)) in [(ds::PITCH, "Pitch"), (ds::DECAY, "Decay"), (ds::VOLUME, "Volume")].iter().enumerate() {
        let cx = 70.0 + k as f32 * 90.0;
        pn.knob(cx, 238.0, 48.0, *id, "");
        pn.text(cx, 304.0, Align2::CENTER_CENTER, l, 10.5, SKIN.dim);
    }
    // Key Tracking, Root Key, Mode
    pn.toggle(330.0, 250.0, 110.0, 22.0, ds::KEY_TRACK, "Key Tracking");
    if pn.on(ds::KEY_TRACK) {
        pn.text(330.0, 292.0, Align2::LEFT_CENTER, "Root Key", 10.5, SKIN.dim);
        pn.field(386.0, 281.0, 54.0, 22.0, ds::ROOT, 1.0);
    }
    pn.text(460.0, 240.0, Align2::LEFT_CENTER, "Mode", 10.5, SKIN.dim);
    pn.seg(460.0, 250.0, 150.0, 22.0, ds::MODE, ds::MODES, false);
}

/// A drawn picture of the instrument (own drawing).
fn picture(p: &egui::Painter, r: Rect, m: ds::Model, col: Color32) {
    let c = r.center();
    let s = Stroke::new(2.5, col);
    let w = r.width();
    use ds::Model::*;
    match m {
        Kick => {
            p.circle_stroke(c, w * 0.45, s);
            p.circle_stroke(c, w * 0.3, Stroke::new(1.5, col.gamma_multiply(0.6)));
            p.circle_filled(c, w * 0.06, col);
        }
        Snare | Rim | Tom | Perc => {
            let rr = Rect::from_center_size(c, vec2(w * 0.9, w * 0.5));
            p.rect_stroke(rr, 6.0, s, egui::StrokeKind::Middle);
            p.line_segment([rr.left_top() + vec2(0.0, w * 0.12), rr.right_top() + vec2(0.0, w * 0.12)], s);
            for k in 0..5 {
                let x = rr.min.x + rr.width() * (k as f32 + 0.5) / 5.0;
                p.line_segment([pos2(x, rr.min.y + w * 0.12), pos2(x, rr.max.y)], Stroke::new(1.2, col.gamma_multiply(0.6)));
            }
        }
        Clap => {
            for k in 0..3 {
                let x = c.x + (k as f32 - 1.0) * w * 0.25;
                p.line_segment([pos2(x, c.y - w * 0.35), pos2(x + w * 0.08, c.y + w * 0.35)], s);
            }
        }
        Metal => {
            p.add(egui::Shape::convex_polygon(vec![pos2(c.x - w * 0.2, c.y - w * 0.35), pos2(c.x + w * 0.2, c.y - w * 0.35), pos2(c.x + w * 0.35, c.y + w * 0.35), pos2(c.x - w * 0.35, c.y + w * 0.35)], Color32::TRANSPARENT, s));
        }
        Hat | OpenHat | Cymbal | Shaker => {
            p.line_segment([pos2(c.x - w * 0.45, c.y), pos2(c.x + w * 0.45, c.y - w * 0.08)], s);
            p.line_segment([pos2(c.x, c.y - w * 0.04), pos2(c.x, c.y + w * 0.45)], Stroke::new(1.5, col.gamma_multiply(0.7)));
            if m == Hat || m == OpenHat {
                let dy = if m == Hat { 0.04 } else { 0.14 };
                p.line_segment([pos2(c.x - w * 0.45, c.y + w * dy), pos2(c.x + w * 0.45, c.y + w * (dy - 0.08))], s);
            }
        }
    }
}
