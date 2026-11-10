//! The mode menu popover (iOS 26 ✓ rows, iOS 18 accent row + Convert switch) and the small
//! Copy/Paste edit menu.

use crate::app::App;
use crate::draw::{self, col, fade};
use crate::fonts;
use calc_core::layout::Mode;
use calc_core::theme::Family;
use egui::{pos2, vec2, Align2, Color32, Id, Pos2, Rect, Sense, Stroke, Ui};

const ROW: f32 = 46.0;
const GAP: f32 = 9.0;
const W: f32 = 250.0;

enum Item {
    Mode(&'static str, &'static str, Mode),
    Convert,
    Themes,
    /// "Update Available…" (only when there is one)
    Update,
    /// the thank-you line with its Support development link
    Thanks,
    Gap,
}

const THANKS_ROW: f32 = 40.0;

/// An iOS switch.
pub fn switch(p: &egui::Painter, r: Rect, on: bool, a: f32) {
    let track = if on { col(0x34C759FF) } else { col(0x39393DFF) };
    p.rect_filled(r, r.height() / 2.0, fade(track, a));
    let k = r.height() / 2.0 - 2.0;
    let x = if on { r.right() - k - 2.0 } else { r.left() + k + 2.0 };
    p.circle_filled(pos2(x, r.center().y), k + 0.5, fade(Color32::from_black_alpha(40), a));
    p.circle_filled(pos2(x, r.center().y), k, fade(Color32::WHITE, a));
}

/// A soft drop shadow under a rounded panel.
pub fn shadow(p: &egui::Painter, r: Rect, rad: f32, a: f32) {
    for i in 1..=6 {
        let e = i as f32 * 3.0;
        draw::rrect(p, r.expand(e).translate(vec2(0.0, e * 0.5)), rad + e, Color32::from_black_alpha((10.0 * a) as u8), Color32::from_black_alpha((10.0 * a) as u8));
    }
}

/// A row's icon; the update row has the suite's small red dot instead.
fn icon_or_dot(p: &egui::Painter, icon: &str, r: Rect, c: Color32, bg: Color32) {
    if icon.is_empty() {
        kn_ui::update::paint_dot(p, r.center(), bg);
    } else {
        draw::icon(p, icon, r, c);
    }
}

