//! Staff styles (Logic: Layout ▸ Staff Styles, the Staff Style window): Logic's predefined
//! styles as editable definitions, the project's own styles (New ▸ Single / Dual / Mapped,
//! duplicate, rename, delete, copy and paste between projects, Select Unused), and the window
//! with its style overview (type, voices, size, transposition, range, brackets and bar line
//! connections, a live picture), the Staff parameters (clef, transposition, key, size, space
//! above and below) and the Voice and Assign parameters (staff, rest, stem, stem position, tie,
//! tuplet, beam, colour, head, split, channel). The score reads every parameter (score.rs).

use crate::app::App;
use crate::score;
use egui::{vec2, Color32};
use lg_core::staffstyle::{self as ss, StaffDef, StaffStyleDef, VoiceDef};
use lg_notation as nt;
use std::sync::{Mutex, OnceLock};

pub const COMMANDS: &[&str] = &["Show/Hide Staff Style Window"];

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if name != COMMANDS[0] {
        return false;
    }
    app.bottom = crate::app::Bottom::PianoRoll;
    score::set_editor_tab(ctx, score::EditorTab::Score);
    score::with_state(ctx, |s| s.styles_window = !s.styles_window);
    true
}

/// The clef name a predefined style's clef and octave sign stand for.
fn clef_name(c: nt::Clef, octave: i8) -> &'static str {
    match (c, octave) {
        (nt::Clef::Treble, o) if o > 0 => "Treble 8va",
        (nt::Clef::Treble, o) if o < 0 => "Treble 8vb",
        (nt::Clef::Treble, _) => "Treble",
        (nt::Clef::Bass, o) if o < 0 => "Bass 8vb",
        (nt::Clef::Bass, o) if o > 0 => "Bass 8va",
        (nt::Clef::Bass, _) => "Bass",
        (nt::Clef::Alto, _) => "Alto",
        (nt::Clef::Tenor, _) => "Tenor",
        (nt::Clef::Percussion, _) => "Percussion",
    }
}

/// lg-notation's clef and octave sign for a Clef pop-up name.
pub fn nt_clef(name: &str) -> (nt::Clef, i8) {
    let oct = if name.ends_with("8va") { 1 } else if name.ends_with("15ma") { 2 } else if name.ends_with("8vb") { -1 } else { 0 };
    let c = if name.starts_with("Bass") {
        nt::Clef::Bass
    } else if name == "Alto" {
        nt::Clef::Alto
    } else if name == "Tenor" {
        nt::Clef::Tenor
    } else if name.starts_with("Drum.") || name.starts_with("no clef.") || name == "Percussion" {
        nt::Clef::Percussion
    } else {
        nt::Clef::Treble
    };
    (c, oct)
}

/// A predefined style as the Staff Style window edits it.
pub fn builtin_def(s: nt::StaffStyle) -> StaffStyleDef {
    let mapped = s.clefs[0] == nt::Clef::Percussion;
    let mut d = if s.clefs.len() > 1 { StaffStyleDef::dual(s.name) } else if mapped { StaffStyleDef::mapped_style(s.name) } else { StaffStyleDef::single(s.name) };
    for (k, c) in s.clefs.iter().enumerate() {
        if let Some(st) = d.staffs.get_mut(k) {
            st.clef = StaffDef::clef_index(clef_name(*c, if k == 0 { s.octave } else { 0 }));
            st.transpose = s.transpose + 12 * s.octave;
        }
    }
    if s.clefs.len() > 1 {
        d.voices[0].split = s.split;
    }
    while d.staffs.len() < s.clefs.len() {
        let k = d.staffs.len();
        let st = StaffDef { clef: StaffDef::clef_index(clef_name(s.clefs[k], 0)), transpose: s.transpose, ..Default::default() };
        d.add_staff(Some(k - 1), st);
    }
    // a polyphonic style (Piano 1/3+4, Organ 1/1/5): a voice per MIDI channel; a channel on two
    // staffs splits at the style's split pitch
    if let Some(layout) = s.channel_layout() {
        d.voices.clear();
        for (k, chans) in layout.iter().enumerate() {
            for &c in chans {
                let lower = layout[k + 1..].iter().any(|l| l.contains(&c));
                d.voices.push(VoiceDef { staff: k as u8, channel: c, split: if lower { s.split } else { 0 }, ..Default::default() });
            }
        }
    }
    d
}

pub fn is_builtin(name: &str) -> bool {
    nt::StaffStyle::by_name(name).is_some()
}

/// Every style: Logic's predefined ones (as the project edited them), then the project's own.
pub fn all(app: &App) -> Vec<StaffStyleDef> {
    let own: &[StaffStyleDef] = app.project.as_ref().map_or(&[], |p| &p.score.staff_styles);
    let mut out: Vec<StaffStyleDef> = nt::StaffStyle::ALL.iter().map(|s| own.iter().find(|d| d.name == s.name).cloned().unwrap_or_else(|| builtin_def(*s))).collect();
    out.extend(own.iter().filter(|d| !is_builtin(&d.name)).cloned());
    out
}

