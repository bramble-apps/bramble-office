//! The Vintage B3 window, laid out as Logic's: the control bar's Main, Rotor Cabinet, Options,
//! Effects and Expert buttons. Main shows the 20 draw bars (upper on the left, the two pedal bars
//! in the centre, lower on the right), the Rotation switch at the lower left and, chosen with
//! the Control / Preset / Split buttons at the lower right, the Scanner Vibrato, Drive and
//! Percussion controls, the preset keys with the Upper Morph slider, or the keyboard with its
//! mode, split points, transpositions and channels.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::organ::{self, LOWER, PEDAL, PRESETS_L, PRESETS_U, UPPER};

use crate::app::App;
use crate::instwin;
use crate::keyswin::{self, group, Pane};
use crate::look;

const FEET: [&str; 9] = ["16'", "5⅓'", "8'", "4'", "2⅔'", "2'", "1⅗'", "1⅓'", "1'"];
const KEYS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// A draw bar: dragging it down pulls it out (louder). `kind`: 0 brown, 1 white, 2 black.
fn drawbar(ui: &mut egui::Ui, r: Rect, id: egui::Id, value: &mut f32, kind: u8, label: &str) -> bool {
    let resp = ui.interact(r, id, Sense::click_and_drag()).on_hover_text(format!("{label}: {:.0}", value.round()));
    let mut changed = false;
    let travel = r.height() - 44.0;
    if resp.dragged() {
        let v = (*value + resp.drag_delta().y / travel * 8.0).clamp(0.0, 8.0);
        if v != *value {
            *value = v;
            changed = true;
        }
    }
    if resp.drag_stopped() && value.fract() != 0.0 {
        *value = value.round();
        changed = true;
    }
    let p = ui.painter();
    let (fill, ink) = match kind {
        0 => (Color32::from_rgb(0x7a, 0x4a, 0x2a), Color32::WHITE),
        1 => (Color32::from_gray(0xee), Color32::from_gray(0x20)),
        _ => (Color32::from_gray(0x1c), Color32::WHITE),
    };
    let top = r.min.y + 16.0;
    let out = travel * (*value / 8.0);
    // the bar's stem with its numbers, then the handle
    let stem = Rect::from_min_max(pos2(r.center().x - 6.0, top), pos2(r.center().x + 6.0, top + out + 4.0));
    p.rect_filled(stem, 1.0, Color32::from_gray(0xc8));
    for n in 1..=8 {
        let y = top + out - travel * (n as f32 - 0.5) / 8.0;
        if y > top {
            p.text(pos2(r.center().x, y), Align2::CENTER_CENTER, format!("{n}"), look::regular(8.0), Color32::from_gray(0x30));
        }
    }
    let handle = Rect::from_min_size(pos2(r.min.x + 1.0, top + out), vec2(r.width() - 2.0, 26.0));
    p.rect_filled(handle, 4.0, if resp.hovered() || resp.dragged() { look::mix(fill, look::accent(), 0.25) } else { fill });
    p.rect_stroke(handle, 4.0, Stroke::new(1.0, Color32::from_gray(0x55)), egui::StrokeKind::Inside);
    p.text(handle.center(), Align2::CENTER_CENTER, format!("{:.0}", value.round()), look::semibold(11.0), ink);
    p.text(pos2(r.center().x, r.min.y + 6.0), Align2::CENTER_CENTER, label, look::regular(9.5), look::text_dim());
    changed
}

fn bars(ui: &mut egui::Ui, pane: &mut Pane, title: &str, first: u32, count: usize, labels: &[&str]) {
    group(ui, title, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(count as f32 * 30.0, 176.0), Sense::hover());
        for i in 0..count {
            let id = first + i as u32;
            let kind = if count == 2 { 0 } else { [0u8, 0, 1, 1, 2, 1, 2, 2, 1][i] };
            let br = Rect::from_min_size(pos2(r.min.x + i as f32 * 30.0 + 2.0, r.min.y), vec2(26.0, 176.0));
            let mut v = pane.get(id);
            if drawbar(ui, br, egui::Id::unique(("b3-bar", pane.t, id)), &mut v, kind, labels[i]) {
                pane.put(id, v);
            }
        }
    });
}

