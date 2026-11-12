//! Note attributes in the Score, the ways Logic offers them: the local Functions ▸ Note
//! Attributes submenu (its items share names such as "Default" and "Up", so they're run here by
//! their submenu), a note head's Control-click menu (Attributes ▸ …, Fix displayed Note
//! Positions, Articulation ▸), the Note Attributes window (double-click a note head, as
//! Settings ▸ Score ▸ Double-Click to Open says; one note at a time), and the Resize and Layout
//! tools' note drags (Size, Horizontal Position: how the note looks, never how it plays).

use crate::app::App;
use crate::score;
use lg_core::score::{NoteAttr, NOTE_HEADS};

pub const COMMANDS: &[&str] = &["Set Resize Tool", "Resize Tool", "Align Object Positions Vertically", "Align Object Positions Horizontally", "Align Object Positions", "Show Margins", "Show Pages in Pairs", "Set Pointer Tool", "Set Pencil Tool", "Set Eraser Tool", "Set Text Tool", "Set Layout Tool", "Set Zoom Tool", "Set Voice Separation Tool", "Set Solo Tool"];

/// The Resize tool's index in `score::TOOLS`.
pub const RESIZE: usize = 9;

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    if let Some(tool) = name.strip_prefix("Set ").and_then(|n| n.strip_suffix(" Tool")).filter(|n| *n != "Resize") {
        // the tools' key commands pick the Score's tool while it has key focus
        if crate::focus::keymap_focus(app, ctx) != crate::keymap::Focus::Score {
            return false;
        }
        let Some(k) = score::TOOLS.iter().position(|t| *t == tool) else { return false };
        score::with_state(ctx, |s| s.tool = k);
        return true;
    }
    if matches!(name, "Show Margins" | "Show Pages in Pairs") {
        // (Page Display Options: Page view only)
        if score::state(ctx).view != score::View::Page {
            return false;
        }
        score::with_state(ctx, |s| if name == "Show Margins" { s.margins_shown = !s.margins_shown } else { s.pairs = !s.pairs });
        return true;
    }
    if name.starts_with("Align Object Positions") {
        // one object selected: Logic asks whether all similar objects take its position
        let Some(sel) = score::state(ctx).sel_symbol else { return false };
        let mode = match name {
            "Align Object Positions Vertically" => 1,
            "Align Object Positions Horizontally" => 2,
            _ => 3,
        };
        let _ = app;
        ctx.data_mut(|d| d.insert_temp(align_id(), Some((sel, mode))));
        return true;
    }
    score::with_state(ctx, |s| s.tool = RESIZE);
    true
}

fn align_id() -> egui::Id {
    egui::Id::new("lg-score-align-ask")
}

/// Align Object Positions: every symbol of the selected one's kind on its track takes its
/// Vertical (1), Horizontal (2) or both (3) positions. One undo step; the number changed.
pub fn align_similar(app: &mut App, sel: (u64, usize), mode: u8) -> usize {
    let Some(src) = app.project.as_ref().and_then(|p| p.score.track(sel.0)?.symbols.get(sel.1).cloned()) else { return 0 };
    app.checkpoint("Align Object Positions");
    let mut n = 0;
    if let Some(p) = app.project.as_mut() {
        for x in p.score.track_mut(sel.0).symbols.iter_mut().filter(|x| x.kind == src.kind) {
            if mode & 1 != 0 {
                x.dy = src.dy;
            }
            if mode & 2 != 0 {
                x.dx = src.dx;
            }
            n += 1;
        }
    }
    app.after_edit();
    n
}

/// The Align Object Positions question.
fn align_ask(app: &mut App, ctx: &egui::Context) {
    let Some((sel, mode)) = ctx.data(|d| d.get_temp::<Option<((u64, usize), u8)>>(align_id())).flatten() else { return };
    let mut answer = None;
    egui::Window::new("Align Object Positions").collapsible(false).resizable(false).title_bar(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -40.0)).show(ctx, |ui| {
        ui.label("Set all similar objects to the same value?");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
            if ui.button("OK").clicked() {
                answer = Some(true);
            }
        });
    });
    if let Some(ok) = answer {
        if ok {
            align_similar(app, sel, mode);
        }
        ctx.data_mut(|d| d.insert_temp::<Option<((u64, usize), u8)>>(align_id(), None));
    }
}

/// The Score command (scorecmd.rs) an item of the Note Attributes submenu runs, by the submenu
/// it's in; `None` when it is set here or can't be done.
pub fn mapped(sub: &str, item: &str) -> Option<&'static str> {
    Some(match (sub, item) {
        ("Stems", "Default") => "Stems: default",
        ("Stems", "Up") => "Stems: up",
        ("Stems", "Down") => "Stems: down",
        ("Stems", "Hide") => "Stems: hide",
        ("Beaming", "Default") => "Default Beams",
        ("Automatic Slurs", "Convert to Manual Slur") => "Convert Auto Slur",
        ("Ties", "Default") => "Ties: default",
        ("Ties", "Above") => "Ties: up",
        ("Ties", "Below") => "Ties: down",
        ("Syncopation", "Default") => "Default Syncopation",
        ("Syncopation", "Force") => "Force Syncopation",
        ("Syncopation", "Defeat") => "Defeat Syncopation",
        ("Interpretation", "Default") => "Default Interpretation",
        ("Interpretation", "Force") => "Force Interpretation",
        ("Interpretation", "Defeat") => "Defeat Interpretation",
        _ => {
            // the rest have unique names Studio's Score commands take as they are
            return crate::scorecmd::COMMANDS.iter().copied().find(|c| *c == item);
        }
    })
}

