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
            // Presets ▸ Create New Transform Set / Create Initialized User Set
            ui.menu_button("Presets ⌄", |ui| {
                if ui.button("Create New Transform Set").clicked() {
                    let mut t = w.t.clone();
                    t.name = format!("New Parameter Set {}", w.slot);
                    w.t = t;
                    ui.close();
                }
                if ui.button("Create Initialized User Set").clicked() {
                    w.t = transform::Transform::new(&format!("New Parameter Set {}", w.slot));
                    ui.close();
                }
            });
            ui.label(if w.roll { "on the Piano Roll's notes" } else { "on the selected regions" });
        });
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut w.t.name).desired_width(200.0)).on_hover_text("A new name saves as a new set (the original stays)");
            ui.label("Mode:");
            let mut mode = w.t.mode as usize;
            egui::ComboBox::from_id_salt("tf-mode").selected_text(transform::MODES[mode]).width(260.0).show_ui(ui, |ui| {
                for (k, m) in transform::MODES.iter().enumerate() {
                    ui.selectable_value(&mut mode, k, *m);
                }
            });
            w.t.mode = mode as u8;
            ui.menu_button("Options ⌄", |ui| {
                ui.checkbox(&mut w.t.hide_unused, "Hide Unused Parameters");
                ui.separator();
                if ui.radio(!w.t.absolute, "Relative Position").clicked() {
                    w.t.absolute = false;
                }
                if ui.radio(w.t.absolute, "Absolute Position").clicked() {
                    w.t.absolute = true;
                }
            });
        });
        // how many regions and events the conditions find
        let (regions, events) = crate::midifunc::transform_count(app, &w.t);
        ui.label(egui::RichText::new(format!("{regions} region{} selected · {events} event{} match", if regions == 1 { "" } else { "s" }, if events == 1 { "" } else { "es" })).small().weak());
        ui.add_space(6.0);
        egui::Grid::new("tf-grid").num_columns(7).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("");
            // (the Status picks the columns: notes have Pitch, Velocity and Length; other events
            // Data Byte 1 and 2, some only one)
            let st = w.t.status;
            let used = |t: &transform::Transform, p: usize| {
                let shown = match (st, p) {
                    (0x90, _) | (0, _) => true,
                    (_, 3) => false,
                    (0xC0 | 0xD0, 2) => false,
                    _ => true,
                };
                shown && (!t.hide_unused || t.conds[p] != Cond::All || t.ops[p] != Op::Thru || t.route[p] as usize != p)
            };
            ui.label(egui::RichText::new("Status").strong());
            for (p, name) in transform::PARAMS.iter().enumerate() {
                let name = match (st, p) {
                    (0x90 | 0, _) => *name,
                    (_, 1) => "Data Byte 1",
                    (_, 2) => "Data Byte 2",
                    _ => *name,
                };
                ui.label(egui::RichText::new(if used(&w.t, p) { name } else { "" }).strong());
            }
            ui.label(egui::RichText::new("Subposition").strong());
            ui.end_row();
            ui.label("Select Conditions:");
            status_pick(ui, "tf-st-cond", &mut w.t.status, ["All", "="]);
            for p in 0..5 {
                if !used(&w.t, p) {
                    ui.label("");
                    continue;
                }
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
            // Subposition: ticks within the bar
            ui.vertical(|ui| {
                let cur = w.t.sub.index().min(6);
                egui::ComboBox::from_id_salt("tf-sub").selected_text(Cond::NAMES[cur]).width(70.0).show_ui(ui, |ui| {
                    for (k, n) in Cond::NAMES.iter().enumerate().take(7) {
                        if ui.selectable_label(k == cur, *n).clicked() {
                            w.t.sub = w.t.sub.with_index(k);
                        }
                    }
                });
                let (mut a, mut b) = w.t.sub.values();
                let n = w.t.sub.arity();
                if n >= 1 {
                    value_drag(ui, &mut a, 0);
                }
                if n >= 2 {
                    value_drag(ui, &mut b, 0);
                }
                w.t.sub.set_values(a, b);
            });
            ui.end_row();
            ui.end_row();
            // the exchange dots: Channel, Data Byte 1 and Data Byte 2 can take each other's value
            ui.label("Exchange:");
            ui.label("");
            for p in 0..5 {
                if ![1usize, 2, 4].contains(&p) || !used(&w.t, p) {
                    ui.label("");
                    continue;
                }
                let src = w.t.route[p] as usize;
                let txt = if src == p { "•".to_string() } else { format!("← {}", ["Position", "Data Byte 1", "Data Byte 2", "Length", "Channel"][src]) };
                if ui.button(txt).on_hover_text("Click to take another column's value (before the operation)").clicked() {
                    let order = [p, if p == 4 { 1 } else { 4 }, if p == 2 { 1 } else { 2 }];
                    let at = order.iter().position(|&x| x == src).unwrap_or(0);
                    w.t.route[p] = order[(at + 1) % 3] as u8;
                }
            }
            ui.end_row();
            ui.label("Operations:");
            status_pick(ui, "tf-st-op", &mut w.t.status_fix, ["Thru", "Fix"]);
            for p in 0..5 {
                if !used(&w.t, p) {
                    ui.label("");
                    continue;
                }
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
        ui.label(egui::RichText::new(if w.t.absolute { "Position counts ticks from the project's start (960 a quarter note); Length in ticks." } else { "Position counts ticks from the region's start (960 a quarter note); Length in ticks." }).small().weak());
        // the map (Use Map / Map): 128 bars, dragged; the fields show the original and mapped value
        if w.t.ops.iter().any(|o| *o == Op::UseMap) || w.t.conds.iter().any(|c| matches!(c, Cond::Map(..))) {
            map_editor(ui, &mut w.t.map);
        }
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

/// The Status condition (All / = type) or operation (Thru / Fix type): 0, or the event type.
fn status_pick(ui: &mut egui::Ui, id: &str, v: &mut u8, names: [&str; 2]) {
    ui.vertical(|ui| {
        egui::ComboBox::from_id_salt((id, 0)).selected_text(if *v == 0 { names[0] } else { names[1] }).width(70.0).show_ui(ui, |ui| {
            if ui.selectable_label(*v == 0, names[0]).clicked() {
                *v = 0;
            }
            if ui.selectable_label(*v != 0, names[1]).clicked() && *v == 0 {
                *v = 0x90;
            }
        });
        if *v != 0 {
            let cur = transform::STATUS.iter().find(|s| s.1 == *v).map_or("Note", |s| s.0);
            egui::ComboBox::from_id_salt((id, 1)).selected_text(cur).width(70.0).show_ui(ui, |ui| {
                for (n, k) in transform::STATUS {
                    if ui.selectable_label(*v == k, n).clicked() {
                        *v = k;
                    }
                }
            });
        }
    });
}

/// The MIDI Transform map: 128 vertical bars (value 0-127 → mapped value), dragged; two fields
/// show and set the original and mapped value.
fn map_editor(ui: &mut egui::Ui, map: &mut Vec<u8>) {
    if map.len() != 128 {
        *map = transform::identity_map();
    }
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(128.0 * 4.0, 128.0), egui::Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, crate::look::lcd_bg());
    for (i, m) in map.iter().enumerate() {
        let x = rect.min.x + i as f32 * 4.0;
        let h = (*m as f32 + 1.0) / 128.0 * rect.height();
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x + 0.5, rect.max.y - h), egui::pos2(x + 3.5, rect.max.y)), 0.0, crate::look::accent());
    }
    let sel_id = ui.make_persistent_id("tf-map-sel");
    let mut sel: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(60);
    if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let i = (((pp.x - rect.min.x) / 4.0) as usize).min(127);
        let v = ((rect.max.y - pp.y) / rect.height() * 128.0).clamp(0.0, 127.0) as u8;
        map[i] = v;
        sel = i;
    }
    ui.horizontal(|ui| {
        let mut a = sel as i64;
        ui.label("Value:");
        ui.add(egui::DragValue::new(&mut a).range(0..=127));
        sel = a as usize;
        let mut b = map[sel] as i64;
        ui.label("→");
        if ui.add(egui::DragValue::new(&mut b).range(0..=127)).changed() {
            map[sel] = b as u8;
        }
        if ui.button("1:1").on_hover_text("Each value maps to itself").clicked() {
            *map = transform::identity_map();
        }
    });
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
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

