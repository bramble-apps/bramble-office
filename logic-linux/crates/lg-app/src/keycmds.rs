//! The Key Commands window (Logic Pro ▸ Key Commands ▸ Edit Assignments…, ⌥K), as Logic's "Key
//! Command Assignments": the More menu (Presets, Import / Import to Selection / Merge, Save,
//! Save As…, Save Customized Key Commands As…, Copy Key Commands to Clipboard, Expand / Collapse
//! All, Scroll to Selection, Initialize All / Selection / Unused), the filter buttons (All, Used,
//! Key, Touch Bar, Unused, Customized, Multi-Used, Conflicts (n), Pressed), the search field with
//! its recent searches, the Command / Key / Touch Bar / Control Surface list grouped by area, and
//! the right pane: Keyboard (Key, Learn by Key Label, Learn by Key Position, Delete), Touch Bar
//! (no Touch Bar on Linux: dimmed) and Control Surface (Learn New Assignment from a MIDI
//! controller, Delete Assignment).
//!
//! Every edit goes straight into the app's key map (keymap::set_bindings) and is kept in
//! ~/.config/bramble-studio/key-commands/Current.bskc, so it survives a restart. Sets are saved in
//! Studio's own text format (.bskc: one "area ⇥ command ⇥ keys ⇥ controller messages" line per
//! command) in ~/.config/bramble-studio/key-commands/ (Logic keeps its .logikcs sets in
//! ~/Music/Audio Music Apps/Key Commands/; their binary blob isn't decoded, so they can't be read).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::keymap;
use crate::look;

pub const COMMANDS: &[&str] = &["Open Key Command Assignments…", "Edit Assignments…", "Key Commands", "U.S."];

/// The built-in set's name (Logic's default preset).
pub const US: &str = "U.S.";
const EXT: &str = "bskc";

#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    pub name: String,
    /// Per command (keymap::all_commands' order): its keys, its controller messages.
    pub keys: Vec<Vec<String>>,
    pub midi: Vec<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Import {
    /// Every command takes the file's assignment.
    All,
    /// Only the selected commands do.
    Selection,
    /// Only commands the file customized (differ from the U.S. set) do.
    Merge,
}

impl Set {
    pub fn us() -> Set {
        let all = keymap::all_commands();
        Set { name: US.into(), keys: all.iter().map(|c| if c.2.is_empty() { Vec::new() } else { vec![c.2.clone()] }).collect(), midi: vec![Vec::new(); all.len()] }
    }

    pub fn bindings(&self) -> Vec<keymap::Binding> {
        let all = keymap::all_commands();
        keymap::bindings_of(self.keys.iter().enumerate().flat_map(|(i, ks)| ks.iter().map(move |k| (all[i].0.as_str(), all[i].1.as_str(), k.as_str()))))
    }

    fn default_keys(i: usize) -> Vec<String> {
        let d = &keymap::all_commands()[i].2;
        if d.is_empty() { Vec::new() } else { vec![d.clone()] }
    }

    /// Differs from the U.S. set.
    pub fn customized(&self, i: usize) -> bool {
        self.keys[i] != Self::default_keys(i) || !self.midi[i].is_empty()
    }

    pub fn used(&self, i: usize) -> bool {
        !self.keys[i].is_empty() || !self.midi[i].is_empty()
    }

    /// The commands using `spec` in `area` (or anywhere, with None).
    pub fn users(&self, spec: &str, area: Option<&str>) -> Vec<usize> {
        let all = keymap::all_commands();
        (0..self.keys.len()).filter(|&i| self.keys[i].iter().any(|k| k == spec) && area.is_none_or(|a| all[i].0 == a)).collect()
    }

    /// The key a command shares with a command in the same group (Logic's Conflicts).
    pub fn conflicts(&self) -> HashSet<usize> {
        let all = keymap::all_commands();
        let mut by: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
        for (i, ks) in self.keys.iter().enumerate() {
            for k in ks {
                by.entry((all[i].0.as_str(), k.as_str())).or_default().push(i);
            }
        }
        by.into_values().filter(|v| v.len() > 1).flatten().collect()
    }