/// A note colour item: 0 default, 1–16 Color 0–15, 17 pitch, 18 velocity.
pub fn color_value(item: &str) -> Option<u8> {
    match item {
        "Default Color" => Some(0),
        "Pitch Color" => Some(17),
        "Velocity Color" => Some(18),
        _ => item.strip_prefix("Color ").and_then(|n| n.parse::<u8>().ok()).filter(|n| *n < 16).map(|n| n + 1),
    }
}

/// Runs an item of the Note Attributes submenu `sub` on the selected notes; false when it
/// can't be done.
pub fn run(app: &mut App, ctx: &egui::Context, sub: &str, item: &str) -> bool {
    if sub == "Colors" {
        let Some(c) = color_value(item) else { return false };
        return score::edit_attrs(app, "Note Color", |a| a.color = c);
    }
    if sub == "Ties" && item == "Hide" {
        return score::edit_attrs(app, "Hide Ties", |a| a.tie = 2);
    }
    let Some(cmd) = mapped(sub, item) else { return false };
    app.command(ctx, cmd);
    true
}

fn can(sub: &str, item: &str) -> bool {
    if sub == "Colors" {
        return color_value(item).is_some();
    }
    (sub == "Ties" && item == "Hide") || mapped(sub, item).is_some()
}

/// The checkmark of an item for the first selected note.
fn checked(a: &NoteAttr, sub: &str, item: &str) -> bool {
    match (sub, item) {
        ("Colors", i) => color_value(i) == Some(a.color),
        ("Stems", "Default") => a.stem == 0,
        ("Stems", "Up") => a.stem == 1,
        ("Stems", "Down") => a.stem == -1,
        ("Stems", "Hide") => a.stem == 2,
        ("Ties", "Default") => a.tie == 0,
        ("Ties", "Above") => a.tie == 1,
        ("Ties", "Below") => a.tie == -1,
        ("Ties", "Hide") => a.tie == 2,
        ("Syncopation", "Default") => a.synco == 0,
        ("Syncopation", "Force") => a.synco == 1,
        ("Syncopation", "Defeat") => a.synco == -1,
        ("Interpretation", "Default") => a.interp == 0,
        ("Interpretation", "Force") => a.interp == 1,
        ("Interpretation", "Defeat") => a.interp == -1,
        ("Independent", "Not Independent") => a.independent == 0,
        ("Independent", "Independent") => a.independent == 1,
        ("Independent", "Independent Grace") => a.independent == 2,
        ("Voice/Staff Assignment", "Default Staff") => a.staff == 0,
        ("Voice/Staff Assignment", "Staff Above Voice") => a.staff == 1,
        ("Voice/Staff Assignment", "Staff Below Voice") => a.staff == -1,
        _ => false,
    }
}

/// The selected notes' first attributes (for checkmarks).
fn first_attr(app: &App) -> NoteAttr {
    let Some((id, keys)) = score::selected_keys(app) else { return NoteAttr::default() };
    keys.first().and_then(|&(pos, pitch)| app.project.as_ref().map(|p| p.score.attr(id, pos, pitch))).unwrap_or_default()
}

/// The Note Attributes items (local Functions menu and the note's Attributes submenu).
pub fn items(app: &mut App, ui: &mut egui::Ui, items: &[crate::menus::Item], sub: &str) {
    let a = first_attr(app);
    for it in items {
        if it.separator {
            ui.separator();
            continue;
        }
        if !it.children.is_empty() {
            let label = it.label.clone();
            ui.menu_button(label.clone(), |ui| items_sub(app, ui, &it.children, &label, &a));
            continue;
        }
        // (Reset All Attributes, at this level)
        let short = crate::keycmds::shortcut_for(app, &it.label).unwrap_or_else(|| it.shortcut.clone());
        if ui.add_enabled(can(sub, &it.label), egui::Button::new(&it.label).shortcut_text(short)).clicked() {
            run(app, ui.ctx(), sub, &it.label);
            ui.close();
        }
    }
}

fn items_sub(app: &mut App, ui: &mut egui::Ui, items: &[crate::menus::Item], sub: &str, a: &NoteAttr) {
    for it in items {
        if it.separator {
            ui.separator();
            continue;
        }
        let ok = can(sub, &it.label);
        let label = if checked(a, sub, &it.label) { format!("✓ {}", it.label) } else { it.label.clone() };
        let short = crate::keycmds::shortcut_for(app, &it.label).unwrap_or_else(|| it.shortcut.clone());
        if ui.add_enabled(ok, egui::Button::new(label).shortcut_text(short)).clicked() {
            run(app, ui.ctx(), sub, &it.label);
            ui.close();
        }
    }
}

/// View ▸ Guides and Duration Bars (their items share "Off"): the mode, checked.
fn view_modes(ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    let guides = it.label == "Guides";
    if !(guides || it.label == "Duration Bars") || it.children.is_empty() {
        return false;
    }
    let ctx = ui.ctx().clone();
    let st = score::state(&ctx);
    let cur = if guides { st.guide_mode } else { st.duration_bars };
    ui.menu_button(it.label.clone(), |ui| {
        for (k, c) in it.children.iter().filter(|c| !c.separator).enumerate() {
            let label = if k as u8 == cur { format!("✓ {}", c.label) } else { c.label.clone() };
            if ui.button(label).clicked() {
                score::with_state(&ctx, |s| {
                    if guides {
                        s.guide_mode = k as u8;
                        s.guides = k != 0;
                        if k != 3 {
                            s.guide_last = k as u8;
                        }
                    } else {
                        s.duration_bars = k as u8;
                    }
                });
                ui.close();
            }
        }
    });
    true
}

