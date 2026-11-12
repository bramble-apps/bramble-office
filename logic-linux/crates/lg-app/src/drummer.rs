//! A Drummer region's performance, made by lg-players (measured against Logic's own regions)
//! from its settings, the song's markers, the rhythm it follows (Follow Rhythm Of: a track,
//! the chord changes) and the Manual view's custom pattern; the editor is spdrum.rs.

use lg_players::drummer::{generate, DrummerParams};
use lg_players::{Marker, RegionSpec};

use crate::app::App;

pub type Params = DrummerParams;

/// Makes the region's performance from its settings (and the song's markers).
pub fn regenerate(app: &mut App, t: usize, r: usize) {
    let p = app.drum_params.get(&(t, r)).cloned().unwrap_or_default();
    let sig = app.project.as_ref().map_or((4, 4), |pr| pr.signature);
    let markers: Vec<Marker> = app.markers.iter().map(|m| Marker { pos: m.0, name: m.1.clone() }).collect();
    let last_end = app.project.as_ref().and_then(|pr| pr.tracks.get(t)).map(|tr| tr.regions.iter().map(|x| x.start + x.length).max().unwrap_or(0)).unwrap_or(0);
    let follows = app.project.as_ref().and_then(|pr| pr.tracks.get(t)).is_some_and(|tr| tr.regions.get(r).is_some_and(|me| tr.regions.iter().any(|o| o.start + o.length == me.start)));
    let bpm = app.project.as_ref().map_or(120.0, |pr| pr.tempo as f32);
    let follow = crate::spdrum::follow_onsets(app, t, r, p.character().genre == lg_players::character::Genre::Percussion);
    let manual = app.spdrum.manual.get(&(t, r)).cloned();
    if let Some(rg) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
        let mut spec = RegionSpec::new(rg.start, rg.length, sig);
        spec.markers = markers;
        spec.ending = rg.start + rg.length == last_end;
        spec.continues = follows;
        spec.bpm = bpm;
        let fol = if follow.is_empty() { None } else { Some(&follow[..]) };
        rg.notes = generate(&p, &spec, fol);
        if let Some(m) = manual.as_ref().filter(|m| m.active() || m.muted.iter().any(|x| *x)) {
            crate::spdrum::apply_manual(&mut rg.notes, m, p.character().kit_map(), rg.start, rg.length, lg_core::project::bar_ticks(sig));
        }
        let ch = p.character();
        rg.name = format!("Drummer - {}", ch.name);
    }
    // the track plays the drummer's kit
    let kit = p.character().kit.to_string();
    app.instrument_choice.insert(t, kit);
    app.load_engine();
}

/// The Session Player editor (the bottom pane for a drummer region): spdrum.rs.
pub fn show(app: &mut App, ui: &mut egui::Ui, area: egui::Rect, t: usize, r: usize) {
    crate::spdrum::show(app, ui, area, t, r);
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
