//! The Session Player editor for a Drummer region, as Logic's: the region's hits along the top;
//! the drummer (pick a character by genre: their kit, style and feel); the XY pad (simple ↔
//! complex across, soft ↔ loud up); the kit sections (Percussion, Hi-Hat / Cymbal, Kick &
//! Snare: on or off, and a pattern variation, 0 = follow the pad); Fills and Swing knobs; and
//! the Details tab (Feel, Ghost Notes, Hi-Hat, Humanize, Dynamics, Follow, Snare, Half / Double
//! time). The performance itself is made by lg-players, measured against Logic's own regions.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_players::character::{drummers_in, Genre};
use lg_players::drummer::{generate, DrummerParams, KitSection, SnareMode, SwingType, TempoFeel, CYMBAL_VARIATIONS, KICK_SNARE_VARIATIONS};
use lg_players::{Marker, RegionSpec};

use crate::app::App;
use crate::look;
use crate::widgets;

pub type Params = DrummerParams;

const GENRES: [Genre; 7] = [Genre::Rock, Genre::Alternative, Genre::Songwriter, Genre::RnB, Genre::Electronic, Genre::HipHop, Genre::Percussion];

/// Makes the region's performance from its settings (and the song's markers).
pub fn regenerate(app: &mut App, t: usize, r: usize) {
    let p = app.drum_params.get(&(t, r)).cloned().unwrap_or_default();
    let sig = app.project.as_ref().map_or((4, 4), |pr| pr.signature);
    let markers: Vec<Marker> = app.markers.iter().map(|m| Marker { pos: m.0, name: m.1.clone() }).collect();
    let last_end = app.project.as_ref().and_then(|pr| pr.tracks.get(t)).map(|tr| tr.regions.iter().map(|x| x.start + x.length).max().unwrap_or(0)).unwrap_or(0);
    let follows = app.project.as_ref().and_then(|pr| pr.tracks.get(t)).is_some_and(|tr| tr.regions.get(r).is_some_and(|me| tr.regions.iter().any(|o| o.start + o.length == me.start)));
    if let Some(rg) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
        let mut spec = RegionSpec::new(rg.start, rg.length, sig);
        spec.markers = markers;
        spec.ending = rg.start + rg.length == last_end;
        spec.continues = follows;
        rg.notes = generate(&p, &spec, None);
        let ch = p.character();
        rg.name = format!("{} - {}", ch.personality, ch.name);
    }
    // the track plays the drummer's kit
    let kit = p.character().kit.to_string();
    app.instrument_choice.insert(t, kit);
    app.load_engine();
}

