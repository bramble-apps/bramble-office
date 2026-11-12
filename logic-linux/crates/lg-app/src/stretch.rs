//! Time stretching regions in the Tracks area (Logic's Edit ▸ Time Stretch):
//! - Option-dragging a region's right edge stretches it: MIDI regions' events are spread or
//!   squeezed in proportion, audio regions are time stretched (to the Snap value, as the edge
//!   snaps);
//! - Time Stretching Algorithm: Universal (default), Complex, Percussive and the Legacy
//!   algorithms (Version 5, Any Material, Monophonic, Pads, Rhythmic, Beats Only), each played
//!   by one of Studio's Flex Time algorithms when the track has no Flex mode of its own:
//!   Universal / Complex / Pads / Any Material: Polyphonic; Percussive / Rhythmic: Rhythmic;
//!   Monophonic: Monophonic; Beats Only: Slicing; Version 5: Tempophone.
//! Audio stretches are non-destructive (Flex markers), not a new file as in Logic.

use lg_audio::flex::Mode;

use crate::app::App;


pub const ALGORITHMS: [(&str, Mode); 9] = [
    ("Universal", Mode::Polyphonic),
    ("Complex", Mode::Polyphonic),
    ("Percussive", Mode::Rhythmic),
    ("Version 5", Mode::Tempophone),
    ("Any Material", Mode::Polyphonic),
    ("Monophonic", Mode::Monophonic),
    ("Pads", Mode::Polyphonic),
    ("Rhythmic", Mode::Rhythmic),
    ("Beats Only", Mode::Slicing),
];

fn id() -> egui::Id {
    egui::Id::unique("lg-stretch-algorithm")
}

pub fn algorithm(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<usize>(id())).unwrap_or(0)
}

pub fn mode(ctx: &egui::Context) -> Mode {
    ALGORITHMS[algorithm(ctx).min(ALGORITHMS.len() - 1)].1
}

pub fn command(app: &mut App, name: &str) -> bool {
    let Some(k) = ALGORITHMS.iter().position(|a| a.0 == name) else { return false };
    app.ctx.data_mut(|d| d.insert_temp(id(), k));
    true
}

/// Edit ▸ Time Stretch ▸ Time Stretching Algorithm, with the tick on the current one.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if it.label != "Time Stretching Algorithm" || it.children.is_empty() {
        return false;
    }
    let cur = algorithm(ui.ctx());
    let mut pick = None;
    ui.menu_button("Time Stretching Algorithm", |ui| {
        ui.set_min_width(200.0);
        let item = |ui: &mut egui::Ui, k: usize, pick: &mut Option<usize>| {
            let n = ALGORITHMS[k].0;
            if ui.button(if cur == k { format!("✓ {n}") } else { format!("   {n}") }).clicked() {
                *pick = Some(k);
                ui.close();
            }
        };
        for k in 0..3 {
            item(ui, k, &mut pick);
        }
        ui.separator();
        ui.menu_button("Legacy Algorithms", |ui| {
            for k in 3..9 {
                item(ui, k, &mut pick);
            }
        });
    });
    if let Some(k) = pick {
        command(app, ALGORITHMS[k].0);
    }
    true
}

/// MIDI events spread from a region's old length to its new one (from the region's start).
pub fn scale_notes(rg: &mut lg_core::Region, old: u32, new: u32) {
    if old == 0 {
        return;
    }
    let k = new as f64 / old as f64;
    let start = rg.start;
    for n in &mut rg.notes {
        n.pos = start + ((n.pos.saturating_sub(start)) as f64 * k).round() as u32;
        n.len = ((n.len as f64) * k).round().max(1.0) as u32;
    }
    for e in &mut rg.ext.events {
        e.pos = (e.pos as f64 * k).round() as u32;
    }
}

/// After an Option-drag of a region's right edge (its length now changed from `old`).
pub fn option_resize(app: &mut App, t: usize, r: usize, old: u32) {
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
    let new = rg.length;
    if new == old {
        return;
    }
    if rg.audio.is_some() {
        if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            x.length = old;
        }
        crate::flex::stretch_region(app, t, r, new);
    } else if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        scale_notes(x, old, new);
    }
    app.after_edit();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_scale_with_the_region() {
        let mut rg = crate::freeze::audio_region("m", 3840, 3840, std::path::PathBuf::new(), 1.0);
        rg.audio = None;
        rg.notes.push(lg_core::events::Note { channel: 0, pitch: 60, velocity: 90, pos: 3840 + 960, len: 480, muted: false });
        rg.ext.events.push(lg_core::events::Control { kind: 0xB0, channel: 0, pos: 1920, number: 1, value: 3 });
        scale_notes(&mut rg, 3840, 7680);
        assert_eq!((rg.notes[0].pos, rg.notes[0].len), (3840 + 1920, 960));
        assert_eq!(rg.ext.events[0].pos, 3840);
        assert_eq!(ALGORITHMS[0].0, "Universal");
    }
}
