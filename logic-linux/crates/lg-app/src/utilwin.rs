//! The Utility instruments' windows (the instrument slot's Utility submenu): Klopfgeist and Test
//! Oscillator as an instrument. Klopfgeist's window, as Logic's: Trigger Mode (Mono / Poly), the
//! Tune and Detune knobs, the Tonality and Damp sliders, and Level via Vel — two handles for the
//! levels at the lowest and the highest velocity; dragging the bar between them moves both.
//! Test Oscillator: its two modes are chosen by clicking their labels (Test Tone, Sine Sweep);
//! the Frequency field takes 1 Hz – 22 kHz when typed (the knob covers 20 Hz – 20 kHz), and in
//! Sine Sweep mode a field under Start / End Frequency shows the sweep's frequency as it runs.
//! External Instrument: MIDI Destination and MIDI Channel (the track's External settings, which
//! send the track's MIDI out), Input (the module's audio, through the side-chain input) and
//! Input Volume, and Send Program Change with the Program, Bank MSB and Bank LSB pop-ups (sent
//! when changed and when the project loads, with the track's MIDI strip settings).

use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};
use lg_synth::utility::{self as ut, ExternalInstrument, Klopfgeist, TestOscInstrument};

use crate::app::App;
use crate::instwin;
use crate::keyswin::{self, group, Pane};
use crate::look;

/// Which Utility window the track's instrument gets.
#[derive(Clone, Copy)]
pub enum Kind {
    Klopfgeist,
    TestOsc,
    External,
}

pub fn kind_of(any: &dyn std::any::Any) -> Option<Kind> {
    if any.is::<Klopfgeist>() {
        Some(Kind::Klopfgeist)
    } else if any.is::<TestOscInstrument>() {
        Some(Kind::TestOsc)
    } else if any.is::<ExternalInstrument>() {
        Some(Kind::External)
    } else {
        None
    }
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize, k: Kind) {
    match k {
        Kind::Klopfgeist => klopfgeist(app, ctx, t),
        Kind::TestOsc => test_osc(app, ctx, t),
        Kind::External => external(app, ctx, t),
    }
}

/// The track plays through External Instrument (its strip is an instrument channel strip).
pub fn ext_instrument(app: &App, t: usize) -> bool {
    app.instrument_choice.get(&t).is_some_and(|c| c == ut::EXTERNAL_INSTRUMENT)
}

/// An instrument was chosen for track `t` (slot menu, New Tracks): External Instrument makes the
/// track send its MIDI out (External settings) and listen to the audio input; choosing another
/// instrument afterwards turns that off.
pub fn picked(app: &mut App, t: usize, choice: Option<&str>) {
    use lg_core::trackmix::External;
    if choice == Some(ut::EXTERNAL_INSTRUMENT) {
        app.instrument_choice.insert(t, ut::EXTERNAL_INSTRUMENT.into());
        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
            let e = tr.extra.set.external.get_or_insert_with(|| External { channel: 1, ..Default::default() });
            e.use_plugin = true;
        }
        crate::sidechain::set(app, t, Some(crate::sidechain::Choice::Input));
    } else if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        if tr.extra.set.external.as_ref().is_some_and(|e| e.use_plugin) && tr.extra.set.default_region != lg_core::trackmix::RegionType::Pattern {
            tr.extra.set.external = None;
            crate::sidechain::set(app, t, None);
        }
    }
}

/// Program and bank select to the module (Send Program Change on).
fn send_program(app: &App, e: &lg_core::trackmix::External) {
    if let Some(pg) = e.program {
        let ch = e.channel.saturating_sub(1) & 15;
        crate::trackops::midi_send(app, &e.port, &[0xb0 | ch, 0, e.bank.0.min(127)]);
        crate::trackops::midi_send(app, &e.port, &[0xb0 | ch, 32, e.bank.1.min(127)]);
        crate::trackops::midi_send(app, &e.port, &[0xc0 | ch, pg.min(127)]);
    }
}

fn number_menu(ui: &mut egui::Ui, id: (&str, usize), label: &str, v: u8, names: Option<&[&str]>) -> Option<u8> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 16.0), egui::Label::new(egui::RichText::new(label).size(11.0).color(look::text_dim())));
        let shown = |n: u8| names.and_then(|l| l.get(n as usize)).map_or(n.to_string(), |s| format!("{n} {s}"));
        egui::ComboBox::from_id_salt(id).selected_text(shown(v)).width(200.0).height(300.0).show_ui(ui, |ui| {
            for n in 0..=127u8 {
                if ui.selectable_label(n == v, shown(n)).clicked() {
                    out = Some(n);
                }
            }
        });
    });
    out
}

