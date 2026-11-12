//! The SysEx Fader Editor (an Environment fader whose Output or Input is SysEx; lg_core::sysexfader
//! does the bytes): an Event List-like list with the eight event type buttons (Command-click
//! makes a new event), MIDI In (captures incoming SysEx), View ▸ SysEx in Hex Format, Plus and
//! Minus beside EOX, and the Checksum and Value (Position, Format) pop-ups at the lower right.
//! Selected events take the fader's value; the others are sent as entered.

use egui::{Color32, RichText, vec2};
use lg_core::environment::Id;
use lg_core::sysexfader::{self as sx, SxEv, Tok};

use crate::app::App;
use crate::look;

fn midi_in_key() -> egui::Id {
    egui::Id::unique("sysex-fader-midi-in")
}

/// A whole MIDI message from an input: SysEx captured while the editor's MIDI In is on, and
/// faders whose Input is SysEx. True when captured.
pub fn midi_in(app: &mut App, ctx: &egui::Context, bytes: &[u8]) -> bool {
    if bytes.first() != Some(&0xF0) {
        return false;
    }
    let c = crate::envwin::clock(app);
    let outs = app.hub.env.env.input_sysex(bytes, c);
    crate::envwin::dispatch(app, ctx, outs);
    let Some(id) = ctx.data(|d| d.get_temp::<Id>(midi_in_key())).filter(|i| app.hub.env.sub == Some((crate::envwin::Sub::SysEx, *i))) else {
        return false;
    };
    let Some(ev) = sx::capture(bytes) else { return false };
    crate::envwin::checkpoint(app);
    let Some(o) = app.hub.env.env.obj_mut(id) else { return false };
    let mut list = sx::list_from(o.texts.get("sysex"));
    for e in list.iter_mut() {
        e.set_sel(false);
    }
    list.push(ev);
    o.texts.insert("sysex".into(), sx::list_text(&list));
    ctx.request_repaint();
    true
}

