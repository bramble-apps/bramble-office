//! View > Show Attachments Browser (⌘3): every attachment of the notes not deleted, in the Mac's six
//! sections (Photos & Videos, Scans, Maps, Websites, Audio, Documents), in place of the list and the
//! note. Click selects, double-click opens; the context menu has the Mac's items (Show in Note,
//! Quick Look Attachment, Open Attachment, Save Attachment…, Copy; Rename and Share greyed).
//! Read on a worker from the store (its own connection), so a big library doesn't stall the window.
//! (No Mac capture of it yet: sizes follow the gallery's.)

use crate::app::{App, Thumb};
use crate::look;
use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use nt_core::model::{Attachment, AttachmentKind};

pub const SECTIONS: [&str; 6] = ["Photos & Videos", "Scans", "Maps", "Websites", "Audio", "Documents"];
const EMPTY: [&str; 6] = [
    "Photos and videos added to your notes appear here.",
    "Scans added to your notes appear here.",
    "Map attachments added to your notes appear here.",
    "Websites added to your notes appear here.",
    "Audio files added to your notes appear here.",
    "Documents added to your notes appear here.",
];

#[derive(Default)]
pub struct Browser {
    pub section: usize,
    /// (note id, attachment); None while it's being read
    pub items: Option<Vec<(String, Attachment)>>,
    pub selected: Option<String>,
}

/// Which section an attachment shows in (tables and other inline objects: none).
pub fn section_of(a: &Attachment) -> Option<usize> {
    let video = a.uti.contains("movie") || a.uti.contains("video") || a.uti.contains("mpeg-4") || a.uti == "public.avi";
    match a.kind {
        AttachmentKind::Image | AttachmentKind::Drawing => Some(0),
        AttachmentKind::File if video => Some(0),
        AttachmentKind::Scan => Some(1),
        AttachmentKind::Url if a.url.as_deref().is_some_and(|u| u.contains("maps.apple.com") || u.contains("google.com/maps")) || a.uti.contains("mapkit") => Some(2),
        AttachmentKind::Url => Some(3),
        AttachmentKind::Audio => Some(4),
        AttachmentKind::Pdf | AttachmentKind::File => Some(5),
        AttachmentKind::Table => None,
    }
}

/// Shows or hides the browser (reading the attachments when it opens).
pub fn toggle(app: &mut App, ctx: &egui::Context) {
    if app.browser.take().is_some() {
        return;
    }
    app.browser = Some(Browser::default());
    if app.db.is_some() {
        app.background(ctx, || {
            let items = match crate::store::open_own().map_err(nt_core::Error::Format).and_then(|st| st.attachments()) {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("bramble-notes: the attachments couldn’t be read: {e}");
                    Vec::new()
                }
            };
            crate::app::JobDone::Attachments(items)
        });
    } else {
        let items = app.library.notes.iter().filter(|n| !n.deleted && !n.locked).flat_map(|n| n.attachments.iter().map(|a| (n.id.clone(), a.clone()))).collect();
        if let Some(b) = &mut app.browser {
            b.items = Some(items);
        }
    }
}

/// The attachment's picture, for the tile.
fn thumb(a: &Attachment) -> Option<Thumb> {
    match a.kind {
        AttachmentKind::Image => a.preview.clone().or(a.file.clone()).map(Thumb::File),
        AttachmentKind::Scan => a.parts.first().and_then(|p| p.display_file()).cloned().or(a.preview.clone()).map(Thumb::File),
        AttachmentKind::Drawing => a.fallback.clone().filter(|p| p.extension().is_some_and(|e| e == "png")).or(a.preview.clone()).map(Thumb::File),
        AttachmentKind::Pdf => Some(a.preview.clone().or(a.file.clone()).or(a.fallback.clone()).map(Thumb::File).unwrap_or(Thumb::Pdf)),
        _ => a.preview.clone().map(Thumb::File),
    }
}

/// What the tile says under it.
fn caption(a: &Attachment) -> String {
    match a.kind {
        AttachmentKind::Url => {
            if !a.name.is_empty() {
                a.name.clone()
            } else {
                a.url.clone().unwrap_or_default()
            }
        }
        _ if !a.name.is_empty() => a.name.clone(),
        AttachmentKind::Scan => "Scanned Document".into(),
        AttachmentKind::Drawing => "Drawing".into(),
        _ => a.file.as_ref().and_then(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "Attachment".into()),
    }
}

/// The file Open / Save / Quick Look use (a scan: its first page; a drawing: its picture).
fn file_of(a: &Attachment) -> Option<std::path::PathBuf> {
    a.file.clone().or_else(|| a.parts.first().and_then(|p| p.display_file()).cloned()).or(a.fallback.clone()).or(a.preview.clone())
}

