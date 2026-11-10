//! The control bar (Logic Pro 13, top of ui-tracks-with-pianoroll.png): view buttons on the
//! left (Library, Inspector, Mixer, Editors), the transport (Stop, Play, Record, Cycle), the
//! LCD (bar, beat, tempo, signature, key) and, on the right, the master level and the List /
//! Browser buttons. Buttons sit in rounded groups.

use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};

use crate::app::{App, Bottom};
use crate::look;

const BTN: f32 = 32.0;
const GAP: f32 = 4.0;

fn group(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, r.height() / 2.0, look::group_bg());
}

/// A round-cornered button in a group: returns its response. `on` draws it lit.
fn button(ui: &mut egui::Ui, r: Rect, id: &str, on: bool, tip: &str, draw: impl FnOnce(&egui::Painter, Rect, Color32)) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(("cb", id)), Sense::click()).on_hover_text(tip);
    let p = ui.painter();
    if on {
        p.rect_filled(r.shrink(2.0), (r.height() - 4.0) / 2.0, look::button_on());
    } else if resp.hovered() {
        p.rect_filled(r.shrink(2.0), (r.height() - 4.0) / 2.0, look::button_on().gamma_multiply(0.5));
    }
    draw(p, r, look::text());
    resp
}

fn icon(p: &egui::Painter, r: Rect, c: Color32, i: kn_ui::icons::Icon) {
    kn_ui::icons::draw(p, Rect::from_center_size(r.center(), vec2(18.0, 18.0)), i, c);
}

