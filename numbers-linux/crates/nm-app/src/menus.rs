//! The menu bar, generated from Numbers 15.4's own menu tree (data/menus.txt, captured on the
//! Mac in research round N1) the way Keydeck generates Keynote's: items Values implements run
//! a command, the rest are shown greyed so the gaps stay visible. Every shortcut in the tree
//! (⌘ = Ctrl, ⌥ = Alt, ⇧ = Shift, ⌃ = Super) is also a keyboard shortcut here.

use std::sync::OnceLock;

use egui::{Key, KeyboardShortcut, Modifiers};
use nm_core::kn_model::edit::ZMove;
use nm_core::kn_model::format::TextProp;
use nm_core::kn_model::tablecells::CellAlign;
use nm_core::kn_model::tablerows::{Axis, HeaderKind};
use nm_core::kn_format::ObjId;
use nm_core::kn_model::DrawableKind;

use crate::app::{App, InspTab, Sel};
use crate::canvas::commit;

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub label: String,
    pub shortcut: Option<String>,
    pub children: Vec<Item>,
    pub separator: bool,
}

fn parse() -> Vec<Item> {
    let mut menus: Vec<Item> = Vec::new();
    for line in include_str!("../data/menus.txt").lines() {
        if let Some(name) = line.strip_prefix("=== ").and_then(|l| l.strip_suffix(" ===")) {
            menus.push(Item { label: name.to_string(), shortcut: None, children: Vec::new(), separator: false });
            continue;
        }
        let Some(top) = menus.last_mut() else { continue };
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if text.is_empty() || text.starts_with('<') {
            continue;
        }
        let depth = indent / 2;
        let mut parent = top;
        for _ in 1..depth {
            if parent.children.is_empty() {
                break;
            }
            parent = parent.children.last_mut().unwrap();
        }
        if text == "---" {
            parent.children.push(Item { label: String::new(), shortcut: None, children: Vec::new(), separator: true });
            continue;
        }
        let text = text.replace(" (disabled now)", "");
        let (label, shortcut) = match text.rsplit_once("   [") {
            Some((l, s)) => (l.to_string(), linux_shortcut(s.trim_end_matches(']'))),
            None => (text.to_string(), None),
        };
        if parent.children.last().is_some_and(|c| c.label == label) {
            continue;
        }
        parent.children.push(Item { label, shortcut, children: Vec::new(), separator: false });
    }
    // macOS-only items, Apple services and AI features have no meaning here
    const MAC_ONLY: &[&str] = &[
        "Learn About Apple Creator Studio",
        "Get Apple Creator Studio",
        "Provide Values Feedback",
        "Privacy & Analytics",
        "Services",
        "Hide Values",
        "Hide Others",
        "Show All",
        "Quit and Keep Windows",
        "Start Dictation…",
        // (an Apple service)
        "Copy Snapshot",
        "Emoji & Symbols",
        "AutoFill",
        "Writing Tools",
        "Generate Image",
        "Generate Shape",
        "Image Playground…",
        "Generate from Image",
        "Super Resolution",
        "Magic Fill",
        "Minimize All",
        "Zoom All",
        "Center",
        "Move & Resize",
        "Full Screen Tile",
        "Move to iPad Pro",
        "Bring All to Front",
        "Arrange in Front",
        "Remove Window from Set",
        "Merge All Windows",
        "Move Tab to New Window",
        "Edit in Pixelmator Pro",
        "Optimize for iPhone and iPad",
        "Movie",
        "Numbers ’09…",
        "Formulas and Functions Help",
        "Remove Watermarks",
        "Share…",
        "Activity Settings…",
        "Hide Participant Cursors",
        "Show Activity Pane",
        "Show Sync Status",
        "Send Values Feedback",
        "What’s New in Values",
        "Welcome to Values",
        "About Premium Features",
        "Service and Support",
        "Phonetic Guide Text…",
        "Remove Phonetic Guide Text",
        "Show Tab Bar",
        "Show All Tabs",
        "Show Previous Tab",
        "Show Next Tab",
    ];
    fn prune(items: &mut Vec<Item>, parent: &str) {
        // ("Center" is Window > Center, macOS's; Arrange > Align Objects > Center is ours)
        items.retain(|i| !(MAC_ONLY.contains(&i.label.as_str()) && !(parent == "Align Objects" && i.label == "Center")) && !i.label.starts_with("<<"));
        for i in items.iter_mut() {
            if !i.children.is_empty() {
                let label = i.label.clone();
                prune(&mut i.children, &label);
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
    prune(&mut menus, "");
    menus
}

/// "⇧⌘Z" -> "Ctrl+Shift+Z" (None for keys a PC keyboard has no equivalent for).
fn linux_shortcut(mac: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut key = String::new();
    for c in mac.chars() {
        match c {
            '⌃' => parts.push("Super"),
            '⌥' => parts.push("Alt"),
            '⇧' => parts.push("Shift"),
            '⌘' => parts.push("Ctrl"),
            _ => key.push(c),
        }
    }
    if key.starts_with("fn") || key.starts_with('m') && key.len() > 1 {
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
        "⇥" => "Tab".to_string(),
        "←" => "Left".to_string(),
        "→" => "Right".to_string(),
        "↑" => "Up".to_string(),
        "↓" => "Down".to_string(),
        k => k.to_uppercase(),
    };
    parts.push(&key);
    Some(parts.join("+"))
}

/// The keyboard shortcut of a shortcut text ("Ctrl+Shift+Z"); None for ones that use Super
/// (the desktop keeps those) or keys egui can't tell apart.
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
    // "Ctrl++" splits into an empty last part
    let key = key.unwrap_or(if s.ends_with('+') { "+" } else { "" });
    let k = match key {
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Enter" => Key::Enter,
        "Tab" => Key::Tab,
        "Left" => Key::ArrowLeft,
        "Right" => Key::ArrowRight,
        "Up" => Key::ArrowUp,
        "Down" => Key::ArrowDown,
        "+" => Key::Plus,
        "-" => Key::Minus,
        "=" => Key::Equals,
        "." => Key::Period,
        "," => Key::Comma,
        "\\" => Key::Backslash,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        ";" => Key::Semicolon,
        "`" => Key::Backtick,
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

/// Whether the cursor cell wraps its text.
fn cursor_wraps(app: &App) -> bool {
    app.sel.and_then(|s| {
        let (_, td) = app.table(s.table)?;
        td.cell_styles.get((s.cursor.0 * td.cols + s.cursor.1) as usize).map(|st| st.wrap)
    }).unwrap_or(true)
}

/// Help > Values Help: a page of every command that works and its shortcut, opened in the
/// browser (written to the cache folder; nothing leaves the computer).
fn help_page() {
    let mut html = String::from("<!doctype html><meta charset=utf-8><title>Values Help</title><style>body{font:15px Cantarell,sans-serif;max-width:860px;margin:2em auto;padding:0 1em;color:#241f31}h1{color:#26a269}h2{margin-top:1.6em;border-bottom:1px solid #deddda}td{padding:2px 12px 2px 0}td.k{color:#5e5c64;font-family:monospace}</style><h1>Values Help</h1><p>Values reads and writes Apple Numbers spreadsheets. These are its menus; each command works as it does in Numbers.</p><p>Stock quotes (Insert &gt; Formula &gt; Stock Quote) come from Yahoo Finance's public endpoint and may stop working. Only the ticker is sent, and only when you insert or refresh a quote.</p>");
    fn rows(items: &[Item], parent: &str, out: &mut String) {
        for i in items {
            if i.separator {
                continue;
            }
            if i.children.is_empty() {
                let name = command_name(parent, &i.label);
                if implemented(&name) {
                    out.push_str(&format!("<tr><td>{}</td><td class=k>{}</td></tr>", name.replace('&', "&amp;").replace('<', "&lt;"), i.shortcut.clone().unwrap_or_default()));
                }
            } else {
                rows(&i.children, &i.label, out);
            }
        }
    }
    for m in menus() {
        let mut t = String::new();
        rows(&m.children, &m.label, &mut t);
        if !t.is_empty() {
            html.push_str(&format!("<h2>{}</h2><table>{t}</table>", m.label));
        }
    }
    let dir = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache"))).unwrap_or_else(std::env::temp_dir).join("numdeck");
    let _ = std::fs::create_dir_all(&dir);
    let page = dir.join("help.html");
    if std::fs::write(&page, html).is_ok() && std::env::var_os("NM_SCRIPT").is_none() {
        let _ = kn_ui::open_command(&page).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }
}

/// The text style of the cursor cell, or of the selected text box's first run.
pub(crate) fn cursor_char(app: &App) -> Option<nm_core::kn_model::CharStyle> {
    if let Some(s) = app.sel {
        let (_, td) = app.table(s.table)?;
        return td.cell_styles.get((s.cursor.0 * td.cols + s.cursor.1) as usize).map(|st| st.text.clone());
    }
    None
}

/// Arrange > Align Objects (to the selection's outer edge / centre) and Distribute Objects
/// (equal gaps, the outermost two staying put; Evenly: both ways). Keydeck's align.
fn align(app: &mut App, name: &str) {
    let how = name.rsplit(" > ").next().unwrap_or("");
    let items: Vec<(ObjId, (f32, f32, f32, f32))> = app
        .selected_objs()
        .into_iter()
        .filter_map(|id| app.drawables.iter().find(|d| d.id == id && !d.locked))
        .map(|d| {
            let g = crate::tiles::frame(d);
            let (lo, hi) = g.bounds();
            (d.id, (lo.x, lo.y, hi.x, hi.y))
        })
        .collect();
    if items.len() < 2 {
        return;
    }
    let minx = items.iter().map(|i| i.1.0).fold(f32::INFINITY, f32::min);
    let miny = items.iter().map(|i| i.1.1).fold(f32::INFINITY, f32::min);
    let maxx = items.iter().map(|i| i.1.2).fold(f32::NEG_INFINITY, f32::max);
    let maxy = items.iter().map(|i| i.1.3).fold(f32::NEG_INFINITY, f32::max);
    let mut moves: Vec<(ObjId, f32, f32)> = items.iter().map(|i| (i.0, 0.0, 0.0)).collect();
    if name.starts_with("Align") {
        for (m, (_, (x0, y0, x1, y1))) in moves.iter_mut().zip(&items) {
            match how {
                "Left" => m.1 = minx - x0,
                "Center" => m.1 = (minx + maxx) / 2.0 - (x0 + x1) / 2.0,
                "Right" => m.1 = maxx - x1,
                "Top" => m.2 = miny - y0,
                "Middle" => m.2 = (miny + maxy) / 2.0 - (y0 + y1) / 2.0,
                _ => m.2 = maxy - y1,
            }
        }
    } else {
        for axis in [0usize, 1] {
            if (how == "Horizontally" && axis == 1) || (how == "Vertically" && axis == 0) {
                continue;
            }
            let lo = |b: &(f32, f32, f32, f32)| if axis == 0 { b.0 } else { b.1 };
            let hi = |b: &(f32, f32, f32, f32)| if axis == 0 { b.2 } else { b.3 };
            let mut order: Vec<usize> = (0..items.len()).collect();
            order.sort_by(|a, b| lo(&items[*a].1).total_cmp(&lo(&items[*b].1)));
            let total: f32 = items.iter().map(|i| hi(&i.1) - lo(&i.1)).sum();
            let start = lo(&items[order[0]].1);
            let end = items.iter().map(|i| hi(&i.1)).fold(f32::NEG_INFINITY, f32::max);
            let gap = (end - start - total) / (items.len() - 1) as f32;
            let mut pos = start;
            for i in order {
                let d = pos - lo(&items[i].1);
                pos += hi(&items[i].1) - lo(&items[i].1) + gap;
                if axis == 0 {
                    moves[i].1 = d;
                } else {
                    moves[i].2 = d;
                }
            }
        }
    }
    let label = if name.starts_with("Align") { "Align Objects" } else { "Distribute Objects" };
    app.edit(label, |d, h| {
        for (id, dx, dy) in &moves {
            if dx.abs() > 0.001 || dy.abs() > 0.001 {
                d.move_by(h, *id, *dx, *dy);
            }
        }
    });
}

/// Table > Select > First / Last Value in Row / Column (Extend: the anchor stays): the first
/// or last cell of the cursor's row / column that has something in it.
fn select_value(app: &mut App, name: &str) {
    let Some(sel) = app.sel else { return };
    let Some((_, td)) = app.table(sel.table) else { return };
    let (r, c) = sel.cursor;
    let row = name.ends_with("Row");
    let last = name.contains("Last");
    let has = |r: u32, c: u32| td.cells.get((r * td.cols + c) as usize).is_some_and(|v| !v.trim().is_empty());
    let found: Option<(u32, u32)> = if row {
        let mut it: Box<dyn Iterator<Item = u32>> = if last { Box::new((0..td.cols).rev()) } else { Box::new(0..td.cols) };
        it.find(|&k| has(r, k)).map(|k| (r, k))
    } else {
        let mut it: Box<dyn Iterator<Item = u32>> = if last { Box::new((0..td.rows).rev()) } else { Box::new(0..td.rows) };
        it.find(|&k| has(k, c)).map(|k| (k, c))
    };
    let Some(at) = found else { return };
    commit(app);
    let s = if name.contains("Extend") { crate::app::Sel { cursor: at, ..sel } } else { crate::app::Sel::cell(sel.table, at.0, at.1) };
    app.sel = Some(s);
    app.reveal_cell(s.table, at.0, at.1);
}

/// Edit > Transformations: the selected cells' text in upper / lower case, or each word
/// capitalized (formulas and numbers are left alone).
fn transform_text(app: &mut App, name: &str) {
    let Some(sel) = app.sel else { return };
    commit(app);
    let t = sel.table;
    let blocks = app.model_blocks(t, sel.block());
    let change = |s: &str| -> String {
        match name {
            "Transformations > Make Upper Case" => s.to_uppercase(),
            "Transformations > Make Lower Case" => s.to_lowercase(),
            _ => {
                let mut out = String::new();
                let mut start = true;
                for ch in s.chars() {
                    if start && ch.is_alphabetic() {
                        out.extend(ch.to_uppercase());
                        start = false;
                    } else {
                        out.extend(ch.to_lowercase());
                        if ch.is_whitespace() {
                            start = true;
                        } else if !ch.is_alphanumeric() && ch != '\'' && ch != '’' {
                            start = true;
                        }
                    }
                }
                out
            }
        }
    };
    let label = name.trim_start_matches("Transformations > ").to_string();
    app.edit(&label, |d, h| {
        let values = d.cell_values(t);
        for (r0, c0, r1, c1) in &blocks {
            for r in *r0..=*r1 {
                for c in *c0..=*c1 {
                    if d.formula_text(t, r, c).is_some() {
                        continue;
                    }
                    if let Some(nm_core::kn_model::table::CellValue::Text(s)) = values.get(&(r, c)) {
                        let new = change(s.as_str());
                        if &new != s {
                            let _ = d.set_cell(h, t, r, c, &nm_core::kn_model::tablecells::CellInput::Text(new));
                        }
                    }
                }
            }
        }
    });
}

/// The sheet's tables' cell comments, in reading order: (table, shown row, column).
fn sheet_comments(app: &App) -> Vec<(ObjId, u32, u32)> {
    let Some(w) = app.wb.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    for d in &app.drawables {
        if !matches!(d.kind, DrawableKind::Table(_)) {
            continue;
        }
        let shown = |m: u32| app.views.get(&d.id).and_then(|v| v.rows.iter().position(|x| *x == Some(m)).map(|p| p as u32)).unwrap_or(m);
        let mut cs: Vec<(u32, u32)> = nm_core::cell_comments(w.file(), d.id).into_keys().map(|(r, c)| (shown(r), c)).collect();
        cs.sort_unstable();
        out.extend(cs.into_iter().map(|(r, c)| (d.id, r, c)));
    }
    out
}

fn any_comments(app: &App) -> bool {
    !sheet_comments(app).is_empty()
}

/// View > Comments > Show Next / Previous Comment: select the next commented cell after the
/// cursor (wrapping round), showing comments.
fn next_comment(app: &mut App, forward: bool) {
    let all = sheet_comments(app);
    if all.is_empty() {
        return;
    }
    let cur = app.sel.map(|s| (s.table, s.cursor.0, s.cursor.1));
    let i = match cur.and_then(|c| all.iter().position(|x| *x == c)) {
        Some(i) if forward => (i + 1) % all.len(),
        Some(i) => (i + all.len() - 1) % all.len(),
        None if forward => 0,
        None => all.len() - 1,
    };
    let (t, r, c) = all[i];
    commit(app);
    app.show_comments = true;
    app.sel = Some(crate::app::Sel::cell(t, r, c));
    app.reveal_cell(t, r, c);
}

/// Whether `name` is a command in the menus (as [`run`] takes it).
#[cfg(test)]
pub(crate) fn is_command(name: &str) -> bool {
    fn walk(items: &[Item], parent: &str, name: &str) -> bool {
        items.iter().any(|it| if it.children.is_empty() { command_name(parent, &it.label) == name } else { walk(&it.children, &it.label, name) })
    }
    menus().iter().any(|m| walk(&m.children, &m.label, name))
}

pub fn menus() -> &'static [Item] {
    static M: OnceLock<Vec<Item>> = OnceLock::new();
    M.get_or_init(parse)
}

/// Submenus whose items are named with the submenu ("Table > Add Row Above").
const QUALIFIED: &[&str] = &[
    "New",
    "Table", "Chart", "Shape", "Line", "Header Rows", "Header Columns", "Footer Rows", "Formula", "Select", "Align Objects", "Distribute Objects", "Export To", "Inspector", "Zoom", "Comments", "Font", "Text",
    "Sheet", "Find", "Autofill Cells", "Chart Type", "Author Color", "Guides", "Create Pivot Table", "Character Spacing", "Ligature", "Baseline", "Capitalization", "Add Link", "Mask with Shape", "Unhide",
    "Advanced", "Revert To", "Spelling and Grammar", "Substitutions", "Transformations", "Shapes and Lines", "Image",
];

fn command_name(parent: &str, label: &str) -> String {
    if QUALIFIED.contains(&parent) { format!("{parent} > {label}") } else { label.to_string() }
}

/// Commands that work now; everything else in the menus is greyed.
const IMPLEMENTED: &[&str] = &[
    "About Values",
    "Support Development…",
    "Show Rulers",
    "Line Break",
    "Column Break",
    "Date & Time",
    "Highlight",
    "Formula > Edit Formula",
    "Table > Transpose Rows and Columns",
    "Show Warnings",
    "Line > Draw With Pen",
    "Save as Template…",
    "Web Video…",
    "Record Audio",
    "Image Gallery",
    "Show Music Browser",
    "Show Photo and Video Browser",
    "Equation...",
    "Start Speaking",
    "Stop Speaking",
    "Advanced > Language & Region…",
    "Advanced > Set as Default Text Box Appearance",
    "Advanced > Create Table Style",
    "Shapes and Lines > Toggle Line Type",
    "Shapes and Lines > Save to My Shapes",
    "Line > Straight Connection Line",
    "Line > Curved Connection Line",
    "Line > Angled Connection Line",
    "Guides > Hide Guides",
    "Guides > Clear All Guides on Sheet",
    "Settings…",
    "Quit Values",
    "New > Numbers Spreadsheet",
    "New > Excel Workbook",
    "Open…",
    "Close",
    "Close All",
    "Save",
    "Save As…",
    "Duplicate",
    "Rename…",
    "Export To > PDF…",
    "Export To > CSV…",
    "Export To > Excel…",
    "Export To > TSV…",
    "Set Password…",
    "Print…",
    "Undo",
    "Redo",
    "Cut",
    "Copy",
    "Paste",
    "Paste and Match Style",
    "Paste Formula Results",
    "Paste and Preserve Style",
    "Delete",
    "Clear All",
    "Duplicate Selection",
    "Select All",
    "Deselect All",
    "Edit Formula",
    "Find > Find…",
    "Find > Find Next",
    "Find > Find Previous",
    "Find > Hide Find Panel",
    "Sheet",
    "Text",
    "Comment",
    "Choose File",
    "Browse Content Hub",
    "Show Content Hub",
    "Line > Line",
    "Line > Line with Arrowhead",
    "Line > Line with Two Arrowheads",
    "Table > Add Row Above",
    "Table > Add Row Below",
    "Table > Add Column Before",
    "Table > Add Column After",
    "Table > Delete Row",
    "Table > Delete Column",
    "Table > Insert Current Date",
    "Table > Insert Current Time",
    "Table > Merge Cells",
    "Table > Hide Row",
    "Table > Hide Column",
    "Table > Unhide All Rows",
    "Table > Unhide All Columns",
    "Autofill Cells > Autofill",
    "Baseline > Use Default",
    "Baseline > Superscript",
    "Baseline > Subscript",
    "Baseline > Raise",
    "Baseline > Lower",
    "Capitalization > None",
    "Capitalization > All Caps",
    "Capitalization > Small Caps",
    "Capitalization > Title Case",
    "Capitalization > Start Case",
    "Character Spacing > Use None",
    "Character Spacing > Tighten",
    "Character Spacing > Loosen",
    "Ligature > Use Default",
    "Ligature > Use None",
    "Ligature > Use All",
    "Group",
    "Ungroup",
    "Open Recent",
    "Remove Highlights and Comments",
    "Font > Show Fonts",
    "Font > Replace Fonts…",
    "Revert To > Browse All Versions…",
    "Single File",
    "Package",
    "Move To…",
    "Reduce File Size…",
    "Print Without Preview…",
    "Substitutions > Smart Quotes",
    "Substitutions > Smart Dashes",
    "Substitutions > Text Replacement",
    "Substitutions > Show Substitutions",
    "Spelling and Grammar > Check Spelling While Typing",
    "Spelling and Grammar > Check Grammar With Spelling",
    "Spelling and Grammar > Correct Spelling Automatically",
    "Spelling and Grammar > Show Spelling and Grammar",
    "Spelling and Grammar > Check Document Now",
    "Show Arrange Tools",
    "Show Adjust Image",
    "Show Colors",
    "Values Help",
    "Text > Increase Indent Level",
    "Text > Decrease Indent Level",
    "Shapes and Lines > Make Editable",
    "Shapes and Lines > Break Apart",
    "Shapes and Lines > Unite Shapes",
    "Shapes and Lines > Intersect Shapes",
    "Shapes and Lines > Subtract Shapes",
    "Shapes and Lines > Exclude Shapes",
    "Shapes and Lines > Reset Text and Object Handles",
    "Shapes and Lines > Make Sharp Point",
    "Shapes and Lines > Make Smooth Point",
    "Shapes and Lines > Make Bézier Point",
    "Add Link > Webpage",
    "Add Link > Email",
    "Add Link > Phone Number",
    "Add Link > Sheet",
    "Substitutions > Smart Web and Email Links",
    "Substitutions > Smart Phone Number Links",
    "Remove Link",
    "Table > Freeze Header Rows",
    "Table > Freeze Header Columns",
    "Table > Wrap Text",
    "Select Parent",
    "Align Objects > Left",
    "Align Objects > Center",
    "Align Objects > Right",
    "Align Objects > Top",
    "Align Objects > Middle",
    "Align Objects > Bottom",
    "Distribute Objects > Horizontally",
    "Distribute Objects > Vertically",
    "Distribute Objects > Evenly",
    "Autofill Cells > Autofill Right",
    "Autofill Cells > Autofill Left",
    "Autofill Cells > Autofill Down",
    "Autofill Cells > Autofill Up",
    "Autofill Cells > Autofill from Row Above",
    "Autofill Cells > Autofill from Column Before",
    "Select > First Value in Row",
    "Select > Last Value in Row",
    "Select > First Value in Column",
    "Select > Last Value in Column",
    "Select > Extend to First Value in Row",
    "Select > Extend to Last Value in Row",
    "Select > Extend to First Value in Column",
    "Select > Extend to Last Value in Column",
    "Table > Resize Row to Fit Content",
    "Table > Resize Column to Fit Content",
    "Table > Distribute Rows Evenly",
    "Table > Distribute Columns Evenly",
    "Transformations > Make Upper Case",
    "Transformations > Make Lower Case",
    "Transformations > Capitalize",
    "Comments > Show Next Comment",
    "Comments > Show Previous Comment",
    "Find > Use Selection for Find",
    "Find > Use Selection for Replace",
    "Copied Rows",
    "Copied Columns",
    "Find > Jump to Selection",
    "Unhide > Unhide All Sheets",
    "Table > Unmerge Cells",
    "Select > Row",
    "Select > Column",
    "Show Sort Options",
    "Show Filter Options",
    "Show Category Options",
    "Font > Bold",
    "Font > Italic",
    "Font > Underline",
    "Font > Strikethrough",
    "Font > Bigger",
    "Font > Smaller",
    "Text > Align Left",
    "Text > Align Center",
    "Text > Align Right",
    "Text > Justify",
    "Text > Auto-Align Table Cell",
    "Sheet > Rename…",
    "Sheet > Duplicate",
    "Sheet > Show Sheet Options",
    "Sheet > Tab Color",
    "Conditional Highlighting…",
    "Copy Style",
    "Paste Style",
    "Image > Remove Background",
    "Image > Edit Mask",
    "Image > Reset Mask",
    "Image > Reset Background",
    "Bring Forward",
    "Bring to Front",
    "Send Backward",
    "Send to Back",
    "Flip Horizontally",
    "Flip Vertically",
    "Lock",
    "Unlock",
    "Inspector > Format",
    "Inspector > Organize",
    "Inspector > Show Next Inspector Tab",
    "Inspector > Show Previous Inspector Tab",
    "Inspector > Hide Inspector",
    "Comments > Hide Comments",
    "Zoom > Zoom In",
    "Zoom > Zoom Out",
    "Zoom > Actual Size",
    "Zoom > Zoom to Selection",
    "Enter Full Screen",
    "Hide Toolbar",
    "Customize Toolbar…",
    "Minimize",
    "Zoom",
    "Fill",
    "Show Previous Sheet",
    "Show Next Sheet",
    "Show First Sheet",
    "Show Last Sheet",
    "Keyboard Shortcuts",
];

/// Insert > Table's items are Numbers' six table styles (in TABLE_STYLES order).
const TABLE_ITEMS: [&str; 6] = ["Headers", "Basic", "Plain", "Sums", "Checklist", "Stock"];
/// Insert > Shape items we have Numbers-made donors for -> our SHAPES index.
const SHAPE_ITEMS: [(&str, usize); 4] = [("Rectangle", 3), ("Rounded Rectangle", 1), ("Oval", 2), ("Arrow", 4)];
/// Insert > Shape's other items -> Keynote's preset outline ("Polygon" is the pentagon).
fn preset_shape(label: &str) -> Option<nm_core::kn_model::insert::BasicShape> {
    nm_core::kn_model::insert::BasicShape::ALL.iter().copied().find(|b| b.name() == label)
}

/// Insert > Formula -> the function.
const FORMULA_ITEMS: [(&str, &str); 6] = [("Sum", "SUM"), ("Average", "AVERAGE"), ("Minimum", "MIN"), ("Maximum", "MAX"), ("Count", "COUNT"), ("Product", "PRODUCT")];

/// The 2D chart menu names -> our chart types (by their names).
/// A chart menu name ("2D Stacked Bar", "3D Pie", "Interactive Column") -> its chart type.
fn chart_enum(label: &str) -> Option<&'static str> {
    let name = label.strip_prefix("2D ").unwrap_or(label);
    nm_core::kn_model::chartdata::CHART_TYPES.iter().find(|(_, n)| *n == name).map(|(e, _)| *e)
}

/// Format > Chart's show / hide items.
const CHART_TOGGLES: [&str; 11] = [
    "Show Title",
    "Show Legend",
    "Show Borders",
    "Show Y Axis Name",
    "Show Y Axis Labels",
    "Show Y Axis 2 Name",
    "Show Y Axis 2 Labels",
    "Show X Axis Name",
    "Show X Axis Labels",
    "Show Series Names",
    "Show Series Value Labels",
];

/// The selected chart and what it shows.
fn selected_chart(app: &App) -> Option<ObjId> {
    let o = app.canvas.obj?;
    app.drawables.iter().any(|d| d.id == o && matches!(d.kind, DrawableKind::Chart(_))).then_some(o)
}

fn chart_data(app: &App) -> Option<&nm_core::kn_model::ChartData> {
    let o = selected_chart(app)?;
    app.drawables.iter().find(|d| d.id == o).and_then(|d| match &d.kind {
        DrawableKind::Chart(c) => Some(c),
        _ => None,
    })
}

/// Whether a Format > Chart item is on for the selected chart.
fn chart_shows(c: &nm_core::kn_model::ChartData, item: &str) -> bool {
    let y = |i: usize| c.value_axes.get(i);
    match item {
        "Show Title" => c.title.is_some(),
        "Show Legend" => c.show_legend,
        "Show Borders" => c.show_border,
        "Show Y Axis Name" => y(0).is_some_and(|a| a.title.is_some()),
        "Show Y Axis Labels" => y(0).is_some_and(|a| a.show_labels),
        "Show Y Axis 2 Name" => y(1).is_some_and(|a| a.title.is_some()),
        "Show Y Axis 2 Labels" => y(1).is_some_and(|a| a.show_labels),
        "Show X Axis Name" => c.category_axis.title.is_some(),
        "Show X Axis Labels" => c.category_axis.show_labels,
        "Show Series Names" => c.series.iter().any(|s| s.show_series_names),
        _ => c.series.iter().any(|s| s.show_value_labels),
    }
}

/// Format > Chart > Show …: turn it the other way.
fn chart_toggle(app: &mut App, item: &str) {
    use nm_core::kn_model::chartdata::ChartFlag;
    let (Some(id), Some(c)) = (selected_chart(app), chart_data(app)) else { return };
    let on = !chart_shows(c, item);
    let title = c.title.clone().unwrap_or_else(|| "Title".into());
    let label = item.trim_start_matches("Show ").to_string();
    app.edit(&label, |d, h| match item {
        "Show Title" => d.set_chart_title(h, id, on.then_some(title.as_str())),
        "Show Legend" => d.set_chart_legend(h, id, on),
        "Show Borders" => d.set_chart_flag(h, id, ChartFlag::Border(on)),
        "Show Y Axis Name" => d.set_chart_flag(h, id, ChartFlag::Axis(Some(0), "showtitle", on)),
        "Show Y Axis Labels" => d.set_chart_flag(h, id, ChartFlag::Axis(Some(0), "showlabels", on)),
        "Show Y Axis 2 Name" => d.set_chart_flag(h, id, ChartFlag::Axis(Some(1), "showtitle", on)),
        "Show Y Axis 2 Labels" => d.set_chart_flag(h, id, ChartFlag::Axis(Some(1), "showlabels", on)),
        "Show X Axis Name" => d.set_chart_flag(h, id, ChartFlag::Axis(None, "showtitle", on)),
        "Show X Axis Labels" => d.set_chart_flag(h, id, ChartFlag::Axis(None, "showlabels", on)),
        "Show Series Names" => d.set_chart_flag(h, id, ChartFlag::SeriesNames(on)),
        _ => d.set_chart_flag(h, id, ChartFlag::ValueLabels(on)),
    });
}

fn chart_index(label: &str) -> Option<usize> {
    let want = match label {
        "2D Column" => "Column",
        "2D Stacked Column" => "Stacked Column",
        "2D Bar" => "Bar",
        "2D Stacked Bar" => "Stacked Bar",
        "2D Line" => "Line",
        "2D Area" => "Area",
        "2D Stacked Area" => "Stacked Area",
        "2D Pie" => "Pie",
        "2D Donut" => "Donut",
        "2D Scatter" => "Scatter",
        "2D Bubble" => "Bubble",
        "2D Radar" => "Radar",
        "2D Mixed" => "Mixed",
        "2D 2-Axis" => "2-Axis",
        _ => return None,
    };
    nm_core::charts::CHART_TYPES.iter().position(|(n, _)| *n == want)
}

fn implemented(name: &str) -> bool {
    if IMPLEMENTED.contains(&name) || crate::organize::is_menu(name) || crate::pivotui::MENU.contains(&name) {
        return true;
    }
    if let Some(l) = name.strip_prefix("Table > ") {
        if TABLE_ITEMS.contains(&l) {
            return true;
        }
    }
    if let Some(l) = name.strip_prefix("Chart > ") {
        return chart_enum(l).is_some() || CHART_TOGGLES.contains(&l) || l == "Edit Chart Data…";
    }
    if let Some(l) = name.strip_prefix("Chart Type > ") {
        return chart_enum(l).is_some();
    }
    if let Some(l) = name.strip_prefix("Author Color > ") {
        return crate::settings::AUTHOR_COLORS.iter().any(|(n, _)| *n == l);
    }
    if name.starts_with("Sheet > Hide ") {
        return true;
    }
    if let Some(l) = name.strip_prefix("Mask with Shape > ") {
        return nm_core::kn_model::insert::BasicShape::ALL.iter().any(|b| b.name() == l);
    }
    if let Some(l) = name.strip_prefix("Shape > ") {
        return SHAPE_ITEMS.iter().any(|(n, _)| *n == l) || preset_shape(l).is_some();
    }
    if let Some(l) = name.strip_prefix("Formula > ") {
        return FORMULA_ITEMS.iter().any(|(n, _)| *n == l) || l == "Stock Quote";
    }
    ["Header Rows > ", "Header Columns > ", "Footer Rows > "].iter().any(|p| name.starts_with(p))
}

fn selected_table(app: &App) -> Option<nm_core::kn_format::ObjId> {
    app.sel.map(|s| s.table).or_else(|| app.canvas.obj.filter(|o| app.table(*o).is_some()))
}

pub fn is_table(app: &App, id: nm_core::kn_format::ObjId) -> bool {
    app.table(id).is_some()
}

/// What an Excel workbook can't hold (Values' Excel writer keeps tables' cells only: one table
/// a sheet, no objects, comments, highlighting rules, stock quotes or table styles): greyed in
/// plain Excel mode (compatibility mode keeps them in the Values copy).
pub(crate) fn excel_blocks(name: &str) -> bool {
    const TABLES: [&str; 6] = ["Table > Headers", "Table > Basic", "Table > Plain", "Table > Sums", "Table > Checklist", "Table > Stock"];
    TABLES.contains(&name)
        || name.strip_prefix("Chart > ").is_some_and(|l| l.starts_with("2D") || l.starts_with("3D") || l.starts_with("Interactive"))
        || name.starts_with("Shape > ")
        || name.starts_with("Line > ")
        || name.starts_with("Mask with Shape > ")
        || matches!(name, "Text" | "Comment" | "Highlight" | "Choose File" | "Equation..." | "Formula > Stock Quote" | "Conditional Highlighting…" | "Advanced > Create Table Style" | "Paste Comment")
}

pub(crate) fn enabled(app: &App, name: &str) -> bool {
    if !implemented(name) {
        return false;
    }
    if app.excel_only() && excel_blocks(name) {
        return false;
    }
    if crate::organize::is_menu(name) {
        return crate::organize::enabled(app, name);
    }
    if crate::pivotui::MENU.contains(&name) {
        return crate::pivotui::enabled(app, name);
    }
    let has_doc = app.wb.is_some();
    let cells = app.sel.is_some();
    let obj = app.canvas.obj.is_some();
    match name {
        "Undo" => app.hist.can_undo() || app.canvas.edit.is_some(),
        "Redo" => app.hist.can_redo(),
        "Cut" | "Copy" | "Delete" | "Duplicate Selection" => cells || obj,
        "Paste" => has_doc,
        "Paste and Match Style" | "Paste and Preserve Style" => cells && app.cell_clip.is_some(),
        "Paste Formula Results" => cells && crate::clip::has_formulas(app),
        "Clear All" | "Edit Formula" | "Select > Row" | "Select > Column" | "Table > Insert Current Date" | "Table > Insert Current Time" | "Text > Auto-Align Table Cell" | "Conditional Highlighting…" => cells,
        "Comment" => app.sel.is_some_and(|s| s.anchor == s.cursor),
        "Deselect All" => cells || obj,
        "Bring Forward" | "Bring to Front" | "Send Backward" | "Send to Back" | "Copy Style" => obj,
        "Paste Style" => obj && app.copied_style.is_some(),
        "Image > Remove Background" | "Image > Reset Background" => app.canvas.obj.is_some_and(|o| app.wb.as_ref().is_some_and(|w| w.file().type_name(o) == "TSD.ImageArchive")),
        "Image > Edit Mask" => crate::mask::selected_image(app).is_some(),
        "Image > Reset Mask" => crate::mask::selected_image(app).is_some_and(|o| app.wb.as_ref().is_some_and(|w| w.file().obj(o).is_some_and(|m| m.ref_id("mask").is_some()))),
        n if n.starts_with("Mask with Shape > ") => crate::mask::selected_image(app).is_some(),
        "Flip Horizontally" | "Flip Vertically" => app.canvas.obj.is_some_and(|o| !is_table(app, o)),
        "Lock" => app.canvas.obj.is_some_and(|o| !app.drawables.iter().any(|d| d.id == o && d.locked)),
        "Unlock" => app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && d.locked)),
        n if ["Baseline > ", "Capitalization > ", "Character Spacing > ", "Ligature > "].iter().any(|p| n.starts_with(p)) => {
            cells || app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(&d.kind, DrawableKind::Shape(s) if s.text.is_some())))
        }
        "Font > Bold" | "Font > Italic" | "Font > Underline" | "Font > Strikethrough" | "Font > Bigger" | "Font > Smaller" | "Text > Align Left" | "Text > Align Center" | "Text > Align Right" | "Text > Justify" => {
            cells || app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(&d.kind, DrawableKind::Shape(s) if s.text.is_some())))
        }
        "Table > Merge Cells" | "Autofill Cells > Autofill" => app.sel.is_some_and(|s| s.anchor != s.cursor) && !app.sel.is_some_and(|s| app.categorized(s.table)),
        "Autofill Cells > Autofill Right" | "Autofill Cells > Autofill Left" => app.sel.is_some_and(|s| s.anchor.1 != s.cursor.1 && !app.categorized(s.table)),
        "Autofill Cells > Autofill Down" | "Autofill Cells > Autofill Up" => app.sel.is_some_and(|s| s.anchor.0 != s.cursor.0 && !app.categorized(s.table)),
        "Autofill Cells > Autofill from Row Above" => app.sel.is_some_and(|s| s.block().0 > 0 && !app.categorized(s.table)),
        "Autofill Cells > Autofill from Column Before" => app.sel.is_some_and(|s| s.block().1 > 0 && !app.categorized(s.table)),
        n if n.starts_with("Select > ") && n.contains("Value") => cells,
        n if n.starts_with("Transformations > ") => cells,
        "Table > Resize Row to Fit Content" | "Table > Resize Column to Fit Content" | "Table > Distribute Rows Evenly" | "Table > Distribute Columns Evenly" => selected_table(app).is_some(),
        "Comments > Show Next Comment" | "Comments > Show Previous Comment" => any_comments(app),
        "Find > Use Selection for Find" | "Find > Use Selection for Replace" | "Find > Jump to Selection" => cells,
        "Copied Rows" | "Copied Columns" => app.sel.is_some_and(|s| !app.categorized(s.table)) && app.cell_clip.as_ref().is_some_and(|c| c.whole == Some(name == "Copied Rows")),
        "Unhide > Unhide All Sheets" => app.sheets().iter().any(|s| s.hidden),
        "Table > Unmerge Cells" | "Table > Add Row Above" | "Table > Add Row Below" | "Table > Delete Row" => app.sel.is_some_and(|s| !app.categorized(s.table)),
        "Table > Add Column Before" | "Table > Add Column After" | "Table > Delete Column" | "Table > Hide Column" => cells,
        "Table > Hide Row" => app.sel.is_some_and(|s| !app.categorized(s.table)),
        "Table > Unhide All Rows" => selected_table(app).is_some_and(|t| app.wb.as_ref().is_some_and(|w| !nm_core::hidden(w.file(), t).0.is_empty())),
        "Table > Unhide All Columns" => selected_table(app).is_some_and(|t| app.wb.as_ref().is_some_and(|w| !nm_core::hidden(w.file(), t).1.is_empty())),
        "Show Previous Sheet" | "Show First Sheet" => app.sheet > 0,
        "Show Next Sheet" | "Show Last Sheet" => app.sheet + 1 < app.sheets().len(),
        n if n.starts_with("Header Rows > ") || n.starts_with("Header Columns > ") || n.starts_with("Footer Rows > ") => selected_table(app).is_some(),
        n if n.starts_with("Formula > ") => cells,
        n if n.starts_with("Chart Type > ") => selected_chart(app).is_some(),
        "Chart > Edit Chart Data…" => selected_chart(app).is_some(),
        n if n.starts_with("Align Objects > ") => app.selected_objs().len() >= 2,
        // Numbers won't group a table (Mac N22 G5: greyed)
        "Group" => app.selected_objs().len() >= 2 && !app.selected_objs().iter().any(|o| is_table(app, *o)),
        "Ungroup" => app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(d.kind, DrawableKind::Group(_)))),
        "Select Parent" => cells,
        "Remove Highlights and Comments" => any_comments(app),
        "Revert To > Browse All Versions…" | "Move To…" => app.doc().is_some_and(|d| d.path.is_some()),
        "Show Adjust Image" => crate::mask::selected_image(app).is_some(),
        "Show Arrange Tools" => obj,
        "Show Colors" => cells || obj,
        "Text > Increase Indent Level" | "Text > Decrease Indent Level" => app.canvas.obj.is_some_and(|o| app.doc().and_then(|d| d.storage_of(o)).is_some()),
        n if n.starts_with("Sheet > Hide ") => app.sheets().iter().filter(|s| !s.hidden).count() > 1,
        "Shapes and Lines > Make Editable" => app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(&d.kind, DrawableKind::Shape(s) if !s.is_text_box))),
        "Shapes and Lines > Break Apart" => app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(&d.kind, DrawableKind::Shape(s) if !s.is_line && !s.is_text_box))),
        n if n.starts_with("Shapes and Lines > ") && n.ends_with(" Shapes") => {
            app.selected_objs().iter().filter(|o| app.drawables.iter().any(|d| d.id == **o && matches!(&d.kind, DrawableKind::Shape(s) if !s.is_line))).count() >= 2
        }
        "Shapes and Lines > Reset Text and Object Handles" => app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(d.kind, DrawableKind::Shape(_)))),
        n if n.starts_with("Shapes and Lines > Make ") && n.ends_with(" Point") => app.points.selected.is_some(),
        n if n.starts_with("Add Link > ") || n == "Remove Link" => crate::links::enabled(app, n),
        "Table > Freeze Header Rows" | "Table > Freeze Header Columns" => selected_table(app).is_some(),
        "Table > Wrap Text" => cells,
        n if n.starts_with("Distribute Objects > ") => app.selected_objs().len() >= 3,
        n if n.strip_prefix("Chart > ").is_some_and(|l| CHART_TOGGLES.contains(&l)) => {
            let l = n.trim_start_matches("Chart > ");
            // the second value axis only on a 2-axis chart
            chart_data(app).is_some_and(|c| !l.contains("Axis 2") || c.value_axes.len() > 1)
        }
        _ => has_doc || matches!(name, "New > Numbers Spreadsheet" | "New > Excel Workbook" | "Open…" | "About Values" | "Support Development…" | "Quit Values" | "Keyboard Shortcuts" | "Settings…"),
    }
}

