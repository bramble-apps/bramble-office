//! The Browser Focus triangle, as Logic's: in the inspector's channel strip, hovering left of a
//! plug-in slot or the Setting button shows a dim triangle; a click lights it and opens the
//! Browser — a plug-in slot in Plug-in Presets for that plug-in, the Setting button in
//! Instruments or Effects (by the track's type) — and turns Link to Selection on. Moving it to
//! another slot changes the preset list; switching the Browser between Plug-in Presets and
//! Instruments / Effects moves it between the first plug-in slot and the Setting button.

use egui::{pos2, vec2, Color32, Rect, Sense, Shape, Stroke};

use crate::app::App;
use crate::browser::Cat;

/// Where the triangle is: the Setting button, the instrument slot or an Audio FX slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Setting,
    Instrument,
    Fx(usize),
}

#[derive(Default)]
pub struct State {
    /// The lit triangle: (track, where).
    pub focus: Option<(usize, Target)>,
    /// Drawing the inspector's strips (the triangle is only there).
    pub in_inspector: bool,
    last_view: Option<Cat>,
}

/// The view a target shows: Plug-in Presets for a plug-in, Instruments or Effects for Setting.
pub fn view_for(t: Target, audio_track: bool) -> Cat {
    match t {
        Target::Setting => {
            if audio_track {
                Cat::Effects
            } else {
                Cat::Instruments
            }
        }
        _ => Cat::Presets,
    }
}

/// The preset list's plug-in for a target (None: the instrument; Some(k): Audio FX k).
pub fn preset_slot(t: Target) -> Option<usize> {
    match t {
        Target::Fx(k) => Some(k),
        _ => None,
    }
}

fn audio_track(app: &App, t: usize) -> bool {
    app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Audio)
}

/// Points the triangle at `target` on track `t` and shows the Browser there.
pub fn focus(app: &mut App, t: usize, target: Target) {
    app.brf.focus = Some((t, target));
    // (setting it turns Link to Selection on)
    app.browser_link = true;
    app.show_browser = true;
    let v = view_for(target, audio_track(app, t));
    app.br.view = v;
    app.brf.last_view = Some(v);
    if let Some(k) = preset_slot(target) {
        app.br.preset_slot = Some(k);
    } else if target == Target::Instrument {
        app.br.preset_slot = None;
    }
    app.br.preset_path.clear();
}

/// The triangle beside slot `slot` (inspector strips only): dim on hover, lit when focused.
pub fn triangle(app: &mut App, ui: &mut egui::Ui, slot: Rect, t: usize, target: Target) {
    if !app.brf.in_inspector {
        return;
    }
    let zone = Rect::from_min_max(pos2(slot.min.x - 8.0, slot.min.y), pos2(slot.min.x, slot.max.y));
    let lit = app.brf.focus == Some((t, target));
    let hover = ui.rect_contains_pointer(zone.union(slot.with_max_x(slot.min.x + 6.0)));
    if lit || hover {
        let c = zone.center();
        let col = if lit { Color32::from_rgb(0x2f, 0x9c, 0xf0) } else { Color32::from_gray(0x80) };
        ui.painter().add(Shape::convex_polygon(vec![pos2(c.x - 2.5, c.y - 4.0), pos2(c.x + 3.0, c.y), pos2(c.x - 2.5, c.y + 4.0)], col, Stroke::NONE));
    }
    let resp = ui.interact(zone, ui.scope_id().with(("br-focus", t, format!("{target:?}"))), Sense::click()).on_hover_text("Browser Focus");
    if resp.clicked() {
        focus(app, t, target);
    }
}

/// Each frame (with the Browser): switching between Plug-in Presets and Instruments / Effects
/// moves the triangle between the first plug-in slot and the Setting button.
pub fn follow_view(app: &mut App) {
    let v = app.br.view;
    if app.brf.last_view == Some(v) {
        return;
    }
    app.brf.last_view = Some(v);
    let Some((t, target)) = app.brf.focus else { return };
    match (v, target) {
        (Cat::Presets, Target::Setting) => {
            let first = if audio_track(app, t) { Target::Fx(0) } else { Target::Instrument };
            app.brf.focus = Some((t, first));
            app.br.preset_slot = preset_slot(first);
        }
        (Cat::Instruments | Cat::Effects, Target::Instrument | Target::Fx(_)) => app.brf.focus = Some((t, Target::Setting)),
        _ => {}
    }
}

#[allow(dead_code)]
fn _v() -> egui::Vec2 {
    vec2(0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_and_views() {
        assert_eq!(view_for(Target::Setting, false), Cat::Instruments);
        assert_eq!(view_for(Target::Setting, true), Cat::Effects);
        assert_eq!(view_for(Target::Fx(2), true), Cat::Presets);
        assert_eq!(preset_slot(Target::Fx(2)), Some(2));
        assert_eq!(preset_slot(Target::Instrument), None);
    }
}