pub fn show(app: &mut App, ui: &mut egui::Ui, bar: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(bar, 0.0, look::bar_bg());
    p.hline(bar.x_range(), bar.max.y, Stroke::new(1.0, look::line()));
    let cy = bar.center().y;
    let h = 38.0;
    let mut x = bar.min.x + 10.0;
    // ---- view buttons: Library, Inspector | Mixer, Editors
    let g1 = Rect::from_min_size(pos2(x, cy - h / 2.0), vec2(2.0 * BTN + 3.0 * GAP, h));
    group(&p, g1);
    let mut bx = g1.min.x + GAP;
    let lib = button(ui, Rect::from_min_size(pos2(bx, g1.min.y + 3.0), vec2(BTN, h - 6.0)), "library", app.show_browser, "Browser (B)", |p, r, c| icon(p, r, c, kn_ui::icons::Icon::Sidebar));
    if lib.clicked() {
        app.show_browser = !app.show_browser;
    }
    bx += BTN + GAP;
    let insp = button(ui, Rect::from_min_size(pos2(bx, g1.min.y + 3.0), vec2(BTN, h - 6.0)), "inspector", app.show_inspector, "Inspector (I)", |p, r, c| icon(p, r, c, kn_ui::icons::Icon::Sliders));
    if insp.clicked() {
        app.show_inspector = !app.show_inspector;
    }
    x = g1.max.x + 8.0;
    let g2 = Rect::from_min_size(pos2(x, cy - h / 2.0), vec2(2.0 * BTN + 3.0 * GAP, h));
    group(&p, g2);
    bx = g2.min.x + GAP;
    let mixer = button(ui, Rect::from_min_size(pos2(bx, g2.min.y + 3.0), vec2(BTN, h - 6.0)), "mixer", app.bottom == Bottom::Mixer, "Mixer (X)", |p, r, c| mixer_icon(p, r, c));
    if mixer.clicked() {
        app.bottom = if app.bottom == Bottom::Mixer { Bottom::None } else { Bottom::Mixer };
    }
    bx += BTN + GAP;
    let ed = button(ui, Rect::from_min_size(pos2(bx, g2.min.y + 3.0), vec2(BTN, h - 6.0)), "editor", app.bottom == Bottom::PianoRoll, "Editors (E)", |p, r, c| scissors_icon(p, r, c));
    if ed.clicked() {
        app.bottom = if app.bottom == Bottom::PianoRoll { Bottom::None } else { Bottom::PianoRoll };
    }
    // ---- transport: Stop, Play, Record, Cycle
    let lcd_w = 300.0;
    let tw = 4.0 * (BTN + 6.0) + GAP * 2.0;
    let center = bar.center().x;
    let lcd = Rect::from_center_size(pos2(center + tw / 2.0, cy), vec2(lcd_w, 50.0));
    let gt = Rect::from_min_size(pos2(lcd.min.x - tw - 12.0, cy - h / 2.0), vec2(tw, h));
    group(&p, gt);
    bx = gt.min.x + GAP;
    let playing = app.playing();
    let stop = button(ui, Rect::from_min_size(pos2(bx, gt.min.y + 3.0), vec2(BTN + 6.0, h - 6.0)), "stop", false, "Stop (Space)", |p, r, c| {
        p.rect_filled(Rect::from_center_size(r.center(), vec2(12.0, 12.0)), 1.5, c);
    });
    if stop.clicked() {
        app.stop();
    }
    bx += BTN + 6.0;
    let play = button(ui, Rect::from_min_size(pos2(bx, gt.min.y + 3.0), vec2(BTN + 6.0, h - 6.0)), "play", playing, "Play (Space)", |p, r, c| {
        let m = r.center();
        let col = if playing { Color32::from_rgb(0x4c, 0xd9, 0x64) } else { c };
        p.add(egui::Shape::convex_polygon(vec![m + vec2(-5.0, -7.0), m + vec2(8.0, 0.0), m + vec2(-5.0, 7.0)], col, Stroke::NONE));
    });
    if play.clicked() {
        app.play_stop();
    }
    bx += BTN + 6.0;
    let rec = button(ui, Rect::from_min_size(pos2(bx, gt.min.y + 3.0), vec2(BTN + 6.0, h - 6.0)), "record", false, "Record (R) — not yet", |p, r, _| {
        p.circle_filled(r.center(), 7.5, look::record_red());
    });
    if rec.clicked() {
        app.tell("Recording isn't available yet");
    }
    bx += BTN + 6.0;
    let cycle_on = app.cycle_on();
    let cyc = button(ui, Rect::from_min_size(pos2(bx, gt.min.y + 3.0), vec2(BTN + 6.0, h - 6.0)), "cycle", cycle_on, "Cycle (C)", |p, r, c| {
        let col = if cycle_on { look::cycle() } else { c };
        let m = r.center();
        let st = Stroke::new(1.8, col);
        p.line_segment([m + vec2(-7.0, -3.0), m + vec2(6.0, -3.0)], st);
        p.line_segment([m + vec2(6.0, 3.0), m + vec2(-7.0, 3.0)], st);
        p.line_segment([m + vec2(3.0, -6.0), m + vec2(6.0, -3.0)], st);
        p.line_segment([m + vec2(-4.0, 6.0), m + vec2(-7.0, 3.0)], st);
    });
    if cyc.clicked() {
        app.toggle_cycle();
    }
    // ---- the LCD
    p.rect_filled(lcd, 10.0, look::lcd_bg());
    let pos = app.position();
    let bar_t = app.bar_ticks() as f64;
    let beat_t = lg_core::events::PPQ as f64 * 4.0 / app.project.as_ref().map_or(4.0, |pr| pr.signature.1 as f64);
    let bar_n = (pos / bar_t).floor() as i64 + 1;
    let beat_n = ((pos % bar_t) / beat_t).floor() as i64 + 1;
    let col_w = lcd_w / 4.0;
    let cell = |i: f32| Rect::from_min_size(pos2(lcd.min.x + i * col_w, lcd.min.y), vec2(col_w, lcd.height()));
    // bar: dimmed leading zeros, as Logic's "001"
    let bar_text = format!("{bar_n}");
    let lead = "0".repeat(3usize.saturating_sub(bar_text.len()));
    let c0 = cell(0.0);
    let big = look::regular(30.0);
    let g_lead = p.layout_no_wrap(lead, big.clone(), look::lcd_dim().gamma_multiply(0.45));
    let g_bar = p.layout_no_wrap(bar_text, big.clone(), look::lcd_text());
    let total = g_lead.size().x + g_bar.size().x;
    let x0 = c0.center().x - total / 2.0;
    let y0 = c0.min.y + 4.0;
    let lead_w = g_lead.size().x;
    p.galley(pos2(x0, y0), g_lead, Color32::WHITE);
    p.galley(pos2(x0 + lead_w, y0), g_bar, Color32::WHITE);
    p.text(pos2(c0.center().x, c0.max.y - 8.0), egui::Align2::CENTER_CENTER, "BAR", look::semibold(9.0), look::lcd_dim());
    let c1 = cell(1.0);
    p.text(pos2(c1.center().x, c1.min.y + 22.0), egui::Align2::CENTER_CENTER, format!("{beat_n}"), big, look::lcd_text());
    p.text(pos2(c1.center().x, c1.max.y - 8.0), egui::Align2::CENTER_CENTER, "BEAT", look::semibold(9.0), look::lcd_dim());
    let c2 = cell(2.0);
    let tempo = app.tempo();
    let tempo_text = if (tempo - tempo.round()).abs() < 0.005 { format!("{tempo:.0}") } else { format!("{tempo:.2}") };
    p.text(pos2(c2.center().x, c2.min.y + 15.0), egui::Align2::CENTER_CENTER, tempo_text, look::regular(17.0), look::lcd_text());
    p.text(pos2(c2.center().x, c2.min.y + 30.0), egui::Align2::CENTER_CENTER, "KEEP", look::semibold(8.0), look::lcd_dim());
    p.text(pos2(c2.center().x, c2.max.y - 8.0), egui::Align2::CENTER_CENTER, "TEMPO", look::semibold(9.0), look::lcd_dim());
    let c3 = cell(3.0);
    let (sig, key) = app.project.as_ref().map_or(((4, 4), "C major".to_string()), |pr| (pr.signature, pr.key.clone()));
    p.text(pos2(c3.center().x, c3.min.y + 15.0), egui::Align2::CENTER_CENTER, format!("{}/{}", sig.0, sig.1), look::regular(17.0), look::lcd_text());
    p.text(pos2(c3.center().x, c3.min.y + 35.0), egui::Align2::CENTER_CENTER, short_key(&key), look::regular(15.0), look::lcd_text());
    // ---- right: master level, list editors / browser buttons
    let rg = Rect::from_min_size(pos2(bar.max.x - 10.0 - (2.0 * BTN + 3.0 * GAP), cy - h / 2.0), vec2(2.0 * BTN + 3.0 * GAP, h));
    group(&p, rg);
    bx = rg.min.x + GAP;
    let list = button(ui, Rect::from_min_size(pos2(bx, rg.min.y + 3.0), vec2(BTN, h - 6.0)), "lists", false, "List Editors (D) — not yet", |p, r, c| icon(p, r, c, kn_ui::icons::Icon::List));
    if list.clicked() {
        app.tell("The List Editors aren't available yet");
    }
    bx += BTN + GAP;
    let open = button(ui, Rect::from_min_size(pos2(bx, rg.min.y + 3.0), vec2(BTN, h - 6.0)), "open", false, "Open a Logic project (⌘O)", |p, r, c| icon(p, r, c, kn_ui::icons::Icon::File));
    if open.clicked() {
        app.open_dialog();
    }
    // the master level meter, as Logic's black bar before the right buttons
    let meter = Rect::from_min_max(pos2(rg.min.x - 190.0, cy - 9.0), pos2(rg.min.x - 14.0, cy + 9.0));
    if meter.min.x > lcd.max.x + 20.0 {
        p.rect_filled(meter, 4.0, look::lcd_bg());
        let level = app.engine.lock().map(|e| e.master_meter).unwrap_or(0.0).clamp(0.0, 1.0);
        let w = (meter.width() - 6.0) * level.sqrt();
        if w > 0.5 {
            let fill = Rect::from_min_size(meter.min + vec2(3.0, 5.0), vec2(w, meter.height() - 10.0));
            p.rect_filled(fill, 2.0, if level > 0.95 { look::record_red() } else { Color32::from_rgb(0x4c, 0xd9, 0x64) });
        }
    }
}

