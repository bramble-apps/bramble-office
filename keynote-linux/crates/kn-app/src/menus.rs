//! Menu bar generated from Keynote's own menu tree (data/menus.txt, captured from Keynote 15.4).
//! Items we implement run a command; the rest are shown disabled so the gaps stay visible.

use std::sync::OnceLock;

use kn_format::ObjId;
use kn_model::edit::ZMove;
use kn_model::insert::{BasicShape, NewObject};

use crate::app::{App, InspectorKind, ViewMode};

#[derive(Debug)]
pub struct Item {
    pub label: String,
    pub shortcut: Option<String>,
    pub children: Vec<Item>,
    pub separator: bool,
}

fn parse() -> Vec<Item> {
    let mut menus: Vec<Item> = Vec::new();
    // stack of (indent, path into tree)
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
        let depth = indent / 2; // 1 = top-level item
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
            Some((l, s)) => (l.to_string(), Some(linux_shortcut(s.trim_end_matches(']')))),
            None => (text.to_string(), None),
        };
        // duplicate consecutive entries (e.g. "New…" twice in the capture)
        if parent.children.last().is_some_and(|c| c.label == label) {
            continue;
        }
        parent.children.push(Item { label, shortcut, children: Vec::new(), separator: false });
    }
    // macOS-only items have no meaning here
    const MAC_ONLY: &[&str] = &[
        "Learn About Apple Creator Studio",
        "Get Apple Creator Studio",
        "Provide Keydeck Feedback",
        "Privacy & Analytics",
        "Services",
        "Hide Keydeck",
        "Hide Others",
        "Show All",
        "Quit and Keep Windows",
        "Start Dictation…",
        "Emoji & Symbols",
        "AutoFill",
        "Import from iPhone or iPad",
        "Writing Tools",
        "Start Dictation",
        // AI features: not part of this app
        "Generate Image",
        "Generate Shape",
        "Image Playground…",
        "Clean Up Slide",
        "Generate Slides from Text…",
        "Generate Presenter Notes",
        "Generate from Image",
        "Super Resolution",
        // macOS window management (Wayland leaves window placement to the desktop)
        "Minimize All",
        "Zoom All",
        "Center",
        "Move & Resize",
        "Full Screen Tile",
        "Bring All to Front",
        "Arrange in Front",
        "Remove Window from Set",
        "Merge All Windows",
        "Untitled 2",
        "Untitled.key",
        // Apple apps and formats
        "Edit in Pixelmator Pro",
        "Optimize for iPhone and iPad",
        "Keynote ’09…",
        "Formulas and Functions Help",
        // Apple services / iCloud / premium content
        "Remove Watermarks",
        "Share…",
        "Activity Settings…",
        "Hide Participant Cursors",
        "Show Activity Pane",
        "Show Sync Status",
        "Send Keydeck Feedback",
        "What’s New in Keydeck",
        "Welcome to Keydeck",
        "About Premium Features",
        "Service and Support",
    ];
    fn prune(items: &mut Vec<Item>) {
        items.retain(|i| !MAC_ONLY.contains(&i.label.as_str()));
        // submenus whose items were all removed go too
        for i in items.iter_mut() {
            if !i.children.is_empty() {
                prune(&mut i.children);
                if i.children.iter().all(|c| c.separator) {
                    i.label.clear();
                    i.separator = true;
                }
            }
        }
        // collapse doubled separators
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
        for i in &mut out {
            prune(&mut i.children);
        }
        *items = out;
    }
    prune(&mut menus);
    menus
}

fn linux_shortcut(mac: &str) -> String {
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
    parts.join("+")
}

/// `kn-app --list-unimplemented`: menu commands that are still greyed out (no command behind them).
pub fn list_unimplemented() {
    fn walk(items: &[Item], parent: &str, path: &str) {
        for it in items {
            if it.separator {
                continue;
            }
            let p = format!("{path} > {}", it.label);
            if it.children.is_empty() && it.label != "Open Recent" {
                let name = command_name(parent, &it.label);
                if !IMPLEMENTED.contains(&name.as_str()) && !name.starts_with("Mask with Shape > ") && chart_type_for(&name).is_none() {
                    println!("{p}");
                }
            } else {
                walk(&it.children, &it.label, &p);
            }
        }
    }
    for m in menus() {
        walk(&m.children, &m.label, &m.label);
    }
}

pub fn menus() -> &'static [Item] {
    static M: OnceLock<Vec<Item>> = OnceLock::new();
    M.get_or_init(parse)
}

/// Commands that work right now (menu label -> enabled check is done in `enabled`).
const IMPLEMENTED: &[&str] = &[
    "About Keydeck",
    "Support Development…",
    "Update Video Support…",
    "New…",
    "Settings…",
    "Keydeck Help",
    "Keyboard Shortcuts",
    "Quit Keydeck",
    "Open…",
    "Close Window",
    "Close Tab",
    "Close All",
    "Close Other Tabs",
    "Show Next Tab",
    "Show Previous Tab",
    "Move Tab to New Window",
    "Minimize",
    "Zoom",
    "Fill",
    "Save…",
    "Save As…",
    "Duplicate",
    "Rename…",
    "Move To…",
    "Undo",
    "Redo",
    "Delete",
    "Select All",
    "Deselect All",
    "New Slide",
    "Skip Slide",
    "Don't Skip Slide",
    "Duplicate Slide",
    "Delete Slide",
    "Next Slide",
    "Previous Slide",
    "First Slide",
    "Last Slide",
    "Bring Forward",
    "Bring to Front",
    "Send Backward",
    "Send to Back",
    "Flip Horizontally",
    "Flip Vertically",
    "Lock",
    "Unlock",
    "Navigator",
    "Slide Only",
    "Light Table",
    "Outline",
    "Format",
    "Animate",
    "Document",
    "Presenter Notes",
    "Show Presenter Notes",
    "Hide Presenter Notes",
    "Play Slideshow",
    "In Fullscreen",
    "In Window",
    "Rehearse Slideshow",
    "PDF…",
    "Find…",
    "Rulers",
    "Add Link",
    "Add Link > Webpage",
    "Add Link > Email",
    "Add Link > Phone Number",
    "Remove Link",
    "Baseline > Superscript",
    "Baseline > Subscript",
    "Baseline > Use Default",
    "Capitalization > None",
    "Capitalization > All Caps",
    "Capitalization > Small Caps",
    "Capitalization > Title Case",
    "Capitalization > Start Case",
    "Paste and Preserve Style",
    "Show Spelling and Grammar",
    "Show Warnings",
    "Show Adjust Image",
    "Show Arrange Tools",
    "Customize Toolbar…",
    "Make Editable",
    "Break Apart",
    "Save to My Shapes",
    "Create Style",
    "Define as Text Placeholder",
    "Define as Media Placeholder",
    "Equation...",
    "Equation…",
    "Highlight",
    "Set as Default Text Box Appearance",
    "Reset Text and Object Handles",
    "Make Motion Path from Shape",
    "Make Sharp Point",
    "Make Smooth Point",
    "Make Bézier Point",
    "Toggle Line Type",
    "Unite Shapes",
    "Intersect Shapes",
    "Subtract Shapes",
    "Exclude Shapes",
    "Select > Row",
    "Select > Column",
    "Select > First Value in Row",
    "Select > Last Value in Row",
    "Select > First Value in Column",
    "Select > Last Value in Column",
    "Select > Extend to First Value in Row",
    "Select > Extend to Last Value in Row",
    "Select > Extend to First Value in Column",
    "Select > Extend to Last Value in Column",
    "Autofill",
    "Autofill Right",
    "Autofill Left",
    "Autofill Down",
    "Autofill Up",
    "Autofill from Row Above",
    "Autofill from Column Before",
    "Clear All",
    "Line > Draw With Pen",
    "Draw With Pen",
    "PowerPoint…",
    "HTML…",
    "Browse All Versions…",
    "Change Theme...",
    "Change Theme…",
    "Change Theme",
    "Set Password…",
    "Change Password…",
    "Save Theme…",
    "Language & Region…",
    "Single File",
    "Package",
    "Show Photo and Video Browser",
    "Show Music Browser",
    "Record Audio",
    "Distribute Rows Evenly",
    "Merge Cells",
    "Unmerge Cells",
    "Paste Formula Results",
    "Web Video…",
    "Live Video",
    "Show Live Video Preview",
    "Record Slideshow...",
    "Play Recorded Slideshow",
    "Clear Recording…",
    "Auto-Align Table Cell",
    "Conditional Highlighting...",
    "Image Gallery",
    "Formula > Sum",
    "Formula > Average",
    "Formula > Minimum",
    "Formula > Maximum",
    "Formula > Count",
    "Formula > Product",
    "Formula > New Formula",
    "Resize Row to Fit Content",
    "Resize Column to Fit Content",
    "Show Borders",
    "Show Y Axis Title",
    "Show Y Axis Labels",
    "Show Y Axis 2 Title",
    "Show Y Axis 2 Labels",
    "Show X Axis Title",
    "Show X Axis Labels",
    "Show Series Value Labels",
    "Show Series Names",
    "Distribute Columns Evenly",
    "Show Presenter Display in Window",
    "Customize Presenter Display…",
    "Auto-Center",
    "Show Colors",
    "Show Fonts",
    "Remove Background",
    "Reset Background",
    "Reset Mask",
    "Edit Mask",
    "Auto Crop",
    "Reduce File Size…",
    "Hide Guides",
    "Show Slide Layout Guides",
    "Clear All Guides on Slide",
    "Copy Guides",
    "Paste Guides",
    "Hide Tab Bar",
    "Author Color > Yellow",
    "Author Color > Green",
    "Author Color > Blue",
    "Author Color > Pink",
    "Author Color > Purple",
    "Author Color > Teal",
    "Author Color > Orange",
    "Author Color > Red",
    "Author Color > Brown",
    "Author Color > Gray Blue",
    "Show All Tabs",
    "Check Document Now",
    "Check Grammar With Spelling",
    "Correct Spelling Automatically",
    "Show Substitutions",
    "Smart Web and Email Links",
    "Smart Phone Number Links",
    "Text Replacement",
    "Start Speaking",
    "Stop Speaking",
    "Jump to Selection",
    "Select Parent",
    "Baseline > Raise",
    "Baseline > Lower",
    "Ligature > Use Default",
    "Ligature > Use None",
    "Ligature > Use All",
    "Add Link > Slide",
    "Increase Indent Level",
    "Decrease Indent Level",
    "Transformations > Make Upper Case",
    "Transformations > Make Lower Case",
    "Transformations > Capitalize",
    "Go To > Slide…",
    "Find Next",
    "Find Previous",
    "Show Slide Numbers on All Slides",
    "Minimize",
    "Zoom",

    "Edit Slide Layouts",
    "Show Chart Data Editor",
    "Group",
    "Ungroup",
    "Hide Comments",
    "Paste and Match Style",
    "Show Object List",
    "Hide Object List",
    "Show Build Order",
    "Smart Quotes",
    "Smart Dashes",
    "Check Spelling While Typing",
    "Hide Find Panel",
    "Use Selection for Find",
    "Use Selection for Replace",
    "Expand",
    "Collapse",
    "Expand All",
    "Collapse All",
    "Reapply Layout to Slide",
    "Replace Fonts…",
    "Character Spacing > Use None",
    "Character Spacing > Tighten",
    "Character Spacing > Loosen",
    "Hide Inspector",
    "Show Inspector",
    "Show Next Inspector Tab",
    "Show Previous Inspector Tab",
    "Hide Toolbar",
    "Show Toolbar",
    "Zoom to Fit Content",
    "Zoom to Selection",
    "Minimize All",
    "Zoom All",
    "Line Break",
    "Column Break",
    "Copy Animation",
    "Paste Animation",
    "Comment",
    "Show Next Comment",
    "Show Previous Comment",
    "Show Comments",
    "Remove Highlights and Comments",
    "Show Rulers",
    "Hide Rulers",
    "Find & Replace…",
    "Images…",
    "Movie…",
    "Animated GIF…",
    "Print…",
    "Fit Slide",
    "Actual Size",
    "Zoom In",
    "Zoom Out",
    "Full Screen",
    "Enter Full Screen",
    "Cut",
    "Copy",
    "Paste",
    "Duplicate Selection",
    "Copy Style",
    "Paste Style",
    "Text",
    "Choose File",
    "Shape > Rectangle",
    "Shape > Rounded Rectangle",
    "Shape > Oval",
    "Shape > Triangle",
    "Shape > Right Triangle",
    "Shape > Arrow",
    "Shape > Double Arrow",
    "Shape > Diamond",
    "Shape > Quote Bubble",
    "Shape > Callout",
    "Shape > Polygon",
    "Shape > Star",
    "Line > Line",
    "Chart > 2D Column",
    "Chart > 2D Pie",
    "Chart > Show Chart Data Editor",
    "Chart > Show Legend",
    "Chart > Show Title",
    "Table > Basic",
    "Table > Add Row Above",
    "Sort Ascending",
    "Sort Descending",
    "Table > Add Row Below",
    "Table > Add Column Before",
    "Table > Add Column After",
    "Table > Delete Row",
    "Table > Delete Column",
    "Line > Line with Arrowhead",
    "Line > Line with Two Arrowheads",
    "Table > Headers",
    "Table > Plain",
    "Table > Sums",
    "Header Rows > 0",
    "Header Rows > 1",
    "Header Rows > 2",
    "Header Rows > 3",
    "Header Rows > 4",
    "Header Rows > 5",
    "Header Columns > 0",
    "Header Columns > 1",
    "Header Columns > 2",
    "Header Columns > 3",
    "Header Columns > 4",
    "Header Columns > 5",
    "Footer Rows > 0",
    "Footer Rows > 1",
    "Footer Rows > 2",
    "Footer Rows > 3",
    "Footer Rows > 4",
    "Footer Rows > 5",
    "Line > Straight Connection Line",
    "Line > Curved Connection Line",
    "Line > Angled Connection Line",
    "Bold",
    "Italic",
    "Underline",
    "Strikethrough",
    "Bigger",
    "Smaller",
    "Align Left",
    "Align Center",
    "Align Right",
    "Justify",
    "Align Objects > Left",
    "Align Objects > Center",
    "Align Objects > Right",
    "Align Objects > Top",
    "Align Objects > Middle",
    "Align Objects > Bottom",
    "Distribute Objects > Horizontally",
    "Distribute Objects > Vertically",
    "Distribute Objects > Evenly",
];