pub fn def_named(app: &App, name: &str) -> Option<StaffStyleDef> {
    if name.is_empty() {
        return None;
    }
    if let Some(d) = app.project.as_ref().and_then(|p| p.score.staff_styles.iter().find(|d| d.name == name)) {
        return Some(d.clone());
    }
    nt::StaffStyle::by_name(name).map(builtin_def)
}

fn intern_str(s: &str) -> &'static str {
    static M: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let mut v = M.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = v.iter().find(|x| **x == s) {
        return x;
    }
    let x: &'static str = Box::leak(s.to_string().into_boxed_str());
    v.push(x);
    x
}

fn intern_clefs(c: Vec<nt::Clef>) -> &'static [nt::Clef] {
    static M: OnceLock<Mutex<Vec<&'static [nt::Clef]>>> = OnceLock::new();
    let mut v = M.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = v.iter().find(|x| **x == c.as_slice()) {
        return x;
    }
    let x: &'static [nt::Clef] = Box::leak(c.into_boxed_slice());
    v.push(x);
    x
}

/// lg-notation's view of a style (clefs, display transposition, split; the name kept).
pub fn to_nt(d: &StaffStyleDef) -> nt::StaffStyle {
    let clefs: Vec<nt::Clef> = d.staffs.iter().map(|s| nt_clef(s.clef_name()).0).collect();
    let clefs = if clefs.is_empty() { vec![nt::Clef::Treble] } else { clefs };
    let first = d.staffs.first().cloned().unwrap_or_default();
    let (_, oct) = nt_clef(first.clef_name());
    let transpose = if d.mapped { 0 } else { first.transpose - 12 * oct };
    let split = d.voices.iter().find(|v| v.staff == 0 && v.split > 0).map_or(60, |v| v.split);
    nt::StaffStyle { name: intern_str(&d.name), clefs: intern_clefs(clefs), split, transpose, octave: oct.signum() }
}

/// Changes a style (one undo step): a predefined one gets a project copy of its own.
pub fn edit(app: &mut App, name: &str, what: &str, f: impl FnOnce(&mut StaffStyleDef)) {
    let Some(mut d) = def_named(app, name) else { return };
    let old = d.name.clone();
    f(&mut d);
    app.checkpoint(what);
    if let Some(p) = app.project.as_mut() {
        let list = &mut p.score.staff_styles;
        if let Some(k) = list.iter().position(|x| x.name == old) {
            list[k] = d.clone();
        } else {
            list.push(d.clone());
        }
        // a renamed style: its tracks follow
        if d.name != old {
            for ts in p.score.tracks.values_mut() {
                if ts.style == old {
                    ts.style = d.name.clone();
                }
            }
        }
    }
    app.after_edit();
}

/// Adds a new style (unique name) and returns its name.
pub fn add(app: &mut App, mut d: StaffStyleDef, what: &str) -> String {
    let names: Vec<String> = all(app).into_iter().map(|d| d.name).collect();
    let base = d.name.clone();
    let mut k = 1;
    while names.contains(&d.name) {
        k += 1;
        d.name = format!("{base} {k}");
    }
    let name = d.name.clone();
    app.checkpoint(what);
    if let Some(p) = app.project.as_mut() {
        p.score.staff_styles.push(d);
    }
    app.after_edit();
    name
}

/// Deletes styles (a predefined one goes back to Logic's own); their tracks become automatic.
pub fn delete(app: &mut App, names: &[String]) {
    app.checkpoint("Delete Staff Style");
    if let Some(p) = app.project.as_mut() {
        p.score.staff_styles.retain(|d| !names.contains(&d.name));
        for ts in p.score.tracks.values_mut() {
            if names.contains(&ts.style) && !is_builtin(&ts.style) {
                ts.style.clear();
            }
        }
    }
    app.after_edit();
}

/// Styles no track uses (Edit ▸ Select Unused).
pub fn unused(app: &App) -> Vec<String> {
    let used: Vec<String> = (0..app.project.as_ref().map_or(0, |p| p.tracks.len())).map(|t| score::style_of(app, t).name.to_string()).collect();
    all(app).into_iter().map(|d| d.name).filter(|n| !used.contains(n)).collect()
}

/// Pastes styles copied from (another) project: missing ones are added, same-named replaced.
pub fn paste_styles(app: &mut App, v: &[StaffStyleDef]) {
    if v.is_empty() {
        return;
    }
    app.checkpoint("Paste Staff Styles");
    if let Some(p) = app.project.as_mut() {
        for d in v {
            p.score.staff_styles.retain(|x| x.name != d.name);
            p.score.staff_styles.push(d.clone());
        }
    }
    app.after_edit();
}

/// The window's own state (kept in egui memory; the copied styles app-wide, for pasting into
/// another project).
#[derive(Clone, Default)]
struct Win {
    cur: String,
    /// 0 everything (the default), 1 overview only, 2 parameters only.
    view: u8,
    list: bool,
    list_sel: Vec<String>,
    staff: Option<usize>,
    voice: Option<usize>,
    renaming: Option<String>,
    clip_staff: Option<StaffDef>,
    clip_voice: Option<VoiceDef>,
}

