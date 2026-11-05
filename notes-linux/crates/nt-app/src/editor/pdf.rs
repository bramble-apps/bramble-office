//! A PDF in a note as Notes 4.13 shows one (out-NO3 pdf/, 2x light): Large (the default) is a
//! grey band the editor's width with a header — "<name> ⌄" and Show / Hide Thumbnails — and every
//! page side by side, the next one peeking in at the right; Show Thumbnails adds a strip of the
//! pages, the current one darker, a click on one going to it. Medium is the same viewer about
//! half as tall; Small a card ("PDF Document · 13 KB", page 1 on its right). Sideways scrolling
//! (a trackpad, or Shift and the wheel) pages through it; up and down always scrolls the note.
//! The header's ⌄ menu: View As, Quick Look, Open, Rename…, Copy, Delete. Quick Look: the pages
//! down a panel with their thumbnails beside them.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Stroke, pos2, vec2};
use nt_core::model::ViewSize;

use super::Editor;
use super::doc::{Att, AttKind};
use super::layout;

/// Measured (points): the header, the thumbnails strip, the pages' height in Large and Medium,
/// the gap between pages, the room under them; the Small card.
pub const HEADER: f32 = 32.0;
pub const THUMB_W: f32 = 45.0;
pub const THUMB_H: f32 = 58.0;
pub const THUMB_PITCH: f32 = 57.0;
pub const THUMBS_ABOVE: f32 = 12.0;
pub const THUMBS_BELOW: f32 = 19.0;
pub const PAGE_LARGE: f32 = 651.0;
pub const PAGE_MEDIUM: f32 = 266.5;
pub const PAGE_GAP: f32 = 20.0;
pub const BOTTOM: f32 = 12.0;
pub const CARD: egui::Vec2 = egui::Vec2::new(291.0, 66.0);
/// Pages are drawn this many pixels tall (twice Large's points) and kept.
const PAGE_PX: u32 = 1302;

/// A PDF viewer's state: where it is scrolled to (and going), the page shown.
#[derive(Clone, Debug, Default)]
pub struct PdfView {
    pub x: f32,
    pub target: f32,
    pub page: usize,
}

/// What a press on a PDF hits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// the whole band (for sideways scrolling)
    Band,
    Header,
    Thumbs,
    Thumb(usize),
    Pages,
    /// the Small card's Quick Look button
    Look,
}

/// How tall a PDF block is in a column `w` wide.
pub fn size(view: ViewSize, thumbs: bool, w: f32, s: f32) -> egui::Vec2 {
    match view {
        ViewSize::Small => vec2(w.min(CARD.x * s), CARD.y * s),
        _ => {
            let page = if view == ViewSize::Medium { PAGE_MEDIUM } else { PAGE_LARGE };
            let strip = if thumbs { THUMBS_ABOVE + THUMB_H + THUMBS_BELOW } else { 0.0 };
            vec2(w, (HEADER + strip + page + BOTTOM) * s)
        }
    }
}

fn band_color() -> Color32 {
    if layout::dark() { Color32::from_gray(44) } else { Color32::from_gray(236) }
}

pub(super) fn dim() -> Color32 {
    if layout::dark() { Color32::from_gray(150) } else { Color32::from_gray(128) }
}

/// The pages' widths at height `h` (points), from their sizes.
fn widths(pages: &[[f32; 2]], h: f32) -> Vec<f32> {
    pages.iter().map(|p| h * p[0] / p[1].max(1.0)).collect()
}

/// Where page `i` starts in the row (points from the row's start).
fn page_x(ws: &[f32], i: usize, gap: f32) -> f32 {
    ws.iter().take(i).map(|w| w + gap).sum()
}