/// Submenus whose item labels are ambiguous on their own (e.g. "Left", "Rectangle"):
/// their items run as "Submenu > Item".
const QUALIFIED: &[&str] = &["Shape", "Line", "Chart", "Table", "Align Objects", "Distribute Objects", "Mask with Shape", "Add Link", "Baseline", "Capitalization", "Character Spacing", "Go To", "Transformations", "Ligature", "Header Rows", "Header Columns", "Footer Rows", "Author Color", "Select", "Formula"];

fn command_name(parent: &str, label: &str) -> String {
    if QUALIFIED.contains(&parent) {
        let q = format!("{parent} > {label}");
        // a submenu item may also be a plain command (Go To > Next Slide = Next Slide)
        if IMPLEMENTED.contains(&q.as_str()) || !IMPLEMENTED.contains(&label) {
            return q;
        }
    }
    label.to_string()
}

pub(crate) fn enabled(app: &App, label: &str) -> bool {
    if let Some(shape) = label.strip_prefix("Mask with Shape > ") {
        return BasicShape::ALL.iter().any(|b| b.name() == shape)
            && app.sel.len() == 1
            && app.doc.as_ref().is_some_and(|d| d.file.type_name(app.sel[0]) == "TSD.ImageArchive");
    }
    if chart_type_for(label).is_some() {
        return app.doc.is_some() && (label.starts_with("Chart > ") || selected_chart(app).is_some());
    }
    if !IMPLEMENTED.contains(&label) {
        return false;
    }
    let has_doc = app.doc.is_some();
    // commands that only apply to some kinds of object are greyed out without one (Keynote)
    if let Some(kind) = needs(label) {
        return has_doc && selection_has(app, kind);
    }
    match label {
        "About Keydeck" | "Support Development…" | "Update Video Support…" | "New…" | "Settings…" | "Keydeck Help" | "Keyboard Shortcuts" | "Quit Keydeck" | "Open…" | "Full Screen" | "Enter Full Screen" => true,
        "Show Next Tab" | "Show Previous Tab" | "Move Tab to New Window" | "Close Other Tabs" => app.tabs.len() > 1,
        "Minimize" | "Zoom" | "Fill" | "Close Window" | "Close All" => true,
        "Undo" => app.hist.can_undo(),
        "Find Next" | "Find Previous" => has_doc && !app.find.query.is_empty(),
        // a picture whose background was removed (its original kept)
        "Reset Background" => app.doc.as_ref().is_some_and(|d| app.sel.iter().any(|id| d.file.obj(*id).is_some_and(|m| m.msg("originalData").is_some()))),
        // something linked: the text being edited, or a selected object with a link
        "Remove Link" => {
            has_doc
                && (app.canvas.editing.is_some()
                    || app.doc.as_ref().and_then(|d| d.scene(&app.slides, app.cur)).is_some_and(|sc| sc.drawables.iter().any(|d| app.sel.contains(&d.id) && d.hyperlink.is_some())))
        }
        "Redo" => app.hist.can_redo(),
        "Copy" | "Cut" if app.nav_focus && app.sel.is_empty() => has_doc && !app.slide_sel.is_empty(),
        "Delete" | "Deselect All" | "Bring Forward" | "Bring to Front" | "Send Backward" | "Send to Back" | "Flip Horizontally" | "Flip Vertically" | "Lock" | "Unlock"
        | "Cut" | "Copy" | "Duplicate Selection" | "Copy Style" | "Bold" | "Italic" | "Underline" | "Strikethrough" | "Bigger" | "Smaller"
        | "Align Left" | "Align Center" | "Align Right" | "Justify" => has_doc && !app.sel.is_empty(),
        "Paste" | "Paste and Match Style" | "Paste and Preserve Style" if crate::tableops::cells_active(app) && app.cell_clip.is_some() => true,
        "Paste Formula Results" => crate::tableops::cells_active(app) && app.cell_clip.is_some(),
        "Auto-Align Table Cell" => crate::tableops::cells_active(app),
        "Play Recorded Slideshow" | "Clear Recording…" => app.doc.as_ref().is_some_and(|d| d.recording().is_some()),
        "Conditional Highlighting..." => crate::tableops::range(app).is_some_and(|(id, ..)| app.doc.as_ref().is_some_and(|d| d.can_highlight(id))),
        "Paste" | "Paste and Match Style" | "Paste and Preserve Style" => has_doc && (app.clip.is_some() || app.canvas.editing.is_some() || (app.nav_focus && app.slide_clip.is_some())),
        "Paste Style" => has_doc && app.copied_style.is_some() && !app.sel.is_empty(),
        "Copy Animation" => has_doc && app.sel.len() == 1,
        "Paste Animation" => has_doc && app.copied_animation.is_some() && !app.sel.is_empty(),
        l if l.starts_with("Align Objects") || l.starts_with("Distribute Objects") => has_doc && app.sel.len() >= 2,
        "Table > Add Row Above" | "Table > Add Row Below" | "Table > Add Column Before" | "Table > Add Column After" | "Table > Delete Row"
        | "Table > Delete Column" => table_rows_enabled(app, label),
        l if l.starts_with("Formula > ") => {
            crate::canvas::table_cursor(app).is_some_and(|(id, ..)| app.doc.as_ref().is_some_and(|d| d.can_insert_formula(id)))
        }
        "Merge Cells" => crate::tableops::range(app).is_some_and(|(id, r0, c0, r1, c1)| (r0, c0) != (r1, c1) && app.doc.as_ref().is_some_and(|d| d.can_merge_cells(id))),
        "Unmerge Cells" => crate::tableops::range(app).is_some_and(|(id, r0, c0, ..)| merged_at(app, id, r0, c0)),
        _ => has_doc,
    }
}

/// What a command needs selected to do anything.
#[derive(Clone, Copy, PartialEq)]
enum Needs {
    Text,
    /// Text in a text box (or being typed): not table cells.
    Prose,
    Table,
    Chart,
    Image,
    ImageMask,
    Shape,
    Anything,
}

fn needs(label: &str) -> Option<Needs> {
    const TEXT_PREFIXES: [&str; 4] = ["Baseline > ", "Capitalization > ", "Character Spacing > ", "Ligature > "];
    if label.starts_with("Transformations > ") {
        return Some(Needs::Prose);
    }
    // links go on text or on any object (Keynote)
    if label.starts_with("Add Link > ") {
        return Some(Needs::Anything);
    }
    const TABLE_PREFIXES: [&str; 5] = ["Header Rows > ", "Header Columns > ", "Footer Rows > ", "Select > ", "Autofill"];
    if TEXT_PREFIXES.iter().any(|p| label.starts_with(p)) {
        return Some(Needs::Text);
    }
    if TABLE_PREFIXES.iter().any(|p| label.starts_with(p)) {
        return Some(Needs::Table);
    }
    Some(match label {
        "Bold" | "Italic" | "Underline" | "Strikethrough" | "Bigger" | "Smaller" | "Align Left" | "Align Center" | "Align Right" | "Justify" => Needs::Text,
        "Increase Indent Level" | "Decrease Indent Level" => Needs::Prose,
        "Distribute Rows Evenly" | "Distribute Columns Evenly" | "Sort Ascending" | "Sort Descending" | "Resize Row to Fit Content" | "Resize Column to Fit Content" => Needs::Table,
        "Show Legend" | "Chart > Show Legend" | "Chart > Show Title" | "Show Chart Data Editor" | "Chart > Show Chart Data Editor" | "Show Series Names" | "Show Series Value Labels"
        | "Show X Axis Labels" | "Show X Axis Title" | "Show Y Axis Labels" | "Show Y Axis Title" | "Show Y Axis 2 Labels" | "Show Y Axis 2 Title" | "Show Borders" => Needs::Chart,
        "Edit Mask" | "Remove Background" | "Auto Crop" => Needs::Image,
        "Reset Mask" => Needs::ImageMask,
        "Make Editable" | "Break Apart" | "Save to My Shapes" => Needs::Shape,
        "Zoom to Selection" | "Jump to Selection" | "Reset Text and Object Handles" => Needs::Anything,
        _ => return None,
    })
}

/// Whether the selection (or the text / cells being edited) has something `kind` acts on.
fn selection_has(app: &App, kind: Needs) -> bool {
    use kn_model::DrawableKind;
    if kind == Needs::Text && (app.canvas.editing.is_some() || crate::tableops::cells_active(app)) {
        return true;
    }
    if kind == Needs::Prose && app.canvas.editing.is_some() {
        return true;
    }
    if kind == Needs::Table {
        return selected_table(app).is_some();
    }
    if kind == Needs::Anything {
        return !app.sel.is_empty();
    }
    let Some(sc) = app.doc.as_ref().and_then(|d| d.scene(&app.slides, app.cur)) else { return false };
    sc.drawables.iter().filter(|d| app.sel.contains(&d.id)).any(|d| match (kind, &d.kind) {
        (Needs::Text | Needs::Prose, DrawableKind::Shape(s)) => s.text.as_ref().is_some_and(|t| t.paragraphs.iter().any(|p| !p.runs.is_empty())),
        (Needs::Text, DrawableKind::Table(_)) => true,
        (Needs::Chart, DrawableKind::Chart(_)) => true,
        (Needs::Image, DrawableKind::Image(_)) => true,
        (Needs::ImageMask, DrawableKind::Image(i)) => i.mask.is_some(),
        (Needs::Shape, DrawableKind::Shape(s)) => !s.is_line,
        _ => false,
    })
}

/// Whether cell (row, col) of table `id` on the current slide is part of a merged cell.
fn merged_at(app: &App, id: ObjId, row: u32, col: u32) -> bool {
    let Some(sc) = app.doc.as_ref().and_then(|d| d.scene(&app.slides, app.cur)) else { return false };
    sc.drawables.iter().any(|d| d.id == id && matches!(&d.kind, kn_model::DrawableKind::Table(t) if t.merge_at(row, col).is_some()))
}

/// Row / column commands need a table cell; deleting needs more than one row / column.
fn table_rows_enabled(app: &App, label: &str) -> bool {
    let (Some(doc), Some((id, _, _))) = (app.doc.as_ref(), crate::canvas::table_cursor(app)) else { return false };
    if doc.table_edit_blocker(id).is_some() {
        return false;
    }
    let Some(m) = doc.file.obj(id).and_then(|t| t.ref_id("tableModel")).and_then(|m| doc.file.obj(m)) else { return false };
    match label {
        "Table > Delete Row" => m.u32("number_of_rows").unwrap_or(0) > 1,
        "Table > Delete Column" => m.u32("number_of_columns").unwrap_or(0) > 1,
        _ => true,
    }
}

/// Width of the icon column in menus (Keynote: 16 pt glyph plus a little air).
const MENU_ICON_W: f32 = 20.0;

fn paint_icon(ui: &egui::Ui, r: egui::Rect, icon: crate::icons::Icon, on: bool) {
    let x = r.min.x + ui.spacing().button_padding.x + MENU_ICON_W / 2.0;
    crate::icons::paint_menu_icon(ui, egui::pos2(x, r.center().y), icon, on);
}

