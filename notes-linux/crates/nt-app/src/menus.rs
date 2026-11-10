//! The menu bar, generated from Notes 4.13's own menu tree (data/menus.txt, captured on the Mac:
//! features/menus/menu-tree-raw-editing-note.txt) the way Letters, Keydeck and Values generate
//! theirs: items Notes implements run a command, the rest are shown greyed so the gaps stay
//! visible. Every shortcut in the tree (⌘ = Ctrl, ⌥ = Alt, ⇧ = Shift, ⌃ = Super) is a keyboard
//! shortcut here. Also the shell's context menus (features/context-menus/): a note in the list,
//! a folder, the toolbar's … (Actions) and View Options menus.
//!
//! Editing commands (Format, most of Edit) go to the editor through `app.editor_cmds`, named as
//! the menu names them; the editor reports what's on in `app.editor_state`.

use std::sync::OnceLock;

use egui::{Key, KeyboardShortcut, Modifiers};
use nt_core::model::ParaStyle;

use crate::app::{App, Dialog, Focus, Place};

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub label: String,
    pub shortcut: Option<String>,
    pub children: Vec<Item>,
    pub separator: bool,
    /// (Writing Direction's items, indented under a greyed heading)
    pub indent: bool,
    /// greyed on the Mac too (a heading)
    pub heading: bool,
}

/// Mac-only items, Apple services and Apple Intelligence: no meaning here.
const MAC_ONLY: &[&str] = &[
    "Accounts…", "Services", "Hide Notes", "Hide Others", "Show All", "Quit and Keep Windows", "Writing Tools", "Speech", "AutoFill", "Start Dictation", "Emoji & Symbols", "Minimize All", "Zoom All",
    "Fill", "Center", "Move & Resize", "Full Screen Tile", "Bring All to Front", "Arrange in Front", "Remove Window from Set", "Send Notes Feedback", "Show Highlights", "Show Note Activity",
    "Show Folder Activity", "Open in Pages",
];

fn parse() -> Vec<Item> {
    let mut menus: Vec<Item> = Vec::new();
    let mut heading = String::new();
    for line in include_str!("../data/menus.txt").lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let Some(path) = f.first() else { continue };
        // "Format > Alignment > Writing Direction > \tDefault": an item under a heading
        let indent = path.ends_with("> ");
        let (path, rest) = if indent { (format!("{}{}", path, f.get(1).copied().unwrap_or("")), &f[1.min(f.len())..]) } else { (path.to_string(), &f[..]) };
        let parts: Vec<&str> = path.split(" > ").map(str::trim).collect();
        if parts.len() < 2 {
            if parts.len() == 1 && !parts[0].is_empty() && !menus.iter().any(|m| m.label == parts[0]) {
                menus.push(Item { label: parts[0].into(), ..Default::default() });
            }
            continue;
        }
        let shortcut = rest.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()).and_then(linux_shortcut);
        let disabled = rest.get(2).is_some_and(|s| s.contains("disabled"));
        if !menus.iter().any(|m| m.label == parts[0]) {
            menus.push(Item { label: parts[0].into(), ..Default::default() });
        }
        let mut node = menus.iter_mut().find(|m| m.label == parts[0]).unwrap();
        for p in &parts[1..parts.len() - 1] {
            let pos = node.children.iter().rposition(|c| c.label == *p && !c.separator);
            node = match pos {
                Some(i) => &mut node.children[i],
                None => {
                    node.children.push(Item { label: p.to_string(), ..Default::default() });
                    node.children.last_mut().unwrap()
                }
            };
        }
        let label = parts[parts.len() - 1];
        if label.starts_with('<') && !label.starts_with("<<") {
            continue;
        }
        if label == "---" {
            node.children.push(Item { separator: true, ..Default::default() });
            continue;
        }
        if node.children.last().is_some_and(|c| c.label == label) {
            continue;
        }
        // (a disabled item followed by indented ones is their heading: "Paragraph", "Selection")
        let is_heading = disabled && node.label == "Writing Direction" && !indent;
        if is_heading {
            heading = label.to_string();
        }
        let label = if indent { format!("{heading} {label}") } else { label.to_string() };
        node.children.push(Item { label, shortcut, indent, heading: is_heading, ..Default::default() });
    }
    fn prune(items: &mut Vec<Item>) {
        items.retain(|i| !MAC_ONLY.contains(&i.label.as_str()) && !i.label.starts_with("<<") && !(i.label.starts_with("Note “")));
        for i in items.iter_mut() {
            if !i.children.is_empty() {
                prune(&mut i.children);
                if i.children.iter().all(|c| c.separator) {
                    i.label.clear();
                    i.separator = true;
                }
            }
        }
        let mut out: Vec<Item> = Vec::new();
        for i in items.drain(..) {
            if i.separator && (out.is_empty() || out.last().unwrap().separator) {
                continue;
            }
            out.push(i);
        }
        while out.last().is_some_and(|i| i.separator) {
            out.pop();
        }
        *items = out;
    }
    prune(&mut menus);
    // the Recent Notes list and File > Share are filled in as the menus open
    for m in &mut menus {
        for c in &mut m.children {
            if c.label == "Recent Notes" {
                c.children.retain(|k| k.separator || matches!(k.label.as_str(), "Previous Note" | "Next Note" | "Clear Menu"));
            }
        }
    }
    menus
}

/// "⇧⌘Z" -> "Ctrl+Shift+Z" (None for keys a PC keyboard has no equivalent for).
fn linux_shortcut(mac: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut key = String::new();
    let mac = mac.replace("[0xf700]", "↑").replace("[0xf701]", "↓").replace("[0xf702]", "←").replace("[0xf703]", "→").replace("[0x8]", "⌦");
    for c in mac.chars() {
        match c {
            '⌃' => parts.push("Super"),
            '⌥' => parts.push("Alt"),
            '⇧' => parts.push("Shift"),
            '⌘' => parts.push("Ctrl"),
            _ => key.push(c),
        }
    }
    if key == "🎤" || (parts.is_empty() && key.chars().all(|c| c.is_ascii_uppercase())) && key != "⌦" {
        return None;
    }
    parts.sort_by_key(|p| match *p {
        "Ctrl" => 0,
        "Super" => 1,
        "Alt" => 2,
        _ => 3,
    });
    let key = match key.as_str() {
        "⌫" => "Backspace".to_string(),
        "⌦" => "Delete".to_string(),
        "↩" => "Enter".to_string(),
        "←" => "Left".to_string(),
        "→" => "Right".to_string(),
        "↑" => "Up".to_string(),
        "↓" => "Down".to_string(),
        k => k.to_uppercase(),
    };
    parts.push(&key);
    Some(parts.join("+"))
}

/// The keyboard shortcut of a shortcut text ("Ctrl+Shift+Z"); None for ones that use Super.
fn keyboard(s: &str) -> Option<KeyboardShortcut> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for p in s.split('+').filter(|p| !p.is_empty()) {
        match p {
            "Ctrl" => m |= Modifiers::COMMAND,
            "Alt" => m |= Modifiers::ALT,
            "Shift" => m |= Modifiers::SHIFT,
            "Super" => return None,
            k => key = Some(k),
        }
    }
    let key = key.unwrap_or(if s.ends_with('+') { "+" } else { "" });
    let k = match key {
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Enter" => Key::Enter,
        "Left" => Key::ArrowLeft,
        "Right" => Key::ArrowRight,
        "Up" => Key::ArrowUp,
        "Down" => Key::ArrowDown,
        "+" => Key::Plus,
        "-" => Key::Minus,
        "." => Key::Period,
        "," => Key::Comma,
        "'" => Key::Quote,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        ";" => Key::Semicolon,
        ":" => {
            m |= Modifiers::SHIFT;
            Key::Semicolon
        }
        "?" => {
            m |= Modifiers::SHIFT;
            Key::Slash
        }
        "|" => {
            m |= Modifiers::SHIFT;
            Key::Backslash
        }
        "{" => {
            m |= Modifiers::SHIFT;
            Key::OpenBracket
        }
        "}" => {
            m |= Modifiers::SHIFT;
            Key::CloseBracket
        }
        k => Key::from_name(k)?,
    };
    Some(KeyboardShortcut::new(m, k))
}

pub fn menus() -> &'static [Item] {
    static M: OnceLock<Vec<Item>> = OnceLock::new();
    M.get_or_init(parse)
}