/// Draws attachment `att` (a PDF) in `r`, noting its controls for the next press.
pub fn paint(ed: &mut Editor, painter: &Painter, att: &Att, r: Rect) {
    let AttKind::Pdf { path, name, pages, view, thumbs, .. } = &att.kind else { return };
    // (Notes titles it without its extension)
    let name = &name.strip_suffix(".pdf").or_else(|| name.strip_suffix(".PDF")).unwrap_or(name).to_string();
    let ctx = painter.ctx().clone();
    let s = ed.settings.text_scale;
    let pages: Vec<[f32; 2]> = if pages.is_empty() { vec![[612.0, 792.0]] } else { pages.clone() };
    let bytes = path.as_deref().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
    if *view == ViewSize::Small {
        // the card: name, "PDF Document · 13 KB", the Quick Look button, page 1
        painter.rect_filled(r, CornerRadius::same((11.0 * s) as u8), if layout::dark() { Color32::from_gray(52) } else { Color32::from_rgb(229, 230, 233) });
        painter.text(pos2(r.min.x + 12.0 * s, r.min.y + 13.0 * s), Align2::LEFT_TOP, name, FontId::new(13.0 * s, layout::family("Sfera-Bold")), layout::text_color());
        painter.text(pos2(r.min.x + 12.0 * s, r.min.y + 33.0 * s), Align2::LEFT_TOP, format!("PDF Document \u{00b7} {}", super::media::size_text(bytes)), FontId::new(13.0 * s, layout::family("Sfera-Regular")), dim());
        let th = Rect::from_min_size(pos2(r.max.x - 12.0 * s - 32.0 * s, r.min.y + 13.0 * s), vec2(32.0 * s, 40.0 * s));
        if let Some((t, _)) = path.as_deref().and_then(|p| ed.media.page(&ctx, p, 0, PAGE_PX / 4)) {
            painter.image(t, th, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }
        let look = Rect::from_center_size(pos2(th.min.x - 22.0 * s, r.center().y + 6.0 * s), vec2(14.0 * s, 14.0 * s));
        painter.circle_filled(look.center(), 7.0 * s, Color32::from_black_alpha(30));
        chevron(painter, look.center(), 3.0 * s, dim());
        ed.media.pdf_hits.push((att.id.clone(), Hit::Look, look.expand(4.0)));
        return;
    }
    let margin = layout::MARGIN_X * s;
    // the band, the pane's width
    let band = Rect::from_min_max(pos2(r.min.x - margin, r.min.y), pos2(r.max.x + margin, r.max.y));
    let clip = painter.with_clip_rect(band.intersect(painter.clip_rect()));
    clip.rect_filled(band, 0.0, band_color());
    ed.media.pdf_hits.push((att.id.clone(), Hit::Band, band));
    // the header: "<name> ⌄" left, Show / Hide Thumbnails right
    let hy = r.min.y + 16.0 * s;
    let g = clip.layout_no_wrap(name.clone(), FontId::new(13.0 * s, layout::family("Sfera-Bold")), layout::text_color());
    let nw = g.size().x;
    clip.galley(pos2(r.min.x - 0.5 * s, hy - g.size().y / 2.0), g, layout::text_color());
    chevron(&clip, pos2(r.min.x + nw + 12.0 * s, hy), 4.5 * s, layout::text_color());
    ed.media.pdf_hits.push((att.id.clone(), Hit::Header, Rect::from_min_max(pos2(r.min.x - 4.0, hy - 11.0 * s), pos2(r.min.x + nw + 20.0 * s, hy + 11.0 * s))));
    let label = if *thumbs { "Hide Thumbnails" } else { "Show Thumbnails" };
    let tr = clip.text(pos2(r.max.x - 1.5 * s, hy), Align2::RIGHT_CENTER, label, FontId::new(13.0 * s, layout::family("Sfera-Regular")), dim());
    ed.media.pdf_hits.push((att.id.clone(), Hit::Thumbs, tr.expand(4.0)));
    let v = ed.media.pdf.entry(att.id.clone()).or_default().clone();
    let mut y = r.min.y + HEADER * s;
    let page_h = (if *view == ViewSize::Medium { PAGE_MEDIUM } else { PAGE_LARGE }) * s;
    let ws = widths(&pages, page_h);
    // the current page: the one most in view from the left
    let current = (0..pages.len()).min_by(|&a, &b| (page_x(&ws, a, PAGE_GAP * s) - v.x).abs().total_cmp(&(page_x(&ws, b, PAGE_GAP * s) - v.x).abs())).unwrap_or(0);
    // (the pages asked for last to first, the ones in view or every one under thumbnails: the
    // workers take the newest first, so the first page, the one read first, is drawn first)
    if let Some(f) = path.as_deref() {
        for (i, w) in ws.iter().enumerate().rev() {
            let px = r.min.x - v.x + page_x(&ws, i, PAGE_GAP * s);
            if *thumbs || (px < band.max.x && px + w > band.min.x) {
                ed.media.page(&ctx, f, i, PAGE_PX);
            }
        }
    }
    if *thumbs {
        y += THUMBS_ABOVE * s;
        for (i, p) in pages.iter().enumerate() {
            let tr = Rect::from_min_size(pos2(r.min.x + 2.5 * s + i as f32 * THUMB_PITCH * s, y), vec2(THUMB_W * s, THUMB_H * s));
            let fit = (THUMB_W / p[0]).min(THUMB_H / p[1]);
            let tr2 = Rect::from_center_size(tr.center(), vec2(p[0] * fit, p[1] * fit) * s);
            let rad = CornerRadius::same((6.0 * s) as u8);
            match path.as_deref().and_then(|f| ed.media.page(&ctx, f, i, PAGE_PX)) {
                Some((t, _)) => {
                    clip.add(egui::epaint::RectShape::filled(tr2, rad, Color32::WHITE).with_texture(t, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))));
                }
                None => {
                    clip.rect_filled(tr2, rad, Color32::WHITE);
                }
            }
            if i == current {
                clip.rect_filled(tr2, rad, Color32::from_black_alpha(46));
            }
            ed.media.pdf_hits.push((att.id.clone(), Hit::Thumb(i), tr));
        }
        y += (THUMB_H + THUMBS_BELOW) * s;
    }
    // the pages, side by side from the column's start, scrolled sideways
    let x0 = r.min.x - v.x;
    let pages_rect = Rect::from_min_max(pos2(band.min.x, y), pos2(band.max.x, y + page_h));
    for (i, w) in ws.iter().enumerate() {
        let pr = Rect::from_min_size(pos2(x0 + page_x(&ws, i, PAGE_GAP * s), y), vec2(*w, page_h));
        if !pr.intersects(band) {
            continue;
        }
        match path.as_deref().and_then(|f| ed.media.page(&ctx, f, i, PAGE_PX)) {
            Some((t, _)) => {
                clip.image(t, pr, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                clip.rect_filled(pr, 0.0, Color32::WHITE);
            }
        }
    }
    ed.media.pdf_hits.push((att.id.clone(), Hit::Pages, pages_rect));
    // easing towards where it's going (a thumbnail's page)
    if (v.target - v.x).abs() > 0.5 {
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        if let Some(st) = ed.media.pdf.get_mut(&att.id) {
            st.x += (st.target - st.x) * (1.0 - (-18.0 * dt).exp());
            st.page = current;
        }
        ctx.request_repaint();
    } else if let Some(st) = ed.media.pdf.get_mut(&att.id) {
        st.x = st.target;
        st.page = current;
    }
}