/// menus.rs hook: the local Functions menu's Note Attributes submenu, run by submenu; View ▸
/// Guides and Duration Bars.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if view_modes(ui, it) {
        return true;
    }
    if it.label != "Note Attributes" || it.children.is_empty() {
        return false;
    }
    ui.menu_button("Note Attributes", |ui| items(app, ui, &it.children, ""));
    true
}

/// A note head's Control-click menu.
pub fn note_menu(app: &mut App, ui: &mut egui::Ui, t: usize) {
    ui.set_min_width(230.0);
    let attrs = crate::menus::local("score-Functions").iter().find(|i| i.label == "Note Attributes").map(|i| i.children.clone()).unwrap_or_default();
    ui.menu_button("Attributes", |ui| items(app, ui, &attrs, ""));
    if crate::artic::has_set(app, t) {
        ui.menu_button("Articulation", |ui| crate::artic::note_menu(app, ui, t));
    }
    ui.separator();
    for c in ["Fix displayed Note Positions", "Fix displayed Note Positions and Durations"] {
        if ui.button(c).clicked() {
            app.command(ui.ctx(), c);
            ui.close();
        }
    }
    ui.separator();
    for c in ["Cut", "Copy", "Delete"] {
        if ui.button(c).clicked() {
            app.command(ui.ctx(), c);
            ui.close();
        }
    }
}

// ---------------------------------------------------------------- the Note Attributes window

#[derive(Clone, Copy)]
struct Win {
    t: usize,
    rix: usize,
    note: usize,
}

fn win_id() -> egui::Id {
    egui::Id::new("lg-score-note-attrs")
}

/// Opens the Note Attributes window for one note.
pub fn open(ctx: &egui::Context, t: usize, rix: usize, note: usize) {
    ctx.data_mut(|d| d.insert_temp(win_id(), Some(Win { t, rix, note })));
}

/// Double-clicking a note head: what Settings ▸ Score ▸ Double-Click to Open says.
pub fn double_click(app: &mut App, ctx: &egui::Context, t: usize, rix: usize, note: usize) {
    match app.logic_settings.get("Score /  / Double-Click to Open:").as_str() {
        "Event List" => app.command(ctx, "Open Event List…"),
        "Piano Roll Editor" => score::set_editor_tab(ctx, score::EditorTab::PianoRoll),
        "Step Editor" => app.command(ctx, "Open Step Editor"),
        _ => open(ctx, t, rix, note),
    }
}

