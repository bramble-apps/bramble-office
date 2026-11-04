//! The toolbar (Notes 4.13, screenshots/toolbar-default.png) and View > Customize Toolbar…
//! (toolbar-customize-sheet.png), on Letters' toolbar code: a list of items chosen by the user
//! (saved in the settings), items next to each other sharing a capsule, spaces separating them
//! and flexible spaces taking the room left over. Notes' toolbar is split at the columns: the
//! items before the folder's title sit over the sidebar, the title and View Options over the
//! list, and the rest (after the tracking separator) over the editor. Gallery view keeps only
//! the items that work without an open note.

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};

use crate::app::App;
use crate::icons::{draw, Glyph};
use crate::look;

pub const HEIGHT: f32 = 52.0;
/// Apple Notes' toolbar buttons (measured on ui-list-view-light): capsules 36.5 pt tall, items
/// 36.5 pt wide, 4.5 pt inside a group's ends; a button on its own is a circle as wide as it's
/// tall; 8 pt between capsules.
const CAP_H: f32 = kn_ui::capsule::HEIGHT;
const ITEM_W: f32 = 36.5;
const CAP_PAD: f32 = kn_ui::capsule::PAD;
const SOLO_W: f32 = kn_ui::capsule::SOLO_W;
const GAP: f32 = 8.0;

pub const TITLE: &str = "Title";
pub const TRACK: &str = "Separator";
pub const SPACE: &str = "Space";
pub const FLEX: &str = "FlexibleSpace";
pub const SEARCH: &str = "Search";
const GAP_ID: &str = "Gap";

/// One toolbar item: our identifier (saved in the settings), label, tooltip, glyph, the menu
/// command it runs (None: a popover or an action of its own).
pub struct Def {
    pub id: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
    pub glyph: Glyph,
    pub command: Option<&'static str>,
}

const fn d(id: &'static str, label: &'static str, tip: &'static str, glyph: Glyph, command: Option<&'static str>) -> Def {
    Def { id, label, tip, glyph, command }
}

/// Every item, in the Customize sheet's order (Notes 4.13's palette).
pub const ITEMS: &[Def] = &[
    d("AddFolder", "Add Folder", "Create a new folder.", Glyph::FolderPlus, Some("New Folder")),
    d("Folders", "Folders", "Show or hide the sidebar.", Glyph::Sidebar, Some("Hide Sidebar")),
    d("Attachments", "Attachments", "Browse attachments.", Glyph::Attachments, Some("Show Attachments Browser")),
    d("NewNote", "New Note", "Create a note.", Glyph::Compose, Some("New Note")),
    d("Format", "Format", "Format text.", Glyph::Format, None),
    d("Checklist", "Checklist", "Make a checklist.", Glyph::Checklist, Some("Checklist")),
    d("Table", "Table", "Add a table.", Glyph::Table, Some("Table")),
    d("Link", "Link", "Add a link.", Glyph::Link, Some("Add Link…")),
    d("Media", "Media", "Add a photo, video or file.", Glyph::Paperclip, None),
    d("Markup", "Markup", "Mark up the note.", Glyph::Markup, Some("Markup")),
    d("Share", "Share this note", "Share this note.", Glyph::Share, None),
    d("Actions", "Actions", "More actions for this note.", Glyph::More, None),
    d(SEARCH, "Search", "Search all notes.", Glyph::Search, None),
    d(SPACE, "Space", "", Glyph::More, None),
    d(FLEX, "Flexible Space", "", Glyph::More, None),
    d("ViewOptions", "View Options", "Change how notes are shown.", Glyph::More, None),
];

/// The toolbar as Notes opens (toolbar-default.png): Add Folder, Folders | title, View Options ‖
/// New Note, ⟷, Format … Markup, ⟷, Share, Actions, Search (without Apple Intelligence's Writing
/// Tools).
pub const DEFAULT: &[&str] = &["AddFolder", "Folders", TITLE, "ViewOptions", TRACK, "NewNote", FLEX, "Format", "Checklist", "Table", "Media", "Markup", FLEX, "Share", "Actions", SEARCH];

/// The default set as the Customize sheet draws it (its strip, toolbar-customize-sheet.png).
const DEFAULT_STRIP: &[&str] = &["AddFolder", "Folders", TITLE, "ViewOptions", TRACK, "NewNote", FLEX, "Format", "Checklist", "Table", "Media", "Markup", "Overflow", SEARCH];

pub fn def(id: &str) -> Option<&'static Def> {
    ITEMS.iter().find(|d| d.id == id)
}

pub fn is_space(id: &str) -> bool {
    id == SPACE || id == FLEX || id == GAP_ID
}

/// Items that need an open note (gallery view hides them).
fn needs_note(id: &str) -> bool {
    matches!(id, "Format" | "Checklist" | "Table" | "Link" | "Media" | "Markup" | "Actions")
}