/// Ticks for the items that are on.
fn checked(app: &App, name: &str) -> Option<bool> {
    let t = selected_table(app);
    for (prefix, kind) in [("Header Rows > ", HeaderKind::Rows), ("Header Columns > ", HeaderKind::Columns), ("Footer Rows > ", HeaderKind::Footer)] {
        if let Some(n) = name.strip_prefix(prefix) {
            let cur = t.and_then(|t| app.doc().map(|d| d.table_header_count(t, kind)));
            return Some(cur.map(|c| c.to_string()) == Some(n.to_string()));
        }
    }
    if let Some(l) = name.strip_prefix("Chart > ").filter(|l| CHART_TOGGLES.contains(l)) {
        return chart_data(app).map(|c| chart_shows(c, l));
    }
    if let Some(ty) = name.strip_prefix("Chart Type > ").and_then(chart_enum) {
        return chart_data(app).map(|c| c.chart_type == ty);
    }
    match name {
        "Single File" => return app.doc().map(|d| !d.package),
        "Package" => return app.doc().map(|d| d.package),
        _ => {}
    }
    let st = &app.settings;
    match name {
        "Substitutions > Smart Quotes" => return Some(st.smart_quotes),
        "Substitutions > Smart Dashes" => return Some(st.smart_dashes),
        "Substitutions > Text Replacement" => return Some(st.text_replacement),
        "Substitutions > Smart Web and Email Links" => return Some(st.smart_links),
        "Substitutions > Smart Phone Number Links" => return Some(st.smart_phone_links),
        "Spelling and Grammar > Check Spelling While Typing" => return Some(st.spell_check),
        "Spelling and Grammar > Check Grammar With Spelling" => return Some(st.grammar),
        "Spelling and Grammar > Correct Spelling Automatically" => return Some(st.autocorrect),
        _ => {}
    }
    if let Some(c) = name.strip_prefix("Author Color > ") {
        return Some(crate::settings::AUTHOR_COLORS.get(app.settings.author_color as usize).is_some_and(|(n, _)| *n == c));
    }
    match name {
        "Table > Freeze Header Rows" | "Table > Freeze Header Columns" => {
            return t.and_then(|t| app.doc().map(|d| nm_core::sheets::headers_frozen(d, t, name.ends_with("Rows"))));
        }
        "Table > Wrap Text" => return app.sel.map(|_| cursor_wraps(app)),
        _ => {}
    }
    match name {
        "Inspector > Format" => Some(app.inspector && app.insp_tab != InspTab::Organize),
        "Inspector > Organize" => Some(app.inspector && app.insp_tab == InspTab::Organize),
        _ => None,
    }
}

