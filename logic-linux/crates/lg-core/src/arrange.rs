//! The Tracks area's structure, as Logic's: track stacks (Folder and Summing) and packed folders
//! kept as a flat track list with nesting depths (a stack's main track, then its subtracks one
//! level deeper), individual track zoom, hidden tracks, track alternatives, the Transposition
//! track, and stable track ids (so state kept by track index elsewhere can follow a reorder).
//!
//! None of this is written into Logic's file yet (the stack and alternative chunks are only
//! partly decoded: research DIFFS E10); a project read from Logic shows its stacks flattened.

use std::ops::Range;

use crate::project::{AutoLane, Region, Track, TrackKind};

/// Logic's track stacks, and the older packed folders (Functions ▸ Folder ▸ Pack Folder).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackKind {
    /// Folder Stack: the main track groups the subtracks (its fader scales theirs).
    Folder,
    /// Summing Stack: the subtracks play into the main track's aux.
    Summing,
    /// A packed folder: one folder region stands for the tracks inside (Go Into Folder shows them).
    PackedFolder,
}

impl StackKind {
    pub fn name(self) -> &'static str {
        match self {
            StackKind::Folder => "Folder Stack",
            StackKind::Summing => "Summing Stack",
            StackKind::PackedFolder => "Folder",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stack {
    pub kind: StackKind,
    /// The subtracks shown (the main track's disclosure triangle).
    pub open: bool,
}

/// An inactive track alternative: its name and the content it keeps.
#[derive(Clone, Debug, Default)]
pub struct Alternative {
    pub name: String,
    pub regions: Vec<Region>,
    pub automation: Vec<AutoLane>,
}

#[derive(Clone, Debug, Default)]
pub struct TrackExtra {
    /// A stable id (0: not given yet; `assign_ids`).
    pub id: u64,
    /// Individual track zoom: the row height as a factor of the area's (0 or 1: as the others).
    pub height: f32,
    /// Hidden (Hide Selected Track); shown again while Hide View is off.
    pub hidden: bool,
    /// This track is a stack's main track (or a packed folder).
    pub stack: Option<Stack>,
    /// Nesting: 0 at the top, a stack's subtracks one deeper than its main track.
    pub depth: u8,
    /// The active track alternative's name ("" until the first alternative is made), and the
    /// inactive ones.
    pub alt_name: String,
    pub alternatives: Vec<Alternative>,
}

impl Track {
    /// An empty track as Studio makes them (no channel object in the file yet).
    pub fn new(name: &str, kind: TrackKind) -> Track {
        Track { name: name.into(), kind, channel: 0, row: 0, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, automation: Vec::new(), plugins: Vec::new(), extra: TrackExtra::default() }
    }

    pub fn zoom(&self) -> f32 {
        if self.extra.height > 0.0 { self.extra.height } else { 1.0 }
    }
}

/// The subtracks of track `i` (empty unless it is a stack's main track): the tracks after it
/// nested deeper.
pub fn members(tracks: &[Track], i: usize) -> Range<usize> {
    let Some(t) = tracks.get(i) else { return i..i };
    if t.extra.stack.is_none() {
        return i + 1..i + 1;
    }
    let d = t.extra.depth;
    let end = (i + 1..tracks.len()).find(|&j| tracks[j].extra.depth <= d).unwrap_or(tracks.len());
    i + 1..end
}

/// Track `i` with its subtracks.
pub fn group(tracks: &[Track], i: usize) -> Range<usize> {
    i..members(tracks, i).end.max(i + 1).min(tracks.len().max(i + 1))
}

/// The stack track `i` sits in (its main track).
pub fn parent(tracks: &[Track], i: usize) -> Option<usize> {
    let d = tracks.get(i)?.extra.depth;
    if d == 0 {
        return None;
    }
    (0..i).rev().find(|&j| tracks[j].extra.depth < d && tracks[j].extra.stack.is_some())
}

/// Every stack main track above `i`, nearest first.
pub fn ancestors(tracks: &[Track], i: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut cur = i;
    while let Some(p) = parent(tracks, cur) {
        out.push(p);
        cur = p;
    }
    out
}

/// The tracks the Tracks area shows, in order: members of closed stacks and packed folders
/// left out, hidden tracks left out while Hide View is on; inside a packed folder (Go Into
/// Folder) only what it holds.
pub fn visible(tracks: &[Track], hide_view: bool, inside: Option<usize>) -> Vec<usize> {
    let range = match inside.filter(|&f| tracks.get(f).is_some_and(|t| t.extra.stack.is_some())) {
        Some(f) => members(tracks, f),
        None => 0..tracks.len(),
    };
    let mut out = Vec::new();
    let mut i = range.start;
    while i < range.end {
        let t = &tracks[i];
        if hide_view && t.extra.hidden {
            i = group(tracks, i).end;
            continue;
        }
        out.push(i);
        match t.extra.stack {
            Some(s) if !s.open || (s.kind == StackKind::PackedFolder && inside != Some(i)) => i = group(tracks, i).end,
            _ => i += 1,
        }
    }
    out
}

/// Gives every track an id (new tracks, and copies that took another's).
pub fn assign_ids(tracks: &mut [Track], next: &mut u64) {
    let mut seen = std::collections::HashSet::new();
    for t in tracks.iter_mut() {
        if t.extra.id == 0 || !seen.insert(t.extra.id) {
            *next += 1;
            t.extra.id = *next;
            seen.insert(t.extra.id);
        }
        *next = (*next).max(t.extra.id);
    }
}

/// Where each track that was at index k (ids `before`) is now (None: gone).
pub fn remap(before: &[u64], tracks: &[Track]) -> Vec<Option<usize>> {
    before.iter().map(|id| tracks.iter().position(|t| t.extra.id == *id && *id != 0)).collect()
}

/// The top-level tracks among `sel` (a selected track inside a selected stack goes with it).
fn roots(tracks: &[Track], sel: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = sel.iter().copied().filter(|&i| i < tracks.len()).collect();
    v.sort_unstable();
    v.dedup();
    v.iter().copied().filter(|&i| !ancestors(tracks, i).iter().any(|a| v.contains(a))).collect()
}

/// Moves the groups `from` (stack-aware, in order) so that they start where the first was, at
/// `depth`; returns the new order (new index -> old index).
fn gather(tracks: &mut Vec<Track>, groups: &[usize], depth: u8, insert_main: Option<Track>) -> Vec<Option<usize>> {
    let first = groups.first().copied().unwrap_or(0);
    let mut taken: Vec<(usize, Track)> = Vec::new();
    let mut rest: Vec<(usize, Track)> = Vec::new();
    let ranges: Vec<Range<usize>> = groups.iter().map(|&g| group(tracks, g)).collect();
    for (i, t) in std::mem::take(tracks).into_iter().enumerate() {
        if ranges.iter().any(|r| r.contains(&i)) {
            taken.push((i, t));
        } else {
            rest.push((i, t));
        }
    }
    // depths: each group root at `depth`, members keep their offset to their root
    let mut out_taken: Vec<(Option<usize>, Track)> = Vec::new();
    for r in &ranges {
        let root_depth = taken.iter().find(|(k, _)| *k == r.start).map(|x| x.1.extra.depth).unwrap_or(0) as i16;
        for (k, t) in taken.iter().filter(|(k, _)| r.contains(k)) {
            let mut t = t.clone();
            t.extra.depth = (t.extra.depth as i16 - root_depth + depth as i16).max(0) as u8;
            out_taken.push((Some(*k), t));
        }
    }
    if let Some(m) = insert_main {
        out_taken.insert(0, (None, m));
    }
    // the gathered tracks go in where the first group was
    let at = rest.iter().position(|(k, _)| *k > first).unwrap_or(rest.len());
    let mut order: Vec<Option<usize>> = Vec::new();
    for (k, t) in rest.drain(..at) {
        order.push(Some(k));
        tracks.push(t);
    }
    for (k, t) in out_taken {
        order.push(k);
        tracks.push(t);
    }
    for (k, t) in rest {
        order.push(Some(k));
        tracks.push(t);
    }
    order
}

/// Create Track Stack (or Pack Folder): a main track named `name` above the selected tracks,
/// which become its subtracks. Returns the main track's index and the new order (new index ->
/// old index; None for the main track).
pub fn create_stack(tracks: &mut Vec<Track>, sel: &[usize], kind: StackKind, name: &str) -> Option<(usize, Vec<Option<usize>>)> {
    let groups = roots(tracks, sel);
    let first = *groups.first()?;
    let depth = tracks[first].extra.depth;
    let mut main = Track::new(name, if tracks[first].kind == TrackKind::Audio { TrackKind::Audio } else { TrackKind::Instrument });
    main.extra.stack = Some(Stack { kind, open: kind != StackKind::PackedFolder });
    main.extra.depth = depth;
    let order = gather(tracks, &groups, depth + 1, Some(main));
    let at = order.iter().position(|o| o.is_none())?;
    Some((at, order))
}

/// Flatten Stack / Unpack Folder: the main track goes, its subtracks move up a level. Returns
/// the main track (None when `i` isn't one).
pub fn flatten(tracks: &mut Vec<Track>, i: usize) -> Option<Track> {
    tracks.get(i)?.extra.stack?;
    let m = members(tracks, i);
    for t in &mut tracks[m] {
        t.extra.depth = t.extra.depth.saturating_sub(1);
    }
    Some(tracks.remove(i))
}

/// Moves track `from` (with its subtracks) so it lands before the track that is now at index
/// `before` (tracks.len(): to the end). It takes the nesting of the track it lands before (a
/// drop between a stack's subtracks puts it in the stack). Returns the new order (new index ->
/// old index), or None when nothing moves.
pub fn move_group(tracks: &mut Vec<Track>, from: usize, before: usize) -> Option<Vec<usize>> {
    let g = group(tracks, from);
    if before >= g.start && before <= g.end || from >= tracks.len() {
        return None;
    }
    let depth = if before < tracks.len() {
        // (landing right after an open stack's main track: into the stack)
        tracks[before].extra.depth
    } else {
        0
    };
    let root_d = tracks[from].extra.depth as i16;
    let mut idx: Vec<usize> = (0..tracks.len()).filter(|i| !g.contains(i)).collect();
    let at = idx.iter().position(|&i| i >= before).unwrap_or(idx.len());
    let moved: Vec<usize> = g.clone().collect();
    idx.splice(at..at, moved.iter().copied());
    let old = std::mem::take(tracks);
    let mut slots: Vec<Option<Track>> = old.into_iter().map(Some).collect();
    for &k in &idx {
        let mut t = slots[k].take()?;
        if g.contains(&k) {
            t.extra.depth = (t.extra.depth as i16 - root_d + depth as i16).max(0) as u8;
        }
        tracks.push(t);
    }
    Some(idx)
}

/// The next free alternative name: A, B, C… (as Logic letters them).
fn next_alt_name(t: &Track) -> String {
    let used: Vec<&str> = std::iter::once(t.extra.alt_name.as_str()).chain(t.extra.alternatives.iter().map(|a| a.name.as_str())).collect();
    for k in 0..26u8 {
        let n = ((b'A' + k) as char).to_string();
        if !used.contains(&n.as_str()) {
            return n;
        }
    }
    format!("{}", used.len() + 1)
}

/// New Track Alternative (empty; `duplicate`: Duplicate Track Alternative, with the content):
/// what the track had becomes an inactive alternative.
pub fn new_alternative(t: &mut Track, duplicate: bool) {
    if t.extra.alt_name.is_empty() {
        t.extra.alt_name = "A".into();
    }
    let keep = Alternative { name: t.extra.alt_name.clone(), regions: t.regions.clone(), automation: t.automation.clone() };
    t.extra.alternatives.push(keep);
    t.extra.alt_name = next_alt_name(t);
    if !duplicate {
        t.regions.clear();
        t.automation.clear();
    }
}

/// Makes inactive alternative `k` the track's content (the current one becomes inactive).
pub fn switch_alternative(t: &mut Track, k: usize) {
    let Some(a) = t.extra.alternatives.get_mut(k) else { return };
    std::mem::swap(&mut a.regions, &mut t.regions);
    std::mem::swap(&mut a.automation, &mut t.automation);
    std::mem::swap(&mut a.name, &mut t.extra.alt_name);
}

/// The Transposition track's value at `tick` (semitones; 0 before the first point).
pub fn transposition_at(points: &[(u32, i8)], tick: u32) -> i8 {
    points.iter().take_while(|p| p.0 <= tick).last().map_or(0, |p| p.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks(n: usize) -> Vec<Track> {
        (0..n).map(|i| {
            let mut t = Track::new(&format!("T{i}"), TrackKind::Instrument);
            t.extra.id = i as u64 + 1;
            t
        }).collect()
    }

    fn names(t: &[Track]) -> Vec<String> {
        t.iter().map(|t| format!("{}{}", "-".repeat(t.extra.depth as usize), t.name)).collect()
    }

    #[test]
    fn stacks_gather_their_tracks_and_flatten_back() {
        let mut t = tracks(5);
        // T1 and T3 into a summing stack: the main track goes where T1 was, T3 follows T1
        let (at, order) = create_stack(&mut t, &[3, 1], StackKind::Summing, "Sum 1").unwrap();
        assert_eq!(at, 1);
        assert_eq!(names(&t), ["T0", "Sum 1", "-T1", "-T3", "T2", "T4"]);
        assert_eq!(order, vec![Some(0), None, Some(1), Some(3), Some(2), Some(4)]);
        assert_eq!(members(&t, 1), 2..4);
        assert_eq!(parent(&t, 3), Some(1));
        // closed: its subtracks aren't shown
        t[1].extra.stack.as_mut().unwrap().open = false;
        assert_eq!(visible(&t, true, None), vec![0, 1, 4, 5]);
        // a stack inside a stack keeps its subtracks one level deeper
        t[1].extra.stack.as_mut().unwrap().open = true;
        let (inner, _) = create_stack(&mut t, &[3], StackKind::Folder, "Inner").unwrap();
        assert_eq!(names(&t), ["T0", "Sum 1", "-T1", "-Inner", "--T3", "T2", "T4"]);
        assert_eq!(members(&t, 1), 2..5);
        assert_eq!(ancestors(&t, inner + 1), vec![inner, 1]);
        let main = flatten(&mut t, 1).unwrap();
        assert_eq!(main.name, "Sum 1");
        assert_eq!(names(&t), ["T0", "T1", "Inner", "-T3", "T2", "T4"]);
    }

    #[test]
    fn packed_folders_show_their_insides_only_when_entered() {
        let mut t = tracks(4);
        let (f, _) = create_stack(&mut t, &[1, 2], StackKind::PackedFolder, "Folder").unwrap();
        assert_eq!(visible(&t, true, None), vec![0, 1, 4]);
        assert_eq!(visible(&t, true, Some(f)), vec![2, 3]);
        t[2].extra.hidden = true;
        assert_eq!(visible(&t, true, Some(f)), vec![3]);
        assert_eq!(visible(&t, false, Some(f)), vec![2, 3]);
    }

    #[test]
    fn reorder_moves_whole_stacks_and_takes_the_landing_level() {
        let mut t = tracks(4);
        create_stack(&mut t, &[1, 2], StackKind::Folder, "Stack").unwrap();
        // ["T0", "Stack", "-T1", "-T2", "T3"]: the stack to the end
        let order = move_group(&mut t, 1, 5).unwrap();
        assert_eq!(names(&t), ["T0", "T3", "Stack", "-T1", "-T2"]);
        assert_eq!(order, vec![0, 4, 1, 2, 3]);
        // T0 dropped between the subtracks joins the stack
        move_group(&mut t, 0, 4).unwrap();
        assert_eq!(names(&t), ["T3", "Stack", "-T1", "-T0", "-T2"]);
        // into itself: nothing
        assert!(move_group(&mut t, 1, 3).is_none());
        let before: Vec<u64> = vec![1, 2, 3, 4, 5];
        let ids: Vec<u64> = t.iter().map(|x| x.extra.id).collect();
        assert_eq!(remap(&before, &t)[0], ids.iter().position(|&i| i == 1));
    }

    #[test]
    fn ids_are_unique_after_copies() {
        let mut t = tracks(2);
        let copy = t[0].clone();
        t.push(copy);
        t.push(Track::new("new", TrackKind::Audio));
        let mut next = 0;
        assign_ids(&mut t, &mut next);
        let mut ids: Vec<u64> = t.iter().map(|x| x.extra.id).collect();
        assert_eq!(ids[..2], [1, 2]);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4);
        assert!(!ids.contains(&0));
    }

    #[test]
    fn alternatives_keep_and_swap_the_content() {
        let mut t = Track::new("Bass", TrackKind::Instrument);
        t.regions.push(Region { name: "take".into(), start: 0, length: 3840, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default() });
        new_alternative(&mut t, false);
        assert_eq!(t.extra.alt_name, "B");
        assert!(t.regions.is_empty());
        assert_eq!(t.extra.alternatives[0].name, "A");
        new_alternative(&mut t, true);
        assert_eq!(t.extra.alt_name, "C");
        assert_eq!(t.extra.alternatives.len(), 2);
        switch_alternative(&mut t, 0);
        assert_eq!(t.extra.alt_name, "A");
        assert_eq!(t.regions.len(), 1);
        assert_eq!(t.extra.alternatives[0].name, "C");
    }

    #[test]
    fn transposition_holds_until_the_next_point() {
        let pts = [(0u32, 0i8), (3840, 2), (7680, -5)];
        assert_eq!(transposition_at(&pts, 100), 0);
        assert_eq!(transposition_at(&pts, 3840), 2);
        assert_eq!(transposition_at(&pts, 9000), -5);
        assert_eq!(transposition_at(&[], 9000), 0);
    }
}
