//! The Cell inspector with no cell selected (the Live Loops grid has the key focus): "Cell: MIDI
//! Defaults" or "Cell: Audio Defaults" for the selected track's type — the cell length and MIDI
//! quantize new cells of that type get, and the recording settings (Mode, Rec-Length, Rec-End
//! Action) cell recordings use.

use egui::{pos2, vec2, Align2, Rect, Stroke};

use crate::app::App;
use crate::inspector::{label, slot_rect, ROW};
use crate::look;
use crate::widgets;

pub const COMMANDS: &[&str] = &[];

pub const MODES_MIDI: [&str; 3] = ["Replace", "Merge", "Takes"];
pub const MODES_AUDIO: [&str; 2] = ["Replace", "Takes"];
pub const REC_LENGTHS: [&str; 3] = ["Cell Length", "Automatic (Bars)", "Automatic (Beats)"];
pub const REC_ENDS: [&str; 2] = ["Play Cell", "Continue Recording"];

/// A cell type's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Defaults {
    /// Length of new cells, in bars.
    pub bars: u32,
    /// Note quantize of new MIDI cells.
    pub quantize: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults { bars: 1, quantize: "Off".into() }
    }
}

fn audio_track(app: &App, t: usize) -> bool {
    app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Audio)
}

/// Shown: the grid focused, no cell (or region) selected, a track selected.
pub fn active(app: &App) -> bool {
    let s = &app.live_loops;
    s.view != crate::liveloops::View::TracksOnly && s.focused && s.selected.is_empty() && app.selected_region.is_none() && app.selected_track.is_some()
}

/// The defaults for cells on track `t`.
pub fn of(app: &App, t: usize) -> &Defaults {
    &app.live_loops.defaults[audio_track(app, t) as usize]
}

/// A new empty cell on track `t` as the defaults say.
pub fn new_cell(app: &App, t: usize, name: String) -> crate::liveloops::Cell {
    let d = of(app, t);
    let q = if audio_track(app, t) { "Off".to_string() } else { d.quantize.clone() };
    crate::liveloops::Cell { name, length: d.bars.max(1) * app.bar_ticks().max(1), quantize: q, ..Default::default() }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32) -> f32 {
    let p = ui.painter().clone();
    let id = ui.scope_id().with("cell-defaults");
    let Some(t) = app.selected_track else { return y };
    let audio = audio_track(app, t);
    let hr = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), 32.0));
    p.text(pos2(hr.min.x + 12.0, hr.center().y), Align2::LEFT_CENTER, "Cell:", look::bold(13.0), look::text());
    let lw = p.layout_no_wrap("Cell:".into(), look::bold(13.0), look::text()).size().x;
    p.text(pos2(hr.min.x + 16.0 + lw, hr.center().y), Align2::LEFT_CENTER, if audio { "Audio Defaults" } else { "MIDI Defaults" }, look::regular(13.0), look::text_dim());
    p.hline(hr.x_range(), hr.max.y, Stroke::new(1.0, look::line()));
    y += 33.0;
    let k = audio as usize;
    let mut d = app.live_loops.defaults[k].clone();
    if !audio {
        label(&p, area, y, "Quantize");
        let qs: Vec<&str> = std::iter::once("Off").chain(crate::inspector::QUANTIZE.iter().skip(1).map(|q| q.0)).collect();
        let cur = qs.iter().position(|q| *q == d.quantize).unwrap_or(0);
        if let Some(i) = widgets::choice_slot(ui, slot_rect(area, y), id.with("q"), cur, &qs) {
            d.quantize = qs[i].to_string();
        }
        y += ROW;
    }
    label(&p, area, y, "Cell Length");
    let mut v = d.bars as f32;
    if widgets::value_slot(ui, slot_rect(area, y), id.with("len"), &mut v, 1.0, 64.0, 1.0, 1.0, &|v: f32| format!("{v:.0} {}", if v == 1.0 { "Bar" } else { "Bars" })) {
        d.bars = v.round().max(1.0) as u32;
    }
    y += ROW;
    app.live_loops.defaults[k] = d;
    // the recording settings
    let s = &app.live_loops;
    let modes: &[&str] = if audio { &MODES_AUDIO } else { &MODES_MIDI };
    let rows: [(&str, &[&str], String); 3] = [("Recording", modes, s.rec_mode.clone()), ("Rec-Length", &REC_LENGTHS, s.rec_length.clone()), ("Rec-End", &REC_ENDS, s.rec_end.clone())];
    for (k, (name, items, cur)) in rows.into_iter().enumerate() {
        label(&p, area, y, name);
        let at = items.iter().position(|x| *x == cur).unwrap_or(0);
        if let Some(i) = widgets::choice_slot(ui, slot_rect(area, y), id.with(("rec", k)), at, items) {
            let v = items[i].to_string();
            let s = &mut app.live_loops;
            match k {
                0 => s.rec_mode = v,
                1 => s.rec_length = v,
                _ => s.rec_end = v,
            }
        }
        y += ROW;
    }
    y + 6.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cells_follow_their_type_defaults() {
        let mut app = crate::tm::test_app();
        crate::liveloops::sync(&mut app);
        let p = app.project.as_ref().unwrap();
        let midi = p.tracks.iter().position(|t| t.kind != lg_core::TrackKind::Audio).unwrap();
        app.live_loops.defaults[0] = Defaults { bars: 4, quantize: "1/16 Note".into() };
        let c = new_cell(&app, midi, "x".into());
        assert_eq!((c.length, c.quantize.as_str()), (4 * app.bar_ticks(), "1/16 Note"));
        // shown with the grid focused and nothing selected
        app.live_loops.view = crate::liveloops::View::Both;
        app.live_loops.focused = true;
        app.live_loops.selected.clear();
        app.selected_region = None;
        app.selected_track = Some(midi);
        assert!(active(&app));
        // Create MIDI Cell uses them
        app.live_loops.selected = [(midi, 3)].into();
        crate::liveloops::command(&mut app, "Create MIDI Cell");
        assert_eq!(app.live_loops.cells[&(midi, 3)].length, 4 * app.bar_ticks());
    }
}