/// A menu's items, scrolling when the menu is taller than the room down to the window's
/// bottom edge (Keydeck's menu_body).
fn menu_body(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let win: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(egui::Id::unique("nm-window-rect")));
    let top = ui.max_rect().min.y;
    let room = win.map(|w| (w.max.y - top - 10.0).min(w.height() - 20.0)).unwrap_or(f32::INFINITY).max(120.0);
    let key = ui.scope_id().with("menu-height");
    let full: f32 = ui.ctx().data(|d| d.get_temp(key)).unwrap_or(0.0);
    if full <= room {
        let before = ui.min_rect().height();
        add(ui);
        let h = ui.min_rect().height() - before;
        ui.ctx().data_mut(|d| d.insert_temp(key, h));
    } else {
        let out = egui::ScrollArea::vertical().max_height(room).min_scrolled_height(room).auto_shrink([true, false]).show(ui, add);
        ui.ctx().data_mut(|d| d.insert_temp(key, out.content_size.y));
    }
}

/// The icon Numbers shows next to a menu item (Keydeck's set: Numbers and Keynote share them).
fn icon(label: &str) -> Option<kn_ui::icons::Icon> {
    kn_ui::icons::menu_icon(label).or_else(|| kn_ui::icons::menu_icon(label.trim_end_matches('…')))
}

