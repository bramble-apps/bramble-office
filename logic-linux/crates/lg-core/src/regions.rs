//! Arranging regions as Logic does: MIDI events other than notes kept in a region (controllers,
//! pitch bend, aftertouch, poly pressure, program changes), aliases of MIDI regions and clones
//! of audio regions, the Tracks area's drag modes (Overlap, No Overlap, X-Fade, Shuffle R / L)
//! and snap values, and the channel strip's MIDI FX slots (their units run in lg-dsp midifx).

use crate::events::{Control, PPQ};
use crate::project::{Region, Track};

/// A region's extras.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegionExt {
    /// Controller, pitch bend (number = LSB, value = MSB), channel / poly pressure and program
    /// change events, at ticks from the region's start (they move with the region).
    pub events: Vec<Control>,
    /// An id given to a region that has aliases or clones (0: none).
    pub id: u64,
    /// This region is an alias (MIDI) or a clone (audio) of the region with that id.
    pub alias_of: u64,
    /// The original's name as last seen (an alias shows it under its own).
    pub parent_name: String,
    /// Region automation of channel and plug-in parameters (ticks from the region's start):
    /// it moves and copies with the region and plays over the track's own automation.
    pub automation: Vec<crate::project::AutoLane>,
    /// Notes' articulation IDs: (ticks from the region's start, pitch, ID 1-255); notes not
    /// listed have none. (Studio keeps them here; not yet written into Logic's file.)
    pub artics: Vec<(u32, u8, u8)>,
    /// Events locked with Lock SMPTE Position in the Event List: (ticks from the region's
    /// start, status nibble 0x90 for notes, pitch / number).
    pub locked: Vec<(u32, u8, u8)>,
}

/// One MIDI FX slot of a channel strip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiFxSlot {
    pub name: String,
    pub values: Vec<f32>,
    pub bypass: bool,
}

/// The kinds of non-note events, as Logic's Event List names them.
pub fn event_kind_name(kind: u8) -> &'static str {
    match kind & 0xF0 {
        0xA0 => "Poly Aftertouch",
        0xB0 => "Control",
        0xC0 => "Program",
        0xD0 => "Aftertouch",
        0xE0 => "Pitch Bend",
        _ => "Event",
    }
}

/// A pitch bend event's 14-bit value as -8192..8191.
pub fn bend_value(c: &Control) -> i16 {
    (((c.value as i32) << 7 | c.number as i32) - 8192) as i16
}

pub fn bend_event(pos: u32, channel: u8, v: i16) -> Control {
    let raw = (v as i32 + 8192).clamp(0, 16383) as u16;
    Control { kind: 0xE0, channel, pos, number: (raw & 0x7F) as u8, value: (raw >> 7) as u8 }
}

/// The value a lane of `kind` / `number` shows for an event (0..127; pitch bend scaled).
pub fn lane_value(c: &Control) -> f32 {
    match c.kind & 0xF0 {
        0xE0 => (bend_value(c) as f32 + 8192.0) / 128.0,
        0xC0 | 0xD0 => c.number as f32,
        _ => c.value as f32,
    }
}

/// An event for a lane at `pos` with value 0..127.
pub fn lane_event(kind: u8, number: u8, channel: u8, pos: u32, v: f32) -> Control {
    let v = v.clamp(0.0, 127.0);
    match kind & 0xF0 {
        0xE0 => bend_event(pos, channel, (v * 128.0).round() as i16 - 8192),
        // program and channel pressure keep their value in the first data byte
        0xC0 | 0xD0 => Control { kind: kind & 0xF0, channel, pos, number: v.round() as u8, value: 0 },
        _ => Control { kind: kind & 0xF0, channel, pos, number, value: v.round() as u8 },
    }
}

/// Does `c` belong to the lane (kind, controller number)?
pub fn on_lane(c: &Control, kind: u8, number: u8) -> bool {
    c.kind & 0xF0 == kind & 0xF0 && (kind & 0xF0 != 0xB0 && kind & 0xF0 != 0xA0 || c.number == number)
}

// ---------------------------------------------------------------------------------------------
// aliases and clones

/// Gives `rg` an id if it has none; returns it.
pub fn ensure_id(rg: &mut Region, next: &mut u64) -> u64 {
    if rg.ext.id == 0 {
        *next += 1;
        rg.ext.id = *next;
    }
    rg.ext.id
}