/// Submenus whose items are named with the submenu ("Sort By > Title").
const QUALIFIED: &[&str] = &["Export To", "Find", "More", "Move Item", "Font", "Baseline", "Alignment", "Writing Direction", "Indentation", "Math Results", "Recent Notes", "Sort By", "Group By Date", "Attachment View", "Spelling and Grammar", "Substitutions", "Transformations"];

pub fn command_name(parent: &str, label: &str) -> String {
    if QUALIFIED.contains(&parent) { format!("{parent} > {label}") } else { label.to_string() }
}

/// Commands the editor carries out (sent to it through `app.editor_cmds`).
fn editor_command(name: &str) -> bool {
    let shell = ["Find > Note List Search…"];
    if shell.contains(&name) {
        return false;
    }
    matches!(
        name,
        "Undo" | "Undo Typing" | "Redo" | "Cut" | "Copy" | "Copy as Markdown" | "Paste" | "Paste and Match Style" | "Paste and Retain Style" | "Paste as Markdown" | "Select All" | "Add Link…" | "Insert Divider Line" | "Rename Attachment…" | "Title" | "Heading" | "Subheading" | "Body" | "Monostyled" | "Bulleted List" | "Dashed List" | "Numbered List" | "Block Quote" | "Checklist" | "Mark as Checked" | "Table" | "Convert to Text" | "Expand Section" | "Expand All Sections" | "Collapse Section" | "Collapse All Sections" | "Markup"
    ) || ["Find > ", "More > ", "Move Item > ", "Font > ", "Baseline > ", "Alignment > ", "Writing Direction > ", "Indentation > ", "Math Results > ", "Attachment View > ", "Spelling and Grammar > ", "Substitutions > ", "Transformations > ", "Highlight > "].iter().any(|p| name.starts_with(p)) && !matches!(name, "More > New Smart Folder with Tag Selection" | "More > Convert Folder to Smart Folder")
}

/// Commands that work; everything else in the menus is greyed.
const IMPLEMENTED: &[&str] = &[
    "About Notes", "Settings…", "Quit Notes", "New Note", "New Folder", "New Smart Folder", "More > New Smart Folder with Tag Selection", "More > Convert Folder to Smart Folder", "Close", "Close All",
    "Import to Notes…", "Import Markdown...", "Export To > Markdown", "Export To > PDF", "Print…", "Pin Note", "Lock Note", "Close All Locked Notes", "Duplicate Note", "Delete", "Rename", "Attach File…", "Find > Note List Search…", "as List", "as Gallery",
    "Recent Notes > Previous Note", "Recent Notes > Next Note", "Recent Notes > Clear Menu", "Sort By > Default (Date Edited)", "Sort By > Date Edited", "Sort By > Date Created", "Sort By > Title",
    "Sort By > Newest First", "Sort By > Oldest First", "Group By Date > Default (On)", "Group By Date > On", "Group By Date > Off", "Hide Sidebar", "Hide Note Count", "Zoom In", "Zoom Out", "Actual Size",
    "Hide Toolbar", "Customize Toolbar…", "Enter Full Screen", "Minimize", "Zoom", "Notes", "Notes Settings", "Notes Help", "Using Tags", "Using Smart Folders", "Toggle Full Screen",
    "Show Attachments Browser", "Show in Note", "Open Note in New Window", "Float on Top", "Record Audio…", "Export…", "Export Library…", "Keyboard Shortcuts", "Import UpNote Library…",
];

fn implemented(name: &str) -> bool {
    IMPLEMENTED.contains(&name) || editor_command(name)
}

fn note_open(app: &App) -> bool {
    app.selected_note().is_some() && !(app.gallery && !app.gallery_open)
}

fn note_editable(app: &App) -> bool {
    note_open(app) && app.selected_note().is_some_and(|n| !n.deleted && crate::lock::open(app, n))
}

pub fn enabled(app: &App, name: &str) -> bool {
    if !implemented(name) {
        return false;
    }
    let st = &app.editor_state;
    match name {
        // (outside the note: the library's own undo — Undo Delete Note, Undo Move Note…)
        "Undo" | "Undo Typing" if library_undo(app) => app.undo.undo_label().is_some(),
        "Redo" if library_undo(app) => app.undo.redo_label().is_some(),
        "Undo" | "Undo Typing" => st.can_undo,
        "Redo" => st.can_redo,
        "Select All" if app.focus == Focus::List => !app.ordered().is_empty(),
        "Cut" | "Copy" | "Copy as Markdown" => st.has_selection,
        "Paste" | "Paste and Match Style" | "Paste and Retain Style" | "Paste as Markdown" | "Select All" => st.focused || app.focus == Focus::Editor,
        "Mark as Checked" => st.in_checklist,
        "Convert to Text" => st.in_table,
        "Rename Attachment…" => st.on_attachment,
        "Show in Note" => crate::attachments::selected_note(app).is_some(),
        "Open Note in New Window" => app.selected_note().is_some_and(|n| !n.deleted) && !crate::lock::hidden(app),
        "Float on Top" => crate::windows::floating(app).is_some(),
        "Find > Use Selection for Find" => st.has_selection,
        "Pin Note" | "Duplicate Note" | "Export To > Markdown" => app.selected_note().is_some_and(|n| !n.deleted),
        // (a locked note only once opened: its text is what's printed)
        "Export To > PDF" | "Print…" => note_open(app) && app.selected_note().is_some_and(|n| crate::lock::open(app, n)),
        "Lock Note" => app.selected_note().is_some_and(|n| !n.deleted),
        "Close All Locked Notes" => !app.lock.unlocked.is_empty(),
        "Delete" => app.focus != Focus::Editor && app.note.is_some() || st.has_selection,
        "Rename" => app.focus == Focus::Sidebar && matches!(&app.place, Place::Folder(f) if app.folder(f).is_some_and(|f| f.name != "Notes")),
        "More > Convert Folder to Smart Folder" => matches!(&app.place, Place::Folder(f) if app.folder(f).is_some_and(|f| !App::is_smart(f) && f.name != "Notes" && f.name != "Quick Notes")),
        "More > New Smart Folder with Tag Selection" => matches!(app.place, Place::Tag(_)),
        "Recent Notes > Previous Note" => !app.recent.is_empty(),
        "Recent Notes > Next Note" => false,
        "Recent Notes > Clear Menu" => !app.recent.is_empty(),
        "Attach File…" => note_editable(app),
        // (Notes: locked notes can't hold audio)
        "Record Audio…" => note_editable(app) && app.selected_note().is_some_and(|n| !n.locked),
        // (the note, or the list's notes)
        "Export…" => app.selected_note().is_some_and(|n| !n.deleted) || !app.ordered().is_empty(),
        // (the app's own library, whole: a .bnotes file)
        "Export Library…" => app.db.is_some(),
        "Zoom In" => app.zoom < 3.0,
        "Zoom Out" => app.zoom > 0.5,
        n if editor_command(n) => note_editable(app),
        _ => true,
    }
}

