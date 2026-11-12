//! Screensets as Logic's: numbered window layouts (1-9 with the digit keys, 11-99 with ⇧ and
//! two digits) recalled in a keystroke. An unlocked screenset keeps every change; a locked one
//! comes back as it was locked. Duplicate…, Rename…, Delete, Lock and Revert to Saved (the
//! Window menu), Next / Previous. The project's own screensets come from its DisplayState.plist
//! (lg_core::displaystate): their numbers, names, locks and panes.

use std::path::{Path, PathBuf};

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
    /// The layout text last written (autosave), and when the layout was last checked.
    written: String,
    checked: Option<std::time::Instant>,
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
    let first = app.screens.loaded_for.is_none();
    app.screens.loaded_for = Some(file.clone());
    app.screens.sets.clear();
    app.screens.tens = None;
    let now = layout(app);
    // Studio's own memory of the layout wins over the file's (it's newer); with no project
    // file, only at startup (the last layout)
    if (!app.automated || std::env::var_os("LG_LAYOUT").is_some()) && (file.is_some() || first) {
        if let Some((sets, cur)) = store_path(file.as_deref()).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| parse_store(&t, now)) {
            app.screens.sets = sets;
            app.screens.current = cur;
            app.screens.saved = app.screens.sets.clone();
            if let Some(l) = app.screens.sets.iter().find(|s| s.slot == cur).map(|s| s.layout) {
                apply(app, l);
            }
            return;
        }
    }
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

/// The command names run here (build.rs and tool/checklist.py read this list).
#[allow(dead_code)]
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

// ---- the layout remembered (Logic keeps it in the project's DisplayState) --------------------

/// A new project's layout, as Logic's: the Tracks area with the Browser and the inspector,
/// no editor or right pane open.
pub fn new_layout(l: Layout) -> Layout {
    Layout { browser: true, inspector: true, bottom: Bottom::None, right_pane: None, scroll_x: 0.0, scroll_y: 0.0, ..l }
}

/// A new, empty project: Logic's starting layout, one screenset.
pub fn new_project(app: &mut App) {
    let l = new_layout(layout(app));
    apply(app, l);
    app.screens.sets = vec![Set { slot: 1, name: String::new(), locked: false, layout: l }];
    app.screens.current = 1;
    app.screens.saved = app.screens.sets.clone();
    app.screens.loaded_for = Some(app.file.as_ref().map(|f| f.0.clone()));
}

/// Where a project's layout is kept (no file: the last layout, for the next start).
fn store_path(file: Option<&Path>) -> Option<PathBuf> {
    match file {
        Some(f) => {
            let side = crate::hosting::sidecar_path(f)?;
            Some(side.parent()?.parent()?.join("layouts").join(side.file_name()?))
        }
        None => {
            let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
            Some(base.join("bramble-studio").join("layouts").join("last.txt"))
        }
    }
}

fn bottom_n(b: Bottom) -> u8 {
    match b {
        Bottom::None => 0,
        Bottom::PianoRoll => 1,
        Bottom::Mixer => 2,
        Bottom::SmartControls => 3,
    }
}

fn pane_n(p: Option<RightPane>) -> i8 {
    match p {
        None => -1,
        Some(RightPane::Lists) => 0,
        Some(RightPane::Notes) => 1,
        Some(RightPane::Loops) => 2,
        Some(RightPane::Files) => 3,
    }
}

pub fn store_text(sets: &[Set], current: u32) -> String {
    let mut t = format!("# Bramble Studio: window layout (screensets)\ncurrent\t{current}\n");
    for s in sets {
        let l = &s.layout;
        let b = |v: bool| if v { 1 } else { 0 };
        t += &format!(
            "set\t{}\t{}\t{}\t{} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
            s.slot, b(s.locked), s.name.replace(['\t', '\n'], " "),
            b(l.browser), b(l.inspector), b(l.control_bar), b(l.toolbar), bottom_n(l.bottom), pane_n(l.right_pane), l.list_tab,
            l.browser_w, l.header_w, l.editor_h, l.mixer_h, l.bar_w, l.scroll_x, l.scroll_y, l.v_zoom, b(l.global_tracks), b(l.automation)
        );
    }
    t
}

