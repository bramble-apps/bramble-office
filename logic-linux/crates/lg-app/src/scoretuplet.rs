//! N-tuplets (Logic: the Part box's N-tuplet symbol, the Tuplet window): any number of notes
//! of a value in the time of another number (5:4 sixteenths, 7:8 thirty-seconds, 8:8/32 with
//! hidden bracket and number to override the display Quantize), with Hide bracket, Hide
//! numbers (in parentheses on screen, left out of print), Show note value, Direction, Allow
//! double / half values. Kept as a score symbol on the track (lg-core TupletText); a new one
//! shows as many rests as its count until notes fill it.

use crate::app::App;
use crate::score;
use egui::{pos2, vec2, Align2, Color32, Stroke};
use lg_core::score::{Symbol, SymbolKind, TupletText};
use lg_notation as nt;

/// The Tuplet window's values: note value pop-up.
pub const BASES: [(u8, &str); 6] = [(2, "1/2"), (4, "1/4"), (8, "1/8"), (16, "1/16"), (32, "1/32"), (64, "1/64")];

#[derive(Clone)]
struct Win {
    track: usize,
    tick: u32,
    t: TupletText,
    /// Editing a tuplet already there.
    existing: bool,
}

fn win_id() -> egui::Id {
    egui::Id::unique("lg-tuplet-window")
}
fn last_id() -> egui::Id {
    egui::Id::unique("lg-tuplet-last")
}

/// The tuplets of a track as lg-notation reads them.
pub fn specs(app: &App, id: u64) -> Vec<nt::TupletSpec> {
    let Some(ts) = app.project.as_ref().and_then(|p| p.score.track(id)) else { return Vec::new() };
    ts.symbols.iter().filter(|s| s.kind == SymbolKind::Tuplet).map(|s| spec_of(s.tick, TupletText::parse(&s.text))).collect()
}

pub fn spec_of(tick: u32, t: TupletText) -> nt::TupletSpec {
    nt::TupletSpec {
        start: tick,
        n: t.n,
        of: t.of,
        base: t.base,
        hide_bracket: t.flags & 1 != 0,
        hide_number: t.flags & 2 != 0,
        show_value: t.flags & 4 != 0,
        dir: if t.flags & 8 != 0 { 1 } else if t.flags & 16 != 0 { -1 } else { 0 },
        double: t.flags & 32 != 0,
        half: t.flags & 64 != 0,
    }
}

/// The N-tuplet dropped on a note (or a place): the Tuplet window, or with Option the last
/// settings straight away.
pub fn insert(app: &mut App, ctx: &egui::Context, track: usize, tick: u32, option: bool) {
    let last: TupletText = ctx.data(|m| m.get_temp(last_id())).unwrap_or_default();
    if option {
        set(app, track, tick, Some(last), "Insert Tuplet");
        return;
    }
    ctx.data_mut(|m| m.insert_temp(win_id(), Some(Win { track, tick, t: last, existing: false })));
}

/// A double-click on a tuplet's number: its Tuplet window.
pub fn edit_at(ctx: &egui::Context, track: usize, tick: u32) {
    ctx.data_mut(|m| m.insert_temp::<Option<Win>>(win_id(), Some(Win { track, tick, t: TupletText::default(), existing: true })));
}

/// The Eraser on a tuplet's number, or the window's Delete: the tuplet goes (its notes stay).
pub fn delete_at(app: &mut App, track: usize, tick: u32) {
    set(app, track, tick, None, "Delete Tuplet");
}

/// Adds, replaces (`Some`) or removes (`None`) the tuplet at a place (one undo step).
pub fn set(app: &mut App, track: usize, tick: u32, t: Option<TupletText>, what: &str) {
    let Some(id) = app.project.as_ref().and_then(|p| p.tracks.get(track)).map(|t| t.extra.id) else { return };
    app.checkpoint(what);
    if let Some(p) = app.project.as_mut() {
        let ts = p.score.track_mut(id);
        ts.symbols.retain(|s| !(s.kind == SymbolKind::Tuplet && s.tick == tick));
        if let Some(t) = t {
            ts.symbols.push(Symbol { tick, kind: SymbolKind::Tuplet, text: t.text(), len: t.len(nt::PPQ), ..Default::default() });
            ts.symbols.sort_by_key(|s| s.tick);
        }
    }
    app.after_edit();
}

fn flag(t: &mut TupletText, bit: u8, on: bool) {
    if on {
        t.flags |= bit;
    } else {
        t.flags &= !bit;
    }
}