fn item_ui(app: &mut App, ui: &mut egui::Ui, parent: &str, item: &Item, indent: bool, fired: &mut Option<String>) {
    if item.separator {
        ui.separator();
        return;
    }
    if item.label == "Open Recent" {
        // lined up with the other items (the icon column)
        let text: egui::Atoms = {
            use egui::IntoAtoms;
            if indent { (egui::Atom::custom(egui::IdSalt::new("menu-icon"), egui::vec2(MENU_ICON_W, 16.0)), item.label.as_str()).into_atoms() } else { item.label.as_str().into_atoms() }
        };
        ui.menu_button(text, |ui| {
            let recent = App::recent_files();
            if recent.is_empty() {
                ui.add_enabled(false, egui::Button::new("No Recent Presentations"));
            }
            for p in recent {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                    *fired = Some(format!("open:{}", p.display()));
                    ui.close();
                }
            }
        });
        return;
    }
    // Keynote's menu icons sit in their own column; every item of a menu that has icons is
    // indented by that column so the labels line up
    let icon = crate::icons::menu_icon(&item.label);
    let lead = || egui::Atom::custom(egui::IdSalt::new("menu-icon"), egui::vec2(MENU_ICON_W, 16.0));
    let text: egui::Atoms = { use egui::IntoAtoms; if indent { (lead(), item.label.as_str()).into_atoms() } else { item.label.as_str().into_atoms() } };
    if !item.children.is_empty() {
        let kids_indent = item.children.iter().any(|c| crate::icons::menu_icon(&c.label).is_some());
        let r = ui.menu_button(text, |ui| {
            menu_body(ui, |ui| {
                for c in &item.children {
                    item_ui(app, ui, &item.label, c, kids_indent, fired);
                }
            })
        });
        if let Some(i) = icon {
            paint_icon(ui, r.response.rect, i, true);
        }
        return;
    }
    let name = command_name(parent, &item.label);
    let mut b = egui::Button::new(text);
    if let Some(s) = &item.shortcut {
        b = b.shortcut_text(s);
    }
    let shown = match item.label.as_str() {
        "Hide Inspector" if !app.show_inspector => Some("Show Inspector"),
        "Hide Toolbar" if !app.show_toolbar => Some("Show Toolbar"),
        "Show Object List" if app.show_object_list => Some("Hide Object List"),
        "Hide Guides" if !app.show_guides => Some("Show Guides"),
        "Hide Tab Bar" if !app.show_tab_bar => Some("Show Tab Bar"),
        "Show Slide Layout Guides" if app.show_layout_guides => Some("Hide Slide Layout Guides"),
        _ => None,
    };
    if let Some(l) = shown {
        let mut b2 = if indent { egui::Button::new((lead(), l)) } else { egui::Button::new(l) };
        if let Some(s) = &item.shortcut {
            b2 = b2.shortcut_text(s);
        }
        if ui.add(b2).clicked() {
            *fired = Some(l.to_string());
            ui.close();
        }
        return;
    }
    let checked = match item.label.as_str() {
        "Navigator" => Some(app.view == ViewMode::Navigator),
        "Slide Only" => Some(app.view == ViewMode::SlideOnly),
        "Light Table" => Some(app.view == ViewMode::LightTable),
        "Outline" => Some(app.view == ViewMode::Outline),
        "In Window" => Some(app.settings.play_in_window),
        "Smart Quotes" => Some(app.settings.smart_quotes),
        "Smart Dashes" => Some(app.settings.smart_dashes),
        "Check Spelling While Typing" => Some(app.settings.spell_check),
        "Check Grammar With Spelling" => Some(app.settings.grammar),
        "Single File" => app.doc.as_ref().map(|d| !d.package),
        "Package" => app.doc.as_ref().map(|d| d.package),
        "Show Borders" | "Show Y Axis Title" | "Show Y Axis Labels" | "Show Y Axis 2 Title" | "Show Y Axis 2 Labels" | "Show X Axis Title" | "Show X Axis Labels" | "Show Series Value Labels" | "Show Series Names" => {
            selected_chart(app).and_then(|id| chart_flag_state(app, id, item.label.as_str()))
        }
        "Show Presenter Display in Window" => Some(app.settings.presenter_in_window),
        "Auto-Center" => Some(app.auto_center),
        "Show Live Video Preview" => Some(app.live_preview),
        n if n.starts_with("Author Color > ") => Some(crate::settings::AUTHOR_COLORS.get(app.settings.author_color as usize).is_some_and(|(c, _)| Some(*c) == n.strip_prefix("Author Color > "))),
        n if n.starts_with("Header Rows > ") || n.starts_with("Header Columns > ") || n.starts_with("Footer Rows > ") => selected_table(app).map(|t| {
            use kn_model::tablerows::HeaderKind;
            let (k, c) = n.split_once(" > ").unwrap();
            let kind = match k {
                "Header Rows" => HeaderKind::Rows,
                "Header Columns" => HeaderKind::Columns,
                _ => HeaderKind::Footer,
            };
            app.doc.as_ref().map(|d| d.table_header_count(t, kind).to_string() == c).unwrap_or(false)
        }),
        "Correct Spelling Automatically" => Some(app.settings.auto_correct),
        "Smart Web and Email Links" => Some(app.settings.smart_links),
        "Smart Phone Number Links" => Some(app.settings.smart_phone_links),
        "Text Replacement" => Some(app.settings.text_replacement),
        "In Fullscreen" => Some(!app.settings.play_in_window),
        _ => None,
    };
    if checked == Some(true) {
        b = b.selected(true);
    }
    let on = enabled(app, &name);
    let resp = ui.add_enabled(on, b);
    if let Some(i) = icon {
        paint_icon(ui, resp.rect, i, on);
    }
    if resp.clicked() {
        *fired = Some(name);
        ui.close();
    }
}

/// A menu's items, scrolling when the menu is taller than the room down to the window's
/// bottom edge (a menu never spills past the window, onto the shadow margin around it).
fn menu_body(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let win: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(egui::Id::unique("kd-window-rect")));
    let top = ui.max_rect().min.y;
    let room = win.map(|w| (w.max.y - top - 10.0).min(w.height() - 20.0)).unwrap_or(f32::INFINITY).max(120.0);
    // the menu's full height last time it was shown: it scrolls only when that doesn't fit
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

/// The menu bar; returns the right edge of its last menu (the bar itself may fill the row).
pub fn menu_bar(app: &mut App, ui: &mut egui::Ui) -> f32 {
    let mut fired = None;
    let mut right = ui.min_rect().min.x;
    egui::MenuBar::new().ui(ui, |ui| {
        // the hover highlight as roomy at the sides as above and below the name (egui gives
        // menu titles 2 px at the sides); the gap between titles shrinks to keep the bar's width
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().button_padding.x = 7.0;
        ui.spacing_mut().item_spacing.x = (gap - 10.0).max(0.0);
        // (the phone layout: every menu under one button)
        let compact = kn_ui::form::compact(ui.ctx());
        let mut all = |ui: &mut egui::Ui| {
            for m in menus() {
                let indent = m.children.iter().any(|c| crate::icons::menu_icon(&c.label).is_some());
                // the app menu shows Keydeck's mark instead of its name
                let app_menu = m.label == "Keydeck" && !compact;
                let label = if app_menu { "\u{2003}\u{2002}".to_string() } else { m.label.clone() };
                let r = ui.menu_button(label, |ui| menu_body(ui, |ui| {
                    for c in &m.children {
                        item_ui(app, ui, &m.label, c, indent, &mut fired);
                    }
                    if m.label == "Window" {
                        // the open presentations, the active one ticked
                        ui.separator();
                        for i in 0..app.tabs.len() {
                            let name = app.tab_name(i);
                            if ui.add(egui::Button::new(name).selected(i == app.active_tab)).clicked() {
                                fired = Some(format!("tab:{i}"));
                                ui.close();
                            }
                        }
                    }
                }));
                if app_menu {
                    let b = r.response.rect;
                    crate::icons::draw(ui.painter(), egui::Rect::from_center_size(b.center() + egui::vec2(0.0, 1.0), egui::vec2(20.0, 20.0)), crate::icons::Icon::Projector, crate::theme::TEXT());
                }
                right = right.max(r.response.rect.max.x);
            }
        };
        if compact {
            let r = ui.menu_button("\u{2630}", |ui| all(ui));
            right = right.max(r.response.rect.max.x);
        } else {
            all(ui);
        }
    });
    if let Some(cmd) = fired {
        run(app, ui.ctx(), &cmd);
    }
    right
}

/// Items of the toolbar's View drop-down.
pub fn view_dropdown(app: &mut App, ui: &mut egui::Ui) {
    let mut fired = None;
    for (label, mode) in [("Navigator", ViewMode::Navigator), ("Slide Only", ViewMode::SlideOnly), ("Light Table", ViewMode::LightTable), ("Outline", ViewMode::Outline)] {
        if ui.add(egui::Button::new(label).selected(app.view == mode)).clicked() {
            fired = Some(label.to_string());
            ui.close();
        }
    }
    ui.separator();
    let notes = if app.show_notes { "Hide Presenter Notes" } else { "Show Presenter Notes" };
    if ui.button(notes).clicked() {
        fired = Some(notes.into());
        ui.close();
    }
    if let Some(c) = fired {
        run(app, ui.ctx(), &c);
    }
}

