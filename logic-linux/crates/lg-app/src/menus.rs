//! The menu bar: Logic Pro 13's menus as the research captured them (data/menus.txt, from
//! menus/menu-tree-raw.txt, context "one software-instrument track selected"), with their
//! shortcuts. Commands Studio has run; the others are greyed out until they're built (as
//! Keydeck's menus do).

use std::sync::OnceLock;

use crate::app::App;

#[derive(Clone, Debug)]
pub struct Item {
    pub label: String,
    pub shortcut: String,
    pub separator: bool,
    pub children: Vec<Item>,
}

/// Apple-only items left out (Apple services, Intelligence, subscriptions).
const LEFT_OUT: &[&str] = &["Learn About Apple Creator Studio…", "Learn About Logic Remote…", "Manage Subscriptions", "Intelligence Features", "Provide Bramble Studio Feedback", "Privacy & Analytics", "Services", "Send Bramble Studio Feedback", "Sound Library…"];

fn parse() -> Vec<Item> {
    parse_text(include_str!("../data/menus.txt"), 2)
}

/// Menus in the research's text form: "=== Name ===" then items indented four spaces a level
/// from `base` (the menu bar file starts its items at 2, the right-click and local menus at 0).
fn parse_text(text: &str, base: usize) -> Vec<Item> {
    let mut menus: Vec<Item> = Vec::new();
    // the path of open submenus: (indent, index path)
    let mut stack: Vec<usize> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("=== ").and_then(|l| l.strip_suffix(" ===")) {
            menus.push(Item { label: name.to_string(), shortcut: String::new(), separator: false, children: Vec::new() });
            stack.clear();
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let level = indent.saturating_sub(base) / 4;
        let mut body = line.trim().to_string();
        let mut shortcut = String::new();
        if let (Some(a), true) = (body.rfind("  ["), body.ends_with(']')) {
            shortcut = body[a + 3..body.len() - 1].to_string();
            body.truncate(a);
        }
        let body = body.replace("  (disabled)", "").replace(" ▸", "");
        let body = body.trim_start_matches("✓ ").trim_start_matches("• ").trim().to_string();
        if LEFT_OUT.contains(&body.as_str()) {
            continue;
        }
        let item = Item { separator: body == "---", label: body, shortcut, children: Vec::new() };
        let Some(menu) = menus.last_mut() else { continue };
        stack.truncate(level);
        let mut list = &mut menu.children;
        for &i in &stack {
            list = &mut list[i].children;
        }
        list.push(item);
        stack.push(list.len() - 1);
    }
    // no separators at the start, end or twice in a row
    fn tidy(items: &mut Vec<Item>) {
        items.dedup_by(|a, b| a.separator && b.separator);
        while items.first().is_some_and(|i| i.separator) {
            items.remove(0);
        }
        while items.last().is_some_and(|i| i.separator) {
            items.pop();
        }
        for i in items.iter_mut() {
            tidy(&mut i.children);
        }
    }
    for m in &mut menus {
        tidy(&mut m.children);
    }
    menus
}

pub fn menus() -> &'static [Item] {
    static M: OnceLock<Vec<Item>> = OnceLock::new();
    M.get_or_init(parse)
}

/// A right-click menu by place ("region-midi", "track-header", "pianoroll-note"…: the research's
/// context-menus/ file names).
pub fn context(place: &str) -> &'static [Item] {
    static M: OnceLock<Vec<Item>> = OnceLock::new();
    let all = M.get_or_init(|| parse_text(include_str!("../data/context-menus.txt"), 0));
    all.iter().find(|m| m.label == place).map_or(&[], |m| m.children.as_slice())
}

/// A local menu ("tracks-Edit", "pianoroll-Functions", "mixer-View"…).
pub fn local(name: &str) -> &'static [Item] {
    static M: OnceLock<Vec<Item>> = OnceLock::new();
    let all = M.get_or_init(|| parse_text(include_str!("../data/local-menus.txt"), 0));
    all.iter().find(|m| m.label == name).map_or(&[], |m| m.children.as_slice())
}

/// Shows `items` in an open menu; runs the one picked.
pub fn show_items(app: &mut App, ui: &mut egui::Ui, items: &[Item]) {
    let mut fired = None;
    ui.set_min_width(230.0);
    for c in items {
        item_ui(app, ui, c, &mut fired);
    }
    if let Some(cmd) = fired {
        app.command(ui.ctx(), &cmd);
    }
}

