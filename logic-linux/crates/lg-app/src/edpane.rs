//! The editor pane as Logic's: it shows the selected track's content even when no region is
//! selected (the region under the playhead, else the next one, else the last one). Its tabs say
//! which editors fit: a selected MIDI region has Piano Roll | Score | Smart Tempo; an
//! instrument track with no region selected has Piano Roll | Score | Step Sequencer | Session
//! Player (RESIZING.txt), where Step Sequencer and Session Player make that kind of region; a
//! Session Player region shows the "Session Player" title alone (ui-session-player-keys-
//! details.png). A track with no regions shows Logic's empty Piano Roll (keyboard, grid, ruler).
//! Logic's tab capsule: centred, 127 pt per tab, 24 pt tall.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;
use crate::score::EditorTab;

/// The tab chosen while no region is selected (Piano Roll, Score, Step Sequencer, Session Player).
fn nosel_id() -> egui::Id {
    egui::Id::unique("lg-nosel-editor-tab")
}

pub fn nosel_tab(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<usize>(nosel_id())).unwrap_or(0)
}

fn set_nosel_tab(ctx: &egui::Context, k: usize) {
    ctx.data_mut(|d| d.insert_temp(nosel_id(), k));
}

/// The region a track's editor shows when none is selected: the one under the playhead, else
/// the next one after it, else the last one before it (pure: for tests).
pub fn pick_region(regions: &[(u32, u32)], playhead: u32) -> Option<usize> {
    if let Some(i) = regions.iter().position(|&(s, l)| s <= playhead && playhead < s + l.max(1)) {
        return Some(i);
    }
    let next = regions.iter().enumerate().filter(|(_, r)| r.0 >= playhead).min_by_key(|(_, r)| r.0).map(|(i, _)| i);
    next.or_else(|| regions.iter().enumerate().max_by_key(|(_, r)| r.0).map(|(i, _)| i))
}

/// The region the editor pane shows: the selected one, else the selected track's (pick_region).
pub fn shown_region(app: &App) -> Option<(usize, usize)> {
    if app.selected_region.is_some() {
        return app.selected_region;
    }
    let t = app.selected_track?;
    let tr = app.project.as_ref()?.tracks.get(t)?;
    let spans: Vec<(u32, u32)> = tr.regions.iter().map(|r| (r.start, r.length)).collect();
    pick_region(&spans, app.position().max(0.0) as u32).map(|r| (t, r))
}

/// The tabs: a selected region's, or an instrument track's with no region selected.
pub fn tab_names(no_region_selected: bool) -> &'static [&'static str] {
    if no_region_selected {
        &["Piano Roll", "Score", "Step Sequencer", "Session Player"]
    } else {
        &["Piano Roll", "Score", "Smart Tempo"]
    }
}

/// An instrument track (not a Drummer's, not audio) is selected with no region selected.
pub fn no_region_selected(app: &App) -> bool {
    let instrument = app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).is_some_and(|t| t.kind == lg_core::TrackKind::Instrument);
    instrument && (app.fit.picked || app.selected_region.is_none())
}

/// Logic's editor tabs, centred in `area`'s top: a click switches editor.
pub fn tabs(app: &mut App, ui: &mut egui::Ui, area: Rect, current: EditorTab) {
    let ctx = ui.ctx().clone();
    if no_region_selected(app) {
        let on = match nosel_tab(&ctx) {
            k @ 2..=3 => k,
            _ => usize::from(current == EditorTab::Score),
        };
        if let Some(i) = draw_tabs(ui, area, tab_names(true), on) {
            set_nosel_tab(&ctx, i);
            match i {
                0 => crate::score::set_editor_tab(&ctx, EditorTab::PianoRoll),
                1 => crate::score::set_editor_tab(&ctx, EditorTab::Score),
                _ => {}
            }
        }
        return;
    }
    let on = usize::from(current == EditorTab::Score);
    if let Some(i) = draw_tabs(ui, area, tab_names(false), on) {
        match i {
            0 => crate::score::set_editor_tab(&ctx, EditorTab::PianoRoll),
            1 => crate::score::set_editor_tab(&ctx, EditorTab::Score),
            _ => app.tell("The Smart Tempo editor for MIDI regions isn't available yet"),
        }
    }
}

