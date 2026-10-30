//! The window toolbar and View > Customize Toolbar… (Keynote 15.4, measured on the Mac):
//! the toolbar holds a user-chosen list of items (saved per user, not in the document), shown
//! as icons, icons with labels or labels only. Items next to each other share a capsule;
//! spaces separate capsules and flexible spaces take the room left over. While the Customize
//! sheet is open items are dragged in from it, moved along the toolbar or dragged off it;
//! otherwise Ctrl-drag (Keynote's ⌘-drag) moves or removes them. Keynote's Apple Intelligence
//! and Content Hub items (premium Apple services) are left out; our Content Hub is in the Media popover.

use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};

use crate::app::{App, InspectorKind, Zoom};
use crate::icons::{draw, Icon};
use crate::theme;

pub const VIEW: &str = "TSKToolbarItemView";
pub const ZOOM: &str = "TSKToolbarItemZoom";
pub const INSPECTOR: &str = "TSKToolbarItemInspectorSwitch";
pub const SPACE: &str = "NSToolbarSpaceItem";
pub const FLEX: &str = "NSToolbarFlexibleSpaceItem";

/// One toolbar item: Keynote's identifier, label, tooltip and icon.
pub struct Def {
    pub id: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
    pub icon: Icon,
}

const fn d(id: &'static str, label: &'static str, tip: &'static str, icon: Icon) -> Def {
    Def { id, label, tip, icon }
}

/// Every item, in the Customize sheet's order.
pub const ITEMS: &[Def] = &[
    d(VIEW, "View", "Choose what to show or hide while creating a presentation.", Icon::Sidebar),
    d(ZOOM, "Zoom", "Click to enlarge or reduce the view of the presentation.", Icon::ChevronDown),
    d("KNMacToolbarItemAddSlide", "Add Slide", "Choose a slide to add to this presentation.", Icon::AddSlide),
    d("TSKToolbarItemPlay", "Play", "Play the slideshow from the selected slide.", Icon::Play),
    d("TSKToolbarItemTable", "Table", "Choose a table to add to this presentation.", Icon::Table),
    d("TSKToolbarItemChart", "Chart", "Choose a chart to add to this presentation.", Icon::Chart),
    d("TSKToolbarItemText", "Text", "Add a text box to this presentation.", Icon::Text),
    d("TSKToolbarItemShape", "Shape", "Choose a shape or line to add to this presentation, or use the pen tool.", Icon::Shape),
    d("TSKToolbarItemMedia", "Media", "Choose an image, movie, or audio file to add to this presentation.", Icon::Media),
    d("TSKToolbarItemComment", "Comment", "Insert a comment, or add one to the selected text, object, or table cell.", Icon::Comment),
    d("TSKToolbarItemShare", "Share", "Share this presentation (not available on Linux).", Icon::Share),
    d(INSPECTOR, "Format", "Show or hide format and style options.", Icon::Format),
    d("KNMacToolbarItemPlayInWindow", "Play in Window", "Play the slideshow from the selected slide in a resizable window.", Icon::PlayWindow),
    d("KNMacToolbarItemRehearse", "Rehearse", "Rehearse the slideshow.", Icon::Rehearse),
    d("TSKToolbarItemObjectList", "Object List", "Show or hide the object list.", Icon::List),
    d("KNMacToolbarItemEditLayouts", "Layouts", "Edit the slide layouts.", Icon::Layouts),
    d("KNMacToolbarItemSkipSlide", "Skip", "Skip or include selected slides when playing the slideshow.", Icon::Skip),
    d("KNMacToolbarItemRecord", "Record", "Record the slideshow and narration.", Icon::Record),
    d("TSKToolbarItemForward", "Forward", "Move selected objects forward one layer.", Icon::LayerForward),
    d("TSKToolbarItemBackward", "Backward", "Move selected objects backward one layer.", Icon::LayerBackward),
    d("TSKToolbarItemFront", "Front", "Move selected objects to the front layer.", Icon::LayerFront),
    d("TSKToolbarItemBack", "Back", "Move selected objects to the back layer.", Icon::LayerBack),
    d("TSKToolbarItemFlipHorizontal", "Flip Horizontal", "Flip the object horizontally.", Icon::FlipH),
    d("TSKToolbarItemFlipVertical", "Flip Vertical", "Flip the object vertically.", Icon::FlipV),
    d("TSKToolbarItemGroup", "Group", "Combine the selected objects into a single object.", Icon::Group),
    d("TSKToolbarItemUngroup", "Ungroup", "Ungroup selected objects.", Icon::Ungroup),
    d("TSKToolbarItemBigger", "Bigger", "Make selected text one size larger.", Icon::Bigger),
    d("TSKToolbarItemSmaller", "Smaller", "Make selected text one size smaller.", Icon::Smaller),
    d("TSKToolbarItemSuperscript", "Superscript", "Make selected text a superscript.", Icon::Superscript),
    d("TSKToolbarItemSubscript", "Subscript", "Make selected text a subscript.", Icon::Subscript),
    d("TSKToolbarItemOutdent", "Outdent", "Decrease the indent level of the selected text.", Icon::Outdent),
    d("TSKToolbarItemIndent", "Indent", "Increase the indent level of the selected text.", Icon::Indent),
    d("TSKToolbarItemConnect", "Connect", "Connect two selected objects with a connection line.", Icon::Connect),
    d("TSKToolbarItemCopyStyle", "Copy Style", "Copy the attributes of the selected text or object.", Icon::CopyStyle),
    d("TSKToolbarItemPasteStyle", "Paste Style", "Paste the copied attributes to the selected text or object.", Icon::PasteStyle),
    d("TSKToolbarItemGuides", "Guides", "Hide alignment guides.", Icon::Guides),
    d("TSKToolbarItemMask", "Mask", "Change how much of the image is visible.", Icon::Mask),
    d("TSKToolbarItemRemoveBackground", "Remove Background", "Make the background or other parts of the image transparent.", Icon::RemoveBg),
    d("TSKToolbarItemLock", "Lock", "Lock selected objects to prevent editing.", Icon::Lock),
    d("TSKToolbarItemUnlock", "Unlock", "Unlock selected objects to allow editing.", Icon::Unlock),
    d("NSToolbarShowFontsItem", "Fonts", "Show or hide the Font window.", Icon::Fonts),
    d("TSKToolbarItemLink", "Link", "Make the selected text a link to a webpage or email address.", Icon::Link),
    d("TSKToolbarItemAdjustImage", "Adjust Image", "Show advanced image adjustment options.", Icon::Sliders),
    d("TSKToolbarItemFind", "Find", "Show Find & Replace.", Icon::Search),
    d("NSToolbarPrintItem", "Print", "Print all or part of the document.", Icon::Print),
    d("NSToolbarShowColorsItem", "Colors", "Show the Colors window.", Icon::Palette),
    d(SPACE, "Space", "", Icon::Overflow),
    d(FLEX, "Flexible Space", "", Icon::Overflow),
];