pub fn window(app: &mut App, ui: &mut egui::Ui, id: Id) {
    let Some(mut o) = app.hub.env.env.obj(id).cloned() else { return };
    let before = o.clone();
    let mut list = sx::list_from(o.texts.get("sysex"));
    let ctx = ui.ctx().clone();
    let hex_key = egui::Id::unique("sysex-fader-hex");
    let mut hex = ctx.data(|d| d.get_temp::<bool>(hex_key)).unwrap_or(false);
    let mut listening = ctx.data(|d| d.get_temp::<Id>(midi_in_key())) == Some(id);
    let cmd = ui.input(|i| i.modifiers.command);
    // the View menu and MIDI In, then Logic's event type buttons (blue tiles, two rows of four)
    ui.horizontal(|ui| {
        ui.menu_button("View ⌄", |ui| {
            if ui.checkbox(&mut hex, "SysEx in Hex Format").clicked() {
                ui.close();
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .selectable_label(listening, "MIDI In")
                .on_hover_text("Captures SysEx messages from the MIDI inputs (change a parameter on the device)")
                .clicked()
            {
                listening = !listening;
            }
        });
    });
    ui.add_space(4.0);
    const LABELS: [&str; 8] = [
        "Notes",
        "Progr. Change",
        "Pitch Bend",
        "Controller",
        "Aftertouch",
        "Poly Aftertouch",
        "Syst. Exclusive",
        "Additional Info",
    ];
    const ORDER: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    let bw = ((ui.available_width() - 3.0 * 6.0) / 4.0).max(60.0);
    for row in 0..2 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for c in 0..4 {
                let k = ORDER[row * 4 + c];
                let btn = egui::Button::new(RichText::new(LABELS[k]).size(11.0).color(Color32::WHITE))
                    .fill(look::accent().gamma_multiply(0.85))
                    .corner_radius(4.0)
                    .min_size(vec2(bw, 20.0));
                let r = ui
                    .add(btn)
                    .on_hover_text(format!("{}: Command-click to make a new event; click to select all of its type", sx::EVENT_TYPES[k]));
                if r.clicked() {
                    if cmd {
                        for e in list.iter_mut() {
                            e.set_sel(false);
                        }
                        let mut e = SxEv::new_of(k);
                        if let SxEv::Sysex { toks, .. } = &mut e {
                            sx::place(toks, o.p("sx_pos").max(0) as usize, o.p("sx_sum").max(0) as usize);
                        }
                        list.push(e);
                    } else {
                        let probe = SxEv::new_of(k).status_name();
                        for e in list.iter_mut() {
                            let on = e.status_name() == probe;
                            e.set_sel(on);
                        }
                    }
                }
            }
        });
    }
    ui.add_space(4.0);
    ui.separator();
    // the list: Status, Ch, Num, Val and the SysEx bytes up to EOX
    let mut delete: Option<usize> = None;
    let shift = ui.input(|i| i.modifiers.shift);
    egui::ScrollArea::vertical().max_height(ui.available_height() - 64.0).show(ui, |ui| {
        egui::Grid::new(("sysex-list", id)).num_columns(6).striped(true).spacing(vec2(10.0, 3.0)).show(ui, |ui| {
            for h in ["Status", "Ch", "Num", "Val", "Data", ""] {
                ui.label(RichText::new(h).size(11.0).strong().color(look::text_dim()));
            }
            ui.end_row();
            let n = list.len();
            for i in 0..n {
                let sel = list[i].sel();
                let name = list[i].status_name();
                let lab = ui.selectable_label(sel, RichText::new(name).color(if sel { Color32::WHITE } else { look::text_dim() }));
                if lab.clicked() {
                    if shift {
                        list[i].set_sel(!sel);
                    } else {
                        for (k, e) in list.iter_mut().enumerate() {
                            e.set_sel(k == i);
                        }
                    }
                }
                match &mut list[i] {
                    SxEv::Chan { st, ch, d1, d2, .. } => {
                        ui.add(egui::DragValue::new(ch).range(1..=16));
                        if matches!(*st, 0xC0 | 0xD0) {
                            ui.add(egui::DragValue::new(d1).range(0..=127));
                            ui.label("");
                        } else if *st == 0xE0 {
                            ui.label("");
                            let mut v = ((*d2 as i32) << 7 | *d1 as i32) - 8192;
                            if ui.add(egui::DragValue::new(&mut v).range(-8192..=8191)).changed() {
                                let w = (v + 8192) as u32;
                                *d1 = (w & 127) as u8;
                                *d2 = (w >> 7) as u8;
                            }
                        } else {
                            ui.add(egui::DragValue::new(d1).range(0..=127));
                            ui.add(egui::DragValue::new(d2).range(0..=127));
                        }
                        ui.label("");
                    }
                    SxEv::Meta { num, val, .. } => {
                        ui.label("");
                        ui.add(egui::DragValue::new(num).range(0..=127));
                        ui.add(egui::DragValue::new(val).range(0..=127));
                        ui.label("");
                    }
                    SxEv::Sysex { toks, .. } => {
                        ui.label("");
                        // Num: the manufacturer ID (the first byte after SysEx)
                        let mut man = match toks.first() {
                            Some(Tok::B(b)) => *b as i32,
                            _ => 0,
                        };
                        let r = ui.add(egui::DragValue::new(&mut man).range(0..=127).custom_formatter(move |x, _| sx::byte_text(x as u8, hex)));
                        if r.changed() {
                            match toks.first_mut() {
                                Some(Tok::B(b)) => *b = man as u8,
                                _ => toks.insert(0, Tok::B(man as u8)),
                            }
                        }
                        ui.label("");
                        // the rest: bytes typed as decimal or $hex, VAL and SUM
                        let key = egui::Id::unique(("sysex-bytes", id, i));
                        let shown = sx::toks_text(toks.get(1..).unwrap_or(&[]), hex);
                        let mut text: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| shown.clone());
                        ui.horizontal(|ui| {
                            let te = ui.add(egui::TextEdit::singleline(&mut text).desired_width(260.0).font(egui::TextStyle::Monospace));
                            if te.has_focus() {
                                ui.data_mut(|d| d.insert_temp(key, text.clone()));
                            } else if te.lost_focus() {
                                if let Some(mut rest) = sx::parse_toks(&text) {
                                    let head = toks.first().copied();
                                    toks.clear();
                                    toks.extend(head);
                                    toks.append(&mut rest);
                                }
                                ui.data_mut(|d| d.remove::<String>(key));
                            }
                            // (Plus / Minus around EOX add or remove a data byte)
                            if ui.small_button("−").on_hover_text("Remove the data byte before EOX").clicked() {
                                if let Some(p) = toks.iter().rposition(|t| matches!(t, Tok::B(_))).filter(|p| *p > 0) {
                                    toks.remove(p);
                                }
                            }
                            ui.label(RichText::new("EOX").monospace());
                            if ui.small_button("+").on_hover_text("Add a data byte before EOX").clicked() {
                                let at = toks.iter().position(|t| matches!(t, Tok::Val | Tok::Sum)).unwrap_or(toks.len());
                                toks.insert(at, Tok::B(0));
                            }
                        });
                    }
                }
                if ui.small_button("✕").on_hover_text("Delete the event").clicked() {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = delete {
        list.remove(i);
    }
    // Checksum and Value at the lower right
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} Event{}", list.len(), if list.len() == 1 { "" } else { "s" }))
                .size(11.0)
                .color(look::text_dim()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut fmt = o.p("sx_fmt").max(0) as usize;
            let mut pos = o.p("sx_pos").max(0) as usize;
            let mut sum = o.p("sx_sum").max(0) as usize;
            let positions = sx::positions();
            egui::ComboBox::from_id_salt(("sx-fmt", id)).selected_text(sx::FORMATS[fmt.min(14)]).width(96.0).show_ui(ui, |ui| {
                for (k, n) in sx::FORMATS.iter().enumerate() {
                    ui.selectable_value(&mut fmt, k, *n);
                }
            });
            egui::ComboBox::from_id_salt(("sx-pos", id))
                .selected_text(positions[pos.min(positions.len() - 1)].clone())
                .width(70.0)
                .show_ui(ui, |ui| {
                    for (k, n) in positions.iter().enumerate() {
                        ui.selectable_value(&mut pos, k, n);
                    }
                });
            ui.label("Value:");
            ui.add_space(12.0);
            egui::ComboBox::from_id_salt(("sx-sum", id)).selected_text(sx::CHECKSUMS[sum.min(5)]).width(110.0).show_ui(ui, |ui| {
                for (k, n) in sx::CHECKSUMS.iter().enumerate() {
                    ui.selectable_value(&mut sum, k, *n);
                }
            });
            ui.label("Checksum:");
            if pos as i32 != o.p("sx_pos") || sum as i32 != o.p("sx_sum") {
                // (VAL and SUM move in the selected SysEx strings)
                for e in list.iter_mut() {
                    if let SxEv::Sysex { toks, sel: true } = e {
                        sx::place(toks, pos, sum);
                    }
                }
            }
            o.params.insert("sx_fmt".into(), fmt as i32);
            o.params.insert("sx_pos".into(), pos as i32);
            o.params.insert("sx_sum".into(), sum as i32);
        });
    });
    ctx.data_mut(|d| {
        d.insert_temp(hex_key, hex);
        if listening {
            d.insert_temp(midi_in_key(), id);
        } else if d.get_temp::<Id>(midi_in_key()) == Some(id) {
            d.remove::<Id>(midi_in_key());
        }
    });
    let text = sx::list_text(&list);
    if o.texts.get("sysex").map_or("", |s| s.as_str()) != text {
        o.texts.insert("sysex".into(), text);
    }
    if o != before {
        crate::envwin::checkpoint(app);
        if let Some(x) = app.hub.env.env.obj_mut(id) {
            *x = o;
        }
    }
}
