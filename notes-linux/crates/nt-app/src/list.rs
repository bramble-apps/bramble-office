//! The note list (Notes 4.13, ui-list-view-light.png) and gallery (ui-gallery-view-light.png).
//!
//! List: section headers (Pinned, Today, Yesterday, …; bold 15 pt) over 56 pt rows — the title in
//! bold 13, then the date and the snippet in 12 (the snippet grey), the folder's name in the
//! All-views, a 44 pt thumbnail on the right; the selected row a rounded yellow (grey when the list
//! hasn't the keyboard). Gallery: 182 × 135 pt cards with a miniature of the note, its title and
//! date under it.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use nt_core::model::{Note, ParaStyle};

use crate::app::{App, Focus, Place, Thumb};
use crate::icons::{draw, Glyph};
use crate::look;

const ROW: f32 = 56.0;
/// A row with the folder's name under it (the All-views, Smart Folders, tags, search).
const ROW_FOLDER: f32 = 71.0;
const HEADER: f32 = 40.0;

pub(crate) fn note_title(n: &Note) -> String {
    if n.title.trim().is_empty() { "New Note".into() } else { n.title.clone() }
}

fn snippet(n: &Note) -> String {
    if !n.snippet.is_empty() || n.body.paragraphs.is_empty() {
        return n.snippet.clone();
    }
    n.body.paragraphs.iter().skip(1).map(|p| p.text().replace(nt_core::model::OBJ, "")).find(|t| !t.trim().is_empty()).unwrap_or_else(|| "No additional text".into())
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    // (a finger scrolls the list: kn_ui::touch)
    kn_ui::touch::pan_area(area);
    let ctx = ui.ctx().clone();
    let now = crate::dates::now();
    let sections = app.sections();
    // (Mac: yellow only while the list has focus in the active window, else grey; NT_INACTIVE=1
    // shows the inactive window in snapshots)
    let active = if app.automated { std::env::var_os("NT_INACTIVE").is_none() } else { ctx.input(|i| i.viewport().focused.unwrap_or(true)) };
    let focused = app.focus == Focus::List && active;
    let all_view = matches!(app.place, Place::All(_) | Place::Tag(_)) || !app.search.trim().is_empty() || app.folder(match &app.place {
        Place::Folder(f) => f,
        _ => "",
    }).is_some_and(App::is_smart);
    let sort = app.sort();
    let mut clicked: Option<String> = None;
    let mut action: Option<String> = None;
    let scroll_to = std::mem::take(&mut app.list_scroll_to);
    let mut bg_ctx = false;
    let mut sc = std::mem::take(&mut app.list_scroll);
    let mut want: Option<Rect> = None;
    // (a horizontal swipe over a row: taken before the list scrolls)
    if let Some(a) = crate::swipe::input(app, &ctx, area) {
        action = Some(a);
    }
    // the rows' and headers' slides (insert, delete, re-sort), keyed to what the list shows
    let list_key = format!("{:?}|{}|{}|{}", app.place, app.search, sort, app.pinned_collapsed);
    let mut glide = Glide::take(&ctx, &list_key);
    // Pinned's chevron shows while the pointer is over the list (Mac, list-pin-note and the
    // no3-sticky recordings: there whenever the pointer was in the list, not only on the header)
    let chevron_t = ctx.animate_bool_with_time(egui::Id::new("pinned-chevron"), ui.rect_contains_pointer(area), 0.15);
    crate::app::scroll_area(&mut sc, ui, "list", |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let w = area.width();
        let top = ui.cursor().top();
        // the section headers' places (for the one held at the top) and the selected row's
        let mut heads: Vec<(String, Rect)> = Vec::new();
        let mut sel_row: Option<Rect> = None;
        // (each row as drawn, and its thumbnail's square: what the held header blurs)
        let mut drawn: Vec<(Rect, bool)> = Vec::new();
        if matches!(app.place, Place::Deleted(_)) {
            let (r, _) = ui.allocate_exact_size(vec2(w, 46.0), Sense::hover());
            ui.painter().text(pos2(r.min.x + 16.0, r.center().y), Align2::LEFT_CENTER, "Notes are deleted after 30 days.", look::regular(11.0), look::text_dim());
        }
        if sections.is_empty() {
            let (r, _) = ui.allocate_exact_size(vec2(w, 200.0), Sense::hover());
            // (while the library opens: what nt-core is doing, a conversion's progress)
            let what = if app.loading() { nt_core::native::opening().message().unwrap_or_else(|| "Loading…".into()) } else if app.search.trim().is_empty() { "No Notes".into() } else { "No Results".into() };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, what, look::regular(18.0), look::text_dim());
        }
        // the list's order once (which row comes next, to hide the line beside the selection)
        let order: Vec<&String> = sections.iter().filter(|(h, _)| !(h == "Pinned" && app.pinned_collapsed)).flat_map(|(_, v)| v.iter()).collect();
        let next_of: std::collections::HashMap<&str, &str> = order.windows(2).map(|w| (w[0].as_str(), w[1].as_str())).collect();
        // (only the rows in view, with some to spare, are drawn; the rest just keep their place)
        let near = ui.clip_rect().expand2(vec2(0.0, 300.0));
        for (header, ids) in &sections {
            let pinned = header == "Pinned";
            if !header.is_empty() {
                let (r, resp) = ui.allocate_exact_size(vec2(w, HEADER), Sense::click());
                heads.push((header.clone(), r));
                let r = r.translate(vec2(0.0, glide.offset(&format!("h:{header}"), r.min.y - top)));
                let p = ui.painter();
                p.text(pos2(r.min.x + 16.0, r.min.y + 29.0), Align2::LEFT_BOTTOM, header, look::bold(15.0), look::text());
                // (Mac, NO2 list-gap at 2x: the header's last point is its line, 1 pt of #E6E6E6
                // from 15 pt in to where the rows' lines end, the first row starting right under
                // it; while the first row is selected the line isn't drawn at all)
                let first_selected = ids.first().is_some_and(|f| app.note.as_deref() == Some(f.as_str())) && !(pinned && app.pinned_collapsed);
                if !first_selected {
                    p.hline(r.min.x + 15.0..=r.max.x - 16.0, r.max.y - 0.5, Stroke::new(1.0, look::line()));
                }
                if pinned {
                    pinned_chevron(p, r, app.pinned_collapsed, chevron_t);
                }
                if pinned && resp.clicked() {
                    app.pinned_collapsed = !app.pinned_collapsed;
                }
                if pinned && app.pinned_collapsed {
                    // (Mac, list-pin-note: a collapsed section's line, then 15 pt before the next
                    // section's header)
                    ui.add_space(15.0);
                    continue;
                }
            }
            for id in ids.iter() {
                let row_h = if all_view { ROW_FOLDER } else { ROW };
                let selected = app.is_selected(id);
                let ahead = Rect::from_min_size(ui.cursor().min, vec2(w, row_h));
                if !ahead.intersects(near) {
                    if selected && scroll_to {
                        want = Some(ahead);
                    }
                    ui.add_space(row_h);
                    continue;
                }
                let Some(n) = app.note_by_id(id).cloned() else { continue };
                // (dragged onto a folder in the sidebar: moved there)
                let (r, resp) = ui.allocate_exact_size(vec2(w, row_h), Sense::click_and_drag());
                resp.dnd_set_drag_payload(crate::app::NoteDrag(id.clone()));
                if resp.dragged() {
                    if let Some(at) = ui.ctx().pointer_interact_pos() {
                        let lp = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("note-drag")));
                        let g = lp.layout_no_wrap(note_title(&n), look::bold(12.0), look::text());
                        let pill = Rect::from_min_size(at + vec2(12.0, 6.0), g.size() + vec2(20.0, 10.0));
                        lp.rect_filled(pill, 8.0, look::content_bg());
                        lp.rect_stroke(pill, 8.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
                        lp.galley(pill.min + vec2(10.0, 5.0), g, look::text());
                    }
                }
                if selected && scroll_to {
                    want = Some(r);
                }
                // (drawn where it is on its way; clicks go to where it's going)
                let r = r.translate(vec2(0.0, glide.offset(id, r.min.y - top)));
                // (swiped: its buttons in the space it leaves, the row slid aside)
                app.swipe.drawn(id, r);
                if let Some(a) = crate::swipe::paint(app, ui, id, r) {
                    action = Some(a);
                }
                let r = r.translate(vec2(app.swipe.offset(id), 0.0));
                // (a slid row stays in its column)
                let p = ui.painter().with_clip_rect(ui.clip_rect().intersect(area));
                // (Mac, NO2 list-gap at 2x: the highlight fills the row's whole 56 pt, from right
                // under the line above it (the header's or the row's, neither drawn then), 9 pt in
                // from the column's left and 10 pt short of the divider, corners of 8 pt)
                let row = Rect::from_min_max(pos2(r.min.x + 9.0, r.min.y), pos2(r.max.x - 10.0, r.max.y));
                if selected {
                    sel_row = Some(row);
                }
                drawn.push((r, app.thumb_of(&n).is_some()));
                if selected {
                    p.rect_filled(row, 8.0, look::selection(focused));
                }
                // the line under each row, inset to the text: not next to the selection, and not under
                // a section's last row (the next header has its own line)
                let next_selected = next_of.get(id.as_str()).is_some_and(|x| app.is_selected(x));
                let last_in_section = ids.last() == Some(id);
                if !selected && !next_selected && !last_in_section {
                    p.hline(r.min.x + 34.0..=r.max.x - 16.0, r.max.y - 0.5, Stroke::new(1.0, look::line()));
                }
                let thumb = app.thumb_of(&n);
                if thumb.is_none() {
                    // (its picture gone: no old one shown again later)
                    app.thumb_tex.remove(id.as_str());
                }
                let tx = r.min.x + 34.0;
                // (Mac: the title runs to 20 pt from the edge; the second line stops 31 pt from it,
                // or 21 pt before the thumbnail)
                let room = r.max.x - if thumb.is_some() { 72.0 } else { 20.0 } - tx;
                let room2 = r.max.x - if thumb.is_some() { 85.0 } else { 31.0 } - tx;
                let title = elide(&p, &note_title(&n), look::bold(13.0), room);
                let lock = "";
                text_with_emoji(&p, pos2(tx, r.min.y + 28.0), &format!("{lock}{title}"), look::bold(13.0), look::text());
                let t = if sort == 1 { n.created } else { n.modified };
                let date = crate::dates::short(t, now);
                let dg = p.layout_no_wrap(date, look::regular(12.0), look::text());
                let dw = dg.size().x;
                p.galley(pos2(tx, r.min.y + 43.0 - dg.size().y), dg, look::text());
                let snip = if n.locked { "Locked".to_string() } else { snippet(&n) };
                let sn = elide(&p, &snip, look::regular(12.0), room2 - dw - 7.0);
                text_with_emoji(&p, pos2(tx + dw + 7.0, r.min.y + 43.0), &sn, look::regular(12.0), look::text_dim());
                if all_view {
                    // the folder: a small folder icon and its name, under the date
                    if let Some(f) = app.folder(&n.folder) {
                        // (the same 15 pt below the date line as that is below the title)
                        draw(&p, Rect::from_center_size(pos2(tx + 6.0, r.min.y + 53.5), vec2(13.0, 13.0)), Glyph::Folder, look::text_dim());
                        p.text(pos2(tx + 16.0, r.min.y + 58.0), Align2::LEFT_BOTTOM, f.name.clone(), look::regular(12.0), look::text_dim());
                    }
                }
                if n.locked {
                    // the lock where a thumbnail would be
                    draw(&p, Rect::from_center_size(pos2(r.max.x - 20.0 - 11.0, r.min.y + ROW / 2.0), vec2(20.0, 20.0)), Glyph::Lock, look::text_dim());
                }
                if let Some(th) = thumb.filter(|_| !n.locked) {
                    let tr = Rect::from_center_size(pos2(r.max.x - 20.0 - 22.0, r.min.y + ROW / 2.0 + if all_view { 8.0 } else { 0.0 }), vec2(44.0, 44.0));
                    paint_thumb(app, &ctx, &p, &n.id, &th, tr);
                }
                if resp.clicked() {
                    // (an open row shuts at a click, as on the iPhone; ⇧-click selects a range,
                    // ⌘-click (Ctrl here) adds or takes out one note)
                    let mods = ui.input(|i| i.modifiers);
                    if app.swipe.is_open(id) {
                        app.swipe.close();
                    } else if mods.shift {
                        app.pick_range(id);
                        app.focus = Focus::List;
                    } else if mods.command {
                        app.toggle_pick(id);
                        app.focus = Focus::List;
                    } else {
                        clicked = Some(id.clone());
                    }
                }
                if resp.double_clicked() && !app.swipe.is_open(id) {
                    action = Some("cmd:Open Note in New Window".into());
                }
                crate::menus::context_menu(&resp, resp.id.with("ctx"), |ui| {
                    if let Some(a) = crate::menus::show_items(ui, &crate::menus::note_menu(app, id)) {
                        action = Some(a);
                    }
                });
                if resp.secondary_clicked() {
                    clicked = Some(id.clone());
                }
            }
        }
        let rest = ui.available_rect_before_wrap();
        if rest.height() > 0.0 {
            let resp = ui.allocate_rect(rest, Sense::click());
            crate::menus::context_menu(&resp, resp.id.with("ctx"), |ui| {
                if ui.button("New Note").clicked() {
                    bg_ctx = true;
                    ui.close();
                }
            });
            if resp.clicked() {
                app.focus = Focus::List;
            }
        }
        sticky_header(app, ui, area, &heads, sel_row, &drawn, focused, if app.show_toolbar { crate::toolbar::HEIGHT } else { 0.0 });
    });
    glide.keep(&ctx);
    if let Some(r) = want {
        crate::app::reveal(&mut sc, area, r);
    }
    app.list_scroll = sc;
    if let Some(a) = crate::swipe::menus(app, &ctx) {
        action = Some(a);
    }
    if let Some(id) = clicked {
        app.select_note(&id);
        app.focus = Focus::List;
    }
    if bg_ctx {
        app.new_note();
    }
    if let Some(a) = action {
        crate::menus::menu_action(app, &ctx, &a);
    }
}