/// Keynote's default set (without the Apple-service items Share and Content Hub, which do
/// nothing here).
pub const DEFAULT: &[&str] = &[
    VIEW,
    FLEX,
    "TSKToolbarItemPlay",
    SPACE,
    "KNMacToolbarItemAddSlide",
    SPACE,
    "TSKToolbarItemTable",
    "TSKToolbarItemChart",
    "TSKToolbarItemText",
    "TSKToolbarItemShape",
    "TSKToolbarItemMedia",
    SPACE,
    "TSKToolbarItemComment",
    SPACE,
    ZOOM,
    SPACE,
    INSPECTOR,
];

pub fn def(id: &str) -> Option<&'static Def> {
    ITEMS.iter().find(|d| d.id == id)
}

fn is_space(id: &str) -> bool {
    id == SPACE || id == FLEX
}

/// The toolbar's items from the settings (the default set when none are saved).
pub fn items(app: &App) -> Vec<String> {
    let s = app.settings.toolbar_items.trim();
    if s.is_empty() {
        return DEFAULT.iter().map(|s| s.to_string()).collect();
    }
    let mut out: Vec<String> = Vec::new();
    for id in s.split(',').map(str::trim).filter(|id| def(id).is_some()) {
        if is_space(id) || !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    out
}

fn set_items(app: &mut App, list: &[String]) {
    let is_default = list.len() == DEFAULT.len() && list.iter().zip(DEFAULT).all(|(a, b)| a == b);
    app.settings.toolbar_items = if is_default { String::new() } else { list.join(",") };
    app.settings.save();
}

/// What is being dragged onto or off the toolbar.
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    /// An item; `from` is its place in the toolbar (None: from the Customize sheet).
    Item { id: String, from: Option<usize> },
    /// The default set strip of the Customize sheet.
    Defaults,
}

pub struct TbState {
    pub drag: Option<Payload>,
    /// Each toolbar entry's place this frame (entry index, rect) and the toolbar's own rect.
    rects: Vec<(usize, Rect)>,
    bar: Rect,
    /// A removed item's puff of smoke: where and when.
    poof: Option<(Pos2, f64)>,
    /// The document title's popover: the name being typed and the folder.
    pub title_edit: Option<(String, Option<std::path::PathBuf>)>,
    /// The popover's Format: 0 Keynote, 1 PowerPoint, 2 PowerPoint + Keydeck.
    pub title_format: u8,
    /// Width of the sidebar(s) under the toolbar's left end: the title starts after it
    /// (Keynote's sidebar tracking separator).
    pub left_w: f32,
    /// How far the canvas's top runs under the floating toolbar.
    pub inset: f32,
}

impl Default for TbState {
    fn default() -> Self {
        TbState { drag: None, rects: Vec::new(), bar: Rect::NOTHING, poof: None, title_edit: None, title_format: 0, left_w: 0.0, inset: 0.0 }
    }
}

/// The toolbar's height for the display mode.
pub fn height(app: &App) -> f32 {
    if app.settings.toolbar_mode == 1 {
        66.0
    } else {
        52.0
    }
}

fn label_font() -> FontId {
    FontId::proportional(11.0)
}

/// Width of an entry (spaces: their minimum).
fn entry_width(ui: &egui::Ui, id: &str, mode: u8, alone: bool) -> f32 {
    // (on top of the 8 px between capsules)
    if id == SPACE {
        return 8.0;
    }
    if id == FLEX {
        return 0.0;
    }
    let text_w = |s: &str, size: f32| ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), FontId::proportional(size), Color32::WHITE).size().x);
    let label = def(id).map(|d| d.label).unwrap_or("");
    match (id, mode) {
        (INSPECTOR, 3) => ["Format", "Animate", "Document"].iter().map(|s| text_w(s, 13.0) + 20.0).sum(),
        (INSPECTOR, 1) => ["Format", "Animate", "Document"].iter().map(|s| (text_w(s, 11.0) + 8.0).max(40.0)).sum(),
        (INSPECTOR, _) => 120.0,
        (ZOOM, _) => 76.0,
        (_, 3) => text_w(label, 13.0) + 20.0,
        (_, 1) => (text_w(label, 11.0) + 8.0).max(if alone { 44.0 } else { 40.0 }),
        _ => {
            if alone {
                44.0
            } else {
                40.0
            }
        }
    }
}

/// Whether an item can be used now (items that don't apply to the selection are greyed).
fn enabled(app: &mut App, id: &str, sc: Option<&kn_model::SlideScene>) -> bool {
    let has_doc = app.doc.is_some();
    let sel = app.sel.len();
    let type_of = |app: &App, i: usize| app.sel.get(i).and_then(|id| app.doc.as_ref().map(|d| d.file.type_name(*id).to_string())).unwrap_or_default();
    let text_ish = has_doc && (sel > 0 || app.canvas.editing.is_some());
    let locked = |want: bool| sc.is_some_and(|sc| sc.drawables.iter().any(|d| app.sel.contains(&d.id) && d.locked == want));
    match id {
        VIEW | INSPECTOR => true,
        "TSKToolbarItemShare" => has_doc,
        "TSKToolbarItemGroup" => has_doc && sel >= 2,
        "TSKToolbarItemUngroup" => has_doc && sel == 1 && type_of(app, 0) == "TSD.GroupArchive",
        "TSKToolbarItemConnect" => has_doc && sel == 2,
        "TSKToolbarItemMask" | "TSKToolbarItemRemoveBackground" | "TSKToolbarItemAdjustImage" => has_doc && sel == 1 && type_of(app, 0) == "TSD.ImageArchive",
        "TSKToolbarItemLock" => has_doc && locked(false),
        "TSKToolbarItemUnlock" => has_doc && locked(true),
        "TSKToolbarItemBigger" | "TSKToolbarItemSmaller" | "TSKToolbarItemSuperscript" | "TSKToolbarItemSubscript" | "TSKToolbarItemOutdent" | "TSKToolbarItemIndent"
        | "TSKToolbarItemLink" => text_ish,
        "TSKToolbarItemForward" | "TSKToolbarItemBackward" | "TSKToolbarItemFront" | "TSKToolbarItemBack" | "TSKToolbarItemFlipHorizontal" | "TSKToolbarItemFlipVertical"
        | "TSKToolbarItemCopyStyle" => has_doc && sel > 0,
        "TSKToolbarItemPasteStyle" => crate::menus::enabled(app, "Paste Style"),
        // an item that runs a menu command is enabled when that command is
        _ => match command(app, id) {
            Some(c) => crate::menus::enabled(app, c),
            None => has_doc,
        },
    }
}