    /// Keys used by more than one command, in any groups (Logic's Multi-Used).
    pub fn multi_used(&self) -> HashSet<usize> {
        let mut by: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, ks) in self.keys.iter().enumerate() {
            for k in ks {
                by.entry(k.as_str()).or_default().push(i);
            }
        }
        by.into_values().filter(|v| v.len() > 1).flatten().collect()
    }

    pub fn to_text(&self, only_customized: bool) -> String {
        let all = keymap::all_commands();
        let mut s = format!("# Bramble Studio key commands\nset\t{}\nscope\t{}\n", self.name, if only_customized { "customized" } else { "all" });
        for (i, c) in all.iter().enumerate() {
            if only_customized && !self.customized(i) {
                continue;
            }
            s += &format!("{}\t{}\t{}\t{}\n", c.0, c.1, self.keys[i].join(" ; "), self.midi[i].join(" ; "));
        }
        s
    }

    /// A set read from a .bskc file. A customized-only file is completed with the U.S. set.
    /// Also returns which commands the file names.
    pub fn from_text(t: &str) -> Option<(Set, HashSet<usize>)> {
        let all = keymap::all_commands();
        let mut set = Set::us();
        let mut named = HashSet::new();
        let mut scope_all = true;
        let mut ok = false;
        for l in t.lines() {
            if l.starts_with('#') || l.trim().is_empty() {
                ok |= l.starts_with("# Bramble Studio key commands");
                continue;
            }
            let f: Vec<&str> = l.split('\t').collect();
            match f.as_slice() {
                ["set", n] => set.name = n.to_string(),
                ["scope", s] => scope_all = *s != "customized",
                [area, cmd, rest @ ..] => {
                    let Some(i) = all.iter().position(|c| c.0 == *area && c.1 == *cmd) else { continue };
                    let split = |s: Option<&&str>| s.map(|s| s.split(" ; ").map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()).unwrap_or_default();
                    set.keys[i] = split(rest.first());
                    set.midi[i] = split(rest.get(1));
                    named.insert(i);
                }
                _ => {}
            }
        }
        if !ok {
            return None;
        }
        if scope_all {
            // (a full set: commands it leaves out are unassigned)
            for i in 0..all.len() {
                if !named.contains(&i) {
                    set.keys[i].clear();
                    set.midi[i].clear();
                }
            }
        }
        Some((set, named))
    }

    /// Takes another set's assignments (Import Key Commands…, …to Selection…, Merge…).
    pub fn import(&mut self, other: &Set, how: Import, selection: &[usize]) {
        for i in 0..self.keys.len() {
            let take = match how {
                Import::All => true,
                Import::Selection => selection.contains(&i),
                Import::Merge => other.customized(i),
            };
            if take {
                self.keys[i] = other.keys[i].clone();
                self.midi[i] = other.midi[i].clone();
            }
        }
    }

    pub fn initialize(&mut self, which: &[usize]) {
        for &i in which {
            self.keys[i] = Self::default_keys(i);
            self.midi[i].clear();
        }
    }

    /// Initialize Unused: commands left without a key get their U.S. key back when no command in
    /// their group took it meanwhile; customized assignments stay.
    pub fn initialize_unused(&mut self) {
        let all = keymap::all_commands();
        for i in 0..self.keys.len() {
            let d = Self::default_keys(i);
            if self.keys[i].is_empty() && !d.is_empty() && self.users(&d[0], Some(&all[i].0)).is_empty() {
                self.keys[i] = d;
            }
        }
    }

    /// Assigns a key: Replace takes it from the commands in the group using it, Add (Logic's OK)
    /// leaves them as they are.
    pub fn assign(&mut self, i: usize, spec: &str, replace: bool) {
        if replace {
            let area = keymap::all_commands()[i].0.clone();
            for j in self.users(spec, Some(&area)) {
                self.keys[j].retain(|k| k != spec);
            }
        }
        if !self.keys[i].iter().any(|k| k == spec) {
            self.keys[i].push(spec.to_string());
        }
    }

    /// The tab-separated list Copy Key Commands to Clipboard makes (for a spreadsheet).
    pub fn clipboard_text(&self, rows: &[usize]) -> String {
        let all = keymap::all_commands();
        let mut s = String::from("Group\tCommand\tKey\tControl Surface\n");
        for &i in rows {
            s += &format!("{}\t{}\t{}\t{}\n", all[i].0, all[i].1, self.keys[i].join(", "), self.midi[i].join(", "));
        }
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Used,
    Key,
    TouchBar,
    Unused,
    Customized,
    MultiUsed,
    Conflicts,
    Pressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Learn {
    Label,
    Position,
    Midi,
}

/// A key asked for that's in use: (the command learning, the key, the commands using it).
#[derive(Clone, Debug)]
pub struct Conflict {
    pub cmd: usize,
    pub spec: String,
    pub users: Vec<usize>,
}

pub struct State {
    pub open: bool,
    pub set: Set,
    /// The file the set was loaded from or saved to (None: the built-in U.S. set).
    pub file: Option<PathBuf>,
    pub filter: Filter,
    pub search: String,
    pub recent: Vec<String>,
    pub selected: Vec<usize>,
    pub collapsed: HashSet<String>,
    pub learn: Option<Learn>,
    pub conflict: Option<Conflict>,
    /// Pressed: the key last pressed.
    pub pressed: Option<String>,
    scroll_to: bool,
    /// Asking a name for Save As… / Save Customized…: (name, customized only).
    save_as: Option<(String, bool)>,
}

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("key-commands"))
}

fn current_path() -> Option<PathBuf> {
    dir().map(|d| d.join("Current.state"))
}

/// The sets saved in the key commands folder (name, path).
pub fn presets() -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == EXT)).filter_map(|p| Some((p.file_stem()?.to_string_lossy().to_string(), p))).collect())
        .unwrap_or_default();
    v.sort();
    v
}

impl Default for State {
    fn default() -> State {
        State { open: false, set: Set::us(), file: None, filter: Filter::All, search: String::new(), recent: Vec::new(), selected: Vec::new(), collapsed: HashSet::new(), learn: None, conflict: None, pressed: None, scroll_to: false, save_as: None }
    }
}

impl State {
    /// The set in use at the last quit (and the file it came from); the U.S. set the first time.
    pub fn load() -> State {
        let mut s = State::default();
        if std::env::var_os("LG_SNAP").is_some() {
            return s;
        }
        if let Some(t) = current_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            let file = t.lines().find_map(|l| l.strip_prefix("file\t")).map(PathBuf::from);
            if let Some((set, _)) = Set::from_text(&t) {
                s.set = set;
                s.file = file;
                keymap::set_bindings(s.set.bindings());
            }
        }
        s
    }

    /// After every edit: the app's keys follow, and the set is kept for the next run.
    fn changed(&mut self) {
        keymap::set_bindings(self.set.bindings());
        if std::env::var_os("LG_SNAP").is_some() {
            return;
        }
        if let (Some(p), Some(d)) = (current_path(), dir()) {
            let _ = std::fs::create_dir_all(d);
            let mut t = self.set.to_text(false);
            if let Some(f) = &self.file {
                t += &format!("file\t{}\n", f.display());
            }
            let _ = std::fs::write(p, t);
        }
    }

    fn shown(&self) -> Vec<usize> {
        let all = keymap::all_commands();
        let conflicts = if self.filter == Filter::Conflicts { self.set.conflicts() } else { HashSet::new() };
        let multi = if self.filter == Filter::MultiUsed { self.set.multi_used() } else { HashSet::new() };
        let q = self.search.to_lowercase();
        (0..all.len())
            .filter(|&i| match self.filter {
                Filter::All => true,
                Filter::Used => self.set.used(i),
                Filter::Key => !self.set.keys[i].is_empty(),
                Filter::TouchBar => false,
                Filter::Unused => !self.set.used(i),
                Filter::Customized => self.set.customized(i),
                Filter::MultiUsed => multi.contains(&i),
                Filter::Conflicts => conflicts.contains(&i),
                Filter::Pressed => self.pressed.as_ref().is_some_and(|p| self.set.keys[i].contains(p)),
            })
            .filter(|&i| q.is_empty() || all[i].1.to_lowercase().contains(&q) || self.set.keys[i].iter().any(|k| k.to_lowercase() == q))
            .collect()
    }

    /// Learns a key for the selected command; a key in use asks first.
    fn learn_key(&mut self, spec: String) {
        let Some(&i) = self.selected.first() else { return };
        let area = keymap::all_commands()[i].0.clone();
        let mut users: Vec<usize> = self.set.users(&spec, Some(&area)).into_iter().filter(|&j| j != i).collect();
        // (a Global Command's key is in use in every group)
        if area != "Global Commands" {
            users.extend(self.set.users(&spec, Some("Global Commands")));
        } else {
            users.extend(self.set.users(&spec, None).into_iter().filter(|&j| j != i));
        }
        users.sort();
        users.dedup();
        if users.is_empty() {
            self.set.assign(i, &spec, false);
            self.changed();
        } else {
            self.conflict = Some(Conflict { cmd: i, spec, users });
        }
        self.learn = None;
    }

    pub fn open_with(&mut self, command: &str) {
        self.open = true;
        if let Some(i) = keymap::all_commands().iter().position(|c| c.1 == command || c.1.trim_end_matches('…') == command.trim_end_matches('…')) {
            self.selected = vec![i];
            self.collapsed.remove(&keymap::all_commands()[i].0);
            self.filter = Filter::All;
            self.search.clear();
            self.scroll_to = true;
        }
    }
}