pub fn paint_thumb(app: &mut App, ctx: &egui::Context, p: &egui::Painter, id: &str, th: &Thumb, r: Rect) {
    let border = Stroke::new(1.0, if kn_ui::theme::is_light() { Color32::from_gray(0xe0) } else { Color32::from_gray(0x44) });
    match th {
        Thumb::Image(_) | Thumb::File(_) => {
            if let Some(tex) = app.thumb_texture(ctx, id, th) {
                // fill the square (cropped to it)
                let [tw, tht] = tex.size();
                let (tw, tht) = (tw as f32, tht as f32);
                let s = (tw / tht).max(0.01);
                let uv = if s > 1.0 { Rect::from_min_max(pos2((1.0 - 1.0 / s) / 2.0, 0.0), pos2((1.0 + 1.0 / s) / 2.0, 1.0)) } else { Rect::from_min_max(pos2(0.0, (1.0 - s) / 2.0), pos2(1.0, (1.0 + s) / 2.0)) };
                p.add(egui::epaint::RectShape::filled(r, 3.0, Color32::WHITE).with_texture(tex.id(), uv));
            } else {
                p.rect_filled(r, 3.0, look::content_bg());
            }
        }
        Thumb::Pdf => {
            p.rect_filled(r, 3.0, Color32::WHITE);
            p.hline(r.center().x - 5.0..=r.center().x + 5.0, r.center().y, Stroke::new(1.0, Color32::from_gray(0x90)));
        }
        Thumb::Drawing => {
            p.rect_filled(r, 3.0, Color32::WHITE);
            p.line_segment([r.lerp_inside(vec2(0.3, 0.3)), r.lerp_inside(vec2(0.55, 0.4))], Stroke::new(0.8, Color32::from_gray(0xa0)));
            p.circle_stroke(r.lerp_inside(vec2(0.4, 0.55)), 2.5, Stroke::new(0.8, Color32::from_gray(0xa0)));
        }
    }
    p.rect_stroke(r, 3.0, border, egui::StrokeKind::Inside);
}