pub(crate) fn checked(app: &App, name: &str) -> Option<bool> {
    let st = &app.editor_state;
    let v = app.view();
    Some(match name {
        "as List" => !app.gallery,
        "as Gallery" => app.gallery,
        "Sort By > Default (Date Edited)" => v.sort.is_none(),
        "Sort By > Date Edited" => v.sort == Some(0),
        "Sort By > Date Created" => v.sort == Some(1),
        "Sort By > Title" => v.sort == Some(2),
        "Sort By > Newest First" => !v.oldest_first,
        "Sort By > Oldest First" => v.oldest_first,
        "Group By Date > Default (On)" => v.group.is_none(),
        "Group By Date > On" => v.group == Some(true),
        "Group By Date > Off" => v.group == Some(false),
        "Title" => st.style == Some(ParaStyle::Title),
        "Heading" => st.style == Some(ParaStyle::Heading),
        "Subheading" => st.style == Some(ParaStyle::Subheading),
        "Body" => st.style.is_none_or(|s| s == ParaStyle::Body),
        "Monostyled" => st.style == Some(ParaStyle::Monostyled),
        "Bulleted List" => st.style == Some(ParaStyle::Bulleted),
        "Dashed List" => st.style == Some(ParaStyle::Dashed),
        "Numbered List" => st.style == Some(ParaStyle::Numbered),
        "Checklist" => st.style == Some(ParaStyle::Checklist),
        "Font > Bold" => st.bold,
        "Font > Italic" => st.italic,
        "Font > Underline" => st.underline,
        "Font > Strikethrough" => st.strike,
        "Spelling and Grammar > Check Spelling While Typing" => app.settings.spell_check,
        "Spelling and Grammar > Check Grammar With Spelling" => app.settings.grammar,
        "Spelling and Grammar > Correct Spelling Automatically" => app.settings.autocorrect,
        "Substitutions > Smart Quotes" => app.settings.smart_quotes,
        "Substitutions > Smart Dashes" => app.settings.smart_dashes,
        "Substitutions > Smart Lists" => app.settings.smart_lists,
        "Substitutions > Smart Copy/Paste" => app.settings.smart_copy_paste,
        "Substitutions > Smart Links" => app.settings.smart_links,
        "Substitutions > Smart Tags" => app.settings.smart_tags,
        "Substitutions > Text Replacement" => app.settings.text_replacement,
        "Math Results > Suggest Results" => true,
        "Alignment > Align Left" => true,
        // (the paragraphs' Notes field 3: 1 natural, 0 left to right, 2 right to left; none ticked
        // when the selection's paragraphs differ)
        "Writing Direction > Paragraph Default" | "Writing Direction > Selection Default" => st.direction == Some(1),
        "Writing Direction > Paragraph Left to Right" | "Writing Direction > Selection Left to Right" => st.direction == Some(0),
        "Writing Direction > Paragraph Right to Left" | "Writing Direction > Selection Right to Left" => st.direction == Some(2),
        "Substitutions > Data Detectors" => st.data_detectors,
        "Float on Top" => crate::windows::floating(app) == Some(true),
        _ => return None,
    })
}

/// Edit > Spelling and Grammar's and Substitutions' ticks, as settings.
fn typing_setting<'a>(app: &'a mut App, name: &str) -> Option<&'a mut bool> {
    let s = &mut app.settings;
    Some(match name {
        "Spelling and Grammar > Check Spelling While Typing" => &mut s.spell_check,
        "Spelling and Grammar > Check Grammar With Spelling" => &mut s.grammar,
        "Spelling and Grammar > Correct Spelling Automatically" => &mut s.autocorrect,
        "Substitutions > Smart Quotes" => &mut s.smart_quotes,
        "Substitutions > Smart Dashes" => &mut s.smart_dashes,
        "Substitutions > Smart Lists" => &mut s.smart_lists,
        "Substitutions > Smart Copy/Paste" => &mut s.smart_copy_paste,
        "Substitutions > Smart Links" => &mut s.smart_links,
        "Substitutions > Smart Tags" => &mut s.smart_tags,
        "Substitutions > Text Replacement" => &mut s.text_replacement,
        _ => return None,
    })
}

/// What a command's item says now (Show / Hide and Pin / Unpin follow the state).
fn shown_label(app: &App, name: &str, item_label: &str) -> String {
    match name {
        "Hide Sidebar" if !app.show_sidebar => "Show Sidebar".into(),
        "Hide Note Count" if !app.show_counts => "Show Note Count".into(),
        "Hide Toolbar" if !app.show_toolbar => "Show Toolbar".into(),
        "Show Attachments Browser" if app.browser.is_some() => "Hide Attachments Browser".into(),
        "Pin Note" if app.selected_note().is_some_and(|n| n.pinned) => "Unpin Note".into(),
        "Lock Note" if app.selected_note().is_some_and(|n| n.locked) => "Remove Lock".into(),
        "Undo Typing" | "Undo" if library_undo(app) => app.undo.undo_label().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into()),
        "Redo" if library_undo(app) => app.undo.redo_label().map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into()),
        "Undo Typing" | "Undo" => app.editor_state.undo_label.as_ref().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into()),
        "Writing Direction > Paragraph Default" | "Writing Direction > Selection Default" => "Default".into(),
        "Writing Direction > Paragraph Left to Right" | "Writing Direction > Selection Left to Right" => "Left to Right".into(),
        "Writing Direction > Paragraph Right to Left" | "Writing Direction > Selection Right to Left" => "Right to Left".into(),
        _ => item_label.to_string(),
    }
}

fn item_ui(app: &mut App, ui: &mut egui::Ui, parent: &str, item: &Item, fired: &mut Option<String>) {
    if item.separator {
        ui.separator();
        return;
    }
    if item.label == "Recent Notes" {
        ui.menu_button("Recent Notes", |ui| {
            for c in &item.children {
                if c.separator {
                    ui.separator();
                    // the notes opened lately, between Next Note and Clear Menu
                    if !app.recent.is_empty() && !ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("recent-shown")).unwrap_or(false)) {
                        let list: Vec<(String, String)> = app.recent.iter().filter_map(|id| app.note_by_id(id).map(|n| (id.clone(), n.title.clone()))).collect();
                        for (id, t) in list {
                            if ui.button(if t.is_empty() { "New Note".to_string() } else { t }).clicked() {
                                *fired = Some(format!("note:{id}"));
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("recent-shown"), true));
                    }
                    continue;
                }
                item_ui(app, ui, "Recent Notes", c, fired);
            }
            ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::new("recent-shown")));
        });
        return;
    }
    if !item.children.is_empty() {
        ui.menu_button(item.label.as_str(), |ui| {
            for c in &item.children {
                item_ui(app, ui, &item.label, c, fired);
            }
        });
        return;
    }
    let name = command_name(parent, &item.label);
    if item.heading {
        ui.add_enabled(false, egui::Button::new(item.label.as_str()));
        return;
    }
    let label = shown_label(app, &name, &item.label);
    let label = if item.indent { format!("    {label}") } else { label };
    let mut b = egui::Button::new(label);
    if let Some(s) = &item.shortcut {
        b = b.shortcut_text(s);
    }
    if checked(app, &name) == Some(true) {
        b = b.selected(true);
    }
    let on = enabled(app, &name);
    let r = ui.add_enabled(on, b);
    if r.clicked() {
        *fired = Some(name);
        ui.close();
    }
}

/// The menu bar; returns the right edge of its last menu.
pub fn menu_bar(app: &mut App, ui: &mut egui::Ui) -> f32 {
    let mut fired = None;
    let mut right = ui.min_rect().min.x;
    egui::MenuBar::new().ui(ui, |ui| {
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().button_padding.x = 7.0;
        ui.spacing_mut().item_spacing.x = (gap - 10.0).max(0.0);
        // Settings > Appearance > Hide the menus: one button (kn-ui's three lines) at the app
        // menu's place holding every menu, until Alt or F10 shows them (GNOME's way)
        if app.look.hide_menus && !app.menus_peek {
            let r = ui.menu_button("\u{2003}\u{2002}", |ui| {
                for m in menus() {
                    let label = if m.label == "Notes" { "Notes".to_string() } else { m.label.clone() };
                    let sub = ui.menu_button(label, |ui| {
                        for c in &m.children {
                            item_ui(app, ui, &m.label, c, &mut fired);
                            if m.label == "Notes" && c.label == "About Notes" {
                                kn_ui::update::menu_item(ui, 0.0);
                            }
                        }
                        if m.label == "Notes" {
                            kn_ui::update::menu_footer(ui);
                        }
                    });
                    if m.label == "Notes" {
                        kn_ui::update::title_dot(ui, sub.response.rect);
                    }
                }
            });
            let b = r.response.rect;
            kn_ui::icons::draw(ui.painter(), egui::Rect::from_center_size(b.center(), egui::vec2(18.0, 18.0)), kn_ui::icons::Icon::Menu, kn_ui::theme::TEXT());
            kn_ui::update::title_dot(ui, b);
            r.response.on_hover_text("Menus (Alt or F10 shows them in the title bar)");
            right = right.max(b.max.x);
            return;
        }
        for m in menus() {
            // the app menu shows a note mark instead of its name (as Keydeck and Letters do)
            let app_menu = m.label == "Notes";
            let label = if app_menu { "\u{2003}\u{2002}".to_string() } else { m.label.clone() };
            let r = ui.menu_button(label, |ui| {
                for c in &m.children {
                    item_ui(app, ui, &m.label, c, &mut fired);
                    if app_menu && c.label == "About Notes" {
                        kn_ui::update::menu_item(ui, 0.0);
                    }
                }
                if app_menu {
                    kn_ui::update::menu_footer(ui);
                }
            });
            if app_menu {
                let b = r.response.rect;
                crate::icons::draw(ui.painter(), egui::Rect::from_center_size(b.center(), egui::vec2(17.0, 17.0)), crate::icons::Glyph::QuickNote, kn_ui::theme::TEXT());
                kn_ui::update::title_dot(ui, b);
            }
            right = right.max(r.response.rect.max.x);
        }
    });
    if let Some(cmd) = fired {
        app.menus_peek = false;
        menu_action(app, ui.ctx(), &if cmd.starts_with("note:") { cmd } else { format!("cmd:{cmd}") });
    }
    right
}