/// The Rotation switch (lower left of Main and Rotor Cabinet): Slow, Brake, Fast.
fn rotation(ui: &mut egui::Ui, pane: &mut Pane) {
    let cur = pane.get(18).round() as i32;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Rotation").size(11.0).color(look::text_dim()));
        ui.spacing_mut().item_spacing.x = 1.0;
        for (v, n) in [(2, "Slow"), (1, "Brake"), (3, "Fast")] {
            if ui.add_enabled(cur != 0, egui::Button::selectable(cur == v, egui::RichText::new(n).size(11.0))).clicked() {
                pane.put(18, v as f32);
            }
        }
    });
}

fn preset_keys(ui: &mut egui::Ui, pane: &mut Pane, manual: u32) {
    let (base, id) = if manual == 0 { (PRESETS_U, 70) } else { (PRESETS_L, 71) };
    let active = pane.get(id).round() as usize;
    let (r, _) = ui.allocate_exact_size(vec2(12.0 * 25.0, 84.0), Sense::hover());
    for k in 0..12usize {
        let kr = Rect::from_min_size(pos2(r.min.x + k as f32 * 25.0, r.min.y), vec2(23.0, 70.0));
        let resp = ui.interact(kr, egui::Id::unique(("b3-preset", pane.t, manual, k)), Sense::click()).on_hover_text(if k == 0 { "C: initialize the registration".to_string() } else { format!("Preset key {}", KEYS[k]) });
        // (the preset keys' colours are reversed: naturals black, sharps white)
        let sharp = matches!(k, 1 | 3 | 6 | 8 | 10);
        let fill = if sharp { Color32::from_gray(0xe6) } else { Color32::from_gray(0x16) };
        let p = ui.painter();
        p.rect_filled(kr, 3.0, if resp.hovered() { look::mix(fill, look::accent(), 0.2) } else { fill });
        if k == active {
            p.rect_stroke(kr, 3.0, Stroke::new(2.0, look::accent()), egui::StrokeKind::Outside);
        }
        // the registration in miniature: one line per draw bar
        if k > 0 {
            for d in 0..9u32 {
                let v = if k == active { pane.get(if manual == 0 { UPPER } else { LOWER } + d) } else { pane.get(base + k as u32 * 9 + d) };
                let x = kr.min.x + 3.5 + d as f32 * 2.0;
                p.line_segment([pos2(x, kr.min.y + 5.0), pos2(x, kr.min.y + 5.0 + 40.0 * v / 8.0 + 1.0)], Stroke::new(1.2, if sharp { Color32::from_gray(0x30) } else { Color32::from_gray(0xd8) }));
            }
        }
        p.text(pos2(kr.center().x, kr.max.y + 8.0), Align2::CENTER_CENTER, KEYS[k], look::regular(9.0), look::text_dim());
        if resp.clicked() {
            pane.put(id, k as f32);
        }
    }
}

fn whole(ui: &mut egui::Ui, pane: &mut Pane, id: u32, label: &str, suffix: &str) {
    let Some(p) = pane.param(id) else { return };
    let mut v = pane.get(id).round() as i32;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
        if ui.add(egui::DragValue::new(&mut v).range(p.min as i32..=p.max as i32).speed(0.1).suffix(suffix)).changed() {
            pane.put(id, v as f32);
        }
    });
}