/// The toolbar's items from the settings (the default set when none are saved). The title and
/// the tracking separator are always there.
pub fn items(app: &App) -> Vec<String> {
    let s = app.settings.toolbar_items.trim();
    if s.is_empty() {
        return DEFAULT.iter().map(|s| s.to_string()).collect();
    }
    let mut out: Vec<String> = Vec::new();
    for id in s.split(',').map(str::trim).filter(|id| def(id).is_some() || *id == TITLE || *id == TRACK) {
        if is_space(id) || !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    for fixed in [TRACK, TITLE] {
        if !out.iter().any(|o| o == fixed) {
            out.insert(0, fixed.to_string());
        }
    }
    out
}

pub fn set_items(app: &mut App, list: &[String]) {
    let is_default = list.len() == DEFAULT.len() && list.iter().zip(DEFAULT).all(|(a, b)| a == b);
    app.settings.toolbar_items = if is_default { String::new() } else { list.join(",") };
    app.settings.save();
}

/// What is being dragged onto or off the toolbar.
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    Item { id: String, from: Option<usize> },
    Defaults,
}

pub struct TbState {
    pub drag: Option<Payload>,
    pub customizing: bool,
    rects: Vec<(usize, Rect)>,
    bar: Rect,
    poof: Option<(Pos2, f64)>,
    /// the search field has the keyboard
    pub search_focus: bool,
}

impl Default for TbState {
    fn default() -> Self {
        TbState { drag: None, customizing: false, rects: Vec::new(), bar: Rect::NOTHING, poof: None, search_focus: false }
    }
}

/// Where the columns are this frame (the toolbar strip over each).
pub struct Columns {
    pub side: Rect,
    pub list: Rect,
    pub editor: Rect,
    pub gallery: bool,
}

pub fn enabled(app: &App, id: &str) -> bool {
    let note = app.selected_note().is_some_and(|n| !n.deleted) && !(app.gallery && !app.gallery_open);
    match id {
        // (Mac captures: Aa is dimmed unless the editor has the caret; Checklist, Table, the
        // paperclip and Markup stay black whenever a note is open)
        "Format" => note && (app.focus == crate::app::Focus::Editor || app.editor_state.focused),
        "Share" => app.selected_note().is_some(),
        "Checklist" | "Table" | "Link" | "Media" | "Markup" | "Actions" => note,
        _ => match def(id).and_then(|d| d.command) {
            Some(c) => crate::menus::enabled(app, c),
            None => true,
        },
    }
}

fn has_popup(id: &str) -> bool {
    matches!(id, "Format" | "Media" | "Share" | "Actions" | "ViewOptions")
}

fn run(app: &mut App, ctx: &egui::Context, id: &str) {
    match id {
        "Folders" => app.show_sidebar = !app.show_sidebar,
        _ => {
            if let Some(c) = def(id).and_then(|d| d.command) {
                crate::menus::run(app, ctx, c);
            }
        }
    }
}

/// What one entry takes.
fn width(id: &str, search_w: f32) -> f32 {
    match id {
        SPACE => 24.0,
        FLEX | TRACK | TITLE | GAP_ID => 0.0,
        SEARCH => search_w,
        _ => ITEM_W,
    }
}

