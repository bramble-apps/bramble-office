//! The window toolbar and View > Customize Toolbar… (Pages 15.4; Keydeck's toolbar.rs with
//! Pages' items): the toolbar holds a list of items chosen by the user (saved in the
//! settings, not in the document), shown as icons, icons with labels or labels only. Items
//! next to each other share a capsule; spaces separate capsules and flexible spaces take the
//! room left over. While the Customize sheet is open, items are dragged in from it, moved along
//! the toolbar or dragged off it; otherwise Ctrl-drag (Numbers' ⌘-drag) moves or removes them.

use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};
use kn_ui::icons::{draw, Icon};
use kn_ui::theme;

use crate::app::{App, InspMode};

pub const VIEW: &str = "View";
pub const ZOOM: &str = "Zoom";
pub const SPACE: &str = "Space";
pub const FLEX: &str = "FlexibleSpace";

/// One toolbar item: our identifier (saved in the settings), label, tooltip, icon and the menu
/// command it runs (None: a popover or an action of its own).
pub struct Def {
    pub id: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
    pub icon: Icon,
    pub command: Option<&'static str>,
}

const fn d(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, command: Option<&'static str>) -> Def {
    Def { id, label, tip, icon, command }
}

/// Every item, in the Customize sheet's order (Pages 15.4's palette, without its Apple
/// services: Collaborate, Writing Tools and Apple Intelligence).
pub const ITEMS: &[Def] = &[
    d(VIEW, "View", "Choose what to show or hide while creating a document.", Icon::Sidebar, None),
    d(ZOOM, "Zoom", "Click to enlarge or reduce the view of the document.", Icon::ChevronDown, None),
    d("AddPage", "Add Page", "Add a page to this document.", Icon::AddSlide, None),
    d("Insert", "Insert", "Choose an item to add to this document.", Icon::AddSlide, None),
    d("Table", "Table", "Choose a table to add to this document.", Icon::Table, None),
    d("Chart", "Chart", "Choose a chart to add to this document.", Icon::Chart, None),
    d("Text", "Text", "Add a text box to this document.", Icon::Text, None),
    d("Shape", "Shape", "Choose a shape or line to add to this document.", Icon::Shape, None),
    d("Media", "Media", "Choose an image to add to this document.", Icon::Media, None),
    d("Comment", "Comment", "Add a comment to the selected text.", Icon::Comment, None),
    d("Share", "Share", "Share this document (not available on Linux).", Icon::Share, None),
    d("Format", "Format", "Show or hide format and style options.", Icon::Format, None),
    d("Document", "Document", "Show or hide document setup options.", Icon::Document, None),
    d("TrackChanges", "Track Changes", "Turn change tracking on or off.", Icon::Comment, Some("Track Changes")),
    d("Forward", "Forward", "Move selected objects forward one layer.", Icon::LayerForward, Some("Bring Forward")),
    d("Backward", "Backward", "Move selected objects backward one layer.", Icon::LayerBackward, Some("Send Backward")),
    d("Front", "Front", "Move selected objects to the front layer.", Icon::LayerFront, Some("Bring to Front")),
    d("Back", "Back", "Move selected objects to the back layer.", Icon::LayerBack, Some("Send to Back")),
    d("FlipHorizontal", "Flip Horizontal", "Flip the object horizontally.", Icon::FlipH, Some("Flip Horizontally")),
    d("FlipVertical", "Flip Vertical", "Flip the object vertically.", Icon::FlipV, Some("Flip Vertically")),
    d("Group", "Group", "Combine the selected objects into a single object.", Icon::Group, Some("Group")),
    d("Ungroup", "Ungroup", "Ungroup selected objects.", Icon::Ungroup, Some("Ungroup")),
    d("Bigger", "Bigger", "Make selected text one size larger.", Icon::Bigger, Some("Font > Bigger")),
    d("Smaller", "Smaller", "Make selected text one size smaller.", Icon::Smaller, Some("Font > Smaller")),
    d("Superscript", "Superscript", "Make selected text a superscript.", Icon::Superscript, Some("Baseline > Superscript")),
    d("Subscript", "Subscript", "Make selected text a subscript.", Icon::Subscript, Some("Baseline > Subscript")),
    d("CopyStyle", "Copy Style", "Copy the attributes of the selected text or object.", Icon::CopyStyle, Some("Copy Style")),
    d("PasteStyle", "Paste Style", "Paste the copied attributes to the selected text or object.", Icon::PasteStyle, Some("Paste Style")),
    d("Guides", "Guides", "Hide alignment guides.", Icon::Guides, Some("Guides > Hide Guides")),
    d("Mask", "Mask", "Change how much of the image is visible.", Icon::Mask, Some("Image > Edit Mask")),
    d("Lock", "Lock", "Lock selected objects to prevent editing.", Icon::Lock, Some("Lock")),
    d("Unlock", "Unlock", "Unlock selected objects to allow editing.", Icon::Unlock, Some("Unlock")),
    d("Fonts", "Fonts", "Show or hide the Font window.", Icon::Fonts, Some("Font > Show Fonts")),
    d("Link", "Link", "Add a link to a webpage to the selected text or object.", Icon::Link, Some("Add Link > Webpage")),
    d("Find", "Find", "Show Find & Replace.", Icon::Search, Some("Find > Find…")),
    d("WordCount", "Word Count", "Show the word count.", Icon::List, Some("Show Word Count")),
    d("Print", "Print", "Print the document.", Icon::Print, Some("Print…")),
    d("Colors", "Colors", "Show the Colors window.", Icon::Palette, Some("Show Colors")),
    d(SPACE, "Space", "", Icon::Overflow, None),
    d(FLEX, "Flexible Space", "", Icon::Overflow, None),
];