/// Logic's shortcut for a menu item now: the key the edited set gives the command of that name.
pub fn shortcut_for(app: &App, label: &str) -> Option<String> {
    let all = keymap::all_commands();
    let i = all.iter().position(|c| c.1 == label)?;
    Some(app.keycmds.set.keys[i].first().cloned().unwrap_or_default())
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Open Key Command Assignments…" | "Edit Assignments…" => {
            let s = &mut app.keycmds;
            s.open = !s.open;
            s.learn = None;
        }
        US => {
            app.keycmds.set = Set::us();
            app.keycmds.file = None;
            app.keycmds.changed();
        }
        _ => return false,
    }
    true
}

/// The Presets submenus (Logic Pro ▸ Key Commands ▸ Presets ▸ and the More menu's): U.S. and the
/// saved sets, the loaded one ticked.
fn presets_menu(app: &mut App, ui: &mut egui::Ui) {
    let cur = app.keycmds.file.clone();
    if ui.button(format!("{} {US}", if cur.is_none() { "✓" } else { "  " })).clicked() {
        command(app, US);
        ui.close();
    }
    let list = presets();
    if !list.is_empty() {
        ui.separator();
    }
    for (name, p) in list {
        let on = cur.as_ref() == Some(&p);
        if ui.button(format!("{} {name}", if on { "✓" } else { "  " })).clicked() {
            load_file(app, &p, Import::All);
            app.keycmds.file = Some(p);
            app.keycmds.changed();
            ui.close();
        }
    }
}

pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if it.label == "Presets" && it.children.iter().any(|c| c.label.contains("U.S.")) {
        ui.menu_button("Presets", |ui| presets_menu(app, ui));
        return true;
    }
    false
}

fn load_file(app: &mut App, p: &std::path::Path, how: Import) {
    let Some((other, _)) = std::fs::read_to_string(p).ok().and_then(|t| Set::from_text(&t)) else {
        app.tell(format!("“{}” isn't a Bramble Studio key command set", p.display()));
        return;
    };
    let sel = app.keycmds.selected.clone();
    app.keycmds.set.import(&other, how, &sel);
    if how == Import::All {
        app.keycmds.set.name = other.name.clone();
    }
    app.keycmds.changed();
}

fn save_to(app: &mut App, path: PathBuf, only_customized: bool) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut set = app.keycmds.set.clone();
    if let Some(n) = path.file_stem() {
        set.name = n.to_string_lossy().to_string();
    }
    match std::fs::write(&path, set.to_text(only_customized)) {
        Ok(()) => {
            if !only_customized {
                app.keycmds.set.name = set.name;
                app.keycmds.file = Some(path);
                app.keycmds.changed();
            }
        }
        Err(e) => app.tell(format!("Couldn't save the key commands: {e}")),
    }
}

fn more(app: &mut App, ctx: &egui::Context, what: &str) {
    let pick = |title: &str| -> Option<PathBuf> {
        let mut d = kn_ui::files::FileDialog::new().set_title(title).add_filter("Bramble Studio Key Commands", &[EXT]);
        if let Some(dir) = dir() {
            let _ = std::fs::create_dir_all(&dir);
            d = d.set_directory(dir);
        }
        d.pick_file()
    };
    match what {
        "Import Key Commands…" | "Import Key Commands to Selection…" | "Merge Key Commands…" => {
            if app.automated {
                return;
            }
            let how = match what {
                "Import Key Commands…" => Import::All,
                "Merge Key Commands…" => Import::Merge,
                _ => Import::Selection,
            };
            if let Some(p) = pick(what.trim_end_matches('…')) {
                load_file(app, &p, how);
            }
        }
        "Save" => match app.keycmds.file.clone() {
            Some(p) => save_to(app, p, false),
            None => app.keycmds.save_as = Some(("My Key Commands".into(), false)),
        },
        "Save As…" => app.keycmds.save_as = Some((app.keycmds.set.name.clone(), false)),
        "Save Customized Key Commands As…" => app.keycmds.save_as = Some(("Customized Key Commands".into(), true)),
        "Copy Key Commands to Clipboard" => {
            // (while searching: only the matching commands)
            let rows = app.keycmds.shown();
            ctx.copy_text(app.keycmds.set.clipboard_text(&rows));
        }
        "Expand All" => app.keycmds.collapsed.clear(),
        "Collapse All" => app.keycmds.collapsed = keymap::all_commands().iter().map(|c| c.0.clone()).collect(),
        "Scroll to Selection" => app.keycmds.scroll_to = true,
        "Initialize all Key Commands" => {
            app.keycmds.set = Set { name: app.keycmds.set.name.clone(), ..Set::us() };
            app.keycmds.changed();
        }
        "Initialize Selection" => {
            let sel = app.keycmds.selected.clone();
            app.keycmds.set.initialize(&sel);
            app.keycmds.changed();
        }
        "Initialize Unused" => {
            app.keycmds.set.initialize_unused();
            app.keycmds.changed();
        }
        _ => {}
    }
}