/// The menu command an item runs (items with their own popover have none).
fn command(app: &App, id: &str) -> Option<&'static str> {
    Some(match id {
        "TSKToolbarItemText" => "Text",
        "TSKToolbarItemComment" => "Comment",
        "KNMacToolbarItemRehearse" => "Rehearse Slideshow",
        "TSKToolbarItemObjectList" => "Show Object List",
        "KNMacToolbarItemEditLayouts" => "Edit Slide Layouts",
        "KNMacToolbarItemSkipSlide" => {
            if app.slides.get(app.cur).is_some_and(|s| s.skipped) {
                "Don't Skip Slide"
            } else {
                "Skip Slide"
            }
        }
        "KNMacToolbarItemRecord" => "Record Slideshow...",
        "TSKToolbarItemForward" => "Bring Forward",
        "TSKToolbarItemBackward" => "Send Backward",
        "TSKToolbarItemFront" => "Bring to Front",
        "TSKToolbarItemBack" => "Send to Back",
        "TSKToolbarItemFlipHorizontal" => "Flip Horizontally",
        "TSKToolbarItemFlipVertical" => "Flip Vertically",
        "TSKToolbarItemGroup" => "Group",
        "TSKToolbarItemUngroup" => "Ungroup",
        "TSKToolbarItemBigger" => "Bigger",
        "TSKToolbarItemSmaller" => "Smaller",
        "TSKToolbarItemSuperscript" => "Baseline > Superscript",
        "TSKToolbarItemSubscript" => "Baseline > Subscript",
        "TSKToolbarItemOutdent" => "Decrease Indent Level",
        "TSKToolbarItemIndent" => "Increase Indent Level",
        "TSKToolbarItemConnect" => "Line > Straight Connection Line",
        "TSKToolbarItemCopyStyle" => "Copy Style",
        "TSKToolbarItemPasteStyle" => "Paste Style",
        "TSKToolbarItemGuides" => "Hide Guides",
        "TSKToolbarItemMask" => "Edit Mask",
        "TSKToolbarItemRemoveBackground" => "Remove Background",
        "TSKToolbarItemLock" => "Lock",
        "TSKToolbarItemUnlock" => "Unlock",
        "NSToolbarShowFontsItem" => "Show Fonts",
        "TSKToolbarItemLink" => "Add Link",
        "TSKToolbarItemAdjustImage" => "Show Adjust Image",
        "TSKToolbarItemFind" => "Find…",
        "NSToolbarPrintItem" => "Print…",
        "NSToolbarShowColorsItem" => "Show Colors",
        _ => return None,
    })
}

/// Toggle items shown pressed.
fn pressed(app: &App, id: &str) -> bool {
    match id {
        "TSKToolbarItemObjectList" => app.show_object_list,
        "KNMacToolbarItemEditLayouts" => app.editing_layouts.is_some(),
        _ => false,
    }
}

/// An item's label (some read differently when on).
fn label(app: &App, id: &str) -> &'static str {
    match id {
        "TSKToolbarItemGuides" if !app.show_guides => "Guides",
        _ => def(id).map(|d| d.label).unwrap_or(""),
    }
}

/// Draw one button cell; returns its response.
#[allow(clippy::too_many_arguments)]
fn cell(ui: &mut egui::Ui, rect: Rect, icon_rect: Rect, id_salt: (&str, usize), icon: Icon, text: &str, mode: u8, on: bool, enabled: bool, sense: Sense) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(id_salt), sense);
    let hover = resp.hovered() && enabled;
    let bg = if on { theme::gray(0x5a) } else if hover { theme::gray(0x48) } else { Color32::TRANSPARENT };
    let col = if enabled { theme::gray(0xee) } else { theme::gray(0x70) };
    match mode {
        3 => {
            if bg != Color32::TRANSPARENT {
                ui.painter().rect_filled(icon_rect.shrink(3.0), icon_rect.height() / 2.0, bg);
            }
            crate::theme::centered(ui.painter(), icon_rect.center(), text, FontId::proportional(13.0), col);
        }
        _ => {
            if bg != Color32::TRANSPARENT {
                ui.painter().rect_filled(icon_rect.shrink(2.0), icon_rect.height() / 2.0, bg);
            }
            let s = 22.0_f32.min(icon_rect.height() - 8.0);
            draw(ui.painter(), Rect::from_center_size(icon_rect.center(), vec2(s, s)), icon, col);
            if mode == 1 {
                crate::theme::centered(ui.painter(), pos2(rect.center().x, rect.max.y - 7.0), text, label_font(), if enabled { theme::TEXT_DIM() } else { theme::gray(0x70) });
            }
        }
    }
    resp
}

