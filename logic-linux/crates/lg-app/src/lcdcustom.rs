//! The LCD's Beats and Custom modes (Logic's LCD ⌄ menu: Beats & Project, Beats & Time, Beats,
//! Time, Custom). Beats shows the musical position alone. Custom shows two-row columns, each
//! ticked in Customize Control Bar and Display: Positions (time over bars), Locators (left over
//! right), Tempo, Signature/Division (division below the time signature) and Key
//! Signature/Project End. The division and the project end are edited there: drag up or down,
//! or double-click to type.

use egui::{pos2, vec2, Align2, Rect, Sense};

use crate::app::App;
use crate::look;

/// Custom's columns (Customize Control Bar and Display ▸ Display, with Custom chosen).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cols {
    pub positions: bool,
    pub locators: bool,
    pub tempo: bool,
    pub sig_division: bool,
    pub key_end: bool,
}

impl Default for Cols {
    fn default() -> Self {
        Cols { positions: true, locators: true, tempo: true, sig_division: true, key_end: true }
    }
}

/// The edits in progress (a double-clicked field's text).
#[derive(Default)]
pub struct State {
    pub cols: Cols,
    pub end_edit: Option<String>,
    pub div_edit: Option<String>,
    drag_from: Option<(f32, u32)>,
}

const POS_W: f32 = 112.0;
const LOC_W: f32 = 84.0;
const TEMPO_W: f32 = 62.0;
const SIG_W: f32 = 46.0;
const KEY_W: f32 = 78.0;
const MENU_W: f32 = 22.0;
const PAD: f32 = 10.0;

/// Custom's width for the ticked columns (the other modes: Logic's 310 pt).
pub fn width(app: &App) -> f32 {
    if app.lcd_mode != crate::controlbar::LcdMode::Custom {
        return 310.0;
    }
    let c = app.lcdc.cols;
    let w: f32 = [(c.positions, POS_W), (c.locators, LOC_W), (c.tempo, TEMPO_W), (c.sig_division, SIG_W), (c.key_end, KEY_W)].iter().filter(|x| x.0).map(|x| x.1).sum();
    (w + 2.0 * PAD + MENU_W).max(120.0)
}

/// A position as Logic's LCD writes it: bar, beat, division, tick ("129 1 1 1").
pub fn bars_text(tick: f64, sig: (u8, u8), div: u32) -> String {
    let (bar, beat, d, t) = split(tick, sig, div);
    format!("{bar} {beat} {d} {t}")
}

/// Bar, beat, division and tick (1-based; bars before bar 1 count 0, -1 …).
pub fn split(tick: f64, sig: (u8, u8), div: u32) -> (i64, i64, i64, i64) {
    let beat_t = (lg_core::events::PPQ * 4 / sig.1.max(1) as u32) as i64;
    let bar_t = beat_t * sig.0.max(1) as i64;
    let div = div.max(1) as i64;
    let t = tick.floor() as i64;
    let bar = t.div_euclid(bar_t);
    let r = t.rem_euclid(bar_t);
    (bar + 1, r / beat_t + 1, (r % beat_t) / div + 1, (r % beat_t) % div + 1)
}

/// The division as the LCD shows it: "/16", "/8", "/16T".
pub fn division_text(div: u32) -> String {
    let whole = lg_core::events::PPQ * 4;
    if div > 0 && whole % div == 0 {
        format!("/{}", whole / div)
    } else if div > 0 && (whole * 2) % (div * 3) == 0 {
        format!("/{}T", whole * 2 / (div * 3))
    } else {
        format!("{div} t")
    }
}

/// A typed division ("16", "/16", "8T") in ticks.
pub fn parse_division(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('/');
    let (n, trip) = match s.strip_suffix(['T', 't']) {
        Some(n) => (n, true),
        None => (s, false),
    };
    let n: u32 = n.trim().parse().ok().filter(|&n: &u32| n > 0 && n <= 192)?;
    Some(crate::nav::note_ticks(n, trip))
}