/// Draws the capsule of tabs (`on` lit); returns a clicked tab.
pub fn draw_tabs(ui: &mut egui::Ui, area: Rect, names: &[&str], on: usize) -> Option<usize> {
    let p = ui.painter_at(area);
    let tw = 127.0f32.min((area.width() - 20.0) / names.len() as f32);
    let h = 24.0;
    let x0 = area.center().x - tw * names.len() as f32 / 2.0;
    let y0 = area.center().y - h / 2.0;
    let pill = Rect::from_min_size(pos2(x0, y0), vec2(tw * names.len() as f32, h));
    p.rect_filled(pill, 7.0, look::group_bg());
    let mut clicked = None;
    for (i, n) in names.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x0 + i as f32 * tw, y0), vec2(tw, h));
        if i == on {
            p.rect_filled(r, 7.0, look::accent());
        } else if i > 0 && i != on + 1 {
            // Logic's thin divider between two unlit tabs
            p.vline(r.min.x, (r.center().y - 7.0)..=(r.center().y + 7.0), Stroke::new(1.0, look::line()));
        }
        // (a narrow pane: the label ends in "…" rather than running into the next tab)
        let col = if i == on { Color32::WHITE } else { look::text() };
        let mut job = egui::text::LayoutJob::simple_singleline(n.to_string(), look::regular(13.0), col);
        job.wrap = egui::text::TextWrapping { max_width: tw - 12.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let g = p.layout_job(job);
        p.galley(r.center() - g.size() / 2.0, g, col);
        if i != on && ui.interact(r, ui.scope_id().with(("edpane-tab", i)), Sense::click()).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

const TABS_H: f32 = 40.0;

/// With no region selected on an instrument track and the Step Sequencer or Session Player tab
/// chosen: that editor's empty view, which makes a region of its kind (Logic's default region
/// types). Returns false when the pane should show its usual editor.
pub fn create_view(app: &mut App, ui: &mut egui::Ui, area: Rect) -> bool {
    let k = nosel_tab(ui.ctx());
    if !no_region_selected(app) || k < 2 {
        return false;
    }
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    tabs(app, ui, Rect::from_min_size(area.min, vec2(area.width(), TABS_H)), EditorTab::PianoRoll);
    let (text, button) = if k == 2 {
        ("No pattern region on this track at the playhead.", "Create Pattern Region")
    } else {
        ("No Session Player region on this track at the playhead.", "Create Session Player Region")
    };
    let c = area.center() + vec2(0.0, TABS_H / 2.0);
    p.text(c - vec2(0.0, 18.0), Align2::CENTER_CENTER, text, look::regular(13.0), look::text_dim());
    let w = ui.fonts_mut(|f| f.layout_no_wrap(button.to_string(), look::regular(13.0), Color32::WHITE).size().x) + 28.0;
    let br = Rect::from_center_size(c + vec2(0.0, 14.0), vec2(w, 24.0));
    let resp = ui.interact(br, ui.scope_id().with("edpane-create"), Sense::click());
    p.rect_filled(br, 6.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
    p.text(br.center(), Align2::CENTER_CENTER, button, look::regular(13.0), look::text());
    if resp.clicked() {
        app.selected_region = None;
        app.fit.picked = false;
        let ctx = ui.ctx().clone();
        app.key_command(&ctx, button);
        set_nosel_tab(&ctx, 0);
    }
    true
}

/// Logic's Piano Roll for a track with no regions: the tabs, the local bar, the keyboard
/// (C3 marked), the grid of rows and bars and the ruler, and nothing in it.
pub fn empty_roll(app: &mut App, ui: &mut egui::Ui, area: Rect, local_h: f32, keys_w: f32, ruler_h: f32, row: f32) {
    let p = ui.painter_at(area);
    let local = Rect::from_min_size(pos2(area.min.x, area.min.y + TABS_H), vec2(area.width(), local_h));
    p.rect_filled(local, 0.0, look::header_bg());
    p.hline(local.x_range(), local.max.y, Stroke::new(1.0, look::line()));
    crate::barfit::roll_bar(app, ui, local);
    let ruler = Rect::from_min_max(pos2(area.min.x + keys_w, local.max.y), pos2(area.max.x, local.max.y + ruler_h));
    let keys = Rect::from_min_max(pos2(area.min.x, ruler.max.y), pos2(area.min.x + keys_w, area.max.y));
    let grid = Rect::from_min_max(pos2(keys.max.x, ruler.max.y), area.max);
    p.rect_filled(ruler, 0.0, look::header_bg());
    p.rect_filled(keys, 0.0, Color32::from_gray(0xee));
    let gp = ui.painter_at(grid);
    let kp = ui.painter_at(keys);
    let top = app.roll_top;
    let rows = (grid.height() / row).ceil() as i32 + 1;
    for k in 0..rows {
        let pitch = top as i32 - k;
        if !(0..=127).contains(&pitch) {
            continue;
        }
        let y1 = grid.min.y + (k + 1) as f32 * row;
        let rr = Rect::from_min_max(pos2(grid.min.x, y1 - row), pos2(grid.max.x, y1));
        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        if black {
            gp.rect_filled(rr, 0.0, look::lane_alt());
            kp.rect_filled(Rect::from_min_max(pos2(keys.min.x, y1 - row), pos2(keys.min.x + keys_w * 0.62, y1)), 1.5, Color32::from_gray(0x1a));
        } else if matches!(pitch % 12, 0 | 5) {
            kp.hline(keys.x_range(), y1, Stroke::new(1.0, Color32::from_gray(0x9a)));
        }
        if pitch % 12 == 0 {
            gp.hline(grid.x_range(), y1, Stroke::new(1.0, look::line()));
            kp.text(pos2(keys.max.x - 3.0, y1 - row / 2.0), Align2::RIGHT_CENTER, format!("C{}", pitch / 12 - 2), look::semibold(9.5), Color32::from_gray(0x40));
        }
    }
    let beats = app.project.as_ref().map_or(4, |p| p.signature.0) as i32;
    let bw = app.roll_bar_w;
    let first = app.roll_scroll_x.floor() as i32;
    for b in first..first + (grid.width() / bw).ceil() as i32 + 1 {
        for k in 0..beats {
            let x = grid.min.x + (b as f32 + k as f32 / beats as f32 - app.roll_scroll_x) * bw;
            gp.vline(x, grid.y_range(), Stroke::new(1.0, if k == 0 { look::line() } else { look::grid_line() }));
            if k == 0 {
                p.text(pos2(x + 3.0, ruler.min.y + 8.0), Align2::LEFT_CENTER, format!("{}", b + 1), look::semibold(11.0), look::text());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_region_under_the_playhead_then_the_next_then_the_last() {
        let regs = [(0, 1920), (3840, 1920), (7680, 960)];
        assert_eq!(pick_region(&regs, 100), Some(0));
        assert_eq!(pick_region(&regs, 2000), Some(1));
        assert_eq!(pick_region(&regs, 4000), Some(1));
        assert_eq!(pick_region(&regs, 9000), Some(2));
        assert_eq!(pick_region(&[], 0), None);
        // a region at bar 3 with the playhead at bar 1: that region (Logic opens it)
        assert_eq!(pick_region(&[(3840, 1920)], 0), Some(0));
    }

    #[test]
    fn logics_tab_sets() {
        assert_eq!(tab_names(true), &["Piano Roll", "Score", "Step Sequencer", "Session Player"]);
        assert_eq!(tab_names(false), &["Piano Roll", "Score", "Smart Tempo"]);
    }
}