/// The pop-ups: (label, choices, getter, setter).
type Field = (&'static str, &'static [&'static str], fn(&NoteAttr) -> usize, fn(&mut NoteAttr, usize));

pub const ACC_POS: [&str; 9] = ["−8", "−6", "−4", "−2", "0", "+2", "+4", "+6", "+8"];
pub const SIZES: [&str; 7] = ["−3", "−2", "−1", "0", "+1", "+2", "+3"];
pub const HPOS: [&str; 11] = ["−20", "−16", "−12", "−8", "−4", "0", "+4", "+8", "+12", "+16", "+20"];

fn fields() -> Vec<Field> {
    vec![
        ("Enharmonic Shift", &["Default", "#", "b"], |a| match a.spell { 1 => 1, -1 => 2, _ => 0 }, |a, k| a.spell = [0, 1, -1][k]),
        ("Accidental Type", &["Default", "Force", "Hide", "Guide", "Force (only flats & sharps)"], |a| a.accidental as usize, |a, k| a.accidental = k as u8),
        ("Accidental Position", &ACC_POS, |a| ((a.acc_pos as i32 / 2) + 4).clamp(0, 8) as usize, |a, k| a.acc_pos = (k as i8 - 4) * 2),
        ("Note Head", &NOTE_HEADS, |a| a.head as usize, |a, k| a.head = k as u8),
        ("Size", &SIZES, |a| (a.size + 3).clamp(0, 6) as usize, |a, k| a.size = k as i8 - 3),
        ("Horizontal Position", &HPOS, |a| ((a.hpos as i32 / 4) + 5).clamp(0, 10) as usize, |a, k| a.hpos = (k as i8 - 5) * 4),
        ("Syncopation", &["Default", "Force", "Defeat"], |a| match a.synco { 1 => 1, -1 => 2, _ => 0 }, |a, k| a.synco = [0, 1, -1][k]),
        ("Interpretation", &["Default", "Force", "Defeat"], |a| match a.interp { 1 => 1, -1 => 2, _ => 0 }, |a, k| a.interp = [0, 1, -1][k]),
        ("Stem Direction", &["Default", "Up", "Down", "Hide"], |a| match a.stem { 1 => 1, -1 => 2, 2 => 3, _ => 0 }, |a, k| a.stem = [0, 1, -1, 2][k]),
        ("Stem Position", &["Default", "Center", "Automatic", "Side"], |a| a.stem_pos as usize, |a, k| a.stem_pos = k as u8),
        ("Tie Direction", &["Default", "Up", "Down", "Hide"], |a| match a.tie { 1 => 1, -1 => 2, 2 => 3, _ => 0 }, |a, k| a.tie = [0, 1, -1, 2][k]),
    ]
}

/// Sets one note's attributes (one undo step).
pub fn edit_note(app: &mut App, t: usize, rix: usize, note: usize, what: &str, f: impl FnOnce(&mut NoteAttr)) -> bool {
    let Some(p) = app.project.as_ref() else { return false };
    let Some(tr) = p.tracks.get(t) else { return false };
    let Some(n) = tr.regions.get(rix).and_then(|r| r.notes.get(note)).copied() else { return false };
    let id = tr.extra.id;
    app.checkpoint(what);
    if let Some(p) = app.project.as_mut() {
        p.score.edit(id, n.pos, n.pitch, f);
    }
    app.after_edit();
    true
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    align_ask(app, ctx);
    let Some(w) = ctx.data(|d| d.get_temp::<Option<Win>>(win_id())).flatten() else { return };
    let Some(p) = app.project.as_ref() else { return };
    let Some(tr) = p.tracks.get(w.t) else { return };
    let Some(n) = tr.regions.get(w.rix).and_then(|r| r.notes.get(w.note)).copied() else {
        ctx.data_mut(|d| d.insert_temp::<Option<Win>>(win_id(), None));
        return;
    };
    let a = p.score.attr(tr.extra.id, n.pos, n.pitch);
    let mut open = true;
    let mut change: Option<(usize, usize)> = None;
    let fields = fields();
    let mut done = false;
    egui::Window::new("Note Attributes").open(&mut open).resizable(false).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
        ui.label(egui::RichText::new(format!("{}  ·  {}", crate::app::note_name(n.pitch), crate::app::bbdt(n.pos, p.signature))).strong());
        ui.add_space(4.0);
        egui::Grid::new("lg-note-attr-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            for (k, (label, choices, get, _)) in fields.iter().enumerate() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(format!("{label}:")));
                let cur = get(&a).min(choices.len() - 1);
                egui::ComboBox::from_id_salt(("lg-note-attr", k)).width(150.0).selected_text(choices[cur]).show_ui(ui, |ui| {
                    for (c, name) in choices.iter().enumerate() {
                        if ui.selectable_label(c == cur, *name).clicked() {
                            change = Some((k, c));
                        }
                    }
                });
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Reset All Attributes").clicked() {
                change = Some((usize::MAX, 0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("OK").min_size(egui::vec2(64.0, 0.0))).clicked() {
                    done = true;
                }
            });
        });
    });
    if let Some((k, c)) = change {
        if k == usize::MAX {
            edit_note(app, w.t, w.rix, w.note, "Reset Note Attributes", |a| *a = NoteAttr::default());
        } else {
            let set = fields[k].3;
            edit_note(app, w.t, w.rix, w.note, fields[k].0, |a| set(a, c));
        }
    }
    if !open || done {
        ctx.data_mut(|d| d.insert_temp::<Option<Win>>(win_id(), None));
    }
}

/// Scale Quantize: the selected notes (all with none selected) off the scale move to the
/// nearest scale note (down first, as the Piano Roll's).
pub fn scale_quantize(notes: &mut [lg_core::events::Note], sel: &[usize], scale: &[bool; 12]) {
    for (i, n) in notes.iter_mut().enumerate() {
        if (!sel.is_empty() && !sel.contains(&i)) || scale[(n.pitch % 12) as usize] {
            continue;
        }
        n.pitch = if n.pitch > 0 && scale[((n.pitch - 1) % 12) as usize] { n.pitch - 1 } else { (n.pitch + 1).min(127) };
    }
}

// ---------------------------------------------------------------- editing a text in place

fn edit_id() -> egui::Id {
    egui::Id::new("lg-score-text-edit")
}

/// The text symbol being edited (track id, index), while Text Entry runs.
pub fn set_editing(ctx: &egui::Context, target: Option<(u64, usize)>) {
    ctx.data_mut(|d| d.insert_temp(edit_id(), target));
}

pub fn take_editing(ctx: &egui::Context) -> Option<(u64, usize)> {
    ctx.data_mut(|d| d.remove_temp::<Option<(u64, usize)>>(edit_id())).flatten()
}

/// A text, lyric or chord symbol's new text (one undo step).
pub fn edit_symbol_text(app: &mut App, track_id: u64, si: usize, text: &str) {
    app.checkpoint("Edit Text");
    if let Some(x) = app.project.as_mut().and_then(|p| p.score.track_mut(track_id).symbols.get_mut(si)) {
        x.text = text.to_string();
    }
    app.after_edit();
}

// ---------------------------------------------------------------- the Voice Separation tool

/// Adds a point to a Voice Separation line: moving back left erases the line right of it.
pub fn vs_extend(path: &mut Vec<egui::Pos2>, p: egui::Pos2) {
    path.retain(|q| q.x <= p.x);
    path.push(p);
}

/// Whether a point is below the line (None: outside its span).
pub fn vs_below(path: &[egui::Pos2], c: egui::Pos2) -> Option<bool> {
    let w = path.windows(2).find(|w| c.x >= w[0].x && c.x <= w[1].x)?;
    let f = if w[1].x > w[0].x { (c.x - w[0].x) / (w[1].x - w[0].x) } else { 0.0 };
    Some(c.y > w[0].y + (w[1].y - w[0].y) * f)
}

/// The channel the Voice Separation line gives a note below it: the next lower voice's (its
/// MIDI channel), else the note's channel + 1.
pub fn vs_channel(def: &lg_core::staffstyle::StaffStyleDef, pitch: u8, channel: u8) -> u8 {
    let vi = def.voice_for(pitch, channel);
    match def.voices.get(vi + 1) {
        Some(v) if v.channel > 0 => v.channel - 1,
        _ => (channel + 1).min(15),
    }
}