fn win_id() -> egui::Id {
    egui::Id::unique("lg-staff-style-window")
}

fn clip() -> &'static Mutex<Vec<StaffStyleDef>> {
    static C: OnceLock<Mutex<Vec<StaffStyleDef>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Opens the window on a style (double-click on a staff).
pub fn open_on(ctx: &egui::Context, name: &str) {
    ctx.data_mut(|m| {
        let w = m.get_temp_mut_or_default::<Win>(win_id());
        w.cur = name.to_string();
        w.list = false;
    });
    // (the Score's state is written back at the end of its frame: it opens the window then)
    ctx.data_mut(|m| m.insert_temp(egui::Id::unique("lg-ssw-open"), true));
}

/// A double-click asked for the window (read once by the Score).
pub fn take_open_request(ctx: &egui::Context) -> bool {
    ctx.data_mut(|m| m.remove_temp::<bool>(egui::Id::unique("lg-ssw-open"))).unwrap_or(false)
}

fn menu_id() -> egui::Id {
    egui::Id::unique("lg-staff-style-clef-menu")
}

pub fn open_clef_menu(ctx: &egui::Context, at: egui::Pos2, track: usize) {
    ctx.data_mut(|m| m.insert_temp(menu_id(), Some((at, track))));
}

pub fn close_clef_menu(ctx: &egui::Context) {
    ctx.data_mut(|m| m.insert_temp::<Option<(egui::Pos2, usize)>>(menu_id(), None));
}

/// The shortcut menu a click on a clef opens in Linear view: the staff styles.
pub fn clef_menu(app: &mut App, ctx: &egui::Context) {
    let Some((at, track)) = ctx.data(|m| m.get_temp::<Option<(egui::Pos2, usize)>>(menu_id())).flatten() else { return };
    let cur = score::style_of(app, track).name.to_string();
    let mut pick = None;
    let r = egui::Area::new(menu_id().with("area")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for d in all(app) {
                    if ui.selectable_label(d.name == cur, d.shown_name()).clicked() {
                        pick = Some(d.name.clone());
                    }
                }
            });
        });
    });
    if let Some(n) = pick {
        score::set_style(app, track, &n);
        close_clef_menu(ctx);
    } else if ctx.input(|i| i.pointer.any_pressed()) && !r.response.contains_pointer() && ctx.input(|i| i.pointer.press_origin()).is_some_and(|p| (p - at).length() > 4.0) {
        close_clef_menu(ctx);
    } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close_clef_menu(ctx);
    }
}

fn popup(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, cur: usize, names: &[&str], w: f32) -> Option<usize> {
    let mut out = None;
    egui::ComboBox::from_id_salt(id).width(w).selected_text(names.get(cur).copied().unwrap_or("–")).show_ui(ui, |ui| {
        for (k, n) in names.iter().enumerate() {
            if ui.selectable_label(k == cur, *n).clicked() {
                out = Some(k);
            }
        }
    });
    out
}

fn pitch_name(p: u8) -> String {
    const N: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
    format!("{}{}", N[(p % 12) as usize], p as i32 / 12 - 2)
}

/// A value dragged up and down (Logic's number fields).
fn drag_num(ui: &mut egui::Ui, v: i32, range: std::ops::RangeInclusive<i32>, fmt: impl Fn(i32) -> String + 'static) -> Option<i32> {
    let mut x = v;
    let r = ui.add(egui::DragValue::new(&mut x).range(range).speed(0.25).custom_formatter(move |n, _| fmt(n as i32)));
    (r.changed() && x != v).then_some(x)
}

const COLOR_NAMES: [&str; ss::COLOR_MODES] = [
    "Black", "Pitch", "Velocity", "Color 1", "Color 2", "Color 3", "Color 4", "Color 5", "Color 6", "Color 7", "Color 8", "Color 9", "Color 10", "Color 11", "Color 12", "Color 13", "Color 14", "Color 15", "Color 16",
];

