//! The menu bar, generated from Pages 15.4's own menu tree (data/menus.txt, captured on the Mac
//! in the Pages research) the way Keydeck and Values generate theirs: items Letters
//! implements run a command, the rest are shown greyed so the gaps stay visible. Every
//! shortcut in the tree (⌘ = Ctrl, ⌥ = Alt, ⇧ = Shift, ⌃ = Super) is a keyboard shortcut here.

use std::sync::OnceLock;

use egui::{Key, KeyboardShortcut, Modifiers};
use pg_core::kn_format::ObjId;
use pg_core::kn_model::edit::ZMove;
use pg_core::kn_model::insert::BasicShape;
use pg_core::kn_model::tablerows::{Axis, HeaderKind};
use pg_core::kn_model::DrawableKind;
use pg_core::objects::NewObject;

use crate::app::{App, Cursor, InspMode, Owner, Sidebar};
use crate::canvas::Fit;
use crate::text::CharChange;

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
            menus.push(Item { label: name.to_string(), ..Default::default() });
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
            parent.children.push(Item { separator: true, ..Default::default() });
            continue;
        }
        let text = text.replace(" (disabled now)", "");
        let (label, shortcut) = match text.rsplit_once("   [") {
            Some((l, s)) => (l.trim().to_string(), linux_shortcut(s.trim_end_matches(']'))),
            None => (text.trim().to_string(), None),
        };
        if parent.children.last().is_some_and(|c| c.label == label) {
            continue;
        }
        parent.children.push(Item { label, shortcut, ..Default::default() });
    }
    // macOS-only items, Apple services and AI features have no meaning here
    const MAC_ONLY: &[&str] = &[
        "Learn About Apple Creator Studio", "Get Apple Creator Studio", "Provide Letters Feedback", "Privacy & Analytics", "Services", "Hide Letters", "Hide Others", "Show All",
        "Quit and Keep Windows", "Start Dictation", "Emoji & Symbols", "AutoFill", "Writing Tools", "Generate Image", "Generate Shape", "Image Playground…", "Generate from Image",
        "Super Resolution", "Minimize All", "Zoom All", "Fill", "Center", "Move & Resize", "Full Screen Tile", "Move to <user device>", "Bring All to Front", "Arrange in Front",
        "Remove Window from Set", "Merge All Windows", "Edit in Pixelmator Pro", "Movie", "Pages ’09…", "Formulas and Functions Help",
        "Remove Watermarks", "Share…", "Activity Settings…", "Hide Participant Cursors", "Show Activity Pane", "Show Sync Status", "Send Pages Feedback",
        "What’s New in Pages", "Welcome to Pages", "About Premium Features", "Service and Support", "Web Video…", "Record Audio", "Show Music Browser", "Show Photo and Video Browser",
        "Image Gallery", "Speech", "Phonetic Guide Text…", "Remove Phonetic Guide Text", "Publish to Apple Books",
        "Manage Bookmark Suggestions…", "EndNote Citations", "EndNote Citation", "Remove Smart Annotations", "Hide Smart Annotations", "Mail Merge…", "Mail Merge Field", "Reduce File Size…",
        "Section Layouts", "Edit Page Templates", "Reapply Page Template", "Show Warnings", "Save to My Shapes", "UI-doc.pages",
        "Language & Region…", "Define as Media Placeholder", "Paste Formula Results", "Show Arrange Tools", "Show Adjust Image", "Draw with Pen", "Smart Web and Email Links",
        "Smart Phone Number Links", "Show Substitutions", "Move To…", "Section", "Define as Placeholder Text", "Set as Default Text Box Appearance", "Enter Full Screen",
        "Save as Template…",
    ];
    fn prune(items: &mut Vec<Item>, parent: &str) {
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
    // Linux additions: Window > Full Screen, Help > Letters Help / Keyboard Shortcuts (kept)
    if let Some(w) = menus.iter_mut().find(|m| m.label == "Window") {
        w.children.push(Item { separator: true, ..Default::default() });
        w.children.push(Item { label: "Toggle Full Screen".into(), shortcut: Some("F11".into()), ..Default::default() });
    }
    if let Some(h) = menus.iter_mut().find(|m| m.label == "Help") {
        for c in &mut h.children {
            if c.label == "Pages Help" {
                c.label = "Letters Help".into();
            }
        }
    }
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
    if key.starts_with("fn") || (key.starts_with('m') && key.len() > 1) || key == "🎤" {
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
        ":" => {
            m |= Modifiers::SHIFT;
            Key::Semicolon
        }
        "`" => Key::Backtick,
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

/// Submenus whose items are named with the submenu ("Table > Basic").
const QUALIFIED: &[&str] = &[
    "Table", "Chart", "Shape", "Line", "Header Rows", "Header Columns", "Footer Rows", "Formula", "Select", "Align Objects", "Distribute Objects", "Export To", "Inspector", "Zoom", "Font", "Text",
    "Find", "Autofill Cells", "Chart Type", "Author Color", "Guides", "Character Spacing", "Ligature", "Baseline", "Capitalization", "Add Link", "Mask with Shape", "Advanced", "Revert To",
    "Spelling and Grammar", "Substitutions", "Transformations", "Shapes and Lines", "Image", "Table of Contents", "Go To", "Comments & Changes", "Change File Type",
];

pub fn command_name(parent: &str, label: &str) -> String {
    if QUALIFIED.contains(&parent) { format!("{parent} > {label}") } else { label.to_string() }
}

/// Commands that work; everything else in the menus is greyed.
const IMPLEMENTED: &[&str] = &[
    "About Letters", "Support Development…", "Settings…", "Quit Letters", "Open Recent", "Show Tab Bar", "Show All Tabs", "Show Previous Tab", "Show Next Tab", "Move Tab to New Window", "Choose File", "Conditional Highlighting…",
    "New…", "Open…", "Close", "Close All", "Save", "Save As…", "Duplicate", "Rename…", "Revert To > Browse All Versions…",
    "Export To > PDF…", "Export To > Word…", "Export To > EPUB…", "Export To > Plain Text…", "Export To > Rich Text Format…", "Export To > Images…",
    "Convert to Page Layout", "Convert to Word Processing", "Change File Type > Single File", "Change File Type > Package", "Set Password…", "Page Setup…", "Print…",
    "Undo", "Redo", "Cut", "Copy", "Paste", "Paste and Match Style", "Paste and Preserve Style", "Delete", "Clear All", "Duplicate Selection", "Select All", "Deselect All",
    "Track Changes", "Accept Change", "Reject Change", "Remove Highlights and Comments",
    "Find > Find…", "Find > Find Next", "Find > Find Previous", "Find > Hide Find Panel", "Find > Use Selection for Find", "Find > Use Selection for Replace", "Find > Jump to Selection",
    "Spelling and Grammar > Show Spelling and Grammar", "Spelling and Grammar > Check Document Now", "Spelling and Grammar > Check Spelling While Typing",
    "Spelling and Grammar > Check Grammar With Spelling", "Spelling and Grammar > Correct Spelling Automatically",
    "Substitutions > Smart Quotes", "Substitutions > Smart Dashes", "Substitutions > Text Replacement",
    "Transformations > Make Upper Case", "Transformations > Make Lower Case", "Transformations > Capitalize",
    "Page Break", "Line Break", "Section Break", "Column Break", "Page Number", "Page Count", "Date & Time", "Table of Contents > Document", "Table of Contents > Section", "Footnote",
    "Text", "Highlight", "Comment", "Bookmark", "Equation…",
    "Font > Show Fonts", "Font > Bold", "Font > Italic", "Font > Underline", "Font > Strikethrough", "Font > Bigger", "Font > Smaller", "Font > Replace Fonts…",
    "Character Spacing > Use None", "Character Spacing > Tighten", "Character Spacing > Loosen",
    "Ligature > Use Default", "Ligature > Use None", "Ligature > Use All",
    "Baseline > Use Default", "Baseline > Superscript", "Baseline > Subscript", "Baseline > Raise", "Baseline > Lower",
    "Capitalization > None", "Capitalization > All Caps", "Capitalization > Small Caps", "Capitalization > Title Case", "Capitalization > Start Case",
    "Text > Align Left", "Text > Align Center", "Text > Align Right", "Text > Justify", "Text > Increase Indent Level", "Text > Decrease Indent Level",
    "Table > Add Row Above", "Table > Add Row Below", "Table > Add Column Before", "Table > Add Column After", "Table > Delete Row", "Table > Delete Column", "Table > Sort Ascending", "Table > Sort Descending",
    "Table > Distribute Rows Evenly", "Table > Distribute Columns Evenly", "Table > Merge Cells", "Table > Unmerge Cells",
    "Chart > Show Chart Data Editor", "Copy Style", "Paste Style",
    "Add Link > Webpage", "Add Link > Email", "Add Link > Phone Number", "Add Link > Bookmark", "Remove Link",
    "Advanced > Create Style", "Advanced > Create Page Template From This Page",
    "Image > Edit Mask", "Image > Reset Mask", "Image > Remove Background", "Image > Reset Background",
    "Shapes and Lines > Make Editable", "Shapes and Lines > Make Sharp Point", "Shapes and Lines > Make Smooth Point", "Shapes and Lines > Make Bézier Point", "Shapes and Lines > Toggle Line Type", "Shapes and Lines > Unite Shapes", "Shapes and Lines > Intersect Shapes", "Shapes and Lines > Subtract Shapes", "Shapes and Lines > Exclude Shapes",
    "Shapes and Lines > Break Apart", "Shapes and Lines > Reset Text and Object Handles", "Shapes and Lines > Save to My Shapes",
    "Bring Forward", "Bring to Front", "Send Backward", "Send to Back",
    "Align Objects > Left", "Align Objects > Center", "Align Objects > Right", "Align Objects > Top", "Align Objects > Middle", "Align Objects > Bottom",
    "Distribute Objects > Horizontally", "Distribute Objects > Vertically", "Distribute Objects > Evenly",
    "Flip Horizontally", "Flip Vertically", "Lock", "Unlock", "Group", "Ungroup", "Move Objects to Section Layout", "Make Layout Objects Selectable",
    "Inspector > Format", "Inspector > Document Setup", "Inspector > Show Next Inspector Tab", "Inspector > Show Previous Inspector Tab", "Inspector > Hide Inspector",
    "Page Thumbnails", "Table of Contents", "Document Only", "Show Word Count", "Show Ruler", "Guides > Hide Guides", "Guides > Hide Page Template Guides", "Guides > Clear All Guides on Page",
    "Go To > Next Page", "Go To > Previous Page", "Go To > First Page", "Go To > Last Page", "Go To > Page…",
    "Enter Screen View", "Show Layout", "Show Invisibles", "Show Comments Pane",
    "Comments & Changes > Hide Comments", "Comments & Changes > Show Next Comment or Change", "Comments & Changes > Show Previous Comment or Change",
    "Show Colors", "Zoom > One Page", "Zoom > Two Pages", "Zoom > Zoom In", "Zoom > Zoom Out", "Zoom > Actual Size", "Zoom > Fit Width", "Zoom > Fit Page", "Zoom > Zoom to Selection",
    "Hide Toolbar", "Customize Toolbar…", "Minimize", "Zoom", "Toggle Full Screen", "Letters Help", "Keyboard Shortcuts",
];

const TABLE_ITEMS: [&str; 4] = ["Headers", "Basic", "Plain", "Sums"];

/// Format > Chart's show / hide items (Pages' labels) -> what they toggle.
fn chart_toggle(label: &str) -> Option<&'static str> {
    Some(match label {
        "Show Title" => "title",
        "Show Legend" => "legend",
        "Show Borders" => "border",
        "Show Y Axis Name" | "Show Y Axis Title" => "y-name",
        "Show Y Axis Labels" => "y-labels",
        "Show Y Axis 2 Name" | "Show Y Axis 2 Title" => "y2-name",
        "Show X Axis Title" | "Show X Axis Name" => "x-name",
        "Show X Axis Labels" => "x-labels",
        "Show Series Names" => "series-names",
        "Show Series Value Labels" => "value-labels",
        _ => return None,
    })
}

/// A chart menu item ("Chart > 2D Bar", "Chart Type > 3D Pie") -> kn-model's chart type.
fn chart_type_for(label: &str) -> Option<&'static str> {
    let l = label.strip_prefix("Chart > ").or_else(|| label.strip_prefix("Chart Type > ")).unwrap_or(label);
    if l.starts_with("Interactive ") {
        return pg_core::kn_model::chartdata::CHART_TYPES.iter().find(|t| t.1 == l).map(|t| t.0);
    }
    let (dim, name) = l.split_once(' ')?;
    if dim != "2D" && dim != "3D" {
        return None;
    }
    let want = if dim == "3D" { format!("3D {name}") } else { name.to_string() };
    pg_core::kn_model::chartdata::CHART_TYPES.iter().find(|t| t.1 == want).map(|t| t.0)
}