pub(super) fn chevron(p: &Painter, c: Pos2, k: f32, col: Color32) {
    p.add(egui::Shape::line(vec![c + vec2(-k, -k * 0.5), c + vec2(0.0, k * 0.5), c + vec2(k, -k * 0.5)], Stroke::new(1.5, col)));
}

/// How far a PDF scrolls sideways: until its last page starts at the column's start.
fn max_x(att: &Att, s: f32) -> f32 {
    let AttKind::Pdf { pages, view, .. } = &att.kind else { return 0.0 };
    let page_h = (if *view == ViewSize::Medium { PAGE_MEDIUM } else { PAGE_LARGE }) * s;
    let ws = widths(pages, page_h);
    page_x(&ws, ws.len().saturating_sub(1), PAGE_GAP * s)
}

/// Sideways scrolling over a PDF (a trackpad's sideways moves, Shift and the wheel): its pages
/// move, the note doesn't. True when the pointer is over one (the note then leaves Shift-wheel
/// to it).
pub fn scroll(ed: &mut Editor, ctx: &egui::Context) -> bool {
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return false };
    let Some(id) = ed.media.pdf_hits.iter().find(|h| h.1 == Hit::Band && h.2.contains(p)).map(|h| h.0.clone()) else { return false };
    let dx: f32 = ctx.input(|i| {
        i.raw.events.iter().map(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. } => {
                let k = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => 40.0,
                    egui::MouseWheelUnit::Page => 400.0,
                };
                (delta.x + if modifiers.shift { delta.y } else { 0.0 }) * k
            }
            _ => 0.0,
        }).sum()
    });
    if dx != 0.0 {
        let s = ed.settings.text_scale;
        let max = ed.doc.att(&id).map_or(0.0, |a| max_x(a, s));
        let st = ed.media.pdf.entry(id).or_default();
        st.x = (st.x - dx).clamp(0.0, max);
        st.target = st.x;
        ctx.request_repaint();
    }
    true
}

