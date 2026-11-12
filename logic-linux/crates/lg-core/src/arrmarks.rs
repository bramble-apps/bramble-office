//! Arrangement markers as Logic edits them: each marker is a section (start, length, name) of
//! the project, and moving, copying, swapping, replacing or deleting a marker does the same to
//! the regions, markers and automation points inside it (unless the content connection is
//! suspended). Sections lie end to end; moving or deleting one closes the gap, dropping one
//! between two others pushes the later ones right. Regions on protected tracks stay put.

use crate::markers::Marker;
use crate::project::{Project, Region};

/// An arrangement marker: (start tick, length in ticks, name).
pub type Section = (u32, u32, String);

/// What lies inside one section, at positions relative to its start.
#[derive(Clone, Debug, Default)]
pub struct Content {
    /// (track, region) with the region shifted to start relative to the section.
    pub regions: Vec<(usize, Region)>,
    pub markers: Vec<Marker>,
    /// (track, lane, tick, value)
    pub points: Vec<(usize, usize, u32, u8)>,
}

/// One slot of a new layout: which old section's content goes there (None: an empty one),
/// with its length and name.
#[derive(Clone, Debug)]
pub struct Slot {
    pub from: Option<usize>,
    pub len: u32,
    pub name: String,
}

fn movable(p: &Project, t: usize) -> bool {
    !p.tracks[t].extra.set.protected
}

fn shift_region(rg: &mut Region, d: i64) {
    crate::regions::shift(rg, d);
}

/// Takes the content starting inside [a, b) out of the project (protected tracks left alone).
fn take(p: &mut Project, a: u32, b: u32) -> Content {
    let mut c = Content::default();
    for t in 0..p.tracks.len() {
        if !movable(p, t) {
            continue;
        }
        let tr = &mut p.tracks[t];
        let mut keep = Vec::new();
        for mut rg in std::mem::take(&mut tr.regions) {
            if rg.start >= a && rg.start < b {
                shift_region(&mut rg, -(a as i64));
                c.regions.push((t, rg));
            } else {
                keep.push(rg);
            }
        }
        tr.regions = keep;
        for (l, lane) in tr.automation.iter_mut().enumerate() {
            lane.points.retain(|&(tick, v)| {
                if tick >= a && tick < b {
                    c.points.push((t, l, tick - a, v));
                    false
                } else {
                    true
                }
            });
        }
    }
    p.markers.retain(|m| {
        if m.0 >= a && m.0 < b {
            let mut m = m.clone();
            m.0 -= a;
            c.markers.push(m);
            false
        } else {
            true
        }
    });
    c
}

fn put(p: &mut Project, c: &Content, at: u32) {
    for (t, rg) in &c.regions {
        let mut rg = rg.clone();
        shift_region(&mut rg, at as i64);
        if let Some(tr) = p.tracks.get_mut(*t) {
            tr.regions.push(rg);
        }
    }
    for &(t, l, tick, v) in &c.points {
        if let Some(lane) = p.tracks.get_mut(t).and_then(|tr| tr.automation.get_mut(l)) {
            lane.points.push((tick + at, v));
        }
    }
    for m in &c.markers {
        let mut m = m.clone();
        m.0 += at;
        p.markers.push(m);
    }
}

fn tidy(p: &mut Project) {
    for tr in &mut p.tracks {
        tr.regions.sort_by_key(|r| r.start);
        for lane in &mut tr.automation {
            lane.points.sort_by_key(|q| q.0);
        }
    }
    p.markers.sort_by_key(|m| m.0);
}

/// Lays the sections out again as `slots`, end to end from the first section's start, taking
/// each slot's content from the old section it names (`connected`: content follows; else only
/// the markers change). What lay after the old sections moves by the change in total length.
pub fn relayout(p: &mut Project, slots: &[Slot], connected: bool) {
    let old = p.arrangement.clone();
    let Some(first) = old.first().map(|s| s.0) else {
        p.arrangement = Vec::new();
        return;
    };
    let old_end = old.last().map_or(first, |s| s.0 + s.1);
    let new_total: u32 = slots.iter().map(|s| s.len).sum();
    if connected {
        let contents: Vec<Content> = old.iter().map(|s| take(p, s.0, s.0 + s.1)).collect();
        let after = take(p, old_end, u32::MAX);
        let mut at = first;
        for s in slots {
            if let Some(c) = s.from.and_then(|k| contents.get(k)) {
                put(p, c, at);
            }
            at += s.len;
        }
        put(p, &after, first + new_total);
        tidy(p);
    }
    let mut at = first;
    p.arrangement = slots
        .iter()
        .map(|s| {
            let sec = (at, s.len, s.name.clone());
            at += s.len;
            sec
        })
        .collect();
}