/// A right-click menu on `resp` from the research's menu for `place`.
pub fn context_menu(app: &mut App, resp: &egui::Response, place: &str) {
    let items = context(place);
    if items.is_empty() {
        return;
    }
    // Settings ▸ Editing ▸ Right Mouse Button
    let rmb = crate::rmb::mode(app, place);
    if rmb == crate::rmb::Rmb::ToolMenu {
        if resp.secondary_clicked() {
            app.tool_menu_open = true;
        }
        return;
    }
    resp.context_menu(|ui| {
        app.midi.menu_roll = place.starts_with("pianoroll");
        if rmb == crate::rmb::Rmb::ToolAndShortcut {
            crate::rmb::tools_section(app, ui, place.starts_with("pianoroll"));
            ui.separator();
        }
        show_items(app, ui, items);
        app.midi.menu_roll = false;
    });
}

/// The commands Studio runs (menu labels; "Menu > Item" where a label is ambiguous).
pub const IMPLEMENTED: &[&str] = &[
    "About Bramble Studio",
    "General…",
    "Audio…",
    "Recording…",
    "MIDI…",
    "Score…",
    "Movie…",
    "Automation…",
    "Control Surfaces…",
    "View…",
    "My Info…",
    "Quit Bramble Studio",
    "Hide Bramble Studio",
    "New",
    "New from Template…",
    "Open…",
    "Close",
    "Close Project",
    "Logic Projects…",
    "Hide Browser",
    "Hide Inspector",
    "Show Mixer",
    "Show Editor",
    "Hide Control Bar",
    "Enter Full Screen",
    "Minimize",
    "Zoom",
    "Open Main Window",
    "Open Mixer",
    "Open Piano Roll",
    "Move Locators Forward by Cycle Length",
    "Project or Section…",
    "Undo",
    "Redo",
    "Save",
    "New Session Player SI Track…",
    "New Software Instrument Track",
    "New Audio Track",
    "Save As…",
    "Save A Copy As…",
    "1 Track as Audio File…",
    "All MIDI Tracks as MIDI File…",
    // the Mixer's slot menus (mixer.rs): Audio FX plug-ins Studio has, sends, outputs, groups
    "Channel EQ", "Compressor", "Limiter", "Adaptive Limiter", "Noise Gate", "ChromaVerb", "Space Designer", "Stereo Delay", "Tape Delay", "Echo", "Chorus", "Ensemble", "Flanger", "Phaser", "Microphaser", "Tremolo", "AutoFilter", "Distortion", "Overdrive", "Bitcrusher", "Clip Distortion", "Exciter", "DeEsser 2", "Gain", "Enveloper", "Vintage Graphic EQ", "Vintage Tube EQ", "Vintage Console EQ", "ChromaGlow", "Phat FX", "Amp Designer", "Bass Amp Designer", "Multipressor", "Pedalboard", "Robo Flanger", "Total Tremolo", "Blue Echo", "Tru-Tape Delay", "Vintage Drive", "Delay Designer", "Mastering Assistant",
    "Post Fader",
    "Pre Fader",
    "No Send",
    "No Group",
    "Group 1: (new)",
    "Mixer Undo Deluxe Classic: Solo",
    "Bus 1", "Bus 2", "Bus 3", "Bus 4", "Bus 5", "Bus 6", "Bus 7", "Bus 8", "Bus 9", "Bus 10", "Bus 11", "Bus 12", "Bus 13", "Bus 14", "Bus 15", "Bus 16", "Bus 17", "Bus 18", "Bus 19", "Bus 20", "Bus 21", "Bus 22", "Bus 23", "Bus 24", "Bus 25", "Bus 26", "Bus 27", "Bus 28", "Bus 29", "Bus 30", "Bus 31", "Bus 32", "Bus 33", "Bus 34", "Bus 35", "Bus 36", "Bus 37", "Bus 38", "Bus 39", "Bus 40", "Bus 41", "Bus 42", "Bus 43", "Bus 44", "Bus 45", "Bus 46", "Bus 47", "Bus 48", "Bus 49", "Bus 50", "Bus 51", "Bus 52", "Bus 53", "Bus 54", "Bus 55", "Bus 56", "Bus 57", "Bus 58", "Bus 59", "Bus 60", "Bus 61", "Bus 62", "Bus 63", "Bus 64", "Bus 65", "Bus 66", "Bus 67", "Bus 68", "Bus 69", "Bus 70", "Bus 71", "Bus 72", "Bus 73", "Bus 74", "Bus 75", "Bus 76", "Bus 77", "Bus 78", "Bus 79", "Bus 80", "Bus 81", "Bus 82", "Bus 83", "Bus 84", "Bus 85", "Bus 86", "Bus 87", "Bus 88", "Bus 89", "Bus 90", "Bus 91", "Bus 92", "Bus 93", "Bus 94", "Bus 95", "Bus 96", "Bus 97", "Bus 98", "Bus 99", "Bus 100", "Bus 101", "Bus 102", "Bus 103", "Bus 104", "Bus 105", "Bus 106", "Bus 107", "Bus 108", "Bus 109", "Bus 110", "Bus 111", "Bus 112", "Bus 113", "Bus 114", "Bus 115", "Bus 116", "Bus 117", "Bus 118", "Bus 119", "Bus 120", "Bus 121", "Bus 122", "Bus 123", "Bus 124", "Bus 125", "Bus 126", "Bus 127", "Bus 128", "Bus 129", "Bus 130", "Bus 131", "Bus 132", "Bus 133", "Bus 134", "Bus 135", "Bus 136", "Bus 137", "Bus 138", "Bus 139", "Bus 140", "Bus 141", "Bus 142", "Bus 143", "Bus 144", "Bus 145", "Bus 146", "Bus 147", "Bus 148", "Bus 149", "Bus 150", "Bus 151", "Bus 152", "Bus 153", "Bus 154", "Bus 155", "Bus 156", "Bus 157", "Bus 158", "Bus 159", "Bus 160", "Bus 161", "Bus 162", "Bus 163", "Bus 164", "Bus 165", "Bus 166", "Bus 167", "Bus 168", "Bus 169", "Bus 170", "Bus 171", "Bus 172", "Bus 173", "Bus 174", "Bus 175", "Bus 176", "Bus 177", "Bus 178", "Bus 179", "Bus 180", "Bus 181", "Bus 182", "Bus 183", "Bus 184", "Bus 185", "Bus 186", "Bus 187", "Bus 188", "Bus 189", "Bus 190", "Bus 191", "Bus 192", "Bus 193", "Bus 194", "Bus 195", "Bus 196", "Bus 197", "Bus 198", "Bus 199", "Bus 200", "Bus 201", "Bus 202", "Bus 203", "Bus 204", "Bus 205", "Bus 206", "Bus 207", "Bus 208", "Bus 209", "Bus 210", "Bus 211", "Bus 212", "Bus 213", "Bus 214", "Bus 215", "Bus 216", "Bus 217", "Bus 218", "Bus 219", "Bus 220", "Bus 221", "Bus 222", "Bus 223", "Bus 224", "Bus 225", "Bus 226", "Bus 227", "Bus 228", "Bus 229", "Bus 230", "Bus 231", "Bus 232", "Bus 233", "Bus 234", "Bus 235", "Bus 236", "Bus 237", "Bus 238", "Bus 239", "Bus 240", "Bus 241", "Bus 242", "Bus 243", "Bus 244", "Bus 245", "Bus 246", "Bus 247", "Bus 248", "Bus 249", "Bus 250", "Bus 251", "Bus 252", "Bus 253", "Bus 254", "Bus 255", "Bus 256",
];