fn short_key(k: &str) -> String {
    // "C major" -> "Cmaj", "G minor" -> "Gmin"
    let mut it = k.split_whitespace();
    let root = it.next().unwrap_or("C");
    let q = it.next().unwrap_or("major");
    format!("{root}{}", if q.starts_with("min") { "min" } else { "maj" })
}

fn mixer_icon(p: &egui::Painter, r: Rect, c: Color32) {
    let m = r.center();
    for (i, k) in [-0.3f32, 0.4, -0.1].iter().enumerate() {
        let x = m.x - 6.0 + i as f32 * 6.0;
        p.line_segment([pos2(x, m.y - 8.0), pos2(x, m.y + 8.0)], Stroke::new(1.4, c));
        p.rect_filled(Rect::from_center_size(pos2(x, m.y + k * 10.0), vec2(5.0, 3.0)), 1.0, c);
    }
}

fn scissors_icon(p: &egui::Painter, r: Rect, c: Color32) {
    // Logic's Editors button: a pencil over lines
    let m = r.center();
    let st = Stroke::new(1.6, c);
    p.line_segment([m + vec2(-6.0, 6.0), m + vec2(6.0, -6.0)], st);
    p.line_segment([m + vec2(-7.0, 7.0), m + vec2(-4.0, 7.0)], st);
}
