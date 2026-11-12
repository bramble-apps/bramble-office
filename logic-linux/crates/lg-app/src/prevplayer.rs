//! The Browser's Preview Player (guide: Work with the results list): previewing a loop or
//! one-shot shows its waveform at the bottom of the results list, with a Play button that
//! restarts and stops it; a click in the waveform plays from there. More ▸ Show Waveform hides
//! or shows it. Also the footer's Preview Volume button and its slider.
//!
//! No Logic picture of the player is in the research: drawn in the Browser's own style (the
//! results list's greys, Logic's white waveform on the lane background).

use egui::{pos2, vec2, Rect, Sense, Stroke};

use crate::app::App;
use crate::glyphs::{ph, ph_fill};
use crate::look;

/// The player's height (points).
pub const HEIGHT: f32 = 44.0;

/// Whether the player shows: Show Waveform on, and the selected sound is the one last previewed
/// (a loop or one-shot).
pub fn shown(app: &App) -> bool {
    app.br.saved.flag("show_waveform", true) && app.preview.wave.as_ref().is_some_and(|w| app.br.selected.first() == Some(&w.id))
}

pub fn height(app: &App) -> f32 {
    if shown(app) { HEIGHT } else { 0.0 }
}

/// Where a click at `x` in the waveform `r` starts (0..1).
pub fn frac_at(r: Rect, x: f32) -> f32 {
    ((x - r.min.x) / r.width().max(1.0)).clamp(0.0, 1.0)
}

/// The player in `r` (below the results).
pub fn show(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    if !shown(app) {
        return;
    }
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, look::group_bg());
    p.hline(r.x_range(), r.min.y + 0.5, Stroke::new(1.0, look::line()));
    let now = app.ctx.input(|i| i.time);
    let at = crate::preview::wave_position(app, now);
    // the Play button: restarts (or stops while playing)
    let play = Rect::from_center_size(pos2(r.min.x + 22.0, r.center().y), vec2(26.0, 26.0));
    let presp = ui.interact(play, ui.scope_id().with("pp-play"), Sense::click()).on_hover_text(if at.is_some() { "Stop" } else { "Play" });
    p.circle_filled(play.center(), 13.0, if presp.hovered() { look::button_on() } else { look::lane_bg() });
    crate::glyphs::paint(&p, play, if at.is_some() { ph_fill::STOP } else { ph_fill::PLAY }, 12.0, look::text());
    // the waveform
    let wr = Rect::from_min_max(pos2(play.max.x + 10.0, r.min.y + 7.0), pos2(r.max.x - 10.0, r.max.y - 7.0));
    p.rect_filled(wr, 3.0, look::lane_bg());
    if let Some(w) = app.preview.wave.as_ref() {
        let n = w.peaks.len().max(1);
        let cy = wr.center().y;
        let half = wr.height() / 2.0 - 2.0;
        let cols = wr.width().floor().max(1.0) as usize;
        // (drawn to fill the height, as Logic's overviews: scaled by the loudest peak)
        let top = w.peaks.iter().fold(0f32, |m, x| m.max(*x)).max(1e-4);
        for c in 0..cols {
            let pk = w.peaks.get(c * n / cols).copied().unwrap_or(0.0) / top;
            let x = wr.min.x + c as f32 + 0.5;
            let played = at.is_some_and(|a| (c as f32) < a * wr.width());
            p.vline(x, (cy - pk * half)..=(cy + pk * half).max(cy + 0.5), Stroke::new(1.0, if played { look::accent() } else { look::text_dim() }));
        }
    }
    if let Some(a) = at {
        let x = wr.min.x + a * wr.width();
        p.vline(x, wr.y_range(), Stroke::new(1.0, look::text()));
        ui.ctx().request_repaint();
    }
    let wresp = ui.interact(wr, ui.scope_id().with("pp-wave"), Sense::click());
    if let (true, Some(pp)) = (wresp.clicked(), wresp.interact_pointer_pos()) {
        crate::preview::play_wave_from(app, frac_at(wr, pp.x));
    }
    if presp.clicked() {
        if at.is_some() {
            app.preview.stop();
        } else {
            crate::preview::play_wave_from(app, 0.0);
        }
    }
}

/// The footer's Preview Volume button (views with loops and one-shots, as Logic's footer), its
/// slider in a pop-up.
pub fn volume_button(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let resp = ui.interact(r, ui.scope_id().with("pp-volume"), Sense::click()).on_hover_text("Preview Volume");
    let p = ui.painter();
    p.rect_filled(r, 6.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
    crate::glyphs::paint(p, r, ph::SPEAKER_HIGH, 14.0, look::text());
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(180.0);
        ui.label(egui::RichText::new("Preview Volume").color(look::text_dim()));
        let mut v = app.br.volume;
        if ui.add(egui::Slider::new(&mut v, 0.0..=1.5).show_value(false)).changed() {
            app.br.volume = v;
            app.preview.set_volume(v);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_positions() {
        let r = Rect::from_min_size(pos2(100.0, 0.0), vec2(200.0, 30.0));
        assert_eq!(frac_at(r, 100.0), 0.0);
        assert_eq!(frac_at(r, 200.0), 0.5);
        assert_eq!(frac_at(r, 400.0), 1.0);
    }

    #[test]
    fn peaks_follow_the_loudness() {
        let mut s = vec![0.0f32; 2000];
        for x in s[1000..].iter_mut() {
            *x = 0.5;
        }
        let pk = crate::preview::peaks(&s, 4);
        assert_eq!(pk, vec![0.0, 0.0, 0.5, 0.5]);
    }
}
