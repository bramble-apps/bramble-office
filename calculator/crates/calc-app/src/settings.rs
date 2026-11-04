//! The sheet frame shared by Units and Settings, and the Settings / Themes sheet: theme cards
//! with live mini previews, number format, haptics and sounds (stored; no-ops on desktop).

use crate::app::App;
use crate::draw::{self, col, fade, Look};
use crate::{fonts, menu};
use calc_core::calc::Calc;
use calc_core::format::NumberStyle;
use calc_core::layout::{self, Insets, LayoutIn, Mode};
use calc_core::theme::{self, Family, Theme};
use egui::{pos2, vec2, Align2, Color32, Id, Rect, Sense, Stroke, Ui, UiBuilder};

/// Panel fill for grouped rows inside a sheet.
pub fn group_fill(t: &Theme) -> Color32 {
    if t.light {
        Color32::WHITE
    } else {
        draw::mix(col(t.panel), Color32::WHITE, 0.07)
    }
}

/// Draws the sheet (sliding up by `t`) with a title and a Done button and returns the content
/// rect. Done, a swipe down on the header or a tap outside closes it.
pub fn frame(app: &mut App, ui: &mut Ui, screen: Rect, t: f32, title: &str) -> Rect {
    let th = app.theme();
    let glass = th.family == Family::Modern26;
    let p = ui.painter().clone();
    // dim behind
    p.rect_filled(screen, 0.0, Color32::from_black_alpha((110.0 * t) as u8));
    let w = (screen.width() - if glass { 16.0 } else { 0.0 }).min(620.0);
    let top_gap = if glass { 44.0 } else { 52.0 };
    let h = (screen.height() - top_gap - if glass { 8.0 } else { 0.0 }).min(900.0);
    let x = screen.center().x - w / 2.0;
    let slide = (1.0 - t) * (h + top_gap) + app.ov.sheet_drag.max(0.0);
    let r = Rect::from_min_size(pos2(x, screen.bottom() - h - if glass { 8.0 } else { 0.0 } + slide), vec2(w, h));
    let rad = if glass { 34.0 } else { 12.0 };
    if app.ov.sheet_open && ui.interact(screen, Id::unique("sheet_scrim"), Sense::click()).clicked() {
        app.ov.sheet_open = false;
    }
    menu::shadow(&p, r, rad, t * 0.6);
    let fill = col(if th.light { 0xF2F2F7FF } else if glass { 0x1C1C1EFF } else { th.panel });
    let body = if glass { r } else { Rect::from_min_max(r.min, pos2(r.right(), r.bottom() + rad)) };
    draw::rrect(&p, body, rad, fill, fill);
    if app.ov.sheet_open {
        ui.interact(r, Id::unique("sheet_body"), Sense::click());
    }
    // header: grabber, title, Done; dragging it down closes the sheet
    let head = Rect::from_min_size(r.min, vec2(w, 62.0));
    if app.ov.sheet_open {
        let hr = ui.interact(head, Id::unique("sheet_head"), Sense::drag());
        if hr.dragged() {
            app.ov.sheet_drag += hr.drag_delta().y;
        }
        if hr.drag_stopped() {
            if app.ov.sheet_drag > 90.0 {
                app.ov.sheet_open = false;
            }
            app.ov.sheet_drag = 0.0;
        }
    }
    p.rect_filled(Rect::from_center_size(pos2(r.center().x, r.top() + 7.0), vec2(36.0, 5.0)), 2.5, fade(col(th.panel_dim), 0.5));
    let text_c = col(th.panel_text);
    let accent = col(th.accent);
    draw::text(&p, pos2(r.center().x, r.top() + 32.0), Align2::CENTER_CENTER, title, fonts::ui(600, 17.0), text_c);
    let done = if glass {
        let c = pos2(r.right() - 36.0, r.top() + 32.0);
        let dr = Rect::from_center_size(c, vec2(44.0, 44.0));
        draw::glass_shape(&p, dr, 22.0, accent, draw::mix(accent, Color32::BLACK, 0.06));
        draw::icon(&p, "check", Rect::from_center_size(c, vec2(22.0, 22.0)), Color32::WHITE);
        dr
    } else {
        draw::text(&p, pos2(r.right() - 18.0, r.top() + 32.0), Align2::RIGHT_CENTER, "Done", fonts::ui(600, 17.0), accent)
            .expand(8.0)
    };
    if app.ov.sheet_open && ui.interact(done, Id::unique("sheet_done"), Sense::click()).clicked() {
        app.ov.sheet_open = false;
    }
    Rect::from_min_max(pos2(r.left(), head.bottom()), r.max)
}

