//! The Inspector (I): "Region:" and "Track:" headers for the selection, the region's facts, and
//! the selected track's channel strip beside the Stereo Out, as Logic's Inspector shows them.

use egui::{pos2, vec2, Align2, Rect, Stroke};

use crate::app::App;
use crate::look;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    let proj = app.project.clone();
    let track = app.selected_track.and_then(|t| proj.as_ref()?.tracks.get(t).cloned());
    let region = app.selected_region.and_then(|(t, r)| proj.as_ref()?.tracks.get(t)?.regions.get(r).cloned());
    let mut y = area.min.y;
    let header = |p: &egui::Painter, y: f32, label: &str, value: &str| {
        let r = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), 32.0));
        p.text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, label, look::bold(13.0), look::text());
        let lw = p.layout_no_wrap(label.to_string(), look::bold(13.0), look::text()).size().x;
        p.with_clip_rect(r.shrink2(vec2(8.0, 0.0))).text(pos2(r.min.x + 16.0 + lw, r.center().y), Align2::LEFT_CENTER, value, look::regular(13.0), look::text_dim());
        p.hline(r.x_range(), r.max.y, Stroke::new(1.0, look::line()));
    };
    header(&p, y, "Region:", region.as_ref().map_or("", |r| r.name.as_str()));
    y += 33.0;
    if let Some(r) = &region {
        let bar = app.bar_ticks();
        let fmt_pos = |t: u32| {
            let beats = app.project.as_ref().map_or(4, |p| p.signature.0) as u32;
            let beat_t = bar / beats;
            format!("{} {} {}", t / bar + 1, (t % bar) / beat_t + 1, (t % beat_t) / (beat_t / 4).max(1) + 1)
        };
        for (k, v) in [("Position", fmt_pos(r.start)), ("Length", format!("{} bars", (r.length as f32 / bar as f32 * 100.0).round() / 100.0)), ("Notes", r.notes.len().to_string())] {
            p.text(pos2(area.min.x + 14.0, y + 10.0), Align2::LEFT_CENTER, k, look::regular(12.0), look::text_dim());
            p.text(pos2(area.max.x - 12.0, y + 10.0), Align2::RIGHT_CENTER, v, look::regular(12.0), look::text());
            y += 21.0;
        }
        y += 6.0;
        p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    }
    header(&p, y, "Track:", track.as_ref().map_or("", |t| t.name.as_str()));
    y += 40.0;
    // the channel strip summary: instrument, plug-ins, volume, pan
    if let Some(t) = &track {
        let rows = [
            ("Volume", format!("{:.1} dB", t.volume_db)),
            ("Pan", format!("{:+.0}", t.pan * 64.0)),
            ("Mute", if t.mute { "On".into() } else { "Off".into() }),
            ("Solo", if t.solo { "On".into() } else { "Off".into() }),
            ("Regions", t.regions.len().to_string()),
        ];
        for (k, v) in rows {
            p.text(pos2(area.min.x + 14.0, y + 10.0), Align2::LEFT_CENTER, k, look::regular(12.0), look::text_dim());
            p.text(pos2(area.max.x - 12.0, y + 10.0), Align2::RIGHT_CENTER, v, look::regular(12.0), look::text());
            y += 21.0;
        }
    }
    // the project's plug-ins (read from its channel strips)
    if let Some(pr) = &proj {
        if !pr.plugins.is_empty() {
            y += 10.0;
            p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
            y += 8.0;
            p.text(pos2(area.min.x + 12.0, y + 8.0), Align2::LEFT_CENTER, "Plug-ins in the project", look::bold(12.0), look::text());
            y += 22.0;
            let mut names: Vec<&str> = pr.plugins.iter().map(|x| x.name.as_str()).filter(|n| !n.is_empty()).collect();
            names.sort();
            names.dedup();
            for n in names.into_iter().take(((area.max.y - y) / 19.0).max(0.0) as usize) {
                p.with_clip_rect(area).text(pos2(area.min.x + 16.0, y + 8.0), Align2::LEFT_CENTER, n, look::regular(11.5), look::text_dim());
                y += 19.0;
            }
        }
    }
}