fn open(a: &Attachment) {
    if let Some(u) = a.url.as_deref().filter(|_| a.kind == AttachmentKind::Url) {
        crate::editor::open_external(std::path::Path::new(u));
    } else if let Some(f) = file_of(a) {
        crate::editor::open_external(&f);
    }
}

/// The section switch over the grid (the Mac's six labels).
fn switch(ui: &mut egui::Ui, at: egui::Pos2, current: usize) -> Option<usize> {
    let font = look::regular(13.0);
    let widths: Vec<f32> = SECTIONS.iter().map(|s| ui.painter().layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x + 22.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let track = Rect::from_center_size(at, vec2(total, 28.0));
    let p = ui.painter().clone();
    let light = kn_ui::theme::is_light();
    p.rect_filled(track, 14.0, if light { Color32::from_gray(0xe6) } else { Color32::from_gray(0x3a) });
    let mut x = track.min.x + 2.0;
    let mut out = None;
    for (k, (s, w)) in SECTIONS.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(pos2(x, track.min.y + 2.0), vec2(*w, 24.0));
        let resp = ui.interact(r, ui.id().with(("att-section", k)), Sense::click());
        if k == current {
            p.rect_filled(r.translate(vec2(0.0, 0.5)), 12.0, Color32::from_black_alpha(if light { 20 } else { 60 }));
            p.rect_filled(r, 12.0, if light { Color32::WHITE } else { Color32::from_gray(0x5c) });
        } else if resp.hovered() {
            p.rect_filled(r, 12.0, if light { Color32::from_black_alpha(10) } else { Color32::from_white_alpha(10) });
        }
        p.text(r.center(), Align2::CENTER_CENTER, s, font.clone(), look::text());
        if resp.clicked() {
            out = Some(k);
        }
        x += w;
    }
    out
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    kn_ui::touch::pan_area(area);
    let ctx = ui.ctx().clone();
    ui.painter().rect_filled(area, 0.0, look::content_bg());
    let Some(b) = app.browser.as_ref() else { return };
    let section = b.section;
    if let Some(k) = switch(ui, pos2(area.center().x, area.min.y + 24.0), section) {
        if let Some(b) = &mut app.browser {
            b.section = k;
            b.selected = None;
        }
    }
    let grid = Rect::from_min_max(pos2(area.min.x, area.min.y + 52.0), area.max);
    let Some(items) = app.browser.as_ref().and_then(|b| b.items.as_ref()) else {
        ui.painter().text(grid.center(), Align2::CENTER_CENTER, "Loading…", look::regular(13.0), look::text_dim());
        return;
    };
    let shown: Vec<(String, Attachment)> = items.iter().filter(|(_, a)| section_of(a) == Some(section)).cloned().collect();
    if shown.is_empty() {
        let p = ui.painter();
        p.text(grid.center() - vec2(0.0, 12.0), Align2::CENTER_CENTER, "No Attachments", look::bold(17.0), look::text_dim());
        p.text(grid.center() + vec2(0.0, 12.0), Align2::CENTER_CENTER, EMPTY[section], look::regular(13.0), look::text_dim());
        return;
    }
    const CW: f32 = 150.0;
    const PITCH_X: f32 = 166.0;
    const PITCH_Y: f32 = 200.0;
    let selected = app.browser.as_ref().and_then(|b| b.selected.clone());
    let mut clicked: Option<String> = None;
    let mut opened: Option<Attachment> = None;
    let mut action: Option<(String, String, Attachment)> = None;
    let mut sc = std::mem::take(&mut app.list_scroll);
    ui.scope_builder(egui::UiBuilder::new().max_rect(grid), |ui| {
        crate::app::scroll_area(&mut sc, ui, "attachments", |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let w = grid.width();
            let cols = ((w - 15.0 + (PITCH_X - CW)) / PITCH_X).floor().max(1.0) as usize;
            let rows = shown.len().div_ceil(cols);
            let (block, _) = ui.allocate_exact_size(vec2(w, rows as f32 * PITCH_Y + 12.0), Sense::hover());
            let clip = ui.clip_rect();
            let left = block.min.x + ((w - (cols as f32 * PITCH_X - (PITCH_X - CW))) / 2.0).max(15.0);
            for (k, (note, a)) in shown.iter().enumerate() {
                let (c, row) = (k % cols, k / cols);
                let tile = Rect::from_min_size(pos2(left + c as f32 * PITCH_X, block.min.y + 8.0 + row as f32 * PITCH_Y), vec2(CW, CW));
                // (only the rows in view are drawn: a big library has thousands)
                if tile.max.y + 50.0 < clip.min.y || tile.min.y > clip.max.y {
                    continue;
                }
                let resp = ui.interact(tile.union(Rect::from_min_size(tile.left_bottom(), vec2(CW, 40.0))), ui.id().with(("att", &a.id)), Sense::click());
                let p = ui.painter().clone();
                let light = kn_ui::theme::is_light();
                let border = Stroke::new(1.0, if light { Color32::from_gray(0xdd) } else { Color32::from_gray(0x40) });
                match thumb(a) {
                    Some(th) => crate::list::paint_thumb(app, &ctx, &p, &format!("att:{}", a.id), &th, tile),
                    None => {
                        p.rect_filled(tile, 8.0, if light { Color32::from_gray(0xf2) } else { Color32::from_gray(0x2c) });
                        let g = match a.kind {
                            AttachmentKind::Url => crate::icons::Glyph::Link,
                            _ => crate::icons::Glyph::Attachments,
                        };
                        crate::icons::draw(&p, Rect::from_center_size(tile.center(), vec2(40.0, 40.0)), g, look::text_dim());
                    }
                }
                p.rect_stroke(tile, 8.0, border, egui::StrokeKind::Inside);
                if selected.as_deref() == Some(a.id.as_str()) {
                    p.rect_stroke(tile.expand(2.0), 10.0, Stroke::new(3.0, look::accent()), egui::StrokeKind::Middle);
                }
                let title = app.note_by_id(note).map(crate::list::note_title).unwrap_or_default();
                p.text(pos2(tile.center().x, tile.max.y + 16.0), Align2::CENTER_CENTER, crate::list::elide(&p, &caption(a), look::regular(12.0), CW), look::regular(12.0), look::text());
                p.text(pos2(tile.center().x, tile.max.y + 31.0), Align2::CENTER_CENTER, crate::list::elide(&p, &title, look::regular(11.0), CW), look::regular(11.0), look::text_dim());
                if resp.clicked() || resp.secondary_clicked() {
                    clicked = Some(a.id.clone());
                }
                if resp.double_clicked() {
                    opened = Some(a.clone());
                }
                let has_file = file_of(a).is_some() || a.url.is_some();
                crate::menus::context_menu(&resp, resp.id.with("ctx"), |ui| {
                    use crate::menus::MItem;
                    let items = vec![
                        MItem::act("Show in Note", "show", true),
                        MItem::Sep,
                        MItem::act("Quick Look Attachment", "look", has_file),
                        MItem::act("Open Attachment", "open", has_file),
                        MItem::act("Save Attachment…", "save", file_of(a).is_some()),
                        MItem::act("Rename Attachment…", "", false),
                        MItem::Sep,
                        MItem::act("Share", "", false),
                        MItem::act("Copy", "copy", has_file),
                    ];
                    if let Some(x) = crate::menus::show_items(ui, &items) {
                        action = Some((x, note.clone(), a.clone()));
                    }
                });
            }
        })
    });
    app.list_scroll = sc;
    if let Some(id) = clicked {
        if let Some(b) = &mut app.browser {
            b.selected = Some(id);
        }
    }
    if let Some(a) = opened {
        open(&a);
    }
    if let Some((what, note, a)) = action {
        match what.as_str() {
            // the note it's in, with the browser closed
            "show" => show_in_note(app, &note),
            "look" | "open" => open(&a),
            "save" => {
                if let Some(f) = file_of(&a) {
                    let name = f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(to) = crate::app::file_dialog(|d| d.set_file_name(&name).save_file()) {
                        if let Err(e) = std::fs::copy(&f, &to) {
                            app.dialog = Some(crate::app::Dialog::Alert("The attachment couldn’t be saved.".into(), e.to_string()));
                        }
                    }
                }
            }
            "copy" => {
                let text = if a.kind == AttachmentKind::Url { a.url.clone() } else { file_of(&a).map(|f| f.display().to_string()) };
                if let Some(t) = text {
                    ctx.copy_text(t);
                }
            }
            _ => {}
        }
    }
}

/// View > Show in Note: the selected attachment's note, the browser closed.
pub fn show_in_note(app: &mut App, note: &str) {
    app.browser = None;
    if let Some(f) = app.note_by_id(note).map(|n| n.folder.clone()) {
        app.go(crate::app::Place::Folder(f));
    }
    app.select_note(note);
    app.focus = crate::app::Focus::Editor;
}

/// The note of the attachment selected in the browser (View > Show in Note).
pub fn selected_note(app: &App) -> Option<String> {
    let b = app.browser.as_ref()?;
    let id = b.selected.as_ref()?;
    b.items.as_ref()?.iter().find(|(_, a)| &a.id == id).map(|(n, _)| n.clone())
}
