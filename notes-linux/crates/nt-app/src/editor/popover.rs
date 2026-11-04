//! The Aa (Format) popover, laid out as screenshots/popover-format-Aa.png (measured on the 2x
//! capture, in points): a top row of B, I, U, S, a divider, the highlighter and the highlight
//! colour's dot; a line; the paragraph styles drawn in their own look with a check on the
//! current one; a line; Block Quote. The dot opens the colour menu (popover-format-Aa-color.png).

use egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use nt_core::model::ParaStyle;

use super::Editor;
use super::layout;

#[derive(Default, Clone, Debug)]
pub struct PopoverState {
    pub colors_open: bool,
}

/// The popover's size in points.
pub const SIZE: Vec2 = vec2(177.5, 325.0);
const TOP_Y: f32 = 19.75;
const BUTTONS_X: [f32; 6] = [21.25, 50.5, 79.25, 108.25, 137.0, 161.5];
const RULE1_Y: f32 = 39.0;
const FIRST_BASE: f32 = 67.5;
const PITCH: f32 = 28.7;
const RULE2_Y: f32 = 284.5;
const QUOTE_BASE: f32 = 310.0;
const TEXT_X: f32 = 36.0;
const CHECK_X: f32 = 22.0;
const INSET: f32 = 6.5;

fn ink() -> Color32 {
    layout::text_color()
}

fn fam(name: &str) -> FontFamily {
    layout::family(name)
}