fn external(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let Some(e) = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.extra.set.external.clone()) else { return };
    let ports = {
        let mut v = vec![(String::new(), crate::midiout::VIRTUAL.to_string())];
        for p in app.hub.outs.ports() {
            if p != crate::midiout::VIRTUAL {
                v.push((p.clone(), p));
            }
        }
        v
    };
    let mut pane = Pane::new(t, &params, &vals);
    let mut ne = e.clone();
    let mut send = false;
    let mut open = true;
    let resp = egui::Window::new("External Instrument").id(egui::Id::unique(("extinst-window", t))).open(&mut open).collapsible(false).resizable(false).default_size(vec2(440.0, 260.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "External Instrument");
        ui.add_space(4.0);
        group(ui, "MIDI", |ui| {
            ui.set_min_width(400.0);
            ui.horizontal(|ui| {
                ui.add_sized(vec2(110.0, 16.0), egui::Label::new(egui::RichText::new("MIDI Destination").size(11.0).color(look::text_dim())));
                let cur = ports.iter().find(|p| p.0 == ne.port).map_or(ne.port.clone(), |p| p.1.clone());
                egui::ComboBox::from_id_salt(("extinst-port", t)).selected_text(cur).width(200.0).show_ui(ui, |ui| {
                    for (k, label) in &ports {
                        if ui.selectable_label(*k == ne.port, label).clicked() {
                            ne.port = k.clone();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add_sized(vec2(110.0, 16.0), egui::Label::new(egui::RichText::new("MIDI Channel").size(11.0).color(look::text_dim())));
                egui::ComboBox::from_id_salt(("extinst-ch", t)).selected_text(ne.channel.clamp(1, 16).to_string()).width(60.0).show_ui(ui, |ui| {
                    for c in 1..=16u8 {
                        ui.selectable_value(&mut ne.channel, c, c.to_string());
                    }
                });
            });
        });
        group(ui, "Audio", |ui| {
            ui.set_min_width(400.0);
            pane.menu(ui, ut::EI_INPUT, "Input");
            pane.slider(ui, ut::EI_VOLUME, "Input Volume");
        });
        group(ui, "Program", |ui| {
            ui.set_min_width(400.0);
            let mut on = ne.program.is_some();
            if ui.checkbox(&mut on, "Send Program Change").changed() {
                ne.program = on.then_some(ne.program.unwrap_or(0));
                send = on;
            }
            ui.add_enabled_ui(on, |ui| {
                if let Some(v) = number_menu(ui, ("extinst-pg", t), "Program", ne.program.unwrap_or(0), Some(&lg_core::environment::GM_PROGRAMS[..])) {
                    ne.program = Some(v);
                    send = true;
                }
                if let Some(v) = number_menu(ui, ("extinst-msb", t), "Bank MSB", ne.bank.0, None) {
                    ne.bank.0 = v;
                    send = true;
                }
                if let Some(v) = number_menu(ui, ("extinst-lsb", t), "Bank LSB", ne.bank.1, None) {
                    ne.bank.1 = v;
                    send = true;
                }
            });
        });
    });
    if ne != e {
        app.checkpoint("External Instrument");
        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
            tr.extra.set.external = Some(ne.clone());
        }
        app.dirty = true;
        if send {
            send_program(app, &ne);
        }
    }
    let set = std::mem::take(&mut pane.set);
    // the Input menu: the module's audio through the side-chain input, or none
    if let Some(&(_, v)) = set.iter().rev().find(|x| x.0 == ut::EI_INPUT) {
        crate::sidechain::set(app, t, (v >= 0.5).then_some(crate::sidechain::Choice::Input));
    }
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "External Instrument Setting", open);
}

/// Level via Vel's drag: which part was grabbed (0 the min handle, 1 the max handle, 2 the bar
/// between them) and how far it moved (0..1 of the track). The bar moves both handles together
/// and stops at either end without changing their distance.
pub fn drag_range(lo: f32, hi: f32, grab: u8, delta: f32) -> (f32, f32) {
    match grab {
        0 => ((lo + delta).clamp(0.0, 1.0), hi),
        1 => (lo, (hi + delta).clamp(0.0, 1.0)),
        _ => {
            let (a, b) = (lo.min(hi), lo.max(hi));
            let d = delta.clamp(-a, 1.0 - b);
            (lo + d, hi + d)
        }
    }
}

/// The part of Level via Vel under the pointer (x as 0..1 of the track).
pub fn grab_at(lo: f32, hi: f32, x: f32, handle: f32) -> u8 {
    let (dl, dh) = ((x - lo).abs(), (x - hi).abs());
    if dl <= handle || dh <= handle {
        if dl <= dh { 0 } else { 1 }
    } else if x > lo.min(hi) && x < lo.max(hi) {
        2
    } else if dl < dh {
        0
    } else {
        1
    }
}

