//! Varispeed, as Logic's: the whole project faster or slower, like a tape machine's varispeed,
//! from -50% (half speed) to +100% (double). Shown in the control bar when ticked in Customize
//! Control Bar and Display (Modes and Functions ▸ Varispeed): the button turns it on or off
//! (Toggle Varispeed); the display's upper part is the mode menu (Speed Only, Varispeed (Speed
//! and Pitch), Varispeed and MIDI), the lower part the value, dragged up or down or typed after
//! a double-click, its unit chosen by a click on it: % (of the original tempo), bpm (the
//! resulting tempo, following the project's tempo changes), st (detune in semitones and
//! cents) or Hz (as a tuning reference). The control bar's Tempo stays the original tempo.
//! Audio is played faster or slower with its pitch (tape); in Speed Only Studio still pitches
//! audio with the speed (no pitch-keeping yet), MIDI instruments keep their pitch.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &["Toggle Varispeed"];

pub const MODES: [&str; 3] = ["Speed Only", "Varispeed (Speed and Pitch)", "Varispeed and MIDI"];
pub const UNITS: [&str; 4] = ["%", "bpm", "st", "Hz"];

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub shown: bool,
    pub on: bool,
    /// percent: -50 … +100
    pub percent: f64,
    pub mode: usize,
    pub unit: usize,
    typing: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        // (LG_VARISPEED=1: shown, for checks)
        State { shown: std::env::var_os("LG_VARISPEED").is_some(), on: false, percent: 0.0, mode: 1, unit: 0, typing: None }
    }
}

fn id() -> egui::Id {
    egui::Id::unique("lg-varispeed")
}

pub fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(id())).unwrap_or_default()
}

fn put(ctx: &egui::Context, s: State) {
    ctx.data_mut(|d| d.insert_temp(id(), s));
}

/// The speed factor (1.0 off).
pub fn factor(s: &State) -> f64 {
    if s.on { 1.0 + s.percent.clamp(-50.0, 100.0) / 100.0 } else { 1.0 }
}

/// The value as the unit shows it.
pub fn shown_value(percent: f64, unit: usize, bpm: f64) -> String {
    let k = 1.0 + percent / 100.0;
    match unit {
        1 => format!("{:.2}", bpm * k),
        2 => {
            let st = 12.0 * k.log2();
            let semis = st.trunc();
            let cents = ((st - semis) * 100.0).round();
            format!("{:+.0} {:+.0}", semis, cents)
        }
        3 => format!("{:.1}", 440.0 * k),
        _ => format!("{:+.2}", percent),
    }
}

/// A typed value in the unit back to percent.
pub fn parse_value(text: &str, unit: usize, bpm: f64) -> Option<f64> {
    let nums: Vec<f64> = text.split_whitespace().filter_map(|x| x.trim_end_matches(['%', 'c']).parse().ok()).collect();
    let v = *nums.first()?;
    let pct = match unit {
        1 => (v / bpm.max(1.0) - 1.0) * 100.0,
        2 => (2f64.powf((v + nums.get(1).copied().unwrap_or(0.0) / 100.0) / 12.0) - 1.0) * 100.0,
        3 => (v / 440.0 - 1.0) * 100.0,
        _ => v,
    };
    Some(pct.clamp(-50.0, 100.0))
}

pub fn command(app: &mut App, name: &str) -> bool {
    if name != "Toggle Varispeed" {
        return false;
    }
    let mut s = state(&app.ctx);
    s.on = !s.on;
    put(&app.ctx, s);
    true
}

/// Every frame: the engine's speed.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let s = state(ctx);
    let f = factor(&s);
    if let Ok(mut e) = app.engine.lock() {
        if e.varispeed != f {
            e.varispeed = f;
        }
    }
}

/// The Customize Control Bar checkbox.
pub fn customize(ui: &mut egui::Ui) {
    let mut s = state(ui.ctx());
    if ui.checkbox(&mut s.shown, "Varispeed").changed() {
        put(ui.ctx(), s);
    }
}

pub fn shown(ctx: &egui::Context) -> bool {
    state(ctx).shown
}