/// The toolbar.
pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mode = app.settings.toolbar_mode;
    let customizing = app.show_customize_toolbar;
    let list = items(app);
    let bar = ui.available_rect_before_wrap();
    let bar = Rect::from_min_size(bar.min, vec2(bar.width(), height(app) - 8.0));
    app.tb.bar = bar;
    // (no click area behind the buttons: it would catch clicks meant for the sidebar,
    // canvas or inspector under the toolbar's empty parts; right-click a button instead)
    let title = app
        .doc
        .as_ref()
        .and_then(|d| d.path.as_ref())
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .or_else(|| app.fileops.untitled_name.clone())
        .unwrap_or_else(|| "Keydeck".into());
    // the whole name; like the Mac, a long one gives way first (cut short with "…" down to a
    // minimum), and only then do items go into the » menu
    let title_full = ui.fonts_mut(|f| f.layout_no_wrap(title.clone(), FontId::new(14.0, egui::FontFamily::Name("bold".into())), Color32::WHITE).size().x) + 44.0;
    let title_min = title_full.min(140.0);

    // capsules: runs of items between spaces; widths and where the room left goes
    let alone = |i: usize| {
        let prev = i == 0 || is_space(&list[i - 1]);
        let next = i + 1 >= list.len() || is_space(&list[i + 1]);
        prev && next
    };
    let mut widths: Vec<f32> = (0..list.len()).map(|i| entry_width(ui, &list[i], mode, alone(i))).collect();
    // the title sits after the View item (or first)
    let title_at = if list.first().is_some_and(|f| f == VIEW) { 1 } else { 0 };
    let caps_pad = |list: &[String], shown: &[bool]| -> f32 {
        // 4 px either side of every capsule and 8 between neighbouring capsules
        let mut n = 0;
        let mut in_cap = false;
        for (i, id) in list.iter().enumerate() {
            if !shown[i] {
                continue;
            }
            if is_space(id) {
                in_cap = false;
            } else if !in_cap {
                n += 1;
                in_cap = true;
            }
        }
        n as f32 * 16.0
    };
    let mut shown = vec![true; list.len()];
    // everything from the View button on starts right of the sidebar (there are no window
    // buttons in the sidebar here to sit beside, unlike the Mac)
    let skip = app.tb.left_w.max(0.0);
    // (+8: the last capsule needs no gap after it)
    let avail = bar.width() - title_min + 8.0 - skip;
    let overflow_w = 44.0;
    let total = |shown: &[bool], widths: &[f32]| -> f32 { widths.iter().zip(shown).filter(|(_, s)| **s).map(|(w, _)| *w).sum::<f32>() + caps_pad(&list, shown) };
    let mut overflow: Vec<usize> = Vec::new();
    // items that don't fit go into the » menu, from the end (the last item keeps its place)
    while total(&shown, &widths) + if overflow.is_empty() { 0.0 } else { overflow_w } > avail {
        let Some(i) = (0..list.len().saturating_sub(1)).rev().find(|i| shown[*i] && !is_space(&list[*i])) else { break };
        shown[i] = false;
        overflow.insert(0, i);
    }
    let flexes = list.iter().enumerate().filter(|(i, id)| shown[*i] && *id == FLEX).count();
    let mut spare = (avail - total(&shown, &widths) - if overflow.is_empty() { 0.0 } else { overflow_w }).max(0.0);
    // the title takes what it needs of the room left, the flexible spaces the rest
    let title_w = title_full.min(title_min + spare);
    spare -= title_w - title_min;
    for (i, id) in list.iter().enumerate() {
        if id == FLEX && flexes > 0 {
            widths[i] += spare / flexes as f32;
        }
    }

    // lay out and draw
    let sc = app.scene(app.cur);
    let mut x = bar.min.x;
    let cy = if mode == 1 { bar.min.y + 20.0 } else { bar.center().y };
    let cap_h = if mode == 1 { 36.0 } else { 40.0 };
    let mut rects: Vec<(usize, Rect)> = Vec::new();
    let mut in_cap: Option<f32> = None; // capsule start x
    let mut caps: Vec<Rect> = Vec::new();
    let painter_idx = ui.painter().add(egui::Shape::Noop);
    let close_cap = |x: &mut f32, in_cap: &mut Option<f32>, caps: &mut Vec<Rect>| {
        if let Some(x0) = in_cap.take() {
            *x += 4.0;
            caps.push(Rect::from_min_max(pos2(x0, cy - cap_h / 2.0), pos2(*x, cy + cap_h / 2.0)));
            *x += 8.0;
        }
    };
    let mut clicked: Option<String> = None;
    let mut drag_start: Option<usize> = None;
    let ctrl = ui.input(|i| i.modifiers.command);
    for i in 0..=list.len() {
        if i == 0 {
            x += skip;
        }
        if i == title_at {
            close_cap(&mut x, &mut in_cap, &mut caps);
            let tr = Rect::from_min_size(pos2(x, bar.min.y), vec2(title_w, bar.height()));
            let dirty = app.doc.as_ref().is_some_and(|d| d.dirty);
            title_item(app, ui, &ctx, tr, cy, &title, dirty, customizing);
            x += title_w;
        }
        let Some(id) = list.get(i) else { break };
        if !shown[i] {
            continue;
        }
        if is_space(id) {
            close_cap(&mut x, &mut in_cap, &mut caps);
            let r = Rect::from_min_size(pos2(x - 8.0, bar.min.y), vec2(widths[i] + 8.0, bar.height()));
            rects.push((i, r));
            if customizing {
                // spaces show as boxes while customizing
                ui.painter().rect_stroke(r.shrink2(vec2(2.0, 10.0)), 3.0, Stroke::new(1.0, theme::gray(0x60)), egui::StrokeKind::Inside);
            }
            x += widths[i];
            continue;
        }
        if in_cap.is_none() {
            in_cap = Some(x);
            x += 4.0;
        }
        let w = widths[i];
        let r = Rect::from_min_max(pos2(x, bar.min.y), pos2(x + w, bar.max.y));
        let icon_r = Rect::from_center_size(pos2(r.center().x, cy), vec2(w, cap_h - 4.0));
        rects.push((i, r));
        x += w;
        let Some(dd) = def(id) else { continue };
        let on = enabled(app, id, sc.as_ref());
        let sense = if customizing || ctrl { Sense::click_and_drag() } else if on { Sense::click() } else { Sense::hover() };
        match id.as_str() {
            INSPECTOR => {
                let parts = [(InspectorKind::Format, Icon::Format, "Format", "Show or hide format and style options."), (InspectorKind::Animate, Icon::Animate, "Animate", "Show or hide animation options."), (InspectorKind::Document, Icon::Document, "Document", "Show or hide document and audio options.")];
                let pw = w / 3.0;
                for (k, (kind, icon, name, tip)) in parts.into_iter().enumerate() {
                    let pr = Rect::from_min_size(pos2(r.min.x + pw * k as f32, r.min.y), vec2(pw, r.height()));
                    let pi = Rect::from_center_size(pos2(pr.center().x, cy), vec2(pw, cap_h - 4.0));
                    let sel = app.show_inspector && app.inspector == kind;
                    let resp = cell(ui, pr, pi, (id.as_str(), k), icon, name, mode, sel, true, sense);
                    if resp.drag_started() {
                        drag_start = Some(i);
                    } else if resp.clicked() && !customizing {
                        if sel {
                            app.show_inspector = false;
                        } else {
                            app.inspector = kind;
                            app.show_inspector = true;
                        }
                    }
                    if !customizing {
                        egui::Popup::context_menu(&resp).id(resp.id.with("tb-context")).show(|ui| toolbar_menu(app, ui));
                        resp.on_hover_text(tip);
                    }
                }
            }
            ZOOM => {
                let resp = ui.interact(r, ui.scope_id().with(("tb", i)), sense);
                let zr = Rect::from_center_size(pos2(r.center().x, cy), vec2(76.0, cap_h - 4.0));
                if resp.hovered() {
                    ui.painter().rect_filled(zr.shrink(2.0), 16.0, theme::gray(0x48));
                }
                let text = match app.zoom {
                    Zoom::Fit => "Fit".to_string(),
                    Zoom::Percent(p) => format!("{p:.0}%"),
                };
                // centred on the capsule by the same rule as the other toolbar labels
                let font = FontId::proportional(14.5);
                ui.painter().text(pos2(zr.min.x + 14.0, cy + font.size * 0.03), egui::Align2::LEFT_CENTER, text, font, theme::TEXT());
                draw(ui.painter(), Rect::from_center_size(pos2(zr.max.x - 14.0, cy), vec2(12.0, 12.0)), Icon::ChevronDown, theme::TEXT());
                if mode == 1 {
                    crate::theme::centered(ui.painter(), pos2(r.center().x, r.max.y - 7.0), "Zoom", label_font(), theme::TEXT_DIM());
                }
                if resp.drag_started() {
                    drag_start = Some(i);
                } else if !customizing {
                    egui::Popup::menu(&resp).show(|ui| {
                        for p in [25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 300.0, 400.0] {
                            if ui.button(format!("{p:.0}%")).clicked() {
                                app.zoom = Zoom::Percent(p);
                            }
                        }
                        ui.separator();
                        if ui.button("Fit Slide").clicked() {
                            app.zoom = Zoom::Fit;
                        }
                    });
                    egui::Popup::context_menu(&resp).id(resp.id.with("tb-context")).show(|ui| toolbar_menu(app, ui));
                    resp.on_hover_text(dd.tip);
                }
            }
            _ => {
                let resp = cell(ui, r, icon_r, (id.as_str(), i), dd.icon, label(app, id), mode, pressed(app, id), on, sense);
                if resp.drag_started() {
                    drag_start = Some(i);
                } else if !customizing {
                    popup(app, id, &resp);
                    if resp.clicked() {
                        clicked = Some(id.clone());
                    }
                    egui::Popup::context_menu(&resp).id(resp.id.with("tb-context")).show(|ui| toolbar_menu(app, ui));
                    resp.on_hover_text(dd.tip);
                }
            }
        }
        if list.get(i + 1).is_none_or(|n| is_space(n)) {
            close_cap(&mut x, &mut in_cap, &mut caps);
        }
    }
    close_cap(&mut x, &mut in_cap, &mut caps);
    // » : the items that didn't fit
    if !overflow.is_empty() {
        let r = Rect::from_center_size(pos2(bar.max.x - overflow_w / 2.0, cy), vec2(40.0, cap_h - 4.0));
        let resp = ui.interact(r, ui.scope_id().with("tb-overflow"), Sense::click());
        ui.painter().rect_filled(r, r.height() / 2.0, if resp.hovered() { theme::gray(0x48) } else { theme::gray(0x36) });
        draw(ui.painter(), Rect::from_center_size(r.center(), vec2(18.0, 18.0)), Icon::Overflow, theme::TEXT());
        egui::Popup::menu(&resp).show(|ui| {
            for &i in &overflow {
                let id = list[i].clone();
                let on = enabled(app, &id, sc.as_ref());
                if ui.add_enabled(on, egui::Button::new(label(app, &id))).clicked() {
                    clicked = Some(id);
                }
            }
        });
    }
    // glass capsules floating over the canvas: translucent, a soft shadow and a hairline edge
    let light = theme::is_light();
    let glass = theme::tint(if light { Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 215) } else { Color32::from_rgba_unmultiplied(0x3a, 0x3a, 0x3c, 225) });
    let edge = if light { Color32::from_black_alpha(22) } else { Color32::from_white_alpha(22) };
    if crate::glass::enabled() {
        // liquid glass (Settings): under the buttons, in place of the plain capsules
        let ctx2 = ui.ctx().clone();
        let shapes: Vec<egui::Shape> = caps.iter().flat_map(|c| crate::glass::shapes(&ctx2, *c, c.height() / 2.0, true)).collect();
        ui.painter().set(painter_idx, egui::Shape::Vec(shapes));
    } else if crate::glass::classic() {
        // the classic look's flat capsules on the toolbar strip
        ui.painter().set(painter_idx, egui::Shape::Vec(caps.iter().map(|c| egui::Shape::rect_filled(*c, c.height() / 2.0, theme::gray(0x36))).collect()));
    } else {
        let mut shapes = Vec::new();
        for c in &caps {
            let r = c.height() / 2.0;
            shapes.push(egui::Shape::rect_filled(c.expand(1.0), r + 1.0, Color32::from_black_alpha(if light { 16 } else { 60 })));
            shapes.push(egui::Shape::rect_filled(*c, r, glass));
            shapes.push(egui::Shape::rect_stroke(*c, r, Stroke::new(1.0, edge), egui::StrokeKind::Inside));
        }
        ui.painter().set(painter_idx, egui::Shape::Vec(shapes));
    }
    app.tb.rects = rects;
    if let Some(i) = drag_start {
        app.tb.drag = Some(Payload::Item { id: list[i].clone(), from: Some(i) });
    }
    if let Some(id) = clicked {
        run(app, &ctx, &id);
    }
}