pub fn run(app: &mut App, ctx: &egui::Context, name: &str) {
    if app.editing_layouts.is_some()
        && matches!(name, "New Slide" | "Duplicate Slide" | "Delete Slide" | "Skip Slide" | "Don't Skip Slide" | "Play Slideshow")
    {
        app.notify("Not available while editing slide layouts — click Done first");
        return;
    }
    if let Some(p) = name.strip_prefix("open:") {
        app.open_path(std::path::PathBuf::from(p));
        return;
    }
    let slide = app.current_slide().map(|s| s.slide);
    match name {
        "About Keydeck" => app.show_about = true,
        "Support Development…" => crate::links::open_url(kn_ui::DONATE_URL),
        "Update Video Support…" => {
            if app.video_update.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                crate::media::update_yt_dlp(tx);
                app.video_update = Some(rx);
                app.notify("Updating video support…");
            }
        }
        "Settings…" => app.show_settings = true,
        "Keydeck Help" | "Keyboard Shortcuts" => app.show_help = true,
        "Quit Keydeck" => app.quit(),
        "Open…" => app.pick_open(),
        "New…" => app.show_theme_chooser = true,
        "Save…" => app.save(),
        "Save As…" => app.pick_save(),
        "Duplicate" => crate::filemenu::duplicate(app),
        "Rename…" => crate::filemenu::start_rename(app),
        "Move To…" => crate::filemenu::start_move(app),
        // closing asks to save unsaved presentations first (App::closing_ui)
        "Close Window" | "Close All" => app.request_close((0..app.tabs.len()).collect()),
        "Close Tab" => app.request_close(vec![app.active_tab]),
        "Close Other Tabs" => app.request_close((0..app.tabs.len()).filter(|&i| i != app.active_tab).collect()),
        c if c.starts_with("tab:") => {
            if let Ok(i) = c[4..].parse::<usize>() {
                app.select_tab(ctx, i);
            }
        }
        "Show Next Tab" => {
            let n = app.tabs.len();
            let i = (app.active_tab + 1) % n;
            app.select_tab(ctx, i);
        }
        "Show Previous Tab" => {
            let n = app.tabs.len();
            let i = (app.active_tab + n - 1) % n;
            app.select_tab(ctx, i);
        }
        "Move Tab to New Window" => crate::filemenu::tab_to_window(app, ctx),
        "Minimize" => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
        "Zoom" => {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        "Fill" | "Zoom All" => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true)),
        "Minimize All" => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
        "Undo" => app.undo(),
        "Redo" => app.redo(),
        "Select All" if app.nav_focus && app.canvas.editing.is_none() => {
            app.slide_sel = (0..app.slides.len()).collect();
        }
        "Select All" => {
            if let Some(sc) = app.scene(app.cur) {
                app.sel = sc.drawables.iter().filter(|d| !d.from_template).map(|d| d.id).collect();
            }
        }
        "Deselect All" => app.sel.clear(),
        "Delete" if app.nav_focus && app.canvas.editing.is_none() && app.sel.is_empty() => {
            // in the slide navigator Delete removes the selected slides (Keynote)
            run(app, ctx, "Delete Slide");
        }
        "Delete" => {
            // with cells selected, Delete empties them (Keynote), not the whole table
            if crate::tableops::cells_active(app) && crate::tableops::clear_cells(app) {
                return;
            }
            if app.sel.is_empty() {
                return;
            }
            let (Some(s), Some(doc)) = (slide, app.doc.as_mut()) else { return };
            let ids = std::mem::take(&mut app.sel);
            doc.begin(&mut app.hist, "Delete");
            doc.delete_drawables(&mut app.hist, s, &ids);
            doc.end(&mut app.hist);
            app.bump(s);
        }
        "Skip Slide" | "Don't Skip Slide" => {
            let picked: Vec<usize> = if app.slide_sel.is_empty() { vec![app.cur] } else { app.slide_sel.clone() };
            let targets: Vec<(kn_format::ObjId, bool)> = picked.iter().filter_map(|i| app.slides.get(*i)).map(|s| (s.node, !s.skipped)).collect();
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Skip Slide");
            for (node, skip) in targets {
                doc.set_skipped(&mut app.hist, node, skip);
            }
            doc.end(&mut app.hist);
            app.bump_all();
        }
        "New Slide" | "Duplicate Slide" => {
            if app.blocked_by_slide_groups() {
                return;
            }
            let i = app.cur;
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, name);
            let r = if name == "New Slide" { doc.new_slide_after(&mut app.hist, i) } else { doc.duplicate_slide(&mut app.hist, i) };
            doc.end(&mut app.hist);
            match r {
                Ok(n) => {
                    app.bump_all();
                    app.go_to(n);
                }
                Err(e) => app.notify(format!("Couldn't add slide: {e}")),
            }
        }
        "Delete Slide" => {
            if app.blocked_by_slide_groups() {
                return;
            }
            let mut idx = app.slide_sel.clone();
            idx.sort_unstable();
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Delete Slide");
            let r = doc.delete_slides(&mut app.hist, &idx);
            doc.end(&mut app.hist);
            match r {
                Ok(()) => {
                    app.bump_all();
                    let c = app.cur.min(app.slides.len().saturating_sub(1));
                    app.go_to(c);
                }
                Err(e) => app.notify(e.to_string()),
            }
        }
        "Next Slide" => app.go_to((app.cur + 1).min(app.slides.len().saturating_sub(1))),
        "Previous Slide" => app.go_to(app.cur.saturating_sub(1)),
        "First Slide" => app.go_to(0),
        "Last Slide" => app.go_to(app.slides.len().saturating_sub(1)),
        "Bring Forward" | "Bring to Front" | "Send Backward" | "Send to Back" => {
            let how = match name {
                "Bring Forward" => ZMove::Forward,
                "Bring to Front" => ZMove::Front,
                "Send Backward" => ZMove::Backward,
                _ => ZMove::Back,
            };
            let ids = app.sel.clone();
            let (Some(s), Some(doc)) = (slide, app.doc.as_mut()) else { return };
            doc.begin(&mut app.hist, name);
            doc.reorder(&mut app.hist, s, &ids, how);
            doc.end(&mut app.hist);
            app.bump(s);
        }
        "Flip Horizontally" | "Flip Vertically" | "Lock" | "Unlock" => {
            let Some(s) = slide else { return };
            let Some(sc) = app.scene(app.cur) else { return };
            let targets: Vec<_> = sc.drawables.iter().filter(|d| app.sel.contains(&d.id)).cloned().collect();
            let geoms: Vec<_> = targets.iter().map(|d| (d.id, app.renderer.effective_geometry(d))).collect();
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, name);
            for (id, mut g) in geoms {
                match name {
                    "Flip Horizontally" => {
                        g.hflip = !g.hflip;
                        doc.set_frame(&mut app.hist, id, g);
                    }
                    "Flip Vertically" => {
                        g.vflip = !g.vflip;
                        doc.set_frame(&mut app.hist, id, g);
                    }
                    "Lock" => doc.set_locked(&mut app.hist, id, true),
                    _ => doc.set_locked(&mut app.hist, id, false),
                }
            }
            doc.end(&mut app.hist);
            app.bump(s);
        }
        "Navigator" => app.view = ViewMode::Navigator,
        "Slide Only" => app.view = ViewMode::SlideOnly,
        "Light Table" => app.view = ViewMode::LightTable,
        "Outline" => app.view = ViewMode::Outline,
        "Format" | "Animate" | "Document" => {
            app.inspector = match name {
                "Format" => InspectorKind::Format,
                "Animate" => InspectorKind::Animate,
                _ => InspectorKind::Document,
            };
            app.show_inspector = true;
        }
        "Hide Inspector" | "Show Inspector" => app.show_inspector = !app.show_inspector,
        "Show Object List" | "Hide Object List" => app.show_object_list = !app.show_object_list,
        "Show Build Order" => app.insp.show_build_order = !app.insp.show_build_order,
        "Show Next Inspector Tab" | "Show Previous Inspector Tab" => {
            let order = [InspectorKind::Format, InspectorKind::Animate, InspectorKind::Document];
            let i = order.iter().position(|k| *k == app.inspector).unwrap_or(0);
            let step = if name == "Show Next Inspector Tab" { 1 } else { 2 };
            app.inspector = order[(i + step) % 3];
            app.show_inspector = true;
        }
        "Hide Toolbar" | "Show Toolbar" => {
            app.show_toolbar = !app.show_toolbar;
            app.settings.toolbar_shown = app.show_toolbar;
            app.settings.save();
        }
        "Zoom to Fit Content" | "Zoom to Selection" => {
            // the selected objects (or every object) filling the canvas, centred
            let Some(sc) = app.scene(app.cur) else { return };
            let only_sel = name == "Zoom to Selection";
            let boxes: Vec<(kn_model::Point, kn_model::Point)> =
                sc.drawables.iter().filter(|d| !only_sel || app.sel.contains(&d.id)).map(|d| app.renderer.effective_geometry(d).bounds()).collect();
            if boxes.is_empty() {
                app.zoom = crate::app::Zoom::Fit;
                return;
            }
            let b = boxes.iter().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |a, (lo, hi)| [a[0].min(lo.x), a[1].min(lo.y), a[2].max(hi.x), a[3].max(hi.y)]);
            app.canvas_focus = Some(b);
        }
        "Column Break" => {
            // Keynote stores a column break as U+000C (verified)
            if !crate::textedit::insert_at_caret(app, "\u{000C}") {
                app.notify("Click into text to insert a column break");
            }
        }
        "Line Break" => {
            if !crate::textedit::insert_at_caret(app, "\u{2028}") {
                app.notify("Click into text to insert a line break");
            }
        }
        "Presenter Notes" => app.show_notes = !app.show_notes,
        "Show Presenter Notes" => app.show_notes = true,
        "Hide Presenter Notes" => app.show_notes = false,
        "Rulers" | "Show Rulers" | "Hide Rulers" => app.show_rulers = !app.show_rulers,
        "Edit Slide Layouts" => app.toggle_layout_editing(),
        "Add Link" | "Add Link > Webpage" | "Add Link > Email" | "Add Link > Phone Number" | "Add Link > Slide" => {
            if app.sel.is_empty() {
                app.notify("Select an object to link");
                return;
            }
            let cur = app.scene(app.cur).and_then(|sc| sc.drawables.iter().find(|d| d.id == app.sel[0]).and_then(|d| d.hyperlink.clone()));
            let kind = match name {
                "Add Link > Email" => 1,
                "Add Link > Phone Number" => 2,
                "Add Link > Slide" => 3,
                _ => 0,
            };
            app.link_dialog.show_kind(cur.as_deref(), kind);
        }
        "Remove Link" => {
            let ids = app.sel.clone();
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Remove Link");
            for id in ids {
                doc.set_hyperlink(&mut app.hist, id, None);
            }
            doc.end(&mut app.hist);
            if let Some(s) = slide {
                app.bump(s);
            }
        }
        "Baseline > Superscript" => crate::editing::set_text(app, "Superscript", kn_model::format::TextProp::Script(1)),
        "Baseline > Subscript" => crate::editing::set_text(app, "Subscript", kn_model::format::TextProp::Script(-1)),
        "Baseline > Use Default" => crate::editing::set_text(app, "Baseline", kn_model::format::TextProp::Script(0)),
        "Capitalization > None" => crate::editing::set_text(app, "Capitalization", kn_model::format::TextProp::Caps(kn_model::Caps::None)),
        "Capitalization > All Caps" => crate::editing::set_text(app, "Capitalization", kn_model::format::TextProp::Caps(kn_model::Caps::All)),
        "Capitalization > Small Caps" => crate::editing::set_text(app, "Capitalization", kn_model::format::TextProp::Caps(kn_model::Caps::Small)),
        "Capitalization > Title Case" => crate::editing::set_text(app, "Capitalization", kn_model::format::TextProp::Caps(kn_model::Caps::Title)),
        "Capitalization > Start Case" => crate::editing::set_text(app, "Capitalization", kn_model::format::TextProp::Caps(kn_model::Caps::Start)),
        "Baseline > Raise" | "Baseline > Lower" => {
            // Keynote moves the baseline 1 pt at a time
            let cur = selected_run_baseline(app);
            let v = if name.ends_with("Raise") { cur + 1.0 } else { cur - 1.0 };
            crate::editing::set_text(app, "Baseline Shift", kn_model::format::TextProp::BaselineShift(v));
        }
        "Ligature > Use Default" => crate::editing::set_text(app, "Ligatures", kn_model::format::TextProp::Ligatures(1)),
        "Ligature > Use None" => crate::editing::set_text(app, "Ligatures", kn_model::format::TextProp::Ligatures(0)),
        "Ligature > Use All" => crate::editing::set_text(app, "Ligatures", kn_model::format::TextProp::Ligatures(2)),
        "Increase Indent Level" | "Decrease Indent Level" => {
            let delta = if name.starts_with("Increase") { 1 } else { -1 };
            text_ranges_edit(app, name, |d, h, st, a, b| d.change_list_level(h, st, a, b, delta), true);
        }
        "Transformations > Make Upper Case" | "Transformations > Make Lower Case" | "Transformations > Capitalize" => {
            let how = if name.ends_with("Upper Case") { "upper" } else if name.ends_with("Lower Case") { "lower" } else { "title" };
            text_ranges_edit(app, "Transform Text", |d, h, st, a, b| d.transform_case(h, st, a, b, how), false);
        }
        "Go To > Slide…" => app.show_goto = true,
        "Find Next" => crate::find::step(app, true),
        "Find Previous" => crate::find::step(app, false),
        "Show Slide Numbers on All Slides" => {
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, name);
            doc.show_slide_numbers_everywhere(&mut app.hist, true);
            doc.end(&mut app.hist);
            app.bump_all();
        }
        "Show Chart Data Editor" => {
            if let Some(id) = app.sel.first().copied() {
                crate::chartedit::open(app, id);
            }
        }
        "Group" => {
            let (Some(s), Some(doc)) = (slide, app.doc.as_mut()) else { return };
            let ids = app.sel.clone();
            doc.begin(&mut app.hist, "Group");
            let r = doc.group(&mut app.hist, s, &ids);
            doc.end(&mut app.hist);
            match r {
                Ok(g) => {
                    app.sel = vec![g];
                    app.bump(s);
                }
                Err(e) => app.notify(e.to_string()),
            }
        }
        "Ungroup" => {
            let (Some(s), Some(doc)) = (slide, app.doc.as_mut()) else { return };
            let groups: Vec<_> = app.sel.iter().copied().filter(|i| doc.file.type_name(*i) == "TSD.GroupArchive").collect();
            if groups.is_empty() {
                app.notify("Select a group to ungroup");
                return;
            }
            doc.begin(&mut app.hist, "Ungroup");
            let mut sel = Vec::new();
            for g in groups {
                if let Ok(kids) = doc.ungroup(&mut app.hist, s, g) {
                    sel.extend(kids);
                }
            }
            doc.end(&mut app.hist);
            app.sel = sel;
            app.bump(s);
        }
        "Hide Comments" | "Show Comments" => app.show_comments = !app.show_comments,
        "Comment" => add_comment(app),
        "Smart Quotes" | "Smart Dashes" | "Check Spelling While Typing" => {
            let s = &mut app.settings;
            match name {
                "Smart Quotes" => s.smart_quotes = !s.smart_quotes,
                "Smart Dashes" => s.smart_dashes = !s.smart_dashes,
                _ => s.spell_check = !s.spell_check,
            }
            s.save();
        }
        "Hide Find Panel" => app.find.open = false,
        "Use Selection for Find" | "Use Selection for Replace" => {
            let sel = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor)));
            let text = sel.and_then(|(st, a, b)| app.doc.as_ref().map(|d| d.storage_text(st)).and_then(|t| t.get(a..b).map(str::to_string)));
            match text.filter(|t| !t.is_empty()) {
                Some(t) if name == "Use Selection for Find" => {
                    app.find.query = t;
                    app.find.show();
                }
                Some(t) => {
                    app.find.replace = t;
                    app.find.show();
                }
                None => app.notify("Select some text first"),
            }
        }
        "Expand" | "Collapse" | "Expand All" | "Collapse All" => {
            let collapse = name.starts_with("Collapse");
            // groups: slides followed by more-indented slides
            let groups: Vec<usize> = (0..app.slides.len()).filter(|&i| app.slides.get(i + 1).is_some_and(|n| n.depth > app.slides[i].depth)).collect();
            let targets: Vec<usize> = if name.ends_with("All") {
                groups
            } else {
                // the current slide's group (itself, or the nearest less-indented slide above it)
                let cur = app.cur;
                let d = app.slides.get(cur).map(|s| s.depth).unwrap_or(0);
                let owner = if groups.contains(&cur) { Some(cur) } else { (0..cur).rev().find(|&j| app.slides[j].depth < d) };
                owner.into_iter().collect()
            };
            let nodes: Vec<kn_format::ObjId> = targets.iter().filter(|&&i| app.slides[i].collapsed != collapse).map(|&i| app.slides[i].node).collect();
            if nodes.is_empty() {
                return;
            }
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, if collapse { "Collapse" } else { "Expand" });
                for n in nodes {
                    doc.set_collapsed(&mut app.hist, n, collapse);
                }
                doc.end(&mut app.hist);
            }
            app.reload_slides();
            if collapse {
                // don't leave the selection on a hidden slide
                let cur = app.cur;
                let d = app.slides.get(cur).map(|s| s.depth).unwrap_or(0);
                if let Some(owner) = (0..cur).rev().find(|&j| app.slides[j].depth < d && app.slides[j].collapsed) {
                    app.go_to(owner);
                }
            }
        }
        "Reapply Layout to Slide" => {
            crate::textedit::commit(app);
            if let (Some(slide), Some(doc)) = (app.current_slide().map(|s| s.slide), app.doc.as_mut()) {
                doc.begin(&mut app.hist, "Reapply Layout");
                doc.reapply_layout(&mut app.hist, slide);
                doc.end(&mut app.hist);
                app.bump(slide);
            }
        }
        "Replace Fonts…" => crate::replacefonts::open(app),
        n if n.starts_with("Mask with Shape > ") => {
            let shape = BasicShape::ALL.into_iter().find(|b| Some(b.name()) == n.strip_prefix("Mask with Shape > "));
            if let (Some(shape), Some(&id), Some(slide)) = (shape, app.sel.first(), app.current_slide().map(|s| s.slide)) {
                if let Some(doc) = app.doc.as_mut() {
                    doc.begin(&mut app.hist, "Mask with Shape");
                    doc.mask_with_shape(&mut app.hist, id, shape);
                    doc.end(&mut app.hist);
                }
                app.bump(slide);
            }
        }
        "Character Spacing > Use None" | "Character Spacing > Tighten" | "Character Spacing > Loosen" => {
            // Keynote steps tracking by 1% of the font size
            let cur = selected_run_tracking(app);
            let t = match name {
                "Character Spacing > Tighten" => cur - 0.01,
                "Character Spacing > Loosen" => cur + 0.01,
                _ => 0.0,
            };
            crate::editing::set_text(app, "Character Spacing", kn_model::format::TextProp::Tracking(t));
        }
        "Copy Animation" => {
            if let (Some(doc), Some(slide), Some(&id)) = (app.doc.as_ref(), app.current_slide().map(|s| s.slide), app.sel.first()) {
                let b = doc.copy_animation(slide, id);
                if b.is_empty() {
                    app.notify("This object has no animations");
                } else {
                    app.copied_animation = Some(b);
                }
            }
        }
        "Paste Animation" => {
            let (Some(builds), Some(slide)) = (app.copied_animation.clone(), app.current_slide().map(|s| s.slide)) else { return };
            let targets = app.sel.clone();
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Paste Animation");
                let r: Result<(), _> = targets.iter().try_for_each(|t| doc.paste_animation(&mut app.hist, slide, *t, &builds));
                doc.end(&mut app.hist);
                if let Err(e) = r {
                    app.notify(format!("Couldn't paste the animation: {e}"));
                }
            }
            app.bump(slide);
        }
        "Show Next Comment" => step_comment(app, true),
        "Show Previous Comment" => step_comment(app, false),
        "Remove Highlights and Comments" => {
            let Some(s) = slide else { return };
            let Some(sc) = app.scene(app.cur) else { return };
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, name);
            for d in sc.drawables.iter().filter(|d| !d.from_template) {
                if doc.comment(d.id).is_some() {
                    doc.delete_comment(&mut app.hist, d.id);
                }
            }
            doc.end(&mut app.hist);
            app.bump(s);
        }
        "Play Slideshow" => app.start_slideshow(app.cur),
        "In Fullscreen" | "In Window" => {
            app.settings.play_in_window = name == "In Window";
            app.settings.save();
        }
        "Rehearse Slideshow" => app.start_play(app.cur, true),
        "Find…" | "Find & Replace…" => app.find.show(),
        "PDF…" => app.export.show(crate::export::Format::Pdf),
        "Images…" => app.export.show(crate::export::Format::Png),
        "Movie…" => app.export.show(crate::export::Format::Movie),
        "Animated GIF…" => app.export.show(crate::export::Format::Gif),
        "Print…" => crate::export::print(app),
        "Fit Slide" => app.zoom = crate::app::Zoom::Fit,
        "Actual Size" => app.zoom = crate::app::Zoom::Percent(100.0),
        "Zoom In" | "Zoom Out" => {
            let cur = match app.zoom {
                crate::app::Zoom::Percent(p) => p,
                crate::app::Zoom::Fit => 50.0,
            };
            let next = if name == "Zoom In" { cur * 1.25 } else { cur / 1.25 };
            app.zoom = crate::app::Zoom::Percent(next.clamp(10.0, 800.0));
        }
        "Full Screen" | "Enter Full Screen" => {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
        }
        // in the slide navigator, Cut / Copy / Paste act on slides (Keynote)
        "Copy" | "Cut" if app.nav_focus && app.canvas.editing.is_none() && app.sel.is_empty() => {
            let mut idx = app.slide_sel.clone();
            idx.sort_unstable();
            let pairs: Vec<(ObjId, ObjId)> = idx.iter().filter_map(|i| app.slides.get(*i)).map(|s| (s.slide, s.node)).collect();
            if let Some(doc) = app.doc.as_ref() {
                if !pairs.is_empty() {
                    app.slide_clip = Some((std::sync::Arc::new(doc.file.clone()), pairs, kn_model::insert::doc_identity(doc)));
                    ctx.copy_text(crate::editing::CLIP_MARKER.to_string());
                }
            }
            if name == "Cut" {
                run(app, ctx, "Delete Slide");
            }
        }
        "Paste" if app.nav_focus && app.canvas.editing.is_none() && app.slide_clip.is_some() => {
            let (src, pairs, id) = app.slide_clip.clone().unwrap();
            let Some(doc) = app.doc.as_mut() else { return };
            if id != kn_model::insert::doc_identity(doc) {
                app.notify("Slides can only be pasted into the presentation they were copied from (for now)");
                return;
            }
            let at = app.slide_sel.iter().max().copied().unwrap_or(app.cur) + 1;
            doc.begin(&mut app.hist, "Paste Slides");
            let r = doc.paste_slides(&mut app.hist, &src, &pairs, at.min(app.slides.len()));
            doc.end(&mut app.hist);
            match r {
                Ok(i) => {
                    app.bump_all();
                    app.go_to(i);
                    app.slide_sel = (i..i + pairs.len()).collect();
                }
                Err(e) => app.notify(format!("Couldn't paste the slides: {e}")),
            }
        }
        // while typing, Cut / Copy act on the selected text
        "Cut" if app.canvas.editing.is_some() => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut),
        "Copy" if app.canvas.editing.is_some() => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy),
        "Copy" | "Cut" if crate::tableops::cells_active(app) => {
            if let Some(tsv) = crate::tableops::copy_cells(app) {
                ctx.copy_text(tsv);
                if name == "Cut" {
                    crate::tableops::run(app, "Clear All");
                }
            }
        }
        "Cut" => {
            if crate::editing::copy(app) {
                ctx.copy_text(crate::editing::CLIP_MARKER.to_string());
                run(app, ctx, "Delete");
            }
        }
        "Paste" | "Paste and Match Style" | "Paste and Preserve Style" | "Paste Formula Results" if crate::tableops::cells_active(app) && app.cell_clip.is_some() => {
            crate::tableops::paste_cells(app, if name == "Paste Formula Results" { name } else { "Paste" }, None)
        }
        "Copy" => {
            if crate::editing::copy(app) {
                ctx.copy_text(crate::editing::CLIP_MARKER.to_string());
            }
        }
        // while typing, the menu pastes the clipboard's text into the text (in the text's own style)
        "Paste" | "Paste and Match Style" | "Paste and Preserve Style" if app.canvas.editing.is_some() => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
        "Paste" | "Paste and Match Style" | "Paste and Preserve Style" => crate::editing::paste(app, "Paste"),
        "Show Spelling and Grammar" => crate::spellwin::show(app),
        "Show Warnings" => crate::warnings::show(app),
        "Remove Background" | "Reset Background" => {
            let (Some(&id), Some(s)) = (app.sel.first(), slide) else { return };
            let Some(doc) = app.doc.as_ref() else { return };
            if doc.file.type_name(id) != "TSD.ImageArchive" {
                return;
            }
            let png = if name == "Remove Background" {
                let Some(bytes) = doc.image_original_data(id).and_then(|d| doc.file.data(d)).map(|(_, b)| b.into_owned()) else { return };
                match crate::imageadj::remove_background(&bytes) {
                    Some(p) => Some(p),
                    None => {
                        app.notify("No background could be found to remove");
                        return;
                    }
                }
            } else {
                None
            };
            let mut err = None;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, name);
                err = doc.set_background_removed(&mut app.hist, id, png).err();
                doc.end(&mut app.hist);
            }
            if let Some(e) = err {
                app.notify(format!("{e}"));
            }
            app.bump(s);
        }
        "Show Presenter Display in Window" => toggle(app, |s| &mut s.presenter_in_window),
        "Customize Presenter Display…" => app.show_pd_custom = true,
        "Auto-Center" => app.auto_center = !app.auto_center,
        "Show Borders" | "Show Y Axis Title" | "Show Y Axis Labels" | "Show Y Axis 2 Title" | "Show Y Axis 2 Labels" | "Show X Axis Title" | "Show X Axis Labels" | "Show Series Value Labels" | "Show Series Names" => {
            use kn_model::chartdata::ChartFlag;
            let (Some(id), Some(s)) = (selected_chart(app), slide) else { return };
            let cur = chart_flag_state(app, id, name).unwrap_or(false);
            let flag = match name {
                "Show Borders" => ChartFlag::Border(!cur),
                "Show Y Axis Title" => ChartFlag::Axis(Some(0), "showtitle", !cur),
                "Show Y Axis Labels" => ChartFlag::Axis(Some(0), "showlabels", !cur),
                "Show Y Axis 2 Title" => ChartFlag::Axis(Some(1), "showtitle", !cur),
                "Show Y Axis 2 Labels" => ChartFlag::Axis(Some(1), "showlabels", !cur),
                "Show X Axis Title" => ChartFlag::Axis(None, "showtitle", !cur),
                "Show X Axis Labels" => ChartFlag::Axis(None, "showlabels", !cur),
                "Show Series Names" => ChartFlag::SeriesNames(!cur),
                _ => ChartFlag::ValueLabels(!cur),
            };
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Chart");
                doc.set_chart_flag(&mut app.hist, id, flag);
                doc.end(&mut app.hist);
            }
            app.bump(s);
        }
        "Resize Row to Fit Content" | "Resize Column to Fit Content" => {
            let rows = name.contains("Row");
            let Some((id, r0, c0, r1, c1)) = crate::tableops::range(app) else {
                app.notify("Select table cells first");
                return;
            };
            let idx: Vec<u32> = if rows { (r0..=r1).collect() } else { (c0..=c1).collect() };
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, name);
                doc.fit_table_to_content(&mut app.hist, id, rows, &idx);
                doc.end(&mut app.hist);
            }
            if let Some(s) = slide {
                app.bump(s);
            }
        }
        n if n.strip_prefix("Formula > ").and_then(kn_model::tableformula::Func::from_menu).is_some() => {
            let f = n.strip_prefix("Formula > ").and_then(kn_model::tableformula::Func::from_menu).unwrap();
            let (Some((id, r, c)), Some(s)) = (crate::canvas::table_cursor(app), slide) else { return };
            crate::canvas::commit_cell(app);
            // a range of cells selected: the result goes in the first cell below it (a row
            // range: to its right), one per column (Keynote, round 30)
            let range = crate::tableops::range(app).filter(|(t, r0, c0, r1, c1)| *t == id && (r0, c0) != (r1, c1));
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Insert Formula");
            let mut select = None;
            let res = crate::textures::guarded("formula", || match range {
                Some((_, r0, c0, r1, c1)) if r0 == r1 => {
                    select = Some((r0, c1 + 1));
                    doc.insert_formula_over(&mut app.hist, id, r0, c1 + 1, f, (r0, c0, r1, c1))
                }
                Some((_, r0, c0, r1, c1)) => {
                    select = Some((r1 + 1, c0));
                    (c0..=c1).try_for_each(|cc| doc.insert_formula_over(&mut app.hist, id, r1 + 1, cc, f, (r0, cc, r1, cc)))
                }
                None => doc.insert_formula(&mut app.hist, id, r, c, f),
            });
            doc.end(&mut app.hist);
            if let Some((sr, sc)) = select {
                app.canvas.table_cursor = Some((id, sr, sc));
                app.canvas.table_range = None;
            }
            match res {
                Some(Ok(())) => {}
                Some(Err(e)) => app.notify(e.to_string()),
                None => app.notify("Couldn't insert the formula"),
            }
            app.bump(s);
        }
        "Sort Ascending" | "Sort Descending" => {
            // the body rows by the selected cell's column (Keynote: a column tab's menu)
            let (Some((id, _, c)), Some(s)) = (crate::canvas::table_cursor(app), slide) else { return };
            crate::canvas::commit_cell(app);
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, name);
            let r = doc.sort_table_rows(&mut app.hist, id, c, name == "Sort Ascending");
            doc.end(&mut app.hist);
            if let Err(e) = r {
                app.notify(e.to_string());
            }
            app.bump(s);
        }
        "Web Video…" => crate::webmedia::show_url_sheet(app),
        "Live Video" => {
            let Some(s) = slide else { return };
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Insert Live Video");
            let r = doc.insert_live_video(&mut app.hist, s);
            doc.end(&mut app.hist);
            match r {
                Ok(id) => app.sel = vec![id],
                Err(e) => app.notify(format!("Couldn't insert live video: {e}")),
            }
            app.bump(s);
        }
        "Show Live Video Preview" => {
            app.live_preview = !app.live_preview;
            if !app.live_preview {
                app.live_cam = None;
            }
        }
        "Record Slideshow..." | "Record Slideshow…" => crate::play::start_narration(app, app.cur),
        "Play Recorded Slideshow" => crate::play::start_replay(app, ctx),
        "Clear Recording…" | "Clear Recording..." => app.confirm_clear_recording = true,
        "Auto-Align Table Cell" | "Align Left" | "Align Center" | "Align Right" | "Justify" if crate::tableops::cells_active(app) => {
            use kn_model::tablecells::CellAlign;
            let align = match name {
                "Align Left" => CellAlign::Left,
                "Align Center" => CellAlign::Center,
                "Align Right" => CellAlign::Right,
                "Justify" => CellAlign::Justify,
                _ => CellAlign::Auto,
            };
            let (Some((id, r0, c0, r1, c1)), Some(s)) = (crate::tableops::range(app), slide) else { return };
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Alignment");
            let r = crate::textures::guarded("align cells", || doc.set_cells_alignment(&mut app.hist, id, (r0, c0, r1, c1), align));
            doc.end(&mut app.hist);
            match r {
                Some(Ok(_)) => {}
                Some(Err(e)) => app.notify(e.to_string()),
                None => app.notify("Couldn't align the cells"),
            }
            app.bump(s);
        }
        "Conditional Highlighting..." | "Conditional Highlighting…" => crate::condwin::show(app),
        "Formula > New Formula" => {
            // the cell editor, ready for a formula such as =SUM(B2:B4)
            if let Some((id, r, c)) = crate::canvas::table_cursor(app) {
                app.canvas.cell = Some((id, r, c, "=".into(), true));
            }
        }
        "Image Gallery" => crate::webmedia::pick_gallery(app),
        "Merge Cells" | "Unmerge Cells" => {
            let (Some((id, r0, c0, r1, c1)), Some(s)) = (crate::tableops::range(app), slide) else { return };
            crate::canvas::commit_cell(app);
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, name);
            let r = if name == "Merge Cells" {
                doc.merge_cells(&mut app.hist, id, kn_model::CellRange { row: r0, col: c0, rows: r1 - r0 + 1, cols: c1 - c0 + 1 }).map_err(|e| e.to_string())
            } else if doc.unmerge_cells(&mut app.hist, id, r0, c0) {
                Ok(())
            } else {
                Err("These cells aren't merged".to_string())
            };
            doc.end(&mut app.hist);
            match r {
                Ok(()) => {
                    app.canvas.table_cursor = Some((id, r0, c0));
                    app.canvas.table_range = None;
                }
                Err(e) => app.notify(e),
            }
            app.bump(s);
        }
        "Distribute Rows Evenly" | "Distribute Columns Evenly" => {
            let (Some(id), Some(s)) = (selected_table(app), slide) else { return };
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, name);
                doc.distribute_table(&mut app.hist, id, name == "Distribute Rows Evenly");
                doc.end(&mut app.hist);
            }
            app.bump(s);
        }
        "Show Photo and Video Browser" => app.media.open = Some(false),
        "Show Music Browser" => app.media.open = Some(true),
        "Record Audio" => app.media.record_open = true,
        "Set Password…" | "Change Password…" => app.dialogs.set_password = Some((String::new(), String::new(), String::new())),
        "Change Theme..." | "Change Theme…" | "Change Theme" => {
            app.changing_theme = true;
            app.show_theme_chooser = true;
        }
        "Browse All Versions…" => app.dialogs.versions = true,
        "Save Theme…" => app.dialogs.save_theme = Some(app.tab_name(app.active_tab)),
        "Language & Region…" => app.dialogs.language = true,
        "Single File" => crate::versions::set_package(app, false),
        "Package" => crate::versions::set_package(app, true),
        "PowerPoint…" => {
            app.export.show(crate::export::Format::Pptx);
        }
        "HTML…" => {
            app.export.show(crate::export::Format::Html);
        }
        "Line > Draw With Pen" | "Draw With Pen" => {
            app.sel.clear();
            app.pen = Some(Vec::new());
        }
        n if n.starts_with("Select > ") || n.starts_with("Autofill") || n == "Clear All" => {
            if !crate::tableops::run(app, n) {
                app.notify("Select a table cell first");
            }
        }
        "Unite Shapes" | "Intersect Shapes" | "Subtract Shapes" | "Exclude Shapes" => {
            use kn_model::boolops::BoolOp;
            let op = match name {
                "Unite Shapes" => BoolOp::Unite,
                "Intersect Shapes" => BoolOp::Intersect,
                "Subtract Shapes" => BoolOp::Subtract,
                _ => BoolOp::Exclude,
            };
            let (ids, si, s) = (app.sel.clone(), app.cur, slide);
            let mut res = None;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, name);
                res = Some(doc.shapes_boolean(&mut app.hist, si, &ids, op));
                doc.end(&mut app.hist);
            }
            match res {
                Some(Ok(id)) => app.sel = vec![id],
                Some(Err(e)) => app.notify(format!("{e}")),
                None => {}
            }
            if let Some(s) = s {
                app.bump(s);
            }
        }
        "Make Editable" => {
            let (Some(&id), Some(s)) = (app.sel.first(), slide) else { return };
            let outline = app.scene(app.cur).and_then(|sc| sc.drawables.into_iter().find(|d| d.id == id)).and_then(|d| match d.kind {
                kn_model::DrawableKind::Shape(sh) => Some(sh.path),
                _ => None,
            });
            let Some(outline) = outline else { return };
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Make Editable");
                doc.make_editable(&mut app.hist, id, &outline);
                doc.end(&mut app.hist);
            }
            app.bump(s);
            if let Some(n) = app.doc.as_ref().and_then(|d| d.editable_nodes(id)) {
                app.point_edit = Some(crate::canvas::PointEdit::new(id, n));
            }
        }
        "Make Motion Path from Shape" => {
            // two objects selected: the shape (the path) and the object to move along it
            let Some(s) = slide else { return };
            let Some(sc) = app.scene(app.cur) else { return };
            let sel: Vec<&kn_model::Drawable> = sc.drawables.iter().filter(|d| app.sel.contains(&d.id)).collect();
            let shape = sel.iter().rev().find(|d| matches!(&d.kind, kn_model::DrawableKind::Shape(sh) if sh.text.as_ref().is_none_or(|t| t.text.trim().is_empty())));
            let (Some(shape), true) = (shape, sel.len() == 2) else {
                app.notify("Select a shape (the path) and the object to move along it");
                return;
            };
            let object = sel.iter().find(|d| d.id != shape.id).map(|d| d.id).unwrap();
            let kn_model::DrawableKind::Shape(sh) = &shape.kind else { return };
            let (sid, sg, path) = (shape.id, shape.geom, sh.path.clone());
            let mut err = None;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Motion Path");
                err = doc.motion_path_from_shape(&mut app.hist, s, sid, sg, &path, object).err();
                doc.end(&mut app.hist);
            }
            if let Some(e) = err {
                app.notify(format!("{e}"));
            } else {
                app.sel = vec![object];
                app.inspector = crate::app::InspectorKind::Animate;
                app.show_inspector = true;
            }
            app.bump(s);
        }
        "Reset Text and Object Handles" => {
            let ids = app.sel.clone();
            crate::editing::apply(app, name, |doc, h, id| ids.contains(&id) && doc.reset_handles(h, id));
        }
        "Set as Default Text Box Appearance" => {
            let style = app.sel.first().and_then(|id| app.doc.as_ref()?.file.obj(*id)?.find_super("TSD.ShapeArchive")?.ref_id("style"));
            let me = app.doc.as_ref().and_then(|d| d.path.clone());
            match style {
                Some(s) => {
                    app.default_textbox = Some((me, s));
                    app.notify("New text boxes will look like this one");
                }
                None => app.notify("Select a text box first"),
            }
        }
        "Highlight" => {
            let Some((storage, a, b)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor))) else {
                app.notify("Select some text to highlight");
                return;
            };
            if a == b {
                app.notify("Select some text to highlight");
                return;
            }
            let who = user_name();
            let [r, g, bl] = crate::settings::AUTHOR_COLORS[app.settings.author_color as usize % 10].1;
            let ok = app.doc.as_mut().map(|doc| {
                doc.begin(&mut app.hist, "Highlight");
                let ok = doc.add_highlight(&mut app.hist, storage, a, b, &who).is_some();
                doc.set_author_color(&mut app.hist, &who, kn_model::Color { r, g, b: bl, a: 1.0 });
                doc.end(&mut app.hist);
                // typing carries on as its own undo step
                doc.begin(&mut app.hist, "Typing");
                ok
            });
            if ok != Some(true) {
                app.notify("Couldn't highlight that text");
            }
            if let Some(ed) = app.canvas.editing.as_mut() {
                ed.version += 1;
                ed.scene = None;
            }
            if let Some(s) = slide {
                app.bump(s);
            }
        }
        "Equation..." | "Equation…" => app.equation.open = true,
        "Define as Text Placeholder" | "Define as Media Placeholder" => {
            if app.editing_layouts.is_none() {
                app.notify("Choose View > Edit Slide Layouts first, then select the object");
                return;
            }
            let Some(&id) = app.sel.first() else {
                app.notify("Select an object on the layout");
                return;
            };
            let media = name.contains("Media");
            app.dialogs.define_placeholder = Some((id, if media { "Media".into() } else { "Text".into() }, if media { "Media".into() } else { "Text".into() }, media));
        }
        "Create Style" => {
            let storage = app.canvas.editing.as_ref().map(|e| e.storage).or_else(|| app.sel.first().and_then(|id| app.doc.as_ref()?.storage_of(*id)));
            let Some(storage) = storage else {
                app.notify("Select some text first");
                return;
            };
            let mut made = None;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Create Style");
                made = doc.create_paragraph_style(&mut app.hist, storage);
                doc.end(&mut app.hist);
            }
            match made {
                Some(n) => app.notify(format!("Created the paragraph style \u{201c}{n}\u{201d}")),
                None => app.notify("Couldn't create a style from this text"),
            }
            if let Some(s) = slide {
                app.bump(s);
            }
        }
        "Save to My Shapes" => crate::editing::save_to_my_shapes(app),
        "Break Apart" => {
            let (Some(&id), Some(s)) = (app.sel.first(), slide) else { return };
            let outline = app.scene(app.cur).and_then(|sc| sc.drawables.into_iter().find(|d| d.id == id)).and_then(|d| match d.kind {
                kn_model::DrawableKind::Shape(sh) => Some(sh.path),
                _ => None,
            });
            let Some(outline) = outline else { return };
            let mut res = None;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Break Apart");
                res = Some(doc.break_apart(&mut app.hist, s, id, &outline));
                doc.end(&mut app.hist);
            }
            match res {
                Some(Ok(ids)) => app.sel = ids,
                Some(Err(e)) => app.notify(format!("{e}")),
                None => {}
            }
            app.bump(s);
        }
        "Make Sharp Point" | "Make Smooth Point" | "Make Bézier Point" | "Toggle Line Type" => {
            use kn_model::points::NodeKind;
            let Some(s) = slide else { return };
            let Some(pe) = app.point_edit.as_mut() else {
                app.notify("Choose Make Editable and select a point first");
                return;
            };
            match name {
                "Toggle Line Type" => {
                    // all corners become curves, or all curves corners
                    let any_curve = pe.nodes.subpaths.iter().flat_map(|s| s.nodes.iter()).any(|n| n.kind != NodeKind::Sharp);
                    let k = if any_curve { NodeKind::Sharp } else { NodeKind::Smooth };
                    for si in 0..pe.nodes.subpaths.len() {
                        for i in 0..pe.nodes.subpaths[si].nodes.len() {
                            pe.nodes.set_kind(si, i, k);
                        }
                    }
                }
                _ => {
                    let Some((si, i)) = pe.selected else {
                        app.notify("Select a point first");
                        return;
                    };
                    let k = match name {
                        "Make Sharp Point" => NodeKind::Sharp,
                        "Make Smooth Point" => NodeKind::Smooth,
                        _ => NodeKind::Bezier,
                    };
                    pe.nodes.set_kind(si, i, k);
                }
            }
            let (id, nodes) = (pe.id, pe.nodes.clone());
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, name);
                doc.set_editable_nodes(&mut app.hist, id, &nodes);
                doc.end(&mut app.hist);
            }
            app.bump(s);
        }
        "Customize Toolbar…" => app.show_customize_toolbar = true,
        "Show Arrange Tools" => app.tools.arrange = !app.tools.arrange,
        "Show Colors" => app.tools.colors = !app.tools.colors,
        "Show Fonts" => crate::toolwins::show_fonts(app),
        "Show Adjust Image" => app.show_adjust_image = !app.show_adjust_image,
        "Reset Mask" => {
            let (Some(&id), Some(s)) = (app.sel.first(), slide) else { return };
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Reset Mask");
                doc.reset_mask(&mut app.hist, id);
                doc.end(&mut app.hist);
            }
            app.bump(s);
        }
        "Auto Crop" => crate::editing::auto_crop(app),
        "Edit Mask" => {
            let id = app.sel.first().copied().filter(|id| app.doc.as_ref().is_some_and(|d| d.file.type_name(*id) == "TSD.ImageArchive"));
            app.mask_edit = match (app.mask_edit, id) {
                (Some(_), _) => None,
                // (a picture without a mask edits a full-frame one, made at the first change)
                (None, Some(id)) => Some(id),
                _ => None,
            };
        }
        "Reduce File Size…" => crate::warnings::reduce_file_size(app),
        "Hide Guides" | "Show Guides" => app.show_guides = !app.show_guides,
        "Show Slide Layout Guides" | "Hide Slide Layout Guides" => app.show_layout_guides = !app.show_layout_guides,
        "Clear All Guides on Slide" | "Paste Guides" => {
            let Some(s) = slide else { return };
            let gs = if name == "Paste Guides" { app.copied_guides.clone().unwrap_or_default() } else { Vec::new() };
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Guides");
                doc.set_guides(&mut app.hist, s, &gs);
                doc.end(&mut app.hist);
            }
            app.bump(s);
        }
        "Copy Guides" => {
            if let (Some(s), Some(doc)) = (slide, app.doc.as_ref()) {
                app.copied_guides = Some(doc.guides(s));
            }
        }
        "Hide Tab Bar" | "Show Tab Bar" => app.show_tab_bar = !app.show_tab_bar,
        n if n.starts_with("Author Color > ") => {
            let Some(i) = crate::settings::AUTHOR_COLORS.iter().position(|(c, _)| Some(*c) == n.strip_prefix("Author Color > ")) else { return };
            app.settings.author_color = i as u8;
            app.settings.save();
            apply_author_color(app);
        }
        "Show All Tabs" => app.show_all_tabs = true,
        "Check Document Now" => crate::spellwin::check_now(app),
        "Check Grammar With Spelling" => toggle(app, |s| &mut s.grammar),
        "Correct Spelling Automatically" => toggle(app, |s| &mut s.auto_correct),
        "Show Substitutions" => app.spell_win.substitutions_open = true,
        "Smart Web and Email Links" => toggle(app, |s| &mut s.smart_links),
        "Smart Phone Number Links" => toggle(app, |s| &mut s.smart_phone_links),
        "Text Replacement" => toggle(app, |s| &mut s.text_replacement),
        "Start Speaking" => speak(app),
        "Stop Speaking" => {
            let _ = std::process::Command::new("spd-say").arg("--cancel").spawn();
        }
        "Jump to Selection" => {
            // back to the slide with the selection, scrolled into view
            let i = app.cur;
            app.nav_scrolled_to = None;
            app.go_to(i);
        }
        "Select Parent" => {
            if let Some(id) = app.canvas.editing.as_ref().map(|e| e.id) {
                crate::textedit::commit(app);
                app.sel = vec![id];
            } else if let Some(&id) = app.sel.first() {
                let parent = app.doc.as_ref().and_then(|d| d.group_of(id));
                if let Some(p) = parent {
                    app.sel = vec![p];
                }
            }
        }
        "Duplicate Selection" => crate::editing::duplicate(app),
        "Copy Style" => crate::editing::copy_style(app),
        "Paste Style" => crate::editing::paste_style(app),
        "Text" => crate::editing::insert_object(app, NewObject::TextBox),
        "Choose File" | "Choose…" => app.pick_media(),
        "Browse Content Hub" | "Show Content Hub" => app.hub.show_hub(),
        "Line > Line" => crate::editing::insert_object(app, NewObject::Line { arrow: false }),
        "Chart > Show Chart Data Editor" => {
            if let Some(c) = selected_chart(app) {
                crate::chartedit::open(app, c);
            }
        }
        "Chart > Show Title" => {
            if let Some(c) = selected_chart(app) {
                let cur = app.scene(app.cur).and_then(|sc| sc.drawables.into_iter().find(|d| d.id == c)).and_then(|d| match d.kind {
                    kn_model::DrawableKind::Chart(ch) => Some(ch.title),
                    _ => None,
                });
                let next = match cur.flatten() {
                    Some(_) => None,
                    None => Some("Title"),
                };
                chart_edit(app, "Chart Title", |doc, h| doc.set_chart_title(h, c, next));
            }
        }
        "Chart > Show Legend" => {
            if let Some(c) = selected_chart(app) {
                let show = !app.scene(app.cur).and_then(|sc| sc.drawables.into_iter().find(|d| d.id == c)).is_some_and(|d| matches!(&d.kind, kn_model::DrawableKind::Chart(ch) if ch.show_legend));
                chart_edit(app, "Legend", |doc, h| doc.set_chart_legend(h, c, show));
            }
        }
        n if chart_type_for(n).is_some() => {
            let ty = chart_type_for(n).unwrap();
            if let Some(rest) = n.strip_prefix("Chart > ") {
                insert_chart(app, ty, rest);
            } else if let Some(c) = selected_chart(app) {
                chart_edit(app, "Chart Type", |doc, h| doc.set_chart_type(h, c, ty));
            }
        }
        "Table > Basic" => crate::editing::insert_object(app, NewObject::BASIC_TABLE),
        "Table > Headers" | "Table > Plain" | "Table > Sums" => {
            // the Basic table with Keynote's header / footer setup for each preset
            crate::editing::insert_object(app, NewObject::BASIC_TABLE);
            let (Some(&id), Some(s)) = (app.sel.first(), app.current_slide().map(|s| s.slide)) else { return };
            use kn_model::tablerows::HeaderKind;
            let counts: &[(HeaderKind, u32)] = match name {
                "Table > Headers" => &[(HeaderKind::Rows, 1), (HeaderKind::Columns, 1), (HeaderKind::Footer, 0)],
                "Table > Plain" => &[(HeaderKind::Rows, 0), (HeaderKind::Columns, 0), (HeaderKind::Footer, 0)],
                _ => &[(HeaderKind::Rows, 1), (HeaderKind::Columns, 0), (HeaderKind::Footer, 1)],
            };
            if let Some(doc) = app.doc.as_mut() {
                doc.begin_merge(&mut app.hist, "Insert Table");
                for (k, n) in counts {
                    let _ = doc.set_table_header_count(&mut app.hist, id, *k, *n);
                }
                doc.end(&mut app.hist);
            }
            app.bump(s);
        }
        n if n.starts_with("Header Rows > ") || n.starts_with("Header Columns > ") || n.starts_with("Footer Rows > ") => {
            use kn_model::tablerows::HeaderKind;
            let (kind, count) = n.split_once(" > ").map(|(k, c)| (match k {
                "Header Rows" => HeaderKind::Rows,
                "Header Columns" => HeaderKind::Columns,
                _ => HeaderKind::Footer,
            }, c.parse::<u32>().unwrap_or(0))).unwrap();
            let (Some(id), Some(s)) = (selected_table(app), app.current_slide().map(|s| s.slide)) else { return };
            let mut err = None;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Table Headers");
                err = doc.set_table_header_count(&mut app.hist, id, kind, count).err();
                doc.end(&mut app.hist);
            }
            if let Some(e) = err {
                app.notify(format!("{e}"));
            }
            app.bump(s);
        }
        "Table > Add Row Above" | "Table > Add Row Below" | "Table > Add Column Before" | "Table > Add Column After" | "Table > Delete Row"
        | "Table > Delete Column" => crate::editing::table_rows(app, name),
        "Line > Line with Arrowhead" => crate::editing::insert_object(app, NewObject::Line { arrow: true }),
        "Line > Line with Two Arrowheads" | "Line > Straight Connection Line" | "Line > Curved Connection Line" | "Line > Angled Connection Line" => {
            use kn_model::insert::Connection;
            let Some(slide) = slide else { return };
            // with two objects selected, the connection line joins them
            let ends = (app.sel.len() == 2).then(|| (app.sel[0], app.sel[1]));
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Insert Line");
            let r = match name {
                "Line > Line with Two Arrowheads" => doc.insert_double_arrow_line(&mut app.hist, slide),
                "Line > Curved Connection Line" => doc.insert_connection_line(&mut app.hist, slide, Connection::Curved, ends),
                "Line > Angled Connection Line" => doc.insert_connection_line(&mut app.hist, slide, Connection::Angled, ends),
                _ => doc.insert_connection_line(&mut app.hist, slide, Connection::Straight, ends),
            };
            doc.end(&mut app.hist);
            match r {
                Ok(id) => app.sel = vec![id],
                Err(e) => app.notify(format!("Couldn't insert: {e}")),
            }
            app.bump(slide);
        }
        "Bold" | "Italic" | "Underline" | "Strikethrough" | "Bigger" | "Smaller" => crate::editing::toggle_font(app, name),
        "Align Left" | "Align Center" | "Align Right" | "Justify" => {
            let a = match name {
                "Align Left" => kn_model::Align::Left,
                "Align Center" => kn_model::Align::Center,
                "Align Right" => kn_model::Align::Right,
                _ => kn_model::Align::Justify,
            };
            crate::editing::set_text(app, "Alignment", kn_model::format::TextProp::Align(a));
        }
        other => {
            if let Some(shape) = other.strip_prefix("Shape > ") {
                if let Some(s) = BasicShape::ALL.iter().find(|s| s.name() == shape) {
                    crate::editing::insert_object(app, NewObject::Shape(*s));
                    return;
                }
            }
            if let Some(how) = other.strip_prefix("Align Objects > ").or(other.strip_prefix("Distribute Objects > ")) {
                crate::editing::align(app, how);
                return;
            }
            app.notify(format!("“{other}” isn't available yet"))
        }
    }
}