/// The highest region id in use.
pub fn max_id(tracks: &[Track]) -> u64 {
    tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.ext.id.max(r.ext.alias_of)).max().unwrap_or(0)
}

/// An alias (MIDI) or clone (audio) of `orig` at `start`: same contents, its own name and
/// region parameters (Logic: Option-Shift-drag, Make Alias, Repeat as Aliases).
pub fn make_alias(orig: &mut Region, start: u32, next: &mut u64) -> Region {
    let id = ensure_id(orig, next);
    let mut a = orig.clone();
    let d = start as i64 - orig.start as i64;
    a.start = start;
    for n in &mut a.notes {
        n.pos = (n.pos as i64 + d).max(0) as u32;
    }
    a.source = None;
    a.origin = orig.source.or(orig.origin);
    a.ext.id = 0;
    a.ext.alias_of = id;
    a.ext.parent_name = orig.name.clone();
    a
}

pub fn is_alias(rg: &Region) -> bool {
    rg.ext.alias_of != 0
}

/// Where the original of an alias is (track, region).
pub fn original_of(tracks: &[Track], id: u64) -> Option<(usize, usize)> {
    if id == 0 {
        return None;
    }
    tracks.iter().enumerate().find_map(|(t, tr)| tr.regions.iter().position(|r| r.ext.id == id).map(|k| (t, k)))
}

/// The aliases of the region with `id` (track, region).
pub fn aliases_of(tracks: &[Track], id: u64) -> Vec<(usize, usize)> {
    if id == 0 {
        return Vec::new();
    }
    tracks.iter().enumerate().flat_map(|(t, tr)| tr.regions.iter().enumerate().filter(|(_, r)| r.ext.alias_of == id).map(move |(k, _)| (t, k))).collect()
}

/// A region split `at` ticks from its start: the events before stay in `a`, the rest move to
/// `b` (from its own start); a split alias's pieces are aliases no more, an original's second
/// piece isn't the original.
pub fn split_events(a: &mut RegionExt, b: &mut RegionExt, at: u32) {
    let all = std::mem::take(&mut a.events);
    a.events = all.iter().copied().filter(|e| e.pos < at).collect();
    b.events = all.iter().filter(|e| e.pos >= at).map(|e| Control { pos: e.pos - at, ..*e }).collect();
    b.automation = a.automation.iter_mut().map(|l| crate::automation::split_lane(l, at)).filter(|l| !l.points.is_empty()).collect();
    b.id = 0;
    for x in [a, b] {
        x.alias_of = 0;
        x.parent_name.clear();
    }
}

/// Every alias follows its original: notes and events (moved to the alias's position), the
/// parent's name; a clone takes its original's window in the audio file (file, offset,
/// length). Orphans (original deleted with Keep) stay as they are. Returns whether anything
/// changed.
pub fn sync_aliases(tracks: &mut [Track]) -> bool {
    // (a copy of an original isn't the original: only the first keeps the id)
    let mut seen = std::collections::HashSet::new();
    for r in tracks.iter_mut().flat_map(|t| t.regions.iter_mut()) {
        if r.ext.id != 0 && !seen.insert(r.ext.id) {
            r.ext.id = 0;
        }
    }
    let originals: Vec<(u64, Region)> = tracks.iter().flat_map(|t| t.regions.iter()).filter(|r| r.ext.id != 0).map(|r| (r.ext.id, r.clone())).collect();
    if originals.is_empty() {
        return false;
    }
    let mut changed = false;
    for tr in tracks.iter_mut() {
        for a in tr.regions.iter_mut().filter(|r| r.ext.alias_of != 0) {
            let Some((_, o)) = originals.iter().find(|(id, _)| *id == a.ext.alias_of) else { continue };
            let d = a.start as i64 - o.start as i64;
            let notes: Vec<_> = o.notes.iter().map(|n| crate::events::Note { pos: (n.pos as i64 + d).max(0) as u32, ..*n }).collect();
            if a.notes != notes {
                a.notes = notes;
                changed = true;
            }
            if a.ext.events != o.ext.events {
                a.ext.events = o.ext.events.clone();
                changed = true;
            }
            if a.ext.parent_name != o.name {
                a.ext.parent_name = o.name.clone();
                changed = true;
            }
            if o.audio.is_some() {
                let (file, off) = (o.audio.as_ref().map(|x| x.file.clone()), o.props.audio_offset);
                if a.audio.as_ref().map(|x| x.file.clone()) != file || a.props.audio_offset != off || a.length != o.length {
                    a.audio = o.audio.clone();
                    a.props.audio_offset = off;
                    a.length = o.length;
                    changed = true;
                }
            } else if a.length == 0 {
                a.length = o.length;
            }
        }
    }
    changed
}

