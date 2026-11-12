//! The Drum Kit Designer window, as Logic's: the kit drawn as a stage (kick, snare, toms, hi-hat,
//! crash and ride cymbals, percussion), a click on a piece plays and selects it; the selected
//! piece's Tune, Damp and Gain (and the piece itself: the kit's own, or another kit's), the kit
//! menu, the kit-wide settings, and the Mapping view: which notes play which piece (Drum Kit
//! Designer's General MIDI map with the hi-hat articulations below C1).
//!
//! Logic's Producer Kits (extra pieces and a mic mixer on several outputs) aren't in Studio: its
//! kits play on one stereo output.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::instwin;
use crate::look;

/// A piece: name, mixer group (0 kick … 5 percussion), its note, where it stands (0..1 of the
/// stage) and its size.
const PIECES: &[(&str, usize, u8, (f32, f32), f32)] = &[
    ("Kick", 0, 36, (0.5, 0.72), 0.17),
    ("Snare", 1, 38, (0.3, 0.6), 0.09),
    ("Tom 1", 2, 48, (0.42, 0.38), 0.07),
    ("Tom 2", 2, 45, (0.58, 0.38), 0.075),
    ("Floor Tom", 2, 41, (0.73, 0.62), 0.1),
    ("Hi-Hat", 3, 42, (0.13, 0.48), 0.08),
    ("Crash", 4, 49, (0.25, 0.22), 0.1),
    ("Ride", 4, 51, (0.82, 0.3), 0.11),
    ("Percussion", 5, 54, (0.92, 0.75), 0.06),
];