/// The Session Player editor (the bottom pane for a drummer region).
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize, r: usize) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(region) = app.project.as_ref().and_then(|pr| pr.tracks.get(t)).and_then(|tr| tr.regions.get(r)).cloned() else { return };
    let mut params = app.drum_params.get(&(t, r)).cloned().unwrap_or_default();
    let before = params.clone();
    let ch = params.character();
    let yellow = look::region_color(lg_core::TrackKind::Drummer);
    // ---- the header: the drummer's name (a menu of every drummer by genre), the tabs
    let head = Rect::from_min_size(area.min, vec2(area.width(), 34.0));
    p.hline(head.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    let who = Rect::from_min_size(head.min + vec2(12.0, 5.0), vec2(260.0, 24.0));
    let who_resp = ui.interact(who, ui.scope_id().with(("drummer-who", t, r)), Sense::click());
    if who_resp.hovered() {
        p.rect_filled(who, 5.0, look::group_bg());
    }
    p.text(pos2(who.min.x + 6.0, who.center().y), Align2::LEFT_CENTER, format!("{} — {}", ch.personality, ch.name), look::semibold(13.0), yellow);
    p.text(pos2(who.max.x - 8.0, who.center().y), Align2::RIGHT_CENTER, "⌄", look::regular(12.0), look::text_dim());
    egui::Popup::menu(&who_resp).show(|ui| {
        ui.set_min_width(240.0);
        for g in GENRES {
            ui.menu_button(g.name(), |ui| {
                for c in drummers_in(g) {
                    if ui.add(egui::Button::selectable(c.id == ch.id, format!("{} — {}", c.personality, c.name)).shortcut_text(c.kit)).clicked() {
                        params = DrummerParams { seed: params.seed, ..DrummerParams::for_character(c) };
                        ui.close();
                    }
                }
            });
        }
    });
    p.text(pos2(who.max.x + 14.0, who.center().y), Align2::LEFT_CENTER, format!("Kit: {}", ch.kit), look::regular(12.0), look::text_dim());
    let names = ["Main", "Details", "Manual"];
    let tw = 82.0;
    for (i, n) in names.iter().enumerate() {
        let tr = Rect::from_center_size(pos2(head.max.x - 14.0 - tw * (2.5 - i as f32), head.center().y), vec2(tw - 4.0, 24.0));
        let on = app.drum_tab == i;
        p.rect_filled(tr, 6.0, if on { yellow } else { look::group_bg() });
        p.text(tr.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if on { Color32::BLACK } else { look::text() });
        if ui.interact(tr, ui.scope_id().with(("drumtab", i)), Sense::click()).clicked() {
            app.drum_tab = i;
        }
    }
    // ---- the region's hits (cymbals high, kick low)
    let strip = Rect::from_min_max(pos2(area.min.x + 12.0, head.max.y + 8.0), pos2(area.max.x - 12.0, head.max.y + 70.0));
    p.rect_filled(strip, 4.0, yellow.gamma_multiply(0.85));
    let span = region.length.max(1) as f32;
    let ink = look::mix(yellow, Color32::BLACK, 0.6);
    let kit = ch.kit_map();
    for n in &region.notes {
        let x = strip.min.x + (n.pos.saturating_sub(region.start)) as f32 / span * strip.width();
        let row = match kit.piece(n.pitch).map(|pc| pc.family()) {
            Some(lg_players::kit::Family::Kick) => 0.85,
            Some(lg_players::kit::Family::Snare) => 0.62,
            Some(lg_players::kit::Family::Tom) => 0.48,
            Some(lg_players::kit::Family::HiHat) => 0.3,
            Some(lg_players::kit::Family::Cymbal) => 0.14,
            _ => 0.4,
        };
        let y = strip.min.y + row * strip.height();
        let h = 2.0 + n.velocity as f32 / 127.0 * 6.0;
        p.add(egui::Shape::convex_polygon(vec![pos2(x, y - h), pos2(x + 3.5, y), pos2(x, y + h)], ink, Stroke::NONE));
    }
    let body = Rect::from_min_max(pos2(area.min.x + 12.0, strip.max.y + 10.0), area.max - vec2(12.0, 8.0));
    match app.drum_tab {
        0 => main_tab(app, ui, body, &mut params, yellow, t, r),
        1 => details_tab(ui, body, &mut params, t, r),
        _ => {
            p.text(body.center(), Align2::CENTER_CENTER, "Manual: the region's notes are in the Piano Roll (double-click the region)", look::regular(12.0), look::text_dim());
        }
    }
    if params != before {
        app.checkpoint("Change Session Player");
        app.drum_params.insert((t, r), params);
        regenerate(app, t, r);
        app.dirty = true;
    }
}