/// An alias made into a real copy (Convert to Region Copy): the original's contents (already
/// mirrored) and the alias's own parameters.
pub fn convert_to_copy(rg: &mut Region) {
    rg.ext.alias_of = 0;
    rg.ext.parent_name.clear();
}

/// Deleting an original that has aliases: Convert makes the aliases real copies; Keep leaves
/// them as orphans.
pub fn before_delete_original(tracks: &mut [Track], id: u64, convert: bool) {
    if convert {
        for (t, k) in aliases_of(tracks, id) {
            convert_to_copy(&mut tracks[t].regions[k]);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// drag modes

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DragMode {
    Overlap,
    #[default]
    NoOverlap,
    XFade,
    ShuffleR,
    ShuffleL,
}

impl DragMode {
    pub const ALL: [DragMode; 5] = [DragMode::Overlap, DragMode::NoOverlap, DragMode::XFade, DragMode::ShuffleR, DragMode::ShuffleL];
    pub fn name(self) -> &'static str {
        match self {
            DragMode::Overlap => "Overlap",
            DragMode::NoOverlap => "No Overlap",
            DragMode::XFade => "X-Fade",
            DragMode::ShuffleR => "Shuffle R",
            DragMode::ShuffleL => "Shuffle L",
        }
    }
}

/// Moves a region by `d` ticks, its notes with it.
pub fn shift(rg: &mut Region, d: i64) {
    let d = d.max(-(rg.start as i64));
    rg.start = (rg.start as i64 + d) as u32;
    for n in &mut rg.notes {
        n.pos = (n.pos as i64 + d).max(0) as u32;
    }
    if rg.props.loop_to > 0 {
        rg.props.loop_to = (rg.props.loop_to as i64 + d).max(0) as u32;
    }
}

fn end(r: &Region) -> u32 {
    r.start + r.length
}

/// After region `k` on a track was moved (or made) in `mode`: the other regions as Logic's
/// drag mode leaves them. `ms_per_tick` sizes X-Fade's fades. Returns the index `k` now has
/// (regions are kept sorted by start).
pub fn after_move(regions: &mut Vec<Region>, k: usize, mode: DragMode, ms_per_tick: f64) -> usize {
    if k >= regions.len() {
        return k;
    }
    match mode {
        DragMode::Overlap => {}
        DragMode::NoOverlap => no_overlap(regions, k),
        DragMode::XFade => {
            let audio = regions[k].audio.is_some();
            if audio {
                crossfade(regions, k, ms_per_tick);
            } else {
                no_overlap(regions, k);
            }
        }
        DragMode::ShuffleL | DragMode::ShuffleR => {
            let mut others: Vec<usize> = (0..regions.len()).filter(|&i| i != k).collect();
            others.sort_by_key(|&i| regions[i].start);
            let s = regions[k].start;
            if mode == DragMode::ShuffleL {
                // closes the gap to the region before it
                let before = others.iter().copied().filter(|&i| regions[i].start <= s).max_by_key(|&i| end(&regions[i]));
                let to = before.map_or(0, |i| end(&regions[i]));
                let d = to as i64 - s as i64;
                shift(&mut regions[k], d);
                // regions it now covers move right after it
                let mut at = end(&regions[k]);
                let later: Vec<usize> = others.into_iter().filter(|&i| regions[i].start >= to && Some(i) != before).collect();
                for i in later {
                    if regions[i].start < at {
                        let d = at as i64 - regions[i].start as i64;
                        shift(&mut regions[i], d);
                    }
                    at = end(&regions[i]);
                }
            } else {
                // closes the gap to the region after it
                let e = end(&regions[k]);
                let after = others.iter().copied().filter(|&i| end(&regions[i]) > s).min_by_key(|&i| regions[i].start);
                if let Some(a) = after {
                    let d = regions[a].start as i64 - e as i64;
                    if regions[a].start >= s {
                        shift(&mut regions[k], d);
                    }
                }
            }
        }
    }
    sort_keep(regions, k)
}

/// After region `k`'s right edge (`right`) or left edge changed length by `delta` ticks.
pub fn after_resize(regions: &mut Vec<Region>, k: usize, right: bool, delta: i64, mode: DragMode, ms_per_tick: f64) -> usize {
    if k >= regions.len() {
        return k;
    }
    match mode {
        DragMode::ShuffleL if right => {
            // the following regions move by the change in length
            let s = regions[k].start;
            for i in 0..regions.len() {
                if i != k && regions[i].start > s {
                    shift(&mut regions[i], delta);
                }
            }
        }
        DragMode::ShuffleR if !right => {
            // the preceding regions move by the change; not before the project start
            let s = regions[k].start;
            let before: Vec<usize> = (0..regions.len()).filter(|&i| i != k && regions[i].start < s + delta.max(0) as u32).collect();
            if before.iter().all(|&i| regions[i].start as i64 - delta >= 0) {
                for i in before {
                    shift(&mut regions[i], -delta);
                }
            }
        }
        DragMode::Overlap | DragMode::ShuffleL | DragMode::ShuffleR => {}
        _ => return after_move(regions, k, mode, ms_per_tick),
    }
    sort_keep(regions, k)
}

/// After region `k` is deleted in a Shuffle mode: the others close the gap. Call before
/// removing it.
pub fn before_delete(regions: &mut [Region], k: usize, mode: DragMode) {
    let Some(r) = regions.get(k) else { return };
    let (s, len) = (r.start, r.length as i64);
    for i in 0..regions.len() {
        if i == k {
            continue;
        }
        match mode {
            DragMode::ShuffleL if regions[i].start >= s => shift(&mut regions[i], -len),
            DragMode::ShuffleR if regions[i].start < s => shift(&mut regions[i], len),
            _ => {}
        }
    }
}

fn no_overlap(regions: &mut Vec<Region>, k: usize) {
    let (s, e) = (regions[k].start, end(&regions[k]));
    for i in 0..regions.len() {
        if i == k {
            continue;
        }
        let (os, oe) = (regions[i].start, end(&regions[i]));
        if os < s && oe > s {
            // the earlier region is shortened to where the moved one starts
            regions[i].length = s - os;
        } else if os > s && os < e {
            regions[k].length = os - s;
        }
    }
}

fn crossfade(regions: &mut [Region], k: usize, ms_per_tick: f64) {
    let (s, e) = (regions[k].start, end(&regions[k]));
    for i in 0..regions.len() {
        if i == k || regions[i].audio.is_none() {
            continue;
        }
        let (os, oe) = (regions[i].start, end(&regions[i]));
        let ms = |ticks: u32| ((ticks as f64 * ms_per_tick).round() as u32).min(u16::MAX as u32) as u16;
        if os < s && oe > s {
            let ov = oe.min(e) - s;
            regions[i].props.fade_out = (ms(ov), 0);
            regions[k].props.fade_in = (ms(ov), 0);
        } else if os > s && os < e {
            let ov = e.min(oe) - os;
            regions[k].props.fade_out = (ms(ov), 0);
            regions[i].props.fade_in = (ms(ov), 0);
        }
    }
}

fn sort_keep(regions: &mut Vec<Region>, k: usize) -> usize {
    let mut idx: Vec<usize> = (0..regions.len()).collect();
    idx.sort_by_key(|&i| (regions[i].start, i));
    let pos = idx.iter().position(|&i| i == k).unwrap_or(k);
    let old = std::mem::take(regions);
    let mut slots: Vec<Option<Region>> = old.into_iter().map(Some).collect();
    *regions = idx.into_iter().filter_map(|i| slots[i].take()).collect();
    pos
}

// ---------------------------------------------------------------------------------------------
// snap

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SnapMode {
    #[default]
    Smart,
    Bar,
    Beat,
    Division,
    Ticks,
    Frames,
    QuarterFrames,
    Samples,
}

impl SnapMode {
    pub const ALL: [SnapMode; 8] = [SnapMode::Smart, SnapMode::Bar, SnapMode::Beat, SnapMode::Division, SnapMode::Ticks, SnapMode::Frames, SnapMode::QuarterFrames, SnapMode::Samples];
    pub fn name(self) -> &'static str {
        match self {
            SnapMode::Smart => "Smart",
            SnapMode::Bar => "Bar",
            SnapMode::Beat => "Beat",
            SnapMode::Division => "Division",
            SnapMode::Ticks => "Ticks",
            SnapMode::Frames => "Frames",
            SnapMode::QuarterFrames => "Quarter Frames",
            SnapMode::Samples => "Samples",
        }
    }
}

/// The grid a snap mode uses, in ticks: bar and beat from the signature, the division (ticks),
/// frames at 30 fps; Smart picks the finest line at least `min_px` apart at `px_per_tick`.
pub fn snap_grid(mode: SnapMode, sig: (u8, u8), division: u32, bpm: f64, px_per_tick: f64, min_px: f64) -> f64 {
    let beat = PPQ as f64 * 4.0 / sig.1.max(1) as f64;
    let bar = beat * sig.0.max(1) as f64;
    let frame = PPQ as f64 * bpm / 60.0 / 30.0;
    match mode {
        SnapMode::Bar => bar,
        SnapMode::Beat => beat,
        SnapMode::Division => division.max(1) as f64,
        SnapMode::Ticks => 1.0,
        SnapMode::Frames => frame,
        SnapMode::QuarterFrames => frame / 4.0,
        SnapMode::Samples => PPQ as f64 * bpm / 60.0 / 48000.0,
        SnapMode::Smart => {
            let mut g = bar;
            for c in [beat, division.max(1) as f64, 1.0] {
                if c * px_per_tick >= min_px {
                    g = c;
                }
            }
            g
        }
    }
}

/// Snaps a region drag: Absolute puts the region's start on the grid; Relative keeps its
/// distance from the grid and moves by whole grid steps.
pub fn snap_move(start: u32, delta: f64, grid: f64, relative: bool) -> u32 {
    let g = grid.max(1.0);
    let to = if relative { start as f64 + (delta / g).round() * g } else { ((start as f64 + delta) / g).round() * g };
    to.max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Note;

    fn rg(start: u32, len: u32) -> Region {
        Region { name: format!("r{start}"), start, length: len, notes: vec![Note { channel: 0, pitch: 60, velocity: 90, pos: start + 10, len: 100, muted: false }], audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
    }

    fn track(regions: Vec<Region>) -> Track {
        let mut t = Track::new("T", crate::TrackKind::Instrument);
        t.regions = regions;
        t
    }

    #[test]
    fn aliases_follow_the_original() {
        let mut next = 0;
        let mut o = rg(0, 3840);
        let a = make_alias(&mut o, 7680, &mut next);
        assert_eq!(a.notes[0].pos, 7690);
        let mut tracks = vec![track(vec![o]), track(vec![a])];
        // the original is edited: the alias follows, at its own place
        tracks[0].regions[0].notes.push(Note { channel: 0, pitch: 64, velocity: 80, pos: 500, len: 10, muted: false });
        tracks[0].regions[0].ext.events.push(Control { kind: 0xB0, channel: 0, pos: 20, number: 1, value: 64 });
        tracks[0].regions[0].name = "Verse".into();
        assert!(sync_aliases(&mut tracks));
        let a = &tracks[1].regions[0];
        assert_eq!(a.notes.iter().map(|n| n.pos).collect::<Vec<_>>(), vec![7690, 8180]);
        assert_eq!(a.ext.events.len(), 1);
        assert_eq!(a.ext.parent_name, "Verse");
        let of = a.ext.alias_of;
        assert!(!sync_aliases(&mut tracks));
        assert_eq!(original_of(&tracks, of), Some((0, 0)));
        assert_eq!(aliases_of(&tracks, tracks[0].regions[0].ext.id), vec![(1, 0)]);
        // a real copy no longer follows
        convert_to_copy(&mut tracks[1].regions[0]);
        tracks[0].regions[0].notes.clear();
        sync_aliases(&mut tracks);
        assert_eq!(tracks[1].regions[0].notes.len(), 2);
    }

    #[test]
    fn no_overlap_shortens_the_earlier_region() {
        let mut v = vec![rg(0, 3840), rg(1920, 3840)];
        let k = after_move(&mut v, 1, DragMode::NoOverlap, 0.5);
        assert_eq!((v[0].length, k), (1920, 1));
        let mut v = vec![rg(0, 3840), rg(1920, 3840)];
        after_move(&mut v, 0, DragMode::Overlap, 0.5);
        assert_eq!(v[0].length, 3840);
        // the moved region is the earlier one: it's shortened
        let mut v = vec![rg(0, 3840), rg(1920, 3840)];
        after_move(&mut v, 0, DragMode::NoOverlap, 0.5);
        assert_eq!(v[0].length, 1920);
    }

    #[test]
    fn xfade_sets_fades_on_audio() {
        let mut a = rg(0, 3840);
        a.audio = Some(crate::project::AudioRef { file: "a.wav".into(), seconds: 2.0 });
        let mut b = a.clone();
        b.start = 2880;
        let mut v = vec![a, b];
        after_move(&mut v, 1, DragMode::XFade, 0.5);
        assert_eq!(v[0].props.fade_out.0, 480);
        assert_eq!(v[1].props.fade_in.0, 480);
        assert_eq!(v[0].length, 3840);
    }

    #[test]
    fn shuffle_modes_close_gaps() {
        // Shuffle L: a region moved left sits right after the one before it
        let mut v = vec![rg(0, 1920), rg(5000, 1920)];
        after_move(&mut v, 1, DragMode::ShuffleL, 0.5);
        assert_eq!(v[1].start, 1920);
        assert_eq!(v[1].notes[0].pos, 1930);
        // Shuffle R: moved right, it ends where the next begins
        let mut v = vec![rg(100, 1920), rg(7680, 1920)];
        after_move(&mut v, 0, DragMode::ShuffleR, 0.5);
        assert_eq!(v[0].start, 7680 - 1920);
        // Shuffle L: lengthening the right edge pushes the following regions
        let mut v = vec![rg(0, 2000), rg(1920, 1920)];
        after_resize(&mut v, 0, true, 80, DragMode::ShuffleL, 0.5);
        assert_eq!(v[1].start, 2000);
        // Shuffle R: lengthening the left edge pushes the preceding ones left
        let mut v = vec![rg(1000, 1920), rg(2920, 1920)];
        v[1].start = 2820;
        after_resize(&mut v, 1, false, 100, DragMode::ShuffleR, 0.5);
        assert_eq!(v[0].start, 900);
        // delete: the following regions close up
        let mut v = vec![rg(0, 1920), rg(1920, 1920), rg(3840, 1920)];
        before_delete(&mut v, 1, DragMode::ShuffleL);
        v.remove(1);
        assert_eq!(v[1].start, 1920);
    }

    #[test]
    fn snap_values() {
        assert_eq!(snap_grid(SnapMode::Bar, (3, 4), 240, 120.0, 0.1, 8.0), 2880.0);
        assert_eq!(snap_grid(SnapMode::Beat, (6, 8), 240, 120.0, 0.1, 8.0), 480.0);
        assert_eq!(snap_grid(SnapMode::Frames, (4, 4), 240, 120.0, 0.1, 8.0), 64.0);
        // Smart: finer as you zoom in
        assert_eq!(snap_grid(SnapMode::Smart, (4, 4), 240, 120.0, 0.005, 8.0), 3840.0);
        assert_eq!(snap_grid(SnapMode::Smart, (4, 4), 240, 120.0, 0.05, 8.0), 240.0);
        assert_eq!(snap_move(130, 1000.0, 960.0, false), 960);
        assert_eq!(snap_move(130, 1000.0, 960.0, true), 1090);
    }

    #[test]
    fn lanes_hold_controllers_and_bend() {
        let b = bend_event(0, 0, -8192);
        assert_eq!(bend_value(&b), -8192);
        assert_eq!(lane_value(&bend_event(0, 0, 0)), 64.0);
        let e = lane_event(0xB0, 7, 0, 10, 100.0);
        assert!(on_lane(&e, 0xB0, 7) && !on_lane(&e, 0xB0, 1));
        assert!(on_lane(&lane_event(0xE0, 0, 0, 0, 3.0), 0xE0, 99));
    }
}