/// The Staff Style window.
pub fn window(app: &mut App, ctx: &egui::Context, open: &mut bool, track: usize) {
    let mut w: Win = ctx.data(|m| m.get_temp(win_id())).unwrap_or_default();
    let styles = all(app);
    if w.cur.is_empty() || !styles.iter().any(|d| d.name == w.cur) {
        w.cur = score::style_of(app, track).name.to_string();
    }
    let Some(cur) = styles.iter().find(|d| d.name == w.cur).cloned() else { return };
    let name = cur.name.clone();
    egui::Window::new("Staff Style").open(open).resizable(true).collapsible(false).default_size(vec2(780.0, 420.0)).show(ctx, |ui| {
        // local menu bar: New, Edit, View + the three View buttons
        ui.horizontal(|ui| {
            ui.menu_button("New", |ui| {
                for (label, mk) in [("Single Staff Style", 0), ("Dual Staff Style", 1), ("Mapped Style", 2)] {
                    if ui.button(label).clicked() {
                        let d = match mk {
                            0 => StaffStyleDef::single("Single Staff"),
                            1 => StaffStyleDef::dual("Dual Staff"),
                            _ => StaffStyleDef::mapped_style("Mapped Style"),
                        };
                        w.cur = add(app, d, &format!("New {label}"));
                        ui.close();
                    }
                }
                if ui.button("Duplicate Style").clicked() {
                    let mut d = cur.clone();
                    d.name = format!("{} copy", cur.name);
                    w.cur = add(app, d, "Duplicate Staff Style");
                    ui.close();
                }
                if ui.add_enabled(!w.list, egui::Button::new("Delete Style")).clicked() {
                    delete(app, std::slice::from_ref(&name));
                    w.cur.clear();
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Copy").clicked() {
                    copy(&mut w, &styles, &cur);
                    ui.close();
                }
                if ui.button("Paste").clicked() {
                    paste(app, &mut w);
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    delete_sel(app, &mut w, &cur);
                    ui.close();
                }
                if ui.add_enabled(w.list, egui::Button::new("Select Unused")).clicked() {
                    w.list_sel = unused(app);
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.radio(!w.list, "Single").clicked() {
                    w.list = false;
                    ui.close();
                }
                if ui.radio(w.list, "List").clicked() {
                    w.list = true;
                    ui.close();
                }
            });
            ui.add_space(12.0);
            for (k, tip, icon) in [(1u8, "Style overview only", "▭"), (0, "Whole window", "▤"), (2, "Staff and voice parameters only", "▦")] {
                if ui.selectable_label(w.view == k, icon).on_hover_text(tip).clicked() {
                    w.view = k;
                }
            }
            ui.add_space(12.0);
            // the style shown (Single view)
            if !w.list {
                egui::ComboBox::from_id_salt("ssw-cur").width(180.0).selected_text(cur.shown_name()).show_ui(ui, |ui| {
                    for d in &styles {
                        if ui.selectable_label(d.name == cur.name, d.shown_name()).clicked() {
                            w.cur = d.name.clone();
                        }
                    }
                });
                if ui.button("Assign to Track").on_hover_text("Sets this style on the selected track").clicked() {
                    score::set_style(app, track, &name);
                }
            }
        });
        // keys in the window: copy, paste, delete
        let (copy_k, paste_k, del_k) = ui.input(|i| (i.events.iter().any(|e| matches!(e, egui::Event::Copy)), i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))), i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)));
        if !ctx.egui_wants_keyboard_input() && crate::focus::in_window(app, ui) {
            if copy_k {
                copy(&mut w, &styles, &cur);
            }
            if paste_k {
                paste(app, &mut w);
            }
            if del_k {
                delete_sel(app, &mut w, &cur);
            }
        }
        ui.separator();
        if w.list {
            list_view(app, ui, &mut w, &styles);
        } else {
            if w.view != 2 {
                overview(app, ui, &mut w, &cur);
            }
            if w.view != 1 {
                ui.separator();
                params(app, ui, &mut w, &cur);
            }
            // a double-click below the parameters: List view
            let rest = ui.allocate_response(vec2(ui.available_width(), 24.0), egui::Sense::click());
            if rest.double_clicked() {
                w.list = true;
            }
        }
    });
    ctx.data_mut(|m| m.insert_temp(win_id(), w));
}

fn copy(w: &mut Win, styles: &[StaffStyleDef], cur: &StaffStyleDef) {
    if w.list {
        *clip().lock().unwrap_or_else(|e| e.into_inner()) = styles.iter().filter(|d| w.list_sel.contains(&d.name)).cloned().collect();
    } else if let Some(v) = w.voice.and_then(|k| cur.voices.get(k)) {
        w.clip_voice = Some(v.clone());
    } else if let Some(s) = w.staff.and_then(|k| cur.staffs.get(k)) {
        w.clip_staff = Some(s.clone());
    } else {
        *clip().lock().unwrap_or_else(|e| e.into_inner()) = vec![cur.clone()];
    }
}

fn paste(app: &mut App, w: &mut Win) {
    if w.list || (w.clip_staff.is_none() && w.clip_voice.is_none()) {
        let v = clip().lock().unwrap_or_else(|e| e.into_inner()).clone();
        paste_styles(app, &v);
        return;
    }
    let (sv, ss_) = (w.voice, w.staff);
    if let Some(v) = w.clip_voice.clone() {
        edit(app, &w.cur.clone(), "Paste Voice", |d| {
            d.paste_voice(sv, ss_, v);
        });
    } else if let Some(s) = w.clip_staff.clone() {
        edit(app, &w.cur.clone(), "Paste Staff", |d| {
            d.paste_staff(ss_, s);
        });
    }
}

fn delete_sel(app: &mut App, w: &mut Win, cur: &StaffStyleDef) {
    if w.list {
        let names = std::mem::take(&mut w.list_sel);
        delete(app, &names);
    } else if let Some(k) = w.voice {
        edit(app, &cur.name, "Delete Voice", |d| {
            d.delete_voice(k);
        });
        w.voice = None;
    } else if let Some(k) = w.staff {
        edit(app, &cur.name, "Delete Staff", |d| {
            d.delete_staff(k);
        });
        w.staff = None;
    }
}