/// Pages' default set (UI-GEOMETRY: View, title, Add Page, Insert … Media, Zoom, Comment, Share,
/// Format | Document).
pub const DEFAULT: &[&str] = &[VIEW, FLEX, "AddPage", "Insert", SPACE, "Table", "Chart", "Text", "Shape", "Media", SPACE, ZOOM, SPACE, "Comment", SPACE, "Share", SPACE, "Format", "Document"];

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
    /// View > Customize Toolbar… is open.
    pub customizing: bool,
    /// Each toolbar entry's place this frame (entry index, rect) and the toolbar's own rect.
    rects: Vec<(usize, Rect)>,
    bar: Rect,
    /// A removed item's puff of smoke: where and when.
    poof: Option<(Pos2, f64)>,
}

impl Default for TbState {
    fn default() -> Self {
        TbState { drag: None, customizing: false, rects: Vec::new(), bar: Rect::NOTHING, poof: None }
    }
}

/// The toolbar's height for the display mode (labels under the icons need more room).
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

fn centered(p: &egui::Painter, at: Pos2, text: &str, font: FontId, col: Color32) {
    p.text(at, Align2::CENTER_CENTER, text, font, col);
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
        (ZOOM, _) => 76.0,
        (_, 3) => text_w(label, 13.0) + 20.0,
        (_, 1) => (text_w(label, 11.0) + 8.0).max(if alone { 44.0 } else { 40.0 }),
        _ if alone => 44.0,
        _ => 40.0,
    }
}

/// Whether an item can be used now (items that don't apply to the selection are greyed).
fn enabled(app: &App, id: &str) -> bool {
    match id {
        "Share" => false,
        "Comment" => crate::menus::enabled(app, "Comment"),
        "AddPage" => app.wb.is_some(),
        VIEW => true,
        _ => match def(id).and_then(|d| d.command) {
            Some(c) => crate::menus::enabled(app, c),
            None => app.wb.is_some(),
        },
    }
}

fn pressed(app: &App, id: &str) -> bool {
    match id {
        "Format" => app.inspector && app.insp_mode == InspMode::Format,
        "Document" => app.inspector && app.insp_mode == InspMode::Document,
        "TrackChanges" => app.wb.as_ref().is_some_and(|w| w.tracking()),
        _ => false,
    }
}

/// Draw one button cell; returns its response.
#[allow(clippy::too_many_arguments)]
fn cell(ui: &mut egui::Ui, rect: Rect, icon_rect: Rect, salt: (&str, usize), icon: Icon, text: &str, mode: u8, on: bool, enabled: bool, sense: Sense) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(salt), sense);
    let hover = resp.hovered() && enabled;
    let bg = if on { theme::gray(0x5a) } else if hover { theme::gray(0x48) } else { Color32::TRANSPARENT };
    let col = if enabled { theme::gray(0xee) } else { theme::gray(0x70) };
    if mode == 3 {
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(icon_rect.shrink(3.0), icon_rect.height() / 2.0, bg);
        }
        centered(ui.painter(), icon_rect.center(), text, FontId::proportional(13.0), col);
    } else {
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(icon_rect.shrink(2.0), icon_rect.height() / 2.0, bg);
        }
        let s = 22.0_f32.min(icon_rect.height() - 8.0);
        let ir = Rect::from_center_size(icon_rect.center(), vec2(s, s));
        if !crate::tbicons::draw(ui.painter(), ir, salt.0, col) {
            draw(ui.painter(), ir, icon, col);
        }
        if mode == 1 {
            centered(ui.painter(), pos2(rect.center().x, rect.max.y - 7.0), text, label_font(), if enabled { theme::TEXT_DIM() } else { theme::gray(0x70) });
        }
    }
    resp
}