/// Whether `c` is drawn from the colour emoji font (emoji, their modifiers and joiners).
fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE0F | 0x200D | 0x1D100..=0x1D1FF)
}

/// Text with its bottom-left at `at`; emoji in their own colours (egui multiplies a colour glyph
/// by the text colour, which would grey them in a grey snippet).
pub fn text_with_emoji(p: &egui::Painter, at: egui::Pos2, s: &str, font: egui::FontId, col: Color32) {
    let g = emoji_galley(p, s, font, col);
    p.galley(pos2(at.x, at.y - g.size().y), g, col);
}

/// `s` laid out with its emoji in colour at any weight: runs that must be emoji (pictographs, a
/// text character with VS16 such as ❤️, sequences) from the suite's "emoji" family, untinted.
pub fn emoji_galley(p: &egui::Painter, s: &str, font: egui::FontId, col: Color32) -> std::sync::Arc<egui::Galley> {
    let emoji_font = kn_ui::theme::has_color_emoji().then(|| egui::FontId::new(font.size, egui::FontFamily::Name("emoji".into())));
    let mut job = egui::text::LayoutJob::default();
    let mut run = String::new();
    let mut run_emoji = false;
    let flush = |job: &mut egui::text::LayoutJob, run: &mut String, emoji: bool| {
        if !run.is_empty() {
            let c = if emoji && !(0x1D100..=0x1D1FF).contains(&(run.chars().next().unwrap() as u32)) { Color32::WHITE } else { col };
            let f = if emoji { emoji_font.clone().unwrap_or_else(|| font.clone()) } else { font.clone() };
            job.append(run, 0.0, egui::TextFormat { font_id: f, color: c, ..Default::default() });
            run.clear();
        }
    };
    let chars: Vec<char> = s.chars().collect();
    for (k, &ch) in chars.iter().enumerate() {
        let e = kn_ui::theme::is_emoji_char(ch, chars.get(k + 1).copied()) || (is_emoji(ch) && run_emoji);
        if e != run_emoji {
            flush(&mut job, &mut run, run_emoji);
            run_emoji = e;
        }
        run.push(ch);
    }
    flush(&mut job, &mut run, run_emoji);
    p.layout_job(job)
}