/// A theme drawn small with the real layout engine: body, display "0", keys.
pub fn preview(p: &egui::Painter, th: &Theme, r: Rect) {
    let d = Calc::default().display(NumberStyle::Comma, th.family.classic());
    let lay = layout::layout(&LayoutIn {
        w: 390.0,
        h: 760.0,
        insets: Insets::default(),
        mode: Mode::Basic,
        convert: false,
        sidebar_open: false,
        theme: th,
        disp: &d,
        decimal: '.',
    });
    let s = r.width() / 390.0;
    let map = |q: calc_core::layout::Rect| Rect::from_min_size(r.min + vec2(q.x, q.y) * s, vec2(q.w, q.h) * s);
    draw::rrect(p, r, 12.0, col(th.bg), col(th.bg2));
    if let Some(b) = lay.body {
        draw::body(p, th, map(b));
    }
    let chrome = lay.body.map(map);
    let disp = map(lay.display);
    if th.display.bg & 0xFF != 0 {
        let rad = th.display.radius * disp.height();
        draw::rrect(p, disp, rad, col(th.display.bg), col(th.display.bg2));
        draw::rrect_stroke(p, disp, rad, 1.0, col(th.display.border));
    }
    let f = lay.main_font * s;
    let base = if th.display.boxed { disp.center().y + f * 0.36 } else { disp.bottom() };
    let fid = fonts::font(th.font, th.result_weight, f);
    let g = p.layout_no_wrap("0".into(), fid, Color32::WHITE);
    let bl = draw::baseline(&g);
    let gw = g.size().x;
    p.galley_with_override_text_color(pos2(disp.right() - lay.text_inset * s - gw, base - bl), g, col(th.display.text));
    for k in &lay.keys {
        let mut k2 = k.clone();
        k2.radius *= s;
        let look = Look { rect: map(k.rect), font: k.font * s, alpha: 1.0, press: 0.0, glow: 0.0, invert: false };
        draw::key(p, th, &k2, &look);
    }
    if let Some(b) = chrome {
        draw::body_chrome(p, th, b, lay.title_h * s);
    }
    for tb in &lay.toolbar {
        crate::app::toolbar_button(p, th, map(tb.rect), tb.icon, false);
    }
}

fn section(p: &egui::Painter, th: &Theme, x: f32, y: f32, s: &str) {
    draw::text(p, pos2(x + 16.0, y), Align2::LEFT_BOTTOM, &s.to_uppercase(), fonts::ui(400, 13.0), col(th.panel_dim));
}