/// Width of the icon column in menus (Keydeck's: a 16 pt glyph plus a little air).
const MENU_ICON_W: f32 = 20.0;

fn paint_icon(ui: &egui::Ui, r: egui::Rect, i: kn_ui::icons::Icon, on: bool) {
    let x = r.min.x + ui.spacing().button_padding.x + MENU_ICON_W / 2.0;
    kn_ui::icons::paint_menu_icon(ui, egui::pos2(x, r.center().y), i, on);
}

/// A label with room for the icon column when the menu has icons (so the labels line up).
fn atoms(label: String, indent: bool) -> egui::Atoms<'static> {
    use egui::IntoAtoms;
    if indent { (egui::Atom::custom(egui::IdSalt::new("menu-icon"), egui::vec2(MENU_ICON_W, 16.0)), label).into_atoms() } else { label.into_atoms() }
}

/// Whether any of `items` has an icon.
fn has_icons(items: &[Item]) -> bool {
    items.iter().any(|c| icon(&c.label).is_some())
}

fn item_ui(app: &mut App, ui: &mut egui::Ui, parent: &str, item: &Item, indent: bool, fired: &mut Option<String>) {
    if item.separator {
        ui.separator();
        return;
    }
    // (the menu file lists Open Recent with no items: its items are the recent files)
    if item.children.is_empty() && item.label == "Open Recent" {
        let fake = Item { children: vec![Item { label: String::new(), ..item.clone() }], ..item.clone() };
        return item_ui(app, ui, parent, &fake, indent, fired);
    }
    if !item.children.is_empty() {
        if item.label == "Open Recent" {
            let recent = app.recent();
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button(atoms("Open Recent".into(), indent), |ui| {
                    for p in recent {
                        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                            *fired = Some(format!("open:{}", p.display()));
                            ui.close();
                        }
                    }
                });
            });
            return;
        }
        let kids = has_icons(&item.children);
        let r = ui.menu_button(atoms(item.label.clone(), indent), |ui| {
            menu_body(ui, |ui| {
                for c in &item.children {
                    item_ui(app, ui, &item.label, c, kids, fired);
                }
            })
        });
        if let Some(i) = icon(&item.label) {
            paint_icon(ui, r.response.rect, i, true);
        }
        return;
    }
    let name = command_name(parent, &item.label);
    // labels that follow the state ("Hide Inspector" / "Show Inspector")
    let label = match name.as_str() {
        "Inspector > Hide Inspector" if !app.inspector => "Show Inspector".to_string(),
        "Comments > Hide Comments" if !app.show_comments => "Show Comments".to_string(),
        "Show Rulers" if app.show_rulers => "Hide Rulers".to_string(),
        "Guides > Hide Guides" if !app.show_guides => "Show Guides".to_string(),
        "Hide Toolbar" if !app.show_toolbar => "Show Toolbar".to_string(),
        "Find > Hide Find Panel" if app.find.is_none() => "Show Find Panel".to_string(),
        "Undo" => app.hist.undo_label().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into()),
        "Redo" => app.hist.redo_label().map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into()),
        "Lock" | "Unlock" => item.label.clone(),
        n if crate::organize::is_menu(n) => crate::organize::label(app, n).unwrap_or_else(|| item.label.clone()),
        _ => item.label.clone(),
    };
    let mut b = egui::Button::new(atoms(label, indent));
    let shortcut = match name.as_str() {
        "Font > Bigger" | "Font > Smaller" => None,
        "Zoom > Zoom In" => Some("Ctrl++".to_string()),
        "Zoom > Zoom Out" => Some("Ctrl+-".to_string()),
        _ => item.shortcut.clone(),
    };
    if let Some(s) = &shortcut {
        b = b.shortcut_text(s);
    }
    if checked(app, &name) == Some(true) {
        b = b.selected(true);
    }
    let on = enabled(app, &name);
    let r = ui.add_enabled(on, b);
    if let Some(i) = icon(&item.label) {
        paint_icon(ui, r.rect, i, on);
    }
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
        for m in menus() {
            // the app menu shows a grid mark instead of its name (as Keydeck shows its own)
            let app_menu = m.label == "Values";
            let label = if app_menu { "\u{2003}\u{2002}".to_string() } else { m.label.clone() };
            let r = ui.menu_button(label, |ui| {
                menu_body(ui, |ui| {
                    let kids = has_icons(&m.children);
                    for c in &m.children {
                        item_ui(app, ui, &m.label, c, kids, &mut fired);
                    }
                })
            });
            if app_menu {
                let b = r.response.rect;
                kn_ui::icons::draw(ui.painter(), egui::Rect::from_center_size(b.center() + egui::vec2(0.0, 1.0), egui::vec2(20.0, 20.0)), kn_ui::icons::Icon::Table, kn_ui::theme::TEXT());
            }
            right = right.max(r.response.rect.max.x);
        }
    });
    if let Some(cmd) = fired {
        run(app, ui.ctx(), &cmd);
    }
    right
}

/// Every shortcut in the menus: (shortcut, command), longest modifier sets first so that
/// Ctrl+Shift+Z isn't taken by Ctrl+Z.
fn shortcut_table() -> &'static [(KeyboardShortcut, String, String)] {
    static T: OnceLock<Vec<(KeyboardShortcut, String, String)>> = OnceLock::new();
    T.get_or_init(|| {
        let mut out = Vec::new();
        fn walk(items: &[Item], parent: &str, out: &mut Vec<(KeyboardShortcut, String, String)>) {
            for it in items {
                if !it.children.is_empty() {
                    walk(&it.children, &it.label, out);
                } else if let Some(s) = &it.shortcut {
                    if let Some(k) = keyboard(s) {
                        out.push((k, command_name(parent, &it.label), s.clone()));
                    }
                }
            }
        }
        for m in menus() {
            walk(&m.children, &m.label, &mut out);
        }
        out.sort_by_key(|(k, _, _)| std::cmp::Reverse(k.modifiers.shift as u8 + k.modifiers.alt as u8 + k.modifiers.command as u8));
        out
    })
}

/// While typing in a cell or a text field these stay with the text (copy, select all, …).
const TEXT_KEEPS: &[&str] = &["Cut", "Copy", "Paste", "Select All", "Deselect All", "Undo", "Redo", "Delete", "Font > Bigger", "Font > Smaller"];

/// Keyboard shortcuts of the menus (run before the canvas's own keys).
pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    let typing = app.canvas.edit.is_some() || app.canvas.editing.is_some() || ctx.egui_wants_keyboard_input();
    // Ctrl+Plus / Ctrl+= / Ctrl+Minus zoom the sheet (here they aren't Bigger / Smaller)
    if !typing {
        let z = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        if ctx.input_mut(|i| i.consume_shortcut(&z(Key::Plus)) | i.consume_shortcut(&z(Key::Equals))) {
            run(app, ctx, "Zoom > Zoom In");
        }
        if ctx.input_mut(|i| i.consume_shortcut(&z(Key::Minus))) {
            run(app, ctx, "Zoom > Zoom Out");
        }
    }
    for (k, name, _) in shortcut_table() {
        if matches!(name.as_str(), "Font > Bigger" | "Font > Smaller") {
            continue;
        }
        // keys the canvas handles itself (arrows, Delete, copy / paste events)
        if matches!(name.as_str(), "Cut" | "Copy" | "Paste" | "Delete" | "Undo" | "Redo") {
            continue;
        }
        if typing && TEXT_KEEPS.contains(&name.as_str()) {
            continue;
        }
        if typing && k.modifiers.is_none() {
            continue;
        }
        if !enabled(app, name) {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(k)) {
            run(app, ctx, name);
        }
    }
}

