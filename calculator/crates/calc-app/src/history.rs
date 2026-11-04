//! History: a panel sliding in from the left on phones, a sidebar on tablets. Groups, tap to
//! load, Edit with selection circles, Delete / Delete All, swipe a row left to delete,
//! right-click to copy.

use crate::app::{toolbar_button, App, PopAct, Popup};
use crate::draw::{self, col, fade};
use crate::fonts;
use egui::{pos2, vec2, Align2, Color32, Id, Rect, Sense, Stroke, Ui, UiBuilder};

pub fn panel(app: &mut App, ui: &mut Ui, screen: Rect, t: f32, w: f32, push: f32) {
    let th = app.theme();
    let p = ui.painter().clone();
    let r = Rect::from_min_size(pos2(screen.left() - w * (1.0 - t), screen.top()), vec2(w, screen.height()));
    // the pushed calculator dims; tapping it closes
    let dim = Rect::from_min_max(pos2(r.right(), screen.top()), screen.max);
    p.rect_filled(dim, 0.0, Color32::from_black_alpha((120.0 * t) as u8));
    let _ = push;
    if app.ov.history && ui.interact(screen, Id::unique("hist_scrim"), Sense::click()).clicked() {
        app.ov.history = false;
    }
    p.rect_filled(r, 0.0, col(th.panel));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(0.5, col(th.separator)));
    if app.ov.history {
        ui.interact(r, Id::unique("hist_body"), Sense::click());
    }
    list(app, ui, r, false, app.ov.history);
}

pub fn sidebar(app: &mut App, ui: &mut Ui, r: Rect, live: bool) {
    let th = app.theme();
    let p = ui.painter().clone();
    // iPadOS 26: a floating rounded card
    draw::rrect(&p, r, 22.0, col(th.panel), col(th.panel));
    draw::rrect_stroke(&p, r, 22.0, 0.5, Color32::from_white_alpha(if th.light { 0 } else { 14 }));
    list(app, ui, r, true, live);
}

