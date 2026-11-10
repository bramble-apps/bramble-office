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
    let text = include_str!("../data/menus.txt");
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
        let level = indent.saturating_sub(2) / 4;
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

/// The commands Studio runs (menu labels; "Menu > Item" where a label is ambiguous).
pub const IMPLEMENTED: &[&str] = &[
    "About Bramble Studio",
    "General…",
    "View…",
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
    "Save As…",
    "Save A Copy As…",
    "1 Track as Audio File…",
    "All MIDI Tracks as MIDI File…",
];

pub fn enabled(app: &App, label: &str) -> bool {
    if label.starts_with("Undo ") {
        return !app.undo.is_empty();
    }
    if label.starts_with("Can’t Redo") || label.starts_with("Redo") {
        return !app.redo.is_empty();
    }
    if !IMPLEMENTED.contains(&label) {
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
            let r = ui.menu_button(&m.label, |ui| {
                ui.set_min_width(240.0);
                for c in &m.children {
                    item_ui(app, ui, c, &mut fired);
                }
            });
            right = right.max(r.response.rect.max.x);
        }
    });
    if let Some(cmd) = fired {
        app.command(ui.ctx(), &cmd);
    }
    right
}