/// The Beats mode: bar, beat, division and tick, large, with their labels.
pub fn beats(app: &mut App, p: &egui::Painter, lcd: Rect) {
    let sig = app.project.as_ref().map_or((4, 4), |pr| pr.signature);
    let (bar, beat, d, t) = split(app.position(), sig, app.nav.division);
    let w = lcd.width() / 4.0;
    for (i, (v, label)) in [(bar, "BAR"), (beat, "BEAT"), (d, "DIV"), (t, "TICK")].into_iter().enumerate() {
        let c = Rect::from_min_size(pos2(lcd.min.x + i as f32 * w, lcd.min.y), vec2(w, lcd.height()));
        p.text(pos2(c.center().x, c.min.y + 22.0), Align2::CENTER_CENTER, v.to_string(), look::regular(if i < 2 { 30.0 } else { 22.0 }), look::lcd_text());
        p.text(pos2(c.center().x, c.max.y - 8.0), Align2::CENTER_CENTER, label, look::semibold(9.0), look::lcd_dim());
    }
}

/// The Custom mode's columns.
pub fn custom(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, lcd: Rect) {
    let sig = app.project.as_ref().map_or((4, 4), |pr| pr.signature);
    let div = app.nav.division;
    let c = app.lcdc.cols;
    let mut x = lcd.min.x + PAD;
    let top = lcd.min.y + 16.0;
    let bottom = lcd.min.y + 35.0;
    let mut col = |w: f32| {
        let r = Rect::from_min_size(pos2(x, lcd.min.y), vec2(w, lcd.height()));
        x += w;
        r
    };
    let pos = app.position();
    if c.positions {
        let r = col(POS_W);
        let time = crate::prefs::time_text(&app.logic_settings, &app.proj, app.seconds_at(pos));
        p.text(pos2(r.center().x, top), Align2::CENTER_CENTER, time, look::regular(15.0), look::lcd_text());
        p.text(pos2(r.center().x, bottom), Align2::CENTER_CENTER, bars_text(pos, sig, div), look::regular(15.0), look::lcd_text());
    }
    if c.locators {
        let r = col(LOC_W);
        let (a, b) = app.cycle;
        p.text(pos2(r.center().x, top), Align2::CENTER_CENTER, bars_text(a as f64, sig, div), look::regular(12.0), look::lcd_dim());
        p.text(pos2(r.center().x, bottom), Align2::CENTER_CENTER, bars_text(b as f64, sig, div), look::regular(12.0), look::lcd_dim());
    }
    if c.tempo {
        let r = col(TEMPO_W);
        let t = app.tempo();
        p.text(pos2(r.center().x, top), Align2::CENTER_CENTER, format!("{t:.4}"), look::regular(13.0), look::lcd_text());
        crate::tempomode::lcd_label(app, ui, p, pos2(r.center().x, bottom));
    }
    if c.sig_division {
        let r = col(SIG_W);
        p.text(pos2(r.center().x, top), Align2::CENTER_CENTER, format!("{}/{}", sig.0, sig.1), look::regular(15.0), look::lcd_text());
        division_field(app, ui, p, Rect::from_center_size(pos2(r.center().x, bottom), vec2(r.width(), 18.0)));
    }
    if c.key_end {
        let r = col(KEY_W);
        let key = app.project.as_ref().map_or("C major".to_string(), |pr| pr.key.clone());
        p.text(pos2(r.center().x, top), Align2::CENTER_CENTER, crate::controlbar::short_key(&key), look::regular(15.0), look::lcd_text());
        end_field(app, ui, p, Rect::from_center_size(pos2(r.center().x, bottom), vec2(r.width(), 18.0)));
    }
}