/// Draws the popover's contents at the top left of `ui` (the shell's popup holds it).
pub fn show(ed: &mut Editor, ui: &mut egui::Ui) {
    layout::install_faces(ui.ctx(), false);
    let (rect, _) = ui.allocate_exact_size(SIZE, Sense::hover());
    let p = ui.painter().clone();
    let o = rect.min;
    let st = ed.state();
    let rule = Stroke::new(0.5, Color32::from_gray(170));
    // ---- the top row
    let row_c = |i: usize| pos2(o.x + BUTTONS_X[i], o.y + TOP_Y);
    let buttons: [(&str, bool); 4] = [("B", st.bold), ("I", st.italic), ("U", st.underline), ("S", st.strike)];
    for (i, (name, on)) in buttons.iter().enumerate() {
        let r = Rect::from_center_size(row_c(i), vec2(24.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with(("aa-btn", i)), Sense::click());
        if *on {
            p.rect_filled(r, CornerRadius::same(5), layout::accent());
        } else if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(5), Color32::from_black_alpha(14));
        }
        let col = if *on { Color32::WHITE } else { ink() };
        let c = r.center();
        match *name {
            "B" => {
                p.text(c, Align2::CENTER_CENTER, "B", FontId::new(17.0, fam("Sfera-Bold")), col);
            }
            "I" => {
                // SF Symbols' "italic": a slanted stem with serifs
                let st = Stroke::new(1.3, col);
                p.line_segment([pos2(c.x + 2.2, c.y - 6.5), pos2(c.x - 2.2, c.y + 6.5)], st);
                p.hline(c.x - 0.8..=c.x + 5.2, c.y - 6.5, st);
                p.hline(c.x - 5.2..=c.x + 0.8, c.y + 6.5, st);
            }
            "U" => {
                p.text(c + vec2(0.0, -1.0), Align2::CENTER_CENTER, "U", FontId::new(16.0, fam("Sfera-Regular")), col);
                p.hline(c.x - 6.0..=c.x + 6.0, c.y + 8.5, Stroke::new(1.2, col));
            }
            _ => {
                p.text(c, Align2::CENTER_CENTER, "S", FontId::new(16.0, fam("Sfera-Regular")), col);
                p.hline(c.x - 7.0..=c.x + 7.0, c.y + 0.5, Stroke::new(1.2, col));
            }
        }
        if resp.clicked() {
            ed.command(Some(ui.ctx()), [super::Cmd::Bold, super::Cmd::Italic, super::Cmd::Underline, super::Cmd::Strike][i].clone());
        }
    }
    // the divider between S and the highlighter
    let dx = o.x + 122.5;
    p.vline(dx, o.y + TOP_Y - 11.0..=o.y + TOP_Y + 11.0, Stroke::new(0.5, Color32::from_gray(150)));
    // the highlighter: toggles the highlight in the chosen colour
    let hr = Rect::from_center_size(row_c(4), vec2(24.0, 24.0));
    let hresp = ui.interact(hr, ui.scope_id().with("aa-pen"), Sense::click());
    let hl_on = st_highlight(ed).is_some();
    if hl_on {
        p.rect_filled(hr, CornerRadius::same(5), layout::accent());
    } else if hresp.hovered() {
        p.rect_filled(hr, CornerRadius::same(5), Color32::from_black_alpha(14));
    }
    pen(&p, hr.center(), if hl_on { Color32::WHITE } else { ink() });
    if hresp.clicked() {
        let c = ed.last_highlight;
        ed.command(Some(ui.ctx()), super::Cmd::Highlight(if hl_on { None } else { Some(c) }));
    }
    // the colour dot: opens the colour menu
    let dc = row_c(5);
    let dr = Rect::from_center_size(dc, vec2(16.0, 24.0));
    let dresp = ui.interact(dr, ui.scope_id().with("aa-dot"), Sense::click());
    if ed.popover.colors_open {
        p.rect_filled(dr, CornerRadius::same(5), layout::accent());
    }
    p.circle_filled(dc, 3.2, layout::highlight_dot(ed.last_highlight));
    if dresp.clicked() {
        ed.popover.colors_open = !ed.popover.colors_open;
    }
    p.hline(o.x + INSET..=o.x + SIZE.x - INSET, o.y + RULE1_Y, rule);
    // ---- the styles, each in its own look
    let styles: [(ParaStyle, &str, &str, f32); 8] = [
        (ParaStyle::Title, "Title", "SferaDisplay-Bold", 20.5),
        (ParaStyle::Heading, "Heading", "Sfera-Bold", 16.3),
        (ParaStyle::Subheading, "Subheading", "Sfera-Bold", 13.5),
        (ParaStyle::Body, "Body", "Sfera-Regular", 13.5),
        (ParaStyle::Monostyled, "Monostyled", "SferaMono-Regular", 12.2),
        (ParaStyle::Bulleted, "\u{2022} Bulleted List", "Sfera-Regular", 13.5),
        (ParaStyle::Dashed, "\u{2013} Dashed List", "Sfera-Regular", 13.5),
        (ParaStyle::Numbered, "1. Numbered List", "Sfera-Regular", 13.5),
    ];
    let cur = st.style;
    for (i, (style, label, face, size)) in styles.iter().enumerate() {
        let base = o.y + FIRST_BASE + PITCH * i as f32;
        let row = Rect::from_min_max(pos2(o.x + INSET, base - PITCH * 0.72), pos2(o.x + SIZE.x - INSET, base + PITCH * 0.28));
        let resp = ui.interact(row, ui.scope_id().with(("aa-style", i)), Sense::click());
        if resp.hovered() {
            p.rect_filled(row, CornerRadius::same(6), Color32::from_black_alpha(10));
        }
        baseline_text(&p, pos2(o.x + TEXT_X, base), label, FontId::new(*size, fam(face)), ink());
        if cur == Some(*style) {
            check(&p, pos2(o.x + CHECK_X, base - 5.0), ink());
        }
        if resp.clicked() {
            if std::env::var_os("NT_EDITOR_SNAP").is_some() {
                eprintln!("popover click {label}");
            }
            ed.command(Some(ui.ctx()), super::Cmd::Style(*style));
        }
    }
    p.hline(o.x + INSET..=o.x + SIZE.x - INSET, o.y + RULE2_Y, rule);
    // Block Quote, with its bar
    let base = o.y + QUOTE_BASE;
    let row = Rect::from_min_max(pos2(o.x + INSET, base - PITCH * 0.72), pos2(o.x + SIZE.x - INSET, base + PITCH * 0.28));
    let resp = ui.interact(row, ui.scope_id().with("aa-quote"), Sense::click());
    if resp.hovered() {
        p.rect_filled(row, CornerRadius::same(6), Color32::from_black_alpha(10));
    }
    p.rect_filled(Rect::from_min_size(pos2(o.x + TEXT_X + 1.0, base - 11.0), vec2(2.0, 12.0)), CornerRadius::same(1), Color32::from_gray(110));
    baseline_text(&p, pos2(o.x + TEXT_X + 8.0, base), "Block Quote", FontId::new(13.5, fam("Sfera-Regular")), ink());
    if st_quote(ed) {
        check(&p, pos2(o.x + CHECK_X, base - 5.0), ink());
    }
    if resp.clicked() {
        ed.command(Some(ui.ctx()), super::Cmd::BlockQuote);
    }
    if ed.popover.colors_open {
        color_menu(ed, ui, pos2(rect.max.x - 10.0, o.y + TOP_Y - 14.0));
    }
}