fn slots_of(p: &Project) -> Vec<Slot> {
    p.arrangement.iter().enumerate().map(|(k, s)| Slot { from: Some(k), len: s.1, name: s.2.clone() }).collect()
}

/// Moves the block `sel` (contiguous indices) so it lands before section `before` (an index
/// into the current list; `len` = after the last). Returns the block's new first index.
pub fn move_block(p: &mut Project, sel: std::ops::Range<usize>, before: usize, connected: bool) -> usize {
    let mut slots = slots_of(p);
    if sel.is_empty() || sel.end > slots.len() || (before >= sel.start && before <= sel.end) {
        return sel.start;
    }
    let block: Vec<Slot> = slots.drain(sel.clone()).collect();
    let at = if before > sel.end { before - block.len() } else { before };
    for (i, s) in block.into_iter().enumerate() {
        slots.insert(at + i, s);
    }
    relayout(p, &slots, connected);
    at
}

/// Copies the block `sel` to land before section `before`, pushing the later ones right.
pub fn copy_block(p: &mut Project, sel: std::ops::Range<usize>, before: usize, connected: bool) -> usize {
    let mut slots = slots_of(p);
    if sel.is_empty() || sel.end > slots.len() || before > slots.len() {
        return sel.start;
    }
    let block: Vec<Slot> = slots[sel].to_vec();
    for (i, s) in block.into_iter().enumerate() {
        slots.insert(before + i, s);
    }
    relayout(p, &slots, connected);
    before
}

/// Swaps section `a` with section `b` (a marker dropped onto another), with their content.
pub fn swap(p: &mut Project, a: usize, b: usize, connected: bool) {
    let mut slots = slots_of(p);
    if a >= slots.len() || b >= slots.len() || a == b {
        return;
    }
    slots.swap(a, b);
    relayout(p, &slots, connected);
}

/// Replaces section `target` with a copy of section `src` (Command-drag onto it); with
/// `keep_src` false the source goes too (a move).
pub fn replace(p: &mut Project, src: usize, target: usize, keep_src: bool, connected: bool) {
    let mut slots = slots_of(p);
    if src >= slots.len() || target >= slots.len() || src == target {
        return;
    }
    slots[target] = slots[src].clone();
    if !keep_src {
        slots.remove(src);
    }
    relayout(p, &slots, connected);
}

/// Deletes the content inside the sections `sel` (the first Delete); the markers stay.
pub fn clear_content(p: &mut Project, sel: std::ops::Range<usize>) -> bool {
    let mut any = false;
    for k in sel {
        if let Some(s) = p.arrangement.get(k).cloned() {
            let c = take(p, s.0, s.0 + s.1);
            any |= !(c.regions.is_empty() && c.markers.is_empty() && c.points.is_empty());
        }
    }
    any
}

/// Whether sections `sel` hold anything (regions, markers, automation).
pub fn has_content(p: &Project, sel: std::ops::Range<usize>) -> bool {
    let mut q = p.clone();
    clear_content(&mut q, sel)
}

/// Removes the markers `sel` (the second Delete), closing the gap.
pub fn delete(p: &mut Project, sel: std::ops::Range<usize>, connected: bool) {
    let mut slots = slots_of(p);
    if sel.end > slots.len() {
        return;
    }
    slots.drain(sel);
    if slots.is_empty() {
        p.arrangement.clear();
        return;
    }
    relayout(p, &slots, connected);
}

/// Splits the section under `tick` in two there (content stays where it is).
pub fn split(p: &mut Project, tick: u32) -> Option<usize> {
    let k = p.arrangement.iter().position(|s| tick > s.0 && tick < s.0 + s.1)?;
    let (start, len, name) = p.arrangement[k].clone();
    p.arrangement[k].1 = tick - start;
    p.arrangement.insert(k + 1, (tick, start + len - tick, name));
    Some(k + 1)
}

/// Drags the boundary between sections `k` and `k + 1` (the Junction tool) to `tick`:
/// both resize, no gap. Content stays.
pub fn junction(p: &mut Project, k: usize, tick: u32, min: u32) -> bool {
    if k + 1 >= p.arrangement.len() {
        return false;
    }
    let a = p.arrangement[k].0;
    let b_end = p.arrangement[k + 1].0 + p.arrangement[k + 1].1;
    let t = tick.clamp(a + min, b_end.saturating_sub(min).max(a + min));
    p.arrangement[k].1 = t - a;
    p.arrangement[k + 1] = (t, b_end - t, p.arrangement[k + 1].2.clone());
    true
}