/// Hidden menus shown for a while: F10, or Alt pressed and let go on its own, toggles them;
/// Escape or a click outside the title bar hides them again (GNOME's behaviour).
pub fn peek_keys(app: &mut App, ctx: &egui::Context, title: egui::Rect) {
    if !app.look.hide_menus {
        app.menus_peek = false;
        return;
    }
    let (alt, keys, f10, esc, press) = ctx.input(|i| {
        let keys = i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)));
        (i.modifiers.alt, keys, i.key_pressed(Key::F10), i.key_pressed(Key::Escape), i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !title.contains(p)))
    });
    if alt {
        if !app.alt_down {
            app.alt_down = true;
            app.alt_clean = true;
        }
        if keys || ctx.input(|i| i.pointer.any_down()) {
            app.alt_clean = false;
        }
    } else if app.alt_down {
        app.alt_down = false;
        if app.alt_clean {
            app.menus_peek = !app.menus_peek;
        }
    }
    if f10 {
        app.menus_peek = !app.menus_peek;
    }
    let menu_open = egui::Popup::is_any_open(ctx);
    if (esc || press) && !menu_open {
        app.menus_peek = false;
    }
}

fn shortcut_table() -> &'static [(KeyboardShortcut, String)] {
    static T: OnceLock<Vec<(KeyboardShortcut, String)>> = OnceLock::new();
    T.get_or_init(|| {
        let mut out = Vec::new();
        fn walk(items: &[Item], parent: &str, out: &mut Vec<(KeyboardShortcut, String)>) {
            for it in items {
                if !it.children.is_empty() {
                    walk(&it.children, &it.label, out);
                } else if let Some(k) = it.shortcut.as_deref().and_then(keyboard) {
                    out.push((k, command_name(parent, &it.label)));
                }
            }
        }
        for m in menus() {
            walk(&m.children, &m.label, &mut out);
        }
        out.push((KeyboardShortcut::new(Modifiers::NONE, Key::F11), "Toggle Full Screen".into()));
        out.push((KeyboardShortcut::new(Modifiers::COMMAND, Key::Y), "Redo".into()));
        out.sort_by_key(|(k, _)| std::cmp::Reverse(k.modifiers.shift as u8 + k.modifiers.alt as u8 + k.modifiers.command as u8));
        out
    })
}

/// Keyboard shortcuts of the menus (the editor's own keys — the clipboard, arrows, Delete — stay
/// with it while it has the keyboard).
pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    let typing = ctx.egui_wants_keyboard_input();
    let mut fire = None;
    for (k, name) in shortcut_table() {
        if k.modifiers.is_none() && k.logical_key != Key::F11 {
            continue;
        }
        if (typing || app.focus == Focus::Editor) && matches!(name.as_str(), "Cut" | "Copy" | "Paste" | "Delete" | "Undo" | "Undo Typing" | "Redo" | "Select All") {
            continue;
        }
        if typing && !k.modifiers.command {
            continue;
        }
        if !enabled(app, name) {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(k)) {
            fire = Some(name.clone());
            break;
        }
    }
    if let Some(n) = fire {
        run(app, ctx, &n);
    }
}

/// How long a context menu takes to fade out as it closes (out-NO1 animations: it opens at once
/// and fades over ~117 ms, the standard NSMenu).
const MENU_FADE_S: f64 = 0.117;

/// A context menu on `resp` (right-click), the app's one way to show one: egui's context menu,
/// which as it closes leaves its last look fading out over ~117 ms, as the Mac's do (`add` draws
/// it again then in a layer that can't be clicked, so nothing in it runs).
pub fn context_menu<R>(resp: &egui::Response, id: egui::Id, mut add: impl FnMut(&mut egui::Ui) -> R) -> Option<R> {
    let ctx = resp.ctx.clone();
    let key = id.with("menu-fade");
    let shown = egui::Popup::context_menu(resp).id(id).show(|ui| add(ui));
    let now = ctx.input(|i| i.time);
    // (where it was, and when it closed)
    let mut state: Option<(egui::Rect, Option<f64>)> = ctx.data(|d| d.get_temp(key));
    match &shown {
        Some(ir) => state = Some((ir.response.rect, None)),
        None => {
            if let Some((r, None)) = state {
                state = Some((r, Some(now)));
            }
        }
    }
    if let Some((r, Some(closed))) = state {
        let u = ((now - closed) / MENU_FADE_S) as f32;
        if u >= 1.0 {
            state = None;
        } else {
            egui::Area::new(key.with("ghost"))
                .order(egui::Order::Foreground)
                .fixed_pos(r.min)
                .interactable(false)
                .fade_in(false)
                .layout(egui::Layout::top_down_justified(egui::Align::Min))
                .info(egui::UiStackInfo::new(egui::UiKind::Menu))
                .show(&ctx, |ui| {
                    if std::env::var_os("NT_MENU_TRACE").is_some() {
                        eprintln!("menu-fade t={:.3} opacity={:.2}", now - closed, 1.0 - u);
                    }
                    ui.multiply_opacity(1.0 - u);
                    egui::containers::menu::menu_style(ui.style_mut());
                    let frame = egui::Frame::popup(ui.style());
                    let w = r.width() - frame.total_margin().sum().x;
                    frame.show(ui, |ui| {
                        ui.set_width(w);
                        add(ui);
                    });
                });
            ctx.request_repaint();
        }
    }
    ctx.data_mut(|d| match state {
        Some(s) => {
            d.insert_temp(key, s);
        }
        None => d.remove::<(egui::Rect, Option<f64>)>(key),
    });
    shown.map(|ir| ir.inner)
}