/// The remembered screensets (`base` fills nothing missing: every field is written).
pub fn parse_store(t: &str, base: Layout) -> Option<(Vec<Set>, u32)> {
    let mut sets = Vec::new();
    let mut current = 0;
    for line in t.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split('\t').collect();
        match w.as_slice() {
            ["current", n] => current = n.parse().ok()?,
            ["set", slot, locked, name, f] => {
                let f: Vec<f32> = f.split(' ').filter_map(|x| x.parse().ok()).collect();
                if f.len() != 17 {
                    continue;
                }
                let on = |i: usize| f[i] != 0.0;
                let layout = Layout {
                    browser: on(0),
                    inspector: on(1),
                    control_bar: on(2),
                    toolbar: on(3),
                    bottom: [Bottom::None, Bottom::PianoRoll, Bottom::Mixer, Bottom::SmartControls].get(f[4] as usize).copied().unwrap_or(base.bottom),
                    right_pane: [RightPane::Lists, RightPane::Notes, RightPane::Loops, RightPane::Files].get(f[5] as i32 as usize).copied().filter(|_| f[5] >= 0.0),
                    list_tab: f[6] as usize,
                    browser_w: f[7],
                    header_w: f[8],
                    editor_h: f[9],
                    mixer_h: f[10],
                    bar_w: f[11],
                    scroll_x: f[12],
                    scroll_y: f[13],
                    v_zoom: f[14],
                    global_tracks: on(15),
                    automation: on(16),
                };
                sets.push(Set { slot: slot.parse().ok()?, name: name.to_string(), locked: *locked == "1", layout });
            }
            _ => {}
        }
    }
    if sets.is_empty() {
        return None;
    }
    if !sets.iter().any(|s| s.slot == current) {
        current = sets[0].slot;
    }
    Some((sets, current))
}

/// Keeps the layout (at Save, and at quit): the current screenset takes what's shown unless
/// it's locked.
/// Every couple of seconds: a changed layout is written straight away, so it comes back even
/// when Studio didn't quit normally.
pub fn autosave(app: &mut App) {
    if app.automated || app.screens.loaded_for.is_none() {
        return;
    }
    if app.screens.checked.is_some_and(|t| t.elapsed().as_secs_f32() < 2.0) {
        return;
    }
    app.screens.checked = Some(std::time::Instant::now());
    let mut sets = app.screens.sets.clone();
    let now = layout(app);
    let cur = app.screens.current;
    if let Some(c) = sets.iter_mut().find(|s| s.slot == cur && !s.locked) {
        c.layout = now;
    }
    if store_text(&sets, cur) != app.screens.written {
        remember(app);
    }
}

pub fn remember(app: &mut App) {
    if app.automated {
        return;
    }
    ensure(app);
    let now = layout(app);
    let cur = app.screens.current;
    if let Some(c) = app.screens.sets.iter_mut().find(|s| s.slot == cur) {
        if !c.locked {
            c.layout = now;
        }
    }
    let Some(path) = store_path(app.file.as_ref().map(|f| f.0.as_path())) else { return };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let text = store_text(&app.screens.sets, cur);
    let _ = std::fs::write(path, &text);
    app.screens.written = text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_layout_round_trips() {
        let l = Layout { browser: false, inspector: true, control_bar: true, toolbar: false, bottom: Bottom::Mixer, right_pane: Some(RightPane::Notes), list_tab: 2, browser_w: 300.5, header_w: 220.0, editor_h: 310.0, mixer_h: 400.0, bar_w: 64.0, scroll_x: 12.5, scroll_y: 40.0, v_zoom: 1.5, global_tracks: true, automation: false };
        let sets = vec![Set { slot: 1, name: "Mix".into(), locked: false, layout: l }, Set { slot: 3, name: "Edit".into(), locked: true, layout: new_layout(l) }];
        let (back, cur) = parse_store(&store_text(&sets, 3), l).unwrap();
        assert_eq!(back, sets);
        assert_eq!(cur, 3);
        assert_eq!(back[1].layout.bottom, Bottom::None);
        assert_eq!(back[1].layout.right_pane, None);
    }

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