/// Level via Vel: the two-handled slider.
fn level_via_vel(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.label(egui::RichText::new("Level via Vel").size(11.0).color(look::text_dim()));
    let (r, resp) = ui.allocate_exact_size(vec2(200.0, 22.0), Sense::click_and_drag());
    let track = Rect::from_center_size(r.center(), vec2(r.width() - 14.0, 4.0));
    let (lo, hi) = (pane.get(ut::K_VEL_MIN), pane.get(ut::K_VEL_MAX));
    let x_of = |v: f32| track.min.x + v * track.width();
    let grab_id = egui::Id::unique(("klopf-grab", pane.t));
    if resp.drag_started() {
        if let Some(p) = resp.interact_pointer_pos() {
            let g = grab_at(lo, hi, (p.x - track.min.x) / track.width(), 7.0 / track.width());
            ui.data_mut(|d| d.insert_temp(grab_id, g));
        }
    }
    if resp.dragged() {
        let g: u8 = ui.data(|d| d.get_temp(grab_id)).unwrap_or(2);
        let (a, b) = drag_range(lo, hi, g, resp.drag_delta().x / track.width());
        if a != lo {
            pane.put(ut::K_VEL_MIN, a);
        }
        if b != hi {
            pane.put(ut::K_VEL_MAX, b);
        }
    }
    if resp.double_clicked() {
        pane.put(ut::K_VEL_MIN, 0.25);
        pane.put(ut::K_VEL_MAX, 1.0);
    }
    let p = ui.painter();
    p.rect_filled(track, 2.0, Color32::from_gray(0x2a));
    p.rect_filled(Rect::from_x_y_ranges(x_of(lo.min(hi))..=x_of(lo.max(hi)), track.y_range()), 2.0, look::accent());
    for (v, up) in [(lo, false), (hi, true)] {
        let c = pos2(x_of(v), track.center().y);
        // the min handle points down, the max handle up (as Logic's)
        let tip = if up { -7.0 } else { 7.0 };
        p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 5.0, c.y), pos2(c.x + 5.0, c.y), pos2(c.x, c.y + tip)], Color32::from_gray(0xd8), Stroke::NONE));
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("min {:.0} %", lo * 100.0)).size(10.0).color(look::text_dim()));
        ui.add_space(110.0);
        ui.label(egui::RichText::new(format!("max {:.0} %", hi * 100.0)).size(10.0).color(look::text_dim()));
    });
}

fn klopfgeist(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let mut pane = Pane::new(t, &params, &vals);
    let mut open = true;
    let resp = egui::Window::new("Klopfgeist").id(egui::Id::unique(("klopfgeist-window", t))).open(&mut open).collapsible(false).resizable(false).default_size(vec2(470.0, 220.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Klopfgeist");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            group(ui, "Trigger Mode", |ui| {
                ui.vertical(|ui| {
                    let cur = pane.get(ut::K_MODE).round() as usize;
                    for (k, n) in ["Mono", "Poly"].iter().enumerate() {
                        if ui.add(egui::Button::selectable(k == cur, egui::RichText::new(*n).size(11.0))).clicked() && k != cur {
                            pane.put(ut::K_MODE, k as f32);
                        }
                    }
                });
            });
            group(ui, "Pitch", |ui| {
                pane.knobs(ui, &[(ut::K_TUNE, "Tune"), (ut::K_DETUNE, "Detune")]);
            });
            group(ui, "Sound", |ui| {
                ui.set_min_width(220.0);
                pane.slider(ui, ut::K_TONALITY, "Tonality");
                pane.slider(ui, ut::K_DAMP, "Damp");
                level_via_vel(ui, &mut pane);
            });
        });
    });
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Klopfgeist Setting", open);
}

/// A clickable mode label (lit when chosen); true when clicked.
fn mode_label(ui: &mut egui::Ui, on: bool, name: &str) -> bool {
    let col = if on { look::accent() } else { look::text_dim() };
    ui.add(egui::Label::new(egui::RichText::new(name).size(13.0).strong().color(col)).sense(Sense::click())).clicked()
}

/// A frequency field: drag, or double-click and type (1 Hz – 22 kHz).
fn freq_field(ui: &mut egui::Ui, pane: &mut Pane, id: u32) {
    let mut v = pane.get(id);
    let r = ui.add(egui::DragValue::new(&mut v).range(1.0..=22_000.0).speed(5.0).suffix(" Hz").max_decimals(1));
    if r.changed() {
        pane.put(id, v);
    }
}