/// A press on a PDF's controls. True when it was one.
pub fn press(ed: &mut Editor, p: Pos2, clicks: u32) -> bool {
    let hit = ed.media.pdf_hits.iter().rev().find(|h| h.1 != Hit::Band && h.2.contains(p)).cloned();
    let Some((id, hit, rect)) = hit else { return false };
    match hit {
        Hit::Header => {
            ed.media.pdf_menu = Some((id, pos2(rect.min.x, rect.max.y + 2.0)));
            if let Some(ctx) = super::shapes::CTX.with_borrow(|c| c.clone()) {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("nt-pdf-menu-new"), true));
            }
        }
        Hit::Thumbs => {
            if let Some(AttKind::Pdf { thumbs, .. }) = ed.doc.att_mut(&id).map(|a| &mut a.kind) {
                *thumbs = !*thumbs;
            }
            ed.request_repaint = true;
        }
        Hit::Thumb(i) => {
            let s = ed.settings.text_scale;
            let x = ed.doc.att(&id).map_or(0.0, |a| match &a.kind {
                AttKind::Pdf { pages, view, .. } => {
                    let h = (if *view == ViewSize::Medium { PAGE_MEDIUM } else { PAGE_LARGE }) * s;
                    page_x(&widths(pages, h), i, PAGE_GAP * s)
                }
                _ => 0.0,
            });
            let st = ed.media.pdf.entry(id).or_default();
            st.target = x;
            st.page = i;
            ed.request_repaint = true;
        }
        Hit::Look => ed.media.quick_look = Some(id),
        // a double click on the pages: Quick Look; a single one selects the PDF (as any block)
        Hit::Pages if clicks >= 2 => ed.media.quick_look = Some(id),
        _ => return false,
    }
    true
}