/// Runs a menu command.
pub fn run(app: &mut App, ctx: &egui::Context, name: &str) {
    let _t = crate::PartTimer::start(format!("command {name}"));
    // the typing ticks live in the settings (the editor copies them across each frame)
    if let Some(flag) = typing_setting(app, name) {
        *flag = !*flag;
        app.settings.save();
        return;
    }
    // the library's undo while the note isn't being edited; ⌘A in the list: every note
    if library_undo(app) && matches!(name, "Undo" | "Undo Typing" | "Redo") {
        if name == "Redo" { crate::undo::redo(app) } else { crate::undo::undo(app) };
        return;
    }
    if name == "Select All" && app.focus == Focus::List {
        app.select_all_notes();
        return;
    }
    if editor_command(name) && !IMPLEMENTED.contains(&name) {
        if enabled(app, name) || std::env::var_os("NT_SNAP").is_some() {
            app.editor_cmds.push(name.to_string());
            app.focus = Focus::Editor;
        }
        return;
    }
    let note = app.note.clone();
    let tb = |s: &str| s.to_string();
    match name {
        "About Notes" => app.dialog = Some(Dialog::Alert("Bramble Notes".into(), "Notes for Linux, part of the Bramble suite. It reads Apple Notes libraries and keeps its own in the same format.".into())),
        "Settings…" | "Notes Settings" => app.show_settings = true,
        "Quit Notes" | "Close" | "Close All" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        "New Note" => app.new_note(),
        "New Folder" => {
            let inside = None;
            app.new_folder(inside);
        }
        "New Smart Folder" => app.dialog = Some(Dialog::SmartFolder { name: "New Smart Folder".into(), smart: true, any: false, rules: vec![crate::smart::Rule::new(0)], convert: None }),
        "More > New Smart Folder with Tag Selection" => {
            // (the tag browser's whole selection: its included and excluded tags, all or any)
            let sel = crate::smart::TagSelection::parse(if let Place::Tag(t) = &app.place { t } else { "" });
            let name = if sel.is_empty() { "New Smart Folder".to_string() } else { sel.title() };
            app.dialog = Some(Dialog::SmartFolder { name, smart: true, any: false, rules: vec![sel.rule()], convert: None });
        }
        "More > Convert Folder to Smart Folder" => {
            if let Place::Folder(f) = &app.place {
                let f = f.clone();
                convert_to_smart(app, &f);
            }
        }
        "Import to Notes…" | "Import Markdown..." => {
            let md = name == "Import Markdown...";
            let picked = crate::app::file_dialog(|d| {
                let d = d.set_title(if md { "Import Markdown" } else { "Import to Notes" });
                let mut kinds = vec!["sqlite", "bnotes"];
                kinds.extend_from_slice(crate::import::EXTENSIONS);
                let d = if md { d.add_filter("Markdown", &["md", "markdown"]) } else { d.add_filter("Notes, Bramble library, text, Markdown, HTML, RTF, Evernote", &kinds) };
                d.pick_files()
            });
            if let Some(ps) = picked {
                crate::dialogs::ask_import(app, ctx, ps);
            }
        }
        // UpNote's library (its folder, its upnote.sqlite3 or a .upnx backup), read on a worker into a
        // Store of its own: into a folder "UpNote", trashed notes into Recently Deleted; again, only
        // what's new. Then read in, as after a sync.
        "Import UpNote Library…" => {
            let start = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config/UpNote"));
            let picked = crate::app::file_dialog(|d| {
                let d = d.set_title("Import UpNote Library").add_filter("UpNote library or backup", &["sqlite3", "upnx"]);
                let d = match start.filter(|s| s.is_dir()) {
                    Some(s) => d.set_directory(s),
                    None => d,
                };
                d.pick_file()
            });
            if let Some(p) = picked {
                import_upnote(app, ctx, p);
            }
        }
        // the note's pages as a PDF, made on a worker (NT_EXPORT_PDF=<path>: there, for tests)
        "Export To > PDF" => {
            if let Some(n) = app.selected_note().cloned() {
                let fname = format!("{}.pdf", if n.title.is_empty() { "New Note" } else { n.title.as_str() }.replace('/', "-"));
                let path = std::env::var_os("NT_EXPORT_PDF").map(std::path::PathBuf::from).or_else(|| crate::app::file_dialog(|d| d.set_file_name(&fname).add_filter("PDF", &["pdf"]).save_file()));
                if let Some(p) = path {
                    app.notify("Exporting…");
                    app.background(ctx, move || match crate::print::to_pdf(&n, &p) {
                        Ok(pages) => crate::app::JobDone::Message(format!("Exported {} ({pages} {})", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), if pages == 1 { "page" } else { "pages" })),
                        Err(e) => crate::app::JobDone::Message(format!("Export failed: {e}")),
                    });
                }
            }
        }
        // the pages made on a worker, then the Print sheet (printer, copies)
        "Print…" => {
            if let Some(n) = app.selected_note().cloned() {
                app.notify("Preparing to print…");
                app.background(ctx, move || {
                    let pdf = std::env::temp_dir().join(format!("bramble-notes-print-{}.pdf", std::process::id()));
                    match crate::print::to_pdf(&n, &pdf) {
                        Ok(_) => crate::app::JobDone::Print(pdf, crate::print::printers()),
                        Err(e) => crate::app::JobDone::Message(format!("Printing failed: {e}")),
                    }
                });
            }
        }
        "Export To > Markdown" => {
            if let Some(n) = app.selected_note().cloned() {
                let fname = format!("{}.md", if n.title.is_empty() { "New Note" } else { n.title.as_str() }.replace('/', "-"));
                if let Some(p) = crate::app::file_dialog(|d| d.set_file_name(&fname).add_filter("Markdown", &["md"]).save_file()) {
                    // nt-core's export (Notes' own Markdown, byte for byte) and its attachment files
                    let ex = nt_core::export::markdown(&n);
                    let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                    let ok = std::fs::write(&p, ex.text).is_ok();
                    for (rel, src) in &ex.files {
                        let to = dir.join(rel);
                        if let Some(d) = to.parent() {
                            let _ = std::fs::create_dir_all(d);
                        }
                        let _ = std::fs::copy(src, to);
                    }
                    app.notify(if ok { format!("Exported to {}", p.display()) } else { "Export failed".into() });
                }
            }
        }
        "Pin Note" => {
            let sel = app.selection();
            app.toggle_pins(&sel);
        }
        "Lock Note" => crate::lock::toggle(app),
        "Close All Locked Notes" => crate::lock::close_all(app),
        "Duplicate Note" => {
            if let Some(n) = note {
                app.duplicate_note(&n);
            }
        }
        "Delete" => {
            if app.focus == Focus::Editor || app.editor_state.has_selection {
                app.editor_cmds.push(tb("Delete"));
            } else if app.focus == Focus::Sidebar {
                if let Place::Folder(f) = &app.place {
                    app.dialog = Some(Dialog::DeleteFolder(f.clone()));
                }
            } else {
                let sel = app.selection();
                app.delete_notes(&sel);
            }
        }
        "Rename" => {
            if let Place::Folder(f) = &app.place {
                let text = app.folder(f).map(|f| f.name.clone()).unwrap_or_default();
                app.renaming = Some(crate::app::Renaming { folder: f.clone(), text, fresh: true });
            }
        }
        "Export…" => crate::export_sheet::open(app, ctx),
        // the whole library as one .bnotes file (notes, folders, files), written on a worker from
        // a Store of its own; File > Import… takes it back (NT_EXPORT_TO=<path>: there, for tests)
        "Export Library…" => {
            let Some(dir) = app.db.as_ref().map(|st| st.dir().to_path_buf()) else { return };
            let to = std::env::var_os("NT_EXPORT_TO").map(std::path::PathBuf::from).or_else(|| crate::app::file_dialog(|d| d.set_title("Export Library").set_file_name("Notes Library.bnotes").add_filter("Bramble Notes library", &["bnotes"]).save_file()));
            if let Some(to) = to {
                let dirty: Vec<String> = app.dirty.keys().cloned().collect();
                for id in dirty {
                    crate::store::save_note(app, &id);
                }
                app.notify("Exporting the library…");
                app.background(ctx, move || match nt_core::store::Store::open(&dir).and_then(|st| nt_core::native::export_bnotes(&st, &to)) {
                    Ok(n) => crate::app::JobDone::Message(format!("Exported the library ({n} {}) to {}", if n == 1 { "note" } else { "notes" }, to.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())),
                    Err(e) => crate::app::JobDone::Message(format!("Export failed: {e}")),
                });
            }
        }
        "Record Audio…" => {
            app.take = None;
            app.dialog = Some(Dialog::RecordAudio);
        }
        "Attach File…" => {
            if let Some(p) = crate::app::file_dialog(|d| d.set_title("Attach File").pick_file()) {
                app.editor_cmds.push(format!("Attach File:{}", p.display()));
            }
        }
        "Find > Note List Search…" => {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("focus-search"), true));
        }
        "as List" => {
            app.gallery = false;
            app.gallery_open = false;
            app.browser = None;
        }
        "as Gallery" => {
            app.gallery = true;
            app.gallery_open = false;
            app.browser = None;
        }
        "Show Attachments Browser" => crate::attachments::toggle(app, ctx),
        "Open Note in New Window" => {
            if let Some(n) = app.note.clone() {
                crate::windows::open(app, ctx, &n);
            }
        }
        "Float on Top" => crate::windows::float_on_top(app, ctx),
        "Show in Note" => {
            if let Some(n) = crate::attachments::selected_note(app) {
                crate::attachments::show_in_note(app, &n);
            }
        }
        "Recent Notes > Previous Note" => {
            if let Some(id) = app.recent.first().cloned() {
                open_note(app, &id);
            }
        }
        "Recent Notes > Clear Menu" => app.recent.clear(),
        "Sort By > Default (Date Edited)" => app.view_mut_of(&app.place.clone()).sort = None,
        "Sort By > Date Edited" => app.view_mut_of(&app.place.clone()).sort = Some(0),
        "Sort By > Date Created" => app.view_mut_of(&app.place.clone()).sort = Some(1),
        "Sort By > Title" => app.view_mut_of(&app.place.clone()).sort = Some(2),
        "Sort By > Newest First" => app.view_mut_of(&app.place.clone()).oldest_first = false,
        "Sort By > Oldest First" => app.view_mut_of(&app.place.clone()).oldest_first = true,
        "Group By Date > Default (On)" => app.view_mut_of(&app.place.clone()).group = None,
        "Group By Date > On" => app.view_mut_of(&app.place.clone()).group = Some(true),
        "Group By Date > Off" => app.view_mut_of(&app.place.clone()).group = Some(false),
        "Hide Sidebar" => app.show_sidebar = !app.show_sidebar,
        "Hide Note Count" => app.show_counts = !app.show_counts,
        "Zoom In" => app.zoom = (app.zoom * 1.1).min(3.0),
        "Zoom Out" => app.zoom = (app.zoom / 1.1).max(0.5),
        "Actual Size" => app.zoom = 1.0,
        "Hide Toolbar" => app.show_toolbar = !app.show_toolbar,
        "Customize Toolbar…" => {
            app.show_toolbar = true;
            app.tb.customizing = true;
        }
        "Enter Full Screen" | "Toggle Full Screen" => {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
        }
        "Minimize" => crate::chrome::minimize(ctx),
        "Zoom" => {
            let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
        }
        "Notes" => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
        "Notes Help" => app.dialog = Some(Dialog::Alert("Notes Help".into(), "Notes keeps notes in folders under each account. Make a note with Ctrl+N, a folder with Ctrl+Shift+N. Type # and a word to tag a note; Smart Folders collect notes by tag and other filters. View > as Gallery shows notes as cards.".into())),
        "Keyboard Shortcuts" => app.dialog = Some(Dialog::Shortcuts),
        "Using Tags" => app.dialog = Some(Dialog::Alert("Using Tags".into(), "Type # followed by a word (#recipes) anywhere in a note to tag it. Tags appear in the sidebar’s Tags section; click one to see every note with it. Smart Folders can collect notes by tag.".into())),
        "Using Smart Folders" => app.dialog = Some(Dialog::Alert("Using Smart Folders".into(), "A Smart Folder shows the notes that match its filters (tags, dates, checklists, attachments, folders, pinned or locked notes) wherever they are kept. Make one with File > New Smart Folder.".into())),
        // (not in the menus: Settings' Appearance, for scripted checks, NT_SCRIPT="menu Appearance > Dark")
        "Appearance > Hide Menus" | "Appearance > Show Menus" => {
            app.look.hide_menus = name.ends_with("Hide Menus");
        }
        "Appearance > Dark" | "Appearance > Light" => {
            app.look.dark = name.ends_with("Dark");
            app.look.apply(ctx);
        }
        // (not in the menus: File > Import to Notes… on a given path, for scripted checks)
        other if other.starts_with("Import Path:") => {
            let ps = other["Import Path:".len()..].split('|').map(|p| std::path::PathBuf::from(p.trim())).collect();
            crate::dialogs::ask_import(app, ctx, ps);
        }
        // (not in the menus: File > Import UpNote Library… on a given path, for scripted checks)
        other if other.starts_with("Import UpNote Path:") => import_upnote(app, ctx, std::path::PathBuf::from(other["Import UpNote Path:".len()..].trim())),
        other => app.notify(format!("{other}: not available yet")),
    }
    app.select_first();
}