pub fn elide(p: &egui::Painter, s: &str, font: egui::FontId, room: f32) -> String {
    // (only what could ever fit is laid out, once: a long snippet used to be laid out again for
    // every character dropped, 70 ms for one row)
    let most = ((room / (font.size * 0.2)).ceil() as usize).max(4) + 4;
    let line = s.lines().next().unwrap_or("");
    let cut_short = s.chars().count() > most || line.len() < s.len();
    let shown: String = line.chars().take(most).collect();
    let g = p.layout_no_wrap(shown.clone(), font.clone(), Color32::WHITE);
    if !cut_short && g.size().x <= room {
        return shown;
    }
    let dots = p.layout_no_wrap("...".to_string(), font, Color32::WHITE).size().x;
    let fit = g.rows.first().map(|r| r.glyphs.iter().take_while(|gl| gl.pos.x + gl.advance_width <= room - dots).count()).unwrap_or(0);
    if fit == 0 && !shown.is_empty() {
        return shown.chars().take(1).collect();
    }
    format!("{}...", shown.chars().take(fit).collect::<String>().trim_end())
}

/// Gallery view: the cards in rows, sections as in the list.
pub fn gallery(app: &mut App, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    kn_ui::touch::pan_area(area);
    let ctx = ui.ctx().clone();
    let now = crate::dates::now();
    let sections = app.sections();
    let sort = app.sort();
    let mut clicked: Option<String> = None;
    let mut opened: Option<String> = None;
    let mut action: Option<String> = None;
    const CW: f32 = 182.0;
    const CH: f32 = 135.0;
    const PITCH_X: f32 = 199.0;
    const PITCH_Y: f32 = 212.0;
    let mut sc = std::mem::take(&mut app.list_scroll);
    crate::app::scroll_area(&mut sc, ui, "gallery", |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let w = area.width();
        let cols = (((w - 15.0) + (PITCH_X - CW)) / PITCH_X).floor().max(1.0) as usize;
        for (header, ids) in &sections {
            // (Mac: the header's middle 11 pt below the section's top, the cards 35 pt below it,
            // 12 pt more after the last row)
            let rows = ids.len().div_ceil(cols);
            let top = if header.is_empty() { 0.0 } else { 35.0 };
            let (block, _) = ui.allocate_exact_size(vec2(w, top + rows as f32 * PITCH_Y + 12.0), Sense::hover());
            if !header.is_empty() {
                ui.painter().text(pos2(block.min.x + 15.0, block.min.y + 11.0), Align2::LEFT_CENTER, header, look::bold(15.0), look::text());
            }
            let clip = ui.clip_rect();
            for (k, id) in ids.iter().enumerate() {
                let (c, row) = (k % cols, k / cols);
                let card = Rect::from_min_size(pos2(block.min.x + 15.0 + c as f32 * PITCH_X, block.min.y + top + row as f32 * PITCH_Y), vec2(CW, CH));
                // (only the cards in view, with a row's margin: a big folder has thousands)
                if card.max.y + 50.0 + PITCH_Y < clip.min.y || card.min.y - PITCH_Y > clip.max.y {
                    continue;
                }
                let Some(n) = app.note_by_id(id).cloned() else { continue };
                let resp = ui.interact(card.union(Rect::from_min_size(card.left_bottom(), vec2(CW, 50.0))), ui.id().with(("card", id)), Sense::click());
                let p = ui.painter().clone();
                let selected = app.is_selected(id);
                p.rect_filled(card, 8.0, look::content_bg());
                p.rect_stroke(card, 8.0, Stroke::new(1.0, if kn_ui::theme::is_light() { Color32::from_gray(0xdd) } else { Color32::from_gray(0x40) }), egui::StrokeKind::Inside);
                miniature(app, &ctx, &p, &n, card.shrink2(vec2(9.0, 8.0)));
                if n.pinned {
                    let b = Rect::from_min_size(pos2(card.max.x - 32.0, card.min.y + 8.0), vec2(23.0, 23.0));
                    p.rect_filled(b, 5.0, if kn_ui::theme::is_light() { Color32::from_gray(0xe4) } else { Color32::from_gray(0x44) });
                    draw(&p, b.shrink(4.0), Glyph::Pin, look::text_dim());
                }
                if selected {
                    p.rect_stroke(card.expand(2.0), 10.0, Stroke::new(3.0, look::accent()), egui::StrokeKind::Middle);
                }
                p.text(pos2(card.center().x, card.max.y + 19.0), Align2::CENTER_CENTER, elide(&p, &note_title(&n), look::regular(13.0), CW + 10.0), look::regular(13.0), look::text());
                let t = if sort == 1 { n.created } else { n.modified };
                p.text(pos2(card.center().x, card.max.y + 35.5), Align2::CENTER_CENTER, crate::dates::short(t, now), look::regular(13.0), look::text_dim());
                if resp.clicked() {
                    clicked = Some(id.clone());
                }
                if resp.double_clicked() {
                    opened = Some(id.clone());
                }
                crate::menus::context_menu(&resp, resp.id.with("ctx"), |ui| {
                    if let Some(a) = crate::menus::show_items(ui, &crate::menus::note_menu(app, id)) {
                        action = Some(a);
                    }
                });
            }
        }
    });
    app.list_scroll = sc;
    if let Some(id) = clicked {
        app.select_note(&id);
        app.focus = Focus::List;
    }
    if let Some(id) = opened {
        app.select_note(&id);
        app.gallery_open = true;
        app.focus = Focus::Editor;
    }
    if let Some(a) = action {
        crate::menus::menu_action(app, &ctx, &a);
    }
}

