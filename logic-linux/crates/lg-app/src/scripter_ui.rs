//! Scripter's window and its Script Editor (lg-dsp scripter.rs runs the scripts).
//!
//! - The window: the Open Script in Editor button (Scripter's one global control), then the
//!   controls the script declares in PluginParameters (sliders, menus, buttons, checkboxes,
//!   text headers; hidden ones left out), in the Controls view's rows.
//! - The Script Editor: Run Script, the code with line numbers, JavaScript / Scripter API
//!   colours and a live syntax check that marks the error's line, and the Interactive Console
//!   (the script's Trace output and errors; type code and press Return to run it, `clear`
//!   empties it).
//! - The Setting menu adds the Tutorial Scripts and the ready-made script processors.

use egui::{pos2, text::LayoutJob, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, TextFormat};
use lg_dsp::scripter::{self, PKind, SParam, View};
use std::collections::HashMap;

use crate::look;

pub const COMMANDS: &[&str] = &["Open Script in Editor", "Run Script"];

#[derive(Default)]
pub struct Editor {
    /// The Script Editor's slot (track, slot).
    pub open: Option<(usize, usize)>,
    buffer: String,
    /// The slot and script the buffer was loaded from.
    loaded: Option<(usize, usize, String)>,
    input: String,
    /// Parameters of a script read in a scratch interpreter (when the engine hasn't run it).
    cache: Option<(String, Vec<SParam>)>,
    /// Last SetParameter count taken from each slot's view.
    serial: HashMap<(usize, usize), u64>,
    held: Option<(usize, usize, usize)>,
}

/// What the window or editor asks of the slot.
#[derive(Default)]
pub struct Out {
    /// A control moved (undo label, a step of its own).
    pub changed: Option<(&'static str, bool)>,
    /// New values (undo label; "" for no undo step).
    pub new_values: Option<(&'static str, Vec<f32>)>,
    /// Commands for the running unit ("run", "clear", "eval:…").
    pub commands: Vec<String>,
}

fn params_for(ed: &mut Editor, values: &[f32], view: Option<&View>) -> Vec<SParam> {
    if let Some(v) = view {
        return v.params.clone();
    }
    let script = scripter::script_of(values);
    if ed.cache.as_ref().is_none_or(|c| c.0 != script) {
        let (p, _) = scripter::read_params(&script);
        ed.cache = Some((script, p));
    }
    ed.cache.as_ref().map(|c| c.1.clone()).unwrap_or_default()
}

/// The Setting menu's script entries; returns the values of the script picked.
pub fn preset_menu(ui: &mut egui::Ui) -> Option<Vec<f32>> {
    let mut pick = None;
    ui.menu_button("Tutorial Scripts", |ui| {
        ui.set_min_width(240.0);
        for (n, s) in scripter::TUTORIALS {
            if ui.button(*n).clicked() {
                pick = Some(scripter::load_script(s));
                ui.close();
            }
        }
    });
    ui.menu_button("Script Processors", |ui| {
        ui.set_min_width(200.0);
        for (n, s) in scripter::FACTORY {
            if ui.button(*n).clicked() {
                pick = Some(scripter::load_script(s));
                ui.close();
            }
        }
    });
    pick
}

/// The plug-in window's body.
pub fn body(ed: &mut Editor, ui: &mut egui::Ui, t: usize, k: usize, values: &mut Vec<f32>, view: Option<&View>) -> Out {
    let mut out = Out::default();
    // the script's SetParameter calls move the controls (no undo step, like automation)
    if let Some(v) = view {
        let last = ed.serial.entry((t, k)).or_insert(v.set_serial);
        if *last != v.set_serial {
            *last = v.set_serial;
            let mut nv = values.clone();
            for (i, x) in v.values.iter().enumerate().take(scripter::MAX_PARAMS) {
                nv[i] = *x;
            }
            out.new_values = Some(("", nv));
        }
    }
    ui.vertical_centered(|ui| {
        if ui.add(egui::Button::new(egui::RichText::new("Open Script in Editor").font(look::regular(12.0))).min_size(vec2(170.0, 22.0))).clicked() {
            ed.open = Some((t, k));
        }
    });
    if let Some(e) = view.and_then(|v| v.error.as_ref()) {
        ui.add_space(4.0);
        ui.colored_label(look::record_red(), format!("Script error: {}", e.1));
    }
    ui.add_space(6.0);
    let params = params_for(ed, values, view);
    egui::ScrollArea::vertical().id_salt(("scripter-controls", t, k)).auto_shrink([false, true]).max_height(ui.available_height().max(120.0)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, p) in params.iter().enumerate().take(scripter::MAX_PARAMS) {
            if p.hidden {
                continue;
            }
            let v = values.get(i).copied().unwrap_or(0.0);
            if let Some(nv) = control_row(ed, ui, t, k, i, p, v) {
                if let Some(x) = values.get_mut(i) {
                    *x = p.snap(nv);
                }
                out.changed = Some(("Change MIDI Plug-in Parameter", matches!(p.kind, PKind::Menu | PKind::Target | PKind::Checkbox | PKind::Momentary)));
            }
        }
        if params.iter().all(|p| p.hidden) {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.label(egui::RichText::new("This script has no controls.").font(look::regular(12.0)).color(look::text_dim())));
        }
    });
    out
}

