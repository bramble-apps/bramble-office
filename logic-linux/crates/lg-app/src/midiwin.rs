//! The MIDI Transform window (Functions ▸ MIDI Transform, ⌘9) and the MIDI sheets: Repeat
//! Multiple, Change Length, Copy MIDI Events. Their commands are midifunc.rs's; this file only
//! draws them (kept apart so its button texts aren't read as command names).

use lg_core::events::PPQ;
use lg_core::functions as f;
use lg_core::transform::{self, Cond, Op};

use crate::app::App;
use crate::midifunc::{edit, run_transform, save_user_presets, Sheet};

/// The note values the sheets offer (ticks).
const NOTE_VALUES: [(&str, u32); 8] = [("1/1", 3840), ("1/2", 1920), ("1/4", 960), ("1/8", 480), ("1/16", 240), ("1/32", 120), ("1/8 triplet", 320), ("1/16 triplet", 160)];

/// Draws the MIDI Transform window and the open sheet (every frame).
pub fn windows(app: &mut App, ctx: &egui::Context) {
    transform_window(app, ctx);
    sheet(app, ctx);
}

fn value_drag(ui: &mut egui::Ui, v: &mut i64, param: usize) {
    let d = egui::DragValue::new(v).speed(if param == 0 || param == 3 { 10.0 } else { 0.5 });
    let d = match param {
        1 => d.range(0..=127).custom_formatter(|x, _| crate::app::note_name(x.clamp(0.0, 127.0) as u8)),
        2 => d.range(-127..=127),
        4 => d.range(-16..=16),
        _ => d,
    };
    ui.add_sized([62.0, 20.0], d);
}

fn transform_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = app.midi.transform.take() else { return };
    let mut open = true;
    let mut run: Option<u8> = None;
    let mut save_slot = None;
    let user = app.midi.user_presets.clone();
    egui::Window::new("MIDI Transform").id(egui::Id::unique("midi-transform")).open(&mut open).collapsible(false).resizable(false).default_width(560.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Preset:");
            egui::ComboBox::from_id_salt("tf-preset").selected_text(w.t.name.clone()).width(240.0).show_ui(ui, |ui| {
                for p in transform::PRESETS {
                    if let Some(t) = transform::preset(p) {
                        if ui.selectable_label(w.t.name == p, p).clicked() {
                            w.t = t;
                        }
                    } else {
                        ui.add_enabled(false, egui::Button::new(p).frame(false)).on_disabled_hover_text("Studio's regions don't keep pitch bend yet");
                    }
                }
                if user.iter().any(|u| u.is_some()) {
                    ui.separator();
                    for (k, u) in user.iter().enumerate() {
                        if let Some(t) = u {
                            if ui.selectable_label(false, format!("{} · {}", k + 1, t.name)).clicked() {
                                w.t = t.clone();
                            }
                        }
                    }
                }
            });
            ui.label(if w.roll { "on the Piano Roll's notes" } else { "on the selected regions" });
        });
        ui.add_space(6.0);
        egui::Grid::new("tf-grid").num_columns(6).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("");
            for p in transform::PARAMS {
                ui.label(egui::RichText::new(p).strong());
            }
            ui.end_row();
            ui.label("Select Conditions:");
            for p in 0..5 {
                ui.vertical(|ui| {
                    let cur = w.t.conds[p].index();
                    egui::ComboBox::from_id_salt(("tf-cond", p)).selected_text(Cond::NAMES[cur]).width(70.0).show_ui(ui, |ui| {
                        for (k, n) in Cond::NAMES.iter().enumerate() {
                            if ui.selectable_label(k == cur, *n).clicked() {
                                w.t.conds[p] = w.t.conds[p].with_index(k);
                            }
                        }
                    });
                    let (mut a, mut b) = w.t.conds[p].values();
                    let n = w.t.conds[p].arity();
                    if n >= 1 {
                        value_drag(ui, &mut a, p);
                    }
                    if n >= 2 {
                        value_drag(ui, &mut b, p);
                    }
                    w.t.conds[p].set_values(a, b);
                });
            }
            ui.end_row();
            ui.label("Operations:");
            for p in 0..5 {
                ui.vertical(|ui| {
                    let cur = w.t.ops[p].index();
                    egui::ComboBox::from_id_salt(("tf-op", p)).selected_text(Op::NAMES[cur]).width(70.0).show_ui(ui, |ui| {
                        for (k, n) in Op::NAMES.iter().enumerate() {
                            if ui.selectable_label(k == cur, *n).clicked() {
                                w.t.ops[p] = w.t.ops[p].with_index(k);
                            }
                        }
                    });
                    let (mut a, mut b) = w.t.ops[p].values();
                    let mut fac = w.t.ops[p].factor();
                    let (ints, has_f) = w.t.ops[p].shape();
                    if has_f {
                        ui.add_sized([62.0, 20.0], egui::DragValue::new(&mut fac).speed(0.01).range(0.0..=16.0).max_decimals(3));
                    }
                    if ints >= 1 {
                        value_drag(ui, &mut a, if matches!(w.t.ops[p], Op::Add(_) | Op::Sub(_) | Op::PlusMinusRand(_) | Op::Scale(..)) && p == 1 { 9 } else { p });
                    }
                    if ints >= 2 {
                        value_drag(ui, &mut b, p);
                    }
                    w.t.ops[p].set_values(a, b, fac);
                });
            }
            ui.end_row();
        });
        ui.label(egui::RichText::new("Position counts ticks from the region's start (960 a quarter note); Length in ticks.").small().weak());
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Select Only").clicked() {
                run = Some(0);
            }
            if ui.button("Operate Only").clicked() {
                run = Some(1);
            }
            if ui.button("Select and Operate").clicked() {
                run = Some(2);
            }
            ui.separator();
            ui.add(egui::DragValue::new(&mut w.slot).range(1..=30).prefix("User Preset "));
            if ui.button("Save").on_hover_text("Keep these settings as a user preset (the Apply / Select and Operate using Transform User Preset key commands run them)").clicked() {
                save_slot = Some(w.slot);
            }
        });
    });
    if let Some(k) = save_slot {
        let mut t = w.t.clone();
        if transform::PRESETS.contains(&t.name.as_str()) {
            t.name = format!("User {k}: {}", t.name);
        }
        w.t.name = t.name.clone();
        app.midi.user_presets[k - 1] = Some(t);
        save_user_presets(&app.midi.user_presets);
        app.tell(format!("Saved as Transform user preset {k}"));
    }
    if let Some(how) = run {
        let t = w.t.clone();
        app.midi.menu_roll = w.roll;
        let ok = run_transform(app, &t, how);
        app.midi.menu_roll = false;
        if !ok {
            app.tell("Select a MIDI region first");
        }
    }
    if open {
        app.midi.transform = Some(w);
    }
}