/// Help > Keyboard Shortcuts: every menu shortcut, by menu.
pub fn shortcuts_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_shortcuts {
        return;
    }
    let mut open = true;
    egui::Window::new("Keyboard Shortcuts").open(&mut open).default_size([460.0, 560.0]).show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            fn walk(ui: &mut egui::Ui, items: &[Item], parent: &str) {
                for it in items {
                    if !it.children.is_empty() {
                        walk(ui, &it.children, &it.label);
                    } else if let Some(s) = &it.shortcut {
                        ui.horizontal(|ui| {
                            ui.label(command_name(parent, &it.label));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(s).monospace()));
                        });
                    }
                }
            }
            for m in menus() {
                let mut any = false;
                fn has(items: &[Item]) -> bool {
                    items.iter().any(|i| i.shortcut.is_some() || has(&i.children))
                }
                any |= has(&m.children);
                if any {
                    ui.add_space(6.0);
                    ui.label(kn_ui::theme::bold(&m.label));
                    walk(ui, &m.children, &m.label);
                }
            }
            ui.add_space(6.0);
            ui.label(kn_ui::theme::bold("Cells"));
            for (k, what) in [
                ("Return / Shift+Return", "Commit, move down / up"),
                ("Tab / Shift+Tab", "Commit, move right / left"),
                ("Alt+Return", "Line break in the cell"),
                ("Alt+arrows", "Add a row / column next to the cell"),
                ("Ctrl+arrows", "Go to the table's edge"),
                ("Shift+arrows", "Extend the selection"),
                ("F2", "Edit the cell"),
                ("Escape", "Cancel the edit"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(what);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(k).monospace()));
                });
            }
            ui.add_space(6.0);
            ui.label(kn_ui::theme::bold("Objects"));
            for (k, what) in [("Delete / Backspace", "Delete the object"), ("Arrows / Shift+arrows", "Move 1 / 10 pt"), ("Escape", "Deselect")] {
                ui.horizontal(|ui| {
                    ui.label(what);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(k).monospace()));
                });
            }
        });
    });
    app.show_shortcuts = open;
}

fn cell_blocks(app: &App) -> Option<(nm_core::kn_format::ObjId, Vec<(u32, u32, u32, u32)>)> {
    let sel = app.sel?;
    Some((sel.table, app.model_blocks(sel.table, sel.block())))
}

/// Character / paragraph formatting of the selected cells or text object.
fn text_prop(app: &mut App, label: &str, prop: TextProp) {
    // text being edited in place: the selected characters (else the whole text), editing on
    if app.canvas.editing.is_some() {
        crate::editing::set_text(app, label, prop);
        return;
    }
    commit(app);
    if let Some((t, blocks)) = cell_blocks(app) {
        if let Some(Err(e)) = app.edit(label, |d, h| blocks.iter().try_for_each(|b| d.set_cells_text_prop(h, t, *b, &prop).map(|_| ()))) {
            app.notify(format!("{label}: {e}"));
        }
    } else if let Some(id) = app.canvas.obj {
        app.edit(label, |d, h| d.set_text_prop(h, id, &prop));
    }
}

/// The selected cell's or object's character style (for Bold etc. to toggle).
fn current_char(app: &App) -> Option<nm_core::kn_model::CharStyle> {
    // the character at the start of the text selection being edited
    if let Some(e) = app.canvas.editing.as_ref() {
        let at = e.cursor.min(e.anchor);
        return app.drawables.iter().find(|d| d.id == e.id).and_then(|d| match &d.kind {
            DrawableKind::Shape(s) => s.text.as_ref().and_then(|t| t.paragraphs.iter().flat_map(|p| p.runs.iter()).find(|r| r.start <= at && at < r.end.max(r.start + 1)).map(|r| r.style.clone())),
            _ => None,
        });
    }
    if let Some(sel) = app.sel {
        let (_, t) = app.table(sel.table)?;
        return t.cell_styles.get((sel.cursor.0 * t.cols + sel.cursor.1) as usize).map(|s| s.text.clone());
    }
    let id = app.canvas.obj?;
    app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
        DrawableKind::Shape(s) => s.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.style.clone()),
        _ => None,
    })
}