/// A card's miniature: the note's first lines at a small size (title bold), and its first image.
fn miniature(app: &mut App, ctx: &egui::Context, p: &egui::Painter, n: &Note, r: Rect) {
    let p = p.with_clip_rect(r);
    let mut y = r.min.y + 1.0;
    let paras: Vec<nt_core::model::Paragraph> = if n.body.paragraphs.is_empty() {
        vec![nt_core::model::Paragraph::plain(ParaStyle::Title, &n.title), nt_core::model::Paragraph::plain(ParaStyle::Body, &n.snippet)]
    } else {
        n.body.paragraphs.clone()
    };
    for (k, q) in paras.iter().enumerate() {
        let font = match q.style {
            ParaStyle::Title => look::bold(10.5),
            ParaStyle::Heading | ParaStyle::Subheading => look::bold(8.0),
            ParaStyle::Monostyled => egui::FontId::monospace(6.5),
            _ => look::regular(7.0),
        };
        let prefix = match q.style {
            ParaStyle::Bulleted => " •  ",
            ParaStyle::Dashed => " –  ",
            ParaStyle::Numbered => " 1. ",
            ParaStyle::Checklist => if q.checked { "◉ " } else { "○ " },
            _ => "",
        };
        if q.text().replace(nt_core::model::OBJ, "").is_empty() && k > 0 {
            y += 5.0;
            continue;
        }
        // highlights and links in their colours, as the Mac's miniatures
        let mut job = egui::text::LayoutJob::default();
        let fmt = |c: Color32, u: bool| egui::TextFormat { font_id: font.clone(), color: c, underline: if u { Stroke::new(0.6, c) } else { Stroke::NONE }, ..Default::default() };
        job.append(prefix, 0.0, fmt(look::text(), false));
        for sp in &q.spans {
            let t = sp.text.replace(nt_core::model::OBJ, "");
            let c = match (sp.highlight, &sp.link) {
                (Some(h), _) => kn_ui::theme::rgb([0xb44ad8, 0xe0436b, 0xf0913a, 0x3bc1a8, 0x3b82f6].get((h as usize).saturating_sub(1)).copied().unwrap_or(0x272727)),
                (None, Some(_)) => look::accent(),
                _ => look::text(),
            };
            job.append(&t, 0.0, fmt(c, sp.link.is_some()));
        }
        let g = p.layout_job(job);
        let h = g.size().y;
        p.galley(pos2(r.min.x, y), g, look::text());
        y += h + if k == 0 { 0.5 } else { 0.0 };
        if y > r.max.y {
            break;
        }
    }
    if let Some(th) = app.thumb_of(n) {
        if matches!(th, Thumb::Image(_) | Thumb::File(_)) {
            if let Some(tex) = app.thumb_texture(ctx, &n.id, &th) {
                let [tw, tht] = tex.size();
                let iw = r.width() * 0.82;
                let ih = iw * tht as f32 / tw.max(1) as f32;
                let ir = Rect::from_min_size(pos2(r.min.x, y + 2.0), vec2(iw, ih));
                p.image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        } else if th == Thumb::Pdf {
            p.text(r.center() + vec2(0.0, 6.0), Align2::CENTER_CENTER, "PDF", look::regular(13.0), look::text());
        }
    }
}

/// The list's keys: ↑/↓ move the selection, Delete moves the note to Recently Deleted, Return in
/// gallery view opens it, Escape goes back to the gallery.
pub fn keys(app: &mut App, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() || app.focus == Focus::Editor {
        if app.gallery && app.gallery_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
            app.gallery_open = false;
        }
        return;
    }
    let order = app.ordered();
    let at = app.note.as_ref().and_then(|n| order.iter().position(|x| x == n));
    let step = |d: i64| -> Option<String> {
        let i = at.map(|i| i as i64 + d).unwrap_or(0).clamp(0, order.len() as i64 - 1);
        order.get(i as usize).cloned()
    };
    let cols = if app.gallery { 5 } else { 1 };
    // (a page: as many rows as the list shows, about)
    let page = 9;
    let k = |key: egui::Key| ctx.input(|i| i.key_pressed(key) && !i.modifiers.command && !i.modifiers.alt);
    let shift = ctx.input(|i| i.modifiers.shift);
    let target = if k(egui::Key::ArrowUp) {
        step(-cols)
    } else if k(egui::Key::ArrowDown) {
        step(cols)
    } else if k(egui::Key::ArrowLeft) && app.gallery {
        step(-1)
    } else if k(egui::Key::ArrowRight) && app.gallery {
        step(1)
    } else if k(egui::Key::Home) {
        order.first().cloned()
    } else if k(egui::Key::End) {
        order.last().cloned()
    } else if k(egui::Key::PageUp) {
        step(-page)
    } else if k(egui::Key::PageDown) {
        step(page)
    } else {
        None
    };
    if let Some(t) = target {
        // (⇧ with the keys extends the selection from where it started)
        if shift {
            app.pick_range(&t);
        } else {
            app.select_note(&t);
        }
        app.list_scroll_to = true;
    }
    if app.focus == Focus::List && ctx.input(|i| (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) && !i.modifiers.shift) {
        let sel = app.selection();
        if !sel.is_empty() {
            app.delete_notes(&sel);
        }
    }
    if app.gallery && ctx.input(|i| i.key_pressed(egui::Key::Enter)) && app.note.is_some() {
        app.gallery_open = true;
        app.focus = Focus::Editor;
    }
    if app.focus == Focus::Editor || !ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
        return;
    }
    app.focus = if app.focus == Focus::Sidebar { Focus::List } else { Focus::Editor };
}

