//! The studio instruments' windows, laid out as Logic's.
//!
//! Studio Strings / Studio Horns: the Articulation pop-up menu in the header, the Preset pop-up
//! (a window of preset tiles; a click loads one, a click outside closes it), the Monophonic,
//! Auto Voice Split and Dynamics via CC buttons, the Last Played Articulation field, the knobs,
//! the keyswitch keyboard, the section's instruments with what each is playing, and the
//! extended parameters behind the disclosure arrow (they differ for solo and section presets).
//!
//! Studio Bass: the Instrument menu, the Main / Details buttons at the upper right, each bass's
//! own pickup controls and the fretboard area along the bottom showing the hand and the notes.
//!
//! Studio Piano: the Instrument menu, the three microphone sliders with their on/off buttons,
//! Main Volume and the noise and resonance levels.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::studio::{Kind, StudioSection};
use lg_synth::studio_bass::{self, StudioBassInst};

use crate::app::App;
use crate::instwin;
use crate::keyswin::{self, group, Pane};
use crate::look;

/// What the window needs from the running section.
struct SectionInfo {
    kind: Kind,
    presets: Vec<(&'static str, bool)>,
    arts: Vec<(&'static str, bool)>,
    section: bool,
    solo: bool,
    key_clicks: bool,
    last: &'static str,
    playing: Vec<(&'static str, Vec<u8>)>,
}

pub fn section(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let Some(info) = instwin::with_inst(app, t, |s: &mut StudioSection| SectionInfo {
        kind: s.kind,
        presets: s.presets().iter().map(|p| (p.name, p.solo)).collect(),
        arts: (0..s.arts().len()).map(|a| (s.arts()[a].name, s.has_art(a))).collect(),
        section: s.is_section(),
        solo: s.preset().solo,
        key_clicks: s.has_key_clicks(),
        last: s.last_played_articulation(),
        playing: s.playing(),
    }) else {
        return;
    };
    let mut pane = Pane::new(t, &params, &vals);
    let (title, plugin) = match info.kind {
        Kind::Strings => ("Studio Strings", "Studio Strings"),
        Kind::Horns => ("Studio Horns", "Studio Horns"),
    };
    let mut open = true;
    let mut play: Option<u8> = None;
    let resp = egui::Window::new(title).id(egui::Id::unique(("studio-section", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(680.0, 470.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, plugin);
        // the header row: Articulation, then the Preset pop-up
        ui.horizontal(|ui| {
            let art = pane.get(13).round() as usize;
            ui.label(egui::RichText::new("Articulation").size(11.0).color(look::text_dim()));
            ui.menu_button(format!("{} ⌄", info.arts.get(art).map(|a| a.0).unwrap_or("")), |ui| {
                for (k, (n, has)) in info.arts.iter().enumerate() {
                    if ui.add_enabled(*has, egui::Button::selectable(k == art, *n)).clicked() {
                        pane.put(13, k as f32);
                        ui.close();
                    }
                }
            });
            ui.add_space(12.0);
            let preset = pane.get(0).round() as usize;
            ui.label(egui::RichText::new("Preset").size(11.0).color(look::text_dim()));
            let button = ui.button(format!("{} ⌄", info.presets.get(preset).map(|p| p.0).unwrap_or("")));
            egui::Popup::menu(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_max_width(430.0);
                ui.horizontal_wrapped(|ui| {
                    for (k, (name, solo)) in info.presets.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(98.0, 74.0), Sense::click());
                        let p = ui.painter();
                        p.rect_filled(r, 8.0, if k == preset { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() });
                        crate::glyphs::paint(p, Rect::from_center_size(r.center() - vec2(0.0, 10.0), vec2(30.0, 30.0)), if *solo { crate::glyphs::ph::MUSIC_NOTE } else { crate::glyphs::ph::MUSIC_NOTES }, 24.0, look::text());
                        p.text(pos2(r.center().x, r.max.y - 12.0), Align2::CENTER_CENTER, *name, look::regular(10.5), look::text());
                        if resp.clicked() || resp.double_clicked() {
                            pane.put(0, k as f32);
                            ui.close();
                        }
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(info.last).size(11.5).strong());
                ui.label(egui::RichText::new("Last Played Articulation").size(11.0).color(look::text_dim()));
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let mut button = |ui: &mut egui::Ui, id: u32, label: &str, enabled: bool| {
                let on = pane.on(id);
                if ui.add_enabled(enabled, egui::Button::selectable(on && enabled, egui::RichText::new(label).size(11.5))).clicked() {
                    pane.put(id, if on { 0.0 } else { 1.0 });
                }
            };
            // Monophonic is for the solo instruments, Auto Voice Split for the sections
            button(ui, 11, "Monophonic", info.solo || !info.section);
            button(ui, 12, "Auto Voice Split", info.section);
            button(ui, 16, "Dynamics via CC", true);
        });
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            group(ui, "Sound", |ui| match info.kind {
                Kind::Strings => pane.knobs(ui, &[(3, "Cutoff"), (4, "Resonance"), (10, "Volume"), (1, "Attack"), (2, "Release")]),
                Kind::Horns => {
                    // Key Clicks is hidden for the trumpets and trombones
                    let mut list = vec![(5u32, "Auto Vibrato"), (6, "Humanize")];
                    if info.key_clicks {
                        list.push((7, "Key Clicks"));
                    }
                    list.extend([(10, "Volume"), (1, "Attack"), (2, "Release")]);
                    pane.knobs(ui, &list);
                }
            });
            group(ui, if info.section { "Section" } else { "Instrument" }, |ui| {
                ui.set_min_width(170.0);
                for (name, notes) in &info.playing {
                    ui.horizontal(|ui| {
                        let on = !notes.is_empty();
                        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 4.0, if on { look::accent() } else { Color32::from_gray(0x44) });
                        ui.label(egui::RichText::new(*name).size(11.0));
                        ui.label(egui::RichText::new(notes.iter().map(|n| lg_synth::sampler_doc::note_name(*n)).collect::<Vec<_>>().join(" ")).size(11.0).color(look::text_dim()));
                    });
                }
            });
        });
        ui.add_space(6.0);
        // the keyswitches (from C0, moved by the Keyswitch Octave) and the playing range
        ui.horizontal(|ui| {
            pane.switch(ui, 15, "Keyswitches");
            let oct = pane.get(14).round() as i32;
            ui.label(egui::RichText::new("Octave").size(11.0).color(look::text_dim()));
            if ui.add_enabled(oct > -2, egui::Button::new("−").small()).clicked() {
                pane.put(14, (oct - 1) as f32);
            }
            ui.label(format!("C{oct}"));
            if ui.add_enabled(oct < 2, egui::Button::new("+").small()).clicked() {
                pane.put(14, (oct + 1) as f32);
            }
        });
        let base = (24 + 12 * pane.get(14).round() as i32).clamp(0, 100) as u8;
        let count = info.arts.len() as u8;
        let art = pane.get(13).round() as u8;
        let ks_on = pane.on(15);
        let (kr, _) = ui.allocate_exact_size(vec2(ui.available_width().min(660.0), 56.0), Sense::hover());
        let mark = |n: u8| -> Option<Color32> {
            if ks_on && n >= base && n < base + count {
                let k = (n - base) as usize;
                Some(if k as u8 == art { look::accent() } else if info.arts[k].1 { Color32::from_rgb(0xd9, 0x8a, 0x2b) } else { Color32::from_gray(0x70) })
            } else {
                None
            }
        };
        if let Some(n) = keyswin::keyboard(ui, kr, egui::Id::unique(("studio-kbd", t)), 12, 108, &mark) {
            if ks_on && n >= base && n < base + count {
                if info.arts[(n - base) as usize].1 {
                    pane.put(13, (n - base) as f32);
                }
            } else {
                play = Some(n);
            }
        }
        if ks_on && art < count {
            ui.label(egui::RichText::new(format!("{} selects {}", lg_synth::sampler_doc::note_name(base + art), info.arts[art as usize].0)).size(10.5).color(look::text_dim()));
        }
        keyswin::extended(ui, egui::Id::unique(("studio-ext", t)), |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    pane.controller_menu(ui, 20, "Vibrato Controller");
                    if info.kind == Kind::Horns {
                        pane.menu(ui, 32, "Manual Vibrato Mode");
                    }
                    ui.add_enabled_ui(pane.on(16), |ui| {
                        pane.controller_menu(ui, 21, "Dynamic Controller");
                        pane.menu(ui, 22, "Dynamic Controller Mode");
                    });
                    let mut legato = pane.on(23);
                    if ui.checkbox(&mut legato, "Legato Transitions").changed() {
                        pane.put(23, legato as u8 as f32);
                    }
                    pane.slider(ui, 24, "Release Samples");
                    pane.slider(ui, 25, "Pitch Bend Range");
                    let mut ext = pane.on(26);
                    if ui.checkbox(&mut ext, "Extended Key Range").changed() {
                        pane.put(26, ext as u8 as f32);
                    }
                });
                // the Auto Voice Split parameters: section presets with the split on
                if info.section {
                    ui.add_enabled_ui(pane.on(12), |ui| {
                        ui.vertical(|ui| {
                            let mut unison = pane.on(27);
                            if ui.checkbox(&mut unison, "Allow Unison").changed() {
                                pane.put(27, unison as u8 as f32);
                            }
                            pane.menu(ui, 28, "Split Voicing");
                            pane.menu(ui, 29, "Split Mode");
                            ui.add_enabled_ui(pane.get(29).round() as i32 == 2, |ui| {
                                let cur = pane.get(30).round() as u8;
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Split Key").size(11.0).color(look::text_dim()));
                                    ui.menu_button(format!("{} ⌄", lg_synth::sampler_doc::note_name(cur)), |ui| {
                                        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                                            for n in 24..=96u8 {
                                                if ui.add(egui::Button::selectable(n == cur, lg_synth::sampler_doc::note_name(n))).clicked() {
                                                    pane.put(30, n as f32);
                                                    ui.close();
                                                }
                                            }
                                        });
                                    });
                                });
                            });
                        });
                    });
                    ui.vertical(|ui| {
                        let mut by_channel = pane.on(31);
                        if ui.checkbox(&mut by_channel, "Inst via MIDI Channel").changed() {
                            pane.put(31, by_channel as u8 as f32);
                        }
                        ui.label(egui::RichText::new("Channel 1 plays the full section; the\nnext channels its single instruments.").size(10.0).color(look::text_dim()));
                    });
                }
            });
        });
    });
    if let Some(n) = play {
        keyswin::audition(app, t, n, 100);
    }
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Studio Instrument Setting", open);
}