/// Per group: (piece choice, tune, damp, gain) parameter ids of the Studio Drum Kit.
const GROUP_IDS: [(u32, u32, u32, u32); 6] = [(24, 3, 4, 2), (25, 6, 7, 5), (26, 10, 11, 9), (27, 30, 14, 12), (28, 31, 16, 15), (29, 32, 33, 17)];
const GROUP_NAMES: [&str; 6] = ["Kick", "Snare", "Toms", "Hi-Hat", "Cymbals", "Percussion"];

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let get = |pid: u32| params.iter().position(|x| x.id == pid).map(|i| vals[i]).unwrap_or(0.0);
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let sel_id = egui::Id::unique(("dkd-piece", t));
    let view_id = egui::Id::unique(("dkd-view", t));
    let mut sel: usize = ctx.data(|d| d.get_temp(sel_id)).unwrap_or(1);
    let mut mapping: bool = ctx.data(|d| d.get_temp(view_id)).unwrap_or(false);
    let mut set: Vec<(u32, f32)> = Vec::new();
    let mut play: Option<u8> = None;
    let mut open = true;
    let resp = egui::Window::new(format!("{track} — Drum Kit Designer")).id(egui::Id::unique(("dkd-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(820.0, 560.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let kit = get(0).round() as usize;
            ui.menu_button(format!("{} ⌄", lg_synth::drums::KITS.get(kit).copied().unwrap_or("")), |ui| {
                for (k, n) in lg_synth::drums::KITS.iter().enumerate() {
                    if ui.add(egui::Button::selectable(k == kit, *n)).clicked() {
                        set.push((0, k as f32));
                        ui.close();
                    }
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::selectable(mapping, "Mapping")).clicked() {
                    mapping = true;
                }
                if ui.add(egui::Button::selectable(!mapping, "Kit")).clicked() {
                    mapping = false;
                }
            });
        });
        if mapping {
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                egui::Grid::new("dkd-map").num_columns(3).striped(true).show(ui, |ui| {
                    ui.label(egui::RichText::new("Note").strong());
                    ui.label(egui::RichText::new("Name").strong());
                    ui.label(egui::RichText::new("Piece").strong());
                    ui.end_row();
                    for n in 24..=87u8 {
                        let Some(g) = lg_synth::drums::piece_of(n) else { continue };
                        ui.label(lg_synth::sampler_doc::note_name(n));
                        ui.label(gm_name(n));
                        ui.label(GROUP_NAMES[g as usize]);
                        ui.end_row();
                    }
                });
            });
            return;
        }
        // the stage
        let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width() - 250.0, 360.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(stage, 8.0, Color32::from_rgb(0x1d, 0x1b, 0x22));
        let at = |(x, y): (f32, f32)| pos2(stage.min.x + x * stage.width(), stage.min.y + y * stage.height());
        for (i, (name, g, note, pos, size)) in PIECES.iter().enumerate() {
            let c = at(*pos);
            let rad = size * stage.height();
            let cymbal = *g == 4 || *g == 3;
            let r = Rect::from_center_size(c, vec2(rad * 2.0, rad * 2.0));
            let resp = ui.interact(r, ui.scope_id().with(("piece", i)), Sense::click());
            let fill = if cymbal { Color32::from_rgb(0xc9, 0xa2, 0x4a) } else { Color32::from_rgb(0xdd, 0xd8, 0xcc) };
            let fill = if resp.hovered() { look::mix(fill, Color32::WHITE, 0.2) } else { fill };
            p.circle_filled(c, rad, fill);
            p.circle_stroke(c, rad, Stroke::new(if sel == *g { 3.0 } else { 1.5 }, if sel == *g { look::accent() } else { Color32::from_gray(0x50) }));
            p.text(c + vec2(0.0, rad + 8.0), Align2::CENTER_CENTER, *name, look::regular(11.0), look::text());
            if resp.clicked() {
                sel = *g;
                play = Some(*note);
            }
        }
        // the selected piece
        let side = Rect::from_min_max(pos2(stage.max.x + 10.0, stage.min.y), pos2(ui.max_rect().max.x, stage.max.y));
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(side));
        let (pc, tu, dm, gn) = GROUP_IDS[sel];
        cell.label(egui::RichText::new(GROUP_NAMES[sel]).strong().size(15.0));
        let cur = get(pc).round() as usize;
        cell.horizontal(|ui| {
            ui.label(egui::RichText::new("Piece").color(look::text_dim()));
            ui.menu_button(format!("{} ⌄", lg_synth::drums::PIECE_KITS.get(cur).copied().unwrap_or("")), |ui| {
                for (k, n) in lg_synth::drums::PIECE_KITS.iter().enumerate() {
                    if ui.add(egui::Button::selectable(k == cur, if k == 0 { "The kit's own" } else { *n })).clicked() {
                        set.push((pc, k as f32));
                        ui.close();
                    }
                }
            });
        });
        let knobs = [(tu, "Tune"), (dm, "Damp"), (gn, "Gain")];
        let (kr, _) = cell.allocate_exact_size(vec2(220.0, 90.0), Sense::hover());
        for (k, (pid, label)) in knobs.iter().enumerate() {
            let Some(prm) = params.iter().find(|x| x.id == *pid).cloned() else { continue };
            let r = Rect::from_min_size(pos2(kr.min.x + k as f32 * 72.0, kr.min.y), vec2(66.0, 84.0));
            let mut v = get(*pid);
            // Damp: less decay to the right
            let shown = |x: f32| if *label == "Damp" { format!("{:.0} %", (1.0 - (x - 0.2) / 2.8) * 100.0) } else if *label == "Gain" { format!("{x:.1} dB") } else { format!("{x:+.1}") };
            if crate::widgets::knob(&mut cell, r, egui::Id::unique(("dkd-knob", t, pid)), &mut v, prm.min, prm.max, prm.default, label, &shown) {
                set.push((*pid, v));
            }
        }
        if sel == 1 {
            let mut v = get(8);
            cell.horizontal(|ui| {
                ui.label(egui::RichText::new("Snare wires").color(look::text_dim()));
                if ui.add(egui::Slider::new(&mut v, 0.0..=2.0)).changed() {
                    set.push((8, v));
                }
            });
        }
        if sel == 3 {
            let mut v = get(13);
            cell.horizontal(|ui| {
                ui.label(egui::RichText::new("Hi-hat tone").color(look::text_dim()));
                if ui.add(egui::Slider::new(&mut v, -1.0..=1.0)).changed() {
                    set.push((13, v));
                }
            });
        }
        cell.separator();
        cell.label(egui::RichText::new("Kit").strong());
        for (pid, label, min, max) in [(1u32, "Volume", -40.0f32, 12.0f32), (18, "Room", 0.0, 1.0), (19, "Velocity Sensitivity", 0.0, 1.0), (20, "Variation", 0.0, 1.0), (21, "Stereo Width", 0.0, 1.0), (22, "Tune", -12.0, 12.0), (23, "Punch", 0.0, 1.0)] {
            let mut v = get(pid);
            cell.horizontal(|ui| {
                ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
                if ui.add(egui::Slider::new(&mut v, min..=max)).changed() {
                    set.push((pid, v));
                }
            });
        }
        // the disclosure arrow at the lower left: Input Mapping and the extra pieces' gains
        crate::keyswin::extended(&mut cell, egui::Id::unique(("dkd-ext", t)), |ui| {
            use lg_synth::drums::{kit_has_extra, EXTRA_GAIN, INPUT_MAPPING, INPUT_MAPPINGS};
            let cur = get(INPUT_MAPPING).round().max(0.0) as usize;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Input Mapping").size(11.0).color(look::text_dim()));
                egui::ComboBox::from_id_salt(("dkd-mapping", t)).selected_text(INPUT_MAPPINGS.get(cur).copied().unwrap_or("GM")).width(250.0).show_ui(ui, |ui| {
                    for (k, n) in INPUT_MAPPINGS.iter().enumerate() {
                        if ui.selectable_label(k == cur, *n).clicked() && k != cur {
                            set.push((INPUT_MAPPING, k as f32));
                        }
                    }
                });
            });
            let kit = get(0).round() as u8;
            for (k, label) in ["Shaker Gain", "Tambourine Gain", "Claps Gain", "Cowbell Gain", "Sticks Gain"].iter().enumerate() {
                let id = EXTRA_GAIN + k as u32;
                let mut v = get(id);
                // (a slider whose sound the kit doesn't have is dimmed: it would do nothing)
                ui.add_enabled_ui(kit_has_extra(kit, k), |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(100.0, 16.0), egui::Label::new(egui::RichText::new(*label).size(11.0).color(look::text_dim())));
                        if ui.add(egui::Slider::new(&mut v, -24.0..=12.0).suffix(" dB")).changed() {
                            set.push((id, v));
                        }
                    });
                });
            }
        });
    });
    ctx.data_mut(|d| {
        d.insert_temp(sel_id, sel);
        d.insert_temp(view_id, mapping);
    });
    app.inst.rect = resp.map(|r| r.response.rect);
    if !set.is_empty() {
        instwin::checkpoint_once(app, ctx, t, "Drum Kit Setting");
        instwin::set_params(app, t, &set);
    }
    if let Some(n) = play {
        let engine = app.engine.clone();
        if let Ok(mut e) = engine.lock() {
            e.live(t, n, 110);
        }
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            if let Ok(mut e) = engine.lock() {
                e.live(t, n, 0);
            }
        });
    }
    if !open {
        app.plugin_window = None;
        app.inst.rect = None;
    }
}

fn gm_name(n: u8) -> &'static str {
    match n {
        33 => "Hi-Hat Semi-Open",
        34 => "Hi-Hat Semi-Open 2",
        35 => "Kick 2",
        36 => "Kick",
        37 => "Side Stick",
        38 => "Snare",
        39 => "Clap",
        40 => "Snare Rimshot",
        41 => "Low Floor Tom",
        42 => "Hi-Hat Closed",
        43 => "High Floor Tom",
        44 => "Hi-Hat Pedal",
        45 => "Low Tom",
        46 => "Hi-Hat Open",
        47 => "Low-Mid Tom",
        48 => "Hi-Mid Tom",
        49 => "Crash 1",
        50 => "High Tom",
        51 => "Ride",
        52 => "China",
        53 => "Ride Bell",
        54 => "Tambourine",
        55 => "Splash",
        56 => "Cowbell",
        57 => "Crash 2",
        59 => "Ride 2",
        60..=87 => "Percussion",
        _ => "Hi-Hat",
    }
}