mod generated {
    include!(concat!(env!("OUT_DIR"), "/handled.rs"));
}

/// A command `App::key_command` runs (the build lists them).
pub fn handled(label: &str) -> bool {
    generated::HANDLED.contains(&label)
}

pub fn enabled(app: &App, label: &str) -> bool {
    if label.starts_with("Undo ") {
        return !app.undo.is_empty();
    }
    if label.starts_with("Can’t Redo") || label.starts_with("Redo") {
        return !app.redo.is_empty();
    }
    if label.starts_with("Mixer Undo") {
        return !app.mix.undo.is_empty();
    }
    if label == "Mixer Redo" {
        return !app.mix.redo.is_empty();
    }
    if !IMPLEMENTED.contains(&label) && !handled(label) && !crate::audioedit::COMMANDS.contains(&label) && !crate::recording::COMMANDS.contains(&label) && !crate::takes::COMMANDS.contains(&label) {
        return false;
    }
    if let Some(e) = crate::share::enabled(app, label) {
        return e;
    }
    if let Some(e) = crate::arranging::enabled(app, label) {
        return e;
    }
    if let Some(e) = crate::tm::enabled(app, label) {
        return e;
    }
    if let Some(e) = crate::trackarea::enabled(app, label).or_else(|| crate::regionops::enabled(app, label)) {
        return e;
    }
    match label {
        "Close Project" | "Close" | "Move Locators Forward by Cycle Length" | "Project or Section…" | "All MIDI Tracks as MIDI File…" => app.project.is_some(),
        "1 Track as Audio File…" => app.selected_track.is_some(),
        "Save" | "Save As…" | "Save A Copy As…" | "Revert to Saved" | "Save as Template…" | "Clean Up…" | "Consolidate…" | "Rename…" | "Show in Finder…" | "New Alternative…" | "Edit Alternatives…" | "Export Alternative as Project…" => app.file.is_some() && !app.demo,
        _ => true,
    }
}