/// The fretboard area: four strings, the frets, the hand's position and the notes sounding.
fn fretboard(ui: &mut egui::Ui, frets: [Option<u8>; 4], hand: u8) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(680.0), 84.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 6.0, Color32::from_rgb(0x3a, 0x26, 0x18));
    let nut = r.min.x + 26.0;
    let count = studio_bass::FRETS as f32;
    // frets get closer together up the neck
    let x_of = |f: f32| nut + (r.width() - 36.0) * (1.0 - 2f32.powf(-f / 12.0)) / (1.0 - 2f32.powf(-count / 12.0));
    let span = Rect::from_min_max(pos2(x_of(hand as f32), r.min.y + 4.0), pos2(x_of(hand as f32 + 4.0), r.max.y - 4.0));
    p.rect_filled(span, 4.0, Color32::from_white_alpha(22));
    for f in 0..=studio_bass::FRETS {
        let x = x_of(f as f32);
        p.line_segment([pos2(x, r.min.y + 6.0), pos2(x, r.max.y - 6.0)], Stroke::new(if f == 0 { 3.0 } else { 1.2 }, Color32::from_gray(if f == 0 { 0xe0 } else { 0x9a })));
        if matches!(f, 3 | 5 | 7 | 9 | 12 | 15 | 17 | 19) {
            p.circle_filled(pos2((x_of(f as f32 - 1.0) + x) / 2.0, r.center().y), if f == 12 { 4.0 } else { 2.5 }, Color32::from_gray(0xb0));
        }
    }
    for (s, name) in ["E", "A", "D", "G"].iter().enumerate() {
        // (the low E string at the bottom, as the player sees the neck)
        let y = r.max.y - 12.0 - s as f32 * (r.height() - 24.0) / 3.0;
        p.line_segment([pos2(nut, y), pos2(r.max.x - 8.0, y)], Stroke::new(2.4 - s as f32 * 0.4, Color32::from_gray(0xd0)));
        p.text(pos2(r.min.x + 10.0, y), Align2::CENTER_CENTER, *name, look::regular(10.0), Color32::from_gray(0xd0));
        if let Some(f) = frets[s] {
            let x = if f == 0 { nut - 7.0 } else { (x_of(f as f32 - 1.0) + x_of(f as f32)) / 2.0 };
            p.circle_filled(pos2(x, y), 6.0, look::accent());
            p.text(pos2(x, y), Align2::CENTER_CENTER, format!("{f}"), look::regular(8.5), Color32::WHITE);
        }
    }
}