/// Apply a text edit to the selection being typed (or, when `whole_without_selection`, the
/// caret's paragraph), or to the whole text of every selected object.
fn text_ranges_edit(app: &mut App, label: &str, f: impl Fn(&mut kn_model::Document, &mut kn_model::edit::History, kn_format::ObjId, usize, usize) -> bool, caret_ok: bool) {
    let slide = app.current_slide().map(|s| s.slide);
    let targets: Vec<(kn_format::ObjId, usize, usize)> = if let Some(ed) = app.canvas.editing.as_ref() {
        let (a, b) = (ed.cursor.min(ed.anchor), ed.cursor.max(ed.anchor));
        if a == b && !caret_ok {
            Vec::new()
        } else {
            vec![(ed.storage, a, b)]
        }
    } else {
        let Some(doc) = app.doc.as_ref() else { return };
        app.sel.iter().filter_map(|id| doc.storage_of(*id)).map(|st| (st, 0, doc.storage_text(st).len())).collect()
    };
    if targets.is_empty() {
        return;
    }
    let editing = app.canvas.editing.is_some();
    let Some(doc) = app.doc.as_mut() else { return };
    if editing {
        doc.end(&mut app.hist);
    }
    doc.begin(&mut app.hist, label);
    let mut ok = false;
    for (st, a, b) in targets {
        ok |= f(doc, &mut app.hist, st, a, b);
    }
    doc.end(&mut app.hist);
    if editing {
        doc.begin(&mut app.hist, "Typing");
        if let Some(ed) = app.canvas.editing.as_mut() {
            ed.version += 1;
            ed.scene = None;
        }
    }
    if ok {
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}

/// Insert > Comment: on the selected object (or the slide's first object), then open it for typing.
fn add_comment(app: &mut App) {
    crate::textedit::commit(app);
    let Some(sc) = app.scene(app.cur) else { return };
    let own: Vec<ObjId> = sc.drawables.iter().filter(|d| !d.from_template).map(|d| d.id).collect();
    let Some(target) = app.sel.first().copied().filter(|s| own.contains(s)).or(own.first().copied()) else {
        app.notify("Add an object to the slide to comment on it");
        return;
    };
    app.show_comments = true;
    let Some(doc) = app.doc.as_mut() else { return };
    if let Some(c) = doc.comment(target) {
        app.open_comment = Some((target, c.text));
        return;
    }
    doc.begin(&mut app.hist, "Add Comment");
    let ok = doc.add_comment(&mut app.hist, target, "", &user_name()).is_some();
    if ok {
        let [r, g, b] = crate::settings::AUTHOR_COLORS[app.settings.author_color as usize % 10].1;
        doc.set_author_color(&mut app.hist, &user_name(), kn_model::Color { r, g, b, a: 1.0 });
    }
    doc.end(&mut app.hist);
    if ok {
        app.open_comment = Some((target, String::new()));
        if let Some(sl) = app.slides.get(app.cur).map(|s| s.slide) {
            app.bump(sl);
        }
    } else {
        app.notify("This presentation can't take comments");
    }
}

/// The person's full name (from the passwd entry), else the login name.
fn user_name() -> String {
    let login = std::env::var("USER").unwrap_or_else(|_| "Linux".into());
    std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with(&format!("{login}:"))).and_then(|l| l.split(':').nth(4)).map(|g| g.split(',').next().unwrap_or("").to_string()))
        .filter(|g| !g.trim().is_empty())
        .unwrap_or(login)
}