fn split_view(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            // the keyboard mode switch, left of the keyboard
            let cur = pane.get(61).round() as usize;
            for (k, n) in organ::KBD_MODE.iter().enumerate() {
                if ui.add_sized(vec2(64.0, 20.0), egui::Button::selectable(cur == k, egui::RichText::new(*n).size(11.0))).clicked() {
                    pane.put(61, k as f32);
                }
            }
        });
        let mode = pane.get(61).round() as i32;
        let (s1, s2) = (pane.get(62).round() as u8, pane.get(63).round() as u8);
        let (lo, hi) = (24u8, 108u8);
        ui.vertical(|ui| {
            let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width().min(640.0), 16.0), Sense::hover());
            let (kr, _) = ui.allocate_exact_size(vec2(strip.width(), 54.0), Sense::hover());
            let x_of = |n: u8| kr.min.x + (n.saturating_sub(lo)) as f32 / (hi - lo + 1) as f32 * kr.width();
            let presets = pane.on(72);
            let mark = |n: u8| -> Option<Color32> {
                if presets && (24..36).contains(&n) {
                    Some(Color32::from_gray(0x80))
                } else if mode == 2 && n < s1 {
                    Some(Color32::from_rgb(0xd9, 0x8a, 0x2b))
                } else if mode >= 1 && n < s2 {
                    Some(Color32::from_rgb(0x4c, 0xa8, 0x5a))
                } else {
                    None
                }
            };
            if let Some(n) = keyswin::keyboard(ui, kr, egui::Id::unique(("b3-kbd", pane.t)), lo, hi, &mark) {
                ui.data_mut(|d| d.insert_temp(egui::Id::unique(("b3-audition", pane.t)), n));
            }
            // the split icons: drag them along the keyboard; one pushes the other along
            let handles: &[(u32, u8)] = if mode == 2 { &[(62, s1), (63, s2)] } else if mode == 1 { &[(63, s2)] } else { &[] };
            for (id, at) in handles {
                let c = pos2(x_of(*at), strip.center().y);
                let hr = Rect::from_center_size(c, vec2(14.0, 14.0));
                let resp = ui.interact(hr, egui::Id::unique(("b3-split", pane.t, *id)), Sense::drag()).on_hover_text(format!("{}: {}", if *id == 62 { "Pedal / Lower" } else { "Lower / Upper" }, lg_synth::sampler_doc::note_name(*at)));
                ui.painter().add(egui::Shape::convex_polygon(vec![pos2(c.x - 6.0, c.y - 6.0), pos2(c.x + 6.0, c.y - 6.0), pos2(c.x, c.y + 7.0)], if *id == 62 { Color32::from_rgb(0xd9, 0x8a, 0x2b) } else { Color32::from_rgb(0x4c, 0xa8, 0x5a) }, Stroke::NONE));
                if resp.dragged() {
                    if let Some(pp) = resp.interact_pointer_pos() {
                        let n = (lo as f32 + ((pp.x - kr.min.x) / kr.width()).clamp(0.0, 1.0) * (hi - lo + 1) as f32).round().clamp(24.0, 108.0) as u8;
                        pane.put(*id, n as f32);
                        if *id == 62 && n > s2 {
                            pane.put(63, n as f32);
                        }
                        if *id == 63 && n < s1 {
                            pane.put(62, n as f32);
                        }
                    }
                }
            }
            ui.horizontal(|ui| {
                for (id, label) in [(66u32, "Pedal Transpose"), (65, "Lower Transpose"), (64, "Upper Transpose")] {
                    let cur = pane.get(id).round() as i32;
                    ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
                    ui.menu_button(format!("{cur:+} oct ⌄"), |ui| {
                        for o in [2, 1, 0, -1, -2] {
                            if ui.add(egui::Button::selectable(o == cur, format!("{o:+} octave{}", if o.abs() == 1 { "" } else { "s" }))).clicked() {
                                pane.put(id, o as f32);
                                ui.close();
                            }
                        }
                    });
                }
            });
            if mode == 2 {
                ui.horizontal(|ui| {
                    whole(ui, pane, 69, "Pedal Channel", "");
                    whole(ui, pane, 68, "Lower Channel", "");
                    whole(ui, pane, 67, "Upper Channel", "");
                });
            }
        });
    });
}

fn main_page(ui: &mut egui::Ui, pane: &mut Pane, lower: &mut usize) {
    ui.horizontal(|ui| {
        bars(ui, pane, "Upper", UPPER, 9, &FEET);
        bars(ui, pane, "Pedal", PEDAL, 2, &["16'", "8'"]);
        bars(ui, pane, "Lower", LOWER, 9, &FEET);
    });
    ui.add_space(4.0);
    egui::Frame::new().fill(look::group_bg()).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
        ui.set_min_height(112.0);
        ui.set_min_width(ui.available_width());
        match *lower {
            1 => {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Upper presets").size(10.0).color(look::text_dim()));
                        preset_keys(ui, pane, 0);
                    });
                    // the Upper Morph slider between the two sets of keys
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Upper Morph").size(10.0).color(look::text_dim()));
                        let mut v = pane.get(75);
                        if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).vertical().show_value(false)).changed() {
                            pane.put(75, v);
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Lower presets").size(10.0).color(look::text_dim()));
                        preset_keys(ui, pane, 1);
                    });
                });
            }
            2 => split_view(ui, pane),
            _ => {
                ui.horizontal(|ui| {
                    group(ui, "Scanner Vibrato", |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                pane.switch(ui, 41, "Upper");
                                pane.switch(ui, 42, "Lower");
                            });
                            // the Type knob: V1..C3
                            let (r, _) = ui.allocate_exact_size(vec2(64.0, 74.0), Sense::hover());
                            let mut v = pane.get(15).max(1.0);
                            if crate::widgets::knob(ui, r, egui::Id::unique(("b3-vib", pane.t)), &mut v, 1.0, 6.0, 6.0, "Type", &|x| ["V1", "V2", "V3", "C1", "C2", "C3"][(x.round() as usize).clamp(1, 6) - 1].to_string()) {
                                pane.put(15, v.round());
                            }
                            ui.label(egui::RichText::new(["Off", "V1", "V2", "V3", "C1", "C2", "C3"][pane.get(15).round() as usize % 7]).size(13.0).strong());
                        });
                    });
                    group(ui, "Distortion", |ui| pane.knob(ui, 17, "Drive"));
                    group(ui, "Percussion", |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                pane.switch(ui, 9, "Percussion");
                                pane.seg(ui, 10, "Harmonic");
                            });
                            ui.vertical(|ui| {
                                pane.seg(ui, 46, "Time");
                                pane.seg(ui, 47, "Volume");
                            });
                        });
                    });
                });
            }
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        rotation(ui, pane);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (k, n) in [(2usize, "Split"), (1, "Preset"), (0, "Control")] {
                if ui.add(egui::Button::selectable(*lower == k, egui::RichText::new(n).size(11.0))).clicked() {
                    *lower = k;
                }
            }
        });
    });
}