pub fn bass(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let Some((frets, hand, last)) = instwin::with_inst(app, t, |b: &mut StudioBassInst| {
        let (f, h) = b.fretboard();
        (f, h, b.last_played())
    }) else {
        return;
    };
    let tempo = app.engine.lock().map(|e| e.tempo as f32).unwrap_or(120.0);
    let mut pane = Pane::new(t, &params, &vals);
    let tab_id = egui::Id::unique(("studio-bass-tab", t));
    let mut tab: usize = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(0);
    let mut open = true;
    let resp = egui::Window::new("Studio Bass").id(egui::Id::unique(("studio-bass", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(700.0, 460.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Studio Bass");
        let inst = pane.get(0).round() as usize;
        let upright = inst == 5;
        ui.horizontal(|ui| {
            pane.menu(ui, 0, "Instrument");
            pane.menu(ui, 44, "Articulation");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut shown = 1 - tab.min(1);
                keyswin::tabs(ui, &mut shown, &["Details", "Main"]);
                tab = 1 - shown;
            });
        });
        ui.add_space(4.0);
        if tab == 0 {
            ui.horizontal_top(|ui| {
                group(ui, "Playing", |ui| {
                    // Playing Style, Mute and Definition aren't there for the upright; Slap only
                    // on the Classic and the Modern
                    ui.add_enabled_ui(!upright, |ui| {
                        let cur = pane.get(1).round() as usize;
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Playing Style").size(11.0).color(look::text_dim()));
                            ui.spacing_mut().item_spacing.x = 1.0;
                            for (k, n) in studio_bass::STYLES.iter().enumerate() {
                                let allowed = studio_bass::style_allowed(inst, k);
                                if ui.add_enabled(allowed, egui::Button::selectable(cur == k && allowed, egui::RichText::new(*n).size(11.0))).clicked() {
                                    pane.put(1, k as f32);
                                }
                            }
                        });
                        pane.slider(ui, 2, "Mute");
                        pane.slider(ui, 3, "Definition");
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Last Played").size(11.0).color(look::text_dim()));
                        ui.label(egui::RichText::new(last).size(11.5).strong());
                    });
                });
                group(ui, studio_bass::INSTRUMENTS[inst.min(5)], |ui| match inst {
                    0 => pane.knobs(ui, &[(10, "Volume"), (11, "Tone")]),
                    1 => {
                        pane.knobs(ui, &[(12, "Neck Volume"), (13, "Bridge Volume")]);
                        ui.horizontal(|ui| {
                            pane.seg(ui, 16, "");
                            pane.switch(ui, 17, "Bass");
                            pane.switch(ui, 18, "Treble");
                        });
                    }
                    2 => {
                        pane.knobs(ui, &[(12, "Neck Volume"), (13, "Bridge Volume"), (14, "Neck Tone"), (15, "Bridge Tone")]);
                        pane.menu(ui, 19, "Pickup");
                    }
                    3 => pane.knobs(ui, &[(12, "Neck Volume"), (13, "Bridge Volume"), (11, "Tone")]),
                    4 => pane.knobs(ui, &[(10, "Volume"), (20, "Bass"), (21, "Treble")]),
                    _ => pane.knobs(ui, &[(22, "Growl"), (23, "Growl Attack"), (5, "Noises")]),
                });
                group(ui, "Output", |ui| {
                    if upright {
                        pane.knob(ui, 4, "Main Volume");
                    } else {
                        pane.knobs(ui, &[(5, "Noises"), (4, "Main Volume")]);
                    }
                });
            });
        } else {
            ui.horizontal_top(|ui| {
                group(ui, "Portamento", |ui| {
                    let Some(prm) = pane.param(30) else { return };
                    let (r, _) = ui.allocate_exact_size(vec2(64.0, 74.0), Sense::hover());
                    let mut v = pane.get(30);
                    let sync = pane.on(31);
                    // milliseconds, or note values with Tempo Sync
                    let shown = |x: f32| {
                        let (secs, name) = studio_bass::slide_time(x, sync, tempo);
                        if name.is_empty() { format!("{:.0} ms", secs * 1000.0) } else { name.to_string() }
                    };
                    if crate::widgets::knob(ui, r, egui::Id::unique(("bass-porta", t)), &mut v, prm.min, prm.max, prm.default, "Portamento", &shown) {
                        pane.put(30, v);
                    }
                    ui.label(egui::RichText::new(shown(v)).size(10.5).color(look::accent()));
                    pane.switch(ui, 31, "Tempo Sync");
                });
                group(ui, "Noises", |ui| {
                    if upright {
                        for (id, n) in [(36u32, "Rattles"), (37, "Releases"), (38, "Handling")] {
                            pane.slider(ui, id, n);
                        }
                    } else {
                        // Scrapes: with the Pick playing style only
                        ui.add_enabled_ui(pane.get(1).round() as i32 == 1, |ui| pane.slider(ui, 32, "Scrapes"));
                        for (id, n) in [(33u32, "String Noise"), (34, "Hum & Hiss"), (35, "Release Noise")] {
                            pane.slider(ui, id, n);
                        }
                    }
                });
                group(ui, "Playing", |ui| {
                    pane.slider(ui, 39, "Pitchbend Up");
                    pane.slider(ui, 40, "Pitchbend Down");
                    pane.menu(ui, 41, "Voice Mode");
                    pane.menu(ui, 42, "Open Strings");
                    ui.add_enabled_ui(!upright, |ui| pane.controller_menu(ui, 43, "Mute Controller"));
                    ui.horizontal(|ui| {
                        pane.switch(ui, 46, "Keyswitches");
                        let oct = pane.get(45).round() as i32;
                        if ui.add_enabled(oct > -1, egui::Button::new("−").small()).clicked() {
                            pane.put(45, (oct - 1) as f32);
                        }
                        ui.label(format!("C{}", oct - 1));
                        if ui.add_enabled(oct < 3, egui::Button::new("+").small()).clicked() {
                            pane.put(45, (oct + 1) as f32);
                        }
                    });
                });
            });
        }
        ui.add_space(6.0);
        fretboard(ui, frets, hand);
    });
    ctx.data_mut(|d| d.insert_temp(tab_id, tab));
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Studio Bass Setting", open);
}