/// Keys while the window is open: learning takes the next key (with any modifiers), Pressed lists
/// the commands on it, ↑ ↓ move the selection; the window keeps them from running commands.
pub fn key_press(app: &mut App, key: egui::Key, m: egui::Modifiers, physical: Option<egui::Key>) -> bool {
    let s = &mut app.keycmds;
    if !s.open {
        return false;
    }
    if s.conflict.is_some() || s.save_as.is_some() {
        return true;
    }
    if let Some(l) = s.learn {
        if key == egui::Key::Escape && m.is_none() {
            s.learn = None;
        } else if l != Learn::Midi {
            let k = if l == Learn::Position { physical.unwrap_or(key) } else { key };
            if let Some(spec) = keymap::spec_of(k, m) {
                s.learn_key(spec);
            }
        }
        return true;
    }
    if s.filter == Filter::Pressed {
        s.pressed = keymap::spec_of(key, m);
        return true;
    }
    if key == egui::Key::K && m.alt && !m.command && !m.shift {
        s.open = false;
        return true;
    }
    match key {
        egui::Key::ArrowDown | egui::Key::ArrowUp if m.is_none() => {
            let rows = s.shown();
            let cur = s.selected.first().and_then(|c| rows.iter().position(|r| r == c));
            let n = match (cur, key == egui::Key::ArrowDown) {
                (Some(k), true) => (k + 1).min(rows.len().saturating_sub(1)),
                (Some(k), false) => k.saturating_sub(1),
                (None, _) => 0,
            };
            if let Some(&r) = rows.get(n) {
                s.selected = vec![r];
                s.scroll_to = true;
            }
        }
        egui::Key::Escape => s.open = false,
        // (Delete removes the selected command's key, as the Delete button)
        egui::Key::Backspace | egui::Key::Delete if m.is_none() => {
            let sel = s.selected.clone();
            for i in sel {
                s.set.keys[i].clear();
            }
            s.changed();
        }
        _ => {}
    }
    true
}

/// A MIDI message in: learned while Learn New Assignment is on, else runs the command it's
/// assigned to. Button releases (note off, value 0) are ignored, as Logic's learn does.
pub fn midi_in(app: &mut App, ctx: &egui::Context, status: u8, a: u8, b: u8) -> bool {
    let kind = status & 0xF0;
    let msg = match kind {
        0x90 if b > 0 => format!("{status:02X} {a:02X}"),
        0xB0 if b > 0 => format!("{status:02X} {a:02X}"),
        0xC0 => format!("{status:02X} {a:02X}"),
        0x80 | 0x90 | 0xB0 => {
            // (the release of an assigned button: used up)
            let probe = format!("{:02X} {a:02X}", if kind == 0x80 { 0x90 | (status & 0x0F) } else { status });
            return app.keycmds.learn == Some(Learn::Midi) || app.keycmds.set.midi.iter().any(|m| m.contains(&probe));
        }
        _ => return false,
    };
    if app.keycmds.open && app.keycmds.learn == Some(Learn::Midi) {
        if let Some(&i) = app.keycmds.selected.first() {
            let s = &mut app.keycmds;
            for m in s.set.midi.iter_mut() {
                m.retain(|x| *x != msg);
            }
            s.set.midi[i].push(msg);
            s.learn = None;
            s.changed();
        }
        return true;
    }
    let all = keymap::all_commands();
    let hit: Vec<String> = app.keycmds.set.midi.iter().enumerate().filter(|(_, m)| m.contains(&msg)).map(|(i, _)| all[i].1.clone()).collect();
    if hit.is_empty() {
        return false;
    }
    for c in hit {
        if !app.key_command(ctx, &c) {
            app.edit_command(ctx, &c);
        }
    }
    true
}

/// "B0 07" → "MIDI Ch 1, Control 7" (the raw string: Studio has no controller scripts naming it).
pub fn describe_midi(m: &str) -> String {
    let b: Vec<u8> = m.split_whitespace().filter_map(|x| u8::from_str_radix(x, 16).ok()).collect();
    match b.as_slice() {
        [s, d] => {
            let ch = (s & 0x0F) + 1;
            match s & 0xF0 {
                0x90 => format!("{m}  (Ch {ch}, Note {})", crate::app::note_name(*d)),
                0xB0 => format!("{m}  (Ch {ch}, Control {d})"),
                0xC0 => format!("{m}  (Ch {ch}, Program {})", *d as u32 + 1),
                _ => m.to_string(),
            }
        }
        _ => m.to_string(),
    }
}

// ---- the window ---------------------------------------------------------------------------------

fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const W: f32 = 1000.0;
const H: f32 = 575.0;
const ROW: f32 = 19.0;