fn shape_named(label: &str) -> Option<BasicShape> {
    BasicShape::ALL.iter().copied().find(|b| b.name() == label)
}

fn implemented(name: &str) -> bool {
    if IMPLEMENTED.contains(&name) {
        return true;
    }
    if crate::tables::is_command(name) {
        return true;
    }
    if let Some(l) = name.strip_prefix("Table > ") {
        return TABLE_ITEMS.contains(&l);
    }
    if let Some(l) = name.strip_prefix("Chart > ") {
        return l.starts_with("2D ") || l.starts_with("3D ") || l.starts_with("Interactive ") || chart_toggle(l).is_some();
    }
    if crate::tables::is_command(name) {
        return true;
    }
    if name.starts_with("Chart Type > ") {
        return chart_type_for(name).is_some();
    }
    if let Some(l) = name.strip_prefix("Shape > ") {
        return shape_named(l).is_some();
    }
    if let Some(l) = name.strip_prefix("Line > ") {
        return matches!(l, "Line" | "Line with Arrowhead" | "Line with Two Arrowheads" | "Straight Connection Line" | "Curved Connection Line" | "Right Angle Connection Line");
    }
    if let Some(l) = name.strip_prefix("Author Color > ") {
        return crate::settings::AUTHOR_COLORS.iter().any(|(n, _)| *n == l);
    }
    if let Some(l) = name.strip_prefix("Mask with Shape > ") {
        return shape_named(l).is_some();
    }
    if let Some(l) = name.strip_prefix("Formula > ") {
        return matches!(l, "Sum" | "Average" | "Minimum" | "Maximum" | "Count" | "Product");
    }
    ["Header Rows > ", "Header Columns > ", "Footer Rows > "].iter().any(|p| name.starts_with(p))
}

fn has_text_cursor(app: &App) -> bool {
    app.cursor.is_some()
}

fn has_selection(app: &App) -> bool {
    app.cursor.is_some_and(|c| !c.is_empty())
}

fn in_body(app: &App) -> bool {
    app.cursor.is_some_and(|c| c.owner == Owner::Body)
}

fn selected_table(app: &App) -> Option<ObjId> {
    app.sel.map(|s| s.table).or_else(|| app.canvas.obj.filter(|o| app.table(*o).is_some()))
}

fn selected_chart(app: &App) -> Option<ObjId> {
    app.canvas.obj.filter(|o| app.drawables.iter().any(|d| d.id == *o && matches!(d.kind, DrawableKind::Chart(_))))
}

fn selected_image(app: &App) -> Option<ObjId> {
    app.canvas.obj.filter(|o| app.drawables.iter().any(|d| d.id == *o && matches!(d.kind, DrawableKind::Image(_))))
}

/// Whether a command can run now.
pub fn enabled(app: &App, name: &str) -> bool {
    if !implemented(name) {
        return false;
    }
    if let Some(e) = crate::tabs::enabled(app, name) {
        return e;
    }
    if app.word.as_ref().is_some_and(|w| !w.hybrid) && crate::app::word_blocks(name) {
        return false;
    }
    let objs = !app.selected_objs().is_empty();
    let wp = app.wb.as_ref().is_some_and(|w| w.is_word_processing());
    match name {
        "Undo" => app.hist.can_undo(),
        "Redo" => app.hist.can_redo(),
        "Cut" | "Copy" | "Delete" => has_selection(app) || objs || app.sel.is_some(),
        "Paste" | "Paste and Match Style" | "Paste and Preserve Style" => has_text_cursor(app) || app.obj_clip_active,
        "Clear All" => app.sel.is_some(),
        "Duplicate Selection" => objs,
        "Deselect All" => app.cursor.is_some() || objs,
        "Accept Change" | "Reject Change" => in_body(app) && app.wb.as_ref().is_some_and(|w| w.body_storage().is_some_and(|b| !w.changes(b).is_empty())),
        "Remove Highlights and Comments" => app.has_annotations_bar(),
        "Page Break" | "Section Break" | "Column Break" | "Footnote" | "Table of Contents > Document" | "Table of Contents > Section" | "Bookmark" => in_body(app),
        "Line Break" | "Page Number" | "Page Count" | "Date & Time" | "Equation…" => has_text_cursor(app),
        "Highlight" => has_selection(app),
        "Comment" => in_body(app),
        "Convert to Page Layout" => wp,
        "Convert to Word Processing" => !wp,
        "Find > Use Selection for Find" | "Find > Use Selection for Replace" | "Find > Jump to Selection" => has_selection(app),
        "Find > Hide Find Panel" => app.find.is_some(),
        "Find > Find Next" | "Find > Find Previous" => app.find.is_some(),
        n if n.starts_with("Transformations > ") => has_selection(app),
        n if n.starts_with("Font > ") || n.starts_with("Character Spacing > ") || n.starts_with("Ligature > ") || n.starts_with("Baseline > ") || n.starts_with("Capitalization > ") => n == "Font > Show Fonts" || n == "Font > Replace Fonts…" || has_text_cursor(app) || app.canvas.obj.is_some(),
        n if n.starts_with("Text > ") => has_text_cursor(app) || app.canvas.obj.is_some(),
        "Add Link > Webpage" | "Add Link > Email" | "Add Link > Phone Number" => has_selection(app) || objs,
        "Add Link > Bookmark" => has_selection(app) && app.wb.as_ref().is_some_and(|w| !w.bookmarks().is_empty()),
        "Remove Link" => app.cursor.is_some_and(|c| app.wb.as_ref().is_some_and(|w| w.link_at(c.storage, c.caret).is_some())),
        "Advanced > Create Style" => has_text_cursor(app),
        "Advanced > Create Page Template From This Page" => !wp,
        "Copy Style" => has_text_cursor(app) || objs,
        "Paste Style" => app.copied_style.is_some() && (has_selection(app) || objs),
        n if n.starts_with("Table > Add") || n.starts_with("Table > Delete") || n.starts_with("Table > Distribute") || n.starts_with("Header ") || n.starts_with("Footer ") => selected_table(app).is_some(),
        "Table > Merge Cells" => app.sel.is_some_and(|s| s.anchor != s.cursor),
        "Table > Unmerge Cells" => app.sel.is_some(),
        "Table > Sort Ascending" | "Table > Sort Descending" => app.sel.is_some(),
        n if n.starts_with("Formula > ") => app.sel.is_some(),
        "Chart > Show Chart Data Editor" => selected_chart(app).is_some(),
        "Image > Edit Mask" | "Image > Reset Mask" | "Image > Remove Background" | "Image > Reset Background" => selected_image(app).is_some(),
        n if n.starts_with("Mask with Shape > ") => selected_image(app).is_some(),
        "Shapes and Lines > Make Sharp Point" | "Shapes and Lines > Make Smooth Point" | "Shapes and Lines > Make Bézier Point" => app.points.selected.is_some(),
        "Shapes and Lines > Toggle Line Type" => app.points.id.is_some(),
        "Guides > Clear All Guides on Page" => crate::guides::has_guides_on_current_page(app),
        "Shapes and Lines > Make Editable" => app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(d.kind, DrawableKind::Shape(_)))),
        n if n.starts_with("Shapes and Lines > ") => app.selected_objs().len() >= 2,
        "Bring Forward" | "Bring to Front" | "Send Backward" | "Send to Back" | "Flip Horizontally" | "Flip Vertically" | "Lock" | "Unlock" => objs,
        n if n.starts_with("Align Objects > ") || n.starts_with("Distribute Objects > ") => app.selected_objs().len() >= 2,
        "Group" => app.selected_objs().len() >= 2,
        "Ungroup" => app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && matches!(d.kind, DrawableKind::Group(_)))),
        "Zoom > Zoom to Selection" => objs || has_text_cursor(app),
        n if n.starts_with("Go To > ") => app.page_count() > 1 || n == "Go To > Page…",
        _ => true,
    }
}

