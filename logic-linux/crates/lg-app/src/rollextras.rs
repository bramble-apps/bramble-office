//! Piano Roll extras (Bramble's, beyond Logic 13):
//! - ghost notes: the notes of the track's other regions (and, optionally, every other MIDI
//!   track's) drawn faintly behind the edited region, in their tracks' colours, not clickable
//!   (View ▸ Show Ghost Notes, View ▸ Ghost Notes from All Tracks);
//! - more MIDI Draw lanes: under the velocity / controller lane, any number of further lanes,
//!   each with its own controller (its title's pop-up), added with + and removed with × at the
//!   lane's right edge.

use egui::{pos2, vec2, Color32, Rect, Stroke};

use crate::app::App;
use crate::glyphs::{self, ph};
use crate::look;

/// The menu items this file runs (Bramble's own: not in Logic's lists).
pub const COMMANDS: &[&str] = &[];

#[derive(Clone, Debug, Default)]
pub struct State {
    /// Ghost notes shown.
    pub ghosts: bool,
    /// From every MIDI track (false: the edited track's other regions only).
    pub ghosts_all: bool,
    /// The lanes under the first MIDI Draw lane: (status kind, controller); None: velocity.
    pub lanes: Vec<Option<(u8, u8)>>,
}

/// The notes to draw as ghosts: (pitch, start, end, track) in song ticks, those overlapping
/// `from..to`, from every region but the edited one (other tracks too when `all`).
pub fn ghost_notes(p: &lg_core::Project, ti: usize, ri: usize, all: bool, from: u32, to: u32) -> Vec<(u8, u32, u32, usize)> {
    let mut v = Vec::new();
    for (t, tr) in p.tracks.iter().enumerate() {
        if t != ti && !all {
            continue;
        }
        for (r, rg) in tr.regions.iter().enumerate() {
            if (t, r) == (ti, ri) || rg.audio.is_some() || rg.muted {
                continue;
            }
            let end = rg.start + rg.length;
            for n in &rg.notes {
                // a note sounds only inside its region
                let (a, b) = (n.pos.max(rg.start), (n.pos + n.len).min(end));
                if n.muted || a >= b || b <= from || a >= to {
                    continue;
                }
                v.push((n.pitch, a, b, t));
            }
        }
    }
    v
}