/// One control row: name at the left, the control, the value at the right (as the Controls view).
fn control_row(ed: &mut Editor, ui: &mut egui::Ui, t: usize, k: usize, i: usize, p: &SParam, v: f32) -> Option<f32> {
    if p.kind == PKind::Text {
        // a header with a divider
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        let pt = ui.painter();
        pt.text(pos2(row.min.x + 8.0, row.center().y + 2.0), Align2::LEFT_CENTER, &p.name, look::semibold(12.0), look::text());
        pt.hline(row.min.x + 8.0..=row.max.x - 4.0, row.max.y - 2.0, Stroke::new(1.0, look::line()));
        return None;
    }
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let pt = ui.painter().clone();
    let name_w = (row.width() * 0.34).min(200.0);
    pt.text(pos2(row.min.x + name_w, row.center().y), Align2::RIGHT_CENTER, &p.name, look::regular(12.0), look::text_dim());
    let ctl = Rect::from_min_max(pos2(row.min.x + name_w + 12.0, row.min.y + 3.0), pos2(row.max.x - 96.0, row.max.y - 3.0));
    let id = ui.scope_id().with(("scripter-param", t, k, i));
    let mut set = None;
    match p.kind {
        PKind::Menu | PKind::Target => {
            let names: Vec<&str> = p.strings.iter().map(String::as_str).collect();
            if let Some(c) = crate::widgets::choice_slot(ui, ctl, id, v.round().max(0.0) as usize, &names) {
                set = Some(c as f32);
            }
            return set;
        }
        PKind::Checkbox => {
            let mut on = v >= 0.5;
            if crate::widgets::check_slot(ui, Rect::from_min_size(ctl.min, vec2(24.0, ctl.height())), id, &mut on) {
                set = Some(on as u8 as f32);
            }
            return set;
        }
        PKind::Momentary => {
            // on while pressed
            let b = Rect::from_min_size(ctl.min, vec2(70.0, ctl.height()));
            let resp = ui.interact(b, id, Sense::click_and_drag());
            let down = resp.is_pointer_button_down_on();
            pt.rect_filled(b, 4.0, if down { look::button_on() } else { look::group_bg() });
            pt.text(b.center(), Align2::CENTER_CENTER, &p.name, look::regular(11.0), look::text());
            let was = ed.held == Some((t, k, i));
            if down && !was {
                ed.held = Some((t, k, i));
                set = Some(1.0);
            } else if !down && was {
                ed.held = None;
                set = Some(0.0);
            }
            return set;
        }
        _ => {}
    }
    let (lo, hi) = (p.min, p.max);
    let log = p.kind == PKind::Log && lo > 0.0 && hi > lo;
    let frac = |x: f32| if log { ((x / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0) } else { ((x - lo) / (hi - lo).max(1e-9)).clamp(0.0, 1.0) };
    let from_frac = |f: f32| if log { lo * (hi / lo).powf(f) } else { lo + f * (hi - lo) };
    let f = frac(v);
    let track = Rect::from_center_size(ctl.center(), vec2(ctl.width(), 4.0));
    pt.rect_filled(track, 2.0, look::group_bg());
    pt.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * f, 4.0)), 2.0, look::accent());
    pt.circle_filled(pos2(track.min.x + track.width() * f, track.center().y), 6.5, Color32::from_gray(0xe0));
    let resp = ui.interact(ctl, id, Sense::click_and_drag());
    if resp.double_clicked() || (resp.clicked() && ui.input(|i| i.modifiers.alt)) {
        set = Some(p.default);
    } else if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        set = Some(from_frac(((pp.x - track.min.x) / track.width()).clamp(0.0, 1.0)));
    }
    pt.text(pos2(row.max.x - 4.0, row.center().y), Align2::RIGHT_CENTER, p.text(set.unwrap_or(v)), look::regular(12.0), look::text());
    set
}