/// File > Import UpNote Library…'s work, on a worker: the edits saved first, then nt-core's import
/// into a Store of its own, and what it did said ("Imported 766 notes (70 to Recently Deleted)").
fn import_upnote(app: &mut App, ctx: &egui::Context, path: std::path::PathBuf) {
    let Some(dir) = app.db.as_ref().map(|st| st.dir().to_path_buf()) else { return };
    let dirty: Vec<String> = app.dirty.keys().cloned().collect();
    for id in dirty {
        crate::store::save_note(app, &id);
    }
    app.notify("Importing from UpNote…");
    app.background(ctx, move || match nt_core::store::Store::open(&dir).and_then(|mut st| nt_core::upnote::import(&mut st, &path)) {
        Ok(r) if r.notes == 0 && r.skipped > 0 => crate::app::JobDone::Message(format!("UpNote: all {} {} already imported", r.skipped, if r.skipped == 1 { "note" } else { "notes" })),
        Ok(r) if r.notes == 0 => crate::app::JobDone::Message("UpNote: no notes to import".into()),
        Ok(r) => crate::app::JobDone::Reload(upnote_summary(&r)),
        Err(e) => crate::app::JobDone::Message(format!("Couldn’t import from UpNote: {e}")),
    });
}

/// "Imported 766 notes (70 to Recently Deleted)", and how many were already in.
pub(crate) fn upnote_summary(r: &nt_core::upnote::UpNoteImport) -> String {
    let mut s = format!("Imported {} {}", r.notes, if r.notes == 1 { "note" } else { "notes" });
    if r.trashed > 0 {
        s += &format!(" ({} to Recently Deleted)", r.trashed);
    }
    if r.skipped > 0 {
        s += &format!(", {} already imported", r.skipped);
    }
    s
}

fn open_note(app: &mut App, id: &str) {
    let Some(n) = app.note_by_id(id) else { return };
    let folder = n.folder.clone();
    let deleted = n.deleted;
    let acct = app.account_of(n);
    if !app.notes_in(&app.place).iter().any(|x| x.id == id) {
        app.place = if deleted { Place::Deleted(acct) } else { Place::Folder(folder) };
    }
    app.search.clear();
    app.select_note(id);
    app.list_scroll_to = true;
}

// ---- context menus -------------------------------------------------------------------------

/// A menu item as data (Letters' MItem): drawn with egui, its action run by `menu_action`.
#[derive(Clone, Debug)]
pub enum MItem {
    /// `action`: "cmd:<menu command>" or one of menu_action's own.
    Cmd { label: String, action: String, enabled: bool, checked: bool, glyph: Option<crate::icons::Glyph> },
    Sep,
    Sub { label: String, kids: Vec<MItem>, enabled: bool },
}

impl MItem {
    fn cmd(app: &App, label: &str, cmd: &str) -> MItem {
        MItem::Cmd { label: label.into(), action: format!("cmd:{cmd}"), enabled: enabled(app, cmd), checked: checked(app, cmd) == Some(true), glyph: None }
    }
    pub(crate) fn act(label: impl Into<String>, action: impl Into<String>, enabled: bool) -> MItem {
        MItem::Cmd { label: label.into(), action: action.into(), enabled, checked: false, glyph: None }
    }
    fn check(label: impl Into<String>, action: impl Into<String>, on: bool) -> MItem {
        MItem::Cmd { label: label.into(), action: action.into(), enabled: true, checked: on, glyph: None }
    }
    fn sub(label: &str, kids: Vec<MItem>) -> MItem {
        MItem::Sub { label: label.into(), kids, enabled: true }
    }
}

/// Draws `items`; the action of the one clicked, if any.
pub fn show_items(ui: &mut egui::Ui, items: &[MItem]) -> Option<String> {
    let mut out = None;
    for it in items {
        match it {
            MItem::Cmd { label, action, enabled, checked, glyph } => {
                let r = ui.add_enabled(*enabled, egui::Button::new(if glyph.is_some() { format!("         {label}") } else { label.clone() }).selected(*checked));
                if let Some(g) = glyph {
                    let c = egui::pos2(r.rect.min.x + 18.0, r.rect.center().y);
                    crate::icons::draw(ui.painter(), egui::Rect::from_center_size(c, egui::vec2(13.0, 13.0)), *g, if *enabled { kn_ui::theme::TEXT() } else { kn_ui::theme::TEXT_DIM() });
                }
                if r.clicked() {
                    ui.close();
                    out = Some(action.clone());
                }
            }
            MItem::Sep => {
                ui.separator();
            }
            MItem::Sub { label, kids, enabled } => {
                let r = ui.add_enabled_ui(*enabled, |ui| ui.menu_button(label.as_str(), |ui| show_items(ui, kids)));
                if let Some(a) = r.inner.inner.flatten() {
                    out = Some(a);
                }
            }
        }
    }
    out
}