/// Applies a finished line to the heads drawn (track, region, note, rect): the notes below it
/// in the region it crosses first go to the next lower voice. One undo step; how many moved.
pub fn vs_apply(app: &mut App, path: &[egui::Pos2], heads: &[(usize, usize, usize, egui::Rect)]) -> usize {
    let Some(&(t, r, _, _)) = heads.iter().find(|h| vs_below(path, h.3.center()).is_some()) else { return 0 };
    let below: Vec<usize> = heads.iter().filter(|h| (h.0, h.1) == (t, r) && vs_below(path, h.3.center()) == Some(true)).map(|h| h.2).collect();
    if below.is_empty() {
        return 0;
    }
    let def = score::def_of(app, t);
    app.checkpoint("Voice Separation");
    let mut n = 0;
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        let mut done: Vec<usize> = Vec::new();
        for i in below {
            if done.contains(&i) {
                continue;
            }
            done.push(i);
            if let Some(nn) = rg.notes.get_mut(i) {
                nn.channel = vs_channel(&def, nn.pitch, nn.channel);
                n += 1;
            }
        }
    }
    app.after_edit();
    n
}

fn vs_id() -> egui::Id {
    egui::Id::new("lg-score-voice-sep")
}

/// The line being drawn (Voice Separation tool), kept between frames.
pub fn vs_path(ctx: &egui::Context) -> Vec<egui::Pos2> {
    ctx.data(|d| d.get_temp::<Vec<egui::Pos2>>(vs_id())).unwrap_or_default()
}

pub fn vs_store(ctx: &egui::Context, path: Vec<egui::Pos2>) {
    ctx.data_mut(|d| d.insert_temp(vs_id(), path));
}

// ---------------------------------------------------------------- Insert Defaults

/// The region parameters new regions take (Logic: the Region inspector with no region
/// selected); kept until Studio quits. Quantize "" = Default (from the LCD division).
#[derive(Clone, Debug, PartialEq)]
pub struct Defaults {
    pub quantize: String,
    pub interp: bool,
    pub synco: bool,
    pub no_overlap: bool,
    pub max_dots: u8,
}

static DEFAULTS: std::sync::Mutex<Option<Defaults>> = std::sync::Mutex::new(None);

pub fn defaults() -> Defaults {
    DEFAULTS.lock().ok().and_then(|d| d.clone()).unwrap_or(Defaults { quantize: String::new(), interp: true, synco: false, no_overlap: true, max_dots: 1 })
}

pub fn set_defaults(d: Defaults) {
    if let Ok(mut x) = DEFAULTS.lock() {
        *x = Some(d);
    }
}

/// Quantize Default: the LCD division's duple value with the next triple value (1/8 → 8,12,
/// 1/12 → 4,12, 1/16 → 16,24, 1/24 → 8,24; finer → 16,24).
pub fn quant_for_division(div: u32) -> &'static str {
    match div {
        960.. => "4",
        480..=959 if div != 640 => "8,12",
        640 | 320 => "4,12",
        240 => "16,24",
        160 => "8,24",
        _ => "16,24",
    }
}

/// The display Quantize, Interpretation and Syncopation a region without its own parameters reads.
pub fn defaults_quant(app: &App) -> (usize, bool, bool) {
    let d = defaults();
    let name = if d.quantize.is_empty() { quant_for_division(app.nav.division) } else { d.quantize.as_str() };
    let q = lg_notation::Quant::CHOICES.iter().position(|c| c.0 == name).unwrap_or(9);
    (q, d.interp, d.synco)
}

// ---------------------------------------------------------------- symbol drags

/// A dragged score symbol: the Pointer moves it in time (on the display grid) and up / down;
/// the Layout tool only graphically (Horizontal / Vertical Pos., its bar position kept);
/// a line's right end changes its length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymMove {
    pub layout: bool,
    pub resize: bool,
    /// New tick and length, and graphic offsets (tenths of a staff space).
    pub tick: u32,
    pub len: u32,
    pub dx: i16,
    pub dy: i16,
}

impl SymMove {
    pub fn tag(&self, sig: (u8, u8)) -> Vec<(String, String)> {
        if self.layout {
            vec![("Horizontal Position:".into(), self.dx.to_string()), ("Vertical Position:".into(), self.dy.to_string())]
        } else if self.resize {
            vec![("Length:".into(), crate::app::bbdt_len(self.len, sig))]
        } else {
            vec![("Position:".into(), crate::app::bbdt(self.tick, sig))]
        }
    }
}

pub fn symbol_drag(s: &lg_core::score::Symbol, layout: bool, at_end: bool, from: egui::Pos2, now: egui::Pos2, sp: f32, grid: f32, tick_of: &dyn Fn(f32) -> f32) -> SymMove {
    let d = now - from;
    let mut m = SymMove { layout, resize: false, tick: s.tick, len: s.len, dx: s.dx, dy: s.dy };
    if layout {
        m.dx = s.dx.saturating_add((d.x * 10.0 / sp).round() as i16);
        m.dy = s.dy.saturating_add((-d.y * 10.0 / sp).round() as i16);
        return m;
    }
    let grid = grid.max(1.0);
    let dt = ((tick_of(now.x) - tick_of(from.x)) / grid).round() * grid;
    if at_end {
        m.resize = true;
        m.len = (s.len as f32 + dt).max(grid) as u32;
    } else {
        m.tick = (s.tick as f32 + dt).max(0.0) as u32;
        m.dy = s.dy.saturating_add((-d.y * 10.0 / sp).round() as i16);
    }
    m
}