/// The toolbar strip over the columns.
pub fn show(app: &mut App, ui: &mut egui::Ui, cols: &Columns) {
    let ctx = ui.ctx().clone();
    let list = items(app);
    let title_at = list.iter().position(|x| x == TITLE).unwrap_or(0);
    let track_at = list.iter().position(|x| x == TRACK).unwrap_or(list.len());
    let customizing = app.tb.customizing;
    // how far the sidebar is open (it slides): 0 closed .. 1 open
    let open = (cols.side.width() / app.sidebar_w.max(1.0)).clamp(0.0, 1.0);
    // (indices into `list`)
    let before: Vec<usize> = (0..title_at.min(track_at)).collect();
    let mid: Vec<usize> = (title_at + 1..track_at).collect();
    let after: Vec<usize> = (track_at + 1..list.len()).collect();
    let bar = Rect::from_min_max(cols.side.min, cols.editor.max);
    app.tb.bar = bar;
    // over the list and editor the toolbar sits on the page's own colour (Apple's: no band; the
    // sidebar keeps its grey under its part); over the note list it's the list's own material,
    // which the list paints (the rows scroll up under it, blurred)
    let from = if cols.gallery || cols.list.width() < 1.0 { cols.list.min.x } else { cols.editor.min.x };
    let band = Rect::from_min_max(pos2(from, bar.min.y), bar.max);
    ui.painter().rect_filled(band, app.corners_for(band), look::content_bg());
    // (at rest nothing is under it: the band stays the page's white, as on the Mac)
    if app.editor_top_inset > 0.0 && !cols.gallery && app.editor_scroll_y > 0.0 {
        editor_material(ui, Rect::from_min_max(pos2(cols.editor.min.x, bar.min.y), bar.max));
    }
    let mut rects: Vec<(usize, Rect)> = Vec::new();
    let mut clicked: Option<String> = None;
    let mut drag_start: Option<usize> = None;
    // The sidebar's buttons move with it, every frame, never jumping (Apple's): the sidebar
    // toggle glides from inside the sidebar's right edge to the band's far left as it closes;
    // Add Folder (and anything else placed before the title) rides with the sidebar's edge and
    // slides out under it, clipped there.
    let toggle = before.iter().copied().find(|&i| list[i] == "Folders");
    let riders: Vec<usize> = before.iter().copied().filter(|&i| Some(i) != toggle).collect();
    let solo = SOLO_W + 2.0 * CAP_PAD;
    let toggle_w = if toggle.is_some() { solo } else { 0.0 };
    if !riders.is_empty() {
        let mut r: Vec<usize> = vec![usize::MAX];
        for (k, &i) in riders.iter().enumerate() {
            if k > 0 {
                r.push(usize::MAX - 1);
            }
            r.push(i);
        }
        let need = riders.len() as f32 * solo + riders.len().saturating_sub(1) as f32 * GAP;
        let right = cols.side.max.x - 8.0 - if toggle.is_some() { toggle_w + GAP } else { 0.0 };
        let mut clipped = ui.new_child(egui::UiBuilder::new().max_rect(bar).id_salt("tb-riders"));
        clipped.set_clip_rect(cols.side.intersect(ui.clip_rect()));
        lay_row(app, &mut clipped, &list, &r, Rect::from_min_max(pos2(right - need, bar.min.y), pos2(right, bar.max.y)), &mut rects, &mut clicked, &mut drag_start);
    }
    // the toggle: open, right-aligned in the sidebar; closed, 8 pt from the band's left
    // (between the two resting places, straight and one way: the open one is where it sits with
    // the sidebar all the way open)
    let toggle_open_x = cols.side.min.x + app.sidebar_w - 8.0 - toggle_w;
    let toggle_closed_x = cols.side.min.x + 8.0;
    let toggle_x = toggle_closed_x + (toggle_open_x - toggle_closed_x) * open;
    if let Some(i) = toggle {
        lay_row(app, ui, &list, &[i], Rect::from_min_max(pos2(toggle_x, bar.min.y), pos2(toggle_x + toggle_w, bar.max.y)), &mut rects, &mut clicked, &mut drag_start);
    }
    // the title glides with it: 19 pt into the list when the sidebar is open, after the toggle
    // when it's closed
    let title_open = cols.side.min.x + app.sidebar_w + if cols.gallery { 11.0 } else { 19.0 };
    let title_closed = toggle_closed_x + toggle_w + 6.0;
    let title_x = title_closed + (title_open - title_closed) * open;
    if std::env::var_os("NT_TB_TRACE").is_some() {
        let pos = |id: &str| rects.iter().find(|(i, _)| list.get(*i).is_some_and(|l| l == id)).map(|(_, r)| *r);
        let af = pos("AddFolder");
        let shown = af.map(|r| (r.intersect(cols.side).width().max(0.0) / r.width()).clamp(0.0, 1.0)).unwrap_or(0.0);
        eprintln!("tb open={open:.3} addfolder_x={:.1} addfolder_shown={shown:.2} toggle_x={toggle_x:.1} title_x={title_x:.1}", af.map(|r| r.min.x).unwrap_or(f32::NAN));
    }
    let title_right;
    if cols.gallery {
        // one strip over the list's and editor's place: the title, then the items that work
        // without a note, right-aligned
        let mut row: Vec<usize> = vec![usize::MAX];
        row.extend(mid.iter().copied());
        // (spaces between what's left become plain gaps, one between any two capsules)
        for &i in after.iter().filter(|&&i| !needs_note(&list[i])) {
            if is_space(&list[i]) {
                if row.last().is_some_and(|&l| l != usize::MAX - 1 && l != usize::MAX) {
                    row.push(usize::MAX - 1);
                }
            } else {
                row.push(i);
            }
        }
        title_right = cols.editor.max.x - 200.0;
        lay_row(app, ui, &list, &row, Rect::from_min_max(pos2(title_x + 180.0, bar.min.y), pos2(cols.editor.max.x - 8.0, bar.max.y)), &mut rects, &mut clicked, &mut drag_start);
    } else {
        let mut r: Vec<usize> = vec![usize::MAX];
        r.extend(mid.iter().copied());
        // (the title runs up to the "…" button's real width, a gap short of it)
        let mid_w = if mid.is_empty() { 0.0 } else if mid.len() == 1 { SOLO_W + 2.0 * CAP_PAD } else { mid.len() as f32 * ITEM_W + 2.0 * CAP_PAD };
        title_right = cols.list.max.x - 8.0 - mid_w - GAP;
        lay_row(app, ui, &list, &r, Rect::from_min_max(pos2(title_x + 60.0, bar.min.y), pos2(cols.list.max.x - 8.0, bar.max.y)), &mut rects, &mut clicked, &mut drag_start);
        lay_row(app, ui, &list, &after, Rect::from_min_max(pos2(cols.editor.min.x + 8.0, bar.min.y), pos2(cols.editor.max.x - 8.0, bar.max.y)), &mut rects, &mut clicked, &mut drag_start);
        if customizing {
            // the tracking separator: a double line at the columns' edge
            let x = cols.editor.min.x;
            for dx in [-1.5, 1.5] {
                ui.painter().vline(x + dx, bar.min.y + 8.0..=bar.max.y - 8.0, Stroke::new(1.0, kn_ui::theme::gray(0x70)));
            }
        }
    }
    // the title: the folder's name (bold 13) over its count (11, grey)
    let title = app.place_title();
    let count = if !app.search.trim().is_empty() { app.ordered().len() } else { app.count(&app.place) };
    let count_text = match count {
        0 => "No notes".to_string(),
        1 => "1 note".to_string(),
        n => format!("{n} notes"),
    };
    let tr = Rect::from_min_max(pos2(title_x, bar.min.y), pos2(title_right.max(title_x + 40.0), bar.max.y));
    let p = ui.painter().with_clip_rect(tr);
    let dim = if customizing { 0.5 } else { 1.0 };
    // the whole name, shortened with "…" only when it doesn't fit
    let title = crate::list::elide(&p, &title, look::bold(13.0), tr.width());
    let count_text = crate::list::elide(&p, &count_text, look::regular(11.0), tr.width());
    p.text(pos2(tr.min.x, bar.min.y + 19.0), Align2::LEFT_CENTER, title, look::bold(13.0), look::header_text().gamma_multiply(dim));
    if app.show_counts {
        p.text(pos2(tr.min.x, bar.min.y + 34.0), Align2::LEFT_CENTER, count_text, look::regular(11.0), look::text_dim().gamma_multiply(dim));
    }
    app.tb.rects = rects;
    if let Some(i) = drag_start {
        app.tb.drag = Some(Payload::Item { id: list[i].clone(), from: Some(i) });
    }
    if let Some(id) = clicked {
        run(app, &ctx, &id);
    }
}