pub fn menu(app: &mut App, ui: &mut Ui, screen: Rect, t: f32) {
    let th = app.theme();
    let ios18 = th.family == Family::Modern18;
    let mut items = vec![
        Item::Mode("Basic", "basic", Mode::Basic),
        Item::Mode("Scientific", "function", Mode::Scientific),
        Item::Mode("Math Notes", "mathnotes", Mode::Notes),
        Item::Gap,
        Item::Convert,
        Item::Gap,
        Item::Themes,
    ];
    if kn_ui::update::available() {
        items.push(Item::Update);
    }
    items.push(Item::Gap);
    items.push(Item::Thanks);
    let row_h = |i: &Item| match i {
        Item::Gap => GAP,
        Item::Thanks => THANKS_ROW,
        _ => ROW,
    };
    let h: f32 = items.iter().map(row_h).sum::<f32>() + 12.0;
    let a = app.ov.menu_anchor;
    let right = a.center().x > screen.center().x;
    let down = a.center().y < screen.center().y;
    let x = if right { a.right() - W } else { a.left() };
    let y = if down { a.top() } else { a.bottom() - h };
    let full = Rect::from_min_size(pos2(x, y), vec2(W, h));
    let full = full.translate(vec2(
        (screen.left() + 8.0 - full.left()).max(0.0) + (screen.right() - 8.0 - full.right()).min(0.0),
        (screen.top() + 8.0 - full.top()).max(0.0) + (screen.bottom() - 8.0 - full.bottom()).min(0.0),
    ));
    let pivot = pos2(if right { full.right() } else { full.left() }, if down { full.top() } else { full.bottom() });
    let s = 0.55 + 0.45 * t;
    let m = |q: Pos2| pivot + (q - pivot) * s;
    let mr = |r: Rect| Rect::from_min_max(m(r.min), m(r.max));

    // tapping outside closes
    if app.ov.menu && ui.interact(screen, Id::unique("menu_scrim"), Sense::click()).clicked() {
        app.ov.menu = false;
    }
    let p = ui.painter().clone();
    p.rect_filled(screen, 0.0, Color32::from_black_alpha(((if th.glass_toolbar { 50.0 } else { 110.0 }) * t) as u8));
    let panel = mr(full);
    shadow(&p, panel, 14.0 * s, t);
    let bgc = col(th.panel);
    draw::rrect(&p, panel, 14.0 * s, fade(bgc, t), fade(bgc, t));
    draw::rrect_stroke(&p, panel, 14.0 * s, 0.5, fade(Color32::from_white_alpha(if th.light { 0 } else { 20 }), t));
    if app.ov.menu {
        ui.interact(panel, Id::unique("menu_panel"), Sense::click());
    }
    let text_c = fade(col(th.panel_text), t);
    let accent = fade(col(th.accent), t);
    let fsz = 17.0 * s;
    let mut yy = full.top() + 6.0;
    let mode = app.s.settings.mode;
    let conv = app.s.settings.convert;
    let mut chosen: Option<usize> = None;
    for (i, it) in items.iter().enumerate() {
        if matches!(it, Item::Gap) {
            let ly = yy + GAP / 2.0;
            p.line_segment([m(pos2(full.left() + 14.0, ly)), m(pos2(full.right() - 14.0, ly))], Stroke::new(if ios18 { 0.6 } else { 1.0 }, fade(col(th.separator), t)));
            yy += GAP;
            continue;
        }
        let row = Rect::from_min_size(pos2(full.left(), yy), vec2(W, row_h(it)));
        yy += row_h(it);
        if matches!(it, Item::Thanks) {
            // small and quiet, as the other apps' menu footers
            let weak = fade(col(th.panel_text).gamma_multiply(0.55), t);
            let f = fonts::ui(400, 11.5 * s);
            let tr = draw::text(&p, m(pos2(row.left() + 16.0, row.top() + 12.0)), Align2::LEFT_CENTER, "Thank you for using Bramble Office", f.clone(), weak);
            // (the system sans may have no ♥: drawn)
            kn_ui::update::paint_heart(&p, pos2(tr.right() + 8.0 * s, tr.center().y), 4.0 * s, weak);
            let lr = draw::text(&p, m(pos2(row.left() + 16.0, row.top() + 28.0)), Align2::LEFT_CENTER, "Support development", f, accent);
            if app.ov.menu {
                let r = ui.interact(lr.expand(3.0), Id::unique("menu_thanks"), Sense::click());
                if r.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if r.clicked() {
                    chosen = Some(i);
                }
            }
            continue;
        }
        if app.ov.menu {
            let r = ui.interact(mr(row), Id::unique(("menu_row", i)), Sense::click());
            if r.is_pointer_button_down_on() || r.hovered() {
                p.rect_filled(mr(row.shrink2(vec2(6.0, 1.0))), 8.0 * s, fade(Color32::from_gray(if th.light { 0 } else { 255 }).gamma_multiply(0.08), t));
            }
            if r.clicked() {
                chosen = Some(i);
            }
        }
        let cy = row.center().y;
        let (label, icon, current) = match it {
            Item::Mode(l, ic, md) => (*l, *ic, *md == mode),
            Item::Convert => ("Convert", "convert", conv),
            Item::Themes => ("Themes…", "paintbrush", false),
            Item::Update => ("Update Available…", "", false),
            Item::Gap | Item::Thanks => unreachable!(),
        };
        let ic_r = |x: f32| mr(Rect::from_center_size(pos2(x, cy), vec2(20.0, 20.0)));
        if ios18 {
            let c = if current && !matches!(it, Item::Convert) { accent } else { text_c };
            if matches!(it, Item::Convert) {
                switch(&p, mr(Rect::from_min_size(pos2(row.left() + 16.0, cy - 13.0), vec2(46.0, 26.0))), conv, t);
            } else {
                icon_or_dot(&p, icon, ic_r(row.left() + 32.0), c, bgc);
            }
            draw::text(&p, m(pos2(row.left() + 76.0, cy)), Align2::LEFT_CENTER, label, fonts::ui(400, fsz), c);
        } else {
            if current {
                draw::icon(&p, "check", mr(Rect::from_center_size(pos2(row.left() + 22.0, cy), vec2(15.0, 15.0))), text_c);
            }
            icon_or_dot(&p, icon, ic_r(row.left() + 50.0), text_c, bgc);
            draw::text(&p, m(pos2(row.left() + 76.0, cy)), Align2::LEFT_CENTER, label, fonts::ui(400, fsz), text_c);
        }
    }
    if let Some(i) = chosen {
        match &items[i] {
            Item::Mode(_, _, md) => {
                app.set_mode(*md);
                app.ov.menu = false;
            }
            Item::Convert => {
                app.set_convert(!conv);
                if !ios18 {
                    app.ov.menu = false;
                }
            }
            Item::Themes => {
                app.ov.menu = false;
                app.open_settings();
            }
            Item::Update => {
                app.ov.menu = false;
                kn_ui::update::open_sheet();
            }
            Item::Thanks => {
                app.ov.menu = false;
                let _ = kn_ui::open_command(kn_ui::DONATE_URL).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
            }
            Item::Gap => {}
        }
    }
}