/// The header's ⌄ menu, Rename's field and Quick Look (drawn over the editor).
pub fn overlays(ed: &mut Editor, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    if let Some((id, at)) = ed.media.pdf_menu.clone() {
        let Some(att) = ed.doc.att(&id).cloned() else {
            ed.media.pdf_menu = None;
            return;
        };
        let AttKind::Pdf { path, view, name, .. } = &att.kind else { return };
        let bytes = path.as_deref().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
        let mut close = false;
        let mut act: Option<Box<dyn FnOnce(&mut Editor)>> = None;
        let resp = egui::Area::new(egui::Id::unique(("nt-pdf-menu", &id))).order(egui::Order::Foreground).fade_in(false).fixed_pos(at).show(&ctx, |ui| {
            // (as the context menus look: egui's menu style, the window's fill)
            egui::containers::menu::menu_style(ui.style_mut());
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                // (a fixed width: View As's submenu stays where it opened)
                ui.set_width(210.0);
                ui.add_enabled(false, egui::Button::new(format!("PDF Document \u{00b7} {}", super::media::size_text(bytes))).frame(false).min_size(vec2(ui.available_width(), 0.0)));
                // (Markup of a PDF's pages isn't here yet)
                ui.add_enabled(false, egui::Button::new("Markup").frame(false).min_size(vec2(ui.available_width(), 0.0)));
                ui.separator();
                if ui.add(egui::Button::new("Quick Look Attachment").frame(false).min_size(vec2(ui.available_width(), 0.0))).clicked() {
                    let id = id.clone();
                    act = Some(Box::new(move |e: &mut Editor| e.media.quick_look = Some(id)));
                    close = true;
                }
                if ui.add_enabled(path.is_some(), egui::Button::new("Open Attachment").frame(false).min_size(vec2(ui.available_width(), 0.0))).clicked() {
                    if let Some(p) = path {
                        super::open_external(p);
                    }
                    close = true;
                }
                ui.separator();
                // View As ▸: a row that opens its submenu beside it while hovered
                let row = ui.add(egui::Button::new("View As").frame(false).min_size(vec2(ui.available_width(), 0.0)));
                ui.painter().text(pos2(row.rect.max.x - 6.0, row.rect.center().y), Align2::RIGHT_CENTER, "\u{25B8}", FontId::proportional(12.0), ui.visuals().text_color());
                let sub_id = egui::Id::unique(("nt-pdf-viewas", &id));
                let was_open = ui.data(|d| d.get_temp::<Rect>(sub_id));
                let pointer = ui.input(|i| i.pointer.hover_pos());
                // (open while hovered, or after a click on the row until the menu closes)
                let pinned_id = sub_id.with("pinned");
                if row.clicked() {
                    ui.data_mut(|d| d.insert_temp(pinned_id, true));
                }
                let pinned = ui.data(|d| d.get_temp::<bool>(pinned_id).unwrap_or(false));
                let keep = pinned || pointer.is_some_and(|p| row.rect.contains(p) || was_open.is_some_and(|r| r.expand(4.0).contains(p)));
                if keep {
                    // (beside the menu's right edge as drawn last frame: steady from its first frame)
                    let right = ui.data(|d| d.get_temp::<Rect>(egui::Id::unique(("nt-pdf-menu-rect", &id)))).map_or(row.rect.max.x + 8.0, |m| m.max.x);
                    let sub = egui::Area::new(sub_id).order(egui::Order::Foreground).fade_in(false).fixed_pos(pos2(right - 2.0, row.rect.min.y - 6.0)).show(ui.ctx(), |ui| {
                        egui::containers::menu::menu_style(ui.style_mut());
                        egui::Frame::menu(ui.style()).show(ui, |ui| {
                            ui.set_width(110.0);
                            for (v, label) in [(ViewSize::Small, "Small"), (ViewSize::Medium, "Medium"), (ViewSize::Large, "Large")] {
                                let mark = if *view == v { "\u{2713} " } else { "    " };
                                if ui.add(egui::Button::new(format!("{mark}{label}")).frame(false).min_size(vec2(ui.available_width(), 0.0))).clicked() {
                                    let id = id.clone();
                                    act = Some(Box::new(move |e: &mut Editor| e.pdf_view_as(&id, v)));
                                    close = true;
                                }
                            }
                        });
                    });
                    ui.data_mut(|d| d.insert_temp(sub_id, sub.response.rect));
                } else {
                    ui.data_mut(|d| d.remove::<Rect>(sub_id));
                }
                if ui.add(egui::Button::new("Rename Attachment\u{2026}").frame(false).min_size(vec2(ui.available_width(), 0.0))).clicked() {
                    let (id, name) = (id.clone(), name.clone());
                    act = Some(Box::new(move |e: &mut Editor| e.media.pdf_rename = Some((id, name))));
                    close = true;
                }
                ui.separator();
                if ui.add(egui::Button::new("Copy").frame(false).min_size(vec2(ui.available_width(), 0.0))).clicked() {
                    let id = id.clone();
                    act = Some(Box::new(move |e: &mut Editor| {
                        e.select_block(&id);
                        e.command_label(None, "Copy");
                    }));
                    close = true;
                }
                // (no share sheet on Linux)
                ui.add_enabled(false, egui::Button::new("Share\u{2026}").frame(false).min_size(vec2(ui.available_width(), 0.0)));
                ui.separator();
                if ui.add(egui::Button::new("Delete").frame(false).min_size(vec2(ui.available_width(), 0.0))).clicked() {
                    let id = id.clone();
                    act = Some(Box::new(move |e: &mut Editor| {
                        e.select_block(&id);
                        e.command_label(None, "Delete");
                    }));
                    close = true;
                }
            });
        });
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("nt-pdf-menu-rect", &id)), resp.response.rect));
        // (the press that opened it isn't one outside it)
        let opened_now = ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("nt-pdf-menu-new"))).unwrap_or(false);
        let sub = ctx.data(|d| d.get_temp::<Rect>(egui::Id::unique(("nt-pdf-viewas", &id))));
        let outside = !opened_now && ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !resp.response.rect.contains(p) && !sub.is_some_and(|r| r.contains(p))));
        if close || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ed.media.pdf_menu = None;
            ctx.data_mut(|d| d.remove::<bool>(egui::Id::unique(("nt-pdf-viewas", &id)).with("pinned")));
        }
        if let Some(f) = act {
            f(ed);
        }
    }
    // Rename Attachment…: a field over the header
    if let Some((id, mut text)) = ed.media.pdf_rename.clone() {
        // (over a PDF's header; over the top of a picture or card)
        let block = ed.doc.paras.iter().position(|p| p.sole_att() == Some(id.as_str())).and_then(|i| ed.block_rect(i));
        let at = ed.media.pdf_hits.iter().find(|h| h.0 == id && h.1 == Hit::Header).map(|h| h.2.min).or(block.map(|r| r.min + vec2(8.0, 8.0))).unwrap_or(pos2(100.0, 100.0));
        let mut done = None;
        egui::Area::new(egui::Id::unique(("nt-pdf-rename", &id))).order(egui::Order::Foreground).fixed_pos(at).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut super::find::SafeText(&mut text)).desired_width(220.0));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                ed.media.pdf_rename = None;
                let t = text.trim().to_string();
                if !t.is_empty() {
                    ed.push_undo(super::ops::Kind::Other);
                    match ed.doc.att_mut(&id).map(|a| &mut a.kind) {
                        Some(AttKind::Pdf { name, .. } | AttKind::Image { name, .. } | AttKind::File { name, .. }) => *name = t,
                        _ => {}
                    }
                    ed.changed = true;
                }
            }
            Some(false) => ed.media.pdf_rename = None,
            None => ed.media.pdf_rename = Some((id, text)),
        }
    }
    quick_look(ed, ui);
}