pub fn piano(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let mut pane = Pane::new(t, &params, &vals);
    let mut open = true;
    let resp = egui::Window::new("Studio Piano").id(egui::Id::unique(("studio-piano", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(560.0, 340.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Studio Piano");
        pane.menu(ui, 0, "Instrument");
        ui.add_space(4.0);
        let mono_only = pane.get(0).round() as i32 == 3;
        ui.horizontal_top(|ui| {
            group(ui, "Microphones", |ui| {
                ui.horizontal(|ui| {
                    for (level, on, name) in [(1u32, 4u32, "Stereo Mic A"), (2, 5, "Stereo Mic B"), (3, 6, "Mono Mic")] {
                        // (the Mono Mic instrument has its one microphone only)
                        let there = !mono_only || level == 3;
                        ui.add_enabled_ui(there, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(84.0);
                                let mut v = pane.get(level);
                                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).vertical().show_value(false)).changed() {
                                    pane.put(level, v);
                                }
                                let lit = if mono_only { level == 3 } else { pane.on(on) };
                                if ui.add(egui::Button::selectable(lit, egui::RichText::new(name).size(10.5))).clicked() && !mono_only {
                                    pane.put(on, if lit { 0.0 } else { 1.0 });
                                }
                            });
                        });
                    }
                });
            });
            ui.vertical(|ui| {
                group(ui, "Output", |ui| pane.knob(ui, 7, "Main Volume"));
                ui.add_space(4.0);
                group(ui, "Details", |ui| pane.knobs(ui, &[(8, "Pedal Noise"), (9, "Key Noise"), (10, "Release Samples"), (11, "Symp. Resonance")]));
            });
        });
    });
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Studio Piano Setting", open);
}