fn item_ui(app: &mut App, ui: &mut egui::Ui, it: &Item, fired: &mut Option<String>) {
    if it.separator {
        ui.separator();
        return;
    }
    // (the Record menu's choice submenus: their items share names)
    if !it.children.is_empty() && crate::movie::menu_ui(app, ui, &it.label) {
        return;
    }
    if crate::stretch::menu_ui(app, ui, it) || crate::tm::menu_ui(app, ui, it) || crate::mixext::menu_ui(app, ui, it) || crate::arranging::menu_ui(app, ui, it) || crate::keycmds::menu_ui(app, ui, it) || crate::prefs::menu_ui(app, ui, it) || crate::recording::menu_ui(app, ui, it) || crate::nav::menu_ui(app, ui, it) || crate::screensets::menu_ui(app, ui, it) || crate::projfile::menu_ui(app, ui, it) || crate::autoext::menu_ui(app, ui, it) || crate::cellinsp::menu_ui(app, ui, it) {
        return;
    }
    if !it.children.is_empty() {
        ui.menu_button(crate::shown_name(&it.label), |ui| {
            for c in &it.children {
                item_ui(app, ui, c, fired);
            }
        });
        return;
    }
    let on = enabled(app, &it.label);
    let checked = checked(app, &it.label);
    let shown = app.menu_label(&it.label);
    let label = if checked { format!("✓ {shown}") } else { shown };
    // (the key the edited key command set gives it)
    let short = crate::keycmds::shortcut_for(app, &it.label).unwrap_or_else(|| it.shortcut.clone());
    let b = egui::Button::new(label).shortcut_text(short);
    let resp = ui.add_enabled(on, b);
    if resp.clicked() {
        // Control-choosing a command opens the Key Commands window with it selected
        if ui.input(|i| i.modifiers.ctrl) {
            app.keycmds.open_with(&it.label);
        } else {
            *fired = Some(it.label.clone());
        }
        ui.close();
    }
}

pub(crate) fn checked(app: &App, label: &str) -> bool {
    if let Some(c) = crate::tm::checked(app, label).or_else(|| crate::autoext::checked(app, label)) {
        return c;
    }
    if let Some(c) = crate::globals::checked(app, label) {
        return c;
    }
    if let Some(c) = crate::envhub::checked(app, label) {
        return c;
    }
    if let Some(c) = crate::stopbutton::checked(app, label) {
        return c;
    }
    if let Some(c) = crate::audioedit::checked(app, label) {
        return c;
    }
    if let Some(c) = crate::mixercmd::checked(app, label) {
        return c;
    }
    if let Some(c) = crate::trackarea::checked(app, label).or_else(|| crate::regionops::checked(app, label)) {
        return c;
    }
    if let Some(c) = crate::midifunc::checked(app, label) {
        return c;
    }
    if let Some(c) = crate::recording::checked(app, label) {
        return c;
    }
    if let Some(c) = crate::nav::checked(app, label).or_else(|| crate::toolbar::checked(app, label)) {
        return c;
    }
    match label {
        "Cycle Mode" => app.cycle_on(),
        _ => false,
    }
}