fn cabinet_page(ui: &mut egui::Ui, pane: &mut Pane) {
    let cab = pane.get(80).round() as usize;
    let real = cab == 0;
    ui.horizontal_top(|ui| {
        // the cabinet with its microphones
        let (r, _) = ui.allocate_exact_size(vec2(230.0, 250.0), Sense::hover());
        let p = ui.painter().clone();
        let wood = matches!(cab, 0 | 1 | 5);
        let body = Rect::from_center_size(r.center(), vec2(130.0, 200.0));
        p.rect_filled(body, 6.0, if wood { Color32::from_rgb(0x5a, 0x3a, 0x22) } else { Color32::from_gray(0x2a) });
        let horn = pos2(body.center().x, body.min.y + 52.0);
        let drum = pos2(body.center().x, body.max.y - 58.0);
        for k in 0..4 {
            let y = body.min.y + 26.0 + k as f32 * 16.0;
            p.line_segment([pos2(body.min.x + 14.0, y), pos2(body.max.x - 14.0, y)], Stroke::new(5.0, Color32::from_black_alpha(120)));
        }
        if cab != 3 {
            p.circle_filled(drum, 34.0, Color32::from_black_alpha(140));
            p.circle_stroke(drum, 34.0, Stroke::new(1.5, Color32::from_gray(0x70)));
        }
        p.text(pos2(body.center().x, body.max.y + 12.0), Align2::CENTER_CENTER, organ::CABINETS[cab.min(7)], look::regular(11.0), look::text_dim());
        // microphone icons: with the Real Cabinet a click chooses the type
        for (k, (at, id)) in [(horn, 88u32), (drum, 89)].into_iter().enumerate() {
            for side in [-1.0f32, 1.0] {
                let mr = Rect::from_center_size(pos2(at.x + side * 92.0, at.y), vec2(24.0, 24.0));
                let resp = ui.interact(mr, egui::Id::unique(("b3-mic", pane.t, k, side as i32)), Sense::click());
                crate::glyphs::paint(&p, mr, crate::glyphs::ph::MICROPHONE, 18.0, if real && resp.hovered() { look::accent() } else { look::text_dim() });
                if real {
                    let cur = pane.get(id).round() as usize;
                    egui::Popup::menu(&resp).show(|ui| {
                        for (m, n) in organ::MICS.iter().enumerate() {
                            if ui.add(egui::Button::selectable(m == cur, *n)).clicked() {
                                pane.put(id, m as f32);
                                ui.close();
                            }
                        }
                    });
                }
            }
        }
        ui.vertical(|ui| {
            ui.horizontal_top(|ui| {
                group(ui, "Cabinet", |ui| {
                    let on = pane.get(18) >= 0.5;
                    if keyswin::toggle(ui, on, "Cabinet") {
                        pane.put(18, if on { 0.0 } else { 2.0 });
                    }
                    pane.menu(ui, 80, "Type");
                    pane.switch(ui, 81, "Deflector");
                });
                group(ui, "Motor", |ui| {
                    pane.knobs(ui, &[(23, "Acceleration"), (82, "Max Rate")]);
                    pane.menu(ui, 83, "Motor Control");
                    pane.menu(ui, 27, "Speed MIDI Control");
                });
                group(ui, "Brake", |ui| {
                    pane.knobs(ui, &[(84, "Dry Level"), (86, "Horn Pos"), (87, "Drum Pos")]);
                    pane.seg(ui, 85, "Output");
                });
            });
            ui.add_space(4.0);
            group(ui, "Microphones", |ui| {
                pane.seg(ui, 90, "Position");
                if real {
                    ui.horizontal(|ui| {
                        pane.knob(ui, 24, "Horn");
                        pane.knob(ui, 91, "Drum");
                        ui.vertical(|ui| {
                            pane.menu(ui, 88, "Horn Mic");
                            pane.menu(ui, 89, "Drum Mic");
                        });
                    });
                } else {
                    pane.knobs(ui, &[(92, "Distance"), (93, "Angle"), (94, "Balance")]);
                }
            });
        });
    });
    ui.add_space(4.0);
    rotation(ui, pane);
}