/// The note list's rows sliding to new places (Notes 4.13, out-NO1 animations: ~250 ms when a note
/// is added, deleted or re-sorted, a gentle symmetric ease, its speed at the ends about half the
/// middle's). Each row or header remembers where it was in the list; when that changes while the
/// list shows the same thing, it's drawn gliding from where it was drawn to its new place. A new
/// row just appears; another folder, a search or a new sort order swaps the list at once.
struct Glide {
    key: String,
    now: f64,
    /// id -> (its place in the list last frame, the offset it set off from, when)
    was: std::collections::HashMap<String, (f32, f32, f64)>,
    seen: std::collections::HashMap<String, (f32, f32, f64)>,
    moving: bool,
}

const GLIDE_S: f64 = 0.25;

impl Glide {
    fn take(ctx: &egui::Context, key: &str) -> Glide {
        let id = egui::Id::new("list-glide");
        let (old_key, was) = ctx.data_mut(|d| d.get_temp::<(String, std::collections::HashMap<String, (f32, f32, f64)>)>(id)).unwrap_or_default();
        let was = if old_key == key { was } else { Default::default() };
        Glide { key: key.to_string(), now: ctx.input(|i| i.time), was, seen: Default::default(), moving: false }
    }

    /// How far from its place in the list (`at`) the row is drawn now.
    fn offset(&mut self, id: &str, at: f32) -> f32 {
        let ease = |u: f32| 0.6 * u + 0.4 * (0.5 - 0.5 * (std::f32::consts::PI * u).cos());
        let left = |off: f32, start: f64| {
            let u = ((self.now - start) / GLIDE_S) as f32;
            if u >= 1.0 { 0.0 } else { off * (1.0 - ease(u.max(0.0))) }
        };
        let (mut off, mut start) = (0.0, f64::NEG_INFINITY);
        if let Some(&(prev, off0, start0)) = self.was.get(id) {
            let drawn = prev + left(off0, start0);
            if (prev - at).abs() > 0.5 {
                // moved: set off from where it was drawn
                (off, start) = (drawn - at, self.now);
            } else {
                (off, start) = (off0, start0);
            }
        }
        let now_off = left(off, start);
        if now_off != 0.0 {
            self.moving = true;
            if std::env::var_os("NT_GLIDE_TRACE").is_some() {
                eprintln!("glide t={:.3} {id} at={at:.1} off={now_off:.2}", self.now - start);
            }
        }
        self.seen.insert(id.to_string(), (at, off, start));
        now_off
    }

    fn keep(self, ctx: &egui::Context) {
        if self.moving {
            ctx.request_repaint();
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("list-glide"), (self.key, self.seen)));
    }
}