pub fn run(app: &mut App, ctx: &egui::Context, name: &str) {
    if crate::organize::is_menu(name) {
        crate::canvas::commit(app);
        crate::organize::run(app, name);
        return;
    }
    if crate::pivotui::MENU.contains(&name) {
        crate::pivotui::run(app, name);
        return;
    }
    match name {
        // Insert > Line Break / Column Break: in the text being edited (a cell takes a new line)
        "Line Break" | "Column Break" => {
            let ins = if name == "Line Break" { "\u{2028}" } else { "\u{000C}" };
            if !crate::textedit::insert_at_caret(app, ins) && !(name == "Line Break" && crate::canvas::cell_insert(app, ctx, "\n")) {
                app.notify(format!("Click into text to insert a {}", name.to_lowercase()));
            }
            return;
        }
        // Insert > Date & Time: today's date and time at the caret, or in the selected cell
        "Date & Time" => {
            let now = nm_core::typed::today() + {
                let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                secs.rem_euclid(86_400.0)
            };
            let text = nm_core::kn_model::table::format_date(now, "MMMM d, y h:mm a");
            if crate::textedit::insert_at_caret(app, &text) || crate::canvas::cell_insert(app, ctx, &text) {
                return;
            }
            if app.sel.is_some() {
                crate::canvas::start_edit(app, Some(nm_core::kn_model::table::format_date(now, "M/d/yy h:mm a")));
                commit(app);
            }
            return;
        }
        // Insert > Highlight: the selected text marked in your author colour (a review highlight)
        "Highlight" => {
            let Some((storage, a, b)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor))).filter(|(_, a, b)| a < b) else {
                app.notify("Select some text to highlight");
                return;
            };
            let who = if app.settings.author.is_empty() { "Values".to_string() } else { app.settings.author.clone() };
            let rgb = crate::settings::AUTHOR_COLORS[app.settings.author_color as usize % 10].1;
            let c = nm_core::kn_model::Color { r: ((rgb >> 16) & 0xff) as f32 / 255.0, g: ((rgb >> 8) & 0xff) as f32 / 255.0, b: (rgb & 0xff) as f32 / 255.0, a: 1.0 };
            if let Some(w) = app.wb.as_mut() {
                let ok = w.doc.add_highlight(&mut app.hist, storage, a, b, &who).is_some();
                w.doc.set_author_color(&mut app.hist, &who, c);
                if !ok {
                    app.notify("Couldn't highlight that text");
                }
            }
            if let Some(e) = app.canvas.editing.as_mut() {
                e.version += 1;
            }
            app.changed();
            return;
        }
        "Table > Transpose Rows and Columns" => {
            if let Some(t) = selected_table(app) {
                crate::clip::transpose_table(app, t);
            }
            return;
        }
        "Advanced > Set as Default Text Box Appearance" => {
            match (app.canvas.obj, app.doc().map(|d| d.file.clone())) {
                (Some(id), Some(k)) if app.doc().and_then(|d| d.storage_of(id)).is_some() => {
                    app.default_textbox = Some((k, id));
                    app.notify("New text boxes will look like this one");
                }
                _ => app.notify("Select a text box first"),
            }
            return;
        }
        "Advanced > Create Table Style" => {
            if let Some(t) = selected_table(app) {
                app.edit("Create Table Style", |d, h| {
                    let _ = d.save_table_style_preset(h, t);
                });
                app.notify("Added to the table styles (Format > Table)");
            }
            return;
        }
        "Shapes and Lines > Toggle Line Type" => {
            use nm_core::kn_model::insert::Connection;
            let Some(id) = app.canvas.obj else { return };
            let Some(m) = app.doc().and_then(|d| d.file.obj(id)) else { return };
            if app.doc().map(|d| d.file.type_name(id)) != Some("TSD.ConnectionLineArchive") {
                app.notify("Select a connection line");
                return;
            }
            let ends = m.ref_id("connected_from").zip(m.ref_id("connected_to"));
            let c = m.find_super("TSD.ShapeArchive").and_then(|s| s.msg("pathsource")).and_then(|p| p.msg("connection_line_path_source"));
            let angled = c.and_then(|c| c.enum_name("type")) == Some("kTSDConnectionLineTypeOrthogonal");
            let pts: Vec<(f32, f32)> = c.and_then(|c| c.msg("super")).and_then(|b| b.msg("path")).map(|p| p.msgs("elements").filter_map(|e| e.msg("points")).map(|q| (q.f32("x").unwrap_or(0.0), q.f32("y").unwrap_or(0.0))).collect()).unwrap_or_default();
            let curved = matches!(pts.as_slice(), [a, m, b] if ((m.0 - (a.0 + b.0) / 2.0).abs() + (m.1 - (a.1 + b.1) / 2.0).abs()) > 1.0);
            // straight -> curved -> angled -> straight
            let next = if angled { Connection::Straight } else if curved { Connection::Angled } else { Connection::Curved };
            app.edit("Line Type", |d, h| {
                let _ = d.make_connection_line(h, id, next, ends);
            });
            return;
        }
        "Shapes and Lines > Save to My Shapes" => {
            crate::myshapes::save(app);
            return;
        }
        "Advanced > Language & Region…" => {
            let cur = app.doc().and_then(nm_core::sheets::document_locale).unwrap_or_else(|| "en_US".into());
            let (l, r) = cur.split_once('_').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or(("en".into(), "US".into()));
            app.language_edit = Some((l, r));
            return;
        }
        // Edit > Speech: the text being edited (its selection), else the selected cells or object
        "Start Speaking" => {
            let mut text = String::new();
            if let Some((st, a, b)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor))) {
                let t = app.doc().map(|d| d.storage_text(st)).unwrap_or_default();
                text = if a < b { t.get(a..b).unwrap_or("").to_string() } else { t };
            } else if let Some(sel) = app.sel {
                if let Some((_, t)) = app.table(sel.table) {
                    let (r0, c0, r1, c1) = sel.block();
                    for r in r0..=r1 {
                        for c in c0..=c1 {
                            if let Some(v) = t.cells.get((r * t.cols + c) as usize).filter(|v| !v.trim().is_empty()) {
                                text.push_str(v);
                                text.push_str(". ");
                            }
                        }
                    }
                }
            } else if let Some(st) = app.canvas.obj.and_then(|o| app.doc().and_then(|d| d.storage_of(o))) {
                text = app.doc().map(|d| d.storage_text(st)).unwrap_or_default();
            }
            let text = text.replace(['\u{2028}', '\u{fffc}'], " ");
            if !text.trim().is_empty() && std::process::Command::new("spd-say").arg("--").arg(&text).stdin(std::process::Stdio::null()).spawn().is_err() {
                app.notify("Speech needs speech-dispatcher (spd-say)");
            }
            return;
        }
        "Stop Speaking" => {
            let _ = std::process::Command::new("spd-say").arg("--cancel").spawn();
            return;
        }
        "Equation..." => {
            app.equation = crate::equation::EquationDialog { open: true, ..Default::default() };
            return;
        }
        "Web Video…" => {
            app.media.url_sheet = Some(String::new());
            return;
        }
        "Record Audio" => {
            app.media.record_open = true;
            return;
        }
        "Image Gallery" => {
            crate::media::pick_gallery(app, None);
            return;
        }
        "Show Music Browser" => {
            app.media.open = Some(true);
            return;
        }
        "Show Photo and Video Browser" => {
            app.media.open = Some(false);
            return;
        }
        "Save as Template…" => {
            app.templates.saving = Some(app.title().trim_end_matches(".numbers").to_string());
            return;
        }
        "Line > Draw With Pen" => {
            commit(app);
            app.sel = None;
            app.canvas.obj = None;
            app.pen = Some(Vec::new());
            return;
        }
        "Show Warnings" => {
            crate::fonts::open_warnings(app);
            return;
        }
        "Formula > Edit Formula" => {
            let start = app.sel.and_then(|s| app.mrow(s.table, s.cursor.0).and_then(|r| app.formula_of(s.table, r, s.cursor.1)));
            crate::canvas::start_edit(app, Some(start.unwrap_or_else(|| "=".into())));
            return;
        }
        "Show Rulers" => {
            app.show_rulers = !app.show_rulers;
            return;
        }
        "Guides > Hide Guides" => {
            app.show_guides = !app.show_guides;
            return;
        }
        "Guides > Clear All Guides on Sheet" => {
            if let Some(s) = app.sheets().get(app.sheet).map(|s| s.id) {
                app.edit("Clear Guides", |d, h| d.set_guides(h, s, &[]));
            }
            return;
        }
        _ => {}
    }
    if let Some(p) = name.strip_prefix("open:") {
        app.open(std::path::Path::new(p));
        return;
    }
    if app.excel_only() && excel_blocks(name) {
        app.notify("An Excel workbook can't hold that. Click the document's name to keep Values features in the workbook, or to make it a Numbers spreadsheet.");
        return;
    }
    let sheet = app.sheets().get(app.sheet).map(|s| s.id);
    if let Some(l) = name.strip_prefix("Table > ") {
        if let Some(i) = TABLE_ITEMS.iter().position(|n| *n == l) {
            app.insert_table(i);
            return;
        }
    }
    if let Some(l) = name.strip_prefix("Chart > ") {
        if let Some(ty) = chart_enum(l) {
            // the nearest chart we have a Numbers-made donor for, then its type set
            let donor = chart_index(l).or_else(|| ty.starts_with("pie").then(|| chart_index("2D Pie")).flatten()).unwrap_or(0);
            app.insert_chart(donor, Some(ty));
            return;
        }
        if CHART_TOGGLES.contains(&l) {
            chart_toggle(app, l);
            return;
        }
        if l == "Edit Chart Data…" {
            if let Some(id) = selected_chart(app) {
                if crate::datarefs::linked(app, id).is_some() {
                    crate::datarefs::start(app, id);
                } else {
                    crate::chartedit::open(app, id);
                }
            }
            return;
        }
    }
    if let Some(ty) = name.strip_prefix("Chart Type > ").and_then(chart_enum) {
        let Some(id) = selected_chart(app) else { return };
        app.edit("Chart Type", |d, h| d.set_chart_type(h, id, ty));
        return;
    }
    if let Some((_, i)) = name.strip_prefix("Shape > ").and_then(|l| SHAPE_ITEMS.iter().find(|(n, _)| *n == l)) {
        app.insert_shape(*i);
        return;
    }
    if let Some(b) = name.strip_prefix("Shape > ").and_then(preset_shape) {
        // Numbers' square, given the preset's outline
        app.insert_shape_then(3, |d, h, id| {
            nm_core::sheets::set_basic_shape(d, h, id, b);
        });
        return;
    }
    if name == "Formula > Stock Quote" {
        crate::stocks::open(app);
        return;
    }
    if let Some((_, f)) = name.strip_prefix("Formula > ").and_then(|l| FORMULA_ITEMS.iter().find(|(n, _)| *n == l)) {
        crate::canvas::start_edit(app, Some(format!("={f}(")));
        return;
    }
    for (prefix, kind) in [("Header Rows > ", HeaderKind::Rows), ("Header Columns > ", HeaderKind::Columns), ("Footer Rows > ", HeaderKind::Footer)] {
        if let Some(n) = name.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok()) {
            let Some(t) = selected_table(app) else { return };
            commit(app);
            if let Some(Err(e)) = app.edit(prefix.trim_end_matches(" > "), |d, h| d.set_table_header_count(h, t, kind, n)) {
                app.notify(format!("{e}"));
            }
            return;
        }
    }
    let table_edit = |app: &mut App, label: &str, f: &dyn Fn(&mut nm_core::kn_model::Document, &mut nm_core::kn_model::edit::History, nm_core::kn_format::ObjId, (u32, u32, u32, u32)) -> Result<(), nm_core::kn_format::Error>| {
        let Some(sel) = app.sel else { return };
        commit(app);
        let b = sel.block();
        // rows go by the model's rows (categories show them in another order)
        let mb = (app.mrow(sel.table, b.0).unwrap_or(b.0), b.1, app.mrow(sel.table, b.2).unwrap_or(b.2), b.3);
        if let Some(Err(e)) = app.edit(label, |d, h| f(d, h, sel.table, mb)) {
            app.notify(format!("{label}: {e}"));
        }
    };
    match name {
        "About Values" => app.show_about = true,
        "Support Development…" => kn_ui::open_url(kn_ui::DONATE_URL),
        "Settings…" => app.show_settings = true,
        "Quit Values" | "Close" | "Close All" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        // Numbers' template chooser (Blank and My Templates)
        "New > Numbers Spreadsheet" => app.templates.chooser = true,
        // a new workbook in Excel mode (Save asks where to put the .xlsx)
        "New > Excel Workbook" => {
            app.new_document();
            app.excel = Some(crate::app::ExcelMode { path: std::path::PathBuf::new(), compat: false, extras: Default::default(), by_sheet: Default::default() });
        }
        "Open…" => app.open_dialog(),
        "Save" => {
            commit(app);
            app.save(false);
        }
        "Save As…" | "Duplicate" | "Rename…" => {
            commit(app);
            app.save(true);
        }
        "Export To > PDF…" => app.export_pdf_dialog(),
        "Print…" => app.print_setup_open = true,
        "Export To > CSV…" => app.export_csv_dialog(),
        "Export To > Excel…" => app.export_xlsx_dialog(),
        "Export To > TSV…" => app.export_tsv_dialog(),
        "Set Password…" => app.open_set_password(),
        "Undo" => {
            if app.canvas.edit.is_some() {
                app.canvas.edit = None;
            } else {
                app.undo();
            }
        }
        "Redo" => app.redo(),
        "Cut" | "Copy" => {
            if app.canvas.obj.is_some() {
                app.copy_objects();
                if name == "Cut" {
                    app.delete_objects();
                }
            } else {
                crate::canvas::copy(app, ctx, name == "Cut");
            }
        }
        "Paste" => {
            if app.obj_clip_active && app.clip.is_some() {
                app.paste_here();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
        }
        "Paste and Match Style" => {
            crate::clip::paste(app, crate::clip::PasteMode::MatchStyle);
        }
        "Paste Formula Results" => {
            crate::clip::paste(app, crate::clip::PasteMode::Results);
        }
        "Paste and Preserve Style" => {
            crate::clip::paste(app, crate::clip::PasteMode::All);
        }
        "Delete" => {
            if app.canvas.obj.is_some() {
                app.delete_objects();
            } else {
                crate::canvas::clear_cells(app);
            }
        }
        "Clear All" => {
            crate::canvas::clear_cells(app);
            // and the cells' formatting
            if let Some((t, blocks)) = cell_blocks(app) {
                app.edit("Clear All", |d, h| blocks.iter().try_for_each(|b| d.set_cells_format(h, t, *b, &nm_core::kn_model::tablefmt::CellFormat::Automatic).map(|_| ())));
            }
        }
        "Duplicate Selection" => app.duplicate_selection(),
        "Select All" => {
            if app.sel.is_some() {
                crate::canvas::select_all(app);
            }
        }
        "Deselect All" => {
            commit(app);
            app.sel = None;
            app.canvas.obj = None;
        }
        "Edit Formula" => {
            let start = app.sel.and_then(|s| app.mrow(s.table, s.cursor.0).and_then(|r| app.formula_of(s.table, r, s.cursor.1)));
            crate::canvas::start_edit(app, Some(start.unwrap_or_else(|| "=".into())));
        }
        "Find > Find…" => {
            commit(app);
            app.find = Some((String::new(), Vec::new(), 0));
        }
        "Find > Find Next" | "Find > Find Previous" => app.find_step(name == "Find > Find Next"),
        "Find > Hide Find Panel" => {
            app.find = if app.find.is_some() { None } else { Some((String::new(), Vec::new(), 0)) };
        }
        "Sheet" => app.add_sheet(),
        "Text" => app.insert_text_box(),
        "Line > Line" => app.insert_shape(0),
        // a connection line: joining the two selected objects (re-routed as they move), else free
        "Line > Straight Connection Line" | "Line > Curved Connection Line" | "Line > Angled Connection Line" => {
            use nm_core::kn_model::insert::Connection;
            let kind = match name {
                "Line > Curved Connection Line" => Connection::Curved,
                "Line > Angled Connection Line" => Connection::Angled,
                _ => Connection::Straight,
            };
            let sel = app.selected_objs();
            let ends = (sel.len() == 2).then(|| (sel[0], sel[1]));
            app.insert_shape_then(0, |d, h, id| {
                let _ = d.make_connection_line(h, id, kind, ends);
            });
        }
        "Line > Line with Arrowhead" | "Line > Line with Two Arrowheads" => {
            let both = name.ends_with("Two Arrowheads");
            app.insert_shape_then(0, |d, h, id| {
                let arrow = nm_core::kn_model::insert::arrow_line_end();
                d.set_line_ends(h, id, both.then_some(&arrow), Some(&arrow));
            });
        }
        "Comment" => app.add_comment(),
        "Choose File" => crate::media::choose(app),
        "Browse Content Hub" | "Show Content Hub" => app.hub.show_hub(),
        "Table > Add Row Above" => table_edit(app, "Add Row", &|d, h, t, b| crate::clip::insert_like(d, h, t, Axis::Row, b.0, b.0)),
        "Table > Add Row Below" => table_edit(app, "Add Row", &|d, h, t, b| crate::clip::insert_like(d, h, t, Axis::Row, b.2 + 1, b.2)),
        "Table > Add Column Before" => table_edit(app, "Add Column", &|d, h, t, b| crate::clip::insert_like(d, h, t, Axis::Column, b.1, b.1)),
        "Table > Add Column After" => table_edit(app, "Add Column", &|d, h, t, b| crate::clip::insert_like(d, h, t, Axis::Column, b.3 + 1, b.3)),
        "Table > Delete Row" => table_edit(app, "Delete Row", &|d, h, t, b| (b.0..=b.2).rev().try_for_each(|r| d.table_delete(h, t, Axis::Row, r))),
        "Table > Delete Column" => table_edit(app, "Delete Column", &|d, h, t, b| (b.1..=b.3).rev().try_for_each(|c| d.table_delete(h, t, Axis::Column, c))),
        "Table > Merge Cells" => table_edit(app, "Merge Cells", &|d, h, t, b| d.merge_cells(h, t, nm_core::kn_model::CellRange { row: b.0, col: b.1, rows: b.2 - b.0 + 1, cols: b.3 - b.1 + 1 })),
        n if n.starts_with("Autofill Cells > Autofill ") => {
            // Numbers: the selection fills from its edge in that direction's opposite (Right:
            // from its first column, Up: from its last row), or from the row / column next to it
            let Some(sel) = app.sel else { return };
            commit(app);
            let (r0, c0, r1, c1) = app.with_merges(sel.table, sel.block());
            let (src, to) = match n {
                "Autofill Cells > Autofill Right" => ((r0, c0, r1, c0), (r0, c0, r1, c1)),
                "Autofill Cells > Autofill Left" => ((r0, c1, r1, c1), (r0, c0, r1, c1)),
                "Autofill Cells > Autofill Down" => ((r0, c0, r0, c1), (r0, c0, r1, c1)),
                "Autofill Cells > Autofill Up" => ((r1, c0, r1, c1), (r0, c0, r1, c1)),
                "Autofill Cells > Autofill from Row Above" => ((r0 - 1, c0, r0 - 1, c1), (r0 - 1, c0, r1, c1)),
                _ => ((r0, c0 - 1, r1, c0 - 1), (r0, c0 - 1, r1, c1)),
            };
            if let Some(Err(e)) = app.edit("Autofill", |d, h| d.autofill(h, sel.table, src, to)) {
                app.notify(format!("Autofill: {e}"));
            }
        }
        n if n.starts_with("Select > ") && n.contains("Value") => select_value(app, n),
        n if n.starts_with("Align Objects > ") || n.starts_with("Distribute Objects > ") => align(app, n),
        "Group" => {
            let ids = app.selected_objs();
            let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
            match app.edit("Group", |d, h| d.group(h, sheet, &ids)) {
                Some(Ok(g)) => {
                    app.canvas.multi.clear();
                    app.canvas.obj = Some(g);
                }
                Some(Err(e)) => app.notify(format!("Couldn't group: {e}")),
                None => {}
            }
        }
        "Ungroup" => {
            let (Some(g), Some(sheet)) = (app.canvas.obj, app.sheets().get(app.sheet).map(|s| s.id)) else { return };
            match app.edit("Ungroup", |d, h| d.ungroup(h, sheet, g)) {
                // the shapes stay selected
                Some(Ok(kids)) if !kids.is_empty() => {
                    app.canvas.obj = Some(kids[0]);
                    app.canvas.multi = kids;
                }
                Some(Err(e)) => app.notify(format!("Couldn't ungroup: {e}")),
                _ => {}
            }
        }
        "Table > Freeze Header Rows" | "Table > Freeze Header Columns" => {
            let Some(t) = selected_table(app) else { return };
            let rows = name.ends_with("Rows");
            let on = !app.doc().is_some_and(|d| nm_core::sheets::headers_frozen(d, t, rows));
            app.edit(if rows { "Freeze Header Rows" } else { "Freeze Header Columns" }, |d, h| nm_core::sheets::set_headers_frozen(d, h, t, rows, on));
        }
        "Table > Wrap Text" => {
            // the other way from the cursor cell's (Numbers wraps by default)
            let Some(sel) = app.sel else { return };
            commit(app);
            let on = !cursor_wraps(app);
            let t = sel.table;
            let blocks = app.model_blocks(t, sel.block());
            if let Some(Err(e)) = app.edit("Wrap Text", |d, h| blocks.iter().try_for_each(|b| d.set_cells_wrap(h, t, *b, on).map(|_| ()))) {
                app.notify(format!("Wrap Text: {e}"));
            }
        }
        n if n.starts_with("Add Link > ") => crate::links::open(app, n.trim_start_matches("Add Link > ")),
        "Remove Link" => crate::links::remove(app),
        "Shapes and Lines > Make Editable" => crate::points::make_editable(app),
        n if n.starts_with("Shapes and Lines > ") && n.ends_with(" Shapes") => {
            use nm_core::kn_model::boolops::BoolOp;
            let op = match n {
                "Shapes and Lines > Unite Shapes" => BoolOp::Unite,
                "Shapes and Lines > Intersect Shapes" => BoolOp::Intersect,
                "Shapes and Lines > Subtract Shapes" => BoolOp::Subtract,
                _ => BoolOp::Exclude,
            };
            let ids = app.selected_objs();
            let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
            let b = match app.doc().map(|d| d.boolean_shape(&app.drawables, &ids, op)) {
                Some(Ok(b)) => b,
                Some(Err(e)) => return app.notify(e.to_string()),
                None => return,
            };
            // the result: Numbers' own square given the combined outline, the shapes removed
            let donor = match nm_core::sheets::shape_donor(3) {
                Ok(d) => d,
                Err(e) => return app.notify(e.to_string()),
            };
            let label = n.trim_start_matches("Shapes and Lines > ").to_string();
            match app.edit(&label, |d, h| {
                let id = nm_core::sheets::insert_object(d, h, sheet, &donor.0, donor.1, b.x, b.y)?;
                d.apply_boolean_shape(h, id, &b);
                nm_core::sheets::detach_drawables(d, h, sheet, &b.originals);
                Ok::<_, nm_core::kn_format::Error>(id)
            }) {
                Some(Ok(id)) => {
                    app.canvas.multi.clear();
                    app.canvas.obj = Some(id);
                }
                Some(Err(e)) => app.notify(e.to_string()),
                None => {}
            }
        }
        "Shapes and Lines > Break Apart" => {
            let Some(id) = app.canvas.obj else { return };
            let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
            let Some(path) = app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
                DrawableKind::Shape(s) => Some(s.path.clone()),
                _ => None,
            }) else {
                return;
            };
            let parts = match app.doc().map(|d| d.break_apart_parts(id, &path)) {
                Some(Ok(e)) => e,
                Some(Err(e)) => return app.notify(e.to_string()),
                None => return,
            };
            // a copy of the shape for each part after the first; each keeps its own part
            match app.edit("Break Apart", |d, h| {
                let clip = nm_core::sheets::copy_objects(d, &[id]).ok_or(nm_core::kn_format::Error::Invalid("can't copy the shape".into()))?;
                let mut out = vec![id];
                for _ in 1..parts.subpaths.len() {
                    out.extend(nm_core::sheets::paste_objects(d, h, sheet, &clip, 0.0)?);
                }
                for (k, sid) in out.iter().enumerate() {
                    let one = nm_core::kn_model::points::Editable { subpaths: vec![parts.subpaths[k].clone()] };
                    d.set_editable_nodes(h, *sid, &one);
                }
                Ok::<_, nm_core::kn_format::Error>(out)
            }) {
                Some(Ok(out)) => {
                    app.canvas.obj = out.first().copied();
                    app.canvas.multi = out;
                }
                Some(Err(e)) => app.notify(e.to_string()),
                None => {}
            }
        }
        "Shapes and Lines > Reset Text and Object Handles" => {
            let Some(id) = app.canvas.obj else { return };
            app.edit("Reset Handles", |d, h| d.reset_handles(h, id));
        }
        "Shapes and Lines > Make Sharp Point" => crate::points::set_kind(app, nm_core::kn_model::points::NodeKind::Sharp),
        "Shapes and Lines > Make Smooth Point" => crate::points::set_kind(app, nm_core::kn_model::points::NodeKind::Smooth),
        "Shapes and Lines > Make Bézier Point" => crate::points::set_kind(app, nm_core::kn_model::points::NodeKind::Bezier),
        "Substitutions > Smart Quotes" | "Substitutions > Smart Dashes" | "Substitutions > Text Replacement" | "Substitutions > Smart Web and Email Links" | "Substitutions > Smart Phone Number Links" | "Spelling and Grammar > Check Spelling While Typing" | "Spelling and Grammar > Check Grammar With Spelling" | "Spelling and Grammar > Correct Spelling Automatically" => {
            let st = &mut app.settings;
            let flag = match name {
                "Substitutions > Smart Quotes" => &mut st.smart_quotes,
                "Substitutions > Smart Dashes" => &mut st.smart_dashes,
                "Substitutions > Text Replacement" => &mut st.text_replacement,
                "Substitutions > Smart Web and Email Links" => &mut st.smart_links,
                "Substitutions > Smart Phone Number Links" => &mut st.smart_phone_links,
                "Spelling and Grammar > Check Spelling While Typing" => &mut st.spell_check,
                "Spelling and Grammar > Check Grammar With Spelling" => &mut st.grammar,
                _ => &mut st.autocorrect,
            };
            *flag = !*flag;
            st.save();
        }
        "Revert To > Browse All Versions…" => app.show_versions = true,
        "Remove Highlights and Comments" => {
            // the selected cells' comments, else every cell comment on the sheet
            commit(app);
            let mut cells: Vec<(ObjId, u32, u32)> = Vec::new();
            match app.sel {
                Some(sel) => {
                    let t = sel.table;
                    for (r0, c0, r1, c1) in app.model_blocks(t, sel.block()) {
                        for r in r0..=r1 {
                            for c in c0..=c1 {
                                cells.push((t, r, c));
                            }
                        }
                    }
                }
                None => {
                    if let Some(w) = app.wb.as_ref() {
                        for d in &app.drawables {
                            if matches!(d.kind, DrawableKind::Table(_)) {
                                cells.extend(nm_core::cell_comments(w.file(), d.id).into_keys().map(|(r, c)| (d.id, r, c)));
                            }
                        }
                    }
                }
            }
            let n = app.edit("Remove Comments", |d, h| cells.iter().filter(|(t, r, c)| nm_core::comments::remove_cell_comment(d, h, *t, *r, *c).unwrap_or(false)).count()).unwrap_or(0);
            app.notify(format!("Removed {n} comment{}", if n == 1 { "" } else { "s" }));
        }
        "Font > Show Fonts" => app.show_fonts = !app.show_fonts,
        "Font > Replace Fonts…" => crate::fonts::open_replace(app),
        "Single File" => crate::fileops::set_package(app, false),
        "Package" => crate::fileops::set_package(app, true),
        "Move To…" => crate::fileops::move_to(app),
        "Reduce File Size…" => crate::fileops::reduce_file_size(app),
        "Print Without Preview…" => crate::fileops::print_now(app),
        "Substitutions > Show Substitutions" => {
            app.show_settings = true;
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("settings-tab"), 2usize));
        }
        "Spelling and Grammar > Check Document Now" | "Spelling and Grammar > Show Spelling and Grammar" => {
            commit(app);
            app.spelling = Some(crate::spelling::Spelling::default());
            crate::spelling::find_next(app);
        }
        n if n.starts_with("Author Color > ") => {
            if let Some(i) = crate::settings::AUTHOR_COLORS.iter().position(|(c, _)| Some(*c) == n.strip_prefix("Author Color > ")) {
                app.settings.author_color = i as u8;
                app.settings.save();
            }
        }
        n if n.starts_with("Sheet > Hide ") => app.hide_sheet(app.sheet),
        "Show Arrange Tools" => {
            app.inspector = true;
            app.insp_tab = InspTab::Arrange;
            app.obj_tab = usize::MAX;
        }
        "Show Adjust Image" => {
            // (a picture's tabs: Style, Image, Arrange)
            app.inspector = true;
            app.obj_tab = 1;
        }
        "Show Colors" => app.show_colors = true,
        "Values Help" => help_page(),
        "Text > Increase Indent Level" | "Text > Decrease Indent Level" => {
            let Some(o) = app.canvas.obj else { return };
            let Some(st) = app.doc().and_then(|d| d.storage_of(o)) else { return };
            let len = app.doc().map(|d| d.storage_text(st).encode_utf16().count()).unwrap_or(0);
            let delta = if name.contains("Increase") { 1 } else { -1 };
            app.edit("Indent", |d, h| d.change_list_level(h, st, 0, len, delta));
        }
        // Numbers: from cells, the table they're in
        "Select Parent" => {
            let Some(s) = app.sel else { return };
            commit(app);
            app.sel = None;
            app.canvas.multi.clear();
            app.canvas.obj = Some(s.table);
            app.insp_tab = InspTab::Arrange;
        }
        n if n.starts_with("Transformations > ") => transform_text(app, n),
        "Table > Resize Row to Fit Content" | "Table > Resize Column to Fit Content" => {
            let Some(sel) = app.sel else { return };
            commit(app);
            let t = sel.table;
            let (r0, c0, r1, c1) = app.with_merges(t, sel.block());
            if name.contains("Row") {
                // a row that fits its content: Numbers stores a height of 0 and lays it out at
                // its tallest cell
                let rows: Vec<(u32, f32)> = (r0..=r1).filter_map(|v| app.mrow(t, v)).map(|r| (r, 0.0)).collect();
                app.edit(name, |d, h| d.set_table_sizes(h, t, true, &rows));
            } else {
                let Some(td) = app.table(t).map(|(_, td)| td.clone()) else { return };
                let cols: Vec<(u32, f32)> = (c0..=c1).filter_map(|c| app.canvas.tiles.fit_width(&td, c).map(|w| (c, w))).collect();
                if !cols.is_empty() {
                    app.edit(name, |d, h| d.set_table_sizes(h, t, false, &cols));
                }
            }
        }
        "Table > Distribute Rows Evenly" | "Table > Distribute Columns Evenly" => {
            let Some(t) = selected_table(app) else { return };
            commit(app);
            let rows = name.contains("Rows");
            app.edit(name, |d, h| d.distribute_table(h, t, rows));
        }
        "Comments > Show Next Comment" | "Comments > Show Previous Comment" => next_comment(app, name.contains("Next")),
        "Find > Use Selection for Find" => {
            let Some(sel) = app.sel else { return };
            let q = app.table(sel.table).and_then(|(_, td)| td.cells.get((sel.cursor.0 * td.cols + sel.cursor.1) as usize).cloned()).unwrap_or_default();
            let res = app.search(&q);
            app.find = Some((q, res, 0));
        }
        "Copied Rows" | "Copied Columns" => crate::clip::insert_copied(app, name == "Copied Rows"),
        "Find > Use Selection for Replace" => {
            let Some(sel) = app.sel else { return };
            let t = app.table(sel.table).and_then(|(_, td)| td.cells.get((sel.cursor.0 * td.cols + sel.cursor.1) as usize).cloned()).unwrap_or_default();
            app.replace = Some(t);
            if app.find.is_none() {
                app.find = Some((String::new(), Vec::new(), 0));
            }
        }
        "Find > Jump to Selection" => {
            if let Some(s) = app.sel {
                app.reveal_cell(s.table, s.cursor.0, s.cursor.1);
            }
        }
        "Unhide > Unhide All Sheets" => {
            let ids: Vec<ObjId> = app.sheets().iter().filter(|s| s.hidden).map(|s| s.id).collect();
            app.edit("Unhide Sheets", |d, h| ids.iter().for_each(|id| nm_core::sheets::set_sheet_hidden(d, h, *id, false)));
        }
        "Autofill Cells > Autofill" => {
            // the selection fills from its first row (down), or its first column (right) when
            // it is one row
            let Some(sel) = app.sel else { return };
            commit(app);
            let (r0, c0, r1, c1) = app.with_merges(sel.table, sel.block());
            let (src, to) = if r1 > r0 { ((r0, c0, r0, c1), (r0, c0, r1, c1)) } else { ((r0, c0, r1, c0), (r0, c0, r1, c1)) };
            if let Some(Err(e)) = app.edit("Autofill", |d, h| d.autofill(h, sel.table, src, to)) {
                app.notify(format!("Autofill: {e}"));
            }
        }
        "Table > Hide Row" | "Table > Hide Column" | "Table > Unhide All Rows" | "Table > Unhide All Columns" => {
            let Some(t) = selected_table(app) else { return };
            commit(app);
            let rows = name.ends_with("Row") || name.ends_with("Rows");
            let hide = name.contains("> Hide");
            let indices: Vec<u32> = if hide {
                let Some(sel) = app.sel else { return };
                let b = sel.block();
                if rows { (b.0..=b.2).filter_map(|r| app.mrow(t, r)).collect() } else { (b.1..=b.3).collect() }
            } else {
                let (r, c) = app.wb.as_ref().map(|w| nm_core::hidden(w.file(), t)).unwrap_or_default();
                if rows { r } else { c }
            };
            if let Some(Err(e)) = app.edit(if hide { "Hide" } else { "Unhide" }, |d, h| nm_core::set_hidden(d, h, t, rows, &indices, hide)) {
                app.notify(format!("{e}"));
            }
            if hide {
                app.sel = None;
            }
        }
        "Table > Unmerge Cells" => {
            let Some(sel) = app.sel else { return };
            commit(app);
            let (r0, c0, ..) = sel.block();
            let mr = app.mrow(sel.table, r0).unwrap_or(r0);
            app.edit("Unmerge Cells", |d, h| d.unmerge_cells(h, sel.table, mr, c0));
        }
        "Table > Insert Current Date" | "Table > Insert Current Time" => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            // Apple's reference date (2001-01-01), local time not applied (UTC)
            let secs = now - 978_307_200.0;
            let pattern = if name.ends_with("Date") { "M/d/yy" } else { "h:mm a" };
            let text = nm_core::kn_model::table::format_date(secs, pattern);
            crate::canvas::start_edit(app, Some(text));
            commit(app);
        }
        "Select > Row" => {
            if let (Some(sel), Some(cols)) = (app.sel, app.sel.and_then(|s| app.table(s.table).map(|(_, t)| t.cols))) {
                app.sel = Some(Sel { table: sel.table, anchor: (sel.block().0, 0), cursor: (sel.block().2, cols.saturating_sub(1)) });
            }
        }
        "Select > Column" => {
            if let (Some(sel), Some(rows)) = (app.sel, app.sel.and_then(|s| app.table(s.table).map(|(_, t)| t.rows))) {
                app.sel = Some(Sel { table: sel.table, anchor: (0, sel.block().1), cursor: (rows.saturating_sub(1), sel.block().3) });
            }
        }
        "Show Sort Options" | "Show Filter Options" | "Show Category Options" | "Inspector > Organize" => {
            app.inspector = true;
            app.insp_tab = InspTab::Organize;
        }
        "Inspector > Format" => {
            app.inspector = true;
            if app.insp_tab == InspTab::Organize {
                app.insp_tab = InspTab::Table;
            }
        }
        "Inspector > Show Next Inspector Tab" | "Inspector > Show Previous Inspector Tab" => {
            let order = [InspTab::Table, InspTab::Cell, InspTab::Text, InspTab::Arrange, InspTab::Organize];
            let i = order.iter().position(|t| *t == app.insp_tab).unwrap_or(0);
            let step = if name.ends_with("Next Inspector Tab") { 1 } else { order.len() - 1 };
            app.insp_tab = order[(i + step) % order.len()];
            app.inspector = true;
        }
        "Inspector > Hide Inspector" => app.inspector = !app.inspector,
        "Comments > Hide Comments" => app.show_comments = !app.show_comments,
        "Zoom > Zoom In" => app.canvas.animate_zoom(crate::chrome::ZOOMS.iter().copied().find(|z| *z > app.canvas.zoom_goal() + 1e-3).unwrap_or(4.0)),
        "Zoom > Zoom Out" => app.canvas.animate_zoom(crate::chrome::ZOOMS.iter().rev().copied().find(|z| *z < app.canvas.zoom_goal() - 1e-3).unwrap_or(0.25)),
        "Zoom > Actual Size" => app.canvas.animate_zoom(1.0),
        "Zoom > Zoom to Selection" => {
            let r = if let Some(d) = app.canvas.obj.and_then(|o| app.drawables.iter().find(|d| d.id == o)) {
                let g = crate::tiles::frame(d);
                Some(egui::Rect::from_min_size(egui::pos2(g.x, g.y), egui::vec2(g.w.max(8.0), g.h.max(8.0))))
            } else if let Some((sel, (d, t))) = app.sel.and_then(|s| Some((s, app.table(s.table)?))) {
                let g = t.frame(&d.geom);
                let b = app.with_merges(sel.table, sel.block());
                let x0 = g.x + t.col_widths.iter().take(b.1 as usize).sum::<f32>();
                let y0 = g.y + t.row_heights.iter().take(b.0 as usize).sum::<f32>();
                let w: f32 = t.col_widths.iter().skip(b.1 as usize).take((b.3 - b.1 + 1) as usize).sum();
                let h: f32 = t.row_heights.iter().skip(b.0 as usize).take((b.2 - b.0 + 1) as usize).sum();
                Some(egui::Rect::from_min_size(egui::pos2(x0, y0), egui::vec2(w, h)))
            } else {
                None
            };
            if let Some(r) = r {
                app.canvas.zoom_to(r);
            }
        }
        "Enter Full Screen" => {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
        }
        "Hide Toolbar" => app.show_toolbar = !app.show_toolbar,
        "Customize Toolbar…" => {
            app.show_toolbar = true;
            app.tb.customizing = true;
        }
        "Minimize" => crate::chrome::minimize(ctx),
        "Zoom" | "Fill" => {
            let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
        }
        "Show Previous Sheet" => app.go_to_sheet(app.sheet.saturating_sub(1)),
        "Show Next Sheet" => app.go_to_sheet(app.sheet + 1),
        "Show First Sheet" => app.go_to_sheet(0),
        "Show Last Sheet" => app.go_to_sheet(app.sheets().len().saturating_sub(1)),
        "Keyboard Shortcuts" => app.show_shortcuts = true,
        // Format > Font's text items (Keydeck's mapping: kn-model text properties)
        "Baseline > Superscript" => crate::editing::set_text(app, "Superscript", TextProp::Script(1)),
        "Baseline > Subscript" => crate::editing::set_text(app, "Subscript", TextProp::Script(-1)),
        "Baseline > Use Default" => {
            crate::editing::set_text(app, "Baseline", TextProp::Script(0));
            crate::editing::set_text(app, "Baseline", TextProp::BaselineShift(0.0));
        }
        "Baseline > Raise" | "Baseline > Lower" => {
            // 1 pt at a time
            let cur = cursor_char(app).map(|c| c.baseline_shift).unwrap_or(0.0);
            let v = if name.ends_with("Raise") { cur + 1.0 } else { cur - 1.0 };
            crate::editing::set_text(app, "Baseline Shift", TextProp::BaselineShift(v));
        }
        n if n.starts_with("Capitalization > ") => {
            use nm_core::kn_model::Caps;
            let caps = match n {
                "Capitalization > All Caps" => Caps::All,
                "Capitalization > Small Caps" => Caps::Small,
                "Capitalization > Title Case" => Caps::Title,
                "Capitalization > Start Case" => Caps::Start,
                _ => Caps::None,
            };
            crate::editing::set_text(app, "Capitalization", TextProp::Caps(caps));
        }
        n if n.starts_with("Character Spacing > ") => {
            // 1 % of the font size a step
            let cur = cursor_char(app).map(|c| c.tracking).unwrap_or(0.0);
            let t = match n {
                "Character Spacing > Tighten" => cur - 0.01,
                "Character Spacing > Loosen" => cur + 0.01,
                _ => 0.0,
            };
            crate::editing::set_text(app, "Character Spacing", TextProp::Tracking(t));
        }
        "Ligature > Use Default" => crate::editing::set_text(app, "Ligatures", TextProp::Ligatures(1)),
        "Ligature > Use None" => crate::editing::set_text(app, "Ligatures", TextProp::Ligatures(0)),
        "Ligature > Use All" => crate::editing::set_text(app, "Ligatures", TextProp::Ligatures(2)),
        "Font > Bold" => {
            let on = current_char(app).is_some_and(|c| c.bold);
            text_prop(app, "Bold", TextProp::Bold(!on));
        }
        "Font > Italic" => {
            let on = current_char(app).is_some_and(|c| c.italic);
            text_prop(app, "Italic", TextProp::Italic(!on));
        }
        "Font > Underline" => {
            let on = current_char(app).is_some_and(|c| c.underline);
            text_prop(app, "Underline", TextProp::Underline(!on));
        }
        "Font > Strikethrough" => {
            let on = current_char(app).is_some_and(|c| c.strike);
            text_prop(app, "Strikethrough", TextProp::Strike(!on));
        }
        "Font > Bigger" | "Font > Smaller" => {
            let size = current_char(app).map(|c| c.size).unwrap_or(10.0);
            let next = if name.ends_with("Bigger") { size + 1.0 } else { (size - 1.0).max(1.0) };
            text_prop(app, "Font Size", TextProp::Size(next));
        }
        "Text > Align Left" | "Text > Align Center" | "Text > Align Right" | "Text > Justify" | "Text > Auto-Align Table Cell" => {
            let (ca, pa) = match name {
                "Text > Align Left" => (CellAlign::Left, nm_core::kn_model::Align::Left),
                "Text > Align Center" => (CellAlign::Center, nm_core::kn_model::Align::Center),
                "Text > Align Right" => (CellAlign::Right, nm_core::kn_model::Align::Right),
                "Text > Justify" => (CellAlign::Justify, nm_core::kn_model::Align::Justify),
                _ => (CellAlign::Auto, nm_core::kn_model::Align::Left),
            };
            commit(app);
            if let Some((t, blocks)) = cell_blocks(app) {
                app.edit("Alignment", |d, h| blocks.iter().try_for_each(|b| d.set_cells_alignment(h, t, *b, ca).map(|_| ())));
            } else if let Some(id) = app.canvas.obj {
                app.edit("Alignment", |d, h| d.set_text_prop(h, id, &TextProp::Align(pa)));
            }
        }
        "Sheet > Rename…" => {
            let n = app.sheets().get(app.sheet).map(|s| s.name.clone()).unwrap_or_default();
            app.renaming = Some((app.sheet, n));
        }
        "Sheet > Duplicate" => app.duplicate_sheet(app.sheet),
        // the Sheet inspector (name, tab colour, duplicate / hide / delete): nothing selected
        "Sheet > Show Sheet Options" | "Sheet > Tab Color" => {
            commit(app);
            app.sel = None;
            app.canvas.obj = None;
            app.inspector = true;
        }
        "Image > Edit Mask" | "Edit Mask" => {
            app.mask_edit = match (app.mask_edit, crate::mask::selected_image(app)) {
                (Some(_), _) => None,
                // (a picture without a mask edits a full-frame one, made at the first change)
                (None, id) => id,
            };
        }
        "Auto Crop" => crate::mask::auto_crop(app),
        "Image > Reset Mask" | "Reset Mask" => {
            let Some(id) = crate::mask::selected_image(app) else { return };
            app.mask_edit = None;
            app.edit("Reset Mask", |d, h| d.reset_mask(h, id));
        }
        n if n.starts_with("Mask with Shape > ") => {
            let Some(id) = crate::mask::selected_image(app) else { return };
            let Some(b) = nm_core::kn_model::insert::BasicShape::ALL.into_iter().find(|b| Some(b.name()) == n.strip_prefix("Mask with Shape > ")) else { return };
            app.edit("Mask with Shape", |d, h| d.mask_with_shape(h, id, b));
        }
        "Image > Remove Background" | "Image > Reset Background" | "Remove Background" | "Reset Background" => {
            let Some(id) = app.canvas.obj else { return };
            let Some(doc) = app.doc() else { return };
            if doc.file.type_name(id) != "TSD.ImageArchive" {
                return;
            }
            let png = if name.ends_with("Remove Background") {
                let Some(bytes) = doc.image_original_data(id).and_then(|d| doc.file.data(d)).map(|(_, b)| b.into_owned()) else { return };
                match crate::imageadj::remove_background(&bytes) {
                    Some(p) => Some(p),
                    None => return app.notify("No background could be found to remove."),
                }
            } else {
                None
            };
            if let Some(Err(e)) = app.edit("Remove Background", |d, h| d.set_background_removed(h, id, png)) {
                app.notify(format!("{e}"));
            }
        }
        "Copy Style" => crate::editing::copy_style(app),
        "Paste Style" => crate::editing::paste_style(app),
        "Conditional Highlighting…" => {
            app.inspector = true;
            app.insp_tab = InspTab::Cell;
        }
        "Bring Forward" | "Bring to Front" | "Send Backward" | "Send to Back" => {
            let how = match name {
                "Bring Forward" => ZMove::Forward,
                "Bring to Front" => ZMove::Front,
                "Send Backward" => ZMove::Backward,
                _ => ZMove::Back,
            };
            let (Some(s), Some(id)) = (sheet, app.canvas.obj) else { return };
            app.edit(name, |d, h| nm_core::sheets::reorder(d, h, s, &[id], how));
        }
        "Flip Horizontally" | "Flip Vertically" => {
            let Some(id) = app.canvas.obj else { return };
            let Some(mut g) = app.drawables.iter().find(|d| d.id == id).map(|d| d.geom) else { return };
            if name == "Flip Horizontally" {
                g.hflip = !g.hflip;
            } else {
                g.vflip = !g.vflip;
            }
            app.edit(name, |d, h| d.set_frame(h, id, g));
        }
        "Lock" | "Unlock" => {
            let Some(id) = app.canvas.obj else { return };
            let on = name == "Lock";
            app.edit(name, |d, h| d.set_locked(h, id, on));
        }
        _ => app.notify(format!("{name} isn't available yet.")),
    }
}