fn options_page(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.horizontal_top(|ui| {
        group(ui, "Master", |ui| pane.knobs(ui, &[(54, "Tune"), (25, "Volume"), (55, "Expression")]));
        group(ui, "Click", |ui| pane.knobs(ui, &[(56, "Key On"), (57, "Key Off"), (58, "Pedal"), (59, "Velocity")]));
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        group(ui, "Percussion", |ui| {
            ui.horizontal(|ui| {
                pane.seg(ui, 50, "Mode");
                pane.seg(ui, 51, "Perc on Preset");
            });
            pane.knobs(ui, &[(48, "Time Slow"), (11, "Time Fast"), (49, "Vol Soft"), (12, "Vol Normal"), (52, "Upper Level"), (53, "Velocity")]);
        });
        group(ui, "Scanner Vibrato", |ui| {
            pane.seg(ui, 43, "Mode");
            pane.knobs(ui, &[(44, "Rate"), (45, "Depth")]);
        });
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        group(ui, "Morph", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 72, "Select via Keyboard");
                pane.seg(ui, 73, "Mode");
            });
            ui.horizontal(|ui| {
                pane.menu(ui, 74, "Range");
                pane.controller_menu(ui, 76, "MIDI Controller");
            });
            // Linear mode: the blended registration can be kept on a preset key
            ui.add_enabled_ui(pane.on(73), |ui| {
                ui.menu_button("Save Morph To ⌄", |ui| {
                    for k in (1..12usize).rev() {
                        if ui.button(KEYS[k]).clicked() {
                            for d in 0..9u32 {
                                let v = pane.get(UPPER + d).round();
                                pane.put(PRESETS_U + k as u32 * 9 + d, v);
                            }
                            ui.close();
                        }
                    }
                });
            });
        });
        group(ui, "Preset", |ui| pane.menu(ui, 77, "Edit Preset Key"));
    });
}

fn effects_page(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.horizontal(|ui| {
        pane.switch(ui, 100, "Effects");
        ui.add_space(12.0);
        // the chain: drag a name onto another to change the order of EQ, Wah and Distortion
        let chain = pane.get(118).round() as usize;
        let orders: [[&str; 3]; 4] = [["EQ", "Wah", "Dist"], ["EQ", "Dist", "Wah"], ["Wah", "Dist", "EQ"], ["Dist", "EQ", "Wah"]];
        let names = orders[chain.min(3)];
        let drag_id = egui::Id::unique(("b3-chain-drag", pane.t));
        let mut dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id));
        let mut drop: Option<(usize, usize)> = None;
        for (i, n) in names.iter().enumerate() {
            let resp = ui.add(egui::Button::new(egui::RichText::new(*n).size(11.0)).sense(Sense::click_and_drag())).on_hover_text("Drag to reorder the effects");
            if resp.drag_started() {
                dragging = Some(i);
            }
            if let Some(from) = dragging {
                if ui.input(|x| x.pointer.any_released()) && resp.contains_pointer() && from != i {
                    drop = Some((from, i));
                }
            }
            if i < 2 {
                ui.label("›");
            }
        }
        if ui.input(|x| x.pointer.any_released()) {
            dragging = None;
        }
        ui.data_mut(|d| match dragging {
            Some(v) => {
                d.insert_temp(drag_id, v);
            }
            None => d.remove::<usize>(drag_id),
        });
        if let Some((from, to)) = drop {
            let mut want: Vec<&str> = names.to_vec();
            let x = want.remove(from);
            want.insert(to, x);
            // (the four orders the organ has; the nearest one starting the same way otherwise)
            let k = orders.iter().position(|o| o[..] == want[..]).or_else(|| orders.iter().position(|o| o[0] == want[0])).unwrap_or(chain);
            pane.put(118, k as f32);
        }
        ui.label("›  Reverb");
        ui.add_space(12.0);
        pane.menu(ui, 118, "");
        pane.seg(ui, 119, "Pedal");
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        group(ui, "EQ", |ui| {
            pane.switch(ui, 101, "On");
            pane.knobs(ui, &[(102, "Low"), (103, "Mid"), (104, "High"), (105, "Level")]);
        });
        group(ui, "Wah", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 106, "On");
                pane.menu(ui, 107, "Type");
            });
            pane.knobs(ui, &[(108, "Range"), (109, "Bite")]);
            pane.controller_menu(ui, 110, "Sweep MIDI Ctrl");
        });
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        group(ui, "Distortion", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 111, "On");
                pane.menu(ui, 112, "Type");
            });
            pane.knobs(ui, &[(113, "Tone"), (17, "Drive")]);
        });
        group(ui, "Chorus", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 41, "Upper");
                pane.switch(ui, 42, "Lower");
            });
            pane.menu(ui, 15, "Type");
            pane.knobs(ui, &[(44, "Rate"), (45, "Depth")]);
        });
        group(ui, "Reverb", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 114, "On");
                pane.seg(ui, 117, "");
            });
            pane.menu(ui, 115, "Type");
            pane.knob(ui, 116, "Level");
        });
    });
}