/// View > Comments > Show Next/Previous Comment: go to the next commented object and open it.
fn step_comment(app: &mut App, forward: bool) {
    let mut all: Vec<(usize, ObjId, String)> = Vec::new();
    for i in 0..app.slides.len() {
        let Some(sc) = app.scene(i) else { continue };
        let Some(doc) = app.doc.as_ref() else { return };
        for d in sc.drawables.iter().filter(|d| !d.from_template) {
            if let Some(c) = doc.comment(d.id) {
                all.push((i, d.id, c.text));
            }
        }
    }
    if all.is_empty() {
        app.notify("No comments");
        return;
    }
    let here = app.open_comment.as_ref().map(|o| o.0);
    let pos = here.and_then(|h| all.iter().position(|c| c.1 == h));
    let n = all.len();
    let next = match (pos, forward) {
        (Some(p), true) => (p + 1) % n,
        (Some(p), false) => (p + n - 1) % n,
        (None, true) => all.iter().position(|c| c.0 >= app.cur).unwrap_or(0),
        (None, false) => all.iter().rposition(|c| c.0 <= app.cur).unwrap_or(n - 1),
    };
    let (i, id, text) = all.swap_remove(next);
    app.show_comments = true;
    if app.cur != i {
        app.go_to(i);
    }
    app.sel = vec![id];
    app.open_comment = Some((id, text));
}

