//! Screensets as Logic's: numbered window layouts (1-9 with the digit keys, 11-99 with ⇧ and
//! two digits) recalled in a keystroke. An unlocked screenset keeps every change; a locked one
//! comes back as it was locked. Duplicate…, Rename…, Delete, Lock and Revert to Saved (the
//! Window menu), Next / Previous. The project's own screensets come from its DisplayState.plist
//! (lg_core::displaystate): their numbers, names, locks and panes.

use std::path::PathBuf;

use crate::app::{App, Bottom, RightPane};

/// What a screenset shows: the main window's panes, their sizes, the zoom and scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub browser: bool,
    pub inspector: bool,
    pub control_bar: bool,
    pub toolbar: bool,
    pub bottom: Bottom,
    pub right_pane: Option<RightPane>,
    pub list_tab: usize,
    pub browser_w: f32,
    pub header_w: f32,
    pub editor_h: f32,
    pub mixer_h: f32,
    pub bar_w: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub v_zoom: f32,
    pub global_tracks: bool,
    pub automation: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    pub slot: u32,
    pub name: String,
    pub locked: bool,
    pub layout: Layout,
}

/// A sheet open: Duplicate… (number, name) or Rename… (name).
#[derive(Clone, Debug, PartialEq)]
pub enum Sheet {
    Duplicate(String, String),
    Rename(String),
}

#[derive(Default)]
pub struct State {
    pub sets: Vec<Set>,
    pub current: u32,
    /// As the project was opened (Revert to Saved).
    saved: Vec<Set>,
    /// ⇧ and a digit: the tens of a two-digit screenset, waiting for the second digit.
    pub tens: Option<u32>,
    pub sheet: Option<Sheet>,
    /// The project file the screensets were read from.
    loaded_for: Option<Option<PathBuf>>,
}

/// Recalls `slot` in `sets` (pure, for the tests and the app): the current one keeps `now`
/// unless it's locked; a slot without a screenset gets one like `now`. Returns the layout to
/// show.
pub fn recall(sets: &mut Vec<Set>, current: u32, slot: u32, now: Layout) -> Layout {
    if let Some(c) = sets.iter_mut().find(|s| s.slot == current) {
        if !c.locked {
            c.layout = now;
        }
    }
    match sets.iter().find(|s| s.slot == slot) {
        Some(s) => s.layout,
        None => {
            sets.push(Set { slot, name: String::new(), locked: false, layout: now });
            sets.sort_by_key(|s| s.slot);
            now
        }
    }
}

/// The next (or previous) screenset's number, wrapping round.
pub fn neighbour(sets: &[Set], current: u32, next: bool) -> Option<u32> {
    let slots: Vec<u32> = sets.iter().map(|s| s.slot).collect();
    let i = slots.iter().position(|&s| s == current)?;
    let n = slots.len();
    Some(slots[if next { (i + 1) % n } else { (i + n - 1) % n }])
}

/// The first free number after `from` (Duplicate…'s suggestion).
pub fn free_slot(sets: &[Set], from: u32) -> u32 {
    (from + 1..100).chain(1..from).find(|n| n % 10 != 0 && !sets.iter().any(|s| s.slot == *n)).unwrap_or(from)
}

fn layout(app: &App) -> Layout {
    Layout {
        browser: app.show_browser,
        inspector: app.show_inspector,
        control_bar: app.show_control_bar,
        toolbar: app.show_toolbar,
        bottom: app.bottom,
        right_pane: app.right_pane,
        list_tab: app.list_tab,
        browser_w: app.browser_w,
        header_w: app.header_w,
        editor_h: app.editor_h,
        mixer_h: app.mixer_h,
        bar_w: app.bar_w,
        scroll_x: app.scroll_x,
        scroll_y: app.scroll_y,
        v_zoom: app.ta.v_zoom,
        global_tracks: app.global_tracks,
        automation: app.show_automation,
    }
}

fn apply(app: &mut App, l: Layout) {
    app.show_browser = l.browser;
    app.show_inspector = l.inspector;
    app.show_control_bar = l.control_bar;
    app.show_toolbar = l.toolbar;
    app.bottom = l.bottom;
    app.right_pane = l.right_pane;
    app.list_tab = l.list_tab;
    app.browser_w = l.browser_w;
    app.header_w = l.header_w;
    app.editor_h = l.editor_h;
    app.mixer_h = l.mixer_h;
    app.bar_w = l.bar_w;
    app.scroll_x = l.scroll_x;
    app.scroll_y = l.scroll_y;
    app.ta.v_zoom = l.v_zoom;
    app.global_tracks = l.global_tracks;
    app.show_automation = l.automation;
}