/// Items with a popover or menu of their own.
fn popup(app: &mut App, id: &str, resp: &egui::Response) {
    match id {
        VIEW => {
            egui::Popup::menu(resp).show(|ui| crate::chrome::view_menu(app, ui));
        }
        "Insert" => {
            egui::Popup::menu(resp).show(|ui| crate::chrome::insert_menu(app, ui));
        }
        "Table" => {
            egui::Popup::menu(resp).show(|ui| crate::chrome::table_popover(app, ui));
        }
        "Chart" => {
            // the suite's chart popover: previews of every type in the document's chart styles
            egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let ctx = ui.ctx().clone();
                let (ids, cols) = app.doc().map(kn_ui::chartpicker::styles).unwrap_or_default();
                if let Some(p) = kn_ui::chartpicker::picker(ui, &cols) {
                    crate::menus::run(app, &ctx, &format!("Chart > {}", p.name));
                    if let (Some(&preset), &[chart]) = (ids.get(p.style), &app.selected_objs()[..]) {
                        if app.doc().and_then(|d| d.chart_preset(chart)) != Some(preset) {
                            app.edit("Chart Style", |d, h| d.apply_chart_preset(h, chart, preset));
                        }
                    }
                    ui.close();
                }
            });
        }
        "Shape" => {
            egui::Popup::menu(resp).show(|ui| crate::chrome::shape_popover(app, ui));
        }
        "Media" => {
            // Pages' Media ▾ (its Apple Intelligence and iPhone camera items aside)
            egui::Popup::menu(resp).show(|ui| {
                let ctx = ui.ctx().clone();
                for (k, label) in ["Choose Photo or Video…", "Content Hub", "Choose Music…", "Add Image Gallery…", "Add Web Video…", "Record Audio", "Choose File…"].iter().enumerate() {
                    if k == 6 {
                        ui.separator();
                    }
                    if ui.button(*label).clicked() {
                        ui.close();
                        crate::media::command(app, label);
                        let _ = &ctx;
                    }
                }
            });
        }
        _ => {}
    }
}

fn has_popup(id: &str) -> bool {
    matches!(id, VIEW | "Insert" | "Table" | "Chart" | "Shape" | "Media")
}

/// A click on an item without a popover.
fn run(app: &mut App, ctx: &egui::Context, id: &str) {
    match id {
        "Text" => crate::insert::insert_object(app, pg_core::objects::NewObject::TextBox),
        "Comment" => crate::menus::run(app, ctx, "Comment"),
        "Format" => app.toggle_inspector(InspMode::Format),
        "Document" => app.toggle_inspector(InspMode::Document),
        "AddPage" => add_page(app),
        _ => {
            if let Some(c) = def(id).and_then(|d| d.command) {
                crate::menus::run(app, ctx, c);
            }
        }
    }
}