/// Quick Look (NO3 2f): a dark panel — close, the name, Open — the pages down it, their
/// thumbnails beside them, the one in view outlined. Esc closes it.
fn quick_look(ed: &mut Editor, ui: &mut egui::Ui) {
    let Some(id) = ed.media.quick_look.clone() else { return };
    let Some(AttKind::Pdf { path, name, pages, .. }) = ed.doc.att(&id).map(|a| a.kind.clone()) else {
        ed.media.quick_look = None;
        return;
    };
    let ctx = ui.ctx().clone();
    let v = ed.view.rect.unwrap_or(ctx.content_rect());
    let size = vec2(758.0f32.min(v.width() - 24.0), 854.0f32.min(v.height() - 24.0));
    let panel = Rect::from_center_size(v.center(), size);
    let pages: Vec<[f32; 2]> = if pages.is_empty() { vec![[612.0, 792.0]] } else { pages };
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut goto: Option<usize> = None;
    let sid = egui::Id::unique(("nt-ql-scroll", &id));
    egui::Area::new(egui::Id::unique(("nt-ql", &id))).order(egui::Order::Foreground).fixed_pos(panel.min).show(&ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(size, Sense::click());
        let p = ui.painter().clone();
        p.rect_filled(r.expand(1.0), CornerRadius::same(16), Color32::from_black_alpha(60));
        p.rect_filled(r, CornerRadius::same(15), Color32::from_gray(106));
        // the title bar
        let xc = pos2(r.min.x + 18.0, r.min.y + 19.0);
        let xr = ui.interact(Rect::from_center_size(xc, vec2(16.0, 16.0)), egui::Id::unique(("nt-ql-x", &id)), Sense::click());
        p.circle_filled(xc, 8.0, Color32::from_gray(if xr.hovered() { 20 } else { 40 }));
        p.text(xc, Align2::CENTER_CENTER, "\u{00d7}", FontId::proportional(13.0), Color32::from_gray(200));
        if xr.clicked() {
            close = true;
        }
        p.text(pos2(r.min.x + 52.0, r.min.y + 19.0), Align2::LEFT_CENTER, &name, FontId::new(13.0, layout::family("Sfera-Bold")), Color32::from_gray(30));
        let ob = Rect::from_min_max(pos2(r.max.x - 145.0, r.min.y + 7.0), pos2(r.max.x - 9.0, r.min.y + 31.0));
        let or = ui.interact(ob, egui::Id::unique(("nt-ql-open", &id)), Sense::click());
        p.rect_filled(ob, CornerRadius::same(12), Color32::from_gray(if or.hovered() { 140 } else { 125 }));
        p.text(ob.center(), Align2::CENTER_CENTER, "Open", FontId::new(13.0, layout::family("Sfera-Regular")), Color32::from_gray(30));
        if or.clicked() {
            if let Some(f) = &path {
                super::open_external(f);
            }
        }
        // the pages down the left, the thumbnails down the right
        let body = Rect::from_min_max(pos2(r.min.x + 5.0, r.min.y + 38.0), pos2(r.max.x - 125.0, r.max.y - 5.0));
        let side = Rect::from_min_max(pos2(r.max.x - 117.0, r.min.y + 38.0), pos2(r.max.x - 7.0, r.max.y - 5.0));
        let pw = body.width();
        let hs: Vec<f32> = pages.iter().map(|p| pw * p[1] / p[0].max(1.0)).collect();
        let tops: Vec<f32> = hs.iter().scan(0.0, |y, h| {
            let t = *y;
            *y += h + 8.0;
            Some(t)
        }).collect();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        let jump = ctx.data_mut(|d| d.remove_temp::<f32>(sid.with("goto")));
        let mut area = egui::ScrollArea::vertical().id_salt(sid).auto_shrink([false, false]);
        if let Some(y) = jump {
            area = area.vertical_scroll_offset(y);
        }
        let out = area.show(&mut child, |ui| {
            for (i, h) in hs.iter().enumerate() {
                let (pr, _) = ui.allocate_exact_size(vec2(pw, *h), Sense::hover());
                match path.as_deref().and_then(|f| ed.media.page(&ctx, f, i, PAGE_PX)) {
                    Some((t, _)) => {
                        ui.painter().add(egui::epaint::RectShape::filled(pr, CornerRadius::same(if i == 0 { 12 } else { 0 }), Color32::WHITE).with_texture(t, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))));
                    }
                    None => {
                        ui.painter().rect_filled(pr, 0.0, Color32::WHITE);
                    }
                }
                ui.add_space(8.0);
            }
        });
        let y = out.state.clamped_offset().y;
        let shown = tops.iter().rposition(|t| *t <= y + body.height() * 0.4).unwrap_or(0);
        let mut ty = side.min.y;
        for (i, pg) in pages.iter().enumerate() {
            let tw = side.width();
            let th = tw * pg[1] / pg[0].max(1.0);
            let tr = Rect::from_min_size(pos2(side.min.x, ty), vec2(tw, th));
            let resp = ui.interact(tr, egui::Id::unique(("nt-ql-t", &id, i)), Sense::click());
            if i == shown {
                p.rect_filled(tr.expand(3.0), CornerRadius::same(6), Color32::from_gray(225));
            }
            match path.as_deref().and_then(|f| ed.media.page(&ctx, f, i, PAGE_PX / 4)) {
                Some((t, _)) => {
                    p.add(egui::epaint::RectShape::filled(tr, CornerRadius::same(4), Color32::WHITE).with_texture(t, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))));
                }
                None => {
                    p.rect_filled(tr, CornerRadius::same(4), Color32::WHITE);
                }
            }
            if resp.clicked() {
                goto = Some(i);
            }
            ty += th + 22.0;
        }
        if let Some(i) = goto {
            // (the pages jump there on the next frame)
            let y = tops[i];
            ctx.data_mut(|d| d.insert_temp(sid.with("goto"), y));
            ctx.request_repaint();
        }
    });
    if close {
        ed.media.quick_look = None;
    }
}

