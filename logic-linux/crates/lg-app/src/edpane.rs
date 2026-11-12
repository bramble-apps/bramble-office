//! The editor pane as Logic's: it shows the selected track's content even when no region is
//! selected (the region under the playhead, else the next one, else the first), its tabs say
//! which editors fit that content, and a Session Player region's editor has Session Player |
//! Piano Roll | Score tabs. Logic's tab capsule: centred, 127 pt per tab, 24 pt tall.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense};

use crate::app::App;
use crate::look;
use crate::score::EditorTab;

/// Which editor a Session Player region shows (its tabs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpTab {
    #[default]
    SessionPlayer,
    PianoRoll,
    Score,
}

fn sp_id() -> egui::Id {
    egui::Id::unique("lg-sp-editor-tab")
}

pub fn sp_tab(ctx: &egui::Context) -> SpTab {
    ctx.data(|d| d.get_temp::<SpTab>(sp_id())).unwrap_or_default()
}

pub fn set_sp_tab(ctx: &egui::Context, tab: SpTab) {
    ctx.data_mut(|d| d.insert_temp(sp_id(), tab));
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

/// The tabs for what the pane shows: a Session Player region's, or the MIDI editors'.
pub fn tab_names(session_player: bool) -> &'static [&'static str] {
    if session_player {
        &["Session Player", "Piano Roll", "Score"]
    } else {
        &["Piano Roll", "Score", "Smart Tempo"]
    }
}

/// Whether the region in the pane is a Bass / Keyboard Player's.
pub fn is_player_region(app: &App) -> bool {
    app.selected_region.is_some_and(|x| app.players.contains_key(&x))
}

/// Logic's editor tabs, centred in `area`'s top: a click switches editor.
pub fn tabs(app: &mut App, ui: &mut egui::Ui, area: Rect, current: EditorTab) {
    let sp = is_player_region(app);
    let names = tab_names(sp);
    let on = if sp {
        match sp_tab(ui.ctx()) {
            SpTab::SessionPlayer => 0,
            SpTab::PianoRoll => 1,
            SpTab::Score => 2,
        }
    } else {
        usize::from(current == EditorTab::Score)
    };
    if let Some(i) = draw_tabs(ui, area, names, on) {
        let ctx = ui.ctx().clone();
        match (sp, i) {
            (true, 0) => set_sp_tab(&ctx, SpTab::SessionPlayer),
            (true, 1) => {
                set_sp_tab(&ctx, SpTab::PianoRoll);
                crate::score::set_editor_tab(&ctx, EditorTab::PianoRoll);
            }
            (true, _) => {
                set_sp_tab(&ctx, SpTab::Score);
                crate::score::set_editor_tab(&ctx, EditorTab::Score);
            }
            (false, 0) => crate::score::set_editor_tab(&ctx, EditorTab::PianoRoll),
            (false, 1) => crate::score::set_editor_tab(&ctx, EditorTab::Score),
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
            p.vline(r.min.x, (r.center().y - 7.0)..=(r.center().y + 7.0), egui::Stroke::new(1.0, look::line()));
        }
        p.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if i == on { Color32::WHITE } else { look::text() });
        if i != on && ui.interact(r, ui.scope_id().with(("edpane-tab", i)), Sense::click()).clicked() {
            clicked = Some(i);
        }
    }
    clicked
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
    fn session_player_regions_get_their_own_tabs() {
        assert_eq!(tab_names(true), &["Session Player", "Piano Roll", "Score"]);
        assert_eq!(tab_names(false), &["Piano Roll", "Score", "Smart Tempo"]);
    }
}