/// Ticks for the items that are on.
fn checked(app: &App, name: &str) -> Option<bool> {
    let st = &app.settings;
    let t = selected_table(app);
    for (prefix, kind) in [("Header Rows > ", HeaderKind::Rows), ("Header Columns > ", HeaderKind::Columns), ("Footer Rows > ", HeaderKind::Footer)] {
        if let Some(n) = name.strip_prefix(prefix) {
            let cur = t.and_then(|t| app.doc().map(|d| d.table_header_count(t, kind)));
            return Some(cur.map(|c| c.to_string()) == Some(n.to_string()));
        }
    }
    if let Some(c) = name.strip_prefix("Author Color > ") {
        return Some(crate::settings::AUTHOR_COLORS.get(st.author_color as usize).is_some_and(|(n, _)| *n == c));
    }
    Some(match name {
        "Track Changes" => app.wb.as_ref().is_some_and(|w| w.tracking()),
        "Make Layout Objects Selectable" => app.layout_selectable,
        // each offers the other kind (a package can become a single file, and back)
        "Change File Type > Single File" => app.doc().is_some_and(|d| d.package),
        "Change File Type > Package" => app.doc().is_some_and(|d| !d.package),
        "Substitutions > Smart Quotes" => st.smart_quotes,
        "Substitutions > Smart Dashes" => st.smart_dashes,
        "Substitutions > Text Replacement" => st.text_replacement,
        "Spelling and Grammar > Check Spelling While Typing" => st.spell_check,
        "Spelling and Grammar > Check Grammar With Spelling" => st.grammar,
        "Spelling and Grammar > Correct Spelling Automatically" => st.autocorrect,
        "Inspector > Format" => app.inspector && app.insp_mode == InspMode::Format,
        "Inspector > Document Setup" => app.inspector && app.insp_mode == InspMode::Document,
        "Page Thumbnails" => app.sidebar == Sidebar::Thumbnails,
        "Table of Contents" => app.sidebar == Sidebar::Toc,
        "Document Only" => app.sidebar == Sidebar::None,
        "Show Word Count" => app.word_count,
        "Show Ruler" => app.ruler,
        "Enter Screen View" => app.screen_view,
        "Show Layout" => app.show_layout,
        "Show Invisibles" => app.invisibles,
        "Show Comments Pane" => app.comments_pane,
        "Zoom > Two Pages" => app.canvas.two_up,
        "Zoom > One Page" => !app.canvas.two_up && app.canvas.fit == Fit::Page,
        "Zoom > Fit Width" => app.canvas.fit == Fit::Width,
        "Zoom > Fit Page" => app.canvas.fit == Fit::Page,
        _ => return None,
    })
}

/// A menu's items, scrolling when taller than the room down to the window's bottom edge.
fn menu_body(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let win = ui.ctx().content_rect();
    let top = ui.max_rect().min.y;
    let room = (win.max.y - top - 10.0).min(win.height() - 20.0).max(120.0);
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

fn icon(label: &str) -> Option<kn_ui::icons::Icon> {
    kn_ui::icons::menu_icon(label).or_else(|| kn_ui::icons::menu_icon(label.trim_end_matches('…')))
}

const MENU_ICON_W: f32 = 20.0;

fn paint_icon(ui: &egui::Ui, r: egui::Rect, i: kn_ui::icons::Icon, on: bool) {
    let x = r.min.x + ui.spacing().button_padding.x + MENU_ICON_W / 2.0;
    kn_ui::icons::paint_menu_icon(ui, egui::pos2(x, r.center().y), i, on);
}

fn atoms(label: String, indent: bool) -> egui::Atoms<'static> {
    use egui::IntoAtoms;
    if indent { (egui::Atom::custom(egui::IdSalt::new("menu-icon"), egui::vec2(MENU_ICON_W, 16.0)), label).into_atoms() } else { label.into_atoms() }
}

fn has_icons(items: &[Item]) -> bool {
    items.iter().any(|c| icon(&c.label).is_some())
}

fn item_ui(app: &mut App, ui: &mut egui::Ui, parent: &str, item: &Item, indent: bool, fired: &mut Option<String>) {
    if item.separator {
        ui.separator();
        return;
    }
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
    let mut name = command_name(parent, &item.label);
    let wp = app.wb.as_ref().is_some_and(|w| w.is_word_processing());
    if name == "Convert to Page Layout" && !wp {
        name = "Convert to Word Processing".into();
    }
    let label = match name.as_str() {
        "Inspector > Hide Inspector" if !app.inspector => "Show Inspector".to_string(),
        "Comments & Changes > Hide Comments" if !app.show_comments => "Show Comments".to_string(),
        "Hide Toolbar" if !app.show_toolbar => "Show Toolbar".to_string(),
        "Show Word Count" if app.word_count => "Hide Word Count".to_string(),
        "Show Ruler" if app.ruler => "Hide Ruler".to_string(),
        "Show Tab Bar" if app.tabs.show_bar => "Hide Tab Bar".to_string(),
        "Guides > Hide Guides" if !crate::guides::shown() => "Show Guides".to_string(),
        "Guides > Hide Page Template Guides" if !crate::guides::template_shown() => "Show Page Template Guides".to_string(),
        "Show Layout" if app.show_layout => "Hide Layout".to_string(),
        "Show Invisibles" if app.invisibles => "Hide Invisibles".to_string(),
        "Show Comments Pane" if app.comments_pane => "Hide Comments Pane".to_string(),
        "Enter Screen View" if app.screen_view => "Exit Screen View".to_string(),
        "Find > Hide Find Panel" if app.find.is_none() => "Show Find Panel".to_string(),
        "Convert to Word Processing" => "Convert to Word Processing".to_string(),
        "Undo" => app.hist.undo_label().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into()),
        "Redo" => app.hist.redo_label().map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into()),
        _ => item.label.clone(),
    };
    let mut b = egui::Button::new(atoms(label, indent));
    let shortcut = match name.as_str() {
        "Zoom > Zoom In" => Some("Ctrl++".to_string()),
        "Zoom > Zoom Out" => Some("Ctrl+-".to_string()),
        "Font > Bigger" => Some("Ctrl+Shift++".to_string()),
        "Font > Smaller" => Some("Ctrl+Shift+-".to_string()),
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
            // the app menu shows a page mark instead of its name (as Keydeck and Values do)
            let app_menu = m.label == "Letters";
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
                kn_ui::icons::draw(ui.painter(), egui::Rect::from_center_size(b.center() + egui::vec2(0.0, 1.0), egui::vec2(20.0, 20.0)), kn_ui::icons::Icon::Text, kn_ui::theme::TEXT());
            }
            right = right.max(r.response.rect.max.x);
        }
    });
    if let Some(cmd) = fired {
        run(app, ui.ctx(), &cmd);
    }
    right
}

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

/// Keyboard shortcuts of the menus.
pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let z = |m, k| KeyboardShortcut::new(m, k);
    if ctx.input_mut(|i| i.consume_shortcut(&z(Modifiers::COMMAND | Modifiers::SHIFT, Key::Plus)) | i.consume_shortcut(&z(Modifiers::COMMAND | Modifiers::SHIFT, Key::Equals))) {
        run(app, ctx, "Font > Bigger");
    }
    if ctx.input_mut(|i| i.consume_shortcut(&z(Modifiers::COMMAND | Modifiers::SHIFT, Key::Minus))) {
        run(app, ctx, "Font > Smaller");
    }
    if ctx.input_mut(|i| i.consume_shortcut(&z(Modifiers::COMMAND, Key::Plus)) | i.consume_shortcut(&z(Modifiers::COMMAND, Key::Equals))) {
        run(app, ctx, "Zoom > Zoom In");
    }
    if ctx.input_mut(|i| i.consume_shortcut(&z(Modifiers::COMMAND, Key::Minus))) {
        run(app, ctx, "Zoom > Zoom Out");
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F11)) {
        run(app, ctx, "Toggle Full Screen");
    }
    if ctx.input_mut(|i| i.consume_shortcut(&z(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) | i.consume_shortcut(&z(Modifiers::COMMAND, Key::Y))) {
        run(app, ctx, "Redo");
    }
    if ctx.input_mut(|i| i.consume_shortcut(&z(Modifiers::COMMAND, Key::Z))) {
        run(app, ctx, "Undo");
    }
    // tabs: Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+Page Down / Up (Linux's; the Mac's ⌃⇥ would be
    // Super+Tab, GNOME's app switcher)
    if app.tabs.len() > 1 {
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) || i.consume_key(Modifiers::CTRL, Key::PageUp)) {
            run(app, ctx, "Show Previous Tab");
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Tab) || i.consume_key(Modifiers::CTRL, Key::PageDown)) {
            run(app, ctx, "Show Next Tab");
        }
    }
    for (k, name, _) in shortcut_table() {
        if matches!(name.as_str(), "Show Next Tab" | "Show Previous Tab") {
            continue;
        }
        // keys the canvas handles itself (arrows, Delete, the clipboard events)
        if matches!(name.as_str(), "Cut" | "Copy" | "Paste" | "Delete" | "Undo" | "Redo" | "Font > Bigger" | "Font > Smaller" | "Zoom > Zoom In" | "Zoom > Zoom Out") {
            continue;
        }
        if k.modifiers.is_none() {
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
    egui::Window::new("Keyboard Shortcuts").open(&mut open).default_size([460.0, 560.0]).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
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
            fn has(items: &[Item]) -> bool {
                items.iter().any(|i| i.shortcut.is_some() || has(&i.children))
            }
            for m in menus() {
                if has(&m.children) {
                    ui.add_space(6.0);
                    ui.label(kn_ui::theme::bold(&m.label));
                    walk(ui, &m.children, &m.label);
                }
            }
            ui.add_space(6.0);
            ui.label(kn_ui::theme::bold("Text"));
            for (k, what) in [
                ("Return", "New paragraph (an empty list item ends the list)"),
                ("Shift+Return", "Line break"),
                ("Ctrl+Return", "Page break"),
                ("Tab / Shift+Tab", "Tab, or list level in a list"),
                ("Ctrl+Left / Right", "Previous / next word"),
                ("Alt+Up / Down", "Paragraph start / end"),
                ("Ctrl+Up / Down, Ctrl+Home / End", "Document start / end"),
                ("Double-click / triple-click", "Select a word / paragraph"),
                ("Escape", "Leave a text box's text (the box stays selected)"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(what);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(k).monospace()));
                });
            }
            ui.add_space(6.0);
            ui.label(kn_ui::theme::bold("Objects"));
            for (k, what) in [("Delete / Backspace", "Delete the object"), ("Arrows / Shift+arrows", "Move 1 / 10 pt"), ("Return", "Edit a text box's text"), ("Ctrl+drag a handle", "Rotate"), ("Escape", "Deselect")] {
                ui.horizontal(|ui| {
                    ui.label(what);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(k).monospace()));
                });
            }
        });
    });
    app.show_shortcuts = open;
}