// ---------------------------------------------------------------------------------------------
// the Script Editor

const KEYWORDS: &[&str] = &["var", "let", "const", "function", "return", "if", "else", "for", "while", "do", "break", "continue", "switch", "case", "default", "throw", "try", "catch", "finally", "new", "typeof", "instanceof", "in", "of", "delete", "void", "this", "true", "false", "null", "undefined"];
const API: &[&str] = &[
    "HandleMIDI", "ProcessMIDI", "ParameterChanged", "Reset", "GetParameter", "SetParameter", "UpdatePluginParameters", "PluginParameters", "NeedsTimingInfo", "GetTimingInfo", "Trace", "MIDI", "Event", "NoteOn", "NoteOff", "PolyPressure", "ControlChange", "ProgramChange", "ChannelPressure", "PitchBend", "TargetEvent", "send", "sendAfterMilliseconds", "sendAtBeat", "sendAfterBeats", "trace", "beatPos",
];

struct Palette {
    text: Color32,
    keyword: Color32,
    string: Color32,
    number: Color32,
    comment: Color32,
    api: Color32,
}

fn palette() -> Palette {
    // Xcode's default source colours (Logic's Script Editor follows them)
    if kn_ui::theme::is_light() {
        Palette { text: Color32::from_rgb(0x1f, 0x1f, 0x24), keyword: Color32::from_rgb(0x9b, 0x23, 0x93), string: Color32::from_rgb(0xc4, 0x1a, 0x16), number: Color32::from_rgb(0x1c, 0x00, 0xcf), comment: Color32::from_rgb(0x5d, 0x6c, 0x79), api: Color32::from_rgb(0x0b, 0x4f, 0x79) }
    } else {
        Palette { text: Color32::from_rgb(0xdf, 0xdf, 0xe0), keyword: Color32::from_rgb(0xfc, 0x5f, 0xa3), string: Color32::from_rgb(0xfc, 0x6a, 0x5d), number: Color32::from_rgb(0xd0, 0xbf, 0x69), comment: Color32::from_rgb(0x6c, 0x79, 0x86), api: Color32::from_rgb(0x5d, 0xd8, 0xff) }
    }
}

/// The code as coloured runs: keywords, strings, numbers, comments, the Scripter API.
pub fn highlight(src: &str, font: FontId) -> LayoutJob {
    let pal = palette();
    let mut job = LayoutJob::default();
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut push = |job: &mut LayoutJob, s: &[char], col: Color32| {
        job.append(&s.iter().collect::<String>(), 0.0, TextFormat { font_id: font.clone(), color: col, ..Default::default() });
    };
    while i < c.len() {
        let st = i;
        let ch = c[i];
        let col = if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            pal.comment
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(c.len());
            pal.comment
        } else if ch == '"' || ch == '\'' || ch == '`' {
            i += 1;
            while i < c.len() && c[i] != ch && (ch == '`' || c[i] != '\n') {
                if c[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(c.len());
            pal.string
        } else if ch.is_ascii_digit() {
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.') {
                i += 1;
            }
            pal.number
        } else if ch.is_alphabetic() || ch == '_' || ch == '$' {
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '$') {
                i += 1;
            }
            let w: String = c[st..i].iter().collect();
            if KEYWORDS.contains(&w.as_str()) {
                pal.keyword
            } else if API.contains(&w.as_str()) {
                pal.api
            } else {
                pal.text
            }
        } else {
            i += 1;
            while i < c.len() && !c[i].is_alphanumeric() && !"\"'`/_$".contains(c[i]) {
                i += 1;
            }
            pal.text
        };
        push(&mut job, &c[st..i], col);
    }
    job
}

fn mono(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Monospace)
}