fn list_view(app: &mut App, ui: &mut egui::Ui, w: &mut Win, styles: &[StaffStyleDef]) {
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        egui::Grid::new("ssw-list").striped(true).show(ui, |ui| {
            for h in ["Name", "Type", "Voices", "Size", "Transp."] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for d in styles {
                let on = w.list_sel.contains(&d.name);
                let r = ui.selectable_label(on, d.shown_name());
                if r.clicked() {
                    if ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                        if on {
                            w.list_sel.retain(|x| x != &d.name);
                        } else {
                            w.list_sel.push(d.name.clone());
                        }
                    } else {
                        w.list_sel = vec![d.name.clone()];
                    }
                }
                if r.double_clicked() {
                    w.cur = d.name.clone();
                    w.list = false;
                }
                ui.label(d.type_name());
                ui.label(d.voices.len().to_string());
                ui.label(d.common_size().map_or("–".into(), |s| s.to_string()));
                ui.label(d.common_transpose().map_or("–".into(), |s| s.to_string()));
                ui.end_row();
            }
        });
    });
    let _ = app;
}

/// The style overview: name, type, voices, size, transposition, range, brackets / bar lines,
/// and a picture of the style.
fn overview(app: &mut App, ui: &mut egui::Ui, w: &mut Win, cur: &StaffStyleDef) {
    let name = cur.name.clone();
    ui.horizontal_top(|ui| {
        egui::Grid::new("ssw-over").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Name");
            if let Some(t) = w.renaming.as_mut() {
                let te = ui.add(egui::TextEdit::singleline(t).desired_width(160.0));
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    let new = t.trim().to_string();
                    w.renaming = None;
                    if !new.is_empty() && new != name && !all(app).iter().any(|d| d.name == new) {
                        edit(app, &name, "Rename Staff Style", |d| d.name = new.clone());
                        w.cur = new;
                    }
                }
            } else if ui.add(egui::Label::new(egui::RichText::new(cur.shown_name()).strong()).sense(egui::Sense::click())).on_hover_text("Double-click to rename").double_clicked() {
                w.renaming = Some(name.clone());
            }
            ui.end_row();
            ui.label("Type");
            let types = ["Single", "Dual", "Mapped"];
            let k = if cur.mapped { 2 } else if cur.staffs.len() > 1 { 1 } else { 0 };
            if let Some(t) = popup(ui, "ssw-type", k, &types, 100.0) {
                if t != k {
                    edit(app, &name, "Staff Style Type", |d| match t {
                        0 => {
                            d.mapped = false;
                            while d.staffs.len() > 1 {
                                d.delete_staff(d.staffs.len() - 1);
                            }
                        }
                        1 => {
                            d.mapped = false;
                            if d.staffs.len() < 2 {
                                d.add_staff(Some(0), StaffDef { clef: StaffDef::clef_index("Bass"), ..Default::default() });
                                d.voices[0].split = 60;
                                d.bracket = 1;
                                d.barline = 1;
                            }
                        }
                        _ => {
                            d.mapped = true;
                            d.staffs.iter_mut().for_each(|s| {
                                s.clef = StaffDef::clef_index("Percussion");
                                s.transpose = 0;
                            });
                        }
                    });
                }
            }
            ui.end_row();
            ui.label("Voices");
            ui.label(cur.voices.len().to_string()).on_hover_text("Set by adding or deleting voices in the Voice parameters");
            ui.end_row();
            ui.label("Size");
            match cur.common_size() {
                Some(s) => {
                    if let Some(v) = drag_num(ui, s as i32, 25..=250, |n| format!("{n} %")) {
                        edit(app, &name, "Staff Style Size", |d| d.set_size(v as u8));
                    }
                }
                None => {
                    if let Some(v) = drag_num(ui, cur.staffs[0].size as i32, 25..=250, |_| "–".into()) {
                        edit(app, &name, "Staff Style Size", |d| d.set_size(v as u8));
                    }
                }
            }
            ui.end_row();
            ui.label("Transpose");
            if cur.mapped {
                ui.label("–");
            } else {
                let tr: Vec<String> = (-24..=24).map(|n: i32| if n == 0 { "0".to_string() } else { format!("{n:+}") }).collect();
                let names: Vec<&str> = std::iter::once("–").chain(tr.iter().map(|s| s.as_str())).collect();
                let k = cur.common_transpose().map_or(0, |t| (t as i32 + 25) as usize);
                if let Some(t) = popup(ui, "ssw-tr", k, &names, 60.0) {
                    if t > 0 {
                        edit(app, &name, "Staff Style Transpose", |d| d.set_transpose(t as i8 - 25));
                    }
                }
            }
            ui.end_row();
            ui.label("Range");
            ui.horizontal(|ui| {
                ui.label("Lo");
                if let Some(v) = drag_num(ui, cur.lo as i32, 0..=127, |n| pitch_name(n as u8)) {
                    edit(app, &name, "Staff Style Range", |d| d.lo = (v as u8).min(d.hi));
                }
                ui.label("Hi");
                if let Some(v) = drag_num(ui, cur.hi as i32, 0..=127, |n| pitch_name(n as u8)) {
                    edit(app, &name, "Staff Style Range", |d| d.hi = (v as u8).max(d.lo));
                }
            });
            ui.end_row();
            ui.label("Brackets");
            if cur.staffs.len() > 1 {
                if let Some(b) = popup(ui, "ssw-br", cur.bracket as usize, &ss::BRACKETS, 100.0) {
                    edit(app, &name, "Staff Style Bracket", |d| d.bracket = b as u8);
                }
            } else {
                ui.weak("multi-staff styles only");
            }
            ui.end_row();
            ui.label("Bar Lines");
            if cur.staffs.len() > 1 {
                if let Some(b) = popup(ui, "ssw-bl", cur.barline as usize, &ss::BARLINES, 100.0) {
                    edit(app, &name, "Staff Style Bar Lines", |d| d.barline = b as u8);
                }
            } else {
                ui.weak("multi-staff styles only");
            }
            ui.end_row();
        });
        ui.add_space(16.0);
        picture(ui, cur);
    });
}

