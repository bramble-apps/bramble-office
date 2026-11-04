//! Convert mode: the two value rows with unit buttons and ⇅, and the Units sheet (category
//! capsules, unit list with ✓, search across all units, currency "Updated … ago").

use crate::app::App;
use crate::draw::{self, col, fade};
use crate::{fonts, settings};
use calc_core::session::now;
use calc_core::theme::Family;
use calc_core::units;
use egui::{pos2, vec2, Align, Align2, Color32, Id, Rect, Sense, Stroke, Ui, UiBuilder};

fn ago(updated: i64) -> String {
    let m = (now() - updated).max(0) / 60;
    match m {
        0 => "Updated just now".into(),
        1 => "Updated 1 minute ago".into(),
        2..=59 => format!("Updated {m} minutes ago"),
        60..=119 => "Updated 1 hour ago".into(),
        120..=2879 => format!("Updated {} hours ago", m / 60),
        _ => format!("Updated {} days ago", m / 1440),
    }
}

/// The value rows. The top one shows what is typed, the lower one the converted value in grey.
pub fn rows(app: &mut App, ui: &mut Ui, live: bool) {
    let (Some(top), Some(bot), Some(sw)) = (app.lay.conv_top, app.lay.conv_bottom, app.lay.conv_swap) else { return };
    let Some(conv) = app.conv.clone() else { return };
    let t = app.theme();
    let p = ui.painter().clone();
    let (top, bot, sw) = (draw::rect(top), draw::rect(bot), draw::rect(sw));
    let disp = draw::rect(app.lay.display);
    let ds = &t.display;
    if ds.bg & 0xFF != 0 {
        let rad = ds.radius * disp.height();
        draw::rrect(&p, disp, rad, col(ds.bg), col(ds.bg2));
        draw::rrect_stroke(&p, disp, rad, 1.0, col(ds.border));
    }
    let f = app.lay.main_font;
    let inset = app.lay.text_inset;
    let face = t.font;
    let rw = t.result_weight;
    let main_f = move |sz: f32| fonts::font(face, rw, sz);
    let unit_f = fonts::font(face, t.label_weight.min(400), (f * 0.3).clamp(13.0, 26.0));
    let grey = col(ds.expr);
    let swap_p = ((app.now - app.swap_t0) / 0.3).clamp(0.0, 1.0) as f32;
    let mut open = None;
    for (i, (row, segs, sym, c)) in [
        (top, app.disp.main.clone(), conv.from.symbol.clone(), col(ds.text)),
        (bot, vec![calc_core::calc::Seg { t: conv.out.clone(), sup: false }], conv.to.symbol.clone(), grey),
    ]
    .into_iter()
    .enumerate()
    {
        let base = row.bottom() - row.height() * 0.14;
        // unit button: symbol + ⌃⌄
        let uw = draw::text_width(&p, &sym, unit_f.clone());
        let ih = unit_f.size * 0.9;
        let btn = Rect::from_min_max(pos2(row.right() - inset - uw - ih - 4.0, base - unit_f.size * 1.1), pos2(row.right() - inset + 4.0, base + 6.0));
        let mut down = false;
        if live {
            let r = ui.interact(btn, Id::unique(("unit_btn", i)), Sense::click());
            down = r.is_pointer_button_down_on();
            if r.clicked() {
                open = Some(i == 0);
            }
        }
        let uc = fade(grey, if down { 0.5 } else { 1.0 });
        let tr = draw::text(&p, pos2(btn.left(), base), Align2::LEFT_BOTTOM, &sym, unit_f.clone(), uc);
        draw::icon(&p, "updown", Rect::from_center_size(pos2(tr.right() + ih / 2.0 + 1.0, tr.center().y + 1.0), vec2(ih, ih)), uc);
        // the number
        let avail = btn.left() - 8.0 - row.left();
        let l = draw::segs(&p, &segs, f, &main_f);
        let sz = draw::fit(l.width, f, avail);
        let l = if sz < f { draw::segs(&p, &segs, sz, &main_f) } else { l };
        let a = if swap_p < 1.0 { 0.3 + 0.7 * swap_p } else { 1.0 };
        l.paint_right(&p, btn.left() - 6.0, base + (1.0 - swap_p) * 6.0 * if i == 0 { -1.0 } else { 1.0 }, fade(c, a));
        if i == 0 && live {
            let hit = Rect::from_min_max(row.min, pos2(btn.left() - 2.0, row.bottom()));
            app.display_input(ui, hit);
        }
    }
    // divider and ⇅
    let ly = (top.bottom() + bot.top()) / 2.0;
    p.line_segment([pos2(sw.right() + 6.0, ly), pos2(top.right(), ly)], Stroke::new(1.0, col(t.separator)));
    let mut down = false;
    if live {
        let r = ui.interact(sw, Id::unique("swap_btn"), Sense::click());
        down = r.is_pointer_button_down_on();
        if r.clicked() {
            app.s.swap_units();
            app.swap_t0 = app.now;
        }
    }
    draw::icon(&p, "swap", Rect::from_center_size(pos2(sw.center().x, ly), vec2(24.0, 24.0)), fade(col(t.accent), if down { 0.5 } else { 1.0 }));
    if let Some(top) = open {
        app.open_units(top);
    }
}