/// Chart type id for an Insert > Chart or Format > Chart > Chart Type item ("2D Stacked Bar").
fn chart_type_for(label: &str) -> Option<&'static str> {
    let l = label.strip_prefix("Chart > ").unwrap_or(label);
    if l.starts_with("Interactive ") {
        return kn_model::chartdata::CHART_TYPES.iter().find(|t| t.1 == l).map(|t| t.0);
    }
    let (dim, name) = l.split_once(' ')?;
    if dim != "2D" && dim != "3D" {
        return None;
    }
    let want = if dim == "3D" { format!("3D {name}") } else { name.to_string() };
    kn_model::chartdata::CHART_TYPES.iter().find(|t| t.1 == want).map(|t| t.0)
}

/// The selected chart (one object selected and it is a chart).
fn selected_chart(app: &App) -> Option<ObjId> {
    let id = *app.sel.first().filter(|_| app.sel.len() == 1)?;
    let doc = app.doc.as_ref()?;
    (doc.file.type_name(id) == "TSCH.ChartDrawableArchive").then_some(id)
}

fn chart_edit(app: &mut App, label: &str, f: impl FnOnce(&mut kn_model::Document, &mut kn_model::edit::History) -> bool) {
    let slide = app.current_slide().map(|s| s.slide);
    if let Some(doc) = app.doc.as_mut() {
        doc.begin(&mut app.hist, label);
        f(doc, &mut app.hist);
        doc.end(&mut app.hist);
    }
    if let Some(sl) = slide {
        app.bump(sl);
    }
}