fn main_tab(app: &mut App, ui: &mut egui::Ui, body: Rect, params: &mut DrummerParams, yellow: Color32, t: usize, r: usize) {
    let p = ui.painter().clone();
    // the XY pad (Logic: Simple → Complex across, Soft → Loud up), the puck where the settings are
    let side = body.height().clamp(100.0, 220.0) - 30.0;
    let pad = Rect::from_min_size(body.min, vec2(side * 1.35, side));
    p.rect_filled(pad, 8.0, look::lcd_bg());
    for k in 1..4 {
        let x = pad.min.x + pad.width() * k as f32 / 4.0;
        let y = pad.min.y + pad.height() * k as f32 / 4.0;
        p.vline(x, pad.y_range(), Stroke::new(1.0, Color32::from_white_alpha(12)));
        p.hline(pad.x_range(), y, Stroke::new(1.0, Color32::from_white_alpha(12)));
    }
    let lbl = look::regular(10.5);
    p.text(pad.left_bottom() + vec2(6.0, -6.0), Align2::LEFT_BOTTOM, "Simple · Soft", lbl.clone(), look::lcd_dim());
    p.text(pad.right_bottom() + vec2(-6.0, -6.0), Align2::RIGHT_BOTTOM, "Complex", lbl.clone(), look::lcd_dim());
    p.text(pad.left_top() + vec2(6.0, 6.0), Align2::LEFT_TOP, "Loud", lbl, look::lcd_dim());
    let resp = ui.interact(pad, ui.scope_id().with(("xy", t, r)), Sense::click_and_drag());
    if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        params.complexity = ((pp.x - pad.min.x) / pad.width()).clamp(0.0, 1.0);
        params.intensity = ((pad.max.y - pp.y) / pad.height()).clamp(0.0, 1.0);
        app.help_tag = Some(vec![("Complexity".into(), format!("{:.0}", params.complexity * 100.0)), ("Intensity".into(), format!("{:.0}", params.intensity * 100.0))]);
    }
    let puck = pos2(pad.min.x + params.complexity * pad.width(), pad.max.y - params.intensity * pad.height());
    p.circle_filled(puck, 9.0, yellow);
    p.circle_stroke(puck, 9.0, Stroke::new(2.0, Color32::WHITE));
    // Regenerate under the pad
    let regen = Rect::from_min_size(pos2(pad.min.x, pad.max.y + 6.0), vec2(pad.width(), 24.0));
    if ui.put(regen, egui::Button::new("Regenerate").corner_radius(6.0)).clicked() {
        *params = params.regenerated();
    }
    // the kit sections: each a switch and a variation slider (0 = Auto, the pad decides)
    let kx = pad.max.x + 24.0;
    let sections: [(&str, u8); 3] = [("Percussion", 4), ("Hi-Hat / Cymbal", CYMBAL_VARIATIONS), ("Kick & Snare", KICK_SNARE_VARIATIONS)];
    let row_h = (pad.height() / 3.0).clamp(40.0, 52.0);
    for (i, (name, max)) in sections.into_iter().enumerate() {
        let y = pad.min.y + i as f32 * row_h;
        let sec: &mut KitSection = match i {
            0 => &mut params.percussion,
            1 => &mut params.cymbals,
            _ => &mut params.kick_snare,
        };
        widgets::check_slot(ui, Rect::from_min_size(pos2(kx, y + 4.0), vec2(20.0, 20.0)), ui.scope_id().with(("sec-on", i, t, r)), &mut sec.enabled);
        p.text(pos2(kx + 26.0, y + 14.0), Align2::LEFT_CENTER, name, look::regular(12.0), if sec.enabled { look::text() } else { look::text_dim() });
        let slider = Rect::from_min_size(pos2(kx + 26.0, y + 26.0), vec2(170.0, 14.0));
        let mut v = sec.variation as f32;
        let r2 = ui.interact(slider.expand(4.0), ui.scope_id().with(("sec-var", i, t, r)), Sense::click_and_drag());
        if let (true, Some(pp)) = (r2.dragged() || r2.clicked(), r2.interact_pointer_pos()) {
            v = (((pp.x - slider.min.x) / slider.width()) * max as f32).round().clamp(0.0, max as f32);
        }
        sec.variation = v as u8;
        p.rect_filled(Rect::from_center_size(slider.center(), vec2(slider.width(), 4.0)), 2.0, look::group_bg());
        for k in 0..=max {
            let x = slider.min.x + slider.width() * k as f32 / max as f32;
            p.circle_filled(pos2(x, slider.center().y), 2.0, look::text_dim());
        }
        let kx2 = slider.min.x + slider.width() * v / max as f32;
        p.circle_filled(pos2(kx2, slider.center().y), 6.0, if sec.enabled { yellow } else { look::text_dim() });
        p.text(pos2(slider.max.x + 10.0, slider.center().y), Align2::LEFT_CENTER, if v == 0.0 { "Auto".to_string() } else { format!("{v:.0}") }, look::regular(11.0), look::text_dim());
    }
    // Fills and Swing knobs
    let fx = kx + 250.0;
    let kr = |k: f32| Rect::from_min_size(pos2(fx + k * 90.0, pad.min.y), vec2(80.0, 96.0));
    widgets::knob(ui, kr(0.0), ui.scope_id().with(("fills", t, r)), &mut params.fills, 0.0, 1.0, 0.35, "Fills", &|v| format!("{:.0}", v * 100.0));
    widgets::knob(ui, kr(1.0), ui.scope_id().with(("swing", t, r)), &mut params.swing, 0.0, 1.0, 0.0, "Swing", &|v| format!("{:.0}", v * 100.0));
    let st = Rect::from_min_size(pos2(fx + 90.0, pad.min.y + 104.0), vec2(80.0, 20.0));
    let i = if params.swing_type == SwingType::Eighths { 0 } else { 1 };
    if let Some(k) = widgets::choice_slot(ui, st, ui.scope_id().with(("swingtype", t, r)), i, &["8th", "16th"]) {
        params.swing_type = if k == 0 { SwingType::Eighths } else { SwingType::Sixteenths };
    }
}