pub fn sheet(app: &mut App, ui: &mut Ui, screen: Rect, t: f32) {
    let content = frame(app, ui, screen, t, "Settings");
    let live = app.ov.sheet_open;
    let mut ui = ui.new_child(UiBuilder::new().max_rect(content.shrink2(vec2(0.0, 4.0))));
    ui.set_clip_rect(content);
    egui::ScrollArea::vertical().id_salt("settings_scroll").auto_shrink(false).show(&mut ui, |ui| {
        let th = app.theme();
        let w = ui.available_width();
        let x0 = ui.max_rect().left();
        let pad = 16.0;
        let text_c = col(th.panel_text);
        let dim = col(th.panel_dim);
        let accent = col(th.accent);
        // themes
        let (hr, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
        section(ui.painter(), &th, x0, hr.bottom() - 4.0, "Theme");
        let all = theme::themes();
        let cols = if w > 560.0 { 3 } else { 2 };
        let gap = 14.0;
        let cw = (w - 2.0 * pad - gap * (cols as f32 - 1.0)) / cols as f32;
        let ch = cw * 760.0 / 390.0;
        let rows = all.len().div_ceil(cols);
        let (grid, _) = ui.allocate_exact_size(vec2(w, rows as f32 * (ch + 52.0)), Sense::hover());
        let mut pick = None;
        for (i, th2) in all.iter().enumerate() {
            let cx = grid.left() + pad + (i % cols) as f32 * (cw + gap);
            let cy = grid.top() + (i / cols) as f32 * (ch + 52.0);
            let card = Rect::from_min_size(pos2(cx, cy), vec2(cw, ch));
            if ui.is_rect_visible(card.expand(40.0)) {
                let p = ui.painter().clone();
                preview(&p, th2, card);
                let sel = th2.id == th.id;
                draw::rrect_stroke(&p, card.expand(if sel { 3.0 } else { 0.5 }), 14.0, if sel { 2.5 } else { 1.0 }, if sel { accent } else { col(th.separator) });
                draw::text(&p, pos2(card.center().x, card.bottom() + 16.0), Align2::CENTER_CENTER, th2.name, fonts::ui(600, 15.0), text_c);
                draw::text(&p, pos2(card.center().x, card.bottom() + 35.0), Align2::CENTER_CENTER, th2.year, fonts::ui(400, 13.0), dim);
            }
            if live && ui.interact(card.expand2(vec2(0.0, 0.0)).union(card.translate(vec2(0.0, 40.0))), Id::unique(("theme_card", i)), Sense::click()).clicked() {
                pick = Some(th2.id);
            }
        }
        if let Some(id) = pick {
            app.s.set_theme(id);
        }

        // number format
        let (hr, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
        section(ui.painter(), &th, x0, hr.bottom() - 6.0, "Number Format");
        let styles = NumberStyle::all();
        let row_h = 46.0;
        let (gr, _) = ui.allocate_exact_size(vec2(w, row_h * styles.len() as f32), Sense::hover());
        let g = gr.shrink2(vec2(pad, 0.0));
        ui.painter().rect_filled(g, 10.0, group_fill(&th));
        for (i, (st, ex)) in styles.iter().enumerate() {
            let rr = Rect::from_min_size(pos2(g.left(), g.top() + i as f32 * row_h), vec2(g.width(), row_h));
            let p = ui.painter().clone();
            draw::text(&p, pos2(rr.left() + 16.0, rr.center().y), Align2::LEFT_CENTER, ex, fonts::ui(400, 17.0), text_c);
            if app.s.settings.style == *st {
                draw::icon(&p, "check", Rect::from_center_size(pos2(rr.right() - 24.0, rr.center().y), vec2(17.0, 17.0)), accent);
            }
            if i > 0 {
                p.line_segment([pos2(rr.left() + 16.0, rr.top()), pos2(rr.right(), rr.top())], Stroke::new(0.6, col(th.separator)));
            }
            if live && ui.interact(rr, Id::unique(("style_row", i)), Sense::click()).clicked() {
                app.s.set_style(*st);
            }
        }

        // feedback
        let (hr, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
        section(ui.painter(), &th, x0, hr.bottom() - 6.0, "Feedback");
        let (gr, _) = ui.allocate_exact_size(vec2(w, row_h * 2.0), Sense::hover());
        let g = gr.shrink2(vec2(pad, 0.0));
        ui.painter().rect_filled(g, 10.0, group_fill(&th));
        for i in 0..2 {
            let rr = Rect::from_min_size(pos2(g.left(), g.top() + i as f32 * row_h), vec2(g.width(), row_h));
            let on = if i == 0 { app.s.settings.haptics } else { app.s.settings.sounds };
            let p = ui.painter().clone();
            draw::text(&p, pos2(rr.left() + 16.0, rr.center().y), Align2::LEFT_CENTER, if i == 0 { "Haptics" } else { "Sounds" }, fonts::ui(400, 17.0), text_c);
            menu::switch(&p, Rect::from_center_size(pos2(rr.right() - 40.0, rr.center().y), vec2(50.0, 30.0)), on, 1.0);
            if i > 0 {
                p.line_segment([pos2(rr.left() + 16.0, rr.top()), pos2(rr.right(), rr.top())], Stroke::new(0.6, col(th.separator)));
            }
            if live && ui.interact(rr, Id::unique(("fb_row", i)), Sense::click()).clicked() {
                if i == 0 {
                    app.s.settings.haptics = !on;
                } else {
                    app.s.settings.sounds = !on;
                }
                app.s.save_settings();
            }
        }
        let (fr, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
        draw::text(ui.painter(), pos2(fr.left() + pad + 16.0, fr.center().y), Align2::LEFT_CENTER, "Used by the Android app; the desktop has no haptics or key sounds.", fonts::ui(400, 12.0), dim);

        // about
        let (ar, _) = ui.allocate_exact_size(vec2(w, 70.0), Sense::hover());
        let about = format!("Calculator {} · a remake of Apple's Calculator · GPL-3.0", env!("CARGO_PKG_VERSION"));
        draw::text(ui.painter(), pos2(ar.center().x, ar.center().y - 8.0), Align2::CENTER_CENTER, &about, fonts::ui(400, 12.0), dim);
        draw::text(ui.painter(), pos2(ar.center().x, ar.center().y + 10.0), Align2::CENTER_CENTER, &format!("Font: {}", app.font_name), fonts::ui(400, 12.0), dim);
    });
}