/// The document's name in the toolbar (Keynote): click for Name and Where (rename or move the
/// file), right-click for the folders it is in.
#[allow(clippy::too_many_arguments)]
fn title_item(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, tr: Rect, cy: f32, title: &str, dirty: bool, customizing: bool) {
    let font = FontId::new(14.0, egui::FontFamily::Name("bold".into()));
    // cut short with "…" when the room is less than the name
    let mut job = egui::text::LayoutJob::simple_singleline(title.to_string(), font, theme::TEXT());
    job.wrap = egui::text::TextWrapping { max_width: (tr.width() - 44.0).max(20.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.painter().layout_job(job);
    let tw = galley.size().x;
    // the name (and "Edited" under it) centred in the box by their visible letters, the
    // chevron inside the box on the right
    let block_h = if dirty { 34.0 } else { 18.0 };
    let hit = Rect::from_min_size(pos2(tr.min.x + 2.0, cy - (block_h + 14.0) / 2.0), vec2((tw + 36.0).min(tr.width() - 4.0), block_h + 14.0));
    let resp = ui.interact(hit, ui.scope_id().with("doc-title"), if customizing { Sense::hover() } else { Sense::click() });
    let name_y = if dirty { cy - 8.0 } else { cy };
    let cut = galley.rows.first().is_some_and(|r| r.text().ends_with('…')) && !title.ends_with('…');
    let resp = if cut && app.tb.title_edit.is_none() { resp.on_hover_text(title) } else { resp };
    if resp.hovered() {
        ui.painter().rect_filled(hit, 8.0, theme::gray(0x3a));
        draw(ui.painter(), Rect::from_center_size(pos2(hit.max.x - 14.0, name_y + 1.0), vec2(11.0, 11.0)), Icon::ChevronDown, theme::TEXT_DIM());
    }
    let p = ui.painter();
    // the line box sits high for capitals: lower it by the difference
    p.galley(pos2(hit.min.x + 8.0, name_y - galley.size().y / 2.0 + 1.5), galley, theme::TEXT());
    if dirty {
        p.text(pos2(hit.min.x + 8.0, cy + 10.0), Align2::LEFT_CENTER, "Edited", FontId::proportional(11.5), theme::TEXT_DIM());
    }
    let path = app.doc.as_ref().and_then(|d| d.path.clone());
    // right-click: the folders the file is in (choose one to open it)
    if let Some(path) = path.clone() {
        resp.context_menu(|ui| {
            for (k, a) in path.ancestors().enumerate() {
                let name = if k == 0 { a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "/".into()) };
                if name.is_empty() {
                    continue;
                }
                if ui.button(format!("{}{name}", if k == 0 { "" } else { "📁 " })).clicked() {
                    let open = if k == 0 { a.parent().unwrap_or(a) } else { a };
                    let _ = kn_ui::open_command(open).stdin(std::process::Stdio::null()).spawn();
                    ui.close();
                }
            }
        });
    }
    if std::env::var_os("KN_DEBUG").is_some() && (resp.clicked() || resp.is_pointer_button_down_on()) {
        eprintln!("doc title: clicked {} down {} hit {hit:?}", resp.clicked(), resp.is_pointer_button_down_on());
    }
    // click: Name and Where (KN_TITLE_POPOVER: open it by itself, for automated checks)
    let scripted = std::env::var_os("KN_TITLE_POPOVER").is_some() && ctx.cumulative_frame_nr() == 60;
    if resp.clicked() || scripted {
        let stem = path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| title.trim_end_matches(".key").trim_end_matches(".pptx").to_string());
        app.tb.title_format = format_of(app, path.as_deref());
        let dir = path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf()).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Documents")));
        app.tb.title_edit = Some((stem, dir));
    }
    let mut apply = false;
    let mut close = false;
    if let Some((name, dir)) = app.tb.title_edit.as_mut() {
        let area = egui::Area::new(egui::Id::unique("doc-title-popover")).order(egui::Order::Foreground).fixed_pos(pos2(hit.min.x, hit.max.y + 6.0));
        let r = area.show(ctx, |ui| {
            egui::Frame::new().fill(theme::gray(0x2c)).stroke(Stroke::new(1.0, theme::gray(0x48))).corner_radius(10).inner_margin(14).shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(110) }).show(ui, |ui| {
                ui.set_width(300.0);
                egui::Grid::new("doc-title-grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                    ui.label("Name:");
                    let te = ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                    te.request_focus();
                    if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        apply = true;
                    }
                    ui.end_row();
                    ui.label("Where:");
                    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                    let mut places: Vec<std::path::PathBuf> = Vec::new();
                    places.extend(dir.clone());
                    for sub in ["Documents", "Desktop", ""] {
                        if let Some(h) = &home {
                            let p = if sub.is_empty() { h.clone() } else { h.join(sub) };
                            if p.is_dir() && !places.contains(&p) {
                                places.push(p);
                            }
                        }
                    }
                    let show = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
                    egui::ComboBox::from_id_salt("doc-where").width(220.0).selected_text(dir.as_deref().map(show).unwrap_or_default()).show_ui(ui, |ui| {
                        for p in places {
                            if ui.selectable_label(dir.as_ref() == Some(&p), format!("📁 {}", show(&p))).clicked() {
                                *dir = Some(p);
                            }
                        }
                        if ui.button("Other…").clicked() {
                            if let Some(f) = rfd::FileDialog::new().set_title("Where").pick_folder() {
                                *dir = Some(f);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Format:");
                    let names = ["Keynote", "PowerPoint", "PowerPoint + Keydeck"];
                    let f = &mut app.tb.title_format;
                    egui::ComboBox::from_id_salt("doc-format").width(220.0).selected_text(names[usize::from(*f).min(2)]).show_ui(ui, |ui| {
                        for (k, n) in names.iter().enumerate() {
                            ui.selectable_value(f, k as u8, *n);
                        }
                    });
                    ui.end_row();
                });
                let note = match app.tb.title_format {
                    0 => "A Keynote presentation (.key): everything Keydeck and Keynote can do.",
                    1 => "A PowerPoint file (.pptx) for PowerPoint, OnlyOffice, LibreOffice and Google Slides. Builds, transitions and some effects are left out.",
                    _ => "A PowerPoint file that also keeps the whole Keynote presentation inside: other apps open the slides, Keydeck opens everything (unless another app changed the slides).",
                };
                ui.add_space(6.0);
                ui.add(egui::Label::new(egui::RichText::new(note).small().color(theme::TEXT_DIM())).wrap());
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        } else if r.response.clicked_elsewhere() && !resp.clicked() {
            // like Keynote, closing the popover keeps the name typed
            apply = true;
        }
    }
    if apply || close {
        let edit = app.tb.title_edit.take();
        if let (true, Some((name, Some(dir)))) = (apply, edit) {
            let format = app.tb.title_format;
            let ext = if format == 0 { "key" } else { "pptx" };
            let name = name.trim().trim_end_matches(".key").trim_end_matches(".pptx").to_string();
            if !name.is_empty() {
                let to = dir.join(format!("{name}.{ext}"));
                let was = format_of(app, path.as_deref());
                if ext == "pptx" {
                    if format == 2 {
                        app.pptx_hybrid.insert(to.clone());
                    } else {
                        app.pptx_hybrid.remove(&to);
                    }
                }
                match path {
                    // same kind of file: rename / move it
                    Some(from) if from.is_file() && format == was => {
                        if to != from {
                            crate::filemenu::relocate(app, ctx, to);
                        }
                    }
                    // another format: saved as that (the old file stays as it was)
                    Some(from) if from.is_file() => {
                        if to.exists() && to != from {
                            app.notify(format!("“{}” already exists", to.display()));
                        } else {
                            app.save_to(to.clone());
                            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — Keydeck", to.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
                        }
                    }
                    // never saved: save it there under that name
                    _ => app.save_to(to),
                }
            }
        }
    }
}

/// The document's format: 0 Keynote, 1 PowerPoint, 2 PowerPoint + Keydeck.
fn format_of(app: &App, path: Option<&std::path::Path>) -> u8 {
    match path {
        Some(p) if crate::pptx::is_pptx(p) => {
            if app.pptx_hybrid.contains(p) {
                2
            } else {
                1
            }
        }
        _ => 0,
    }
}

/// Items with a popover or menu of their own.
fn popup(app: &mut App, id: &str, resp: &egui::Response) {
    match id {
        VIEW => {
            egui::Popup::menu(resp).show(|ui| crate::menus::view_dropdown(app, ui));
        }
        "KNMacToolbarItemAddSlide" => {
            egui::Popup::menu(resp).show(|ui| app.layout_picker(ui));
        }
        "TSKToolbarItemShape" => {
            egui::Popup::menu(resp).show(|ui| crate::editing::shape_popover(app, ui));
        }
        "TSKToolbarItemMedia" => {
            egui::Popup::menu(resp).show(|ui| {
                for (label, cmd) in [("Choose…", "Choose File"), ("Content Hub", "Browse Content Hub"), ("Photos and Videos", "Show Photo and Video Browser"), ("Music", "Show Music Browser"), ("Record Audio", "Record Audio")] {
                    if ui.button(label).clicked() {
                        app.command(ui.ctx(), cmd);
                    }
                    if label == "Content Hub" {
                        ui.separator();
                    }
                }
            });
        }
        "TSKToolbarItemChart" => {
            // the suite's chart popover: previews of every type in the theme's chart styles
            egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let (ids, cols) = app.doc.as_ref().map(kn_ui::chartpicker::styles).unwrap_or_default();
                if let Some(p) = kn_ui::chartpicker::picker(ui, &cols) {
                    app.command(ui.ctx(), &format!("Chart > {}", p.name));
                    if let (Some(&preset), &[chart]) = (ids.get(p.style), &app.sel[..]) {
                        let slide = app.current_slide().map(|s| s.slide);
                        if let Some(doc) = app.doc.as_mut().filter(|d| d.chart_preset(chart) != Some(preset)) {
                            doc.begin(&mut app.hist, "Chart Style");
                            doc.apply_chart_preset(&mut app.hist, chart, preset);
                            doc.end(&mut app.hist);
                            if let Some(s) = slide {
                                app.bump(s);
                            }
                        }
                    }
                    ui.close();
                }
            });
        }
        // Share (there's no share sheet on Linux): mail a copy, find the file, or export it
        "TSKToolbarItemShare" => {
            egui::Popup::menu(resp).show(|ui| {
                let path = app.doc.as_ref().and_then(|d| d.path.clone());
                if ui.add_enabled(path.is_some(), egui::Button::new("Email a Copy…")).on_disabled_hover_text("Save the presentation first").clicked() {
                    if let Some(p) = &path {
                        let name = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        if std::process::Command::new("xdg-email").arg("--subject").arg(&name).arg("--attach").arg(p).spawn().is_err() {
                            app.notify("No mail app is set up (xdg-email)");
                        }
                    }
                }
                if ui.add_enabled(path.is_some(), egui::Button::new("Show in Files")).clicked() {
                    if let Some(dir) = path.as_ref().and_then(|p| p.parent()) {
                        let _ = kn_ui::open_command(dir).spawn();
                    }
                }
                if ui.add_enabled(path.is_some(), egui::Button::new("Copy File Path")).clicked() {
                    if let Some(p) = &path {
                        ui.ctx().copy_text(p.to_string_lossy().to_string());
                    }
                }
                ui.separator();
                for (label, cmd) in [("Export to PDF…", "PDF…"), ("Export to PowerPoint…", "PowerPoint…"), ("Export to Movie…", "Movie…"), ("Export as Images…", "Images…")] {
                    if ui.button(label).clicked() {
                        app.command(ui.ctx(), cmd);
                    }
                }
            });
        }
        "TSKToolbarItemTable" => {
            egui::Popup::menu(resp).show(|ui| {
                for label in ["Basic", "Headers", "Plain", "Sums"] {
                    if ui.button(label).clicked() {
                        app.command(ui.ctx(), &format!("Table > {label}"));
                    }
                }
            });
        }
        _ => {}
    }
}