/// Trims an outer edge (the Trim tool): the first section's start or the last one's end.
pub fn trim(p: &mut Project, k: usize, left: bool, tick: u32, min: u32) -> bool {
    let n = p.arrangement.len();
    let Some(s) = p.arrangement.get_mut(k) else { return false };
    let end = s.0 + s.1;
    if left && k == 0 {
        let t = tick.min(end.saturating_sub(min));
        *s = (t, end - t, s.2.clone());
        true
    } else if !left && k + 1 == n {
        s.1 = tick.max(s.0 + min) - s.0;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Track;

    fn region(start: u32, length: u32) -> Region {
        Region { name: format!("r{start}"), start, length, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
    }

    fn project() -> Project {
        let mut p = Project::default();
        let mut t = Track::new("T", crate::TrackKind::Instrument);
        // A at 0, B at 100, C at 300 (lengths 100, 200, 100); one region in each, one after
        t.regions = vec![region(0, 50), region(150, 50), region(300, 100), region(450, 10)];
        t.automation = vec![crate::project::AutoLane { param: 7, points: vec![(10, 1), (120, 2), (310, 3)], ..Default::default() }];
        p.tracks.push(t);
        p.arrangement = vec![(0, 100, "A".into()), (100, 200, "B".into()), (300, 100, "C".into())];
        p.markers = vec![Marker::new(160, "m")];
        p
    }

    fn starts(p: &Project) -> Vec<u32> {
        p.tracks[0].regions.iter().map(|r| r.start).collect()
    }

    #[test]
    fn moving_a_section_moves_its_content_and_closes_the_gap() {
        let mut p = project();
        // C before A
        assert_eq!(move_block(&mut p, 2..3, 0, true), 0);
        assert_eq!(p.arrangement, vec![(0, 100, "C".into()), (100, 100, "A".into()), (200, 200, "B".into())]);
        assert_eq!(starts(&p), vec![0, 100, 250, 450]);
        assert_eq!(p.markers[0].0, 260);
        assert_eq!(p.tracks[0].automation[0].points, vec![(10, 3), (110, 1), (220, 2)]);
    }

    #[test]
    fn swap_copy_replace_delete() {
        let mut p = project();
        swap(&mut p, 0, 2, true);
        assert_eq!(p.arrangement.iter().map(|s| s.2.as_str()).collect::<Vec<_>>(), vec!["C", "B", "A"]);
        assert_eq!(starts(&p), vec![0, 150, 300, 450]);

        let mut p = project();
        copy_block(&mut p, 0..1, 3, true);
        assert_eq!(p.arrangement.len(), 4);
        assert_eq!(p.arrangement[3], (400, 100, "A".into()));
        // copied region, and the one after the sections pushed right by 100
        assert_eq!(starts(&p), vec![0, 150, 300, 400, 550]);

        let mut p = project();
        replace(&mut p, 2, 0, true, true);
        assert_eq!(p.arrangement[0], (0, 100, "C".into()));
        assert_eq!(starts(&p), vec![0, 150, 300, 450]);

        let mut p = project();
        assert!(has_content(&p, 1..2));
        assert!(clear_content(&mut p, 1..2));
        assert_eq!(p.arrangement.len(), 3);
        assert!(p.markers.is_empty());
        assert!(!has_content(&p, 1..2));
        delete(&mut p, 1..2, true);
        assert_eq!(p.arrangement, vec![(0, 100, "A".into()), (100, 100, "C".into())]);
        assert_eq!(starts(&p), vec![0, 100, 250]);
    }

    #[test]
    fn suspended_connection_and_protected_tracks_leave_content() {
        let mut p = project();
        move_block(&mut p, 2..3, 0, false);
        assert_eq!(p.arrangement[0].2, "C");
        assert_eq!(starts(&p), vec![0, 150, 300, 450]);
        let mut p = project();
        p.tracks[0].extra.set.protected = true;
        move_block(&mut p, 2..3, 0, true);
        assert_eq!(starts(&p), vec![0, 150, 300, 450]);
    }

    #[test]
    fn split_junction_trim() {
        let mut p = project();
        assert_eq!(split(&mut p, 200), Some(2));
        assert_eq!(p.arrangement[1], (100, 100, "B".into()));
        assert_eq!(p.arrangement[2], (200, 100, "B".into()));
        assert!(junction(&mut p, 0, 60, 10));
        assert_eq!((p.arrangement[0].1, p.arrangement[1].0, p.arrangement[1].1), (60, 60, 140));
        assert!(trim(&mut p, 3, false, 500, 10));
        assert_eq!(p.arrangement[3], (300, 200, "C".into()));
        assert!(!trim(&mut p, 1, false, 500, 10));
    }
}