fn st_highlight(ed: &Editor) -> Option<u8> {
    let f = ed.typing_fmt();
    if ed.chars_all(|x| x.highlight.is_some()) { f.highlight.or(Some(ed.last_highlight)) } else { None }
}

fn st_quote(ed: &Editor) -> bool {
    let r = ed.sel_paras();
    ed.flow(ed.head.cell)[r].iter().all(|p| p.quote)
}

/// The colour menu beside the dot: Purple, Pink, Orange, Mint, Blue with their dots.
fn color_menu(ed: &mut Editor, ui: &mut egui::Ui, at: Pos2) {
    let names = ["Purple", "Pink", "Orange", "Mint", "Blue"];
    let size = vec2(145.0, 10.0 + 24.0 * names.len() as f32);
    let mut pick = None;
    egui::Area::new(ui.scope_id().with("aa-colors")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        let (r, _) = ui.allocate_exact_size(size, Sense::hover());
        let p = ui.painter();
        p.rect_filled(r.translate(vec2(0.0, 3.0)).expand(2.0), CornerRadius::same(14), Color32::from_black_alpha(16));
        p.rect_filled(r, CornerRadius::same(12), if layout::dark() { Color32::from_gray(44) } else { Color32::from_gray(252) });
        for (i, name) in names.iter().enumerate() {
            let row = Rect::from_min_size(pos2(r.min.x + 5.0, r.min.y + 5.0 + 24.0 * i as f32), vec2(size.x - 10.0, 24.0));
            let resp = ui.interact(row, ui.scope_id().with(("aa-color", i)), Sense::click());
            if resp.hovered() {
                p.rect_filled(row, CornerRadius::same(7), layout::accent());
            }
            p.circle_filled(pos2(row.min.x + 14.5, row.center().y), 5.0, layout::highlight_dot(i as u8 + 1));
            p.text(pos2(row.min.x + 27.0, row.center().y), Align2::LEFT_CENTER, *name, FontId::new(13.5, fam("Sfera-Regular")), ink());
            if resp.clicked() {
                pick = Some(i as u8 + 1);
            }
        }
    });
    if let Some(c) = pick {
        ed.popover.colors_open = false;
        ed.command(Some(ui.ctx()), super::Cmd::Highlight(Some(c)));
    }
}

fn baseline_text(p: &Painter, at: Pos2, t: &str, font: FontId, c: Color32) {
    let g = p.layout_no_wrap(t.to_string(), font, c);
    let b = g.rows.first().and_then(|r| r.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
    p.galley(pos2(at.x, at.y - b), g, c);
}

fn check(p: &Painter, c: Pos2, col: Color32) {
    let pts = vec![pos2(c.x - 4.5, c.y), pos2(c.x - 1.2, c.y + 3.8), pos2(c.x + 4.8, c.y - 4.5)];
    p.add(egui::Shape::line(pts, Stroke::new(1.5, col)));
}

/// The highlighter pen with the line under it.
fn pen(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.3, col);
    let tip = pos2(c.x - 6.0, c.y + 5.0);
    let a = pos2(c.x + 3.5, c.y - 6.5);
    let b = pos2(c.x + 6.5, c.y - 3.5);
    let d = pos2(c.x - 3.0, c.y + 6.0);
    let e = pos2(c.x - 5.0, c.y + 2.0);
    p.add(egui::Shape::closed_line(vec![e, a, b, d, tip], s));
    p.line_segment([pos2(c.x + 1.5, c.y - 4.5), pos2(c.x + 4.5, c.y - 1.5)], s);
    p.hline(c.x - 8.0..=c.x + 8.0, c.y + 9.0, Stroke::new(1.3, col));
}