/// The toolbar strip's empty parts: right-click for Customize Toolbar… (ctx-toolbar.txt). Called
/// before the columns so their own clicks win.
pub fn strip_menu(app: &mut App, ui: &mut egui::Ui, bar: Rect) {
    let strip = ui.interact(bar, ui.id().with("toolbar-strip"), Sense::click());
    crate::menus::context_menu(&strip, egui::Id::new("tb-context-strip"), |ui| toolbar_menu(app, ui));
}

/// Lay out `idx` (into `list`; usize::MAX = a flexible space, which pushes what follows to the
/// right) in `area`; returns the right edge used.
#[allow(clippy::too_many_arguments)]
fn lay_row(app: &mut App, ui: &mut egui::Ui, list: &[String], idx: &[usize], area: Rect, rects: &mut Vec<(usize, Rect)>, clicked: &mut Option<String>, drag_start: &mut Option<usize>) -> f32 {
    if idx.is_empty() {
        return area.min.x;
    }
    let cy = area.min.y + HEIGHT / 2.0;
    // (usize::MAX: a flexible space; usize::MAX - 1: a gap that only separates capsules)
    let id_of = |i: usize| if i == usize::MAX { FLEX } else if i == usize::MAX - 1 { GAP_ID } else { list[i].as_str() };
    let solo = |k: usize| -> bool {
        let id = id_of(idx[k]);
        let prev = k.checked_sub(1).map(|j| id_of(idx[j]));
        let next = idx.get(k + 1).map(|&j| id_of(j));
        prev.is_none_or(|p| is_space(p) || p == SEARCH) && next.is_none_or(|n| is_space(n) || n == SEARCH) && id != SEARCH
    };
    // capsules: runs of buttons between spaces (the search field is a capsule of its own)
    let mut caps = 0;
    let mut searches = 0;
    let mut in_cap = false;
    for &i in idx {
        let id = id_of(i);
        if is_space(id) || id == SEARCH {
            in_cap = false;
            caps += (id == SEARCH) as usize;
            searches += (id == SEARCH) as usize;
        } else if !in_cap {
            caps += 1;
            in_cap = true;
        }
    }
    let base = |search_w: f32| -> f32 {
        let items: f32 = idx.iter().enumerate().map(|(k, &i)| if solo(k) { SOLO_W } else { width(id_of(i), search_w) }).sum();
        // (the search field has no padding of its own)
        items + (caps - searches) as f32 * 2.0 * CAP_PAD + caps.saturating_sub(1) as f32 * GAP
    };
    // the search field takes what's left, up to 325 pt (Mac: 325 pt in a 1065 pt window)
    let search_w = (area.width() - base(0.0) - 80.0).clamp(120.0, 326.0);
    let mut widths: Vec<f32> = idx.iter().enumerate().map(|(k, &i)| if solo(k) { SOLO_W } else { width(id_of(i), search_w) }).collect();
    let flexes = idx.iter().filter(|&&i| id_of(i) == FLEX).count();
    let spare = (area.width() - base(search_w)).max(0.0);
    for (k, &i) in idx.iter().enumerate() {
        if id_of(i) == FLEX && flexes > 0 {
            widths[k] = spare / flexes as f32;
        }
    }
    let mut x = area.min.x;
    let mut cap_start: Option<f32> = None;
    let mut caps_r: Vec<Rect> = Vec::new();
    let painter_idx = ui.painter().add(egui::Shape::Noop);
    let close = |x: &mut f32, cap_start: &mut Option<f32>, caps_r: &mut Vec<Rect>| {
        if let Some(x0) = cap_start.take() {
            *x += CAP_PAD;
            caps_r.push(Rect::from_min_max(pos2(x0, cy - CAP_H / 2.0), pos2(*x, cy + CAP_H / 2.0)));
            *x += GAP;
        }
    };
    let customizing = app.tb.customizing;
    let ctrl = ui.input(|i| i.modifiers.command);
    for (k, &i) in idx.iter().enumerate() {
        let id = id_of(i).to_string();
        if is_space(&id) {
            close(&mut x, &mut cap_start, &mut caps_r);
            let r = Rect::from_min_size(pos2(x - GAP, area.min.y), vec2(widths[k] + GAP, HEIGHT));
            if i < usize::MAX - 1 {
                rects.push((i, r));
                if customizing {
                    ui.painter().rect_stroke(Rect::from_center_size(r.center(), vec2((r.width() - 12.0).max(24.0), 33.0)), 1.0, Stroke::new(1.0, kn_ui::theme::gray(0x80)), egui::StrokeKind::Inside);
                    if ui.interact(r, ui.id().with(("tb-space", i)), Sense::drag()).drag_started() {
                        *drag_start = Some(i);
                    }
                }
            }
            x += widths[k];
            continue;
        }
        if id == SEARCH {
            close(&mut x, &mut cap_start, &mut caps_r);
            let r = Rect::from_min_size(pos2(x, cy - CAP_H / 2.0), vec2(widths[k], CAP_H));
            rects.push((i, r));
            search_field(app, ui, r, i);
            x += widths[k] + GAP;
            continue;
        }
        if cap_start.is_none() {
            cap_start = Some(x);
            x += CAP_PAD;
        }
        let w = widths[k];
        let r = Rect::from_min_size(pos2(x, cy - CAP_H / 2.0), vec2(w, CAP_H));
        rects.push((i, r));
        x += w;
        let next_space = idx.get(k + 1).is_none_or(|&j| is_space(id_of(j)) || id_of(j) == SEARCH);
        let Some(dd) = def(&id) else {
            if next_space {
                close(&mut x, &mut cap_start, &mut caps_r);
            }
            continue;
        };
        let on = enabled(app, &id) && !customizing;
        let sense = if customizing || ctrl { Sense::click_and_drag() } else if on { Sense::click() } else { Sense::hover() };
        let resp = ui.interact(r, ui.id().with(("tb", id.as_str())), sense);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, on, dd.label));
        let open = has_popup(&id) && egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
        // (Letters' cell: the hovered or pressed button a rounded grey under its glyph)
        let bg = if (resp.hovered() && on) || open { kn_ui::theme::gray(0x48) } else { Color32::TRANSPARENT };
        let icon_r = Rect::from_center_size(r.center(), vec2(w.max(CAP_H - 4.0), CAP_H - 4.0));
        if bg != Color32::TRANSPARENT {
            if w == SOLO_W {
                // a button on its own (Mac): the hover fills its whole circle, edge to edge, just
                // inside the rim
                kn_ui::capsule::solo_highlight(ui.painter(), r.center(), bg);
            } else {
                // in a group: a rounded cell inset under its glyph
                ui.painter().rect_filled(icon_r.shrink(2.0), icon_r.height() / 2.0, bg);
            }
        }
        let col = if on || customizing { kn_ui::theme::gray(0xee) } else { kn_ui::theme::gray(0x70) };
        let sz = 22.0_f32.min(icon_r.height() - 8.0);
        draw(ui.painter(), Rect::from_center_size(r.center(), vec2(sz, sz)), dd.glyph, col);
        if resp.drag_started() {
            *drag_start = Some(i);
        } else if !customizing {
            if has_popup(&id) {
                popup(app, &id, &resp);
            } else if resp.clicked() {
                *clicked = Some(id.clone());
            }
            crate::menus::context_menu(&resp, resp.id.with("tb-context"), |ui| toolbar_menu(app, ui));
            resp.on_hover_text(dd.tip);
        }
        if next_space {
            close(&mut x, &mut cap_start, &mut caps_r);
        }
    }
    close(&mut x, &mut cap_start, &mut caps_r);
    paint_capsules(ui, painter_idx, &caps_r);
    x - GAP
}