/// Help > Letters Help: every command that works and its shortcut, opened in the browser.
fn help_page() {
    let mut html = String::from("<!doctype html><meta charset=utf-8><title>Letters Help</title><style>body{font:15px Cantarell,sans-serif;max-width:860px;margin:2em auto;padding:0 1em;color:#241f31}h1{color:#e5a50a}h2{margin-top:1.6em;border-bottom:1px solid #deddda}td{padding:2px 12px 2px 0}td.k{color:#5e5c64;font-family:monospace}</style><h1>Letters Help</h1><p>Letters reads and writes Apple Pages documents: word processing (text that flows from page to page) and page layout (objects on free pages). These are its menus; each command works as it does in Pages.</p>");
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
    let dir = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache"))).unwrap_or_else(std::env::temp_dir).join("letters");
    let _ = std::fs::create_dir_all(&dir);
    let page = dir.join("help.html");
    if std::fs::write(&page, html).is_ok() && std::env::var_os("CS_SCRIPT").is_none() && std::env::var_os("CS_SNAP").is_none() {
        let _ = kn_ui::open_command(&page).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }
}

/// Open a link in the browser (Ctrl-click in text).
pub fn open_link(app: &mut App, url: &str) {
    if url.starts_with("#bookmark-") {
        if let Ok(at) = url.trim_start_matches("#bookmark-").parse::<usize>() {
            if let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) {
                app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: at, caret: at, goal_x: None });
                app.canvas.follow_caret = true;
            }
        }
        return;
    }
    if std::env::var_os("CS_SCRIPT").is_none() && std::env::var_os("CS_SNAP").is_none() {
        let _ = kn_ui::open_command(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }
    app.notify(format!("Opening {url}"));
}

/// Scroll to page `p` (and put the caret at its start in a word-processing document).
pub fn go_to_page(app: &mut App, p: usize) {
    let p = p.min(app.page_count().saturating_sub(1));
    app.canvas.reveal_page(p);
    let start = app.layout.as_ref().and_then(|l| Some((l.pages.get(p)?.start, l.body.as_ref()?.storage)));
    if let (Some((s, st)), Some(b)) = (start, app.layout.as_ref().and_then(|l| l.body.as_ref())) {
        let stored = b.to_stored(s);
        crate::canvas::deselect_objects(app);
        app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: stored, caret: stored, goal_x: None });
    }
}