impl Editor {
    /// The attachment block the selection is on (or the caret beside).
    pub fn selected_block(&self) -> Option<String> {
        if self.head.cell.is_some() {
            return None;
        }
        self.doc.block_of(self.doc.paras.get(self.head.para)?).map(|a| a.id.clone())
    }

    /// View As for a picture or PDF (pictures: Small or Large).
    pub fn view_as(&mut self, id: &str, v: ViewSize) {
        match self.doc.att(id).map(|a| &a.kind) {
            Some(AttKind::Pdf { .. }) => self.pdf_view_as(id, v),
            Some(AttKind::Image { .. }) => {
                self.push_undo(super::ops::Kind::Other);
                if let Some(AttKind::Image { view, .. }) = self.doc.att_mut(id).map(|a| &mut a.kind) {
                    *view = if v == ViewSize::Small { ViewSize::Small } else { ViewSize::Large };
                }
                self.changed = true;
            }
            _ => {}
        }
    }

    /// View > Attachment View > Set All to Small / Large: every picture and PDF (one undo step).
    pub fn set_all_views(&mut self, v: ViewSize) {
        self.push_undo(super::ops::Kind::Other);
        for a in &mut self.doc.atts {
            if let AttKind::Image { view, .. } | AttKind::Pdf { view, .. } = &mut a.kind {
                *view = v;
            }
        }
        self.changed = true;
    }