/// The capsules behind the buttons: Letters' chrome::paint_capsules (liquid glass when on, the
/// classic strip, Numbers' dark capsules, the light glass).
fn paint_capsules(ui: &egui::Ui, idx: egui::layers::ShapeIdx, caps: &[Rect]) {
    ui.painter().set(idx, egui::Shape::Vec(capsule_shapes(ui, caps)));
}

/// (kn_ui::capsule, shared with the suite's apps: glass, classic, Notes' dark rimmed and light
/// solid capsules)
fn capsule_shapes(ui: &egui::Ui, caps: &[Rect]) -> Vec<egui::Shape> {
    kn_ui::capsule::shapes(ui.ctx(), caps, &|c| kn_ui::capsule::dark_rimmed(c, look::capsule_fill()))
}

/// The search field: magnifier with ⌄ (search options), "Search" grey, the text typed.
fn search_field(app: &mut App, ui: &mut egui::Ui, r: Rect, i: usize) {
    let customizing = app.tb.customizing;
    let p = ui.painter().clone();
    // (the same capsule as the buttons')
    p.extend(capsule_shapes(ui, &[r]));
    let dim = kn_ui::theme::TEXT_DIM();
    draw(&p, Rect::from_center_size(pos2(r.min.x + 21.0, r.center().y), vec2(20.0, 20.0)), Glyph::Search, dim);
    draw(&p, Rect::from_center_size(pos2(r.min.x + 34.0, r.center().y + 1.0), vec2(10.0, 10.0)), Glyph::ChevronDown, dim);
    let field = Rect::from_min_max(pos2(r.min.x + 44.0, r.min.y + 4.0), pos2(r.max.x - 26.0, r.max.y - 4.0));
    if customizing {
        p.text(pos2(field.min.x, r.center().y), Align2::LEFT_CENTER, "Search", look::semibold(14.0), dim);
        if ui.interact(r, ui.id().with(("tb-search", i)), Sense::drag()).drag_started() {
            app.tb.drag = Some(Payload::Item { id: SEARCH.into(), from: Some(i) });
        }
        return;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let te = egui::TextEdit::singleline(&mut app.search)
        .id(egui::Id::new("note-search"))
        .frame(egui::Frame::NONE)
        .hint_text(egui::RichText::new("Search").color(dim).font(look::semibold(14.0)))
        .font(look::regular(15.0))
        .text_color(look::text())
        .desired_width(field.width())
        .vertical_align(egui::Align::Center);
    let resp = child.add(te);
    app.tb.search_focus = resp.has_focus();
    if resp.changed() {
        app.note = None;
        app.select_first();
    }
    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.search.clear();
        resp.surrender_focus();
        app.select_first();
    }
    if !app.search.is_empty() {
        // the clear button
        let c = pos2(r.max.x - 15.0, r.center().y);
        let x = ui.interact(Rect::from_center_size(c, vec2(16.0, 16.0)), ui.id().with("search-clear"), Sense::click());
        p.circle_filled(c, 7.5, dim);
        let k = Stroke::new(1.4, look::content_bg());
        p.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], k);
        p.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], k);
        if x.clicked() {
            app.search.clear();
            app.select_first();
        }
    }
    // Edit > Find > Note List Search… puts the keyboard here
    if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::new("focus-search")).unwrap_or(false)) {
        resp.request_focus();
    }
}

