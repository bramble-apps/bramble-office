//! View > Show Ruler: the horizontal ruler over the pages (Pages: 28 pt), in Settings > Rulers'
//! units, with the page margins and the column gutters shaded and the caret paragraph's indents
//! as markers (in the caret's column). Drag a margin edge to change the document margins, a
//! gutter edge to change the gap between the columns (all of them: the columns stay equal), a
//! marker to change the paragraph's indents.

use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Stroke};
use kn_ui::theme;

use crate::app::{App, Owner};

/// A length in the ruler's units ("2.54 cm").
pub fn fmt_len(v: f32, units: u8) -> String {
    match units {
        1 => format!("{:.2} in", v / 72.0),
        2 => format!("{:.0} pt", v),
        3 => format!("{:.0} mm", v / 72.0 * 25.4),
        _ => format!("{:.2} cm", v / 72.0 * 2.54),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Grab {
    LeftMargin,
    RightMargin,
    First,
    Left,
    Right,
    /// The edge of gutter `n` (between column n and n + 1): left edge? .
    Gutter(usize, bool),
}

pub fn ruler(app: &mut App, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let Some(l) = app.layout.as_ref() else { return };
    let setup = l.setup.clone();
    let page = app.current_page();
    // the current page's left edge on screen
    let left = app.canvas.page_to_screen(l, page, pos2(0.0, 0.0)).x;
    let z = app.canvas.zoom;
    let x_of = |pt: f32| left + pt * z;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme::PANEL_BG());
    let band = Rect::from_min_max(pos2(x_of(0.0), rect.min.y + 4.0), pos2(x_of(setup.width), rect.max.y - 4.0));
    p.rect_filled(band, 2.0, theme::gray(0x5a));
    let text_band = Rect::from_min_max(pos2(x_of(setup.left), band.min.y), pos2(x_of(setup.width - setup.right), band.max.y));
    p.rect_filled(text_band, 0.0, Color32::from_gray(0xf2));
    // the body's columns: their gutters shaded like the margins, a grip at each edge
    let cols: Vec<(f32, f32)> = if l.columns.len() > 1 { l.columns.clone() } else { Vec::new() };
    let gutters: Vec<(f32, f32)> = cols.windows(2).map(|w| (w[0].0 + w[0].1, w[1].0)).collect();
    for &(g0, g1) in &gutters {
        let gr = Rect::from_min_max(pos2(x_of(g0), band.min.y), pos2(x_of(g1), band.max.y));
        p.rect_filled(gr, 0.0, theme::gray(0x5a));
        for x in [gr.min.x, gr.max.x] {
            p.rect_filled(Rect::from_center_size(pos2(x, band.center().y), vec2(3.0, band.height() - 6.0)), 1.0, Color32::from_gray(0x9a));
        }
    }
    // ticks: every unit, labels at whole units
    let (step, label_every): (f32, usize) = match app.settings.ruler_units {
        1 => (72.0 / 8.0, 8),
        2 => (12.0, 6),
        3 => (72.0 / 25.4 * 5.0, 2),
        _ => (72.0 / 2.54 / 2.0, 2),
    };
    let mut i = 0usize;
    let mut v = 0.0f32;
    while v <= setup.width {
        let x = x_of(v);
        let major = i % label_every == 0;
        let h = if major { 8.0 } else { 4.0 };
        p.line_segment([pos2(x, band.max.y - h), pos2(x, band.max.y)], Stroke::new(1.0, Color32::from_gray(0x70)));
        if major && i > 0 {
            let n = i / label_every;
            p.text(pos2(x + 2.0, band.min.y + 1.0), egui::Align2::LEFT_TOP, n.to_string(), egui::FontId::proportional(9.0), Color32::from_gray(0x50));
        }
        i += 1;
        v += step;
        let _ = x;
    }
    // indents of the caret paragraph, in the column its first line is in
    let mut indents = None;
    let (mut col_l, mut col_r) = (setup.left, setup.width - setup.right);
    if let (Some(c), Some(body)) = (app.cursor, l.body.as_ref()) {
        if c.owner == Owner::Body {
            let pi = body.para_at_stored(c.caret);
            if let Some(pa) = body.tb.paragraphs.get(pi) {
                indents = Some((pa.first_line_indent + pa.left_indent, pa.left_indent, pa.right_indent));
            }
            if let Some(col) = l.pages.get(page).and_then(|pg| pg.lines.iter().find(|ln| ln.para == pi)).and_then(|ln| l.columns.get(ln.column)) {
                (col_l, col_r) = (col.0, col.0 + col.1);
            }
        }
    }
    let marker = |p: &egui::Painter, x: f32, down: bool, col: Color32| {
        let (y0, y1) = if down { (band.min.y, band.min.y + 7.0) } else { (band.max.y, band.max.y - 7.0) };
        p.add(egui::Shape::convex_polygon(vec![pos2(x - 5.0, y0), pos2(x + 5.0, y0), pos2(x, y1)], col, Stroke::new(1.0, Color32::from_gray(0x30))));
    };
    let blue = Color32::from_rgb(0x3d, 0x8b, 0xf0);
    if let Some((first, li, ri)) = indents {
        marker(&p, x_of(col_l + first), true, blue);
        marker(&p, x_of(col_l + li), false, blue);
        marker(&p, x_of(col_r - ri), false, blue);
    }
    // dragging
    let id = ui.scope_id().with("ruler");
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let grab: Option<Grab> = ui.data(|d| d.get_temp(id.with("grab")));
    let near = |pos: Pos2| -> Option<Grab> {
        let mut c: Vec<(f32, Grab)> = vec![(x_of(setup.left), Grab::LeftMargin), (x_of(setup.width - setup.right), Grab::RightMargin)];
        if let Some((first, li, ri)) = indents {
            c.push((x_of(col_l + first), Grab::First));
            c.push((x_of(col_l + li), Grab::Left));
            c.push((x_of(col_r - ri), Grab::Right));
        }
        for (n, &(g0, g1)) in gutters.iter().enumerate() {
            c.push((x_of(g0), Grab::Gutter(n, true)));
            c.push((x_of(g1), Grab::Gutter(n, false)));
        }
        // indent markers win over margins at the same place: top half = first line
        c.into_iter()
            .filter(|(x, _)| (pos.x - x).abs() < 6.0)
            .min_by(|a, b| {
                let score = |g: &(f32, Grab)| {
                    let half = if matches!(g.1, Grab::First) == (pos.y < rect.center().y) { 0.0 } else { 3.0 };
                    let margin = if matches!(g.1, Grab::LeftMargin | Grab::RightMargin) { 1.0 } else { 0.0 };
                    (pos.x - g.0).abs() + half + margin
                };
                score(a).total_cmp(&score(b))
            })
            .map(|x| x.1)
    };
    if let Some(pos) = resp.hover_pos() {
        if near(pos).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    if resp.drag_started() {
        // where the button went down (the pointer has moved on by the time a drag starts)
        if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
            if let Some(g) = near(pos) {
                ui.data_mut(|d| d.insert_temp(id.with("grab"), g));
            } else {
                // away from the markers: pull a guide down onto the page
                crate::guides::start_from_ruler(app);
            }
        }
    }
    // (on the frame the button comes up there's no interact position: the latest one)
    if let (Some(g), Some(pos)) = (grab, resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos()))) {
        let v = ((pos.x - left) / z).clamp(0.0, setup.width);
        p.vline(pos.x, rect.y_range(), Stroke::new(1.0, theme::ACCENT()));
        let tip = match g {
            Grab::LeftMargin => fmt_len(v, app.settings.ruler_units),
            Grab::RightMargin => fmt_len(setup.width - v, app.settings.ruler_units),
            Grab::Gutter(n, _) => {
                let mid = gutters.get(n).map(|g| (g.0 + g.1) / 2.0).unwrap_or(v);
                format!("Gutter {}", fmt_len((v - mid).abs() * 2.0, app.settings.ruler_units))
            }
            _ => fmt_len(v - col_l, app.settings.ruler_units),
        };
        p.text(pos2(pos.x + 6.0, rect.min.y + 2.0), egui::Align2::LEFT_TOP, tip, egui::FontId::proportional(10.0), theme::TEXT());
        if resp.drag_stopped() {
            ui.data_mut(|d| d.remove::<Grab>(id.with("grab")));
            match g {
                Grab::LeftMargin | Grab::RightMargin => {
                    let mut s = setup.clone();
                    if g == Grab::LeftMargin {
                        s.left = v.min(setup.width - setup.right - 36.0);
                    } else {
                        s.right = (setup.width - v).min(setup.width - setup.left - 36.0);
                    }
                    app.edit_pages("Margins", |pp, h| pp.set_setup(h, &s));
                }
                Grab::Gutter(n, _) => {
                    // a gutter grows or shrinks about its middle; the columns stay equal
                    if let Some(&(g0, g1)) = gutters.get(n) {
                        let gap = ((v - (g0 + g1) / 2.0).abs() * 2.0).clamp(0.0, setup.text_width() / cols.len() as f32);
                        let count = cols.len() as u32;
                        let frac = gap / setup.text_width().max(1.0);
                        app.edit_pages("Columns", |pp, h| pp.set_columns(h, count, frac));
                    }
                }
                Grab::First => {
                    let li = indents.map(|i| i.1).unwrap_or(0.0);
                    let val = v - col_l - li;
                    crate::textinsp::apply_para(app, "Indent", move |pp| pp.set_f32("first_line_indent", val));
                }
                Grab::Left => {
                    let (first, li) = indents.map(|i| (i.0, i.1)).unwrap_or((0.0, 0.0));
                    let nl = (v - col_l).max(0.0);
                    // the first line stays where it was
                    let nf = first - nl;
                    let _ = li;
                    crate::textinsp::apply_para(app, "Indent", move |pp| {
                        pp.set_f32("left_indent", nl);
                        pp.set_f32("first_line_indent", nf);
                    });
                }
                Grab::Right => {
                    let val = (col_r - v).max(0.0);
                    crate::textinsp::apply_para(app, "Indent", move |pp| pp.set_f32("right_indent", val));
                }
            }
        }
    }
    let _ = vec2(0.0, 0.0);
}