    /// Rename Attachment…: the name field over it.
    pub fn start_rename(&mut self, id: &str) {
        let name = match self.doc.att(id).map(|a| &a.kind) {
            Some(AttKind::Pdf { name, .. }) => name.strip_suffix(".pdf").unwrap_or(name).to_string(),
            Some(AttKind::Image { name, path, .. }) => if name.is_empty() { path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name.clone() },
            Some(AttKind::File { name, .. }) => name.clone(),
            _ => return,
        };
        self.media.pdf_rename = Some((id.to_string(), name));
    }

    /// View As ▸ Small / Medium / Large (an undo step; the page shown is kept).
    pub fn pdf_view_as(&mut self, id: &str, v: ViewSize) {
        self.push_undo(super::ops::Kind::Other);
        if let Some(AttKind::Pdf { view, .. }) = self.doc.att_mut(id).map(|a| &mut a.kind) {
            *view = v;
        }
        // (the page shown stays the page shown at the new size)
        let s = self.settings.text_scale;
        if let (Some(att), Some(st)) = (self.doc.att(id).cloned(), self.media.pdf.get_mut(id)) {
            if let AttKind::Pdf { pages, view, .. } = &att.kind {
                let h = (if *view == ViewSize::Medium { PAGE_MEDIUM } else { PAGE_LARGE }) * s;
                st.x = page_x(&widths(pages, h), st.page, PAGE_GAP * s);
                st.target = st.x;
            }
        }
        self.changed = true;
    }

    /// Selects the block that is attachment `id`.
    pub fn select_block(&mut self, id: &str) {
        if let Some(i) = self.doc.paras.iter().position(|p| p.sole_att() == Some(id)) {
            let n = self.doc.paras[i].len();
            self.anchor = super::Loc { cell: None, para: i, off: 0 };
            self.head = super::Loc { cell: None, para: i, off: n };
        }
    }
}
