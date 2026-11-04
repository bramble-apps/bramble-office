//! Drawing a laid out note: the date line, highlight bands, the selection, text, list labels,
//! checklist circles, block-quote bars, heading chevrons, spelling marks, note-link icons,
//! dividers, tables and attachments, the caret, the math suggestion and the autocomplete list.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};

use nt_core::model::ParaStyle;

use super::doc::AttKind;
use super::layout::{self, Block, Flow, Label, PBox};
use super::{Cell, Editor};

pub enum Link {
    Web(String),
    Note(String),
    Tag(String),
}

/// The link, note link or tag under a point.
pub fn link_at(ed: &Editor, p: Pos2) -> Option<Link> {
    let l = ed.hit(p, true)?;
    let para = ed.para(l);
    let flow = ed.view.flow.as_ref()?;
    let (f, o) = ed.flow_geo(flow, ed.view.origin, l.cell)?;
    let pb = f.boxes.get(l.para)?;
    let g = pb.galley.as_ref()?;
    // only when the point is over the character itself
    let ch = layout::hit(g, p - (o + vec2(pb.galley_x, pb.top + pb.gy)));
    let byte = pb.map.to_byte(ch);
    for b in [byte, super::ops::prev_char(&para.text, byte)] {
        let Some(fm) = para.fmt_of(b) else { continue };
        // the whole token (a tag, a note link) or the one character
        let (ca, cb) = pb.map.segs.iter().find(|sg| sg.token && sg.byte <= b && b < sg.byte + sg.blen).map_or((pb.map.to_char(b), pb.map.to_char(b) + 1), |sg| (sg.ch, sg.ch + sg.clen));
        let r = layout::char_rects(g, ca, cb, true);
        let over = r.iter().any(|r| r.translate((o + vec2(pb.galley_x, pb.top + pb.gy)).to_vec2()).expand(1.0).contains(p));
        if !over {
            continue;
        }
        if let Some(u) = &fm.link {
            return Some(Link::Web(u.clone()));
        }
        if let Some(a) = fm.att.as_deref().and_then(|id| ed.doc.att(id)) {
            match &a.kind {
                AttKind::Tag(t) => return Some(Link::Tag(t.clone())),
                AttKind::NoteLink { url, .. } => return Some(Link::Note(url.clone())),
                _ => {}
            }
        }
    }
    None
}

