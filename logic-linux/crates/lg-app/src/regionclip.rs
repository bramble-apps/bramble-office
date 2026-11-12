//! Copy, Cut and Paste of regions in the Tracks area, as Logic: every selected region is copied
//! (on several tracks too); Paste puts them at the playhead, the first on the selected track and
//! the others on the tracks below it as they were, keeping their distances. Audio regions keep
//! their file offset, gain, fades and flex (the audio editor's extras). One undo step.

use crate::app::App;
use crate::audioedit::RegionAudio;

#[derive(Clone, Debug)]
pub struct Clip {
    /// Tracks below the topmost copied region's.
    pub dt: usize,
    pub region: lg_core::Region,
    pub audio: Option<RegionAudio>,
}

/// The selected regions onto the clipboard; false with none selected.
pub fn copy(app: &mut App) -> bool {
    let mut sel = app.selected_regions.clone();
    if let Some(x) = app.selected_region.filter(|x| !sel.contains(x)) {
        sel.push(x);
    }
    sel.sort_unstable();
    let Some(top) = sel.first().map(|x| x.0) else { return false };
    let mut out = Vec::new();
    for &(t, r) in &sel {
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { continue };
        let audio = rg.audio.is_some().then(|| crate::audioedit::extras(app, t, r));
        out.push(Clip { dt: t - top, region: rg, audio });
    }
    // (the single-region clipboard others read: Live Loops, Paste Replace…)
    app.clipboard = out.first().map(|c| c.region.clone());
    app.region_clip = out;
    true
}

/// Where each clip lands: (track, start) for a paste at `at` with `track` selected.
pub fn places(clip: &[Clip], track: usize, at: u32, n_tracks: usize) -> Vec<Option<(usize, u32)>> {
    let first = clip.iter().map(|c| c.region.start).min().unwrap_or(0);
    clip.iter().map(|c| {
        let t = track + c.dt;
        (t < n_tracks).then(|| (t, at + (c.region.start - first)))
    }).collect()
}

/// The clipboard at the playhead on the selected track; false when there's nothing to paste.
pub fn paste(app: &mut App) -> bool {
    let (Some(track), false) = (app.selected_track, app.region_clip.is_empty()) else { return false };
    let clip = app.region_clip.clone();
    let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let at = app.position() as u32;
    crate::audioedit::checkpoint(app, "Paste");
    let mut pasted = Vec::new();
    for (c, place) in clip.iter().zip(places(&clip, track, at, n_tracks)) {
        let Some((t, start)) = place else { continue };
        let mut rg = c.region.clone();
        rg.origin = rg.source.or(rg.origin);
        rg.source = None;
        let d = start as i64 - rg.start as i64;
        rg.start = start;
        for n in &mut rg.notes {
            n.pos = (n.pos as i64 + d).max(0) as u32;
        }
        let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { continue };
        tr.regions.push(rg);
        let r = tr.regions.len() - 1;
        if let Some(x) = c.audio.clone() {
            crate::audioedit::set_extras(app, t, r, x);
        }
        pasted.push((t, r));
    }
    app.selected_region = pasted.first().copied();
    app.selected_regions = pasted;
    app.after_edit();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(dt: usize, start: u32) -> Clip {
        let mut region = crate::freeze::audio_region("x", start, 960, std::path::PathBuf::new(), 1.0);
        region.audio = None;
        Clip { dt, region, audio: None }
    }

    #[test]
    fn paste_keeps_tracks_and_distances() {
        let c = vec![clip(0, 3840), clip(1, 5760)];
        assert_eq!(places(&c, 1, 960, 4), vec![Some((1, 960)), Some((2, 2880))]);
        // (a track past the last: that region is left out)
        assert_eq!(places(&c, 3, 0, 4), vec![Some((3, 0)), None]);
    }

    #[test]
    fn copy_and_paste_every_selected_region_with_one_undo() {
        let mut a = crate::tm::test_app();
        let n0: Vec<usize> = a.project.as_ref().unwrap().tracks.iter().map(|t| t.regions.len()).collect();
        a.selected_regions = vec![(0, 0), (1, 0)];
        a.selected_region = Some((1, 0));
        assert!(copy(&mut a));
        assert_eq!(a.region_clip.len(), 2);
        a.selected_track = Some(0);
        let undo = a.undo.len();
        assert!(paste(&mut a));
        assert_eq!(a.undo.len(), undo + 1);
        let n1: Vec<usize> = a.project.as_ref().unwrap().tracks.iter().map(|t| t.regions.len()).collect();
        assert_eq!((n1[0], n1[1]), (n0[0] + 1, n0[1] + 1));
        assert_eq!(a.selected_regions.len(), 2);
    }
}