#[cfg(test)]
mod tests {
    /// Lists the menu commands that don't work yet (run with --ignored --nocapture).
    #[test]
    #[ignore]
    fn list_greyed_commands() {
        fn walk(items: &[Item], parent: &str, out: &mut Vec<String>) {
            for it in items {
                if it.children.is_empty() {
                    let n = command_name(parent, &it.label);
                    if !it.label.is_empty() && !it.label.starts_with('-') && !implemented(&n) {
                        out.push(n);
                    }
                } else {
                    walk(&it.children, &it.label, out);
                }
            }
        }
        for m in menus() {
            let mut out = Vec::new();
            walk(&m.children, &m.label, &mut out);
            println!("== {}: {}", m.label, out.join(" | "));
        }
    }

    use super::*;

    #[test]
    fn menus_parse_and_shortcuts_map() {
        let m = menus();
        let names: Vec<&str> = m.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(names, ["Values", "File", "Edit", "Insert", "Table", "Organize", "Format", "Arrange", "View", "Window", "Help"]);
        assert_eq!(linux_shortcut("⇧⌘Z").as_deref(), Some("Ctrl+Shift+Z"));
        assert_eq!(linux_shortcut("⌥⌘⌫").as_deref(), Some("Ctrl+Alt+Backspace"));
        assert_eq!(keyboard("Ctrl+Shift+Z"), Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)));
        assert_eq!(keyboard("Ctrl++"), Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus)));
        assert_eq!(keyboard("Ctrl+Super+M"), None);
        let t = shortcut_table();
        for want in ["Duplicate Selection", "Sheet", "Lock", "Bring to Front", "Find > Find…", "Comment", "Zoom > Actual Size"] {
            assert!(t.iter().any(|(_, n, _)| n == want), "{want} has no shortcut");
        }
        // every implemented command is in the menus
        fn all(items: &[Item], parent: &str, out: &mut Vec<String>) {
            for i in items {
                if i.children.is_empty() {
                    out.push(command_name(parent, &i.label));
                } else {
                    all(&i.children, &i.label, out);
                }
            }
        }
        let mut names = Vec::new();
        for m in m {
            all(&m.children, &m.label, &mut names);
        }
        for c in IMPLEMENTED {
            assert!(names.iter().any(|n| n == c), "{c} is not in the menus");
        }
    }

    /// The commands still greyed, menu by menu (cargo test -p nm-app greyed -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn greyed() {
        fn all(items: &[Item], parent: &str, top: &str, out: &mut Vec<(String, String)>) {
            for i in items {
                if i.separator {
                    continue;
                }
                if i.children.is_empty() {
                    out.push((top.to_string(), command_name(parent, &i.label)));
                } else {
                    all(&i.children, &i.label, top, out);
                }
            }
        }
        let mut names = Vec::new();
        for m in menus() {
            all(&m.children, &m.label, &m.label, &mut names);
        }
        let grey: Vec<&(String, String)> = names.iter().filter(|(_, n)| !implemented(n)).collect();
        for (top, n) in &grey {
            println!("{top}\t{n}");
        }
        println!("{} commands, {} working, {} greyed", names.len(), names.len() - grey.len(), grey.len());
    }
}
