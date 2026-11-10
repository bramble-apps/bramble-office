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
    resp.context_menu(|ui| show_items(app, ui, items));
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
    if !IMPLEMENTED.contains(&label) && !handled(label) && !crate::audioedit::COMMANDS.contains(&label) {
        return false;
    }
    match label {
        "Close Project" | "Close" | "Move Locators Forward by Cycle Length" | "Project or Section…" | "All MIDI Tracks as MIDI File…" => app.project.is_some(),
        "1 Track as Audio File…" => app.selected_track.is_some(),
        "Save" | "Save As…" | "Save A Copy As…" => app.file.is_some(),
        _ => true,
    }
}

fn item_ui(app: &mut App, ui: &mut egui::Ui, it: &Item, fired: &mut Option<String>) {
    if it.separator {
        ui.separator();
        return;
    }
    if !it.children.is_empty() {
        ui.menu_button(&it.label, |ui| {
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
    let b = egui::Button::new(label).shortcut_text(it.shortcut.clone());
    if ui.add_enabled(on, b).clicked() {
        *fired = Some(it.label.clone());
        ui.close();
    }
}

fn checked(app: &App, label: &str) -> bool {
    if let Some(c) = crate::audioedit::checked(app, label) {
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
        ui.spacing_mut().button_padding.x = 7.0;
        for m in menus() {
            // the app menu (left of File): the update notice and the thank-you line
            let app_menu = m.label == "Bramble Studio";
            let r = ui.menu_button(&m.label, |ui| {
                ui.set_min_width(240.0);
                for c in &m.children {
                    item_ui(app, ui, c, &mut fired);
                    if app_menu && c.label == "About Bramble Studio" {
                        kn_ui::update::menu_item(ui, 0.0);
                    }
                }
            });
            if app_menu {
                kn_ui::update::title_dot(ui, r.response.rect);
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
            ui.menu_button(format!("{title} ⌄"), |ui| show_items(app, ui, local(name)));
        }
    });
    let end = ui.min_rect().max.x;
    let h = 24.0;
    let cy = ui.min_rect().center().y;
    let r = egui::Rect::from_min_max(egui::pos2(start - 4.0, cy - h / 2.0), egui::pos2(end + 4.0, cy + h / 2.0));
    let mut shapes = vec![egui::Shape::rect_filled(r, 6.0, crate::look::group_bg())];
    for x in seps {
        shapes.push(egui::Shape::line_segment([egui::pos2(x, cy - 7.0), egui::pos2(x, cy + 7.0)], egui::Stroke::new(1.0, crate::look::line())));
    }
    ui.painter().set(bg, egui::Shape::Vec(shapes));
    ui.add_space(12.0);
}
