//! The menus as data, for front ends that draw them (ffi.rs): the menu bar as the desktop's
//! (labels and ticks following the state, what's greyed out), the right-click menus of the
//! slide and the navigator, and the toolbar popovers that are lists.

use serde_json::{Value, json};

use super::{Item, checked, command_name, enabled, menus, shown_label};
use crate::app::App;

/// The menu bar: [{label, command, enabled, checked, shortcut} | {separator} | {label, kids}].
pub fn as_data(app: &App) -> Value {
    fn item(app: &App, parent: &str, it: &Item) -> Value {
        if it.separator {
            return json!({ "separator": true });
        }
        if it.label == "Open Recent" {
            let kids: Vec<Value> = App::recent_files().iter().map(|p| json!({ "label": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), "command": format!("open:{}", p.display()), "enabled": true })).collect();
            return json!({ "label": "Open Recent", "kids": kids, "enabled": !kids.is_empty() });
        }
        if !it.children.is_empty() {
            let kids: Vec<Value> = it.children.iter().map(|c| item(app, &it.label, c)).collect();
            return json!({ "label": it.label, "kids": kids, "enabled": true });
        }
        let name = command_name(parent, &it.label);
        match shown_label(app, &it.label) {
            Some(l) => json!({ "label": l, "command": l, "enabled": true, "shortcut": it.shortcut }),
            None => json!({ "label": it.label, "command": name, "enabled": enabled(app, &name), "checked": checked(app, &it.label) == Some(true), "shortcut": it.shortcut }),
        }
    }
    Value::Array(menus().iter().map(|m| item(app, "", m)).collect())
}

/// A menu item running a command (`label` shown, `command` run with lt_run's "cmd:").
fn cmd(app: &App, label: &str, command: &str) -> Value {
    json!({ "label": label, "action": format!("cmd:{command}"), "enabled": enabled(app, command) })
}

fn sep() -> Value {
    json!({ "separator": true })
}

/// The slide's right-click menu (canvas.rs context_menu) for what's selected now.
pub fn context_items(app: &mut App) -> Vec<Value> {
    // the text being edited: the clipboard and the selection
    if app.canvas.editing.is_some() {
        return vec![cmd(app, "Cut", "Cut"), cmd(app, "Copy", "Copy"), cmd(app, "Paste", "Paste"), cmd(app, "Select All", "Select All"), sep(), cmd(app, "Add Link…", "Add Link"), cmd(app, "Comment", "Comment")];
    }
    if app.sel.is_empty() {
        return vec![cmd(app, "Paste", "Paste"), cmd(app, "Select All", "Select All"), sep(), cmd(app, "New Slide", "New Slide"), cmd(app, "Reapply Layout to Slide", "Reapply Layout to Slide")];
    }
    let mut out: Vec<Value> = ["Cut", "Copy", "Paste", "Delete"].iter().map(|l| cmd(app, l, l)).collect();
    out.push(cmd(app, "Duplicate", "Duplicate Selection"));
    out.push(sep());
    for l in ["Bring to Front", "Bring Forward", "Send Backward", "Send to Back"] {
        out.push(cmd(app, l, l));
    }
    out.push(sep());
    let locked = app.scene(app.cur).is_some_and(|sc| app.sel.iter().any(|id| sc.drawables.iter().any(|d| d.id == *id && d.locked)));
    let l = if locked { "Unlock" } else { "Lock" };
    out.push(cmd(app, l, l));
    let g = if app.sel.len() >= 2 { "Group" } else { "Ungroup" };
    out.push(cmd(app, g, g));
    out.push(sep());
    out.push(cmd(app, "Comment", "Comment"));
    out.push(cmd(app, "Add Link…", "Add Link"));
    out.push(sep());
    for l in ["Copy Style", "Paste Style", "Copy Animation", "Paste Animation"] {
        out.push(cmd(app, l, l));
    }
    out
}

/// A slide's right-click menu in the navigator (navigator.rs).
pub fn slide_items(app: &App, skipped: bool) -> Vec<Value> {
    let skip = if skipped { "Don't Skip Slide" } else { "Skip Slide" };
    vec![
        cmd(app, "Cut", "Cut"),
        cmd(app, "Copy", "Copy"),
        cmd(app, "Paste", "Paste"),
        sep(),
        cmd(app, "New Slide", "New Slide"),
        cmd(app, "Duplicate", "Duplicate Slide"),
        cmd(app, "Delete", "Delete Slide"),
        sep(),
        cmd(app, skip, skip),
        sep(),
        json!({ "label": "Play from This Slide", "action": "play", "enabled": true }),
    ]
}

/// The View popover (menus.rs view_dropdown): the views, then what shows.
pub fn view_items(app: &App) -> Vec<Value> {
    let mut out = Vec::new();
    for l in ["Navigator", "Slide Only", "Light Table", "Outline"] {
        out.push(json!({ "label": l, "action": format!("cmd:{l}"), "enabled": true, "checked": checked(app, l) == Some(true) }));
    }
    out.push(sep());
    for l in ["Show Rulers", "Show Comments", "Show Presenter Notes", "Show Object List", "Hide Guides"] {
        let shown = shown_label(app, l).unwrap_or(l);
        out.push(json!({ "label": shown, "action": format!("cmd:{shown}"), "enabled": enabled(app, l) || shown != l }));
    }
    out.push(sep());
    out.push(cmd(app, "Edit Slide Layouts", "Edit Slide Layouts"));
    out
}

/// The Media popover, as Letters' (Pages' Media ▾): the pickers are Android's ("media:…").
pub fn media_items() -> Vec<Value> {
    vec![
        json!({ "label": "Choose Photo or Video…", "action": "media:Choose Photo or Video…", "enabled": true }),
        json!({ "label": "Content Hub", "action": "cmd:Browse Content Hub", "enabled": true }),
        json!({ "label": "Choose Music…", "action": "media:Choose Music…", "enabled": true }),
        json!({ "label": "Add Image Gallery…", "action": "cmd:Image Gallery", "enabled": true }),
        json!({ "label": "Add Web Video…", "action": "cmd:Web Video…", "enabled": true }),
        json!({ "label": "Record Audio", "action": "cmd:Record Audio", "enabled": true }),
        sep(),
        json!({ "label": "Choose File…", "action": "media:Choose File…", "enabled": true }),
    ]
}

/// The menu command a Ctrl shortcut runs (as the menus show it: "Ctrl+Shift+Z"), if any.
pub fn shortcut_command(m: egui::Modifiers, key: egui::Key) -> Option<String> {
    let mut want = String::from("Ctrl+");
    if m.alt {
        want.push_str("Alt+");
    }
    if m.shift {
        want.push_str("Shift+");
    }
    want.push_str(match key {
        egui::Key::Equals | egui::Key::Plus => "=",
        egui::Key::Minus => "-",
        k => k.name(),
    });
    fn find(items: &[Item], parent: &str, want: &str) -> Option<String> {
        for it in items {
            if it.children.is_empty() {
                if it.shortcut.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(want)) {
                    return Some(command_name(parent, &it.label));
                }
            } else if let Some(c) = find(&it.children, &it.label, want) {
                return Some(c);
            }
        }
        None
    }
    menus().iter().find_map(|m| find(&m.children, &m.label, &want))
}