fn ensure(app: &mut App) {
    if app.screens.sets.is_empty() {
        let l = layout(app);
        app.screens.sets.push(Set { slot: 1, name: String::new(), locked: false, layout: l });
        app.screens.current = 1;
        app.screens.saved = app.screens.sets.clone();
    }
}

/// A project opened: its screensets (DisplayState.plist), the current one shown.
pub fn frame(app: &mut App) {
    let file = app.file.as_ref().map(|f| f.0.clone());
    if app.screens.loaded_for.as_ref() == Some(&file) {
        return;
    }
    app.screens.loaded_for = Some(file.clone());
    app.screens.sets.clear();
    app.screens.tens = None;
    let now = layout(app);
    let Some(ds) = file.as_deref().and_then(lg_core::displaystate::read) else {
        ensure(app);
        return;
    };
    // the panes' heights are percents of the main area (about 760 pt in Logic's default window)
    let main_h = 760.0;
    for s in &ds.screensets {
        use lg_core::displaystate::Editor;
        let mut l = now;
        l.browser = s.browser.unwrap_or(l.browser);
        l.inspector = s.inspector.unwrap_or(l.inspector);
        l.control_bar = s.control_bar.unwrap_or(l.control_bar);
        l.toolbar = s.toolbar.unwrap_or(l.toolbar);
        if s.lists == Some(true) {
            l.right_pane = Some(RightPane::Lists);
        } else if s.lists == Some(false) && l.right_pane == Some(RightPane::Lists) {
            l.right_pane = None;
        }
        if let Some(e) = s.editor {
            l.bottom = match e {
                Editor::None => Bottom::None,
                Editor::Mixer => Bottom::Mixer,
                Editor::SmartControls => Bottom::SmartControls,
                _ => Bottom::PianoRoll,
            };
        }
        if let Some(p) = s.mixer_percent.filter(|p| *p > 1.0 && *p < 100.0) {
            l.mixer_h = (p as f32 / 100.0 * main_h).max(260.0);
        }
        if let Some(p) = s.other_percent.filter(|p| *p > 1.0 && *p < 100.0) {
            l.editor_h = (p as f32 / 100.0 * main_h).max(120.0);
        }
        app.screens.sets.push(Set { slot: s.slot, name: s.name.clone(), locked: s.locked, layout: l });
    }
    if app.screens.sets.is_empty() {
        ensure(app);
        return;
    }
    app.screens.current = if app.screens.sets.iter().any(|s| s.slot == ds.current) { ds.current } else { app.screens.sets[0].slot };
    app.screens.saved = app.screens.sets.clone();
    // (checks keep Studio's own layout unless they ask for the project's)
    if !app.automated || std::env::var_os("LG_LAYOUT").is_some() {
        let cur = app.screens.current;
        if let Some(l) = app.screens.sets.iter().find(|s| s.slot == cur).map(|s| s.layout) {
            apply(app, l);
        }
    }
}

fn go(app: &mut App, slot: u32) {
    ensure(app);
    let now = layout(app);
    let cur = app.screens.current;
    if slot == cur {
        // the current one again: a locked screenset comes back as it was locked
        if let Some(s) = app.screens.sets.iter().find(|s| s.slot == slot && s.locked).map(|s| s.layout) {
            apply(app, s);
        }
        return;
    }
    let l = recall(&mut app.screens.sets, cur, slot, now);
    app.screens.current = slot;
    apply(app, l);
}

/// The screenset's menu name: its own, else "Screenset n (Tracks/…)" from what it shows.
pub fn title(s: &Set) -> String {
    if !s.name.is_empty() {
        return s.name.clone();
    }
    let l = &s.layout;
    let mut parts = vec!["Tracks"];
    if l.browser {
        parts.push("Browser");
    }
    match l.bottom {
        Bottom::Mixer => parts.push("Mixer"),
        Bottom::SmartControls => parts.push("Smart Controls"),
        Bottom::PianoRoll => parts.push("Editor"),
        Bottom::None => {}
    }
    format!("Screenset {} ({})", s.slot, parts.join("/"))
}