fn button(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, label: &str, on: bool, enabled: bool) -> bool {
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() });
    let fill = if on { c(0x3478f6) } else if resp.hovered() && enabled { c(0x4a4948) } else { c(0x3a3938) };
    p.rect_filled(r, 5.0, fill);
    p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(12.0), if enabled { Color32::WHITE } else { c(0x6a6968) });
    enabled && resp.clicked()
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.keycmds.open {
        return;
    }
    let mut open = true;
    let title = format!("Key Command Assignments – {}", app.keycmds.set.name);
    let screen = ctx.content_rect();
    let mut fired: Option<String> = None;
    egui::Window::new(title).id(egui::Id::unique("key-commands")).open(&mut open).resizable(false).collapsible(false).fixed_size(vec2(W, H)).default_pos(pos2(screen.center().x - W / 2.0, screen.min.y + 60.0)).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(W, H), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(r, 0.0, c(0x222120));
        let id = ui.scope_id();
        // the More button (⋯ ⌄)
        let mr = Rect::from_min_size(r.min + vec2(16.0, 15.0), vec2(39.0, 22.0));
        let mresp = ui.interact(mr, id.with("more"), Sense::click());
        p.rect_filled(mr, 5.0, if mresp.hovered() { c(0x4a4948) } else { c(0x3a3938) });
        crate::glyphs::paint(&p, Rect::from_center_size(mr.min + vec2(12.0, 11.0), vec2(16.0, 16.0)), crate::glyphs::ph::DOTS_THREE_CIRCLE, 15.0, Color32::WHITE);
        crate::glyphs::paint(&p, Rect::from_center_size(mr.min + vec2(29.0, 11.0), vec2(12.0, 12.0)), crate::glyphs::ph::CARET_DOWN, 10.0, Color32::WHITE);
        egui::Popup::menu(&mresp).show(|ui| {
            ui.set_min_width(260.0);
            ui.menu_button("Presets", |ui| presets_menu(app, ui));
            ui.separator();
            for item in ["Import Key Commands…", "Import Key Commands to Selection…", "Merge Key Commands…", "-", "Save", "Save As…", "Save Customized Key Commands As…", "-", "Copy Key Commands to Clipboard", "-", "Expand All", "Collapse All", "Scroll to Selection", "-", "Initialize all Key Commands", "Initialize Selection", "Initialize Unused"] {
                if item == "-" {
                    ui.separator();
                    continue;
                }
                let on = !matches!(item, "Import Key Commands to Selection…" | "Initialize Selection" | "Scroll to Selection") || !app.keycmds.selected.is_empty();
                if ui.add_enabled(on, egui::Button::new(item)).clicked() {
                    fired = Some(item.to_string());
                    ui.close();
                }
            }
        });
        // the filter buttons
        let n_conf = app.keycmds.set.conflicts().len();
        let conf_label = format!("Conflicts ({n_conf})");
        let filters: [(Filter, &str); 9] = [(Filter::All, "All"), (Filter::Used, "Used"), (Filter::Key, "Key"), (Filter::TouchBar, "Touch Bar"), (Filter::Unused, "Unused"), (Filter::Customized, "Customized"), (Filter::MultiUsed, "Multi-Used"), (Filter::Conflicts, conf_label.as_str()), (Filter::Pressed, "Pressed")];
        let mut x = r.min.x + 68.0;
        for (k, (f, label)) in filters.iter().enumerate() {
            let w = p.layout_no_wrap(label.to_string(), look::regular(12.0), Color32::WHITE).size().x + 20.0;
            // (All … Unused share one segmented control; the others stand alone)
            let gap = if k >= 5 { 12.0 } else { 0.0 };
            x += gap;
            let br = Rect::from_min_size(pos2(x, r.min.y + 15.0), vec2(w, 22.0));
            x += w;
            if button(ui, &p, br, id.with(("filter", k)), label, app.keycmds.filter == *f, true) {
                let s = &mut app.keycmds;
                s.filter = if s.filter == *f && *f == Filter::Pressed { Filter::All } else { *f };
                s.pressed = None;
            }
        }
        // the search field (its magnifying glass lists recent searches)
        let sr = Rect::from_min_size(pos2(r.max.x - 208.0, r.min.y + 14.0), vec2(192.0, 24.0));
        p.rect_filled(sr, 12.0, c(0x1b1a19));
        let gl = Rect::from_center_size(pos2(sr.min.x + 14.0, sr.center().y), vec2(14.0, 14.0));
        crate::glyphs::paint(&p, gl, crate::glyphs::ph::MAGNIFYING_GLASS, 13.0, c(0x9a9998));
        let glr = ui.interact(gl.expand(3.0), id.with("recent"), Sense::click());
        egui::Popup::menu(&glr).show(|ui| {
            ui.label(egui::RichText::new("Recent Searches").color(look::text_dim()));
            let recent = app.keycmds.recent.clone();
            for q in recent {
                if ui.button(&q).clicked() {
                    app.keycmds.search = q;
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Clear").clicked() {
                app.keycmds.recent.clear();
                ui.close();
            }
        });
        let te = ui.put(Rect::from_min_max(pos2(sr.min.x + 26.0, sr.min.y + 2.0), pos2(sr.max.x - 24.0, sr.max.y - 2.0)), egui::TextEdit::singleline(&mut app.keycmds.search).hint_text("Search Key Command").frame(egui::Frame::NONE).font(look::regular(12.0)));
        if te.lost_focus() {
            let q = app.keycmds.search.trim().to_string();
            if !q.is_empty() && !app.keycmds.recent.contains(&q) {
                app.keycmds.recent.insert(0, q);
                app.keycmds.recent.truncate(10);
            }
        }
        if !app.keycmds.search.is_empty() {
            let cr = Rect::from_center_size(pos2(sr.max.x - 12.0, sr.center().y), vec2(14.0, 14.0));
            crate::glyphs::paint(&p, cr, crate::glyphs::ph_fill::X_CIRCLE, 13.0, c(0x8a8988));
            if ui.interact(cr, id.with("clear"), Sense::click()).clicked() {
                app.keycmds.search.clear();
            }
        }
        // the list
        let list = Rect::from_min_max(pos2(r.min.x + 16.0, r.min.y + 54.0), pos2(r.min.x + 783.0, r.max.y - 36.0));
        p.rect_filled(list, 0.0, c(0x1f1e1d));
        p.rect_stroke(list, 0.0, Stroke::new(1.0, c(0x3b3a39)), egui::StrokeKind::Inside);
        let head = Rect::from_min_size(list.min, vec2(list.width(), 22.0));
        let cols = [(list.min.x + 15.0, "Command"), (list.min.x + 455.0, "Key"), (list.min.x + 548.0, "Touch Bar"), (list.min.x + 634.0, "Control Surface")];
        for (k, (cx, t)) in cols.iter().enumerate() {
            p.text(pos2(*cx, head.center().y), Align2::LEFT_CENTER, *t, look::semibold(12.0), Color32::WHITE);
            if k > 0 {
                p.vline(cx - 8.0, head.y_range(), Stroke::new(1.0, c(0x3b3a39)));
            }
        }
        p.hline(list.x_range(), head.max.y, Stroke::new(1.0, c(0x3b3a39)));
        let shown = app.keycmds.shown();
        let all = keymap::all_commands();
        // rows: group headers and their commands
        enum R {
            Group(String, usize),
            Cmd(usize),
        }
        let mut rows: Vec<R> = Vec::new();
        let mut k = 0;
        while k < shown.len() {
            let area = &all[shown[k]].0;
            let n = shown[k..].iter().take_while(|&&i| all[i].0 == *area).count();
            rows.push(R::Group(area.clone(), n));
            if !app.keycmds.collapsed.contains(area) || !app.keycmds.search.is_empty() {
                rows.extend(shown[k..k + n].iter().map(|&i| R::Cmd(i)));
            }
            k += n;
        }
        let body = Rect::from_min_max(pos2(list.min.x + 1.0, head.max.y + 1.0), pos2(list.max.x - 1.0, list.max.y - 1.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        let mut sa = egui::ScrollArea::vertical().id_salt("kc-list").auto_shrink([false, false]);
        if std::mem::take(&mut app.keycmds.scroll_to) {
            if let Some(pos) = app.keycmds.selected.first().and_then(|s| rows.iter().position(|r| matches!(r, R::Cmd(i) if i == s))) {
                sa = sa.vertical_scroll_offset((pos as f32 * ROW - body.height() / 2.0).max(0.0));
            }
        }
        let alt = ui.input(|i| i.modifiers.alt);
        let shift_cmd = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        sa.show_rows(&mut child, ROW, rows.len(), |ui, range| {
            for ri in range {
                let (rr, _) = ui.allocate_exact_size(vec2(body.width(), ROW), Sense::hover());
                let p = ui.painter();
                match &rows[ri] {
                    R::Group(area, _) => {
                        let open = !app.keycmds.collapsed.contains(area);
                        p.rect_filled(rr, 0.0, c(0x2a2928));
                        let dr = Rect::from_center_size(pos2(rr.min.x + 12.0, rr.center().y), vec2(12.0, 12.0));
                        crate::glyphs::paint(p, dr, if open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 10.0, c(0xa0a09f));
                        p.text(pos2(rr.min.x + 26.0, rr.center().y), Align2::LEFT_CENTER, format!("{area}"), look::semibold(12.0), c(0xc8c7c6));
                        if ui.interact(rr, ui.scope_id().with(("g", ri)), Sense::click()).clicked() {
                            // (Option-click: all groups)
                            if alt {
                                if open {
                                    app.keycmds.collapsed = all.iter().map(|c| c.0.clone()).collect();
                                } else {
                                    app.keycmds.collapsed.clear();
                                }
                            } else if open {
                                app.keycmds.collapsed.insert(area.clone());
                            } else {
                                app.keycmds.collapsed.remove(area);
                            }
                        }
                    }
                    R::Cmd(i) => {
                        let i = *i;
                        let sel = app.keycmds.selected.contains(&i);
                        p.rect_filled(rr, 0.0, if sel { c(0x2c5fc4) } else if ri % 2 == 0 { c(0x252423) } else { c(0x1f1e1d) });
                        let tc = Color32::WHITE;
                        p.with_clip_rect(Rect::from_min_max(rr.min, pos2(rr.min.x + 430.0, rr.max.y))).text(pos2(rr.min.x + 40.0, rr.center().y), Align2::LEFT_CENTER, &all[i].1, look::regular(12.0), tc);
                        let keys = app.keycmds.set.keys[i].join("  ");
                        p.text(pos2(rr.min.x + 530.0, rr.center().y), Align2::RIGHT_CENTER, keys, look::regular(12.0), if app.keycmds.set.customized(i) && !sel { c(0x8fb7ff) } else { tc });
                        if let Some(m) = app.keycmds.set.midi[i].first() {
                            p.text(pos2(rr.min.x + 633.0, rr.center().y), Align2::LEFT_CENTER, m, look::regular(11.0), tc);
                        }
                        let resp = ui.interact(rr, ui.scope_id().with(("c", i)), Sense::click());
                        if resp.clicked() {
                            let s = &mut app.keycmds;
                            match shift_cmd {
                                (_, true) => {
                                    if let Some(k) = s.selected.iter().position(|x| *x == i) {
                                        s.selected.remove(k);
                                    } else {
                                        s.selected.push(i);
                                    }
                                }
                                (true, _) => {
                                    if let Some(&first) = s.selected.first() {
                                        let a = shown.iter().position(|x| *x == first).unwrap_or(0);
                                        let b = shown.iter().position(|x| *x == i).unwrap_or(0);
                                        let (lo, hi) = (a.min(b), a.max(b));
                                        s.selected = std::iter::once(first).chain(shown[lo..=hi].iter().copied().filter(|x| *x != first)).collect();
                                    }
                                }
                                _ => s.selected = vec![i],
                            }
                            s.learn = None;
                        }
                        let handled = crate::menus::handled(&all[i].1);
                        resp.on_hover_text(format!("{} ▸ {}{}", all[i].0, all[i].1, if handled { "" } else { "\n(not in Bramble Studio yet: the key is kept but does nothing)" }));
                    }
                }
            }
        });
        // the right pane
        let pane = Rect::from_min_max(pos2(r.min.x + 792.0, r.min.y + 54.0), pos2(r.max.x - 16.0, r.max.y - 36.0));
        p.rect_filled(pane, 8.0, c(0x2a2928));
        let sel = app.keycmds.selected.first().copied();
        let lx = pane.min.x + 16.0;
        let mut y = pane.min.y + 14.0;
        p.text(pos2(lx, y + 8.0), Align2::LEFT_CENTER, "Keyboard", look::bold(13.0), Color32::WHITE);
        y += 26.0;
        p.text(pos2(lx + 38.0, y + 11.0), Align2::RIGHT_CENTER, "Key:", look::regular(12.0), Color32::WHITE);
        let kf = Rect::from_min_size(pos2(lx + 44.0, y), vec2(pane.max.x - lx - 60.0, 22.0));
        p.rect_filled(kf, 4.0, c(0x1b1a19));
        if let Some(i) = sel {
            p.text(pos2(kf.min.x + 8.0, kf.center().y), Align2::LEFT_CENTER, app.keycmds.set.keys[i].join(", "), look::regular(12.0), Color32::WHITE);
        }
        y += 30.0;
        let bw = pane.max.x - lx - 16.0;
        let learn = app.keycmds.learn;
        for (lab, mode) in [("Learn by Key Label", Learn::Label), ("Learn by Key Position", Learn::Position)] {
            if button(ui, &p, Rect::from_min_size(pos2(lx, y), vec2(bw, 22.0)), id.with(lab), lab, learn == Some(mode), sel.is_some()) {
                app.keycmds.learn = if learn == Some(mode) { None } else { Some(mode) };
            }
            y += 26.0;
        }
        if button(ui, &p, Rect::from_min_size(pos2(lx, y), vec2(bw, 22.0)), id.with("del-key"), "Delete", false, sel.is_some_and(|i| !app.keycmds.set.keys[i].is_empty())) {
            let s = &mut app.keycmds;
            for i in s.selected.clone() {
                s.set.keys[i].clear();
            }
            s.changed();
        }
        y += 34.0;
        p.hline(lx..=pane.max.x - 16.0, y, Stroke::new(1.0, c(0x3b3a39)));
        y += 12.0;
        // Touch Bar: none on Linux
        p.text(pos2(lx, y + 8.0), Align2::LEFT_CENTER, "Touch Bar", look::bold(13.0), Color32::WHITE);
        y += 26.0;
        for lab in ["Button:", "Label:", "Color:"] {
            p.text(pos2(lx + 46.0, y + 11.0), Align2::RIGHT_CENTER, lab, look::regular(12.0), c(0x8a8988));
            p.rect_filled(Rect::from_min_size(pos2(lx + 52.0, y), vec2(bw - 52.0, 22.0)), 4.0, c(0x252423));
            y += 26.0;
        }
        button(ui, &p, Rect::from_min_size(pos2(lx, y), vec2(bw, 22.0)), id.with("tb-learn"), "Learn Touch Bar", false, false);
        y += 26.0;
        button(ui, &p, Rect::from_min_size(pos2(lx, y), vec2(bw, 22.0)), id.with("tb-del"), "Delete", false, false);
        y += 34.0;
        p.hline(lx..=pane.max.x - 16.0, y, Stroke::new(1.0, c(0x3b3a39)));
        y += 12.0;
        p.text(pos2(lx, y + 8.0), Align2::LEFT_CENTER, "Control Surface", look::bold(13.0), Color32::WHITE);
        y += 22.0;
        let box_r = Rect::from_min_max(pos2(lx, y), pos2(pane.max.x - 16.0, pane.max.y - 70.0));
        p.rect_filled(box_r, 0.0, c(0x1f1e1d));
        p.rect_stroke(box_r, 0.0, Stroke::new(1.0, c(0x3b3a39)), egui::StrokeKind::Inside);
        if let Some(i) = sel {
            for (k, m) in app.keycmds.set.midi[i].iter().enumerate() {
                p.with_clip_rect(box_r).text(pos2(box_r.min.x + 6.0, box_r.min.y + 10.0 + k as f32 * 18.0), Align2::LEFT_CENTER, describe_midi(m), look::regular(11.0), Color32::WHITE);
            }
        }
        let y2 = pane.max.y - 62.0;
        if button(ui, &p, Rect::from_min_size(pos2(lx, y2), vec2(bw, 22.0)), id.with("cs-learn"), "Learn New Assignment", learn == Some(Learn::Midi), sel.is_some()) {
            app.keycmds.learn = if learn == Some(Learn::Midi) { None } else { Some(Learn::Midi) };
        }
        if button(ui, &p, Rect::from_min_size(pos2(lx, y2 + 28.0), vec2(bw, 22.0)), id.with("cs-del"), "Delete Assignment", false, sel.is_some_and(|i| !app.keycmds.set.midi[i].is_empty())) {
            let s = &mut app.keycmds;
            for i in s.selected.clone() {
                s.set.midi[i].clear();
            }
            s.changed();
        }
        // the status line under the list
        let status = match (app.keycmds.learn, app.keycmds.filter) {
            (Some(Learn::Midi), _) => "Learn New Assignment: send a message from a MIDI controller (a key, button or knob)".to_string(),
            (Some(_), _) => "Learn: press a key, with any of Shift, Option and Ctrl (Esc: stop learning)".to_string(),
            (None, Filter::Pressed) => format!("Pressed: press a key to list the commands on it{}", app.keycmds.pressed.as_ref().map(|k| format!(" — {k}")).unwrap_or_default()),
            // (Logic shows no line under the list when nothing is being learnt)
            _ => String::new(),
        };
        p.text(pos2(list.min.x, r.max.y - 18.0), Align2::LEFT_CENTER, status, look::regular(11.0), c(0x9a9998));
    });
    if let Some(f) = fired {
        more(app, ctx, &f);
    }
    if !open {
        app.keycmds.open = false;
        app.keycmds.learn = None;
    }
    conflict_alert(app, ctx);
    save_as_sheet(app, ctx);
}