/// Insert > Chart > any type: Keynote's column (or pie) chart, then switched to the type.
fn insert_chart(app: &mut App, ty: &str, name: &str) {
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    app.canvas.cancel();
    let Some(doc) = app.doc.as_mut() else { return };
    let round = ty.starts_with("pie") || ty.starts_with("donut");
    doc.begin(&mut app.hist, "Insert Chart");
    let r = crate::textures::guarded("insert", || doc.insert(&mut app.hist, slide, &NewObject::Chart { pie: round }));
    if let Some(Ok(id)) = &r {
        if !matches!(ty, "columnChartType2D" | "pieChartType2D") {
            doc.set_chart_type(&mut app.hist, *id, ty);
        }
    }
    doc.end(&mut app.hist);
    match r {
        Some(Ok(id)) => {
            app.sel = vec![id];
            app.bump(slide);
        }
        _ => {
            if app.hist.undo_label() == Some("Insert Chart") {
                app.undo();
            }
            app.notify(format!("Couldn't insert the {name} chart"));
        }
    }
}

/// Character spacing at the start of the text selection (or of the selected text box).
/// Give this user's comments in the document the chosen author colour.
fn apply_author_color(app: &mut App) {
    let [r, g, b] = crate::settings::AUTHOR_COLORS[app.settings.author_color as usize % 10].1;
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, "Author Color");
    let changed = doc.set_author_color(&mut app.hist, &user_name(), kn_model::Color { r, g, b, a: 1.0 });
    doc.end(&mut app.hist);
    if changed {
        app.bump_all();
    }
}

/// Whether a Format > Chart option is on for chart `id`.
fn chart_flag_state(app: &App, id: ObjId, name: &str) -> Option<bool> {
    let sc = app.slides.get(app.cur).and_then(|_| app.doc.as_ref()?.scene(&app.slides, app.cur))?;
    let c = sc.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
        kn_model::DrawableKind::Chart(c) => Some(c.clone()),
        _ => None,
    })?;
    Some(match name {
        "Show Borders" => c.show_border,
        "Show Y Axis Title" => c.value_axes.first().is_some_and(|a| a.title.is_some()),
        "Show Y Axis Labels" => c.value_axes.first().is_some_and(|a| a.show_labels),
        "Show Y Axis 2 Title" => c.value_axes.get(1).is_some_and(|a| a.title.is_some()),
        "Show Y Axis 2 Labels" => c.value_axes.get(1).is_some_and(|a| a.show_labels),
        "Show X Axis Title" => c.category_axis.title.is_some(),
        "Show X Axis Labels" => c.category_axis.show_labels,
        "Show Series Names" => c.series.first().is_some_and(|s| s.show_series_names),
        _ => c.series.first().is_some_and(|s| s.show_value_labels),
    })
}

/// The selected table (or the one whose cell is being edited).
fn selected_table(app: &App) -> Option<ObjId> {
    if let Some((id, ..)) = crate::canvas::table_cursor(app) {
        return Some(id);
    }
    let doc = app.doc.as_ref()?;
    app.sel.iter().copied().find(|id| doc.file.type_name(*id) == "TST.TableInfoArchive")
}

/// Flip an on/off setting from a menu and save it.
fn toggle(app: &mut App, f: impl Fn(&mut crate::settings::Settings) -> &mut bool) {
    let v = f(&mut app.settings);
    *v = !*v;
    app.settings.save();
}

/// Edit > Speech > Start Speaking: the selected text, else the edited text box, else the
/// current slide's text (speech-dispatcher's spd-say).
fn speak(app: &mut App) {
    let mut text = String::new();
    if let Some((storage, a, b)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor))) {
        let t = app.doc.as_ref().map(|d| d.storage_text(storage)).unwrap_or_default();
        text = if a < b { t.get(a..b).unwrap_or("").to_string() } else { t };
    }
    if text.trim().is_empty() {
        if let Some(sc) = app.scene(app.cur) {
            for d in sc.drawables.iter().filter(|d| app.sel.is_empty() || app.sel.contains(&d.id)) {
                if let kn_model::DrawableKind::Shape(s) = &d.kind {
                    if let Some(t) = &s.text {
                        text.push_str(&t.text);
                        text.push_str(". ");
                    }
                }
            }
        }
    }
    let text = text.replace(['\u{2028}', '\u{fffc}'], " ");
    if text.trim().is_empty() {
        return;
    }
    if std::process::Command::new("spd-say").arg("--wait").arg("--").arg(&text).stdin(std::process::Stdio::null()).spawn().is_err() {
        app.notify("Speech needs speech-dispatcher (spd-say)");
    }
}

/// Character style at the caret (or of the selected text box's first run).
fn selected_run_style(app: &mut App) -> Option<kn_model::CharStyle> {
    let (id, at) = match app.canvas.editing.as_ref() {
        Some(e) => (e.id, e.cursor.min(e.anchor)),
        None => (*app.sel.first()?, 0),
    };
    let sc = app.scene(app.cur)?;
    sc.drawables
        .iter()
        .find(|d| d.id == id)
        .and_then(|d| match &d.kind {
            kn_model::DrawableKind::Shape(s) => s.text.as_ref(),
            _ => None,
        })
        .and_then(|t| t.paragraphs.iter().flat_map(|p| p.runs.iter()).find(|r| r.end > at || r.start >= at).map(|r| r.style.clone()))
}

fn selected_run_tracking(app: &mut App) -> f32 {
    selected_run_style(app).map(|s| s.tracking).unwrap_or(0.0)
}

fn selected_run_baseline(app: &mut App) -> f32 {
    selected_run_style(app).map(|s| s.baseline_shift).unwrap_or(0.0)
}

/// One item of a right-click menu: Keynote's icon (if any), label, shortcut, enabled like the
/// menu bar's item; runs the command.
pub fn context_item(app: &mut App, ui: &mut egui::Ui, label: &str, command: &str) {
    let icon = crate::icons::menu_icon(label);
    let mut b = egui::Button::new((egui::Atom::custom(egui::IdSalt::new("menu-icon"), egui::vec2(MENU_ICON_W, 16.0)), label));
    if let Some(sc) = find_shortcut(command) {
        b = b.shortcut_text(sc);
    }
    let on = enabled(app, command);
    let resp = ui.add_enabled(on, b);
    if let Some(i) = icon {
        paint_icon(ui, resp.rect, i, on);
    }
    if resp.clicked() {
        let ctx = ui.ctx().clone();
        ui.close();
        run(app, &ctx, command);
    }
}

/// The shortcut text the menu bar shows for a command.
fn find_shortcut(command: &str) -> Option<String> {
    fn walk(items: &[Item], parent: &str, command: &str) -> Option<String> {
        for it in items {
            if command_name(parent, &it.label) == command {
                if let Some(s) = &it.shortcut {
                    return Some(s.clone());
                }
            }
            if let Some(s) = walk(&it.children, &it.label, command) {
                return Some(s);
            }
        }
        None
    }
    menus().iter().find_map(|m| walk(&m.children, &m.label, command))
}

/// KN_SHOW_MENU=<menu>: draw that menu's items in a window (automated screenshot checks).
pub fn debug_menu(app: &mut App, ctx: &egui::Context) {
    let Ok(want) = std::env::var("KN_SHOW_MENU") else { return };
    let Some(m) = menus().iter().find(|m| m.label == want) else { return };
    let mut fired = None;
    egui::Window::new(format!("menu: {want}")).default_pos([60.0, 60.0]).show(ctx, |ui| {
        ui.set_min_width(300.0);
        let indent = m.children.iter().any(|c| crate::icons::menu_icon(&c.label).is_some());
        for c in &m.children {
            item_ui(app, ui, &m.label, c, indent, &mut fired);
        }
    });
}

/// KN_AUDIT=1 (automated checks): run every implemented command that is enabled now and report
/// the dead ends: "DEAD" (falls through to "isn't available yet") and "NOOP" (nothing that can
/// be seen changed: no undo step, selection, panel, window or message).
pub fn audit(app: &mut App, ctx: &egui::Context) {
    let skip = ["Quit Keydeck", "Close", "Close All", "Close Window", "Revert To", "Minimize", "Zoom", "Enter Full Screen", "Hide Keydeck", "Hide Others", "Play Slideshow", "Play Slideshow in Window", "Rehearse Slideshow", "Record Slideshow...", "Print…", "Open…", "Save", "Save As…", "Export To", "Move To…", "Rename…", "Duplicate", "Start Speaking", "Open Recent"];
    let fingerprint = |app: &App| {
        format!(
            "{} {:?} {:?} {:?} {:?} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {:?} {:?} {:?} {:?} {}",
            app.hist.undo_len(),
            app.sel,
            app.view,
            app.inspector,
            app.zoom,
            app.show_inspector,
            app.show_toolbar,
            app.show_object_list,
            app.show_notes,
            app.show_rulers,
            app.show_comments,
            app.show_about,
            app.show_settings,
            app.show_goto,
            app.show_theme_chooser,
            app.show_help,
            app.show_guides,
            app.show_layout_guides,
            app.show_tab_bar,
            app.show_all_tabs,
            app.show_adjust_image,
            app.show_pd_custom,
            app.show_customize_toolbar,
            app.editing_layouts.is_some(),
            app.tools,
            app.toast.as_ref().map(|t| t.0.clone()),
            app.canvas.editing.is_some(),
            app.canvas.table_cursor,
            app.cur,
        ) + &format!(
            " {} {} {} {} {} {} {} {} {:?} {} {} {} {} {} {} {} {:?} {} {} {} {} {} {} {} {} {} {} {:?}",
            app.export.open,
            app.link_dialog.open,
            app.find.open,
            app.spell_win.open,
            app.spell_win.substitutions_open,
            app.webmedia.url_sheet.is_some(),
            app.cond_win.open,
            app.warnings.open,
            app.media.open,
            app.media.record_open,
            app.dialogs.versions,
            app.dialogs.save_theme.is_some(),
            app.dialogs.language,
            app.dialogs.password_prompt.is_some(),
            app.dialogs.set_password.is_some(),
            app.equation.open,
            app.mask_edit,
            app.auto_center,
            app.copied_style.is_some(),
            app.clip.is_some(),
            app.copied_guides.is_some(),
            app.pen.is_some(),
            app.point_edit.is_some(),
            app.chart_edit.is_some(),
            app.replace_fonts.is_some(),
            app.changing_theme,
            app.live_preview,
            app.settings,
        )
    };
    let mut report = Vec::new();
    // every command starts from the same selection and modes
    let (base_sel, base_cur, base_cursor, base_range, base_view) = (app.sel.clone(), app.cur, app.canvas.table_cursor, app.canvas.table_range, app.view);
    for &name in IMPLEMENTED {
        app.sel = base_sel.clone();
        app.cur = base_cur;
        app.canvas.table_cursor = base_cursor;
        app.canvas.table_range = base_range;
        app.canvas.editing = None;
        app.canvas.cell = None;
        app.view = base_view;
        app.mask_edit = None;
        app.point_edit = None;
        app.pen = None;
        app.chart_edit = None;
        app.editing_layouts = None;
        if skip.contains(&name) || !enabled(app, name) {
            continue;
        }
        app.toast = None;
        {
            use std::io::Write;
            eprintln!("RUN {name}");
            let _ = std::io::stderr().flush();
        }
        let before = fingerprint(app);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(app, ctx, name)));
        let after = fingerprint(app);
        let msg = app.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
        if r.is_err() {
            report.push(format!("PANIC {name}"));
        } else if msg.contains("isn't available yet") {
            report.push(format!("DEAD  {name}"));
        } else if before == after {
            report.push(format!("NOOP  {name}"));
        }
        // undo what it changed so the next command starts from the same document
        if app.hist.undo_len() > 0 && before.split(' ').next() != after.split(' ').next() {
            run(app, ctx, "Undo");
        }
    }
    use std::io::Write;
    for l in report {
        println!("{l}");
    }
    let _ = std::io::stdout().flush();
}