pub const COMMANDS: &[&str] = &[
    "Recall Screenset 1", "Recall Screenset 2", "Recall Screenset 3", "Recall Screenset 4", "Recall Screenset 5", "Recall Screenset 6", "Recall Screenset 7", "Recall Screenset 8", "Recall Screenset 9",
    "Recall Screenset 1x", "Recall Screenset 2x", "Recall Screenset 3x", "Recall Screenset 4x", "Recall Screenset 5x", "Recall Screenset 6x", "Recall Screenset 7x", "Recall Screenset 8x", "Recall Screenset 9x",
    "Duplicate Screenset…", "Rename Screenset…", "Delete Screenset", "Lock/Unlock Current Screenset", "Revert to Saved Screenset", "Next Screenset", "Previous Screenset",
    "Duplicate…", "Rename…", "Lock", "Revert to Saved",
];

pub fn command(app: &mut App, _ctx: &egui::Context, name: &str) -> bool {
    if let Some(n) = name.strip_prefix("Recall Screenset ") {
        ensure(app);
        if let Some(t) = n.strip_suffix('x') {
            app.screens.tens = t.parse().ok();
            app.tell(format!("Screenset {t}…"));
            return true;
        }
        let Ok(d) = n.parse::<u32>() else { return false };
        let slot = match app.screens.tens.take() {
            Some(t) => t * 10 + d,
            None => d,
        };
        go(app, slot);
        return true;
    }
    ensure(app);
    let cur = app.screens.current;
    let idx = app.screens.sets.iter().position(|s| s.slot == cur);
    match name {
        "Next Screenset" | "Previous Screenset" => {
            let Some(to) = neighbour(&app.screens.sets, cur, name.starts_with("Next")) else { return false };
            go(app, to);
        }
        "Duplicate Screenset…" | "Duplicate…" => {
            let n = free_slot(&app.screens.sets, cur);
            let base = idx.map(|i| app.screens.sets[i].name.clone()).unwrap_or_default();
            app.screens.sheet = Some(Sheet::Duplicate(n.to_string(), if base.is_empty() { String::new() } else { format!("{base} copy") }));
        }
        "Rename Screenset…" | "Rename…" => {
            let n = idx.map(|i| app.screens.sets[i].name.clone()).unwrap_or_default();
            app.screens.sheet = Some(Sheet::Rename(n));
        }
        "Delete Screenset" => {
            // (the last one stays: there's always a screenset)
            if app.screens.sets.len() < 2 {
                return false;
            }
            let Some(i) = idx else { return false };
            app.screens.sets.remove(i);
            let to = app.screens.sets[i.min(app.screens.sets.len() - 1)].slot;
            app.screens.current = to;
            let l = app.screens.sets.iter().find(|s| s.slot == to).map(|s| s.layout);
            if let Some(l) = l {
                apply(app, l);
            }
        }
        "Lock/Unlock Current Screenset" | "Lock" => {
            let now = layout(app);
            let Some(i) = idx else { return false };
            let s = &mut app.screens.sets[i];
            if !s.locked {
                s.layout = now;
            }
            s.locked = !s.locked;
        }
        "Revert to Saved Screenset" | "Revert to Saved" => {
            let Some(s) = app.screens.saved.iter().find(|s| s.slot == cur).cloned() else { return false };
            if let Some(i) = idx {
                app.screens.sets[i] = s.clone();
            }
            apply(app, s.layout);
        }
        _ => return false,
    }
    true
}

/// Duplicate (a number, a name) or rename the current screenset.
pub fn finish_sheet(app: &mut App, sheet: Sheet) -> Result<(), String> {
    let cur = app.screens.current;
    match sheet {
        Sheet::Rename(name) => {
            if let Some(s) = app.screens.sets.iter_mut().find(|s| s.slot == cur) {
                s.name = name.trim().to_string();
            }
        }
        Sheet::Duplicate(n, name) => {
            let slot: u32 = n.trim().parse().map_err(|_| "Type a number from 1 to 99".to_string())?;
            if !(1..100).contains(&slot) || slot % 10 == 0 {
                return Err("Screensets are numbered 1–9 and 11–99".into());
            }
            if app.screens.sets.iter().any(|s| s.slot == slot) {
                return Err(format!("Screenset {slot} exists already"));
            }
            let now = layout(app);
            if let Some(c) = app.screens.sets.iter_mut().find(|s| s.slot == cur && !s.locked) {
                c.layout = now;
            }
            app.screens.sets.push(Set { slot, name: name.trim().to_string(), locked: false, layout: now });
            app.screens.sets.sort_by_key(|s| s.slot);
            app.screens.current = slot;
        }
    }
    Ok(())
}

