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


// ---------------------------------------------------------------------------------------------
// Logic's click rules for new flex markers (Time stretch using flex markers / marquee selections)

/// The transient before and after `at` (source frames, sorted list), a few frames clear of it.
fn around(transients: &[usize], at: usize) -> (Option<usize>, Option<usize>) {
    (transients.iter().copied().filter(|&t| t + 2 < at).max(), transients.iter().copied().filter(|&t| t > at + 2).min())
}

/// A click in a Flex Time waveform at source frame `at` (`near`: the transient under the
/// pointer): the upper half adds one marker (on the transient, else at the click); the lower
/// half three (on the transient or at the click, and on the transients before and after).
/// Returns the markers' source frames and the one the drag moves.
pub fn click_markers(transients: &[usize], at: usize, near: Option<usize>, upper: bool) -> (Vec<usize>, usize) {
    let c = near.unwrap_or(at);
    if upper {
        return (vec![c], c);
    }
    let (p, n) = around(transients, c);
    (p.into_iter().chain(std::iter::once(c)).chain(n).collect(), c)
}

/// A Pointer click inside a marquee selection `a`..`b` (source frames) at `at`: the upper half
/// adds four markers (the marquee's edges and the transients outside them; the drag moves the
/// selected audio, stretching only outside it), the lower half three (the click and the edges;
/// the drag moves the click, stretching only inside). Returns the markers and what the drag
/// moves (one marker, or both edges).
pub fn marquee_markers(transients: &[usize], a: usize, b: usize, at: usize, upper: bool) -> (Vec<usize>, Vec<usize>) {
    if upper {
        let (p, _) = around(transients, a);
        let (_, n) = around(transients, b);
        (p.into_iter().chain([a, b]).chain(n).collect(), vec![a, b])
    } else {
        (vec![a, at, b], vec![at])
    }
}

/// Markers added at the source frames `srcs` where their audio plays now (none where one is
/// within two frames already). Returns whether any was added.
pub fn add_at(flex: &mut Vec<(usize, usize)>, srcs: &[usize]) -> bool {
    let map = lg_audio::flex::TimeMap::new(flex);
    let mut added = false;
    for &s in srcs {
        if s == 0 || flex.iter().any(|m| m.0.abs_diff(s) < 2) {
            continue;
        }
        flex.push((s, map.dst(s as f64).round().max(1.0) as usize));
        added = true;
    }
    flex.sort_by_key(|m| m.0);
    added
}

/// Marker `k` dragged to `dst`: when it reaches the marker before it, that one jumps back to the
/// transient before it (and the one after forward to the next transient), widening the range
/// being stretched. Returns the dragged marker's index after.
pub fn cross_extend(flex: &mut [(usize, usize)], k: usize, dst: usize, transients: &[usize]) -> usize {
    if k >= flex.len() {
        return k;
    }
    let others: Vec<(usize, usize)> = flex.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, m)| *m).collect();
    let map = lg_audio::flex::TimeMap::new(&others);
    if k > 0 && dst <= flex[k - 1].1 + 1 {
        let floor = if k > 1 { flex[k - 2].0 } else { 0 };
        if let Some(tp) = transients.iter().copied().filter(|&t| t + 2 < flex[k - 1].0 && t > floor + 2).max() {
            flex[k - 1] = (tp, map.dst(tp as f64).round().max(1.0) as usize);
        }
    }
    if k + 1 < flex.len() && dst + 1 >= flex[k + 1].1 {
        let ceil = flex.get(k + 2).map_or(usize::MAX, |m| m.0);
        if let Some(tn) = transients.iter().copied().filter(|&t| t > flex[k + 1].0 + 2 && t + 2 < ceil).min() {
            flex[k + 1] = (tn, map.dst(tn as f64).round().max(1.0) as usize);
        }
    }
    k
}

#[cfg(test)]
mod click_tests {
    use super::*;

    #[test]
    fn clicks_add_one_or_three_markers_marquees_three_or_four() {
        let trs = [1000, 2000, 3000, 4000];
        assert_eq!(click_markers(&trs, 2010, Some(2000), true), (vec![2000], 2000));
        assert_eq!(click_markers(&trs, 2500, None, true), (vec![2500], 2500));
        assert_eq!(click_markers(&trs, 2010, Some(2000), false), (vec![1000, 2000, 3000], 2000));
        assert_eq!(click_markers(&trs, 2500, None, false), (vec![2000, 2500, 3000], 2500));
        assert_eq!(click_markers(&trs, 500, None, false), (vec![500, 1000], 500));
        assert_eq!(marquee_markers(&trs, 1500, 2500, 2000, true), (vec![1000, 1500, 2500, 3000], vec![1500, 2500]));
        assert_eq!(marquee_markers(&trs, 1500, 2500, 2000, false), (vec![1500, 2000, 2500], vec![2000]));
        let mut f = vec![(1000, 1100)];
        assert!(add_at(&mut f, &[1000, 2000]));
        // (after the last marker the audio plays at speed 1: where it plays now)
        assert_eq!(f, vec![(1000, 1100), (2000, 2100)]);
    }

    #[test]
    fn a_marker_dragged_onto_its_neighbour_pushes_it_to_the_next_transient() {
        let trs = [1000, 2000, 3000, 4000];
        let mut f = vec![(2000, 2000), (3000, 3000), (4000, 4000)];
        // the middle one dragged onto the first: the first jumps back to 1000
        cross_extend(&mut f, 1, 1990, &trs);
        assert_eq!(f[0], (1000, 1000));
        // dragged onto the last: no transient after it, it stays
        let mut g = vec![(2000, 2000), (3000, 3000), (4000, 4000)];
        cross_extend(&mut g, 1, 4000, &trs);
        assert_eq!(g[2], (4000, 4000));
    }
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