/// The overview's picture: the staffs with their clefs, brace or bracket (updated live).
fn picture(ui: &mut egui::Ui, d: &StaffStyleDef) {
    let sp = 5.0;
    let h: f32 = d.staffs.iter().map(|s| 9.0 * sp * s.size as f32 / 100.0 + (s.above + s.below) as f32 * sp / 2.0).sum::<f32>() + 10.0;
    let (r, p) = ui.allocate_painter(vec2(200.0, h.clamp(60.0, 400.0)), egui::Sense::hover());
    let rect = r.rect;
    p.rect_filled(rect, 4.0, Color32::from_rgb(0xfe, 0xff, 0xf3));
    let ink = Color32::from_rgb(0x10, 0x10, 0x10);
    let mut y = rect.min.y + 6.0;
    let mut tops = Vec::new();
    for s in &d.staffs {
        let k = sp * s.size as f32 / 100.0;
        y += 2.0 * k + s.above as f32 * k / 2.0;
        let lines = s.lines().max(1);
        for l in 0..lines {
            p.hline(rect.min.x + 20.0..=rect.max.x - 8.0, y + l as f32 * k, egui::Stroke::new(1.0, ink));
        }
        let bottom = y + (lines.max(1) - 1) as f32 * k;
        tops.push((y, bottom));
        // the clef (Bravura), on its line
        let (c, _) = nt_clef(s.clef_name());
        let (ch, line) = match c {
            nt::Clef::Treble => ('\u{E050}', 1.0),
            nt::Clef::Bass => ('\u{E062}', 3.0),
            nt::Clef::Alto => ('\u{E05C}', 2.0),
            nt::Clef::Tenor => ('\u{E05C}', 3.0),
            nt::Clef::Percussion => ('\u{E069}', 2.0),
        };
        if lines >= 5 && !s.clef_name().starts_with("no clef") {
            score::smufl(&p, ch, egui::pos2(rect.min.x + 26.0, bottom - line * k), k, ink, false);
        }
        p.text(egui::pos2(rect.max.x - 10.0, (y + bottom) / 2.0), egui::Align2::RIGHT_CENTER, s.clef_name(), egui::FontId::proportional(9.0), Color32::from_gray(0x70));
        y = bottom + 3.0 * k + s.below as f32 * k / 2.0;
    }
    if let (Some(f), Some(l)) = (tops.first(), tops.last()) {
        if d.staffs.len() > 1 {
            let x = rect.min.x + 20.0;
            if d.barline > 0 {
                p.vline(x, f.0..=l.1, egui::Stroke::new(1.0, ink));
            }
            match d.bracket {
                1 => {
                    p.text(egui::pos2(x - 6.0, (f.0 + l.1) / 2.0), egui::Align2::CENTER_CENTER, "{", egui::FontId::proportional(l.1 - f.0 + 4.0), ink);
                }
                2 => {
                    p.vline(x - 6.0, f.0 - 2.0..=l.1 + 2.0, egui::Stroke::new(3.0, ink));
                }
                _ => {}
            }
        }
    }
}