thread_local! {
    /// The Window menu is being drawn (its Delete is the screenset's, not the Edit menu's).
    pub static IN_WINDOW_MENU: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The Window menu's screenset part: the project's window, every screenset (✓ the current
/// one), Duplicate…, Rename…, Delete, Lock / Unlock, Revert to Saved.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if !IN_WINDOW_MENU.with(|c| c.get()) {
        return false;
    }
    let l = it.label.as_str();
    if l.ends_with(".logicx - Tracks") {
        ui.add_enabled(false, egui::Button::new(format!("✓ {} - Tracks", app.project_title())));
        return true;
    }
    if l.starts_with("Screenset ") {
        ensure(app);
        let sets = app.screens.sets.clone();
        for s in sets {
            let on = s.slot == app.screens.current;
            let key = if s.slot < 10 { s.slot.to_string() } else { format!("⇧{} {}", s.slot / 10, s.slot % 10) };
            let text = format!("{}{}{}", if on { "✓ " } else { "   " }, title(&s), if s.locked { " 🔒" } else { "" });
            if ui.add(egui::Button::new(text).shortcut_text(key)).clicked() {
                go(app, s.slot);
                ui.close();
            }
        }
        return true;
    }
    let cmd = match l {
        "Delete" => "Delete Screenset",
        "Lock" => "Lock/Unlock Current Screenset",
        "Duplicate…" => "Duplicate Screenset…",
        "Rename…" => "Rename Screenset…",
        "Revert to Saved" => "Revert to Saved Screenset",
        _ => return false,
    };
    ensure(app);
    let locked = app.screens.sets.iter().any(|s| s.slot == app.screens.current && s.locked);
    let shown = if l == "Lock" && locked { "Unlock" } else { l };
    let can = match l {
        "Delete" => app.screens.sets.len() > 1 && !locked,
        "Rename…" => !locked,
        "Revert to Saved" => app.screens.saved.iter().any(|s| s.slot == app.screens.current),
        _ => true,
    };
    if ui.add_enabled(can, egui::Button::new(shown)).clicked() {
        let ctx = ui.ctx().clone();
        command(app, &ctx, cmd);
        ui.close();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(x: f32) -> Layout {
        Layout { browser: true, inspector: true, control_bar: true, toolbar: false, bottom: Bottom::None, right_pane: None, list_tab: 0, browser_w: 300.0, header_w: 265.0, editor_h: 300.0, mixer_h: 420.0, bar_w: 70.0, scroll_x: x, scroll_y: 0.0, v_zoom: 1.0, global_tracks: false, automation: false }
    }

    #[test]
    fn recall_keeps_changes_unless_locked() {
        let mut sets = vec![Set { slot: 1, name: String::new(), locked: false, layout: l(0.0) }];
        // 2 doesn't exist: made from the window as it is
        assert_eq!(recall(&mut sets, 1, 2, l(5.0)), l(5.0));
        assert_eq!(sets.len(), 2);
        // back to 1: it kept what it showed when left (5.0, the change made in 1)
        assert_eq!(recall(&mut sets, 2, 1, l(7.0)), l(5.0));
        assert_eq!(sets[1].layout, l(7.0));
        // a locked one doesn't keep changes
        sets[0].locked = true;
        recall(&mut sets, 1, 2, l(9.0));
        assert_eq!(sets[0].layout, l(5.0));
    }

    #[test]
    fn numbers() {
        let sets: Vec<Set> = [1, 2, 5, 12].iter().map(|&n| Set { slot: n, name: String::new(), locked: false, layout: l(0.0) }).collect();
        assert_eq!(neighbour(&sets, 5, true), Some(12));
        assert_eq!(neighbour(&sets, 12, true), Some(1));
        assert_eq!(neighbour(&sets, 1, false), Some(12));
        assert_eq!(neighbour(&sets, 7, false), None);
        assert_eq!(free_slot(&sets, 2), 3);
        assert_eq!(free_slot(&sets, 9), 11);
        let mut s = Set { slot: 3, name: String::new(), locked: false, layout: l(0.0) };
        assert_eq!(title(&s), "Screenset 3 (Tracks/Browser)");
        s.layout.bottom = Bottom::Mixer;
        s.layout.browser = false;
        assert_eq!(title(&s), "Screenset 3 (Tracks/Mixer)");
        s.name = "Mix".into();
        assert_eq!(title(&s), "Mix");
    }
}