/// File > Convert to Page Layout (true) / Word Processing.
pub fn convert(app: &mut App, to_layout: bool) {
    if to_layout {
        // each page's text goes into a text box on that page
        let texts: Vec<(usize, String)> = match (&app.layout, &app.wb) {
            (Some(l), Some(w)) => {
                let body = w.body_text();
                l.pages
                    .iter()
                    .map(|pg| {
                        let b = l.body.as_ref();
                        let (a, e) = b.map(|b| (b.to_stored(pg.start), b.to_stored(pg.end))).unwrap_or((0, 0));
                        (pg.index, pg_core::text_plain(body.get(a..e).unwrap_or("")).trim_end().to_string())
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        app.cursor = None;
        let res = app.edit_pages("Convert to Page Layout", |p, h| p.convert_to_page_layout(h, &texts));
        if let Some(Err(e)) = res {
            app.notify(format!("Couldn't convert: {e}"));
        }
    } else {
        app.edit_pages("Convert to Word Processing", |p, h| p.convert_to_word_processing(h));
        if let Some(b) = app.wb.as_ref().and_then(|w| w.body_storage()) {
            app.cursor = Some(Cursor { storage: b, owner: Owner::Body, anchor: 0, caret: 0, goal_x: None });
        }
    }
}

/// The template chooser's templates.
pub fn new_from_template(app: &mut App, i: usize) {
    app.new_document(i == 4);
    match i {
        1 => {
            // Blank Landscape
            app.edit_pages("Landscape", |p, h| {
                let mut s = p.setup();
                s.landscape = true;
                (s.width, s.height) = (792.0, 612.0);
                p.set_setup(h, &s);
            });
        }
        2 => {
            app.edit_pages("A4", |p, h| {
                let mut s = p.setup();
                s.paper_id = "iso-a4".into();
                (s.width, s.height) = (595.28, 841.89);
                p.set_setup(h, &s);
            });
        }
        3 | 5 => {
            let st = app.wb.as_ref().and_then(|w| w.body_storage());
            if let Some(st) = st {
                let (title, body) = if i == 3 { ("Notes", "Start typing your notes here.") } else { ("Essay Title", "Begin your essay here.") };
                app.edit_pages("Template", |p, h| {
                    pg_core::edit::replace(&mut p.doc, h, st, 0, 0, &format!("{title}\n{}{body}", if i == 5 { "Introduction\n" } else { "" }));
                    if let Some(t) = pg_core::edit::para_style_named(&p.doc, "Title") {
                        pg_core::edit::set_para_style(&mut p.doc, h, st, 0, 0, t);
                    }
                    if i == 5 {
                        if let Some(hd) = pg_core::edit::para_style_named(&p.doc, "Heading") {
                            let at = title.len() + 1;
                            pg_core::edit::set_para_style(&mut p.doc, h, st, at, at, hd);
                        }
                    }
                });
            }
        }
        _ => {}
    }
    app.hist = Default::default();
    if let Some(w) = &mut app.wb {
        w.doc.dirty = false;
    }
    if let Some(b) = app.wb.as_ref().and_then(|w| w.body_storage()).filter(|_| i != 4) {
        let len = app.doc().map(|d| d.storage_text(b).len()).unwrap_or(0);
        app.cursor = Some(Cursor { storage: b, owner: Owner::Body, anchor: len, caret: len, goal_x: None });
    }
}

/// Arrange > Align Objects / Distribute Objects (Keydeck's align).
fn align(app: &mut App, name: &str) {
    let how = name.rsplit(" > ").next().unwrap_or("");
    let ds: Vec<pg_core::kn_model::Drawable> = app.selected_objs().into_iter().filter_map(|id| app.drawables.iter().find(|d| d.id == id && !d.locked).cloned()).collect();
    let items: Vec<(ObjId, (f32, f32, f32, f32))> = ds
        .iter()
        .map(|d| {
            let r = crate::canvas::frame(app, d);
            (d.id, (r.min.x, r.min.y, r.max.x, r.max.y))
        })
        .collect();
    if items.len() < 2 {
        return;
    }
    let minx = items.iter().map(|i| i.1 .0).fold(f32::INFINITY, f32::min);
    let miny = items.iter().map(|i| i.1 .1).fold(f32::INFINITY, f32::min);
    let maxx = items.iter().map(|i| i.1 .2).fold(f32::NEG_INFINITY, f32::max);
    let maxy = items.iter().map(|i| i.1 .3).fold(f32::NEG_INFINITY, f32::max);
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

/// A character change for the selection (or typing), toggling against the current state.
fn char_toggle(app: &mut App, label: &str, f: impl Fn(&pg_core::kn_model::CharStyle) -> CharChange) {
    if app.cursor.is_none() {
        // a selected text box: all of its text
        if crate::textinsp::target(app).is_none() {
            return;
        }
    }
    let cs = current_char(app).unwrap_or_default();
    crate::textinsp::apply_char(app, label, f(&cs));
}

/// The character look at the caret / the start of the selection.
pub(crate) fn current_char(app: &App) -> Option<pg_core::kn_model::CharStyle> {
    let t = crate::textinsp::target(app)?;
    let d = app.doc()?;
    let ps = pg_core::edit::para_style_at(d, t.storage, t.a);
    let cs = pg_core::edit::char_style_at(d, t.storage, if t.a < t.b { t.a + 1 } else { t.a });
    let mut s = pg_core::kn_model::style::char_style(&d.file, ps, cs);
    for p in &app.typing_props {
        match p {
            CharChange::Bold(b) => s.bold = *b,
            CharChange::Italic(b) => s.italic = *b,
            CharChange::Underline(b) => s.underline = *b,
            CharChange::Strike(b) => s.strike = *b,
            CharChange::Size(v) => s.size = *v,
            _ => {}
        }
    }
    Some(s)
}

/// Raw char-property edit of the selection (spacing, ligatures, capitalisation…).
fn char_prop(app: &mut App, label: &str, f: impl Fn(&mut pg_core::kn_format::Msg) + 'static) {
    let Some(t) = crate::textinsp::target(app) else { return };
    if t.a >= t.b {
        return;
    }
    let st = t.storage;
    app.edit(label, |d, h| {
        pg_core::edit::set_char_props(d, h, st, t.a, t.b, &f);
    });
}

/// Objects are selected and none of them has its text being edited: Edit commands act on them
/// (the body caret stays, hidden, under a selected object).
fn objects_focus(app: &App) -> bool {
    !app.selected_objs().is_empty() && app.canvas.editing.is_none() && app.sel.is_none()
}

pub fn run(app: &mut App, ctx: &egui::Context, name: &str) {
    if let Some(p) = name.strip_prefix("open:") {
        crate::tabs::open_in_tab(app, std::path::Path::new(p));
        return;
    }
    let body_at = |app: &App| app.cursor.filter(|c| c.owner == Owner::Body).map(|c| (c.storage, c.range().0));
    match name {
        n if crate::tables::is_command(n) => {
            crate::tables::run_command(app, n);
        }
        "Conditional Highlighting…" => crate::condwin::show(),
        "Choose File" => { crate::media::command(app, "Choose File…"); }
        "Browse Content Hub" | "Show Content Hub" => app.hub.show_hub(),
        n if crate::media::command(app, n) => {}
        // Letters menu
        "About Letters" => app.show_about = true,
        "Support Development…" => kn_ui::open_url(kn_ui::DONATE_URL),
        "Settings…" => app.show_settings = true,
        "Close" => crate::tabs::request_close(app, false),
        "Quit Letters" | "Close All" => crate::tabs::request_close(app, true),
        n if crate::tabs::command(app, n) => {}
        // File
        "New…" => {
            if app.doc().is_some_and(|d| d.dirty || d.path.is_some()) {
                crate::tabs::new_tab(app);
            } else {
                app.chooser = true;
            }
        }
        "Open…" => app.open_dialog(),
        "Save" => app.save(false),
        "Save As…" => app.save(true),
        "Duplicate" => crate::fileops::duplicate(app),
        "Rename…" => {
            let stem = app.title().trim_end_matches(".pages").to_string();
            let dir = app.doc().and_then(|d| d.path.clone()).and_then(|p| p.parent().map(|p| p.to_path_buf()));
            app.doc_title = Some((stem, dir));
        }
        "Revert To > Browse All Versions…" => app.show_versions = true,
        n if n.starts_with("Export To > ") => crate::fileops::export_dialog(app, n.trim_start_matches("Export To > ")),
        "Convert to Page Layout" => convert(app, true),
        "Convert to Word Processing" => convert(app, false),
        "Change File Type > Single File" | "Change File Type > Package" => {
            let pkg = name.ends_with("Package");
            if let Some(w) = &mut app.wb {
                w.doc.package = pkg;
                w.doc.dirty = true;
            }
            app.notify(if pkg { "Saves as a package (a folder) from now on." } else { "Saves as a single file from now on." });
        }
        "Set Password…" => app.open_set_password(),
        "Page Setup…" => {
            app.inspector = true;
            app.insp_mode = InspMode::Document;
            app.doc_tab = 0;
        }
        "Print…" => crate::fileops::print(app),
        // Edit
        "Undo" => app.undo(),
        "Redo" => app.redo(),
        "Cut" | "Copy" if app.sel.is_some() && app.cursor.is_none() => {
            // the selected cells, as tab-separated text
            if let Some(tsv) = crate::tables::copy_cells(app) {
                ctx.copy_text(tsv);
                if name == "Cut" {
                    crate::tables::clear_cells(app, "Cut");
                }
            }
        }
        "Cut" | "Copy" => {
            let cut = name == "Cut";
            if !objects_focus(app) && app.cursor.is_some_and(|c| !c.is_empty()) {
                crate::text::copy(app, ctx, cut);
            } else if !app.selected_objs().is_empty() {
                crate::insert::copy_objects(app, ctx);
                if cut {
                    crate::insert::delete_selected(app);
                }
            }
        }
        "Paste" | "Paste and Preserve Style" | "Paste and Match Style" if app.sel.is_some() && app.cursor.is_none() => {
            // into the selected cells (the system clipboard comes back as a Paste event)
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        "Paste" | "Paste and Preserve Style" | "Paste and Match Style" => {
            if app.obj_clip_active && (app.cursor.is_none() || objects_focus(app)) {
                crate::insert::paste_objects(app, 20.0);
            } else if let Some(clip) = app.text_clip.clone() {
                crate::text::paste(app, &clip.text, name == "Paste and Match Style");
            } else {
                // the system clipboard: egui sends it as a Paste event; ask for it
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
        }
        "Delete" if app.sel.is_some() && app.cursor.is_none() => crate::tables::clear_cells(app, "Delete"),
        "Delete" => {
            if !objects_focus(app) && app.cursor.is_some_and(|c| !c.is_empty()) {
                crate::text::replace_selection(app, "", "Delete", false);
            } else {
                crate::insert::delete_selected(app);
            }
        }
        "Clear All" => {
            // the selected cells' contents (Pages' Edit > Clear All), or the selected text
            if let Some(sl) = app.sel {
                let (r0, c0, r1, c1) = sl.block();
                let t = sl.table;
                app.edit("Clear All", |d, h| {
                    for r in r0..=r1 {
                        for c in c0..=c1 {
                            let _ = d.set_cell(h, t, r, c, &pg_core::kn_model::tablecells::CellInput::Empty);
                        }
                    }
                    d.recalc_formulas(h, t);
                });
            } else if app.cursor.is_some_and(|c| !c.is_empty()) {
                crate::text::replace_selection(app, "", "Clear All", false);
            }
        }
        "Duplicate Selection" => crate::insert::duplicate(app, ctx),
        "Select All" => {
            if let Some(c) = app.cursor {
                let n = app.doc().map(|d| d.storage_text(c.storage).len()).unwrap_or(0);
                app.cursor = Some(Cursor { anchor: 0, caret: n, ..c });
            } else if app.wb.as_ref().is_some_and(|w| !w.is_word_processing()) {
                let page = app.current_page();
                let ids: Vec<ObjId> = app.drawables.iter().filter(|d| app.obj_page.get(&d.id) == Some(&page)).map(|d| d.id).collect();
                app.canvas.obj = ids.first().copied();
                app.canvas.objs = ids;
            } else if let Some(b) = app.wb.as_ref().and_then(|w| w.body_storage()) {
                let n = app.doc().map(|d| d.storage_text(b).len()).unwrap_or(0);
                app.cursor = Some(Cursor { storage: b, owner: Owner::Body, anchor: 0, caret: n, goal_x: None });
            }
        }
        "Deselect All" => {
            if let Some(c) = app.cursor {
                app.cursor = Some(Cursor { anchor: c.caret, ..c });
            }
            crate::canvas::deselect_objects(app);
        }
        "Track Changes" => {
            let on = !app.wb.as_ref().is_some_and(|w| w.tracking());
            app.edit_pages("Track Changes", |p, h| p.set_tracking(h, on));
        }
        "Accept Change" => crate::notes::accept(app, true),
        "Reject Change" => crate::notes::accept(app, false),
        "Remove Highlights and Comments" => {
            if let Some(b) = app.wb.as_ref().and_then(|w| w.body_storage()) {
                app.edit_pages("Remove Highlights and Comments", |p, h| p.remove_all_comments(h, b));
            }
        }
        "Find > Find…" => {
            let q = crate::text::selected_stored(app).map(|s| pg_core::text_plain(&s)).filter(|s| !s.contains('\n')).unwrap_or_default();
            let m = crate::find::search(app, &q);
            app.find = Some((q, m, 0));
        }
        "Find > Find Next" => crate::find::step(app, true),
        "Find > Find Previous" => crate::find::step(app, false),
        "Find > Hide Find Panel" => {
            if app.find.is_some() {
                app.find = None;
                app.replace = None;
            } else {
                app.find = Some((String::new(), Vec::new(), 0));
            }
        }
        "Find > Use Selection for Find" => {
            if let Some(q) = crate::text::selected_stored(app).map(|s| pg_core::text_plain(&s)) {
                let m = crate::find::search(app, &q);
                app.find = Some((q, m, 0));
            }
        }
        "Find > Use Selection for Replace" => {
            if let Some(q) = crate::text::selected_stored(app).map(|s| pg_core::text_plain(&s)) {
                app.replace = Some(q);
                if app.find.is_none() {
                    app.find = Some((String::new(), Vec::new(), 0));
                }
            }
        }
        "Find > Jump to Selection" => app.canvas.follow_caret = true,
        "Spelling and Grammar > Show Spelling and Grammar" | "Spelling and Grammar > Check Document Now" => crate::spelling::open(app),
        "Spelling and Grammar > Check Spelling While Typing" => {
            app.settings.spell_check = !app.settings.spell_check;
            app.settings.save();
        }
        "Spelling and Grammar > Check Grammar With Spelling" => {
            app.settings.grammar = !app.settings.grammar;
            app.settings.save();
        }
        "Spelling and Grammar > Correct Spelling Automatically" => {
            app.settings.autocorrect = !app.settings.autocorrect;
            app.settings.save();
        }
        "Substitutions > Smart Quotes" => {
            app.settings.smart_quotes = !app.settings.smart_quotes;
            app.settings.save();
        }
        "Substitutions > Smart Dashes" => {
            app.settings.smart_dashes = !app.settings.smart_dashes;
            app.settings.save();
        }
        "Substitutions > Text Replacement" => {
            app.settings.text_replacement = !app.settings.text_replacement;
            app.settings.save();
        }
        n if n.starts_with("Transformations > ") => {
            let how = match n {
                "Transformations > Make Upper Case" => "upper",
                "Transformations > Make Lower Case" => "lower",
                _ => "title",
            };
            if let Some(c) = app.cursor.filter(|c| !c.is_empty()) {
                let (a, b) = c.range();
                let st = c.storage;
                app.edit("Transformations", |d, h| d.transform_case(h, st, a, b, how));
            }
        }
        // Insert
        "Add Page" => crate::toolbar::add_page(app),
        "Delete Page" => {
            // page layout: the current page and its objects (Pages: page thumbnail > Delete)
            if app.wb.as_ref().is_some_and(|w| !w.is_word_processing()) && app.page_count() > 1 {
                let at = app.current_page();
                app.edit_pages("Delete Page", |p, h| p.delete_layout_page(h, at));
                app.refresh();
            }
        }
        "Page Break" => crate::text::replace_selection(app, "\u{5}", "Page Break", false),
        "Column Break" => crate::text::replace_selection(app, "\u{c}", "Column Break", false),
        "Line Break" => crate::text::replace_selection(app, "\u{2028}", "Line Break", false),
        "Section Break" => {
            if let Some((_, at)) = body_at(app) {
                let res = app.edit_pages("Section Break", |p, h| p.insert_section_break(h, at));
                match res {
                    Some(Ok(_)) => {
                        if let Some(c) = app.cursor.as_mut() {
                            c.anchor = at + 1;
                            c.caret = at + 1;
                        }
                    }
                    Some(Err(e)) => app.notify(format!("Couldn't insert a section break: {e}")),
                    None => {}
                }
            }
        }
        "Page Number" | "Page Count" => {
            if let Some(c) = app.cursor {
                let at = c.range().0;
                let count = name == "Page Count";
                app.edit("Insert Page Number", |d, h| pg_core::edit::insert_page_field(d, h, c.storage, at, count));
                app.cursor = Some(Cursor { anchor: at + 3, caret: at + 3, ..c });
            }
        }
        "Date & Time" => {
            if let Some(c) = app.cursor {
                let at = c.range().0;
                let n = app.edit_pages("Insert Date & Time", |p, h| p.insert_date(h, c.storage, at)).unwrap_or(0);
                app.cursor = Some(Cursor { anchor: at + n, caret: at + n, ..c });
            }
        }
        "Table of Contents > Document" | "Table of Contents > Section" => {
            if let Some((_, at)) = body_at(app) {
                if let Some(Err(e)) = app.edit_pages("Insert Table of Contents", |p, h| p.insert_toc(h, at)) {
                    app.notify(format!("Couldn't insert the table of contents: {e}"));
                }
            }
        }
        "Footnote" => {
            if let Some((_, at)) = body_at(app) {
                match app.edit_pages("Insert Footnote", |p, h| p.insert_footnote(h, at)) {
                    Some(Ok(fs)) => {
                        // the caret goes into the new footnote, after its mark
                        app.refresh();
                        let page = app.layout.as_ref().and_then(|l| l.pages.iter().position(|p| p.footnotes.iter().any(|f| f.storage == fs))).unwrap_or(0);
                        let len = app.doc().map(|d| d.storage_text(fs).len()).unwrap_or(0);
                        app.cursor = Some(Cursor { storage: fs, owner: Owner::Footnote { page }, anchor: len, caret: len, goal_x: None });
                        app.canvas.follow_caret = true;
                    }
                    Some(Err(e)) => app.notify(format!("Couldn't insert a footnote: {e}")),
                    None => {}
                }
            }
        }
        "Text" => crate::insert::insert_object(app, NewObject::TextBox),
        n if n.starts_with("Table > ") && TABLE_ITEMS.contains(&n.trim_start_matches("Table > ")) => crate::insert::insert_object(app, NewObject::Table),
        n if n.starts_with("Chart > 2D ") || n.starts_with("Chart > 3D ") || n.starts_with("Chart > Interactive ") => {
            // Pages' column chart, then switched to the type picked
            crate::insert::insert_object(app, NewObject::Chart);
            if let (Some(ty), Some(c)) = (chart_type_for(n), selected_chart(app)) {
                if ty != "column" {
                    app.edit("Chart Type", |d, h| d.set_chart_type(h, c, ty));
                }
            }
        }
        n if n.starts_with("Chart Type > ") => {
            if let (Some(ty), Some(c)) = (chart_type_for(n), selected_chart(app)) {
                app.edit("Chart Type", |d, h| d.set_chart_type(h, c, ty));
            }
        }
        n if n.starts_with("Chart > ") && chart_toggle(&n["Chart > ".len()..]).is_some() => {
            use pg_core::kn_model::chartdata::ChartFlag;
            let Some(c) = selected_chart(app) else { return };
            let Some(ch) = app.drawables.iter().find(|d| d.id == c).and_then(|d| match &d.kind {
                DrawableKind::Chart(ch) => Some(ch.clone()),
                _ => None,
            }) else {
                return;
            };
            let which = chart_toggle(&n["Chart > ".len()..]).unwrap();
            match which {
                "title" => {
                    let next = if ch.title.is_some() { None } else { Some("Title") };
                    app.edit("Chart Title", |d, h| d.set_chart_title(h, c, next));
                }
                "legend" => {
                    let show = !ch.show_legend;
                    app.edit("Legend", |d, h| d.set_chart_legend(h, c, show));
                }
                _ => {
                    let y = ch.value_axes.first();
                    let y2 = ch.value_axes.get(1);
                    let flag = match which {
                        "border" => ChartFlag::Border(!ch.show_border),
                        "y-name" => ChartFlag::Axis(Some(0), "showtitle", !y.is_some_and(|a| a.title.is_some())),
                        "y-labels" => ChartFlag::Axis(Some(0), "showlabels", !y.is_some_and(|a| a.show_labels)),
                        "y2-name" => ChartFlag::Axis(Some(1), "showtitle", !y2.is_some_and(|a| a.title.is_some())),
                        "x-name" => ChartFlag::Axis(None, "showtitle", ch.category_axis.title.is_none()),
                        "x-labels" => ChartFlag::Axis(None, "showlabels", !ch.category_axis.show_labels),
                        "series-names" => ChartFlag::SeriesNames(!ch.series.first().is_some_and(|s| s.show_series_names)),
                        _ => ChartFlag::ValueLabels(!ch.series.first().is_some_and(|s| s.show_value_labels)),
                    };
                    app.edit("Chart", |d, h| d.set_chart_flag(h, c, flag));
                }
            }
        }
        n if n.starts_with("Shape > ") => {
            if let Some(s) = shape_named(n.trim_start_matches("Shape > ")) {
                crate::insert::insert_object(app, NewObject::Shape(s));
            }
        }
        "Line > Straight Connection Line" | "Line > Curved Connection Line" | "Line > Right Angle Connection Line" => {
            use pg_core::kn_model::insert::Connection;
            let kind = match name {
                "Line > Curved Connection Line" => Connection::Curved,
                "Line > Right Angle Connection Line" => Connection::Angled,
                _ => Connection::Straight,
            };
            // with two objects selected, the line joins them
            let sel = app.selected_objs();
            let ends = (sel.len() == 2).then(|| (sel[0], sel[1]));
            let pg_core::objects::Placement::Floating { page, x, y } = crate::insert::floating_spot(app, 300.0, 200.0) else { return };
            match app.edit_pages("Insert Line", |p, h| p.insert_connection_line(h, kind, ends, page, (x, y))) {
                Some(Ok(id)) => {
                    app.refresh();
                    crate::canvas::select_object(app, id);
                }
                Some(Err(e)) => app.notify(format!("Couldn't insert the line: {e}")),
                None => {}
            }
        }
        n if n.starts_with("Line > ") => {
            crate::insert::insert_object(app, NewObject::Line);
            let arrows = match n {
                "Line > Line with Arrowhead" => 1,
                "Line > Line with Two Arrowheads" => 2,
                _ => 0,
            };
            if arrows > 0 {
                if let Some(id) = app.canvas.obj {
                    let end = pg_core::kn_model::insert::arrow_line_end();
                    app.edit("Line Ends", |d, h| {
                        d.set_line_ends(h, id, if arrows == 2 { Some(&end) } else { None }, Some(&end));
                    });
                }
            }
        }
        "Highlight" => {
            if let Some(c) = app.cursor.filter(|c| !c.is_empty()) {
                let (a, b) = c.range();
                let who = crate::text::author(app);
                app.edit_pages("Highlight", |p, h| p.add_text_comment(h, c.storage, a, b, "", &who));
                app.show_comments = true;
            }
        }
        "Comment" => app.comment_edit = Some((None, String::new())),
        "Bookmark" => {
            if let Some(c) = app.cursor {
                let (mut a, mut b) = c.range();
                if a == b {
                    let t = app.doc().map(|d| d.storage_text(c.storage)).unwrap_or_default();
                    (a, b) = crate::canvas::word_at(&t, a);
                }
                app.edit_pages("Add Bookmark", |p, h| p.add_bookmark(h, c.storage, a, b));
                app.notify("Bookmark added (Document inspector > Bookmarks).");
            }
        }
        "Equation…" => app.equation = Some(String::new()),
        n if n.starts_with("Formula > ") && n != "Formula > New Formula" => {
            if let Some(s) = app.sel {
                let f = match n.trim_start_matches("Formula > ") {
                    "Sum" => "SUM",
                    "Average" => "AVERAGE",
                    "Minimum" => "MIN",
                    "Maximum" => "MAX",
                    "Count" => "COUNT",
                    _ => "PRODUCT",
                };
                if let Some(func) = pg_core::kn_model::tableformula::Func::from_menu(f) {
                    let (t, r, c) = (s.table, s.cursor.0, s.cursor.1);
                    if let Some(Err(e)) = app.edit("Insert Formula", |d, h| d.insert_formula(h, t, r, c, func)) {
                        app.notify(format!("Couldn't insert the formula: {e}"));
                    }
                }
            }
        }
        // Format > Font
        "Font > Show Fonts" => app.show_fonts = !app.show_fonts,
        "Font > Bold" => char_toggle(app, "Bold", |c| CharChange::Bold(!c.bold)),
        "Font > Italic" => char_toggle(app, "Italic", |c| CharChange::Italic(!c.italic)),
        "Font > Underline" => char_toggle(app, "Underline", |c| CharChange::Underline(!c.underline)),
        "Font > Strikethrough" => char_toggle(app, "Strikethrough", |c| CharChange::Strike(!c.strike)),
        "Font > Bigger" => char_toggle(app, "Bigger", |c| CharChange::Size((c.size + 1.0).round())),
        "Font > Smaller" => char_toggle(app, "Smaller", |c| CharChange::Size((c.size - 1.0).max(1.0).round())),
        "Font > Replace Fonts…" => crate::fonts::open_replace(app),
        "Character Spacing > Use None" => char_prop(app, "Character Spacing", |cp| cp.set_f32("tracking", 0.0)),
        "Character Spacing > Tighten" | "Character Spacing > Loosen" => {
            let cur = current_char(app).map(|c| c.tracking).unwrap_or(0.0);
            let v = (cur + if name.ends_with("Tighten") { -0.01 } else { 0.01 }).clamp(-0.5, 2.0);
            char_prop(app, "Character Spacing", move |cp| cp.set_f32("tracking", v));
        }
        n if n.starts_with("Ligature > ") => {
            let v = match n {
                "Ligature > Use None" => "kRequiredLigatures",
                "Ligature > Use All" => "kAllLigatures",
                _ => "kStandardLigatures",
            };
            char_prop(app, "Ligatures", move |cp| cp.set_enum("ligatures", v));
        }
        "Baseline > Use Default" => char_toggle(app, "Baseline", |_| CharChange::Script(0)),
        "Baseline > Superscript" => char_toggle(app, "Superscript", |c| CharChange::Script(if c.script == 1 { 0 } else { 1 })),
        "Baseline > Subscript" => char_toggle(app, "Subscript", |c| CharChange::Script(if c.script == -1 { 0 } else { -1 })),
        "Baseline > Raise" | "Baseline > Lower" => {
            let cur = current_char(app).map(|c| c.baseline_shift).unwrap_or(0.0);
            let v = cur + if name.ends_with("Raise") { 1.0 } else { -1.0 };
            char_prop(app, "Baseline Shift", move |cp| cp.set_f32("baseline_shift", v));
        }
        n if n.starts_with("Capitalization > ") => {
            let v = n.trim_start_matches("Capitalization > ").to_string();
            char_prop(app, "Capitalization", move |cp| crate::text::set_caps(cp, &v));
        }
        // Format > Text
        "Text > Align Left" | "Text > Align Center" | "Text > Align Right" | "Text > Justify" => {
            let v = match name {
                "Text > Align Center" => "TATvalue2",
                "Text > Align Right" => "TATvalue1",
                "Text > Justify" => "TATvalue3",
                _ => "TATvalue0",
            };
            crate::textinsp::apply_para(app, "Alignment", move |pp| pp.set_enum("alignment", v));
        }
        "Text > Increase Indent Level" | "Text > Decrease Indent Level" => {
            if let Some(t) = crate::textinsp::target(app) {
                let d = if name.contains("Increase") { 1 } else { -1 };
                app.edit("Indent", |doc, h| pg_core::edit::change_level(doc, h, t.storage, t.a, t.b, d));
            }
        }
        // Format > Table
        n if n.starts_with("Table > ") => table_command(app, n),
        n if n.starts_with("Header Rows > ") || n.starts_with("Header Columns > ") || n.starts_with("Footer Rows > ") => {
            if let Some(t) = selected_table(app) {
                let kind = if n.starts_with("Header Rows") { HeaderKind::Rows } else if n.starts_with("Header Columns") { HeaderKind::Columns } else { HeaderKind::Footer };
                let v: u32 = n.rsplit(" > ").next().and_then(|x| x.parse().ok()).unwrap_or(0);
                if let Some(Err(e)) = app.edit("Headers", |d, h| d.set_table_header_count(h, t, kind, v)) {
                    app.notify(format!("{e}"));
                }
            }
        }
        "Chart > Show Chart Data Editor" => {
            if let Some(c) = selected_chart(app) {
                crate::chartedit::open(app, c);
            }
        }
        "Copy Style" => crate::editing::copy_style(app),
        "Paste Style" => crate::editing::paste_style(app),
        n if n.starts_with("Add Link > ") => app.link_edit = Some((n.trim_start_matches("Add Link > ").to_string(), String::new())),
        "Remove Link" => {
            if let Some(c) = app.cursor {
                app.edit_pages("Remove Link", |p, h| p.remove_link(h, c.storage, c.caret));
            }
        }
        "Advanced > Create Style" => app.new_style = Some("New Style".into()),
        "Advanced > Create Page Template From This Page" => app.notify("This page's layout is kept as the document's page template."),
        "Image > Edit Mask" => app.mask_edit = if app.mask_edit.is_some() { None } else { selected_image(app) },
        "Image > Remove Background" | "Image > Reset Background" => {
            let Some(id) = selected_image(app) else { return };
            let Some(doc) = app.doc() else { return };
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
        "Image > Reset Mask" => {
            if let Some(i) = selected_image(app) {
                app.edit("Reset Mask", |d, h| d.reset_mask(h, i));
            }
        }
        n if n.starts_with("Mask with Shape > ") => {
            if let (Some(i), Some(s)) = (selected_image(app), shape_named(n.trim_start_matches("Mask with Shape > "))) {
                app.edit("Mask with Shape", |d, h| d.mask_with_shape(h, i, s));
            }
        }
        "Shapes and Lines > Make Editable" => {
            crate::points::make_editable(app);
        }
        "Auto Crop" => crate::mask::auto_crop(app),
        "Shapes and Lines > Make Sharp Point" => crate::points::set_kind(app, pg_core::kn_model::points::NodeKind::Sharp),
        "Shapes and Lines > Make Smooth Point" => crate::points::set_kind(app, pg_core::kn_model::points::NodeKind::Smooth),
        "Shapes and Lines > Make Bézier Point" => crate::points::set_kind(app, pg_core::kn_model::points::NodeKind::Bezier),
        "Shapes and Lines > Break Apart" => crate::editing::break_apart(app),
        "Shapes and Lines > Toggle Line Type" => crate::points::toggle_line_type(app),
        "Shapes and Lines > Save to My Shapes" => crate::editing::save_to_my_shapes(app),
        "Shapes and Lines > Reset Text and Object Handles" => {
            let ids = app.selected_objs();
            app.edit("Reset Handles", |d, h| {
                for id in &ids {
                    d.reset_handles(h, *id);
                }
            });
        }
        n if n.starts_with("Shapes and Lines > ") => {
            let op = match n.trim_start_matches("Shapes and Lines > ") {
                "Unite Shapes" => pg_core::kn_model::boolops::BoolOp::Unite,
                "Intersect Shapes" => pg_core::kn_model::boolops::BoolOp::Intersect,
                "Subtract Shapes" => pg_core::kn_model::boolops::BoolOp::Subtract,
                _ => pg_core::kn_model::boolops::BoolOp::Exclude,
            };
            crate::editing::bool_op(app, op);
        }
        // Arrange
        "Bring Forward" | "Bring to Front" | "Send Backward" | "Send to Back" => {
            let how = match name {
                "Bring Forward" => ZMove::Forward,
                "Bring to Front" => ZMove::Front,
                "Send Backward" => ZMove::Backward,
                _ => ZMove::Back,
            };
            let ids = app.selected_objs();
            app.edit_pages(name, |p, h| p.reorder(h, &ids, how));
        }
        n if n.starts_with("Align Objects > ") || n.starts_with("Distribute Objects > ") => align(app, n),
        "Flip Horizontally" | "Flip Vertically" => {
            // the frames as shown (a shape's flip lives on its path source)
            let frames: Vec<(ObjId, pg_core::kn_model::Geometry)> = app.selected_objs().iter().filter_map(|id| app.drawables.iter().find(|d| d.id == *id).map(|d| (d.id, d.geom))).collect();
            let hz = name == "Flip Horizontally";
            app.edit(name, |d, h| {
                for (id, mut g) in frames {
                    if hz {
                        g.hflip = !g.hflip;
                    } else {
                        g.vflip = !g.vflip;
                    }
                    d.set_frame(h, id, g);
                }
            });
        }
        "Lock" | "Unlock" => {
            let ids = app.selected_objs();
            let lock = name == "Lock";
            app.edit(name, |d, h| {
                for id in &ids {
                    d.set_locked(h, *id, lock);
                }
            });
        }
        "Move Objects to Section Layout" => {
            // floating objects onto their page's template (wallpaper); selected layout objects
            // back onto the page they were picked on
            let ids = app.selected_objs();
            let layout_ids: Vec<ObjId> = ids.iter().copied().filter(|i| app.layout_drawables.iter().any(|d| d.id == *i)).collect();
            if !layout_ids.is_empty() {
                let page = layout_ids.first().and_then(|i| app.obj_page.get(i).copied()).unwrap_or_else(|| app.current_page());
                app.edit_pages("Move Objects to Page", |p, h| p.move_from_section_layout(h, &layout_ids, page));
            } else if let Some(l) = app.layout.as_ref() {
                let floating: Vec<ObjId> = ids.iter().copied().filter(|i| app.wb.as_ref().is_some_and(|w| w.floating_page(*i).is_some())).collect();
                let page = floating.first().and_then(|i| app.obj_page.get(i).copied()).unwrap_or(0);
                if let Some(t) = app.wb.as_ref().and_then(|w| w.page_template_of(l, page)) {
                    if floating.is_empty() {
                        app.notify("Only objects that stay on the page can go to the section layout");
                    } else {
                        app.edit_pages("Move Objects to Section Layout", |p, h| p.move_to_section_layout(h, &floating, t));
                        crate::canvas::deselect_objects(app);
                    }
                }
            }
        }
        "Make Layout Objects Selectable" => {
            app.layout_selectable = !app.layout_selectable;
            crate::canvas::deselect_objects(app);
        }
        "Group" => crate::editing::group(app),
        "Ungroup" => crate::editing::ungroup(app),
        // View
        "Inspector > Format" => app.toggle_inspector(InspMode::Format),
        "Inspector > Document Setup" => app.toggle_inspector(InspMode::Document),
        "Inspector > Hide Inspector" => app.inspector = !app.inspector,
        "Inspector > Show Next Inspector Tab" | "Inspector > Show Previous Inspector Tab" => {
            let d: isize = if name.contains("Next") { 1 } else { -1 };
            if app.insp_mode == InspMode::Document {
                app.doc_tab = (app.doc_tab as isize + d).rem_euclid(3) as usize;
            } else {
                app.obj_tab = (app.obj_tab as isize + d).rem_euclid(3) as usize;
                app.text_tab = (app.text_tab as isize + d).rem_euclid(3) as usize;
            }
        }
        "Page Thumbnails" => app.sidebar = if app.sidebar == Sidebar::Thumbnails { Sidebar::None } else { Sidebar::Thumbnails },
        "Table of Contents" => app.sidebar = if app.sidebar == Sidebar::Toc { Sidebar::None } else { Sidebar::Toc },
        "Document Only" => app.sidebar = Sidebar::None,
        "Show Word Count" => app.word_count = !app.word_count,
        "Show Ruler" => app.ruler = !app.ruler,
        n if crate::guides::command(app, n) => {}
        "Go To > Next Page" => go_to_page(app, app.current_page() + 1),
        "Go To > Previous Page" => go_to_page(app, app.current_page().saturating_sub(1)),
        "Go To > First Page" => go_to_page(app, 0),
        "Go To > Last Page" => go_to_page(app, app.page_count().saturating_sub(1)),
        "Go To > Page…" => app.go_to = Some(String::new()),
        "Enter Screen View" => {
            app.screen_view = !app.screen_view;
            if app.screen_view {
                // the page follows the window, so the zoom can't follow the page
                app.canvas.fit = Fit::None;
                app.canvas.two_up = false;
            }
        }
        "Show Layout" => app.show_layout = !app.show_layout,
        "Show Invisibles" => app.invisibles = !app.invisibles,
        "Show Comments Pane" => app.comments_pane = !app.comments_pane,
        "Comments & Changes > Hide Comments" => app.show_comments = !app.show_comments,
        "Comments & Changes > Show Next Comment or Change" => crate::notes::step(app, true),
        "Comments & Changes > Show Previous Comment or Change" => crate::notes::step(app, false),
        n if n.starts_with("Author Color > ") => {
            if let Some(i) = crate::settings::AUTHOR_COLORS.iter().position(|(c, _)| *c == n.trim_start_matches("Author Color > ")) {
                app.settings.author_color = i as u8;
                app.settings.save();
                let (author, col) = (crate::text::author(app), crate::settings::AUTHOR_COLORS[i].1);
                let c = pg_core::kn_model::Color { r: ((col >> 16) & 255) as f32 / 255.0, g: ((col >> 8) & 255) as f32 / 255.0, b: (col & 255) as f32 / 255.0, a: 1.0 };
                app.edit("Author Color", |d, h| d.set_author_color(h, &author, c));
            }
        }
        "Show Colors" => app.show_colors = !app.show_colors,
        "Zoom > One Page" => {
            app.canvas.two_up = false;
            app.canvas.fit = Fit::Page;
        }
        "Zoom > Two Pages" => {
            // Pages: One Page / Two Pages are a choice, not a toggle
            app.canvas.two_up = true;
            app.canvas.fit = Fit::Page;
        }
        "Zoom > Zoom In" => {
            let z = crate::chrome::ZOOMS.iter().copied().find(|z| *z > app.canvas.zoom_goal() + 1e-3).unwrap_or(4.0);
            app.canvas.animate_zoom(z);
        }
        "Zoom > Zoom Out" => {
            let z = crate::chrome::ZOOMS.iter().rev().copied().find(|z| *z < app.canvas.zoom_goal() - 1e-3).unwrap_or(0.25);
            app.canvas.animate_zoom(z);
        }
        "Zoom > Actual Size" => app.canvas.animate_zoom(1.0),
        "Zoom > Fit Width" => app.canvas.fit = Fit::Width,
        "Zoom > Fit Page" => app.canvas.fit = Fit::Page,
        "Zoom > Zoom to Selection" => {
            app.canvas.animate_zoom(2.0);
            app.canvas.follow_caret = true;
        }
        "Hide Toolbar" => app.show_toolbar = !app.show_toolbar,
        "Customize Toolbar…" => app.tb.customizing = true,
        // Window, Help
        "Minimize" => crate::chrome::minimize(ctx),
        "Zoom" => {
            let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
        }
        "Toggle Full Screen" => {
            let f = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!f));
        }
        "Letters Help" => help_page(),
        "Keyboard Shortcuts" => app.show_shortcuts = true,
        other => {
            if std::env::var_os("CS_DEBUG").is_some() {
                eprintln!("menu: {other} not handled");
            }
        }
    }
}

fn table_command(app: &mut App, n: &str) {
    let Some(t) = selected_table(app) else { return };
    let (r, c) = app.sel.map(|s| s.cursor).unwrap_or((0, 0));
    let res = match n {
        "Table > Add Row Above" => app.edit("Add Row", |d, h| d.table_insert(h, t, Axis::Row, r, r)),
        "Table > Add Row Below" => app.edit("Add Row", |d, h| d.table_insert(h, t, Axis::Row, r + 1, r)),
        "Table > Add Column Before" => app.edit("Add Column", |d, h| d.table_insert(h, t, Axis::Column, c, c)),
        "Table > Add Column After" => app.edit("Add Column", |d, h| d.table_insert(h, t, Axis::Column, c + 1, c)),
        "Table > Delete Row" => app.edit("Delete Row", |d, h| d.table_delete(h, t, Axis::Row, r)),
        "Table > Delete Column" => app.edit("Delete Column", |d, h| d.table_delete(h, t, Axis::Column, c)),
        "Table > Distribute Rows Evenly" => {
            app.edit("Distribute Rows", |d, h| {
                d.distribute_table(h, t, true);
            });
            None
        }
        "Table > Distribute Columns Evenly" => {
            app.edit("Distribute Columns", |d, h| {
                d.distribute_table(h, t, false);
            });
            None
        }
        "Table > Sort Ascending" | "Table > Sort Descending" => {
            // the body rows by the selected cell's column
            let asc = n.ends_with("Ascending");
            app.edit("Sort", |d, h| d.sort_table_rows(h, t, c, asc)).and_then(|r| r.err()).map(Err)
        }
        "Table > Merge Cells" => app.sel.and_then(|s| {
            let (r0, c0, r1, c1) = s.block();
            app.edit("Merge Cells", |d, h| d.merge_cells(h, t, pg_core::kn_model::CellRange { row: r0, col: c0, rows: r1 - r0 + 1, cols: c1 - c0 + 1 }))
        }),
        "Table > Unmerge Cells" => {
            if let Some(s) = app.sel {
                app.edit("Unmerge Cells", |d, h| {
                    d.unmerge_cells(h, t, s.cursor.0, s.cursor.1);
                });
            }
            None
        }
        _ => None,
    };
    if let Some(Err(e)) = res {
        app.notify(format!("{e}"));
    }
}

thread_local! {
    /// The context menu's spelling lookup: (version, storage, anchor, caret) -> (word range, word,
    /// misspelled, suggestions).
    static SPELL_MENU: std::cell::RefCell<Option<((u64, ObjId, usize, usize), (usize, usize, String, bool, Vec<String>))>> = const { std::cell::RefCell::new(None) };
}

/// The right-click menu for what is under the pointer (body text, an object, an empty page).
pub fn context_menu(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let fire = |ui: &mut egui::Ui, app: &App, label: &str, cmd: &str| -> Option<String> {
        if ui.add_enabled(enabled(app, cmd), egui::Button::new(label)).clicked() {
            ui.close();
            return Some(cmd.to_string());
        }
        None
    };
    let mut cmd: Option<String> = None;
    if matches!(app.cursor, Some(Cursor { owner: Owner::HeaderFooter { .. }, .. })) {
        // a header / footer field: the clipboard and page numbers
        for (l, c) in [("Cut", "Cut"), ("Copy", "Copy"), ("Paste", "Paste")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        ui.menu_button("Insert Page Number", |ui| crate::hfnumber::menu_items(app, ui));
        if ui.button("Insert Page Count").clicked() {
            ui.close();
            cmd = Some("Page Count".into());
        }
    } else if app.sel.is_some() {
        // table cells (Pages: the cell menu)
        for (l, c) in [("Add Row Above", "Table > Add Row Above"), ("Add Row Below", "Table > Add Row Below"), ("Add Column Before", "Table > Add Column Before"), ("Add Column After", "Table > Add Column After")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        for (l, c) in [("Delete Row", "Table > Delete Row"), ("Delete Column", "Table > Delete Column")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        for (l, c) in [("Sort Ascending", "Table > Sort Ascending"), ("Sort Descending", "Table > Sort Descending")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        for (l, c) in [("Merge Cells", "Table > Merge Cells"), ("Unmerge Cells", "Table > Unmerge Cells")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        for (l, c) in [("Cut", "Cut"), ("Copy", "Copy"), ("Paste", "Paste"), ("Delete", "Delete")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
    } else if !app.selected_objs().is_empty() && app.canvas.editing.is_none() {
        for (l, c) in [("Bring to Front", "Bring to Front"), ("Send to Back", "Send to Back")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.menu_button("Align Objects", |ui| {
            for h in ["Left", "Center", "Right", "Top", "Middle", "Bottom"] {
                cmd = cmd.clone().or(fire(ui, app, h, &format!("Align Objects > {h}")));
            }
        });
        ui.separator();
        for (l, c) in [("Cut", "Cut"), ("Copy", "Copy"), ("Paste", "Paste"), ("Duplicate", "Duplicate Selection"), ("Delete", "Delete")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        let locked = app.canvas.obj.is_some_and(|o| app.drawables.iter().any(|d| d.id == o && d.locked));
        cmd = cmd.or(fire(ui, app, if locked { "Unlock" } else { "Lock" }, if locked { "Unlock" } else { "Lock" }));
        ui.menu_button("Add Link", |ui| {
            for k in ["Webpage", "Email", "Phone Number"] {
                cmd = cmd.clone().or(fire(ui, app, k, &format!("Add Link > {k}")));
            }
        });
        if selected_image(app).is_some() {
            cmd = cmd.or(fire(ui, app, "Edit Mask", "Image > Edit Mask"));
            cmd = cmd.or(fire(ui, app, "Reset Mask", "Image > Reset Mask"));
        }
    } else if app.cursor.is_some() {
        // a misspelled word: suggestions first
        if let Some(c) = app.cursor {
            // worked out once while the menu stays open (the whole text and the suggestions
            // each frame made hovering the menu lag)
            let key = (app.version, c.storage, c.anchor, c.caret);
            let (a, b, word, bad, sugg) = SPELL_MENU.with(|m| {
                let mut m = m.borrow_mut();
                if m.as_ref().is_none_or(|(k, _)| *k != key) {
                    let text = app.doc().map(|d| d.storage_text(c.storage)).unwrap_or_default();
                    let (a, b) = if c.is_empty() { crate::canvas::word_at(&text, c.caret) } else { c.range() };
                    let word = text.get(a..b).unwrap_or("").to_string();
                    let bad = !word.trim().is_empty() && word.chars().all(|ch| ch.is_alphabetic() || ch == '\'') && !kn_ui::spell::misspelled(&word).is_empty();
                    let sugg = if bad { kn_ui::spell::suggestions(&word) } else { Vec::new() };
                    *m = Some((key, (a, b, word, bad, sugg)));
                }
                m.as_ref().map(|(_, v)| v.clone()).unwrap_or_default()
            });
            if bad {
                for s in sugg.iter().take(5) {
                    if ui.button(kn_ui::theme::bold(s)).clicked() {
                        app.cursor = Some(Cursor { anchor: a, caret: b, ..c });
                        crate::text::replace_selection(app, s, "Spelling", false);
                        ui.close();
                    }
                }
                if sugg.is_empty() {
                    ui.label(egui::RichText::new("No Guesses Found").weak());
                }
                if ui.button("Learn Spelling").clicked() {
                    kn_ui::spell::learn(&word);
                    ui.close();
                }
                if ui.button("Ignore Spelling").clicked() {
                    kn_ui::spell::ignore(&word);
                    ui.close();
                }
                ui.separator();
            }
        }
        for (l, c) in [("Cut", "Cut"), ("Copy", "Copy"), ("Paste", "Paste"), ("Paste and Match Style", "Paste and Match Style")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        ui.menu_button("Add Link", |ui| {
            for k in ["Webpage", "Email", "Phone Number", "Bookmark"] {
                cmd = cmd.clone().or(fire(ui, app, k, &format!("Add Link > {k}")));
            }
        });
        cmd = cmd.or(fire(ui, app, "Remove Link", "Remove Link"));
        ui.separator();
        for (l, c) in [("Comment", "Comment"), ("Highlight", "Highlight"), ("Bookmark", "Bookmark")] {
            cmd = cmd.or(fire(ui, app, l, c));
        }
        ui.separator();
        ui.menu_button("Transformations", |ui| {
            for k in ["Make Upper Case", "Make Lower Case", "Capitalize"] {
                cmd = cmd.clone().or(fire(ui, app, k, &format!("Transformations > {k}")));
            }
        });
    } else {
        cmd = cmd.or(fire(ui, app, "Paste", "Paste"));
        if app.wb.as_ref().is_some_and(|w| !w.is_word_processing()) {
            if ui.button("Add Page").clicked() {
                let at = app.current_page();
                app.edit_pages("Add Page", |p, h| p.add_layout_page(h, at));
                ui.close();
            }
            if ui.button("Delete Page").clicked() {
                let at = app.current_page();
                app.edit_pages("Delete Page", |p, h| p.delete_layout_page(h, at));
                ui.close();
            }
        }
        cmd = cmd.or(fire(ui, app, "Show Invisibles", "Show Invisibles"));
    }
    crate::guides::menu_items(app, ui);
    if let Some(c) = cmd {
        run(app, &ctx, &c);
    }
}

#[cfg(test)]
mod audit {
    /// The menus' commands that do nothing yet (cargo test -p pg-app audit -- --nocapture).
    #[test]
    fn unimplemented_commands() {
        fn walk(items: &[super::Item], parent: &str, out: &mut Vec<String>) {
            for it in items {
                if it.children.is_empty() {
                    if !it.separator && !it.label.is_empty() {
                        out.push(super::command_name(parent, &it.label));
                    }
                } else {
                    walk(&it.children, &it.label, out);
                }
            }
        }
        let mut all = Vec::new();
        for m in super::menus() {
            walk(&m.children, &m.label, &mut all);
        }
        let missing: Vec<&String> = all.iter().filter(|c| !super::implemented(c)).collect();
        println!("{} of {} commands not implemented:", missing.len(), all.len());
        for c in missing {
            println!("  {c}");
        }
    }
}