/// The Script Editor window for the open slot.
pub fn editor(ed: &mut Editor, ctx: &egui::Context, title: &str, t: usize, k: usize, values: &[f32], view: Option<&View>) -> Out {
    let mut out = Out::default();
    let script = scripter::script_of(values);
    // (re)load the buffer when another slot opens or the slot's script changed elsewhere
    if ed.loaded.as_ref().is_none_or(|l| l.0 != t || l.1 != k || l.2 != script) {
        let fresh = ed.loaded.as_ref().is_none_or(|l| l.0 != t || l.1 != k || ed.buffer == l.2);
        if fresh {
            ed.buffer = script.clone();
        }
        ed.loaded = Some((t, k, script.clone()));
    }
    let syntax = lg_dsp::js::check(&ed.buffer);
    let mut open = true;
    egui::Window::new(format!("{title} — Script Editor")).id(egui::Id::unique(("script-editor", t, k))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(640.0, 560.0)).show(ctx, |ui| {
        // the toolbar
        ui.horizontal(|ui| {
            let run = ui.add(egui::Button::new(egui::RichText::new("Run Script").font(look::regular(12.0))).min_size(vec2(90.0, 22.0))).on_hover_text("Runs the script and sets up the plug-in (⌘S)");
            let key = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
            if run.clicked() || key {
                if ed.buffer == script {
                    out.commands.push("run".into());
                } else {
                    out.new_values = Some(("Run Script", scripter::rerun_values(values, &script, &ed.buffer)));
                }
            }
            if ed.buffer != script {
                ui.label(egui::RichText::new("Edited: Run Script to use the changes").font(look::regular(11.0)).color(look::text_dim()));
            }
        });
        ui.add_space(4.0);
        let console_h = 150.0;
        let code_h = (ui.available_height() - console_h - 8.0).max(120.0);
        let err_line = syntax.as_ref().map(|e| e.line).or_else(|| if ed.buffer == script { view.and_then(|v| v.error.as_ref()).map(|e| e.0).filter(|l| *l > 0) } else { None });
        // the code, with the line numbers in a gutter
        let frame = egui::Frame::new().fill(if kn_ui::theme::is_light() { Color32::WHITE } else { Color32::from_rgb(0x1f, 0x1f, 0x24) }).stroke(Stroke::new(1.0, look::line())).corner_radius(4.0);
        frame.show(ui, |ui| {
            ui.set_min_height(code_h);
            egui::ScrollArea::both().id_salt(("script-code", t, k)).max_height(code_h).auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let gutter_w = 36.0;
                    let (gutter, _) = ui.allocate_exact_size(vec2(gutter_w, 10.0), Sense::hover());
                    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _w: f32| {
                        let job = highlight(text.as_str(), mono(12.0));
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let te = egui::TextEdit::multiline(&mut ed.buffer).code_editor().frame(egui::Frame::NONE).desired_width(f32::INFINITY).desired_rows(20).lock_focus(true).layouter(&mut layouter).show(ui);
                    let p = ui.painter();
                    let mut line = 1u32;
                    let mut new_line = true;
                    for row in &te.galley.rows {
                        let r = row.rect().translate(te.galley_pos.to_vec2());
                        if new_line {
                            let col = if Some(line) == err_line { look::record_red() } else { look::text_dim() };
                            p.text(pos2(gutter.min.x + gutter_w - 6.0, r.center().y), Align2::RIGHT_CENTER, line.to_string(), mono(11.0), col);
                            if Some(line) == err_line {
                                let band = Rect::from_min_max(pos2(gutter.min.x, r.min.y), pos2(te.galley_pos.x + te.galley.rect.width().max(ui.available_width()), r.max.y));
                                p.rect_filled(band, 0.0, look::record_red().gamma_multiply(0.18));
                            }
                        }
                        new_line = row.ends_with_newline;
                        if new_line {
                            line += 1;
                        }
                    }
                });
            });
        });
        if let Some(e) = &syntax {
            ui.colored_label(look::record_red(), format!("Line {}: {}", e.line, e.msg));
        } else {
            ui.add_space(2.0);
        }
        // the Interactive Console
        ui.label(egui::RichText::new("Console").font(look::semibold(11.0)).color(look::text_dim()));
        let lines: Vec<String> = view.map(|v| v.console.clone()).unwrap_or_default();
        egui::Frame::new().fill(look::lcd_bg()).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt(("script-console", t, k)).max_height(console_h - 54.0).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                for l in &lines {
                    let col = if l.contains("Error") || l.starts_with("Script stopped") { look::record_red() } else { look::lcd_text() };
                    ui.label(egui::RichText::new(l).font(mono(11.0)).color(col));
                }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(">").font(mono(11.0)).color(look::lcd_dim()));
                let te = ui.add(egui::TextEdit::singleline(&mut ed.input).font(mono(11.0)).frame(egui::Frame::NONE).desired_width(f32::INFINITY).hint_text("Type code and press Return"));
                if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let code = std::mem::take(&mut ed.input);
                    if code.trim() == "clear" {
                        out.commands.push("clear".into());
                    } else if !code.trim().is_empty() {
                        out.commands.push(format!("eval:{code}"));
                    }
                    te.request_focus();
                }
            });
        });
    });
    if !open {
        ed.open = None;
    }
    out
}
