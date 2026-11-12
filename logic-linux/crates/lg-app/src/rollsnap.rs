//! The Piano Roll's own Snap (Logic's: separate from the Tracks area's): the Snap pop-up in its
//! local bar with Smart (bar, beat or division by zoom), Bar, Beat, Division, Ticks, Frames,
//! Quarter Frames, Samples and the note values 1/1 … 1/64 and their triplets; Snap Notes to
//! Absolute Value (the default: notes land on grid positions) or Relative Value (they keep
//! their distance from the grid, moving by whole grid steps). It applies to moving, resizing,
//! adding and cutting notes. While editing, Control snaps to the division and Control-Shift to
//! ticks; Shift (Studio's) doesn't snap.

use lg_core::regions::{self, SnapMode};

use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Mode(SnapMode),
    /// a note value in ticks
    Note(u32),
}

pub const NOTES: [(&str, u32); 13] = [
    ("1/1 Note", 3840),
    ("1/2 Note", 1920),
    ("1/4 Note", 960),
    ("1/8 Note", 480),
    ("1/16 Note", 240),
    ("1/32 Note", 120),
    ("1/64 Note", 60),
    ("1/2 Triplet", 1280),
    ("1/4 Triplet", 640),
    ("1/8 Triplet", 320),
    ("1/16 Triplet", 160),
    ("1/32 Triplet", 80),
    ("1/64 Triplet", 40),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub value: Value,
    pub relative: bool,
}

impl Default for State {
    fn default() -> Self {
        State { value: Value::Mode(SnapMode::Smart), relative: false }
    }
}

fn id() -> egui::Id {
    egui::Id::unique("lg-roll-snap")
}

pub fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(id())).unwrap_or_default()
}

fn put(ctx: &egui::Context, s: State) {
    ctx.data_mut(|d| d.insert_temp(id(), s));
}

fn name(v: Value) -> String {
    match v {
        Value::Mode(m) => m.name().to_string(),
        Value::Note(t) => NOTES.iter().find(|n| n.1 == t).map_or(format!("{t} ticks"), |n| n.0.to_string()),
    }
}

/// The grid in ticks for this edit (None: no snapping).
pub fn grid(app: &App, ctx: &egui::Context, px_per_tick: f32) -> Option<f32> {
    let m = ctx.input(|i| i.modifiers);
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let division = lg_core::events::PPQ / 4;
    if m.ctrl && m.shift {
        return Some(1.0);
    }
    if m.ctrl {
        return Some(division as f32);
    }
    if m.shift {
        return None;
    }
    Some(match state(ctx).value {
        Value::Note(t) => t as f32,
        Value::Mode(mode) => regions::snap_grid(mode, sig, division, app.tempo(), px_per_tick as f64, 8.0) as f32,
    })
}

/// A move of notes starting at `orig` by `dt` ticks, snapped: Absolute puts the moved note on
/// the grid, Relative moves by whole grid steps.
pub fn snap_move(orig: f32, dt: f32, g: f32, relative: bool) -> f32 {
    if g <= 0.0 {
        return dt;
    }
    if relative {
        (dt / g).round() * g
    } else {
        ((orig + dt) / g).round() * g - orig
    }
}

/// The snapped move for the Piano Roll's current settings.
pub fn delta(app: &App, ctx: &egui::Context, orig: f32, dt: f32, px_per_tick: f32) -> f32 {
    match grid(app, ctx, px_per_tick) {
        Some(g) => snap_move(orig, dt, g, state(ctx).relative),
        None => dt,
    }
}

/// A length change snapped to the grid.
pub fn length_delta(app: &App, ctx: &egui::Context, dt: f32, px_per_tick: f32) -> f32 {
    match grid(app, ctx, px_per_tick) {
        Some(g) => (dt / g).round() * g,
        None => dt,
    }
}

/// A position for a new note (the grid line at or before it).
pub fn floor(app: &App, ctx: &egui::Context, tick: f32, px_per_tick: f32) -> f32 {
    match grid(app, ctx, px_per_tick) {
        Some(g) => ((tick / g).floor() * g).max(0.0),
        None => tick.max(0.0),
    }
}

pub fn round(app: &App, ctx: &egui::Context, tick: f32, px_per_tick: f32) -> f32 {
    match grid(app, ctx, px_per_tick) {
        Some(g) => ((tick / g).round() * g).max(0.0),
        None => tick.max(0.0),
    }
}

/// The Snap pop-up for the Piano Roll's local bar.
pub fn popup(ui: &mut egui::Ui) {
    let mut s = state(ui.ctx());
    ui.menu_button(egui::RichText::new(format!("Snap: {} ⌄", name(s.value))).size(11.5), |ui| {
        ui.set_min_width(220.0);
        let item = |ui: &mut egui::Ui, v: Value, s: &mut State| {
            if ui.button(if s.value == v { format!("✓ {}", name(v)) } else { format!("   {}", name(v)) }).clicked() {
                s.value = v;
                ui.close();
            }
        };
        for m in SnapMode::ALL {
            item(ui, Value::Mode(m), &mut s);
        }
        ui.separator();
        for (_, t) in NOTES {
            item(ui, Value::Note(t), &mut s);
        }
        ui.separator();
        if ui.button(if !s.relative { "✓ Snap Notes to Absolute Value" } else { "   Snap Notes to Absolute Value" }).clicked() {
            s.relative = false;
            ui.close();
        }
        if ui.button(if s.relative { "✓ Snap Notes to Relative Value" } else { "   Snap Notes to Relative Value" }).clicked() {
            s.relative = true;
            ui.close();
        }
    });
    put(ui.ctx(), s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_and_relative() {
        // a note at 1.2.1.16 (960 + 15) moved about a bar: absolute → bar 2's grid (3840 + 960 by
        // the beat), relative keeps the 15-tick offset
        let orig = 960.0 + 15.0;
        let g = 960.0;
        assert_eq!(orig + snap_move(orig, 3800.0, g, false), 3840.0 + 960.0);
        assert_eq!(orig + snap_move(orig, 3800.0, g, true), 3840.0 + 960.0 + 15.0);
        assert_eq!(snap_move(10.0, 7.0, 0.0, false), 7.0);
    }
}