/// A click on an item without a popover.
fn run(app: &mut App, ctx: &egui::Context, id: &str) {
    match id {
        "TSKToolbarItemPlay" => app.start_slideshow(app.cur),
        "KNMacToolbarItemPlayInWindow" => {
            let was = app.settings.play_in_window;
            app.settings.play_in_window = true;
            app.start_slideshow(app.cur);
            app.settings.play_in_window = was;
        }
        "TSKToolbarItemGuides" => app.show_guides = !app.show_guides,
        _ => {
            if let Some(cmd) = command(app, id) {
                app.command(ctx, cmd);
            }
        }
    }
}

/// While something is dragged: its image under the pointer, where it would land in the
/// toolbar, and what happens when it is let go. Also the puff of a removed item.
pub fn drag_overlay(app: &mut App, ctx: &egui::Context) {
    if let Some((at, t0)) = app.tb.poof {
        let t = (ctx.input(|i| i.time) - t0) as f32 / 0.35;
        if t >= 1.0 {
            app.tb.poof = None;
        } else {
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("tb-poof")));
            for k in 0..6 {
                let a = k as f32 * std::f32::consts::TAU / 6.0;
                p.circle_filled(at + vec2(a.cos(), a.sin()) * (6.0 + 22.0 * t), 7.0 * (1.0 - t * 0.5), Color32::from_white_alpha(((1.0 - t) * 160.0) as u8));
            }
            ctx.request_repaint();
        }
    }
    let Some(payload) = app.tb.drag.clone() else { return };
    let Some(p) = ctx.pointer_latest_pos() else { return };
    let over = app.tb.bar.expand2(vec2(0.0, 14.0)).contains(p);
    let list = items(app);
    // insertion point: before the first entry whose middle is right of the pointer
    let slot = app.tb.rects.iter().find(|(_, r)| p.x < r.center().x).map(|(i, _)| *i).unwrap_or(list.len());
    let layer = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("tb-drag")));
    let (icon, text) = match &payload {
        Payload::Item { id, .. } => (def(id).map(|d| d.icon).unwrap_or(Icon::Overflow), def(id).map(|d| d.label).unwrap_or("")),
        Payload::Defaults => (Icon::Sidebar, "Default Set"),
    };
    layer.circle_filled(p, 20.0, theme::gray(0x48).gamma_multiply(0.9));
    draw(&layer, Rect::from_center_size(p, vec2(22.0, 22.0)), icon, theme::TEXT());
    crate::theme::centered(&layer, p + vec2(0.0, 30.0), text, label_font(), theme::TEXT());
    if over && payload != Payload::Defaults {
        let x = app.tb.rects.iter().find(|(i, _)| *i == slot).map(|(_, r)| r.min.x).or_else(|| app.tb.rects.last().map(|(_, r)| r.max.x)).unwrap_or(app.tb.bar.min.x);
        layer.vline(x, app.tb.bar.y_range(), Stroke::new(3.0, theme::ACCENT()));
    } else if over {
        layer.rect_stroke(app.tb.bar, 8.0, Stroke::new(2.0, theme::ACCENT()), egui::StrokeKind::Inside);
    }
    ctx.request_repaint();
    if !ctx.input(|i| i.pointer.any_released()) {
        return;
    }
    app.tb.drag = None;
    let mut list = list;
    match payload {
        Payload::Defaults if over => {
            list = DEFAULT.iter().map(|s| s.to_string()).collect();
        }
        Payload::Defaults => return,
        Payload::Item { id, from } => {
            if over {
                // an item already in the toolbar moves rather than appearing twice
                let from = from.or_else(|| (!is_space(&id)).then(|| list.iter().position(|x| *x == id)).flatten());
                let mut at = slot.min(list.len());
                if let Some(f) = from {
                    list.remove(f);
                    if at > f {
                        at -= 1;
                    }
                }
                list.insert(at.min(list.len()), id);
            } else if let Some(f) = from {
                // dragged off the toolbar: gone, in a puff of smoke
                list.remove(f);
                app.tb.poof = Some((p, ctx.input(|i| i.time)));
            } else {
                return;
            }
        }
    }
    set_items(app, &list);
}