fn expert_page(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            group(ui, "Pitch", |ui| {
                for (id, n) in [(120u32, "Upper Stretch"), (121, "Lower Stretch"), (122, "Warmth"), (28, "Pitchbend Up"), (123, "Pitchbend Down")] {
                    pane.slider(ui, id, n);
                }
            });
            ui.add_space(4.0);
            group(ui, "Sustain", |ui| {
                for (id, n) in [(124u32, "Upper Manual"), (125, "Lower Manual"), (126, "Pedals")] {
                    pane.slider(ui, id, n);
                }
                pane.seg(ui, 127, "Mode");
            });
            ui.add_space(4.0);
            group(ui, "Misc", |ui| pane.menu(ui, 139, "Hardware Controller"));
        });
        ui.vertical(|ui| {
            group(ui, "Condition", |ui| {
                for (id, n) in [(13u32, "Click Minimum"), (14, "Click Maximum"), (60, "Click Color"), (128, "Filter Age"), (16, "Leakage"), (129, "Drawbar Leak"), (130, "Crosstalk"), (131, "Random FM")] {
                    pane.slider(ui, id, n);
                }
            });
            ui.add_space(4.0);
            group(ui, "Organ", |ui| {
                for (id, n) in [(132u32, "Maximum Wheels"), (26, "Tonal Balance"), (133, "Lower Volume"), (134, "Pedals Volume"), (135, "Shape"), (136, "Bass Filter")] {
                    pane.slider(ui, id, n);
                }
                ui.horizontal(|ui| {
                    pane.switch(ui, 137, "Extended Bass");
                    pane.switch(ui, 138, "Ultrabass");
                });
            });
        });
    });
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let mut pane = Pane::new(t, &params, &vals);
    let tab_id = egui::Id::unique(("b3-tab", t));
    let lower_id = egui::Id::unique(("b3-lower", t));
    let mut tab: usize = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(0);
    let mut lower: usize = ctx.data(|d| d.get_temp(lower_id)).unwrap_or(0);
    let mut open = true;
    let resp = egui::Window::new("Vintage B3").id(egui::Id::unique(("b3-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(760.0, 470.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Vintage B3 Organ");
        keyswin::tabs(ui, &mut tab, &["Main", "Rotor Cabinet", "Options", "Effects", "Expert"]);
        ui.add_space(4.0);
        match tab {
            1 => cabinet_page(ui, &mut pane),
            2 => options_page(ui, &mut pane),
            3 => effects_page(ui, &mut pane),
            4 => expert_page(ui, &mut pane),
            _ => main_page(ui, &mut pane, &mut lower),
        }
    });
    let audition_id = egui::Id::unique(("b3-audition", t));
    if let Some(n) = ctx.data_mut(|d| d.remove_temp::<u8>(audition_id)) {
        keyswin::audition(app, t, n, 100);
    }
    ctx.data_mut(|d| {
        d.insert_temp(tab_id, tab);
        d.insert_temp(lower_id, lower);
    });
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Vintage B3 Setting", open);
}