/// Items with a popover or menu of their own.
fn popup(app: &mut App, id: &str, resp: &egui::Response) {
    let close = if id == "Format" { egui::PopupCloseBehavior::CloseOnClickOutside } else { egui::PopupCloseBehavior::CloseOnClick };
    egui::Popup::menu(resp).close_behavior(close).show(|ui| popover_body(app, id, ui));
}

/// What an item's popover or menu holds.
pub fn popover_body(app: &mut App, id: &str, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    match id {
        // (the editor draws the Aa popover's contents: editor::popover)
        "Format" => crate::editor::format_popover(app, ui),
        "Media" => {
            // (Notes' paperclip: the photo library or a file; its iPhone items need a phone)
            for label in ["Photos…", "Choose File…"] {
                if ui.button(label).clicked() {
                    crate::menus::run(app, &ctx, "Attach File…");
                    ui.close();
                }
            }
        }
        "Share" => {
            for (label, cmd) in [("Export as PDF…", "Export To > PDF"), ("Export as Markdown…", "Export To > Markdown"), ("Export…", "Export…")] {
                if ui.add_enabled(crate::menus::enabled(app, cmd), egui::Button::new(label)).clicked() {
                    crate::menus::run(app, &ctx, cmd);
                    ui.close();
                }
            }
            ui.separator();
            ui.add_enabled(false, egui::Button::new("Collaborate (not available on Linux)"));
        }
        "Actions" => {
            let items = crate::menus::more_menu(app);
            if let Some(a) = crate::menus::show_items(ui, &items) {
                crate::menus::menu_action(app, &ctx, &a);
            }
        }
        "ViewOptions" => {
            let items = crate::menus::view_options_menu(app);
            if let Some(a) = crate::menus::show_items(ui, &items) {
                crate::menus::menu_action(app, &ctx, &a);
            }
        }
        _ => {}
    }
}

/// While something is dragged: its image under the pointer, where it would land, and what
/// happens when it is let go (Letters'). Also the puff of a removed item.
pub fn drag_overlay(app: &mut App, ctx: &egui::Context) {
    if let Some((at, t0)) = app.tb.poof {
        let t = (ctx.input(|i| i.time) - t0) as f32 / 0.35;
        if t >= 1.0 {
            app.tb.poof = None;
        } else {
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tb-poof")));
            for k in 0..6 {
                let a = k as f32 * std::f32::consts::TAU / 6.0;
                p.circle_filled(at + vec2(a.cos(), a.sin()) * (6.0 + 22.0 * t), 7.0 * (1.0 - t * 0.5), Color32::from_gray(160).gamma_multiply(1.0 - t));
            }
            ctx.request_repaint();
        }
    }
    let Some(payload) = app.tb.drag.clone() else { return };
    let Some(p) = ctx.pointer_latest_pos() else { return };
    let over = app.tb.bar.expand2(vec2(0.0, 14.0)).contains(p);
    let list = items(app);
    // insertion point: before the leftmost entry whose middle is right of the pointer
    let slot = app.tb.rects.iter().filter(|(_, r)| p.x < r.center().x).min_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x)).map(|(i, _)| *i).unwrap_or(list.len());
    let layer = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tb-drag")));
    let (glyph, text) = match &payload {
        Payload::Item { id, .. } => (def(id).map(|d| d.glyph).unwrap_or(Glyph::More), def(id).map(|d| d.label).unwrap_or("")),
        Payload::Defaults => (Glyph::Sidebar, "Default Set"),
    };
    layer.circle_filled(p, 20.0, look::capsule_fill());
    layer.circle_stroke(p, 20.0, Stroke::new(1.0, look::capsule_rim()));
    draw(&layer, Rect::from_center_size(p, vec2(22.0, 22.0)), glyph, kn_ui::theme::gray(0xee));
    layer.text(p + vec2(0.0, 30.0), Align2::CENTER_CENTER, text, look::regular(11.0), look::text());
    if over && payload != Payload::Defaults {
        let x = app.tb.rects.iter().find(|(i, _)| *i == slot).map(|(_, r)| r.min.x).unwrap_or(app.tb.bar.max.x - 4.0);
        layer.vline(x, app.tb.bar.y_range(), Stroke::new(3.0, look::accent()));
    } else if over {
        layer.rect_stroke(app.tb.bar, 8.0, Stroke::new(2.0, look::accent()), egui::StrokeKind::Inside);
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
                // dragged off the toolbar: gone, in a puff of smoke (never the title or separator)
                if list[f] == TITLE || list[f] == TRACK {
                    return;
                }
                list.remove(f);
                app.tb.poof = Some((p, ctx.input(|i| i.time)));
            } else {
                return;
            }
        }
    }
    set_items(app, &list);
}