/// Runs a menu item's action.
pub fn menu_action(app: &mut App, ctx: &egui::Context, action: &str) {
    let _t = crate::PartTimer::start(format!("menu {action}"));
    let (kind, rest) = action.split_once(':').unwrap_or((action, ""));
    match kind {
        "cmd" => run(app, ctx, rest),
        "note" => open_note(app, rest),
        "pin" => {
            let ids = targets(app, rest);
            app.toggle_pins(&ids);
        }
        "lock" => {
            app.select_note(rest);
            crate::lock::toggle(app);
        }
        "dup" => app.duplicate_note(rest),
        // (a note of a multi-selection: all of them, as the Mac's menu does)
        "delete" => {
            let ids = targets(app, rest);
            app.delete_notes(&ids);
        }
        "recover" => app.recover(rest),
        "move" => {
            if let Some((note, folder)) = rest.split_once('|') {
                let ids = targets(app, note);
                app.move_notes(&ids, folder);
            }
        }
        // "nest:<folder>|<parent>": a folder dropped on another (an empty parent: the top level)
        "window" => crate::windows::open(app, ctx, rest),
        // a row's swipe Share menu: the note's export, as the Share button's
        "swipe-export" => {
            if let Some((id, cmd)) = rest.split_once('|') {
                app.select_note(id);
                run(app, ctx, cmd);
            }
        }
        "nest" => {
            if let Some((f, parent)) = rest.split_once('|') {
                let parent = (!parent.is_empty()).then(|| parent.to_string());
                if app.db.is_some() {
                    let (f2, p2) = (f.to_string(), parent.clone());
                    if crate::store::with_db(app, "Moving the folder", |st| st.move_folder(&f2, p2.as_deref())).is_some() {
                        crate::store::reload(app);
                    }
                } else if let Some(fo) = app.library.folders.iter_mut().find(|x| x.id == f) {
                    fo.parent = parent;
                }
            }
        }
        "tag" => {
            if let Some((note, tag)) = rest.split_once('|') {
                app.editor_cmds.push(format!("Toggle Tag:{note}|{tag}"));
            }
        }
        "rename" => {
            let text = app.folder(rest).map(|f| f.name.clone()).unwrap_or_default();
            app.renaming = Some(crate::app::Renaming { folder: rest.to_string(), text, fresh: true });
        }
        "delfolder" => app.dialog = Some(Dialog::DeleteFolder(rest.to_string())),
        "newfolder" => app.new_folder(if rest.is_empty() { None } else { Some(rest.to_string()) }),
        "smart" => convert_to_smart(app, rest),
        // a Smart Folder's rules: the sheet with them filled in
        "editsmart" => {
            if let Some(f) = app.folder(rest) {
                let q = crate::smart::parse(f.smart.as_deref().unwrap_or(""));
                let rules = if q.rules.is_empty() { vec![crate::smart::Rule::new(0)] } else { q.rules };
                app.dialog = Some(Dialog::SmartFolder { name: f.name.clone(), smart: true, any: q.any, rules, convert: Some(rest.to_string()) });
            }
        }
        "view" => {
            // "view:<place key>|<what>"
            let (pk, what) = rest.split_once('|').unwrap_or(("", rest));
            let place = place_from_key(app, pk);
            let v = app.view_mut_of(&place);
            match what {
                "sort-default" => v.sort = None,
                "sort-edited" => v.sort = Some(0),
                "sort-created" => v.sort = Some(1),
                "sort-title" => v.sort = Some(2),
                "newest" => v.oldest_first = false,
                "oldest" => v.oldest_first = true,
                "group-default" => v.group = None,
                "group-on" => v.group = Some(true),
                "group-off" => v.group = Some(false),
                _ => {}
            }
        }
        "editor" => app.editor_cmds.push(rest.to_string()),
        _ => {}
    }
    app.select_first();
}

fn place_key(p: &Place) -> String {
    match p {
        Place::All(a) => format!("all={a}"),
        Place::Folder(f) => format!("folder={f}"),
        Place::Tag(t) => format!("tag={t}"),
        Place::Deleted(a) => format!("deleted={a}"),
        Place::Shared => "shared=".into(),
    }
}

fn place_from_key(app: &App, k: &str) -> Place {
    match k.split_once('=') {
        Some(("all", a)) => Place::All(a.into()),
        Some(("folder", f)) => Place::Folder(f.into()),
        Some(("tag", t)) => Place::Tag(t.into()),
        Some(("deleted", a)) => Place::Deleted(a.into()),
        Some(("shared", _)) => Place::Shared,
        _ => app.place.clone(),
    }
}

/// Sort By ▸ and Group By Date ▸ for `place` (ctx-folder.txt, ctx-toolbar-view-options.txt).
fn sort_group(app: &App, place: &Place) -> Vec<MItem> {
    let v = app.view_of(place);
    let k = place_key(place);
    let a = |w: &str| format!("view:{k}|{w}");
    vec![
        MItem::sub(
            "Sort By",
            vec![
                MItem::check(format!("Default ({})", ["Date Edited", "Date Created", "Title"][app.settings.sort.min(2) as usize]), a("sort-default"), v.sort.is_none()),
                MItem::check("Date Edited", a("sort-edited"), v.sort == Some(0)),
                MItem::check("Date Created", a("sort-created"), v.sort == Some(1)),
                MItem::check("Title", a("sort-title"), v.sort == Some(2)),
                MItem::Sep,
                MItem::check("Newest First", a("newest"), !v.oldest_first),
                MItem::check("Oldest First", a("oldest"), v.oldest_first),
            ],
        ),
        MItem::sub(
            "Group By Date",
            vec![
                MItem::check(format!("Default ({})", if app.settings.group_by_date { "On" } else { "Off" }), a("group-default"), v.group.is_none()),
                MItem::check("On", a("group-on"), v.group == Some(true)),
                MItem::check("Off", a("group-off"), v.group == Some(false)),
            ],
        ),
    ]
}

/// Convert to Smart Folder: the sheet, as the Mac's, unless the folder can't be (the Mac's
/// "Can't Convert Folder": it has subfolders, or locked notes).
fn convert_to_smart(app: &mut App, f: &str) {
    let Some(folder) = app.folder(f) else { return };
    let name = folder.name.clone();
    let why = if app.library.folders.iter().any(|x| x.parent.as_deref() == Some(f)) {
        Some("Folders with subfolders can’t be turned into Smart Folders.")
    } else if app.library.notes.iter().any(|n| n.folder == f && n.locked && !n.deleted) {
        Some("Folders containing shared or locked notes can’t be turned into Smart Folders.")
    } else {
        None
    };
    if let Some(why) = why {
        app.dialog = Some(Dialog::Alert("Can’t Convert Folder".into(), why.into()));
        return;
    }
    let rule = crate::smart::Rule::Tags { any_tag: false, included: vec![name.to_lowercase().replace(' ', "")], excluded: Vec::new(), all: true };
    app.dialog = Some(Dialog::SmartFolder { name, smart: true, any: false, rules: vec![rule], convert: Some(f.to_string()) });
}

/// Move to ▸: accounts ▸ their folders, the note's own folder and Smart Folders greyed.
pub(crate) fn move_to(app: &App, id: &str) -> MItem {
    let cur = app.note_by_id(id).map(|n| n.folder.clone()).unwrap_or_default();
    let mut accts = Vec::new();
    for a in &app.library.accounts {
        let mut kids = Vec::new();
        fn walk(app: &App, acct: &str, parent: Option<&str>, depth: usize, id: &str, cur: &str, out: &mut Vec<MItem>) {
            let mut level: Vec<_> = app.library.folders.iter().filter(|f| f.account == acct && f.parent.as_deref() == parent && f.kind != nt_core::model::FolderKind::Trash).collect();
            level.sort_by_key(|f| (if f.name == "Quick Notes" { 0 } else if f.name == "Notes" { 1 } else { 2 }, f.name.to_lowercase()));
            for f in level {
                let ok = f.id != cur && !App::is_smart(f);
                out.push(MItem::act(format!("{}{}", "    ".repeat(depth), f.name), format!("move:{id}|{}", f.id), ok));
                walk(app, acct, Some(&f.id), depth + 1, id, cur, out);
            }
        }
        walk(app, &a.id, None, 0, id, &cur, &mut kids);
        accts.push(MItem::sub(&crate::app::account_label(a), kids));
    }
    MItem::sub("Move to", accts)
}

/// A note's right-click menu in the list (ctx-note-in-list.txt).
pub fn note_menu(app: &App, id: &str) -> Vec<MItem> {
    let Some(n) = app.note_by_id(id) else { return Vec::new() };
    if n.deleted {
        return vec![move_to(app, id), MItem::Sep, MItem::act("Delete", format!("delete:{id}"), true)];
    }
    let tags = app.all_tags();
    let has = App::tags_of(n);
    let tag_items: Vec<MItem> = if tags.is_empty() { vec![MItem::act("No Tags", "", false)] } else { tags.iter().map(|t| MItem::check(format!("#{t}"), format!("tag:{id}|{t}"), has.iter().any(|h| h.eq_ignore_ascii_case(t)))).collect() };
    vec![
        MItem::act("Open Note in New Window", format!("window:{id}"), app.note_by_id(id).is_some_and(|n| !n.locked)),
        MItem::Sep,
        MItem::act(if n.pinned { "Unpin Note" } else { "Pin Note" }, format!("pin:{id}"), true),
        MItem::act(if n.locked { "Remove Lock" } else { "Lock Note" }, format!("lock:{id}"), true),
        MItem::Sep,
        MItem::cmd(app, "New Note", "New Note"),
        MItem::act("Duplicate Note", format!("dup:{id}"), true),
        MItem::Sep,
        MItem::act("Share Note", "", false),
        MItem::Sep,
        MItem::sub("Tags", tag_items),
        MItem::Sep,
        move_to(app, id),
        MItem::Sep,
        MItem::act("Delete", format!("delete:{id}"), true),
    ]
}