/// Staff parameters (one row per staff) and Voice / Assign parameters (one row per voice).
fn params(app: &mut App, ui: &mut egui::Ui, w: &mut Win, cur: &StaffStyleDef) {
    let name = cur.name.clone();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Staff").strong());
                if ui.small_button("+").on_hover_text("Add Staff").clicked() {
                    let after = w.staff;
                    edit(app, &name, "Add Staff", |d| {
                        let k = d.add_staff(after, StaffDef::default());
                        w.staff = Some(k);
                    });
                    w.voice = None;
                }
            });
            egui::Grid::new("ssw-staffs").striped(true).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                let mut cols = vec!["#", "Space Above", "Space Below", "Size", "Clef"];
                if !cur.mapped {
                    cols.extend(["Transpose", "Key"]);
                }
                for h in cols {
                    ui.label(egui::RichText::new(h).small().strong());
                }
                ui.end_row();
                for (k, s) in cur.staffs.iter().enumerate() {
                    if ui.selectable_label(w.staff == Some(k) && w.voice.is_none(), (k + 1).to_string()).clicked() {
                        w.staff = Some(k);
                        w.voice = None;
                    }
                    if let Some(v) = drag_num(ui, s.above as i32, -40..=80, |n| n.to_string()) {
                        edit(app, &name, "Staff Space Above", |d| d.staffs[k].above = v as i8);
                    }
                    if let Some(v) = drag_num(ui, s.below as i32, -40..=80, |n| n.to_string()) {
                        edit(app, &name, "Staff Space Below", |d| d.staffs[k].below = v as i8);
                    }
                    if let Some(v) = drag_num(ui, s.size as i32, 25..=250, |n| format!("{n} %")) {
                        edit(app, &name, "Staff Size", |d| d.staffs[k].size = v as u8);
                    }
                    let clefs: Vec<&str> = ss::CLEFS.to_vec();
                    if let Some(c) = popup(ui, ("ssw-clef", k), s.clef as usize, &clefs, 90.0) {
                        edit(app, &name, "Staff Clef", |d| d.staffs[k].clef = c as u8);
                    }
                    if !cur.mapped {
                        if let Some(v) = drag_num(ui, s.transpose as i32, -36..=36, |n| if n == 0 { "0".into() } else { format!("{n:+}") }) {
                            edit(app, &name, "Staff Transpose", |d| d.staffs[k].transpose = v as i8);
                        }
                        let mut key = s.key;
                        if ui.checkbox(&mut key, "").changed() {
                            edit(app, &name, "Staff Key", |d| d.staffs[k].key = key);
                        }
                    }
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Voice").strong());
                if ui.small_button("+").on_hover_text("Add Voice").clicked() {
                    let (after, staff) = (w.voice, w.staff);
                    edit(app, &name, "Add Voice", |d| {
                        w.voice = d.add_voice(after, staff);
                    });
                }
                ui.add_space(40.0);
                ui.label(egui::RichText::new("Assign").strong());
            });
            egui::Grid::new("ssw-voices").striped(true).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                let mut cols = vec![];
                if cur.mapped {
                    cols.push("Voice");
                }
                cols.extend(["Staff", "Rest", "Stem", "Stem Pos.", "Tie", "Tuplet", "Beam", "Color", "Head", "Split", "Chan"]);
                for h in cols {
                    ui.label(egui::RichText::new(h).small().strong());
                }
                ui.end_row();
                for (k, v) in cur.voices.iter().enumerate() {
                    if cur.mapped {
                        ui.label((k + 1).to_string());
                    }
                    if ui.selectable_label(w.voice == Some(k), (v.staff + 1).to_string()).clicked() {
                        w.voice = Some(k);
                        w.staff = Some(v.staff as usize);
                    }
                    let mut set = |ui: &mut egui::Ui, id: &str, val: u8, names: &[&str], wd: f32, what: &str, f: fn(&mut VoiceDef, u8)| {
                        if let Some(x) = popup(ui, (id, k), val as usize, names, wd) {
                            edit(app, &name, what, |d| f(&mut d.voices[k], x as u8));
                        }
                    };
                    set(ui, "r", v.rest, &ss::RESTS, 56.0, "Voice Rest", |v, x| v.rest = x);
                    set(ui, "s", v.stem, &ss::STEMS, 50.0, "Voice Stem", |v, x| v.stem = x);
                    set(ui, "sp", v.stem_pos, &ss::STEM_POS, 56.0, "Voice Stem Position", |v, x| v.stem_pos = x);
                    set(ui, "t", v.tie, &ss::TIES, 50.0, "Voice Tie", |v, x| v.tie = x);
                    set(ui, "tu", v.tuplet, &ss::TUPLETS, 50.0, "Voice Tuplet", |v, x| v.tuplet = x);
                    set(ui, "b", v.beam, &ss::BEAMS, 54.0, "Voice Beam", |v, x| v.beam = x);
                    set(ui, "c", v.color, &COLOR_NAMES, 64.0, "Voice Color", |v, x| v.color = x);
                    set(ui, "h", v.head, &ss::HEADS, 80.0, "Voice Head", |v, x| v.head = x);
                    if let Some(x) = drag_num(ui, v.split as i32, 0..=127, |n| if n == 0 { "–".into() } else { pitch_name(n as u8) }) {
                        edit(app, &name, "Voice Split", |d| d.voices[k].split = x as u8);
                    }
                    if let Some(x) = drag_num(ui, v.channel as i32, 0..=16, |n| if n == 0 { "All".into() } else { n.to_string() }) {
                        edit(app, &name, "Voice Channel", |d| d.voices[k].channel = x as u8);
                    }
                    ui.end_row();
                }
            });
            if cur.mapped {
                drum_groups(app, ui, w, cur);
            }
        });
    });
}