/// View > Customize Toolbar…: the sheet from the toolbar (toolbar-customize-sheet.png): the
/// palette, the default set as one strip, Done (Notes has no Icon/Text choice).
pub fn sheet(app: &mut App, ctx: &egui::Context) {
    if !app.tb.customizing {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.tb.customizing = false;
        return;
    }
    let in_bar = items(app);
    let screen = kn_ui::winframe::inner_rect(ctx);
    let bar = if app.tb.bar.is_positive() { app.tb.bar } else { Rect::from_min_size(pos2(screen.min.x, screen.min.y + 30.0), vec2(screen.width(), HEIGHT)) };
    let w = (screen.width() - 24.0).min(1000.0);
    let top = pos2(screen.center().x - w / 2.0, bar.max.y + 2.0);
    let light = kn_ui::theme::is_light();
    egui::Area::new(egui::Id::new("customize-toolbar")).order(egui::Order::Foreground).fixed_pos(top).show(ctx, |ui| {
        let fill = if light { Color32::from_rgb(0xf7, 0xf7, 0xf6) } else { Color32::from_gray(0x2a) };
        egui::Frame::new()
            .fill(fill)
            .corner_radius(14)
            .stroke(Stroke::new(1.0, if light { Color32::from_black_alpha(30) } else { Color32::from_gray(0x44) }))
            .shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if light { 50 } else { 120 }) })
            .inner_margin(egui::Margin { left: 26, right: 26, top: 18, bottom: 16 })
            .show(ui, |ui| {
                ui.set_width(w - 52.0);
                ui.label(egui::RichText::new("Drag your favorite items into the toolbar…").font(look::regular(13.0)).color(look::text()));
                ui.add_space(14.0);
                let cell_w = (w - 52.0) / 9.0;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 6.0);
                    for dd in ITEMS.iter() {
                        let wide = dd.id == SEARCH;
                        let (r, resp) = ui.allocate_exact_size(vec2(if wide { cell_w * 1.25 } else { cell_w }, 70.0), Sense::drag());
                        let used = !is_space(dd.id) && in_bar.iter().any(|x| x == dd.id);
                        let p = ui.painter();
                        let ic = pos2(r.center().x, r.min.y + 18.0);
                        let col = if used { kn_ui::theme::gray(0xee).gamma_multiply(0.35) } else { kn_ui::theme::gray(0xee) };
                        match dd.id {
                            SPACE => {
                                p.rect_stroke(Rect::from_center_size(ic, vec2(28.0, 28.0)), 1.0, Stroke::new(1.0, kn_ui::theme::gray(0x80)), egui::StrokeKind::Inside);
                            }
                            FLEX => {
                                p.rect_stroke(Rect::from_center_size(ic, vec2(28.0, 28.0)), 1.0, Stroke::new(1.0, kn_ui::theme::gray(0x80)), egui::StrokeKind::Inside);
                                p.text(ic, Align2::CENTER_CENTER, "←···→", look::regular(7.0), kn_ui::theme::gray(0xa0));
                            }
                            SEARCH => {
                                let b = Rect::from_center_size(ic, vec2(108.0, 33.0));
                                p.rect_filled(b, 16.5, look::capsule_fill());
                                p.rect_stroke(b, 16.5, Stroke::new(1.0, look::capsule_rim()), egui::StrokeKind::Inside);
                                draw(p, Rect::from_center_size(pos2(b.min.x + 16.0, ic.y), vec2(15.0, 15.0)), Glyph::Search, col);
                                p.text(pos2(b.min.x + 30.0, ic.y), Align2::LEFT_CENTER, "Search", look::regular(13.0), col);
                            }
                            _ => {
                                p.circle_filled(ic, 16.5, look::capsule_fill());
                                p.circle_stroke(ic, 16.5, Stroke::new(1.0, look::capsule_rim()));
                                draw(p, Rect::from_center_size(ic, vec2(22.0, 22.0)), dd.glyph, col);
                            }
                        }
                        p.text(pos2(r.center().x, r.min.y + 46.0), Align2::CENTER_CENTER, dd.label, look::regular(11.0), if used { look::text_dim() } else { look::text() });
                        if resp.drag_started() {
                            app.tb.drag = Some(Payload::Item { id: dd.id.to_string(), from: None });
                        }
                        if !dd.tip.is_empty() {
                            resp.on_hover_text(dd.tip);
                        }
                    }
                });
                ui.add_space(24.0);
                ui.label(egui::RichText::new("… or drag the default set into the toolbar.").font(look::regular(13.0)).color(look::text()));
                ui.add_space(10.0);
                // the default set as one strip
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 70.0), Sense::drag());
                let p = ui.painter();
                p.rect_filled(r, 10.0, if light { Color32::from_rgb(0xf0, 0xf0, 0xef) } else { Color32::from_gray(0x33) });
                let mut x = r.min.x + 16.0;
                let c = |x: f32| pos2(x, r.min.y + 26.0);
                let lab = |x: f32, t: &str| {
                    p.text(pos2(x, r.min.y + 55.0), Align2::CENTER_CENTER, t, look::regular(11.0), look::text());
                };
                let button = |x: f32, g: Glyph| {
                    p.circle_filled(c(x), 16.5, look::capsule_fill());
                    p.circle_stroke(c(x), 16.5, Stroke::new(1.0, look::capsule_rim()));
                    draw(p, Rect::from_center_size(c(x), vec2(22.0, 22.0)), g, kn_ui::theme::gray(0xee));
                };
                for id in DEFAULT_STRIP {
                    match *id {
                        TITLE => {
                            p.text(pos2(x + 6.0, r.min.y + 21.0), Align2::LEFT_CENTER, app.place_title(), look::bold(12.0), look::header_text());
                            p.text(pos2(x + 6.0, r.min.y + 35.0), Align2::LEFT_CENTER, format!("{} notes", app.count(&app.place)), look::regular(10.0), look::text_dim());
                            x += 140.0;
                        }
                        TRACK => {
                            p.vline(x, r.min.y + 16.0..=r.min.y + 40.0, Stroke::new(1.0, kn_ui::theme::gray(0x80)));
                            x += 20.0;
                        }
                        FLEX => {
                            p.rect_stroke(Rect::from_center_size(c(x + 36.0), vec2(28.0, 28.0)), 1.0, Stroke::new(1.0, kn_ui::theme::gray(0x80)), egui::StrokeKind::Inside);
                            lab(x + 36.0, "Flexible Space");
                            x += 80.0;
                        }
                        "Overflow" => {
                            button(x + 22.0, Glyph::Overflow);
                            x += 50.0;
                        }
                        SEARCH => {
                            let b = Rect::from_min_size(pos2(x, r.min.y + 9.0), vec2((r.max.x - x - 12.0).clamp(80.0, 108.0), 33.0));
                            p.rect_filled(b, 16.5, look::capsule_fill());
                            p.rect_stroke(b, 16.5, Stroke::new(1.0, look::capsule_rim()), egui::StrokeKind::Inside);
                            draw(p, Rect::from_center_size(pos2(b.min.x + 16.0, b.center().y), vec2(18.0, 18.0)), Glyph::Search, kn_ui::theme::gray(0xee));
                            p.text(pos2(b.min.x + 30.0, b.center().y), Align2::LEFT_CENTER, "Search", look::bold(12.0), look::text());
                            lab(b.center().x, "Search");
                        }
                        _ => {
                            let Some(dd) = def(id) else { continue };
                            let lw = p.layout_no_wrap(dd.label.to_string(), look::regular(11.0), Color32::WHITE).size().x;
                            let cw = (lw + 10.0).max(44.0);
                            button(x + cw / 2.0, dd.glyph);
                            lab(x + cw / 2.0, dd.label);
                            x += cw;
                        }
                    }
                }
                if resp.drag_started() {
                    app.tb.drag = Some(Payload::Defaults);
                }
                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Done").font(look::regular(13.0)).color(look::on_accent())).fill(look::accent()).corner_radius(6).min_size(vec2(72.0, 24.0))).clicked() {
                        app.tb.customizing = false;
                    }
                });
            });
    });
}