fn details_tab(ui: &mut egui::Ui, body: Rect, params: &mut DrummerParams, t: usize, r: usize) {
    let kw = 92.0;
    let kr = |k: f32| Rect::from_min_size(pos2(body.min.x + k * kw, body.min.y + 6.0), vec2(kw - 10.0, 100.0));
    let base = ui.scope_id();
    let id = move |s: &str| base.with((s, t, r));
    widgets::knob(ui, kr(0.0), id("feel"), &mut params.feel, -1.0, 1.0, 0.0, "Feel", &|v| if v.abs() < 0.02 { "Center".into() } else if v > 0.0 { format!("Push {:.0}", v * 100.0) } else { format!("Pull {:.0}", -v * 100.0) });
    widgets::knob(ui, kr(1.0), id("ghost"), &mut params.ghost_notes, 0.0, 1.0, 0.22, "Ghost Notes", &|v| format!("{:.0}", v * 100.0));
    widgets::knob(ui, kr(2.0), id("hat"), &mut params.hihat, 0.0, 1.0, 0.35, "Hi-Hat", &|v| if v < 0.2 { "Closed".into() } else if v > 0.7 { "Open".into() } else { "Half".into() });
    widgets::knob(ui, kr(3.0), id("human"), &mut params.humanize, 0.0, 1.0, 0.22, "Humanize", &|v| format!("{:.0}", v * 100.0));
    widgets::knob(ui, kr(4.0), id("dyn"), &mut params.dynamics, 0.0, 2.0, 1.0, "Dynamics", &|v| format!("{:.0} %", v * 100.0));
    widgets::knob(ui, kr(5.0), id("follow"), &mut params.follow, 0.0, 1.0, 0.0, "Follow", &|v| if v == 0.0 { "Off".into() } else { format!("{:.0}", v * 100.0) });
    let y = body.min.y + 116.0;
    let painter = ui.painter().clone();
    let label = |x: f32, s: &str| {
        painter.text(pos2(x, y + 10.0), Align2::LEFT_CENTER, s, look::regular(12.0), look::text_dim());
    };
    label(body.min.x, "Snare");
    let snares = [SnareMode::Automatic, SnareMode::Snare, SnareMode::SideStick, SnareMode::Rimshot];
    let cur = snares.iter().position(|s| *s == params.snare).unwrap_or(0);
    if let Some(k) = widgets::choice_slot(ui, Rect::from_min_size(pos2(body.min.x + 50.0, y), vec2(130.0, 20.0)), id("snare"), cur, &["Automatic", "Snare", "Side Stick", "Rimshot"]) {
        params.snare = snares[k];
    }
    label(body.min.x + 210.0, "Tempo");
    let tempos = [TempoFeel::Standard, TempoFeel::Half, TempoFeel::Double];
    let cur = tempos.iter().position(|s| *s == params.tempo).unwrap_or(0);
    if let Some(k) = widgets::choice_slot(ui, Rect::from_min_size(pos2(body.min.x + 260.0, y), vec2(130.0, 20.0)), id("tempo"), cur, &["Standard", "Half Time", "Double Time"]) {
        params.tempo = tempos[k];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_a_performance() {
        let p = DrummerParams::default();
        let n = generate(&p, &RegionSpec::new(0, 8 * 3840, (4, 4)), None);
        assert!(n.len() > 8 * 8, "a groove: {} notes", n.len());
        assert!(n.iter().all(|x| x.pos < 8 * 3840));
    }
}