/// Draws the ghost notes (before the region's own).
#[allow(clippy::too_many_arguments)]
pub fn draw_ghosts(app: &App, p: &egui::Painter, grid: Rect, ti: usize, ri: usize, x_of: &dyn Fn(f32) -> f32, tick_at: &dyn Fn(f32) -> f32, y_of: &dyn Fn(u8) -> Option<f32>, row: f32) {
    if !app.rx.ghosts {
        return;
    }
    let Some(pr) = app.project.as_ref() else { return };
    let from = tick_at(grid.min.x).max(0.0) as u32;
    let to = tick_at(grid.max.x).max(0.0) as u32 + 1;
    for (pitch, a, b, t) in ghost_notes(pr, ti, ri, app.rx.ghosts_all, from, to) {
        let Some(y) = y_of(pitch) else { continue };
        let c = pr.tracks.get(t).map_or(Color32::GRAY, crate::trackhdr::color_of);
        let r = Rect::from_min_max(pos2(x_of(a as f32), y - row + 2.5), pos2(x_of(b as f32).max(x_of(a as f32) + 3.0), y - 2.5));
        if !r.intersects(grid) {
            continue;
        }
        p.rect_filled(r, 2.0, c.gamma_multiply(0.22));
        p.rect_stroke(r, 2.0, Stroke::new(1.0, c.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    }
}

/// The Piano Roll's View menu: Bramble's items after Logic's.
pub fn view_items(app: &mut App, ui: &mut egui::Ui) {
    ui.separator();
    let tick = |on: bool, s: &str| if on { format!("✓ {s}") } else { s.to_string() };
    if ui.button(tick(app.rx.ghosts, "Show Ghost Notes")).clicked() {
        app.rx.ghosts = !app.rx.ghosts;
        ui.close();
    }
    if ui.add_enabled(app.rx.ghosts, egui::Button::new(tick(app.rx.ghosts_all, "Ghost Notes from All Tracks"))).clicked() {
        app.rx.ghosts_all = !app.rx.ghosts_all;
        ui.close();
    }
    if ui.button("Add MIDI Draw Lane").clicked() {
        add_lane(app);
        ui.close();
    }
}

fn add_lane(app: &mut App) {
    app.midi.velocity_lane = true;
    // a new lane starts on Modulation (the velocity lane is usually there already)
    app.rx.lanes.push(Some((0xB0, 1)));
}

/// The height of each MIDI Draw lane, and how many there are, for `avail` points under the
/// ruler (one lane: as before; more share up to two thirds of the editor).
pub fn lane_layout(n: usize, avail: f32, lane_h: f32) -> f32 {
    if n <= 1 {
        return lane_h.min(avail * 0.4);
    }
    let share = (0.4 + 0.12 * (n - 1) as f32).min(0.66);
    lane_h.min(avail * share / n as f32)
}

/// The extra lanes under the first one (`first`), each drawn by `draw` with its controller in
/// place of the first lane's; the + / × buttons at the right edge.
pub fn lanes(app: &mut App, ui: &mut egui::Ui, first: Rect, draw: &mut dyn FnMut(&mut App, &mut egui::Ui, Rect)) {
    let h = first.height();
    let mut remove = None;
    let main = app.arr.lane;
    for k in 0..app.rx.lanes.len() {
        let r = first.translate(vec2(0.0, h * (k + 1) as f32));
        app.arr.lane = app.rx.lanes[k];
        ui.push_id(("bramble-lane", k), |ui| draw(app, ui, r));
        app.rx.lanes[k] = app.arr.lane;
        app.arr.lane = main;
        if lane_button(ui, Rect::from_min_size(pos2(r.max.x - 22.0, r.min.y + 3.0), vec2(18.0, 16.0)), ui.scope_id().with(("lane-x", k)), ph::X, "Remove this MIDI Draw lane") {
            remove = Some(k);
        }
    }
    if let Some(k) = remove {
        app.rx.lanes.remove(k);
    }
    if lane_button(ui, Rect::from_min_size(pos2(first.max.x - 22.0, first.min.y + 3.0), vec2(18.0, 16.0)), ui.scope_id().with("lane-add"), ph::PLUS, "Add a MIDI Draw lane") {
        add_lane(app);
    }
}

fn lane_button(ui: &mut egui::Ui, r: Rect, id: egui::Id, icon: &str, tip: &str) -> bool {
    let resp = ui.interact(r, id, egui::Sense::click()).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, look::button_on());
    }
    glyphs::paint(ui.painter(), r, icon, 12.0, look::text());
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lg_core::events::Note;

    fn note(pos: u32, pitch: u8) -> Note {
        Note { channel: 0, pitch, velocity: 100, pos, len: 240, muted: false }
    }

    #[test]
    fn ghosts_come_from_the_other_regions() {
        let mut p = lg_core::Project::default();
        let region = |start: u32, notes: Vec<Note>| lg_core::Region { name: "R".into(), start, length: 3840, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        let track = |regions: Vec<lg_core::Region>| lg_core::Track { name: "T".into(), kind: lg_core::TrackKind::Instrument, channel: 0, row: 0, regions, volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() };
        let a = track(vec![region(0, vec![note(0, 60)]), region(3840, vec![note(3840, 62), note(7600, 63)])]);
        let b = track(vec![region(0, vec![note(100, 40)])]);
        p.tracks = vec![a, b];
        // this track's other region only
        let g = ghost_notes(&p, 0, 0, false, 0, 100_000);
        assert_eq!(g.iter().map(|x| x.0).collect::<Vec<_>>(), vec![62, 63]);
        // the second note is cut at its region's end
        assert_eq!(g[1].2, 7680);
        // every track
        let g = ghost_notes(&p, 0, 0, true, 0, 100_000);
        assert_eq!(g.iter().map(|x| (x.0, x.3)).collect::<Vec<_>>(), vec![(62, 0), (63, 0), (40, 1)]);
        // only what's in view
        assert!(ghost_notes(&p, 0, 0, true, 5000, 6000).is_empty());
    }

    #[test]
    fn lanes_share_the_space() {
        assert_eq!(lane_layout(1, 500.0, 96.0), 96.0);
        assert_eq!(lane_layout(1, 100.0, 96.0), 40.0);
        let h = lane_layout(3, 600.0, 96.0);
        assert!(h * 3.0 <= 600.0 * 0.66 + 0.01 && h > 50.0);
    }
}