/// The toolbar's right-click menu (ctx-toolbar.txt): Customize Toolbar….
fn toolbar_menu(app: &mut App, ui: &mut egui::Ui) {
    if ui.button("Customize Toolbar…").clicked() {
        app.tb.customizing = true;
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_unique_and_default_known() {
        let mut ids: Vec<&str> = ITEMS.iter().map(|d| d.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(DEFAULT.iter().all(|id| def(id).is_some() || *id == TITLE || *id == TRACK));
    }
}

/// The band over the note (Mac, no3-scroll-edges): translucent and heavily blurred, so what
/// scrolls under it shows only as a soft tint of its colours (about 6 % at most), never as
/// readable glyphs. Drawn as the list's material is: each line's box under it (editor::under) as a
/// wide Gaussian blob of its colour, dithered, on the opaque page colour.
fn editor_material(ui: &egui::Ui, band: Rect) {
    const SIGMA: f32 = 12.0;
    let band = band.intersect(kn_ui::winframe::inner_rect(ui.ctx()));
    // (lines just below the band bleed up into it, as through a blur)
    let near = Rect::from_min_max(band.min, pos2(band.max.x, band.max.y + 2.0 * SIGMA));
    let boxes = crate::editor::under(near);
    if boxes.is_empty() {
        return;
    }
    let p = ui.painter().with_clip_rect(band);
    for (r, col) in boxes {
        let tint = col.gamma_multiply(0.08);
        p.add(egui::Shape::mesh(crate::list::gaussian_rect(r, SIGMA, tint, band)));
    }
    crate::list::dither(ui.ctx(), &p, band);
}