/// The control bar's Varispeed button and display from `x` (centre line `cy`); returns the
/// capsule's rect.
pub fn control_bar(app: &mut App, ui: &mut egui::Ui, x: f32, cy: f32, h: f32) -> Rect {
    let mut s = state(ui.ctx());
    let p = ui.painter().clone();
    let cap = Rect::from_min_size(pos2(x, cy - h / 2.0), vec2(34.0 + 92.0 + 6.0, h));
    let btn = Rect::from_min_size(pos2(x + 3.0, cy - h / 2.0 + 2.0), vec2(34.0, h - 4.0));
    let resp = ui.interact(btn, ui.scope_id().with("cb-varispeed"), Sense::click()).on_hover_text("Varispeed");
    let cell = btn.shrink2(vec2(1.0, 2.0));
    if s.on {
        p.rect_filled(cell, cell.height() / 2.0, look::accent());
    } else if resp.hovered() {
        p.rect_filled(cell, cell.height() / 2.0, look::button_on().gamma_multiply(0.55));
    }
    crate::glyphs::paint(&p, btn, crate::glyphs::ph::GAUGE, 17.0, if s.on { Color32::WHITE } else { look::text() });
    if resp.clicked() {
        s.on = !s.on;
    }
    // the display: mode (upper), value and unit (lower)
    let d = Rect::from_min_max(pos2(btn.max.x + 3.0, cy - h / 2.0 + 3.0), pos2(cap.max.x - 3.0, cy + h / 2.0 - 3.0));
    p.rect_filled(d, 5.0, look::lcd_bg());
    let upper = Rect::from_min_max(d.min, pos2(d.max.x, d.center().y));
    let lower = Rect::from_min_max(pos2(d.min.x, d.center().y), d.max);
    let short = ["SPEED ONLY", "VARISPEED", "VARI + MIDI"][s.mode];
    p.text(upper.center(), Align2::CENTER_CENTER, short, look::semibold(9.0), look::lcd_dim());
    let mresp = ui.interact(upper, ui.scope_id().with("cb-vs-mode"), Sense::click()).on_hover_text("Varispeed mode");
    egui::Popup::menu(&mresp).show(|ui| {
        for (k, m) in MODES.iter().enumerate() {
            if ui.button(if s.mode == k { format!("✓ {m}") } else { format!("   {m}") }).clicked() {
                s.mode = k;
                ui.close();
            }
        }
    });
    let bpm = app.project.as_ref().map_or(120.0, |pr| lg_core::timeline::Map::of(pr).bpm_at(app.position()));
    let unit_r = Rect::from_min_max(pos2(lower.max.x - 26.0, lower.min.y), lower.max);
    let val_r = Rect::from_min_max(lower.min, pos2(unit_r.min.x, lower.max.y));
    if let Some(mut t) = s.typing.clone() {
        let te = ui.put(val_r, egui::TextEdit::singleline(&mut t).font(look::regular(11.0)));
        te.request_focus();
        if te.lost_focus() {
            if let Some(v) = parse_value(&t, s.unit, bpm) {
                s.percent = v;
            }
            s.typing = None;
        } else {
            s.typing = Some(t);
        }
    } else {
        let col = if s.on { look::lcd_text() } else { look::lcd_dim() };
        p.text(pos2(val_r.max.x - 2.0, val_r.center().y), Align2::RIGHT_CENTER, shown_value(s.percent, s.unit, bpm), look::regular(11.0), col);
        let vresp = ui.interact(val_r, ui.scope_id().with("cb-vs-val"), Sense::click_and_drag()).on_hover_text("Drag up or down; double-click to type");
        if vresp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            let fine = ui.input(|i| i.modifiers.shift);
            s.percent = (s.percent - vresp.drag_delta().y as f64 * if fine { 0.01 } else { 0.25 }).clamp(-50.0, 100.0);
        }
        if vresp.double_clicked() {
            s.typing = Some(shown_value(s.percent, s.unit, bpm));
        }
    }
    p.text(unit_r.center(), Align2::CENTER_CENTER, UNITS[s.unit], look::semibold(10.0), look::lcd_dim());
    let uresp = ui.interact(unit_r, ui.scope_id().with("cb-vs-unit"), Sense::click()).on_hover_text("Unit");
    egui::Popup::menu(&uresp).show(|ui| {
        for (k, (u, name)) in UNITS.iter().zip(["Percentage (%)", "Resulting Tempo (bpm)", "Detune (st)", "Tuning Reference (Hz)"]).enumerate() {
            let _ = u;
            if ui.button(if s.unit == k { format!("✓ {name}") } else { format!("   {name}") }).clicked() {
                s.unit = k;
                ui.close();
            }
        }
    });
    put(ui.ctx(), s);
    cap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_round_trip() {
        assert_eq!(factor(&State { on: true, percent: -50.0, ..Default::default() }), 0.5);
        assert_eq!(factor(&State { on: false, percent: 30.0, ..Default::default() }), 1.0);
        assert_eq!(shown_value(100.0, 1, 120.0), "240.00");
        assert_eq!(shown_value(100.0, 2, 120.0), "+12 +0");
        assert_eq!(shown_value(0.0, 3, 120.0), "440.0");
        assert!((parse_value("150", 1, 120.0).unwrap() - 25.0).abs() < 1e-9);
        assert!((parse_value("12", 2, 120.0).unwrap() - 100.0).abs() < 1e-9);
        assert!((parse_value("220", 3, 120.0).unwrap() + 50.0).abs() < 1e-9);
        assert_eq!(parse_value("300", 0, 120.0), Some(100.0));
    }
}