fn list(app: &mut App, ui: &mut Ui, r: Rect, sidebar: bool, live: bool) {
    let th = app.theme();
    let p = ui.painter().clone();
    let accent = col(th.accent);
    let text_c = col(th.panel_text);
    let dim = col(th.panel_dim);
    let pad = 16.0;
    // header
    let head = Rect::from_min_size(pos2(r.left(), r.top() + 6.0), vec2(r.width(), 44.0));
    if !sidebar {
        let ib = Rect::from_min_size(pos2(r.left() + (r.width() * 0.045).clamp(12.0, 28.0) - 6.0, head.top()), vec2(44.0, 44.0));
        let mut down = false;
        if live {
            let resp = ui.interact(ib, Id::unique("hist_close"), Sense::click());
            down = resp.is_pointer_button_down_on();
            if resp.clicked() {
                app.ov.history = false;
            }
        }
        toolbar_button(&p, &th, ib, if th.glass_toolbar { "clock" } else { "list" }, down);
    }
    let edit = app.ov.hist_edit;
    let label = if edit { "Done" } else { "Edit" };
    // in the sidebar the toggle button sits top-right; Edit is a capsule left of it
    let toggle = app.lay.toolbar.iter().find(|b| b.id == "history").map(|b| draw::rect(b.rect)).filter(|b| sidebar && r.contains(b.center()));
    let er = match toggle {
        Some(b) => {
            let f = fonts::ui(500, 15.0);
            let w = draw::text_width(&p, label, f.clone()) + 26.0;
            let cap = Rect::from_min_size(pos2(b.left() - 8.0 - w, b.top()), vec2(w, b.height()));
            if th.glass_toolbar {
                let (top, bot) = crate::app::glass_fill(false);
                draw::glass_shape(&p, cap, b.height() / 2.0, top, bot);
            } else {
                p.rect_filled(cap, b.height() / 2.0, col(if th.light { 0xE5E5EAFF } else { 0x2C2C2EFF }));
            }
            draw::text(&p, cap.center(), Align2::CENTER_CENTER, label, f, if th.glass_toolbar { text_c } else { accent });
            cap.shrink(10.0)
        }
        None => draw::text(&p, pos2(r.right() - pad - 4.0, head.center().y), Align2::RIGHT_CENTER, label, fonts::ui(if edit { 600 } else { 400 }, 17.0), accent),
    };
    if live && ui.interact(er.expand(10.0), Id::unique(("hist_edit", sidebar)), Sense::click()).clicked() {
        app.ov.hist_edit = !edit;
        app.ov.hist_sel.clear();
        app.ov.hist_swiped = None;
    }
    let groups = app.s.history_groups(app.utc);
    let bottom_bar = if edit { 52.0 } else { 0.0 };
    let body = Rect::from_min_max(pos2(r.left(), head.bottom() + 8.0), pos2(r.right(), r.bottom() - bottom_bar - 8.0));
    if groups.is_empty() {
        draw::text(&p, body.center(), Align2::CENTER_CENTER, "No History", fonts::ui(600, 17.0), dim);
    }
    let mut load = None;
    let mut delete = Vec::new();
    let mut lui = ui.new_child(UiBuilder::new().max_rect(body));
    lui.set_clip_rect(body.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical().id_salt(("hist_list", sidebar)).auto_shrink(false).show(&mut lui, |ui| {
        let w = ui.available_width();
        let x0 = ui.max_rect().left();
        let mut first = true;
        for g in &groups {
            let (gr, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
            draw::text(ui.painter(), pos2(x0 + pad, gr.bottom() - 8.0), Align2::LEFT_BOTTOM, &g.title, fonts::ui(600, 15.0), dim);
            for (i, row) in g.rows.iter().enumerate() {
                let (rr, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click_and_drag());
                let id = row.id;
                let swiped = app.ov.hist_swiped == Some(id);
                let off = ui.ctx().animate_value_with_time(Id::unique(("hist_off", id, sidebar)), if swiped { -84.0 } else { 0.0 }, 0.2);
                if live {
                    if resp.dragged() && !edit {
                        if let Some(d) = resp.total_drag_delta() {
                            if d.x < -40.0 {
                                app.ov.hist_swiped = Some(id);
                            } else if d.x > 20.0 && swiped {
                                app.ov.hist_swiped = None;
                            }
                        }
                    }
                    if resp.clicked() {
                        if edit {
                            if !app.ov.hist_sel.remove(&id) {
                                app.ov.hist_sel.insert(id);
                            }
                        } else if app.ov.hist_swiped.is_some() {
                            app.ov.hist_swiped = None;
                        } else {
                            load = Some(id);
                        }
                    }
                    if resp.secondary_clicked() {
                        if let Some(text) = app.s.history_text(id) {
                            let pos = resp.interact_pointer_pos().unwrap_or(rr.center());
                            app.ov.popup = Some(Popup { pos, items: vec![("Copy", PopAct::CopyText(text))] });
                        }
                    }
                }
                if !ui.is_rect_visible(rr) {
                    continue;
                }
                let p = ui.painter().clone();
                if off < -0.5 {
                    let del = Rect::from_min_max(pos2(rr.right() + off, rr.top()), rr.max);
                    p.rect_filled(del, 0.0, col(0xFF3B30FF));
                    draw::text(&p, del.center(), Align2::CENTER_CENTER, "Delete", fonts::ui(500, 15.0), Color32::WHITE);
                    if live && swiped && ui.interact(del, Id::unique(("hist_del", id, sidebar)), Sense::click()).clicked() {
                        delete.push(id);
                    }
                }
                let rr2 = rr.translate(vec2(off, 0.0));
                let current = sidebar && (app.hist_current == Some(id) || app.hist_current.is_none() && app.disp.is_result && first);
                first = false;
                if current {
                    p.rect_filled(rr2.shrink2(vec2(8.0, 2.0)), 12.0, if th.light { Color32::from_black_alpha(16) } else { Color32::from_white_alpha(26) });
                }
                if resp.is_pointer_button_down_on() && !resp.dragged() {
                    p.rect_filled(rr2, 0.0, if th.light { Color32::from_black_alpha(12) } else { Color32::from_white_alpha(12) });
                }
                let mut x = rr2.left() + pad;
                if edit {
                    let sel = app.ov.hist_sel.contains(&id);
                    draw::icon(&p, if sel { "checkcircle" } else { "circle" }, Rect::from_center_size(pos2(x + 11.0, rr2.center().y), vec2(22.0, 22.0)), if sel { accent } else { dim });
                    x += 38.0;
                }
                let face = th.font;
                let f13 = move |s: f32| fonts::font(if face == "chicago" { face } else { "system" }, 400, s);
                match &row.conv {
                    Some((a, b)) => {
                        draw::text(&p, pos2(x, rr2.top() + 26.0), Align2::LEFT_BOTTOM, a, f13(14.0), dim);
                        draw::text(&p, pos2(x, rr2.top() + 52.0), Align2::LEFT_BOTTOM, b, f13(20.0), text_c);
                    }
                    None => {
                        let l = draw::segs(&p, &row.expr, 14.0, &f13);
                        l.paint(&p, x, rr2.top() + 24.0, dim);
                        let l = draw::segs(&p, &[calc_core::calc::Seg { t: row.result.clone(), sup: false }], 20.0, &f13);
                        l.paint(&p, x, rr2.top() + 50.0, text_c);
                    }
                }
                draw::text(&p, pos2(rr2.right() - pad, rr2.top() + 24.0), Align2::RIGHT_BOTTOM, &row.date, f13(13.0), dim);
                if i + 1 < g.rows.len() && !sidebar {
                    p.line_segment([pos2(x, rr2.bottom() - 0.5), pos2(rr.right(), rr2.bottom() - 0.5)], Stroke::new(0.6, col(th.separator)));
                }
            }
        }
    });
    if edit {
        let bar = Rect::from_min_max(pos2(r.left(), r.bottom() - bottom_bar), r.max);
        p.line_segment([bar.left_top(), bar.right_top()], Stroke::new(0.6, col(th.separator)));
        let any = !app.ov.hist_sel.is_empty();
        let dr = draw::text(&p, pos2(bar.left() + pad + 4.0, bar.center().y), Align2::LEFT_CENTER, "Delete", fonts::ui(400, 17.0), fade(accent, if any { 1.0 } else { 0.35 }));
        let ar = draw::text(&p, pos2(bar.right() - pad - 4.0, bar.center().y), Align2::RIGHT_CENTER, "Delete All", fonts::ui(400, 17.0), accent);
        if live && any && ui.interact(dr.expand(10.0), Id::unique(("hist_delsel", sidebar)), Sense::click()).clicked() {
            delete.extend(app.ov.hist_sel.drain());
        }
        if live && ui.interact(ar.expand(10.0), Id::unique(("hist_delall", sidebar)), Sense::click()).clicked() {
            app.s.history_clear();
            app.ov.hist_sel.clear();
            app.ov.hist_edit = false;
        }
    }
    if !delete.is_empty() {
        app.s.history_delete(&delete);
        app.ov.hist_swiped = None;
    }
    if let Some(id) = load {
        app.s.history_load(id);
        app.hist_current = Some(id);
        if !sidebar {
            app.ov.history = false;
        }
    }
}