fn conflict_alert(app: &mut App, ctx: &egui::Context) {
    let Some(cf) = app.keycmds.conflict.clone() else { return };
    let all = keymap::all_commands();
    let mut answer = None;
    egui::Window::new("key-conflict").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).fixed_size(vec2(380.0, 0.0)).show(ctx, |ui| {
        ui.label(egui::RichText::new(format!("The key command “{}” is already assigned to:", cf.spec)).strong());
        for j in &cf.users {
            ui.label(format!("“{}” ({})", all[*j].1, all[*j].0));
        }
        ui.label(format!("Replace moves it to “{}”; OK assigns it there as well.", all[cf.cmd].1));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                answer = Some(0);
            }
            if ui.button("Replace").clicked() {
                answer = Some(1);
            }
            if ui.button("OK").clicked() {
                answer = Some(2);
            }
        });
    });
    if let Some(a) = answer {
        let s = &mut app.keycmds;
        if a > 0 {
            if a == 1 {
                for j in &cf.users {
                    s.set.keys[*j].retain(|k| *k != cf.spec);
                }
            }
            s.set.assign(cf.cmd, &cf.spec, false);
            s.changed();
        }
        s.conflict = None;
    }
}

fn save_as_sheet(app: &mut App, ctx: &egui::Context) {
    let Some((mut name, only)) = app.keycmds.save_as.clone() else { return };
    let mut done = None;
    egui::Window::new(if only { "Save Customized Key Commands As" } else { "Save Key Commands As" }).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Save As:");
            let te = ui.text_edit_singleline(&mut name);
            crate::widgets::focus_once(ui, &te);
            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
        });
        ui.label(egui::RichText::new(dir().map(|d| d.display().to_string()).unwrap_or_default()).color(look::text_dim()));
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
            if ui.button("Save").clicked() {
                done = Some(true);
            }
        });
    });
    app.keycmds.save_as = Some((name.clone(), only));
    match done {
        Some(true) => {
            app.keycmds.save_as = None;
            let clean: String = name.chars().filter(|c| !"/\\\0".contains(*c)).collect();
            if let (Some(d), false) = (dir(), clean.trim().is_empty()) {
                save_to(app, d.join(format!("{}.{EXT}", clean.trim())), only);
            }
        }
        Some(false) => app.keycmds.save_as = None,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_set_makes_logics_bindings() {
        let us = Set::us();
        let b = us.bindings();
        assert_eq!(b.len(), keymap::defaults().len());
        assert!(!us.customized(0));
        assert!(keymap::all_commands().len() > 2000);
    }

    #[test]
    fn assign_replace_and_conflicts() {
        let all = keymap::all_commands();
        let mut s = Set::us();
        let play = all.iter().position(|c| c.0 == "Global Commands" && c.1 == "Play or Stop").unwrap();
        let rec = all.iter().position(|c| c.0 == "Global Commands" && c.1 == "Record").unwrap();
        assert!(s.conflicts().is_empty() || !s.conflicts().contains(&rec));
        // OK: both keep ˽
        s.assign(rec, "˽", false);
        assert!(s.conflicts().contains(&rec) && s.conflicts().contains(&play));
        assert!(s.customized(rec));
        // Replace: only Record has it
        s.assign(rec, "˽", true);
        assert!(s.keys[play].is_empty());
        assert_eq!(s.users("˽", Some("Global Commands")), vec![rec]);
        // the app's keys follow the set
        let b = s.bindings();
        assert!(b.iter().any(|b| b.command == "Record" && b.key == egui::Key::Space));
        // Initialize Selection, Unused
        s.initialize(&[rec]);
        assert_eq!(s.keys[rec], vec!["R".to_string()]);
        assert!(s.keys[play].is_empty());
        s.initialize_unused();
        assert_eq!(s.keys[play], vec!["˽".to_string()]);
    }

    #[test]
    fn files_round_trip_and_import_modes() {
        let all = keymap::all_commands();
        let rec = all.iter().position(|c| c.1 == "Record").unwrap();
        let stop = all.iter().position(|c| c.0 == "Global Commands" && c.1 == "Stop").unwrap();
        let mut a = Set::us();
        a.keys[rec] = vec!["⌥⇧R".into()];
        a.midi[stop] = vec!["B0 07".into()];
        let (b, _) = Set::from_text(&a.to_text(false)).unwrap();
        assert_eq!(a.keys, b.keys);
        assert_eq!(a.midi, b.midi);
        // a customized-only file names only what changed, and fills in from U.S.
        let t = a.to_text(true);
        assert!(t.lines().count() < 10);
        let (c, named) = Set::from_text(&t).unwrap();
        assert_eq!(c.keys, a.keys);
        assert_eq!(named.len(), 2);
        // Merge takes only the customized ones; to Selection only the selection
        let mut d = Set::us();
        d.keys[stop + 1] = vec!["⌥⇧⌘9".into()];
        d.import(&a, Import::Merge, &[]);
        assert_eq!(d.keys[stop + 1], vec!["⌥⇧⌘9".to_string()]);
        assert_eq!(d.keys[rec], vec!["⌥⇧R".to_string()]);
        let mut e = Set::us();
        e.import(&a, Import::Selection, &[stop]);
        assert_eq!(e.keys[rec], vec!["R".to_string()]);
        assert_eq!(e.midi[stop], vec!["B0 07".to_string()]);
        assert!(Set::from_text("not a set").is_none());
        // clipboard: tab-separated, a row per command
        assert_eq!(a.clipboard_text(&[rec]).lines().nth(1).unwrap(), "Global Commands\tRecord\t⌥⇧R\t");
    }

    #[test]
    fn learned_specs_parse_back() {
        let s = keymap::spec_of(egui::Key::K, egui::Modifiers { alt: true, shift: true, ..Default::default() }).unwrap();
        assert_eq!(s, "⌥⇧K");
        assert!(keymap::parse_spec(&s).is_some());
        assert_eq!(keymap::spec_of(egui::Key::ArrowLeft, egui::Modifiers::COMMAND).unwrap(), "⌘←");
        assert_eq!(describe_midi("B0 07"), "B0 07  (Ch 1, Control 7)");
    }
}