/// The toolbar in `bar`.
pub fn toolbar(app: &mut App, ui: &mut egui::Ui, bar: Rect) {
    let ctx = ui.ctx().clone();
    let mode = app.settings.toolbar_mode;
    let customizing = app.tb.customizing;
    let list = items(app);
    app.tb.bar = bar;
    // (no click area behind the buttons: it would catch clicks meant for the sidebar,
    // canvas or inspector under the toolbar's empty parts; right-click a button instead)
    let font = FontId::new(15.0, egui::FontFamily::Name("bold".into()));
    let title = app.title();
    // the whole name; a long one gives way first (cut short with "…" down to a minimum), and
    // only then do items go into the » menu
    let title_full = ui.fonts_mut(|f| f.layout_no_wrap(title.clone(), font.clone(), Color32::WHITE).size().x) + 44.0;
    let title_min = title_full.min(140.0);

    // capsules: runs of items between spaces; widths and where the room left goes
    let alone = |i: usize| (i == 0 || is_space(&list[i - 1])) && (i + 1 >= list.len() || is_space(&list[i + 1]));
    let mut widths: Vec<f32> = (0..list.len()).map(|i| entry_width(ui, &list[i], mode, alone(i))).collect();
    // the title sits after the View item (or first)
    let title_at = if list.first().is_some_and(|f| f == VIEW) { 1 } else { 0 };
    let caps_pad = |shown: &[bool]| -> f32 {
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
    let avail = bar.width() - title_min + 8.0;
    let overflow_w = 44.0;
    let total = |shown: &[bool], widths: &[f32]| -> f32 { widths.iter().zip(shown).filter(|(_, s)| **s).map(|(w, _)| *w).sum::<f32>() + caps_pad(shown) };
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
    let mut x = bar.min.x;
    let cy = if mode == 1 { bar.min.y + 20.0 } else { bar.center().y };
    let cap_h = if mode == 1 { 36.0 } else { 40.0 };
    let mut rects: Vec<(usize, Rect)> = Vec::new();
    let mut in_cap: Option<f32> = None;
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
        if i == title_at {
            close_cap(&mut x, &mut in_cap, &mut caps);
            let tr = Rect::from_min_size(pos2(x + 8.0, bar.min.y), vec2(title_w - 16.0, bar.height()));
            let mut name = title.clone();
            let fits = |s: &str, ui: &egui::Ui| ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x) <= tr.width() - 28.0;
            while !fits(&name, ui) && name.chars().count() > 2 {
                let n = name.chars().count();
                name = name.chars().take(n.saturating_sub(2)).collect::<String>() + "…";
                name = name.replacen("……", "…", 1);
            }
            let dirty = app.doc().is_some_and(|d| d.dirty);
            if !customizing {
                crate::chrome::title_item(app, ui, tr, cy, &name, font.clone(), dirty);
            } else {
                ui.painter().text(pos2(tr.min.x, cy), Align2::LEFT_CENTER, &name, font.clone(), theme::TEXT_DIM());
            }
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
                // spaces show as boxes while customizing, and can be dragged
                ui.painter().rect_stroke(r.shrink2(vec2(2.0, 10.0)), 3.0, Stroke::new(1.0, theme::gray(0x60)), egui::StrokeKind::Inside);
                if ui.interact(r, ui.scope_id().with(("tb-space", i)), Sense::drag()).drag_started() {
                    drag_start = Some(i);
                }
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
        let on = enabled(app, id);
        let sense = if customizing || ctrl { Sense::click_and_drag() } else if on { Sense::click() } else { Sense::hover() };
        if id == ZOOM {
            let resp = ui.interact(r, ui.scope_id().with(("tb", i)), sense);
            let zr = Rect::from_center_size(pos2(r.center().x, cy), vec2(76.0, cap_h - 4.0));
            if resp.hovered() {
                ui.painter().rect_filled(zr.shrink(2.0), 16.0, theme::gray(0x48));
            }
            // "100% ⌄" in bold, the chevron right after the number, centred together (Mac N2)
            let f = FontId::new(14.5, egui::FontFamily::Name("bold".into()));
            let g = ui.painter().layout_no_wrap(format!("{:.0}%", app.canvas.zoom * 100.0), f, theme::TEXT());
            let total = g.size().x + 4.0 + 10.0;
            let x0 = zr.center().x - total / 2.0;
            ui.painter().galley(pos2(x0, cy - g.size().y / 2.0), g, theme::TEXT());
            draw(ui.painter(), Rect::from_center_size(pos2(x0 + total - 5.0, cy + 1.0), vec2(10.0, 10.0)), Icon::ChevronDown, theme::TEXT());
            if mode == 1 {
                centered(ui.painter(), pos2(r.center().x, r.max.y - 7.0), "Zoom", label_font(), theme::TEXT_DIM());
            }
            if resp.drag_started() {
                drag_start = Some(i);
            } else if !customizing {
                egui::Popup::menu(&resp).show(|ui| crate::chrome::zoom_menu(app, ui));
                egui::Popup::context_menu(&resp).id(resp.id.with("tb-context")).show(|ui| toolbar_menu(app, ui));
                resp.on_hover_text(dd.tip);
            }
        } else {
            let resp = cell(ui, r, icon_r, (id.as_str(), i), dd.icon, dd.label, mode, pressed(app, id), on, sense);
            if resp.drag_started() {
                drag_start = Some(i);
            } else if !customizing {
                if has_popup(id) {
                    popup(app, id, &resp);
                } else if resp.clicked() {
                    clicked = Some(id.clone());
                }
                egui::Popup::context_menu(&resp).id(resp.id.with("tb-context")).show(|ui| toolbar_menu(app, ui));
                resp.on_hover_text(dd.tip);
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
                let on = enabled(app, &id) && !has_popup(&id);
                if ui.add_enabled(on, egui::Button::new(def(&id).map(|d| d.label).unwrap_or(""))).clicked() {
                    clicked = Some(id);
                }
            }
        });
    }
    crate::chrome::paint_capsules(ui, painter_idx, &caps);
    app.tb.rects = rects;
    if let Some(i) = drag_start {
        app.tb.drag = Some(Payload::Item { id: list[i].clone(), from: Some(i) });
    }
    if let Some(id) = clicked {
        run(app, &ctx, &id);
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
    centered(&layer, p + vec2(0.0, 30.0), text, label_font(), theme::TEXT());
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
        Payload::Defaults if over => list = DEFAULT.iter().map(|s| s.to_string()).collect(),
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
    if !app.tb.customizing {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.tb.customizing = false;
        return;
    }
    let in_bar = items(app);
    let screen = ctx.content_rect();
    let bar = if app.tb.bar.is_positive() { app.tb.bar } else { Rect::from_min_size(pos2(screen.min.x + 10.0, screen.min.y + 50.0), vec2(screen.width() - 20.0, 44.0)) };
    let w = (bar.width() - 40.0).clamp(600.0, 1000.0).min(screen.width() - 40.0);
    let top = pos2(bar.center().x, bar.max.y + 6.0);
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
                        let (r, resp) = ui.allocate_exact_size(vec2(cell_w, 74.0), Sense::drag());
                        let used = !is_space(dd.id) && in_bar.iter().any(|x| x == dd.id);
                        let dim = if used { 0.35 } else { 1.0 };
                        let p = ui.painter();
                        let ic = pos2(r.center().x, r.min.y + 24.0);
                        match dd.id {
                            SPACE => {
                                p.rect_stroke(Rect::from_center_size(ic, vec2(30.0, 30.0)), 2.0, Stroke::new(1.0, theme::gray(0x80).gamma_multiply(dim)), egui::StrokeKind::Inside);
                            }
                            FLEX => {
                                p.rect_stroke(Rect::from_center_size(ic, vec2(52.0, 30.0)), 2.0, Stroke::new(1.0, theme::gray(0x80)), egui::StrokeKind::Inside);
                                centered(p, ic, "←···→", FontId::proportional(10.0), theme::gray(0xa0));
                            }
                            ZOOM => {
                                let b = Rect::from_center_size(ic, vec2(64.0, 30.0));
                                p.rect_filled(b, 15.0, theme::gray(0x40).gamma_multiply(dim));
                                centered(p, ic, "100%", FontId::proportional(12.5), theme::gray(0xee).gamma_multiply(dim));
                            }
                            _ => {
                                p.circle_filled(ic, 20.0, crate::look::capsule_fill().gamma_multiply(dim));
                                let ir = Rect::from_center_size(ic, vec2(22.0, 22.0));
                                if !crate::tbicons::draw(p, ir, dd.id, theme::gray(0xee).gamma_multiply(dim)) {
                                    draw(p, ir, dd.icon, theme::gray(0xee).gamma_multiply(dim));
                                }
                            }
                        }
                        centered(p, pos2(r.center().x, r.min.y + 56.0), dd.label, FontId::proportional(12.0), theme::TEXT().gamma_multiply(dim));
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
                let Some(dd) = def(id) else { continue };
                let ir = Rect::from_center_size(c, vec2(20.0, 20.0));
                if !crate::tbicons::draw(p, ir, dd.id, theme::gray(0xee)) {
                    draw(p, ir, dd.icon, theme::gray(0xee));
                }
                centered(p, c + vec2(0.0, 25.0), dd.label, FontId::proportional(11.0), theme::TEXT_DIM());
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
                        app.tb.customizing = false;
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
        // every command an item runs is a menu command
        let bad: Vec<&str> = ITEMS.iter().filter_map(|d| d.command).filter(|c| !crate::menus::is_command(c)).collect();
        assert!(bad.is_empty(), "{bad:?}");
    }
}

/// The toolbar's Add Page: a page break at the end of a word-processing document (a new blank
/// page, the caret on it), a new page after the current one in page layout.
pub fn add_page(app: &mut App) {
    let wp = app.wb.as_ref().is_some_and(|w| w.is_word_processing());
    if wp {
        let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) else { return };
        let n = app.doc().map(|d| d.storage_text(st).len()).unwrap_or(0);
        app.cursor = Some(crate::app::Cursor { storage: st, owner: crate::app::Owner::Body, anchor: n, caret: n, goal_x: None });
        crate::text::replace_selection(app, "\u{5}", "Add Page", false);
        app.canvas.follow_caret = true;
    } else {
        let at = app.current_page();
        app.edit_pages("Add Page", |p, h| p.add_layout_page(h, at));
        app.refresh();
        app.canvas.reveal_page(at + 1);
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
        app.tb.customizing = true;
        ui.close();
    }
}