/// Applies a symbol drag (Option: a copy, the original stays). One undo step.
pub fn apply_symbol_drag(app: &mut App, track_id: u64, si: usize, m: &SymMove, copy: bool) {
    app.checkpoint(match (copy, m.layout, m.resize) {
        (true, ..) => "Copy Symbol",
        (_, true, _) => "Move Symbol Graphically",
        (_, _, true) => "Change Symbol Length",
        _ => "Move Symbol",
    });
    if let Some(p) = app.project.as_mut() {
        let ts = p.score.track_mut(track_id);
        if let Some(mut x) = ts.symbols.get(si).cloned() {
            x.tick = m.tick;
            x.len = m.len;
            x.dx = m.dx;
            x.dy = m.dy;
            if copy {
                ts.symbols.push(x);
            } else {
                ts.symbols[si] = x;
            }
        }
    }
    app.after_edit();
}

// ---------------------------------------------------------------- velocity drags

/// A Control-Command drag on a selected note: the selected notes' velocities, up / down.
#[derive(Clone)]
struct VelDrag {
    t: usize,
    rix: usize,
    notes: Vec<usize>,
    orig: Vec<u8>,
    from: egui::Pos2,
    started: bool,
}

fn vel_id() -> egui::Id {
    egui::Id::new("lg-score-vel-drag")
}

pub fn start_velocity(app: &App, ctx: &egui::Context, t: usize, rix: usize, from: egui::Pos2) {
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(rix)) else { return };
    let notes = app.selected_notes.clone();
    let orig = notes.iter().map(|&i| rg.notes.get(i).map_or(64, |n| n.velocity)).collect();
    ctx.data_mut(|d| d.insert_temp(vel_id(), Some(VelDrag { t, rix, notes, orig, from, started: false })));
}

/// A velocity after dragging `dy` points (up raises it, a point a step).
pub fn dragged_velocity(orig: u8, dy: f32) -> u8 {
    (orig as i32 - dy.round() as i32).clamp(1, 127) as u8
}

/// Follows a running velocity drag; true while one runs.
pub fn follow_velocity(app: &mut App, ctx: &egui::Context) -> bool {
    let Some(mut d) = ctx.data(|x| x.get_temp::<Option<VelDrag>>(vel_id())).flatten() else { return false };
    let (pos, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
    if let Some(pp) = pos {
        let dy = pp.y - d.from.y;
        if !d.started && dy.abs() > 1.0 {
            app.checkpoint("Change Velocity");
            d.started = true;
        }
        if d.started {
            if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(d.t)?.regions.get_mut(d.rix)) {
                for (k, &i) in d.notes.iter().enumerate() {
                    if let Some(n) = rg.notes.get_mut(i) {
                        n.velocity = dragged_velocity(d.orig[k], dy);
                    }
                }
            }
            let v = dragged_velocity(d.orig.first().copied().unwrap_or(64), dy);
            app.help_tag = Some(vec![("Velocity:".into(), v.to_string())]);
        }
    }
    if !down {
        if d.started {
            app.after_edit();
        }
        ctx.data_mut(|x| x.insert_temp::<Option<VelDrag>>(vel_id(), None));
        return false;
    }
    ctx.data_mut(|x| x.insert_temp(vel_id(), Some(d)));
    true
}

// ---------------------------------------------------------------- Region ▸ Score Show / Hide

/// The Region inspector's Score parameter (one undo step); every selected region of the
/// track takes it.
pub fn set_region_hidden(app: &mut App, t: usize, rix: usize, hide: bool) {
    let Some(p) = app.project.as_ref() else { return };
    let Some(tr) = p.tracks.get(t) else { return };
    let id = tr.extra.id;
    let mut starts: Vec<u32> = tr.regions.get(rix).map(|r| r.start).into_iter().collect();
    starts.extend(app.selected_regions.iter().filter(|x| x.0 == t).filter_map(|x| tr.regions.get(x.1)).map(|r| r.start));
    app.checkpoint(if hide { "Hide Region in Score" } else { "Show Region in Score" });
    if let Some(p) = app.project.as_mut() {
        let ts = p.score.track_mut(id);
        for s in starts {
            if hide {
                ts.hidden.insert(s);
            } else {
                ts.hidden.remove(&s);
            }
        }
    }
    app.after_edit();
}

/// Whether a region is left out of the Score: hidden, unless it alone is shown (its own
/// display level).
pub fn region_left_out(hidden: bool, alone_and_selected: bool) -> bool {
    hidden && !alone_and_selected
}

// ---------------------------------------------------------------- Resize / Layout drags

/// A note drag with the Resize tool (vertical: Size) or the Layout tool (horizontal:
/// Horizontal Position): the notes' keys and their values before.
#[derive(Clone)]
pub struct AttrDrag {
    pub resize: bool,
    pub id: u64,
    pub keys: Vec<(u32, u8)>,
    pub orig: Vec<i8>,
    pub from: egui::Pos2,
    pub started: bool,
}

fn drag_id() -> egui::Id {
    egui::Id::new("lg-score-attr-drag")
}