/// The checklist circle and heading chevron hit areas: handles a press on one (true).
pub fn press_widgets(ed: &mut Editor, p: Pos2) -> bool {
    let Some(flow) = ed.view.flow.clone() else { return false };
    let o = ed.view.origin;
    let s = ed.settings.text_scale;
    for pb in flow.boxes.iter().filter(|b| !b.hidden) {
        let top = o.y + pb.top;
        if p.y < top || p.y > top + pb.height.max(pb.size) {
            continue;
        }
        if let Label::Check(on) = pb.label {
            let c = check_center(pb, o, s);
            if (p - c).length() <= layout::CHECK_D * s * 0.75 {
                if let Some(id) = ed.set_checked(None, pb.para, !on) {
                    ed.view.tick_anim = Some((id, ed.view.last_click.0));
                }
                return true;
            }
        }
        if let Some(collapsed) = pb.heading {
            let r = chevron_rect(pb, o, s);
            if r.expand(3.0).contains(p) && (collapsed || ed.view.hover == Some(pb.para)) {
                ed.set_collapsed(pb.para, !collapsed);
                return true;
            }
        }
    }
    // circles in table cells
    for pb in flow.boxes.iter() {
        let Some(Block::Table(tb)) = &pb.block else { continue };
        for (r, row) in tb.cells.iter().enumerate() {
            for (c, f) in row.iter().enumerate() {
                let cell = Cell { para: pb.para, row: r, col: c };
                let Some((_, co)) = ed.flow_geo(&flow, o, Some(cell)) else { continue };
                for b in &f.boxes {
                    if let Label::Check(on) = b.label {
                        if (p - check_center(b, co, s)).length() <= layout::CHECK_D * s * 0.75 {
                            if let Some(id) = ed.set_checked(Some(cell), b.para, !on) {
                                ed.view.tick_anim = Some((id, ed.view.last_click.0));
                            }
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn first_row_mid(pb: &PBox) -> f32 {
    match &pb.galley {
        Some(g) => pb.gy + g.rows.first().map_or(pb.height / 2.0, |r| r.pos.y + r.row.size.y / 2.0),
        None => pb.height / 2.0,
    }
}

fn check_center(pb: &PBox, o: Pos2, s: f32) -> Pos2 {
    pos2(o.x + pb.label_x - layout::LABEL_X * s + layout::CHECK_D * s / 2.0 - 0.75 * s, o.y + pb.top + first_row_mid(pb))
}

fn chevron_rect(pb: &PBox, o: Pos2, s: f32) -> Rect {
    let c = pos2(o.x - 12.0 * s, o.y + pb.top + first_row_mid(pb));
    Rect::from_center_size(c, vec2(12.0 * s, 12.0 * s))
}

pub fn paint(ed: &mut Editor, ui: &mut egui::Ui, rect: Rect, has_kb: bool) {
    // (drawn up under the toolbar strip too, which the shell lays its material over)
    let painter = ui.painter_at(Rect::from_min_max(pos2(rect.min.x, ed.view.clip_top.min(rect.min.y)), rect.max));
    // (the window's rounded corners where the column reaches them)
    let c = ed.corners;
    painter.rect_filled(painter.clip_rect(), egui::CornerRadius { nw: 0, ne: 0, sw: c.sw, se: c.se }, layout::bg_color());
    let s = ed.settings.text_scale;
    let ctx = ui.ctx().clone();
    // the date line, centred over the pane, scrolling with the text
    if !ed.date_line.is_empty() {
        let font = FontId::new(layout::DATE_SIZE * s, layout::family("Sfera-Regular"));
        let fmt = egui::text::TextFormat { font_id: font, color: layout::dim_color(), extra_letter_spacing: layout::tracking(layout::Face::Text, layout::DATE_SIZE) * s, ..Default::default() };
        let g = ctx.fonts_mut(|f| f.layout_job(egui::text::LayoutJob::single_section(ed.date_line.clone(), fmt)));
        let base = g.rows.first().and_then(|r| r.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
        let y = rect.top() + layout::DATE_BASELINE * s - ed.view.scroll.offset - base;
        ed.view.date_rect = Some(Rect::from_min_size(pos2(rect.center().x - g.size().x / 2.0, y), g.size()));
        painter.galley(pos2(rect.center().x - g.size().x / 2.0, y), g, layout::dim_color());
    }
    let Some(flow) = ed.view.flow.clone() else { return };
    let o = ed.view.origin;
    paint_flow(ed, &painter, &flow, o, None, painter.clip_rect());
    // with a finger: handles at the selection's ends to drag (as Letters)
    if ed.has_sel() && ed.table_sel.is_none() {
        let (sa, sb) = ed.range();
        if let (Some(a), Some(b)) = (ed.caret_screen(sa), ed.caret_screen(sb)) {
            kn_ui::touch::selection_handles(&painter, [a.center_top(), a.center_bottom()], [b.center_top(), b.center_bottom()], layout::accent());
        }
    }
    // the caret (blinking), unless text is selected
    let focused = ed.focused && has_kb;
    // (beside a drawing the caret would be a tall line at its edge: Notes shows none there)
    let at_drawing = ed.head.cell.is_none() && ed.doc.paras.get(ed.head.para).and_then(|p| p.sole_att()).and_then(|id| ed.doc.att(id)).is_some_and(|a| matches!(a.kind, AttKind::Drawing { .. }));
    if focused && !ed.has_sel() && ed.table_sel.is_none() && ed.draw.is_none() && !at_drawing {
        let t = ctx.input(|i| i.time) - ed.view.blink;
        let on = (t % 1.0) < 0.5 || std::env::var_os("NT_SNAP").is_some();
        if let Some(c) = ed.caret_screen(ed.head) {
            if on {
                let w = 2.0 * s.max(0.75);
                painter.rect_filled(Rect::from_min_max(pos2(c.min.x - w / 2.0, c.min.y), pos2(c.min.x + w / 2.0, c.max.y)), CornerRadius::same(1), layout::accent());
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(500 - ((t * 1000.0) as u64 % 500)));
            // Math Results (Suggest): the value after the "=" in grey
            if let Some((at, r)) = &ed.view.ghost {
                if *at == ed.head {
                    let p = ed.para(ed.head);
                    let f = layout::run_format(p, &p.fmt_at(ed.head.off), s);
                    let g = ctx.fonts_mut(|fo| fo.layout_no_wrap(r.clone(), f.font_id.clone(), layout::dim_color()));
                    let gy = c.center().y - g.size().y / 2.0;
                    painter.galley(pos2(c.max.x + 1.0, gy), g, layout::dim_color());
                }
            }
        }
    }
    // where a dragged checklist item would land
    if let (Some(super::input::Drag::Item(_)), Some(i)) = (&ed.view.drag, ed.view.drop_at) {
        let y = o.y + flow.boxes.get(i).map_or(flow.height, |b| b.top);
        painter.hline(o.x..=rect.right() - layout::MARGIN_X * s, y, Stroke::new(2.0, layout::accent()));
    }
    super::table::paint_selection(ed, &painter, &flow, o);
    paint_complete(ed, ui);
}

/// Paints one flow (the note or a table cell) whose top left is at `o`.
pub fn paint_flow(ed: &mut Editor, painter: &Painter, flow: &Flow, o: Pos2, cell: Option<Cell>, clip: Rect) {
    let s = ed.settings.text_scale;
    let (sa, sb) = ed.range();
    let sel_here = ed.has_sel() && sa.cell == cell && ed.table_sel.is_none();
    let focused = ed.focused;
    for pb in &flow.boxes {
        if pb.hidden {
            continue;
        }
        let top = o.y + pb.top;
        if top > clip.bottom() || top + pb.height < clip.top() {
            continue;
        }
        let Some(para) = ed.flow(cell).get(pb.para).cloned() else { continue };
        let base = o + vec2(pb.galley_x, pb.top + pb.gy);
        // Monostyled: its run's grey block, the column's width
        if para.style == ParaStyle::Monostyled {
            let paras = ed.flow(cell);
            let shown = |j: usize| flow.boxes.get(j).is_some_and(|b| !b.hidden) && paras.get(j).is_some_and(|q| q.style == ParaStyle::Monostyled);
            let first = pb.para == 0 || !shown(pb.para - 1);
            let last = !shown(pb.para + 1);
            let y0 = top + if first { layout::MONO_GAP * s } else { 0.0 };
            let y1 = top + pb.height - if last { layout::MONO_GAP * s } else { 0.0 };
            let x0 = o.x + pb.x - layout::MONO_INSET * s;
            let r = Rect::from_min_max(pos2(x0, y0), pos2(x0 + pb.width + 2.0 * layout::MONO_INSET * s, y1));
            let rad = (5.0 * s) as u8;
            let cr = CornerRadius { nw: if first { rad } else { 0 }, ne: if first { rad } else { 0 }, sw: if last { rad } else { 0 }, se: if last { rad } else { 0 } };
            painter.rect_filled(r, cr, mono_block());
            painter.rect_stroke(r, cr, Stroke::new(0.5, Color32::from_black_alpha(if layout::dark() { 60 } else { 18 })), egui::StrokeKind::Inside);
        }
        // block quote: a bar down the paragraph's left
        if let Some(qx) = pb.quote_x {
            let x = o.x + qx + 0.5 * s;
            painter.rect_filled(Rect::from_min_max(pos2(x, top + 1.0), pos2(x + 3.0 * s, top + pb.height - 1.0)), CornerRadius::same(2), quote_bar());
        }
        if let Some(block) = &pb.block {
            if sel_here && (sa.para, sa.off) <= (pb.para, 0) && (sb.para, sb.off) >= (pb.para, para.len()) {
                let r = Rect::from_min_size(pos2(o.x + pb.x, top), vec2(pb.width, pb.height)).expand(2.0);
                let drawing = para.sole_att().and_then(|id| ed.doc.att(id)).is_some_and(|a| matches!(a.kind, AttKind::Drawing { .. }));
                if drawing {
                    // a drawing has no paper to cover it: a light tint and a thin outline, its
                    // ink still showing
                    let c = layout::selection_color(focused);
                    painter.rect_filled(r, CornerRadius::same(4), c.gamma_multiply(0.3));
                    painter.rect_stroke(r, CornerRadius::same(4), Stroke::new(1.5, c), egui::StrokeKind::Inside);
                } else {
                    painter.rect_filled(r, CornerRadius::same(4), layout::selection_color(focused));
                }
            }
            let id = para.sole_att().unwrap_or_default().to_string();
            match block {
                Block::Divider => {
                    let y = (top + layout::DIVIDER_LINE * s).round() + 0.5;
                    painter.hline(o.x + pb.x..=o.x + pb.x + pb.width, y, Stroke::new(1.0, divider_color()));
                }
                Block::Table(tb) => super::table::paint_table(ed, painter, pb, tb, pos2(o.x + pb.x, top), clip),
                Block::Media(size) => {
                    let r = Rect::from_min_size(pos2(o.x + pb.x, top + layout::MEDIA_PAD * s), *size);
                    super::media::paint(ed, painter, &id, r);
                }
            }
            continue;
        }
        let Some(g) = &pb.galley else { continue };
        // highlights: a rounded band per row and colour, a little wider than the text
        for (r, f) in para.spans() {
            let Some(h) = f.highlight else { continue };
            let (a, b) = (pb.map.to_char(r.start), pb.map.to_char(r.end));
            for cr in layout::char_rects(g, a, b, true) {
                // measured on "34 Highlight Colors": 18 pt of a 19 pt line, ½ pt above the line's middle
                let band = Rect::from_min_max(pos2(cr.min.x - layout::HL_PAD_X * s, cr.min.y - 0.5 * s), pos2(cr.max.x + layout::HL_PAD_RIGHT * s, cr.max.y - 1.5 * s)).translate(base.to_vec2());
                painter.rect_filled(band, CornerRadius::same((layout::HL_RADIUS * s) as u8), layout::highlight_colors(h).0);
            }
        }
        // the selection
        if sel_here && pb.para >= sa.para && pb.para <= sb.para {
            let a = if pb.para == sa.para { pb.map.to_char(sa.off) } else { 0 };
            let b = if pb.para == sb.para { pb.map.to_char(sb.off) } else { pb.map.chars() };
            let past_end = pb.para < sb.para;
            for (i, cr) in layout::char_rects(g, a, b, true).into_iter().enumerate() {
                let _ = i;
                let mut r = cr.translate(base.to_vec2());
                if past_end && (r.max.x - (base.x + g.rows.last().map_or(0.0, |row| row.pos.x + row.size.x))).abs() < 1.0 {
                    r.max.x += 4.0 * s;
                }
                painter.rect_filled(r, 0.0, layout::selection_color(focused));
            }
            if a == b && past_end {
                // an empty line inside the selection
                let c = layout::caret_rect(g, 0).translate(base.to_vec2());
                painter.rect_filled(Rect::from_min_max(c.min, pos2(c.min.x + 4.0 * s, c.max.y)), 0.0, layout::selection_color(focused));
            }
        }
        super::find::paint_matches(ed, painter, cell, pb.para, g, &pb.map, base);
        painter.galley(base, g.clone(), layout::text_color());
        // note-link icons
        for (r, f) in para.spans() {
            let Some(AttKind::NoteLink { .. }) = f.att.as_deref().and_then(|id| ed.doc.att(id)).map(|a| &a.kind) else { continue };
            let ch = pb.map.to_char(r.start);
            if let Some(cr) = layout::char_rects(g, ch, ch + 1, true).first() {
                // the icon sits in the leading space before the link's first character
                let c = cr.translate(base.to_vec2());
                let w = layout::NOTE_ICON_W * s;
                let ir = Rect::from_min_size(pos2(c.min.x - (layout::NOTE_ICON_W + layout::NOTE_ICON_GAP) * s, c.center().y - w * 0.46), vec2(w, w * 0.92));
                note_icon(painter, ir, layout::accent());
            }
        }
        // spelling: a red dotted line under misspelled words (not the one being typed)
        if ed.settings.spelling && cell.is_none() {
            let typing_here = ed.head.para == pb.para && ed.head.cell == cell;
            // (checked on the workers, cached by text; until then nothing is marked)
            let marks = match ed.view.spell.get(&para.text) {
                Some(m) => m.clone(),
                None => {
                    super::spelling::check_later(&mut ed.view, &para.text, painter.ctx());
                    Vec::new()
                }
            };
            for (a, b) in marks {
                if typing_here && ed.head.off >= a && ed.head.off <= b {
                    continue;
                }
                if para.fmt_of(a).is_some_and(|f| f.att.is_some() || f.link.is_some()) {
                    continue;
                }
                for cr in layout::char_rects(g, pb.map.to_char(a), pb.map.to_char(b), true) {
                    let r = cr.translate(base.to_vec2());
                    let y = r.max.y - pb.size * 0.12;
                    let mut x = r.min.x + 1.0;
                    while x < r.max.x {
                        painter.circle_filled(pos2(x, y), 1.1 * s, Color32::from_rgb(255, 59, 48));
                        x += 3.2 * s;
                    }
                }
            }
        }
        // data detectors: a light dotted line under dates, phone numbers and addresses
        if ed.settings.data_detectors && cell.is_none() {
            let found = ed.view.detect.entry(para.text.clone()).or_insert_with(|| super::detect::find(&para.text)).clone();
            for (a, b, _) in found {
                if para.fmt_of(a).is_some_and(|f| f.att.is_some() || f.link.is_some()) {
                    continue;
                }
                for cr in layout::char_rects(g, pb.map.to_char(a), pb.map.to_char(b), true) {
                    let r = cr.translate(base.to_vec2());
                    let y = r.max.y - pb.size * 0.06;
                    let mut x = r.min.x + 0.5;
                    while x < r.max.x {
                        painter.line_segment([pos2(x, y), pos2((x + 1.6 * s).min(r.max.x), y)], Stroke::new(0.9 * s, layout::dim_color()));
                        x += 3.2 * s;
                    }
                }
            }
        }
        // list labels and checklist circles
        let label_y = top + pb.gy + g.rows.first().map_or(0.0, |r| r.pos.y + r.glyphs.first().map_or(r.row.size.y * 0.78, |gl| gl.pos.y));
        let label_font = layout::run_format(&para, &para.fmt_at(0), s).font_id;
        let lx = o.x + pb.label_x;
        // labels end a little before the text (the gallery's "27 Autoformat"): dashes and numbers
        // right-aligned 6 pt before it, the bullet a round dot centred 11.5 pt before it
        let text_x = o.x + pb.x;
        let _ = lx;
        match &pb.label {
            Label::None => {}
            Label::Bullet => {
                let cy = label_y - pb.size * 0.33;
                painter.circle_filled(pos2(text_x - layout::BULLET_X * s, cy), layout::BULLET_R * s * pb.size / 16.0, layout::text_color());
            }
            Label::Dash => text_right(painter, pos2(text_x - layout::LABEL_GAP * s, label_y), "\u{2013}", label_font, layout::text_color()),
            Label::Number(n) => text_right(painter, pos2(text_x - layout::NUMBER_GAP * s, label_y), &format!("{n}."), label_font, layout::text_color()),
            Label::Check(on) => {
                let c = check_center(pb, o, s);
                let r = layout::CHECK_D * s / 2.0;
                // the tick (NO2, 60 fps): the circle fills at once, the white mark comes in from
                // 67 to 133 ms, done by 150 ms; unticking empties it at once
                let since = ed.view.tick_anim.as_ref().filter(|a| !para.check_id.is_empty() && a.0 == para.check_id).map(|a| painter.ctx().input(|i| i.time) - a.1);
                if since.is_some_and(|t| t < 0.15) {
                    painter.ctx().request_repaint();
                }
                if *on {
                    painter.circle_filled(c, r, layout::accent());
                    let mark = since.map_or(1.0, |t| ((t - 0.067) / 0.066).clamp(0.0, 1.0) as f32);
                    if mark > 0.0 {
                        tick(painter, c, r, Color32::WHITE.gamma_multiply(mark));
                    }
                } else {
                    painter.circle_stroke(c, r - layout::CHECK_RING * s / 2.0, Stroke::new(layout::CHECK_RING * s, check_ring()));
                }
                // (no drag handle on hover: Notes shows none, NO2; Move Item ⌃⌘↑/↓ moves items)
            }
        }
        // the collapse chevron: on hover, while collapsed, and on the caret's heading (NO2)
        if let Some(collapsed) = pb.heading {
            let caret_here = ed.focused && cell.is_none() && ed.head.cell.is_none() && ed.head.para == pb.para;
            if collapsed || ed.view.hover == Some(pb.para) || caret_here {
                chevron(painter, chevron_rect(pb, o, s), !collapsed, layout::accent());
            }
        }
    }
}

fn text_right(painter: &Painter, baseline_right: Pos2, t: &str, font: FontId, c: Color32) {
    let g = painter.layout_no_wrap(t.to_string(), font, c);
    let b = g.rows.first().and_then(|r| r.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
    painter.galley(pos2(baseline_right.x - g.size().x, baseline_right.y - b), g, c);
}

#[allow(dead_code)]
fn text_at(painter: &Painter, baseline_left: Pos2, t: &str, font: FontId, c: Color32) {
    let g = painter.layout_no_wrap(t.to_string(), font, c);
    let b = g.rows.first().and_then(|r| r.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
    painter.galley(pos2(baseline_left.x, baseline_left.y - b), g, c);
}

fn quote_bar() -> Color32 {
    if layout::dark() { Color32::from_gray(110) } else { Color32::from_gray(189) }
}

fn mono_block() -> Color32 {
    if layout::dark() { Color32::from_gray(52) } else { Color32::from_gray(234) }
}

fn divider_color() -> Color32 {
    if layout::dark() { Color32::from_gray(120) } else { Color32::from_gray(61) }
}

fn check_ring() -> Color32 {
    if layout::dark() { Color32::from_gray(110) } else { Color32::from_gray(189) }
}

/// Six dots: the handle that drags a checklist item.
/// A press on a checklist item's drag handle: the item.
/// (kept off: Notes has no checklist drag handle)
const GRIP: bool = false;

pub fn grip_at(ed: &Editor, p: Pos2) -> Option<usize> {
    // Notes shows no drag handle on checklist items (NO2): items move with Move Item ⌃⌘↑/↓
    if !GRIP {
        return None;
    }
    let flow = ed.view.flow.as_ref()?;
    let s = ed.settings.text_scale;
    let o = ed.view.origin;
    let pb = flow.boxes.iter().find(|b| !b.hidden && b.label != Label::None && ed.view.hover == Some(b.para))?;
    if !matches!(pb.label, Label::Check(_)) {
        return None;
    }
    let c = pos2(o.x - 12.0 * s, check_center(pb, o, s).y);
    (Rect::from_center_size(c, vec2(12.0 * s, 16.0 * s)).contains(p)).then_some(pb.para)
}

/// A tick mark inside a circle of radius `r`.
pub fn tick(painter: &Painter, c: Pos2, r: f32, col: Color32) {
    let pts = vec![pos2(c.x - r * 0.42, c.y + r * 0.02), pos2(c.x - r * 0.12, c.y + r * 0.32), pos2(c.x + r * 0.45, c.y - r * 0.35)];
    painter.add(egui::Shape::line(pts, Stroke::new(r * 0.2, col)));
}

/// A disclosure chevron: pointing down (expanded) or right.
pub fn chevron(painter: &Painter, r: Rect, down: bool, col: Color32) {
    let c = r.center();
    let k = r.width() * 0.28;
    let pts = if down { vec![pos2(c.x - k, c.y - k * 0.5), pos2(c.x, c.y + k * 0.5), pos2(c.x + k, c.y - k * 0.5)] } else { vec![pos2(c.x - k * 0.5, c.y - k), pos2(c.x + k * 0.5, c.y), pos2(c.x - k * 0.5, c.y + k)] };
    painter.add(egui::Shape::line(pts, Stroke::new(1.6, col)));
}

/// The little note icon before a note link.
fn note_icon(painter: &Painter, r: Rect, col: Color32) {
    // a note: rounded outline, a filled top band, two text lines
    painter.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.2, col), egui::StrokeKind::Inside);
    let band = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + r.height() * 0.3));
    painter.rect_filled(band, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, col);
    let x0 = r.min.x + r.width() * 0.2;
    let x1 = r.max.x - r.width() * 0.2;
    for k in [0.55, 0.77] {
        painter.hline(x0..=x1, r.min.y + r.height() * k, Stroke::new(1.0, col));
    }
}

/// The # / >> autocomplete list under the caret.
fn paint_complete(ed: &mut Editor, ui: &mut egui::Ui) {
    let Some(c) = ed.view.complete.clone() else { return };
    let items = ed.complete_items(&c);
    if items.is_empty() {
        return;
    }
    let Some(cr) = ed.caret_screen(ed.head) else { return };
    let s = ed.settings.text_scale;
    let row_h = 22.0;
    let w = 240.0;
    let pos = pos2(cr.min.x - 8.0, cr.max.y + 4.0 * s);
    let area = egui::Area::new(egui::Id::unique("nt-complete")).order(egui::Order::Foreground).fixed_pos(pos);
    let mut chosen = None;
    area.show(ui.ctx(), |ui| {
        let r = Rect::from_min_size(pos, vec2(w, row_h * items.len() as f32 + 10.0));
        let p = ui.painter();
        p.rect_filled(r.translate(vec2(0.0, 2.0)).expand(1.0), CornerRadius::same(10), Color32::from_black_alpha(24));
        p.rect_filled(r, CornerRadius::same(10), if layout::dark() { Color32::from_gray(44) } else { Color32::from_gray(250) });
        for (i, (name, _)) in items.iter().enumerate() {
            let rr = Rect::from_min_size(pos2(r.min.x + 5.0, r.min.y + 5.0 + i as f32 * row_h), vec2(w - 10.0, row_h));
            let resp = ui.interact(rr, egui::Id::unique(("nt-complete", i)), egui::Sense::click());
            let picked = i == c.pick.min(items.len() - 1);
            if picked || resp.hovered() {
                p.rect_filled(rr, CornerRadius::same(6), layout::accent());
            }
            let label = if c.kind == '#' { format!("#{name}") } else { name.clone() };
            p.text(pos2(rr.min.x + 10.0, rr.center().y), Align2::LEFT_CENTER, label, FontId::new(13.0, layout::family("Sfera-Regular")), if picked { Color32::WHITE } else { layout::text_color() });
            if resp.clicked() {
                chosen = Some(i);
            }
        }
    });
    if let Some(i) = chosen {
        if let Some(cc) = ed.view.complete.as_mut() {
            cc.pick = i;
        }
        ed.return_key();
    }
}