/// The topmost section's header held at the top of the list while its rows scroll under it
/// (Notes 4.13, no3-sticky recordings): no push, the next header slides under it and takes its
/// place in one frame when its own row is ~20.5 pt from the top. Its material lets about a quarter of
/// what's under it through, blurred to colour only (text under it reads as nothing; the selected
/// row's colour tints it), and a hairline marks its bottom while it's held.
fn sticky_header(app: &mut App, ui: &mut egui::Ui, column: Rect, heads: &[(String, Rect)], sel_row: Option<Rect>, drawn: &[(Rect, bool)], focused: bool, strip_h: f32) {
    // (the list column itself, inside the window: egui's scroll area lets its content's clip run
    // out to the window's sides, shadow margin and all, so nothing here goes by that)
    let window = kn_ui::winframe::inner_rect(ui.ctx());
    let view = ui.clip_rect().intersect(column).intersect(window);
    let v = view.top();
    // the next header takes over once its row is this close to the top (Mac, frame by frame:
    // its row ~20.5 pt below the list's top, its label then ~20 pt under the held one), its own
    // place in the list emptied at once
    const LIFT: f32 = 20.5;
    // (held only once the list has scrolled: at rest the first header is just itself)
    let held = heads.iter().rev().find(|(_, r)| r.min.y <= v + LIFT).filter(|(_, r)| r.min.y < v - 0.5 || r.min.y > v + 0.5).cloned();
    let scrolled = heads.first().map(|(_, r)| r.min.y).or_else(|| drawn.first().map(|(r, _)| r.min.y)).is_some_and(|y| y < v - 0.5);
    // the material: one piece from the top of the column (the toolbar's strip over the list,
    // whose title and buttons are drawn on it after) down through the held header (Mac: the
    // list scrolls up under both, with no seam between them)
    let band = held.as_ref().map(|(_, r)| Rect::from_min_size(pos2(r.min.x, v), r.size()));
    let bottom = band.map_or(v, |b| b.max.y);
    let mat = Rect::from_min_max(pos2(view.min.x, v - strip_h), pos2(view.max.x, bottom)).intersect(window);
    if mat.height() <= 0.0 {
        return;
    }
    let p = ui.ctx().layer_painter(ui.layer_id()).with_clip_rect(mat.union(view));
    if let Some((_, r)) = held.as_ref().filter(|(_, r)| r.min.y > v) {
        p.rect_filled(*r, 0.0, look::content_bg());
    }
    p.rect_filled(mat, app.corners_for(mat), look::content_bg());
    // what's under it, blurred to colour only (Mac, still-stuck-Today-over-yellow-selected-row:
    // the yellow smeared softly across it, ~26 % of it; the text lines a faint smudge): each
    // drawn as a Gaussian blur of its own shape (egui's shadow), clipped to the material
    let under = p.with_clip_rect(mat);
    let blur = |r: Rect, radius: u8, col: Color32| egui::Shape::mesh(gaussian_rect(r, radius as f32 / 2.0, col, mat));
    let dark = !kn_ui::theme::is_light();
    let ink = if dark { Color32::from_white_alpha(10) } else { Color32::from_black_alpha(9) };
    for (r, thumb) in drawn.iter().filter(|(r, _)| r.intersects(mat)) {
        let tx = r.min.x + 34.0;
        for (y0, y1) in [(13.0, 29.0), (31.0, 44.0)] {
            under.add(blur(Rect::from_min_max(pos2(tx, r.min.y + y0), pos2(tx + (r.width() - 110.0).max(40.0), r.min.y + y1)), 22, ink));
        }
        if *thumb {
            under.add(blur(Rect::from_center_size(pos2(r.max.x - 20.0 - 22.0, r.center().y), vec2(44.0, 44.0)), 22, ink.gamma_multiply(1.6)));
        }
    }
    if let Some(s) = sel_row.filter(|s| s.intersects(mat)) {
        // (in dark the selection's own colour, mixed into the dark page, smears to a muddy
        // brown; the material takes its yellow at a lower strength instead, a soft warm tint)
        let tint = if dark && focused { kn_ui::theme::rgb(0xfae49b).gamma_multiply(0.16) } else { look::selection(focused).gamma_multiply(0.30) };
        under.add(blur(s, 28, tint));
    }
    // (and it fades into the list below with a short soft edge, once anything is under it)
    if band.is_some() || scrolled {
        // (a smoothstep down 8 pt, in many rows, so it has no edge of its own)
        let fade = Rect::from_min_max(pos2(mat.min.x, mat.max.y), pos2(mat.max.x, mat.max.y + 8.0));
        let mut m = egui::Mesh::default();
        let rows = 16;
        for k in 0..=rows {
            let t = k as f32 / rows as f32;
            let a = 0.7 * (1.0 - t * t * (3.0 - 2.0 * t));
            let y = fade.min.y + fade.height() * t;
            m.colored_vertex(pos2(fade.min.x, y), look::content_bg().gamma_multiply(a));
            m.colored_vertex(pos2(fade.max.x, y), look::content_bg().gamma_multiply(a));
            if k > 0 {
                let i = 2 * (k - 1);
                m.add_triangle(i, i + 1, i + 2);
                m.add_triangle(i + 1, i + 3, i + 2);
            }
        }
        p.with_clip_rect(view).add(egui::Shape::mesh(m));
        dither(ui.ctx(), &p.with_clip_rect(fade.intersect(view)), fade);
    }
    if band.is_some() || scrolled {
        dither(ui.ctx(), &under, mat);
    }
    let (Some((label, _)), Some(band)) = (held, band) else { return };
    let p = p.with_clip_rect(view);
    p.text(pos2(band.min.x + 16.0, band.min.y + 29.0), Align2::LEFT_BOTTOM, &label, look::bold(15.0), look::text());
    p.hline(band.min.x + 15.0..=band.max.x - 16.0, band.max.y - 0.5, Stroke::new(1.0, look::line()));
    let pinned = label == "Pinned";
    let resp = ui.interact(band, ui.id().with("sticky-header"), Sense::click());
    if pinned {
        let t = ui.ctx().animate_bool_with_time(egui::Id::new("pinned-chevron"), ui.rect_contains_pointer(view), 0.15);
        pinned_chevron(&p, band, app.pinned_collapsed, t);
    }
    if pinned && resp.clicked() {
        app.pinned_collapsed = !app.pinned_collapsed;
    }
    if std::env::var_os("NT_STICKY_TRACE").is_some() {
        let next = heads.iter().find(|(_, h)| h.min.y > v + 0.5).map(|(l, h)| format!("{l}@{:.1}", h.min.y - v)).unwrap_or_default();
        eprintln!("sticky {label} next={next}");
    }
}

/// A rectangle blurred by a Gaussian of `sigma` (its exact blur: erf along each axis), as a mesh
/// whose colour goes by vertex: samples close together across its soft edges, far apart where
/// it's flat, all within `clip` (the GPU blends between them, so there are no steps of its own).
pub(crate) fn gaussian_rect(r: Rect, sigma: f32, col: Color32, clip: Rect) -> egui::Mesh {
    gaussian_rect_masked(r, sigma, col, clip, &|_| 1.0)
}