fn sheet(app: &mut App, ctx: &egui::Context) {
    let Some((mut s, roll)) = app.midi.sheet.take() else { return };
    let mut open = true;
    let mut done = false;
    let mut apply = false;
    let title = match s {
        Sheet::Repeat { .. } => "Repeat Notes",
        Sheet::Length { .. } => "Change Length",
        Sheet::Copy { .. } => "Copy MIDI Events",
    };
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    egui::Window::new(title).id(egui::Id::unique("midi-sheet")).collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        match &mut s {
            Sheet::Repeat { copies, adjust } => {
                egui::Grid::new("rep").num_columns(2).show(ui, |ui| {
                    ui.label("Number of Copies:");
                    ui.add(egui::DragValue::new(copies).range(1..=999));
                    ui.end_row();
                    ui.label("Adjustment:");
                    let names = ["Auto", "Bar", "Beat", "Division", "None"];
                    egui::ComboBox::from_id_salt("rep-adj").selected_text(names[*adjust]).show_ui(ui, |ui| {
                        for (k, n) in names.iter().enumerate() {
                            ui.selectable_value(adjust, k, *n);
                        }
                    });
                    ui.end_row();
                });
            }
            Sheet::Length { ticks } => {
                ui.horizontal(|ui| {
                    ui.label("Length:");
                    ui.add(egui::DragValue::new(ticks).range(1..=3840 * 64).speed(5.0).suffix(" ticks"));
                    ui.label(crate::app::bbdt_len(*ticks, sig));
                });
                ui.horizontal_wrapped(|ui| {
                    for (n, v) in NOTE_VALUES {
                        if ui.small_button(n).clicked() {
                            *ticks = v;
                        }
                    }
                });
            }
            Sheet::Copy { mode, left, right, dest, copies } => {
                egui::Grid::new("cme").num_columns(3).show(ui, |ui| {
                    ui.label("Mode:");
                    egui::ComboBox::from_id_salt("cme-mode").selected_text(f::CopyMode::ALL[*mode].name()).show_ui(ui, |ui| {
                        for (k, m) in f::CopyMode::ALL.iter().enumerate() {
                            ui.selectable_value(mode, k, m.name());
                        }
                    });
                    ui.label("");
                    ui.end_row();
                    for (label, v) in [("Left Locator:", &mut *left), ("Right Locator:", &mut *right), ("Destination:", &mut *dest)] {
                        ui.label(label);
                        ui.add(egui::DragValue::new(v).speed(10.0).suffix(" ticks"));
                        ui.label(crate::app::bbdt(*v, sig));
                        ui.end_row();
                    }
                    ui.label("Number of Copies:");
                    ui.add(egui::DragValue::new(copies).range(1..=99));
                    ui.label("");
                    ui.end_row();
                });
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                done = true;
            }
            if ui.button("OK").clicked() {
                apply = true;
                done = true;
            }
        });
    });
    if apply {
        app.midi.menu_roll = roll;
        match s.clone() {
            Sheet::Repeat { copies, adjust } => {
                let bar = app.bar_ticks();
                let beat = bar / sig.0.max(1) as u32;
                let round = [PPQ / 4, bar, beat, beat / 4, 1][adjust.min(4)];
                edit(app, "Repeat Notes", |notes, sel, _| {
                    f::repeat_notes(notes, sel, copies, round);
                });
            }
            Sheet::Length { ticks } => {
                edit(app, "Change Length", |notes, sel, _| f::set_lengths(notes, sel, ticks));
            }
            Sheet::Copy { mode, left, right, dest, copies } => {
                edit(app, "Copy MIDI Events", |notes, sel, _| {
                    f::copy_events(notes, sel, left, right, dest, copies, f::CopyMode::ALL[mode]);
                });
            }
        }
        app.midi.menu_roll = false;
    }
    if open && !done {
        app.midi.sheet = Some((s, roll));
    }
}