/// A folder's right-click menu (ctx-folder.txt).
pub fn folder_menu(app: &App, place: &Place, folder: Option<&str>, system: bool) -> Vec<MItem> {
    let mut out = Vec::new();
    match (place, folder) {
        (Place::Folder(_), Some(f)) => {
            out.push(MItem::act("Rename Folder", format!("rename:{f}"), !system));
            out.push(MItem::act("Delete Folder", format!("delfolder:{f}"), !system));
            out.push(MItem::Sep);
            out.push(MItem::act("New Folder", format!("newfolder:{f}"), app.folder(f).is_some_and(|x| !App::is_smart(x))));
            out.push(MItem::Sep);
            out.extend(sort_group(app, place));
            if app.folder(f).is_some_and(|x| !App::is_smart(x)) {
                out.push(MItem::Sep);
                out.push(MItem::act("Convert to Smart Folder", format!("smart:{f}"), !system));
            } else {
                out.insert(0, MItem::act("Edit Smart Folder", format!("editsmart:{f}"), true));
            }
        }
        _ => {
            out.push(MItem::cmd(app, "New Folder", "New Folder"));
            out.push(MItem::Sep);
            out.extend(sort_group(app, place));
        }
    }
    out
}

/// The toolbar's … (Actions) menu (ctx-toolbar-more.txt).
pub fn more_menu(app: &App) -> Vec<MItem> {
    let Some(n) = app.selected_note() else { return Vec::new() };
    let id = n.id.clone();
    let mut recent: Vec<MItem> = vec![MItem::cmd(app, "Previous Note", "Recent Notes > Previous Note"), MItem::act("Next Note", "", false), MItem::Sep];
    for r in &app.recent {
        if let Some(rn) = app.note_by_id(r) {
            recent.push(MItem::act(if rn.title.is_empty() { "New Note".to_string() } else { rn.title.clone() }, format!("note:{r}"), true));
        }
    }
    recent.push(MItem::Sep);
    recent.push(MItem::cmd(app, "Clear Menu", "Recent Notes > Clear Menu"));
    let find = MItem::Cmd { label: "Find in Note".into(), action: "cmd:Find > Find…".into(), enabled: enabled(app, "Find > Find…"), checked: false, glyph: Some(crate::icons::Glyph::Search) };
    vec![
        MItem::act(if n.pinned { "Unpin Note" } else { "Pin Note" }, format!("pin:{id}"), true),
        MItem::act(if n.locked { "Remove Lock" } else { "Lock Note" }, format!("lock:{id}"), true),
        MItem::Sep,
        find,
        move_to(app, &id),
        MItem::sub("Recent Notes", recent),
        MItem::sub("Math Results", vec![MItem::cmd(app, "Insert Results", "Math Results > Insert Results"), MItem::cmd(app, "Suggest Results", "Math Results > Suggest Results"), MItem::cmd(app, "Off", "Math Results > Off")]),
        MItem::sub("Attachment View", vec![MItem::cmd(app, "Set All to Small", "Attachment View > Set All to Small"), MItem::cmd(app, "Set All to Large", "Attachment View > Set All to Large")]),
        MItem::act("Delete Note", format!("delete:{id}"), true),
    ]
}

/// The list's View Options (…) menu (ctx-toolbar-view-options.txt, popover-view-options.png).
pub fn view_options_menu(app: &App) -> Vec<MItem> {
    let mut out = vec![
        MItem::Cmd { label: if app.gallery { "View as List".into() } else { "View as Gallery".into() }, action: if app.gallery { "cmd:as List".into() } else { "cmd:as Gallery".into() }, enabled: true, checked: false, glyph: Some(if app.gallery { crate::icons::Glyph::List } else { crate::icons::Glyph::Gallery }) },
        MItem::Sep,
    ];
    out.extend(sort_group(app, &app.place));
    out.push(MItem::act("View Attachments", "cmd:Show Attachments Browser", true));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// The menus' parity list (cargo test menu_audit -- --nocapture): every command, and
    /// whether it works here (shell or editor) or is greyed.
    fn menu_audit() {
        fn walk(items: &[Item], path: &str, parent: &str) {
            for it in items.iter().filter(|i| !i.separator) {
                let here = if path.is_empty() { it.label.clone() } else { format!("{path} > {}", it.label) };
                if it.children.is_empty() {
                    let name = command_name(parent, &it.label);
                    let status = if it.heading { "heading" } else if editor_command(&name) { "editor" } else if IMPLEMENTED.contains(&name.as_str()) { "shell" } else { "GREYED" };
                    println!("{status}\t{here}\t{name}");
                } else {
                    walk(&it.children, &here, &it.label);
                }
            }
        }
        walk(menus(), "", "");
    }

    #[test]
    fn tree_parses() {
        let m = menus();
        let names: Vec<&str> = m.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(names, ["Notes", "File", "Edit", "Format", "View", "Window", "Help"]);
        let format = m.iter().find(|i| i.label == "Format").unwrap();
        let title = format.children.iter().find(|c| c.label == "Title").unwrap();
        assert_eq!(title.shortcut.as_deref(), Some("Ctrl+Shift+T"));
        assert!(keyboard("Ctrl+Shift+7").is_some());
        assert!(keyboard("Ctrl+'").is_some());
        // Services and Writing Tools are gone
        assert!(!m.iter().any(|x| x.children.iter().any(|c| c.label == "Services" || c.label == "Writing Tools")));
    }

    #[test]
    fn every_menu_command_is_known() {
        fn walk(items: &[Item], parent: &str, out: &mut Vec<String>) {
            for it in items {
                if it.separator || it.heading {
                    continue;
                }
                if it.children.is_empty() {
                    out.push(command_name(parent, &it.label));
                } else if it.label != "Recent Notes" {
                    walk(&it.children, &it.label, out);
                }
            }
        }
        let mut all = Vec::new();
        for m in menus() {
            walk(&m.children, &m.label, &mut all);
        }
        let missing: Vec<&String> = all.iter().filter(|n| !implemented(n)).collect();
        eprintln!("greyed (not implemented): {missing:?}");
        assert!(all.len() > 120, "{}", all.len());
        assert!(missing.len() < 12, "{missing:?}");
    }
}

/// Undo and Redo are the library's (not the note's text) while the note isn't being edited.
fn library_undo(app: &App) -> bool {
    app.focus != Focus::Editor && !app.editor_state.focused
}

/// The notes a row's menu acts on: the selection when the row is part of it, else that note.
fn targets(app: &App, id: &str) -> Vec<String> {
    let sel = app.selection();
    if sel.len() > 1 && sel.iter().any(|s| s == id) { sel } else { vec![id.to_string()] }
}

/// Help > Keyboard Shortcuts: the note list's keys, then every menu command's shortcut (Ctrl for
/// the Mac's ⌘), as the menus have them.
pub fn shortcut_list(ctx: &egui::Context) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = [
        ("↑ ↓", "Previous / next note"),
        ("Shift+↑ ↓", "Extend the selection"),
        ("Home / End", "First / last note"),
        ("Page Up / Page Down", "A page up / down"),
        ("Shift+click", "Select the notes in between"),
        ("Ctrl+click", "Add a note to the selection, or take it out"),
        ("Ctrl+A (in the list)", "Select every note in the list"),
        ("Delete / Backspace", "Delete the selected notes"),
        ("Ctrl+Z / Ctrl+Shift+Z", "Outside the note: undo / redo (delete, move, pin, folders)"),
        ("Tab", "From the sidebar to the list to the note"),
        ("Enter (gallery)", "Open the selected card"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let mut seen = std::collections::HashSet::new();
    for (k, name) in shortcut_table() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let label = name.rsplit(" > ").next().unwrap_or(name).trim_end_matches('…').to_string();
        out.push((ctx.format_shortcut(k), label));
    }
    out
}