fn division_field(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect) {
    if let Some(text) = app.lcdc.div_edit.as_mut() {
        let te = ui.put(r, egui::TextEdit::singleline(text).align(egui::Align2::CENTER_CENTER).font(look::regular(13.0)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if let (false, Some(d)) = (cancel, parse_division(text)) {
                app.nav.division = d;
            }
            app.lcdc.div_edit = None;
        }
        return;
    }
    p.text(r.center(), Align2::CENTER_CENTER, division_text(app.nav.division), look::regular(13.0), look::lcd_text());
    let resp = ui.interact(r, ui.scope_id().with("lcd-division"), Sense::click_and_drag()).on_hover_text("Division");
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if resp.drag_started() {
        app.lcdc.drag_from = Some((resp.interact_pointer_pos().map_or(0.0, |p| p.y), app.nav.division));
    }
    if let (true, Some((y0, _)), Some(pp)) = (resp.dragged(), app.lcdc.drag_from, resp.interact_pointer_pos()) {
        // (every 8 pt a step: up finer divisions, down coarser)
        let steps = ((y0 - pp.y) / 8.0) as i32;
        if steps != 0 {
            app.nav.division = crate::nav::division_step(app.nav.division, steps > 0);
            app.lcdc.drag_from = Some((pp.y, app.nav.division));
        }
    }
    if resp.drag_stopped() {
        app.lcdc.drag_from = None;
    }
    if resp.double_clicked() {
        app.lcdc.div_edit = Some(division_text(app.nav.division).trim_start_matches('/').to_string());
    }
}

/// The Project End value: drag up or down by bars, or double-click to type a position.
fn end_field(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect) {
    let Some(sig) = app.project.as_ref().map(|pr| pr.signature) else { return };
    let end = crate::projend::end(app);
    if let Some(text) = app.lcdc.end_edit.as_mut() {
        let te = ui.put(r, egui::TextEdit::singleline(text).align(egui::Align2::CENTER_CENTER).font(look::regular(13.0)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let typed = crate::nav::parse_position(text, sig, app.nav.division);
            app.lcdc.end_edit = None;
            if let (false, Some(t)) = (cancel, typed) {
                set_end(app, t);
            }
        }
        return;
    }
    p.text(r.center(), Align2::CENTER_CENTER, bars_text(end as f64, sig, app.nav.division), look::regular(13.0), look::lcd_text());
    let resp = ui.interact(r, ui.scope_id().with("lcd-project-end"), Sense::click_and_drag()).on_hover_text("Project End");
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if resp.drag_started() {
        app.checkpoint("Project End");
        app.lcdc.drag_from = Some((resp.interact_pointer_pos().map_or(0.0, |p| p.y), end));
    }
    if let (true, Some((y0, from)), Some(pp)) = (resp.dragged(), app.lcdc.drag_from, resp.interact_pointer_pos()) {
        let bar = lg_core::project::bar_ticks(sig) as i64;
        let bars = ((y0 - pp.y) / 4.0).round() as i64;
        let t = (from as i64 + bars * bar).max(bar) as u32;
        if let Some(pr) = app.project.as_mut() {
            pr.end = Some(t);
        }
    }
    if resp.drag_stopped() {
        app.lcdc.drag_from = None;
    }
    if resp.double_clicked() {
        app.lcdc.end_edit = Some(bars_text(end as f64, sig, app.nav.division));
    }
}

/// A typed end: on a bar line or not (Logic keeps what's typed), never before bar 2.
fn set_end(app: &mut App, t: u32) {
    let bar = app.bar_ticks();
    app.checkpoint("Project End");
    if let Some(pr) = app.project.as_mut() {
        pr.end = Some(t.max(bar));
    }
}

/// Customize Control Bar and Display: Custom's column checkboxes.
pub fn customize(app: &mut App, ui: &mut egui::Ui) {
    let on = app.lcd_mode == crate::controlbar::LcdMode::Custom;
    let c = &mut app.lcdc.cols;
    ui.add_enabled_ui(on, |ui| {
        ui.checkbox(&mut c.positions, "Positions (SMPTE/Bars)");
        ui.checkbox(&mut c.locators, "Locators/Cycle");
        ui.checkbox(&mut c.tempo, "Tempo");
        ui.checkbox(&mut c.sig_division, "Time Signature/Division");
        ui.checkbox(&mut c.key_end, "Key Signature/Project End");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_as_logic_writes_them() {
        assert_eq!(bars_text(0.0, (4, 4), 240), "1 1 1 1");
        assert_eq!(bars_text(128.0 * 3840.0, (4, 4), 240), "129 1 1 1");
        assert_eq!(bars_text(3840.0 + 960.0 + 240.0 + 5.0, (4, 4), 240), "2 2 2 6");
        // before bar 1: bar 0
        assert_eq!(split(-3840.0, (4, 4), 240).0, 0);
    }

    #[test]
    fn divisions() {
        assert_eq!(division_text(240), "/16");
        assert_eq!(division_text(480), "/8");
        assert_eq!(division_text(160), "/24");
        assert_eq!(parse_division("/16"), Some(240));
        assert_eq!(parse_division("8"), Some(480));
        assert_eq!(parse_division("0"), None);
    }

    #[test]
    fn custom_is_wider_with_more_columns() {
        let all: f32 = POS_W + LOC_W + TEMPO_W + SIG_W + KEY_W + 2.0 * PAD + MENU_W;
        assert!(all > 310.0);
    }
}