/// gaussian_rect, its colour scaled at each height by `mask` (0..1), as a feathered edge fades.
pub(crate) fn gaussian_rect_masked(r: Rect, sigma: f32, col: Color32, clip: Rect, mask: &dyn Fn(f32) -> f32) -> egui::Mesh {
    fn erf(x: f32) -> f32 {
        // (Abramowitz & Stegun 7.1.26)
        let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
        let y = 1.0 - (((((1.061_405_4 * t - 1.453_152_0) * t) + 1.421_413_7) * t - 0.284_496_7) * t + 0.254_829_6) * t * (-x * x).exp();
        if x >= 0.0 { y } else { -y }
    }
    let sigma = sigma.max(0.5);
    let k = std::f32::consts::SQRT_2 * sigma;
    let profile = |v: f32, a: f32, b: f32| 0.5 * (erf((v - a) / k) - erf((v - b) / k));
    // samples: every sigma/3 within 3 sigma of each edge, and the middle's ends
    let samples = |a: f32, b: f32, lo: f32, hi: f32| {
        let mut v: Vec<f32> = Vec::new();
        for e in [a, b] {
            let mut x = e - 3.0 * sigma;
            while x <= e + 3.0 * sigma {
                v.push(x);
                x += sigma / 3.0;
            }
        }
        v.retain(|x| *x >= lo && *x <= hi);
        v.extend([lo.max(a - 3.0 * sigma), hi.min(b + 3.0 * sigma)]);
        v.sort_by(|p, q| p.total_cmp(q));
        v.dedup_by(|p, q| (*p - *q).abs() < 0.05);
        v
    };
    let xs = samples(r.min.x, r.max.x, clip.min.x, clip.max.x);
    let mut ys = samples(r.min.y, r.max.y, clip.min.y, clip.max.y);
    // (under a fade, rows every 2 pt as well, so the fade's curve is followed, not interpolated
    // across the whole shape)
    if mask(clip.min.y) != mask(clip.max.y) {
        let (lo, hi) = (clip.min.y.max(r.min.y - 3.0 * sigma), clip.max.y.min(r.max.y + 3.0 * sigma));
        let mut y = lo;
        while y < hi {
            ys.push(y);
            y += 2.0;
        }
        ys.push(hi);
        ys.sort_by(|p, q| p.total_cmp(q));
        ys.dedup_by(|p, q| (*p - *q).abs() < 0.05);
    }
    let mut m = egui::Mesh::default();
    if xs.len() < 2 || ys.len() < 2 {
        return m;
    }
    let gx: Vec<f32> = xs.iter().map(|x| profile(*x, r.min.x, r.max.x)).collect();
    for y in &ys {
        let gy = profile(*y, r.min.y, r.max.y);
        for (x, g) in xs.iter().zip(&gx) {
            m.colored_vertex(pos2(*x, *y), col.gamma_multiply((g * gy * mask(*y)).clamp(0.0, 1.0)));
        }
    }
    let w = xs.len() as u32;
    for j in 0..ys.len() as u32 - 1 {
        for i in 0..w - 1 {
            let a = j * w + i;
            m.add_triangle(a, a + 1, a + w);
            m.add_triangle(a + 1, a + w + 1, a + w);
        }
    }
    m
}

/// A fixed fine noise (each device pixel a touch lighter, darker or as it is, ±1/255) over
/// `r`, so soft gradients show no 8-bit bands; tied to the window, so it doesn't shimmer as the
/// list scrolls.
pub(crate) fn dither(ctx: &egui::Context, p: &egui::Painter, r: Rect) {
    const N: usize = 64;
    let id = egui::Id::new("material-dither");
    let tex = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let mut img = egui::ColorImage::new([N, N], vec![Color32::TRANSPARENT; N * N]);
        let mut seed: u32 = 0x9e37_79b9;
        for px in img.pixels.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *px = match seed % 3 {
                0 => Color32::from_rgba_premultiplied(1, 1, 1, 1),
                1 => Color32::from_rgba_premultiplied(0, 0, 0, 1),
                _ => Color32::TRANSPARENT,
            };
        }
        let t = ctx.load_texture("material-dither", img, egui::TextureOptions::NEAREST_REPEAT);
        ctx.data_mut(|d| d.insert_temp(id, t.clone()));
        t
    });
    // (one texel per device pixel, from the window's corner)
    let tile = N as f32 / ctx.pixels_per_point();
    let uv = Rect::from_min_max(pos2(r.min.x / tile, r.min.y / tile), pos2(r.max.x / tile, r.max.y / tile));
    p.image(tex.id(), r, uv, Color32::WHITE);
}

/// The Pinned header's chevron (Mac, measured on the no3-sticky recording at 2x): a light grey
/// "⌄" 8 pt wide, its middle 25 pt from the list's right edge and level with the label's caps;
/// "›" while the section is collapsed. `t` fades it in and out.
fn pinned_chevron(p: &egui::Painter, header: Rect, collapsed: bool, t: f32) {
    if t <= 0.0 {
        return;
    }
    let g = if collapsed { Glyph::ChevronRight } else { Glyph::ChevronDown };
    // (Apple's darkest pixel: 178 on white; on dark the same share of the text colour)
    let col = look::text().gamma_multiply(0.35 * t);
    draw(p, Rect::from_center_size(pos2(header.max.x - 25.0, header.min.y + 20.0), vec2(14.5, 14.5)), g, col);
}

