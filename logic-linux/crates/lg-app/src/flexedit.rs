//! Flex marker edits in the Audio Track Editor (Logic's "Delete, reset and move flex markers"):
//! - a flex marker's right-click menu: Delete Flex Marker, Set to Neutral Position (the marker
//!   moved to where its audio would play without it);
//! - the region background's menu on a Flex track: Reset Manual Flex Edits (the markers added by
//!   hand go; those audio quantization placed stay), Reset All Flex Edits (every marker, the
//!   region back to its original length);
//! - Option-dragging a marker moves it without time stretching (its audio point moves with it);
//! - the Eraser on a marker, or dragged across markers, deletes them (on the region header it
//!   deletes the region).

use crate::app::App;
use crate::audioedit::{self, RegionAudio};

/// The source frame of a destination frame (both from the region's start) through `flex`.
pub fn src_of(flex: &[(usize, usize)], dst: f64) -> f64 {
    let mut prev = (0.0, 0.0);
    for &(s, d) in flex {
        let (s, d) = (s as f64, d as f64);
        if dst <= d {
            return if d > prev.1 { prev.0 + (dst - prev.1) * (s - prev.0) / (d - prev.1) } else { s };
        }
        prev = (s, d);
    }
    prev.0 + (dst - prev.1)
}

pub fn dst_of(flex: &[(usize, usize)], src: f64) -> f64 {
    let mut prev = (0.0, 0.0);
    for &(s, d) in flex {
        let (s, d) = (s as f64, d as f64);
        if src <= s {
            return if s > prev.0 { prev.1 + (src - prev.0) * (d - prev.1) / (s - prev.0) } else { d };
        }
        prev = (s, d);
    }
    prev.1 + (src - prev.0)
}

/// Set to Neutral Position: marker `k` where its audio plays when it is left out.
pub fn neutral(flex: &[(usize, usize)], k: usize) -> Vec<(usize, usize)> {
    let mut v = flex.to_vec();
    if k >= v.len() {
        return v;
    }
    let m = v.remove(k);
    let d = dst_of(&v, m.0 as f64).round().max(1.0) as usize;
    v.insert(k, (m.0, d));
    v
}

/// Option-drag: marker `k` moved to `dst` with its audio point (no stretch either side).
pub fn moved_without_stretch(flex: &[(usize, usize)], k: usize, dst: usize) -> Vec<(usize, usize)> {
    let mut v = flex.to_vec();
    if k >= v.len() {
        return v;
    }
    v.remove(k);
    let src = src_of(&v, dst as f64).round().max(1.0) as usize;
    v.push((src, dst.max(1)));
    v.sort_by_key(|m| m.0);
    v
}

/// Reset Manual Flex Edits: the markers audio quantization didn't place go.
pub fn reset_manual(x: &mut RegionAudio) {
    let auto = x.flex_auto.clone();
    x.flex.retain(|m| auto.contains(&m.0));
}

fn edit(app: &mut App, t: usize, r: usize, what: &str, f: impl FnOnce(&mut App, &mut RegionAudio)) {
    crate::flex::checkpoint(app, what);
    let mut x = audioedit::extras(app, t, r);
    f(app, &mut x);
    audioedit::set_extras(app, t, r, x);
    app.after_edit();
}

/// Reset All Flex Edits: no markers, the region at its source length.
pub fn reset_all(app: &mut App, t: usize, r: usize) {
    edit(app, t, r, "Reset All Flex Edits", |app, x| {
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
        let Some(file) = rg.audio.as_ref().map(|a| a.file.clone()) else { return };
        let rate = audioedit::load(app, &file).map_or(48_000.0, |l| l.audio.rate as f64);
        let start = app.seconds_at(rg.start as f64);
        let frames = (app.seconds_at((rg.start + rg.length) as f64) - start) * rate;
        let src = src_of(&x.flex, frames);
        x.flex.clear();
        x.flex_auto.clear();
        let end = crate::smarttempo::tick_at(app, start + src / rate);
        if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            g.length = (end - g.start as f64).round().max(1.0) as u32;
        }
    });
}

/// The menus: over a marker its own, else the background's flex items above `rest`.
pub fn marker_menu(app: &mut App, ui: &mut egui::Ui, t: usize, r: usize, k: usize) {
    if ui.button("Delete Flex Marker").clicked() {
        edit(app, t, r, "Delete Flex Marker", |_, x| {
            if k < x.flex.len() {
                x.flex.remove(k);
            }
        });
        ui.close();
    }
    if ui.button("Set to Neutral Position").clicked() {
        edit(app, t, r, "Set to Neutral Position", |_, x| x.flex = neutral(&x.flex, k));
        ui.close();
    }
}

pub fn background_items(app: &mut App, ui: &mut egui::Ui, t: usize, r: usize) {
    if !crate::flex::track(app, t).on {
        return;
    }
    if ui.button("Reset Manual Flex Edits").clicked() {
        edit(app, t, r, "Reset Manual Flex Edits", |_, x| reset_manual(x));
        ui.close();
    }
    if ui.button("Reset All Flex Edits").clicked() {
        reset_all(app, t, r);
        ui.close();
    }
    ui.separator();
}

/// The Eraser over markers (x positions `hits`: (x, src, marker index)) from x `a` to `b`:
/// those markers deleted; true when any was there.
pub fn erase(app: &mut App, t: usize, r: usize, hits: &[(f32, usize, Option<usize>)], a: f32, b: f32) -> bool {
    let (lo, hi) = (a.min(b) - 4.0, a.max(b) + 4.0);
    let mut ks: Vec<usize> = hits.iter().filter(|h| h.0 >= lo && h.0 <= hi).filter_map(|h| h.2).collect();
    if ks.is_empty() {
        return false;
    }
    ks.sort_unstable();
    edit(app, t, r, "Delete Flex Marker", |_, x| {
        for k in ks.into_iter().rev() {
            if k < x.flex.len() {
                x.flex.remove(k);
            }
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_and_option_moves() {
        // a stretch: 0..1000 → 0..2000, then 1000..2000 → 2000..2500
        let flex = vec![(1000, 2000), (2000, 2500)];
        // the first marker without itself would be at 1000 → ~1667 on the line 0..2000 → 2500
        let n = neutral(&flex, 0);
        assert_eq!(n[0], (1000, 1250));
        // Option-move: the second marker to 2400 keeps the audio unstretched around it
        let m = moved_without_stretch(&flex, 1, 2400);
        assert_eq!(m[1].1, 2400);
        assert_eq!(m[1].0, 1400);
        let mut x = RegionAudio { flex: flex.clone(), flex_auto: vec![2000], ..Default::default() };
        reset_manual(&mut x);
        assert_eq!(x.flex, vec![(2000, 2500)]);
    }
}