/// The menu bar; returns where it ends.
pub fn menu_bar(app: &mut App, ui: &mut egui::Ui) -> f32 {
    let mut fired = None;
    let mut right = ui.min_rect().min.x;
    egui::MenuBar::new().ui(ui, |ui| {
        // as the suite's other apps (Keydeck's menu bar): the hover highlight roomy at the
        // sides, the gap between titles shrunk to keep the bar's width
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().button_padding.x = 7.0;
        ui.spacing_mut().item_spacing.x = (gap - 10.0).max(0.0);
        for m in menus() {
            // the app menu (left of File): the update notice and the thank-you line
            let app_menu = m.label == "Bramble Studio";
            // the app menu shows the app's mark (the Echoes rings) instead of its name, as Keydeck,
            // Values, Letters and Notes do
            let label = if app_menu { "\u{2003}\u{2002}".to_string() } else { m.label.clone() };
            let r = ui.menu_button(label, |ui| {
                ui.set_min_width(240.0);
                crate::screensets::IN_WINDOW_MENU.with(|c| c.set(m.label == "Window"));
                for c in &m.children {
                    item_ui(app, ui, c, &mut fired);
                    if app_menu && c.label == "About Bramble Studio" {
                        kn_ui::update::menu_item(ui, 0.0);
                    }
                }
            });
            crate::screensets::IN_WINDOW_MENU.with(|c| c.set(false));
            if app_menu {
                let b = r.response.rect;
                echoes_mark(ui.painter(), b.center() + egui::vec2(0.0, 1.0), crate::look::text());
                kn_ui::update::title_dot(ui, b);
                let resp = r.response.clone().on_hover_text(crate::APP_NAME);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, crate::APP_NAME));
            }
            right = right.max(r.response.rect.max.x);
        }
    });
    if let Some(cmd) = fired {
        app.command(ui.ctx(), &cmd);
    }
    right
}

/// A local menu bar as Logic draws it: "Edit ⌄ | Functions ⌄ | View ⌄" in one rounded capsule.
pub fn local_bar(app: &mut App, ui: &mut egui::Ui, menus: &[(&str, &str)]) {
    let bg = ui.painter().add(egui::Shape::Noop);
    let start = ui.cursor().min.x;
    let mut seps = Vec::new();
    ui.spacing_mut().item_spacing.x = 0.0;
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
        for (i, (title, name)) in menus.iter().enumerate() {
            if i > 0 {
                seps.push(ui.cursor().min.x);
            }
            ui.menu_button(format!("{title} ⌄"), |ui| {
                app.midi.menu_roll = name.starts_with("pianoroll");
                show_items(app, ui, local(name));
                if *name == "pianoroll-View" {
                    crate::rollextras::view_items(app, ui);
                }
                app.midi.menu_roll = false;
            });
        }
    });
    let end = ui.min_rect().max.x;
    let h = 24.0;
    let cy = ui.min_rect().center().y;
    let r = egui::Rect::from_min_max(egui::pos2(start - 4.0, cy - h / 2.0), egui::pos2(end + 4.0, cy + h / 2.0));
    let mut shapes = vec![egui::Shape::rect_filled(r, 6.0, crate::look::group_bg())];
    for x in seps {
        // (Logic's dividers between the menus: a light grey hairline)
        shapes.push(egui::Shape::line_segment([egui::pos2(x, cy - 7.0), egui::pos2(x, cy + 7.0)], egui::Stroke::new(1.0, crate::look::text_dim().gamma_multiply(0.55))));
    }
    ui.painter().set(bg, egui::Shape::Vec(shapes));
    ui.add_space(12.0);
}

/// The app's mark in line form: three rings touching at the bottom, as on the app icon.
pub fn echoes_mark(p: &egui::Painter, c: egui::Pos2, col: egui::Color32) {
    let bottom = c.y + 8.4;
    for r in [8.4f32, 5.5, 2.7] {
        p.circle_stroke(egui::pos2(c.x, bottom - r), r, egui::Stroke::new(1.3, col));
    }
}