fn search_field(app: &mut App, ui: &mut Ui, r: Rect) {
    let t = app.theme();
    let p = ui.painter().clone();
    let fill = if t.light { Color32::from_black_alpha(14) } else { Color32::from_white_alpha(22) };
    p.rect_filled(r, r.height() / 2.0, fill);
    let dim = col(t.panel_dim);
    draw::icon(&p, "search", Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), vec2(18.0, 18.0)), dim);
    draw::icon(&p, "mic", Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(18.0, 18.0)), dim);
    let er = Rect::from_min_max(pos2(r.left() + 40.0, r.top()), pos2(r.right() - 40.0, r.bottom()));
    let mut cui = ui.new_child(UiBuilder::new().max_rect(er).layout(egui::Layout::left_to_right(Align::Center)));
    let te = egui::TextEdit::singleline(&mut app.ov.units_q)
        .frame(egui::Frame::NONE)
        .hint_text(egui::RichText::new("Search all units").color(dim).size(17.0))
        .font(fonts::ui(400, 17.0))
        .text_color(col(t.panel_text))
        .desired_width(er.width());
    cui.add(te);
}

pub fn sheet(app: &mut App, ui: &mut Ui, screen: Rect, t: f32) {
    let content = settings::frame(app, ui, screen, t, "Units");
    let th = app.theme();
    let glass = th.family == Family::Modern26;
    let live = app.ov.sheet_open;
    let accent = col(th.accent);
    let text_c = col(th.panel_text);
    let dim = col(th.panel_dim);
    let pad = 16.0;
    let mut r = content;
    // search: iOS 18 on top, iOS 26 at the bottom
    let search_h = if glass { 48.0 } else { 38.0 };
    let search = if glass {
        let s = Rect::from_min_size(pos2(r.left() + pad, r.bottom() - search_h - 14.0), vec2(r.width() - 2.0 * pad, search_h));
        r.max.y = s.top() - 8.0;
        s
    } else {
        let s = Rect::from_min_size(pos2(r.left() + pad, r.top()), vec2(r.width() - 2.0 * pad, search_h));
        r.min.y = s.bottom() + 10.0;
        s
    };
    search_field(app, ui, search);
    let searching = !app.ov.units_q.trim().is_empty();

    let cats = units::categories(&app.s.rates);
    // category capsules
    if !searching {
        let cr = Rect::from_min_size(r.min, vec2(r.width(), 40.0));
        r.min.y = cr.bottom() + 6.0;
        let mut cui = ui.new_child(UiBuilder::new().max_rect(cr));
        cui.set_clip_rect(cr);
        let scroll_now = !app.ov.units_scrolled;
        app.ov.units_scrolled = t >= 1.0;
        egui::ScrollArea::horizontal().id_salt("unit_cats").auto_shrink(false).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut cui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(pad - 4.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                for c in &cats {
                    let sel = c.id == app.ov.units_cat;
                    let f = fonts::ui(if sel { 700 } else { 600 }, 15.0);
                    let tw = draw::text_width(ui.painter(), c.name, f.clone());
                    let (cell, resp) = ui.allocate_exact_size(vec2(tw + 24.0, 32.0), Sense::click());
                    if sel {
                        ui.painter().rect_filled(cell, 16.0, accent);
                        if scroll_now {
                            ui.scroll_to_rect(cell, Some(Align::Center));
                        }
                    }
                    draw::text(ui.painter(), cell.center(), Align2::CENTER_CENTER, c.name, f, if sel { Color32::WHITE } else { fade(accent, if resp.is_pointer_button_down_on() { 0.5 } else { 1.0 }) });
                    if live && resp.clicked() {
                        app.ov.units_cat = c.id.to_string();
                        app.ov.units_list_scrolled = false;
                    }
                }
                ui.add_space(pad);
            });
        });
    }
    ui.painter().line_segment([pos2(r.left(), r.top()), pos2(r.right(), r.top())], Stroke::new(0.6, col(th.separator)));
    // the list starts a little below the capsules' separator, so no row runs under it
    r.min.y += 6.0;

    // the list
    let s = &app.s.settings;
    let cur_unit = if app.ov.units_top { s.conv_from.clone() } else { s.conv_to.clone() };
    let cur_cat = s.conv_cat.clone();
    let rows: Vec<(String, units::Unit, String)> = if searching {
        units::search(&app.s.rates, &app.ov.units_q)
            .into_iter()
            .map(|(c, u)| {
                let name = cats.iter().find(|k| k.id == c).map(|k| k.name).unwrap_or(c);
                (c.to_string(), u, name.to_string())
            })
            .collect()
    } else {
        cats.iter().find(|c| c.id == app.ov.units_cat).map(|c| c.units.iter().map(|u| (c.id.to_string(), u.clone(), String::new())).collect()).unwrap_or_default()
    };
    // keep asking until the sheet has settled so the list knows its height
    let scroll_list = !app.ov.units_list_scrolled;
    app.ov.units_list_scrolled = t >= 1.0;
    let currency = !searching && app.ov.units_cat == "currency";
    let mut pick: Option<(String, String)> = None;
    let mut lui = ui.new_child(UiBuilder::new().max_rect(r));
    lui.set_clip_rect(r.intersect(screen));
    egui::ScrollArea::vertical().id_salt(("unit_list", app.ov.units_cat.clone(), searching)).auto_shrink(false).show(&mut lui, |ui| {
        let w = ui.available_width();
        let row_h = 60.0;
        for (i, (cat, u, cat_name)) in rows.iter().enumerate() {
            let (rr, resp) = ui.allocate_exact_size(vec2(w, row_h), Sense::click());
            let sel = *cat == cur_cat && u.id == cur_unit;
            if sel && scroll_list {
                // the chosen unit third from the top, the rows above it whole (a centred row
                // left a cut-off one at the top)
                ui.scroll_to_rect(Rect::from_min_max(pos2(rr.left(), rr.top() - 2.0 * row_h), rr.max), Some(Align::Min));
            }
            if !ui.is_rect_visible(rr) {
                continue;
            }
            let p = ui.painter().clone();
            if resp.is_pointer_button_down_on() {
                p.rect_filled(rr, 0.0, if th.light { Color32::from_black_alpha(14) } else { Color32::from_white_alpha(14) });
            }
            let x = rr.left() + pad + 24.0;
            if sel {
                draw::icon(&p, "check", Rect::from_center_size(pos2(rr.left() + pad + 8.0, rr.top() + 22.0), vec2(16.0, 16.0)), accent);
            }
            draw::text(&p, pos2(x, rr.top() + 22.0), Align2::LEFT_CENTER, &u.name, fonts::ui(400, 17.0), if sel { accent } else { text_c });
            let sub = if cat_name.is_empty() { u.symbol.clone() } else { format!("{}  ·  {}", u.symbol, cat_name) };
            draw::text(&p, pos2(x, rr.top() + 43.0), Align2::LEFT_CENTER, &sub, fonts::ui(400, 13.0), if sel { fade(accent, 0.8) } else { dim });
            if i + 1 < rows.len() {
                p.line_segment([pos2(x, rr.bottom() - 0.5), pos2(rr.right() - pad, rr.bottom() - 0.5)], Stroke::new(0.6, col(th.separator)));
            }
            if live && resp.clicked() {
                pick = Some((cat.clone(), u.id.clone()));
            }
        }
        if rows.is_empty() {
            let (rr, _) = ui.allocate_exact_size(vec2(w, 80.0), Sense::hover());
            draw::text(ui.painter(), rr.center(), Align2::CENTER_CENTER, "No Results", fonts::ui(600, 17.0), dim);
        }
        if currency {
            let (rr, _) = ui.allocate_exact_size(vec2(w, 56.0), Sense::hover());
            let p = ui.painter().clone();
            draw::text(&p, pos2(rr.left() + pad + 8.0, rr.center().y), Align2::LEFT_CENTER, &app.s.rates.source, fonts::ui(700, 15.0), dim);
            draw::text(&p, pos2(rr.right() - pad, rr.center().y), Align2::RIGHT_CENTER, &ago(app.s.rates.updated), fonts::ui(400, 13.0), dim);
        }
    });
    if let Some((cat, unit)) = pick {
        app.s.set_unit(app.ov.units_top, &cat, &unit);
        app.ov.sheet_open = false;
    }
}