/// A mapped style's drum groups: under each staff and voice, the groups the voice shows, each
/// with its Pos; + adds one to the selected voice.
fn drum_groups(app: &mut App, ui: &mut egui::Ui, w: &mut Win, cur: &StaffStyleDef) {
    let name = cur.name.clone();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Drum Groups").strong());
        if ui.small_button("+").on_hover_text("Add a drum group to the selected voice").clicked() {
            let k = w.voice.unwrap_or(0).min(cur.voices.len().saturating_sub(1));
            let used: Vec<String> = cur.voices.iter().flat_map(|v| v.groups.iter().map(|g| g.0.clone())).collect();
            let g = crate::scoredrums::groups(app).into_iter().find(|g| !used.contains(g)).unwrap_or_else(|| "Kick".into());
            edit(app, &name, "Add Drum Group", |d| d.voices[k].groups.push((g, 0)));
        }
    });
    let all = crate::scoredrums::groups(app);
    let names: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
    let line = ui.visuals().widgets.noninteractive.bg_stroke;
    for (s, _) in cur.staffs.iter().enumerate() {
        // (the hierarchy: a line above each staff, voice and group row)
        ui.add(egui::Separator::default().spacing(2.0));
        ui.label(egui::RichText::new(format!("Staff {}", s + 1)).small().strong());
        for k in cur.voices_on(s) {
            let v = &cur.voices[k];
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let r = ui.selectable_label(w.voice == Some(k), egui::RichText::new(format!("Voice {}", k + 1)).small());
                if r.clicked() {
                    w.voice = Some(k);
                    w.staff = Some(s);
                }
                let y = r.rect.top() - 1.0;
                ui.painter().hline(r.rect.left()..=r.rect.left() + 260.0, y, line);
            });
            for (gi, (g, pos)) in v.groups.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_space(28.0);
                    let cur_i = names.iter().position(|n| n == g).unwrap_or(0);
                    if let Some(x) = popup(ui, ("ssw-grp", k, gi), cur_i, &names, 90.0) {
                        let new = names[x].to_string();
                        edit(app, &name, "Drum Group", |d| d.voices[k].groups[gi].0 = new);
                    }
                    ui.label(egui::RichText::new("Pos").small());
                    let mut p = *pos as f32 / 2.0;
                    if ui.add(egui::DragValue::new(&mut p).range(-12.0..=12.0).speed(0.1).fixed_decimals(1)).changed() {
                        let v = (p * 2.0).round() as i8;
                        edit(app, &name, "Drum Group Pos", |d| d.voices[k].groups[gi].1 = v);
                    }
                    if ui.small_button("–").on_hover_text("Remove the group from the voice").clicked() {
                        edit(app, &name, "Remove Drum Group", |d| {
                            d.voices[k].groups.remove(gi);
                        });
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predefined_styles_round_trip_through_definitions() {
        for s in nt::StaffStyle::ALL {
            let back = to_nt(&builtin_def(s));
            assert_eq!((back.name, back.clefs, back.transpose, back.octave), (s.name, s.clefs, s.transpose, s.octave), "{}", s.name);
            if s.clefs.len() > 1 {
                assert_eq!(back.split, s.split);
            }
        }
        let mut d = StaffStyleDef::single("Mine");
        d.staffs[0].clef = StaffDef::clef_index("Treble 15ma");
        let n = to_nt(&d);
        assert_eq!((n.written(60), n.clefs, n.name), (36, &[nt::Clef::Treble][..], "Mine"));
        // interned: the same slice twice
        assert!(std::ptr::eq(to_nt(&d).name, n.name));
    }

    #[test]
    fn polyphonic_predefined_styles_take_voices_by_channel() {
        let organ = builtin_def(nt::StaffStyle::by_name("Organ 1/1/5").unwrap());
        assert_eq!(organ.staffs.len(), 3);
        assert_eq!(organ.staffs.iter().map(|s| s.clef_name()).collect::<Vec<_>>(), ["Treble", "Bass", "Bass"]);
        // channel 1 splits at C3 between the manuals; channel 5 is the pedal staff (channels 0-based)
        assert_eq!((organ.staff_for(72, 0), organ.staff_for(48, 0), organ.staff_for(36, 4)), (0, 1, 2));
        assert!(!organ.takes(1));
        let p = builtin_def(nt::StaffStyle::by_name("Piano 1+2/3+4").unwrap());
        assert_eq!(p.voices.len(), 4);
        assert_eq!((p.staff_for(40, 1), p.staff_for(80, 2), p.staff_for(80, 3)), (0, 1, 1));
        assert_eq!(p.voices_on(1), vec![2, 3]);
        assert_eq!(p.type_name(), "Dual");
        let d = builtin_def(nt::StaffStyle::by_name("# Drums").unwrap());
        assert_eq!((d.mapped, d.shown_name().as_str()), (true, "# Drums"));
    }
}