/// The edit menu: a dark capsule of items above the pointer.
pub fn popup(app: &mut App, ui: &mut Ui, screen: Rect) {
    let Some(pop) = app.ov.popup.take() else { return };
    let p = ui.painter().clone();
    let f = fonts::ui(400, 15.0);
    let widths: Vec<f32> = pop.items.iter().map(|(l, _)| draw::text_width(&p, l, f.clone()) + 28.0).collect();
    let w: f32 = widths.iter().sum();
    let h = 38.0;
    let mut r = Rect::from_min_size(pos2(pop.pos.x - w / 2.0, pop.pos.y - h - 12.0), vec2(w, h));
    r = r.translate(vec2((screen.left() + 8.0 - r.left()).max(0.0) + (screen.right() - 8.0 - r.right()).min(0.0), (screen.top() + 8.0 - r.top()).max(0.0)));
    let scrim = ui.interact(screen, Id::unique("popup_scrim"), Sense::click());
    shadow(&p, r, 10.0, 1.0);
    p.rect_filled(r, 10.0, col(0x2C2C2EFF));
    let mut x = r.left();
    let mut picked = None;
    for (i, ((l, _), w)) in pop.items.iter().zip(&widths).enumerate() {
        let cell = Rect::from_min_size(pos2(x, r.top()), vec2(*w, h));
        let resp = ui.interact(cell, Id::unique(("popup", i)), Sense::click());
        if resp.is_pointer_button_down_on() {
            p.rect_filled(cell, 10.0, Color32::from_white_alpha(30));
        }
        if resp.clicked() {
            picked = Some(i);
        }
        draw::text(&p, cell.center(), Align2::CENTER_CENTER, l, f.clone(), Color32::WHITE);
        if i > 0 {
            p.line_segment([pos2(x, r.top() + 8.0), pos2(x, r.bottom() - 8.0)], Stroke::new(1.0, Color32::from_white_alpha(40)));
        }
        x += w;
    }
    if let Some(i) = picked {
        let act = pop.items.into_iter().nth(i).map(|it| it.1);
        if let Some(a) = act {
            app.popup_act(a);
        }
        return;
    }
    if scrim.clicked() || scrim.secondary_clicked() {
        return;
    }
    app.ov.popup = Some(pop);
}