fn test_osc(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let sweep_hz = instwin::with_inst::<TestOscInstrument, _>(app, t, |i| i.sweep_hz()).unwrap_or(0.0);
    let mut pane = Pane::new(t, &params, &vals);
    let mut open = true;
    let gen_ = pane.get(ut::TO_GENERATOR).round() as usize;
    let sweep = gen_ == 5;
    let last_id = egui::Id::unique(("testosc-last-tone", t));
    let resp = egui::Window::new("Test Oscillator").id(egui::Id::unique(("testosc-window", t))).open(&mut open).collapsible(false).resizable(false).default_size(vec2(460.0, 260.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Test Oscillator");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            group(ui, "", |ui| {
                ui.set_min_width(200.0);
                if mode_label(ui, !sweep, "Test Tone") && sweep {
                    let last: usize = ui.data(|d| d.get_temp(last_id)).unwrap_or(0);
                    pane.put(ut::TO_GENERATOR, last as f32);
                }
                if !sweep {
                    ui.data_mut(|d| d.insert_temp(last_id, gen_));
                }
                ui.add_enabled_ui(!sweep, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (k, n) in ["Sine", "Square", "Needle Pulse", "White Noise", "Pink Noise"].iter().enumerate() {
                            if ui.add(egui::Button::selectable(k == gen_, egui::RichText::new(*n).size(11.0))).clicked() {
                                pane.put(ut::TO_GENERATOR, k as f32);
                            }
                        }
                    });
                    pane.switch(ui, 1001, "Decorrelated");
                    pane.switch(ui, 1002, "Anti Aliased");
                    let (kr, _) = ui.allocate_exact_size(vec2(64.0, 74.0), Sense::hover());
                    let mut f = pane.get(1003).clamp(20.0, 20_000.0);
                    if crate::widgets::knob(ui, kr, egui::Id::unique(("testosc-freq", t)), &mut f, 20.0, 20_000.0, 1000.0, "Frequency", &|x| format!("{x:.0} Hz")) {
                        pane.put(1003, f);
                    }
                    freq_field(ui, &mut pane, 1003);
                });
            });
            group(ui, "", |ui| {
                ui.set_min_width(200.0);
                if mode_label(ui, sweep, "Sine Sweep") && !sweep {
                    pane.put(ut::TO_GENERATOR, 5.0);
                }
                ui.add_enabled_ui(sweep, |ui| {
                    pane.slider(ui, 1006, "Time");
                    pane.menu(ui, 1007, "Sweep Mode");
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Start").size(11.0).color(look::text_dim()));
                        freq_field(ui, &mut pane, 1008);
                        ui.label(egui::RichText::new("End").size(11.0).color(look::text_dim()));
                        freq_field(ui, &mut pane, 1009);
                    });
                    // the sweep's frequency now (Logic's Frequency field under Start / End)
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Frequency").size(11.0).color(look::text_dim()));
                        let shown = if pane.on(ut::TO_TRIGGER) { format!("{sweep_hz:.0} Hz") } else { "—".into() };
                        ui.label(egui::RichText::new(shown).size(12.0).monospace());
                    });
                    pane.menu(ui, 1010, "Trigger");
                    let on = pane.on(ut::TO_TRIGGER);
                    if ui.add(egui::Button::selectable(on, "Trigger")).clicked() {
                        pane.put(ut::TO_TRIGGER, if on { 0.0 } else { 1.0 });
                    }
                });
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            pane.slider(ui, 1004, "Level");
            pane.switch(ui, 1005, "Dim");
        });
        if sweep && pane.on(ut::TO_TRIGGER) {
            ui.ctx().request_repaint();
        }
    });
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Test Oscillator Setting", open);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_via_vel_bar_moves_both_handles() {
        assert_eq!(drag_range(0.2, 0.6, 2, 0.1), (0.3, 0.70000005));
        // stops at the end, keeping the distance
        let (a, b) = drag_range(0.2, 0.6, 2, 0.9);
        assert!((b - 1.0).abs() < 1e-6 && (b - a - 0.4).abs() < 1e-6);
        assert_eq!(drag_range(0.2, 0.6, 0, -0.5), (0.0, 0.6));
        assert_eq!(drag_range(0.2, 0.6, 1, 0.1).0, 0.2);
    }

    #[test]
    fn level_via_vel_grab() {
        assert_eq!(grab_at(0.2, 0.6, 0.21, 0.03), 0);
        assert_eq!(grab_at(0.2, 0.6, 0.6, 0.03), 1);
        assert_eq!(grab_at(0.2, 0.6, 0.4, 0.03), 2);
        assert_eq!(grab_at(0.2, 0.6, 0.9, 0.03), 1);
    }
}