pub fn start_drag(app: &App, ctx: &egui::Context, resize: bool, from: egui::Pos2) {
    let Some((id, keys)) = score::selected_keys(app) else { return };
    let Some(p) = app.project.as_ref() else { return };
    let orig = keys.iter().map(|&(pos, pitch)| { let a = p.score.attr(id, pos, pitch); if resize { a.size } else { a.hpos } }).collect();
    ctx.data_mut(|d| d.insert_temp(drag_id(), Some(AttrDrag { resize, id, keys, orig, from, started: false })));
}

/// The value a drag of `d` points gives (Size: 12 pt per step up; Horizontal Position: tenths
/// of the staff space `sp`).
pub fn dragged(orig: i8, resize: bool, d: egui::Vec2, sp: f32) -> i8 {
    if resize {
        (orig as i32 + (-d.y / 12.0).round() as i32).clamp(-3, 3) as i8
    } else {
        (orig as i32 + (d.x * 10.0 / sp).round() as i32).clamp(-60, 60) as i8
    }
}

/// Follows a running Resize / Layout drag; true while one runs.
pub fn follow_drag(app: &mut App, ctx: &egui::Context, sp: f32) -> bool {
    let Some(mut d) = ctx.data(|x| x.get_temp::<Option<AttrDrag>>(drag_id())).flatten() else { return false };
    let (pos, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
    if let Some(pp) = pos {
        let delta = pp - d.from;
        if !d.started && delta.length() > 2.0 {
            app.checkpoint(if d.resize { "Resize Notes" } else { "Move Notes Graphically" });
            d.started = true;
        }
        if d.started {
            if let Some(p) = app.project.as_mut() {
                for (k, &(pos, pitch)) in d.keys.iter().enumerate() {
                    let v = dragged(d.orig[k], d.resize, delta, sp);
                    p.score.edit(d.id, pos, pitch, |a| if d.resize { a.size = v } else { a.hpos = v });
                }
            }
            let first = dragged(d.orig.first().copied().unwrap_or(0), d.resize, delta, sp);
            app.help_tag = Some(vec![(if d.resize { "Size:".into() } else { "Horizontal Position:".into() }, format!("{first:+}"))]);
        }
    }
    if !down {
        if d.started {
            app.after_edit();
        }
        ctx.data_mut(|x| x.insert_temp::<Option<AttrDrag>>(drag_id(), None));
        return false;
    }
    ctx.data_mut(|x| x.insert_temp(drag_id(), Some(d)));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submenu_items_map_to_commands() {
        assert_eq!(mapped("Stems", "Up"), Some("Stems: up"));
        assert_eq!(mapped("Ties", "Below"), Some("Ties: down"));
        assert_eq!(mapped("Syncopation", "Force"), Some("Force Syncopation"));
        assert_eq!(mapped("Interpretation", "Defeat"), Some("Defeat Interpretation"));
        assert_eq!(mapped("Accidentals", "Hide Accidental"), Some("Hide Accidental"));
        assert_eq!(mapped("Independent", "Independent Grace"), Some("Independent Grace"));
        assert_eq!(mapped("", "Reset All Attributes"), Some("Reset All Attributes"));
        assert_eq!(mapped("Ties", "Dotted Line"), None);
        // every item of Logic's submenu but the two tie shapes does something
        let na = crate::menus::local("score-Functions").iter().find(|i| i.label == "Note Attributes").expect("Note Attributes");
        for sub in &na.children {
            for it in sub.children.iter().filter(|i| !i.separator) {
                let ok = can(&sub.label, &it.label);
                assert!(ok || matches!(it.label.as_str(), "Regular Shape" | "Dotted Line"), "{} ▸ {}", sub.label, it.label);
            }
        }
    }

    #[test]
    fn region_hide_and_note_window_edits() {
        let mut app = crate::tm::test_app();
        let (t, r) = {
            let p = app.project.as_ref().unwrap();
            let t = p.tracks.iter().position(|t| t.regions.iter().any(|r| r.audio.is_none() && !r.notes.is_empty())).unwrap();
            (t, p.tracks[t].regions.iter().position(|r| r.audio.is_none() && !r.notes.is_empty()).unwrap())
        };
        set_region_hidden(&mut app, t, r, true);
        let (id, start) = { let p = app.project.as_ref().unwrap(); (p.tracks[t].extra.id, p.tracks[t].regions[r].start) };
        assert!(app.project.as_ref().unwrap().score.track(id).unwrap().hidden.contains(&start));
        assert!(region_left_out(true, false) && !region_left_out(true, true) && !region_left_out(false, false));
        app.undo();
        assert!(app.project.as_ref().unwrap().score.track(id).is_none_or(|x| x.hidden.is_empty()));
        // the Note Attributes window's edits: one note, one undo step each
        assert!(edit_note(&mut app, t, r, 0, "Size", |a| a.size = 2));
        let n = app.project.as_ref().unwrap().tracks[t].regions[r].notes[0];
        assert_eq!(app.project.as_ref().unwrap().score.attr(id, n.pos, n.pitch).size, 2);
    }

    #[test]
    fn symbol_drags() {
        let s = lg_core::score::Symbol { tick: 960, len: 1920, dx: 1, dy: 2, ..Default::default() };
        let tick_of = |x: f32| x * 10.0;
        // the Pointer: time on the grid, up / down as Vertical Pos.
        let m = symbol_drag(&s, false, false, egui::pos2(0.0, 0.0), egui::pos2(50.0, -8.0), 8.0, 240.0, &tick_of);
        assert_eq!((m.tick, m.len, m.dx, m.dy), (1440, 1920, 1, 12));
        // the right end: the length
        let m = symbol_drag(&s, false, true, egui::pos2(0.0, 0.0), egui::pos2(-200.0, 0.0), 8.0, 240.0, &tick_of);
        assert_eq!((m.tick, m.len), (960, 240));
        // the Layout tool: graphically only, its bar position kept
        let m = symbol_drag(&s, true, false, egui::pos2(0.0, 0.0), egui::pos2(16.0, 8.0), 8.0, 240.0, &tick_of);
        assert_eq!((m.tick, m.dx, m.dy), (960, 21, -8));
        let mut app = crate::tm::test_app();
        let id = app.project.as_ref().unwrap().tracks[0].extra.id;
        app.project.as_mut().unwrap().score.track_mut(id).symbols.push(s.clone());
        apply_symbol_drag(&mut app, id, 0, &m, true);
        edit_symbol_text(&mut app, id, 1, "cresc.");
        assert_eq!(app.project.as_ref().unwrap().score.track(id).unwrap().symbols[1].text, "cresc.");
        assert_eq!(app.project.as_ref().unwrap().score.track(id).unwrap().symbols.len(), 2, "Option copies");
        // Align Object Positions Vertically: the similar objects take the first's height
        app.project.as_mut().unwrap().score.track_mut(id).symbols[0].dy = 30;
        assert_eq!(align_similar(&mut app, (id, 0), 1), 2);
        assert!(app.project.as_ref().unwrap().score.track(id).unwrap().symbols.iter().all(|x| x.dy == 30));
    }

    #[test]
    fn insert_defaults_follow_the_division() {
        assert_eq!(quant_for_division(480), "8,12");
        assert_eq!(quant_for_division(320), "4,12");
        assert_eq!(quant_for_division(240), "16,24");
        assert_eq!(quant_for_division(160), "8,24");
        for q in ["8,12", "4,12", "16,24", "8,24"] {
            assert!(lg_notation::Quant::CHOICES.iter().any(|c| c.0 == q), "{q}");
        }
        let mut app = crate::tm::test_app();
        app.nav.division = 160;
        assert_eq!(lg_notation::Quant::CHOICES[defaults_quant(&app).0].0, "8,24");
        // a set value wins over the division
        set_defaults(Defaults { quantize: "8".into(), interp: false, synco: true, no_overlap: false, max_dots: 2 });
        let (q, i, s) = defaults_quant(&app);
        assert_eq!((lg_notation::Quant::CHOICES[q].0, i, s), ("8", false, true));
        set_defaults(Defaults { quantize: String::new(), interp: true, synco: false, no_overlap: true, max_dots: 1 });
    }

    #[test]
    fn voice_separation_line() {
        let mut path = Vec::new();
        for (x, y) in [(0.0, 10.0), (10.0, 10.0), (20.0, 20.0), (30.0, 20.0)] {
            vs_extend(&mut path, egui::pos2(x, y));
        }
        // moving back left erases the part to its right
        vs_extend(&mut path, egui::pos2(15.0, 12.0));
        assert_eq!(path.len(), 3);
        assert_eq!(vs_below(&path, egui::pos2(5.0, 12.0)), Some(true));
        assert_eq!(vs_below(&path, egui::pos2(5.0, 8.0)), Some(false));
        assert_eq!(vs_below(&path, egui::pos2(40.0, 8.0)), None);
        let def = lg_core::staffstyle::StaffStyleDef::dual("x");
        assert!(vs_channel(&def, 60, 0) <= 15);
        let mut app = crate::tm::test_app();
        let (t, r) = {
            let p = app.project.as_ref().unwrap();
            let t = p.tracks.iter().position(|t| t.regions.iter().any(|r| r.audio.is_none() && r.notes.len() > 1)).unwrap();
            (t, p.tracks[t].regions.iter().position(|r| r.audio.is_none() && r.notes.len() > 1).unwrap())
        };
        let ch = app.project.as_ref().unwrap().tracks[t].regions[r].notes[1].channel;
        let heads = [(t, r, 0, egui::Rect::from_center_size(egui::pos2(5.0, 5.0), egui::vec2(2.0, 2.0))), (t, r, 1, egui::Rect::from_center_size(egui::pos2(5.0, 15.0), egui::vec2(2.0, 2.0)))];
        assert_eq!(vs_apply(&mut app, &path, &heads), 1);
        assert_ne!(app.project.as_ref().unwrap().tracks[t].regions[r].notes[1].channel, ch);
    }

    #[test]
    fn colors_and_drags() {
        assert_eq!((dragged_velocity(80, -10.0), dragged_velocity(5, 50.0), dragged_velocity(120, -40.0)), (90, 1, 127));
        assert_eq!(color_value("Color 0"), Some(1));
        assert_eq!(color_value("Color 15"), Some(16));
        assert_eq!(color_value("Color 16"), None);
        assert_eq!(color_value("Velocity Color"), Some(18));
        assert_eq!(dragged(0, true, egui::vec2(0.0, -24.0), 8.0), 2);
        assert_eq!(dragged(2, true, egui::vec2(0.0, -200.0), 8.0), 3);
        assert_eq!(dragged(0, false, egui::vec2(8.0, 30.0), 8.0), 10);
        // the window's pop-ups set and read back each value
        let mut a = NoteAttr::default();
        for (_, choices, get, set) in fields() {
            for c in 0..choices.len() {
                set(&mut a, c);
                assert_eq!(get(&a), c);
            }
        }
    }
}