/// The Tuplet window.
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = ctx.data(|m| m.get_temp::<Option<Win>>(win_id())).flatten() else { return };
    if w.existing && w.t == TupletText::default() {
        // read the tuplet being edited
        let id = app.project.as_ref().and_then(|p| p.tracks.get(w.track)).map(|t| t.extra.id);
        if let Some(s) = id.and_then(|id| app.project.as_ref()?.score.track(id)?.symbols.iter().find(|s| s.kind == SymbolKind::Tuplet && s.tick == w.tick).cloned()) {
            w.t = TupletText::parse(&s.text);
        }
    }
    let mut open = true;
    let mut close = false;
    egui::Window::new("Tuplet").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        // the graphic: reflects the settings
        let (r, p) = ui.allocate_painter(vec2(220.0, 64.0), egui::Sense::hover());
        let rect = r.rect;
        p.rect_filled(rect, 4.0, Color32::from_rgb(0xfe, 0xff, 0xf3));
        let ink = Color32::from_rgb(0x10, 0x10, 0x10);
        let n = w.t.n.clamp(1, 12) as usize;
        let up = w.t.flags & 16 == 0;
        let x0 = rect.min.x + 20.0;
        let dx = (rect.width() - 40.0) / n as f32;
        let yh = rect.center().y + if up { 10.0 } else { -10.0 };
        for k in 0..n {
            let x = x0 + k as f32 * dx;
            p.circle_filled(pos2(x, yh), 3.6, ink);
            let ys = if up { yh - 20.0 } else { yh + 20.0 };
            p.line_segment([pos2(x + if up { 3.4 } else { -3.4 }, yh), pos2(x + if up { 3.4 } else { -3.4 }, ys)], Stroke::new(1.0, ink));
        }
        let yb = if up { yh - 28.0 } else { yh + 28.0 };
        if w.t.flags & 1 == 0 {
            p.line_segment([pos2(x0, yb), pos2(x0 + (n - 1) as f32 * dx + 4.0, yb)], Stroke::new(1.0, ink));
        }
        let label = if w.t.flags & 2 != 0 { format!("({})", w.t.n) } else if w.t.flags & 4 != 0 { format!("{}:{}", w.t.n, w.t.of) } else { w.t.n.to_string() };
        p.text(pos2(rect.center().x, yb), Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), ink);
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut w.t.n).range(1..=64)).on_hover_text("Number of tuplet notes");
            ui.label(":");
            ui.add(egui::DragValue::new(&mut w.t.of).range(1..=64)).on_hover_text("Time value the tuplet occupies");
            let cur = BASES.iter().position(|b| b.0 == w.t.base).unwrap_or(3);
            egui::ComboBox::from_id_salt("tuplet-base").width(60.0).selected_text(BASES[cur].1).show_ui(ui, |ui| {
                for b in BASES {
                    ui.selectable_value(&mut w.t.base, b.0, b.1);
                }
            });
        });
        let mut hb = w.t.flags & 1 != 0;
        let mut hn = w.t.flags & 2 != 0;
        let mut sv = w.t.flags & 4 != 0;
        let mut dbl = w.t.flags & 32 != 0;
        let mut half = w.t.flags & 64 != 0;
        ui.checkbox(&mut hb, "Hide bracket");
        ui.checkbox(&mut hn, "Hide numbers");
        ui.checkbox(&mut sv, "Show note value");
        ui.checkbox(&mut dbl, "Allow double values");
        ui.checkbox(&mut half, "Allow half values");
        flag(&mut w.t, 1, hb);
        flag(&mut w.t, 2, hn);
        flag(&mut w.t, 4, sv);
        flag(&mut w.t, 32, dbl);
        flag(&mut w.t, 64, half);
        ui.horizontal(|ui| {
            ui.label("Direction:");
            let mut d = if w.t.flags & 8 != 0 { 1 } else if w.t.flags & 16 != 0 { 2 } else { 0 };
            egui::ComboBox::from_id_salt("tuplet-dir").width(70.0).selected_text(["Auto", "Up", "Down"][d]).show_ui(ui, |ui| {
                for (k, n) in ["Auto", "Up", "Down"].iter().enumerate() {
                    ui.selectable_value(&mut d, k, *n);
                }
            });
            flag(&mut w.t, 8, d == 1);
            flag(&mut w.t, 16, d == 2);
        });
        ui.separator();
        ui.horizontal(|ui| {
            if w.existing && ui.button("Delete").clicked() {
                delete_at(app, w.track, w.tick);
                close = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if ui.button("OK").clicked() {
                set(app, w.track, w.tick, Some(w.t), if w.existing { "Edit Tuplet" } else { "Insert Tuplet" });
                ctx.data_mut(|m| m.insert_temp(last_id(), w.t));
                close = true;
            }
        });
    });
    let keep = open && !close;
    ctx.data_mut(|m| m.insert_temp(win_id(), keep.then_some(w)));
    let _ = score::TOOLS;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuplet_text_flags_make_specs() {
        let t = TupletText { n: 8, of: 8, base: 32, flags: 1 | 2 | 16 | 32 };
        let s = spec_of(480, TupletText::parse(&t.text()));
        assert_eq!((s.n, s.of, s.base, s.len()), (8, 8, 32, 960));
        assert!(s.hide_bracket && s.hide_number && !s.show_value && s.double && !s.half);
        assert_eq!(s.dir, -1);
    }
}