/// View > Customize Toolbar…: the sheet that drops down from the toolbar.
pub fn sheet(app: &mut App, ctx: &egui::Context) {
    if !app.show_customize_toolbar {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.show_customize_toolbar = false;
        return;
    }
    let in_bar = items(app);
    let screen = ctx.content_rect();
    let w = (app.tb.bar.width() - 40.0).clamp(600.0, 1000.0).min(screen.width() - 40.0);
    let top = pos2(app.tb.bar.center().x, app.tb.bar.max.y + 6.0);
    egui::Area::new(egui::Id::unique("customize-toolbar")).order(egui::Order::Foreground).fixed_pos(top - vec2(w / 2.0, 0.0)).show(ctx, |ui| {
        egui::Frame::new().fill(theme::gray(0x2a)).corner_radius(14).stroke(Stroke::new(1.0, theme::gray(0x44))).shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(120) }).inner_margin(20).show(ui, |ui| {
            ui.set_width(w - 40.0);
            ui.label(egui::RichText::new("Drag your favorite items into the toolbar…").size(13.5).color(theme::TEXT()));
            ui.add_space(10.0);
            let cols = ((w - 40.0) / 104.0).floor().max(4.0) as usize;
            let cell_w = (w - 40.0) / cols as f32;
            // the grid scrolls when the window is too short for all of it
            let grid_h = (screen.max.y - top.y - 300.0).max(160.0);
            egui::ScrollArea::vertical().max_height(grid_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 6.0);
                for dd in ITEMS.iter() {
                    let span = if dd.id == INSPECTOR { 2.0 } else { 1.0 };
                    let (r, resp) = ui.allocate_exact_size(vec2(cell_w * span, 74.0), Sense::drag());
                    let used = !is_space(dd.id) && in_bar.iter().any(|x| x == dd.id);
                    let dim = if used { 0.35 } else { 1.0 };
                    let p = ui.painter();
                    let ic = pos2(r.center().x, r.min.y + 24.0);
                    let label_col = theme::TEXT().gamma_multiply(dim);
                    match dd.id {
                        SPACE => {
                            p.rect_stroke(Rect::from_center_size(ic, vec2(30.0, 30.0)), 2.0, Stroke::new(1.0, theme::gray(0x80).gamma_multiply(dim)), egui::StrokeKind::Inside);
                        }
                        FLEX => {
                            let b = Rect::from_center_size(ic, vec2(52.0, 30.0));
                            p.rect_stroke(b, 2.0, Stroke::new(1.0, theme::gray(0x80)), egui::StrokeKind::Inside);
                            crate::theme::centered(&p, ic, "←···→", FontId::proportional(10.0), theme::gray(0xa0));
                        }
                        INSPECTOR => {
                            let b = Rect::from_center_size(ic, vec2(96.0, 32.0));
                            p.rect_filled(b, 16.0, theme::gray(0x40).gamma_multiply(dim));
                            for (n, icon) in [Icon::Format, Icon::Animate, Icon::Document].into_iter().enumerate() {
                                draw(p, Rect::from_center_size(pos2(b.min.x + 16.0 + n as f32 * 32.0, ic.y), vec2(18.0, 18.0)), icon, theme::gray(0xee).gamma_multiply(dim));
                            }
                        }
                        _ => {
                            p.circle_filled(ic, 20.0, theme::gray(0x40).gamma_multiply(dim));
                            draw(p, Rect::from_center_size(ic, vec2(22.0, 22.0)), dd.icon, theme::gray(0xee).gamma_multiply(dim));
                        }
                    }
                    let text = if dd.id == INSPECTOR { "Format  Animate  Document" } else { dd.label };
                    crate::theme::centered(&p, pos2(r.center().x, r.min.y + 56.0), text, FontId::proportional(12.0), label_col);
                    if resp.drag_started() {
                        app.tb.drag = Some(Payload::Item { id: dd.id.to_string(), from: None });
                    }
                    if !dd.tip.is_empty() {
                        resp.on_hover_text(dd.tip);
                    }
                }
            });
            });
            ui.add_space(14.0);
            ui.label(egui::RichText::new("… or drag the default set into the toolbar.").size(13.5).color(theme::TEXT()));
            ui.add_space(8.0);
            // the default set as one strip
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::drag());
            let p = ui.painter();
            p.rect_filled(r, 10.0, theme::gray(0x33));
            let shown: Vec<&str> = DEFAULT.iter().copied().filter(|id| !is_space(id)).collect();
            let step = (r.width() - 40.0) / shown.len() as f32;
            for (n, id) in shown.iter().enumerate() {
                let c = pos2(r.min.x + 20.0 + step * (n as f32 + 0.5), r.min.y + 22.0);
                let dd = def(id).unwrap();
                draw(p, Rect::from_center_size(c, vec2(20.0, 20.0)), if *id == ZOOM { Icon::ChevronDown } else { dd.icon }, theme::gray(0xee));
                crate::theme::centered(&p, c + vec2(0.0, 25.0), dd.label, FontId::proportional(11.0), theme::TEXT_DIM());
            }
            if resp.drag_started() {
                app.tb.drag = Some(Payload::Defaults);
            }
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Show");
                let names = ["Icon and Text", "Icon Only", "Text Only"];
                let mut m = app.settings.toolbar_mode;
                egui::ComboBox::from_id_salt("tb-show").selected_text(names[(m as usize).clamp(1, 3) - 1]).show_ui(ui, |ui| {
                    for (k, name) in names.iter().enumerate() {
                        ui.selectable_value(&mut m, k as u8 + 1, *name);
                    }
                });
                if m != app.settings.toolbar_mode {
                    app.settings.toolbar_mode = m;
                    app.settings.save();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Done").color(Color32::WHITE)).fill(theme::ACCENT()).min_size(vec2(80.0, 26.0))).clicked() {
                        app.show_customize_toolbar = false;
                    }
                });
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_have_unique_ids_and_defaults_exist() {
        let mut ids: Vec<&str> = ITEMS.iter().map(|d| d.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(DEFAULT.iter().all(|id| def(id).is_some()));
    }
}

/// A toolbar button's context menu: display mode and Customize Toolbar…
fn toolbar_menu(app: &mut App, ui: &mut egui::Ui) {
    for (m, name) in [(1u8, "Icon and Text"), (2, "Icon Only"), (3, "Text Only")] {
        if ui.radio(app.settings.toolbar_mode == m, name).clicked() {
            app.settings.toolbar_mode = m;
            app.settings.save();
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Customize Toolbar…").clicked() {
        app.show_customize_toolbar = true;
        ui.close();
    }
}
