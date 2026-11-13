//! Tracks area region edits: Edit ▸ Delete and Move (the regions after a deleted one move up to
//! close the gap), Edit ▸ Convert ▸ Loops to Regions (a looped MIDI region's repetitions become
//! regions) and Repetitions to Loops (adjacent identical copies of a MIDI region become its
//! loop, so edits to it carry over), Edit ▸ Move ▸ To Selected Track.

use lg_core::Region;

use crate::app::App;

pub const COMMANDS: &[&str] = &["Delete and Move", "Convert Loops to Regions", "Convert Repetitions to Loops", "Loops to Regions", "Repetitions to Loops", "To Selected Track"];

fn shift(rg: &mut Region, d: i64) {
    rg.start = (rg.start as i64 + d).max(0) as u32;
    for n in &mut rg.notes {
        n.pos = (n.pos as i64 + d).max(0) as u32;
    }
}

/// Deletes regions `idx` of a track; every region after one moves left by its length.
pub fn delete_and_move(regions: &mut Vec<Region>, idx: &[usize]) {
    let mut v: Vec<usize> = idx.iter().copied().filter(|&i| i < regions.len()).collect();
    v.sort_unstable_by_key(|&i| std::cmp::Reverse(regions[i].start));
    v.dedup();
    for i in v {
        let gone = regions.remove(i);
        let end = gone.start + gone.length;
        for r in regions.iter_mut().filter(|r| r.start >= end) {
            shift(r, -(gone.length as i64));
        }
    }
}

/// Where a looped region's repetitions end: its dragged loop end, the next region, or `end`.
pub fn loop_end(regions: &[Region], i: usize, end: u32) -> u32 {
    let r = &regions[i];
    if r.props.loop_to > 0 {
        return r.props.loop_to;
    }
    let stop = r.start + r.length;
    regions.iter().map(|x| x.start).filter(|&s| s >= stop).min().unwrap_or(end.max(stop))
}

/// Loops to Regions: each looped MIDI region's repetitions become regions (the last one cut at
/// the loop's end); the loops go off. The new regions' indices.
pub fn loops_to_regions(regions: &mut Vec<Region>, idx: &[usize], end: u32) -> Vec<usize> {
    let mut made = Vec::new();
    for &i in idx {
        let Some(r) = regions.get(i) else { continue };
        if !r.props.looped || r.audio.is_some() || r.length == 0 {
            continue;
        }
        let to = loop_end(regions, i, end);
        let base = regions[i].clone();
        let mut at = base.start + base.length;
        while at < to {
            let mut c = base.clone();
            shift(&mut c, (at - base.start) as i64);
            c.props.looped = false;
            c.props.loop_to = 0;
            c.source = None;
            c.origin = base.source.clone().or(base.origin.clone());
            if at + c.length > to {
                c.length = to - at;
                let stop = to;
                c.notes.retain(|n| n.pos < stop);
                for n in &mut c.notes {
                    n.len = n.len.min(stop - n.pos);
                }
                let rel = c.length;
                c.ext.events.retain(|e| e.pos < rel);
            }
            regions.push(c);
            made.push(regions.len() - 1);
            at += base.length;
        }
        regions[i].props.looped = false;
        regions[i].props.loop_to = 0;
    }
    made
}

/// The region's contents relative to its start (to find identical repetitions).
fn content(r: &Region) -> (u32, Vec<(u32, u32, u8, u8, u8, bool)>, Vec<(u32, u8, u8, u8, u8)>) {
    (
        r.length,
        r.notes.iter().map(|n| (n.pos - r.start.min(n.pos), n.len, n.pitch, n.velocity, n.channel, n.muted)).collect(),
        r.ext.events.iter().map(|e| (e.pos, e.kind, e.channel, e.number, e.value)).collect(),
    )
}

/// Repetitions to Loops: a MIDI region followed directly by identical copies keeps only itself,
/// looped over them. Returns how many copies became the loop.
pub fn repetitions_to_loops(regions: &mut Vec<Region>, i: usize) -> usize {
    let Some(r) = regions.get(i) else { return 0 };
    if r.audio.is_some() {
        return 0;
    }
    let want = content(r);
    let mut end = r.start + r.length;
    let mut copies = Vec::new();
    while let Some(j) = (0..regions.len()).find(|&j| j != i && regions[j].start == end && regions[j].audio.is_none() && content(&regions[j]) == want) {
        copies.push(j);
        end += regions[j].length;
    }
    if copies.is_empty() {
        return 0;
    }
    let n = copies.len();
    let next = regions.iter().map(|x| x.start).filter(|&s| s >= end).min();
    regions[i].props.looped = true;
    // (to the next region the loop would reach anyway: no explicit end)
    regions[i].props.loop_to = if next == Some(end) { 0 } else { end };
    copies.sort_unstable_by(|a, b| b.cmp(a));
    for j in copies {
        regions.remove(j);
    }
    n
}

/// Option-Shift resize: every selected region takes region (t, r)'s length (audio regions not
/// past their file's end).
pub fn same_length(app: &mut App, t: usize, r: usize) {
    let Some(len) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|x| x.length) else { return };
    for (tt, rr) in app.region_selection() {
        if (tt, rr) == (t, r) {
            continue;
        }
        let limit = crate::regiontrim::end_limit(app, tt, rr);
        if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(tt)?.regions.get_mut(rr)) {
            let max = limit.map_or(u32::MAX, |m| (m - x.start as f64).round().max(1.0) as u32);
            x.length = len.min(max);
        }
    }
    app.after_edit();
}

/// Names for `n` regions renamed at once: a name ending in a number counts up from it.
pub fn numbered_names(name: &str, n: usize) -> Vec<String> {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || n < 2 {
        return vec![name.to_string(); n];
    }
    let (stem, num) = name.split_at(name.len() - digits);
    let first: u64 = num.parse().unwrap_or(0);
    (0..n as u64).map(|k| format!("{stem}{:0w$}", first + k, w = digits)).collect()
}

/// Renames regions (in time order, then track order) with `numbered_names`.
pub fn rename_all(app: &mut App, sel: &[(usize, usize)], name: &str) {
    let Some(p) = app.project.as_mut() else { return };
    let mut v: Vec<(usize, usize)> = sel.iter().copied().filter(|&(t, r)| p.tracks.get(t).is_some_and(|x| r < x.regions.len())).collect();
    v.sort_by_key(|&(t, r)| (p.tracks[t].regions[r].start, t));
    for ((t, r), n) in v.iter().zip(numbered_names(name, v.len())) {
        p.tracks[*t].regions[*r].name = n;
    }
}

pub fn command(app: &mut App, name: &str) -> bool {
    if !COMMANDS.contains(&name) || !crate::focus::tracks(app) && name != "Convert Loops to Regions" && name != "Convert Repetitions to Loops" {
        return false;
    }
    let sel = app.region_selection();
    if sel.is_empty() {
        return false;
    }
    let end = crate::projend::end(app);
    match name {
        "Delete and Move" => {
            app.checkpoint("Delete and Move");
            if let Some(p) = app.project.as_mut() {
                for t in 0..p.tracks.len() {
                    let idx: Vec<usize> = sel.iter().filter(|x| x.0 == t).map(|x| x.1).collect();
                    if !idx.is_empty() {
                        delete_and_move(&mut p.tracks[t].regions, &idx);
                    }
                }
            }
            app.select_regions(Vec::new());
        }
        "Convert Loops to Regions" | "Loops to Regions" => {
            app.checkpoint("Convert Loops to Regions");
            let mut new_sel = sel.clone();
            if let Some(p) = app.project.as_mut() {
                for t in 0..p.tracks.len() {
                    let idx: Vec<usize> = sel.iter().filter(|x| x.0 == t).map(|x| x.1).collect();
                    new_sel.extend(loops_to_regions(&mut p.tracks[t].regions, &idx, end).into_iter().map(|r| (t, r)));
                }
            }
            app.select_regions(new_sel);
        }
        "Convert Repetitions to Loops" | "Repetitions to Loops" => {
            app.checkpoint("Convert Repetitions to Loops");
            let mut kept = Vec::new();
            if let Some(p) = app.project.as_mut() {
                // (latest first, so the earlier indices stay put)
                let mut v = sel.clone();
                v.sort_unstable_by(|a, b| b.cmp(a));
                for (t, r) in v {
                    let start = p.tracks.get(t).and_then(|x| x.regions.get(r)).map(|x| x.start);
                    if let (Some(tr), Some(s)) = (p.tracks.get_mut(t), start) {
                        repetitions_to_loops(&mut tr.regions, r);
                        if let Some(k) = tr.regions.iter().position(|x| x.start == s) {
                            kept.push((t, k));
                        }
                    }
                }
            }
            app.select_regions(kept);
        }
        "To Selected Track" => {
            let Some(to) = app.selected_track else { return false };
            app.checkpoint("Move to Selected Track");
            let mut v = sel.clone();
            v.sort_unstable_by(|a, b| b.cmp(a));
            let mut moved = Vec::new();
            for (t, r) in v {
                if t != to {
                    if let Some(k) = crate::arranging::move_to_track(app, t, r, to) {
                        moved.push((to, k));
                    }
                }
            }
            app.select_regions(moved);
        }
        _ => return false,
    }
    app.after_edit();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use lg_core::events::Note;

    fn reg(start: u32, len: u32, pitch: u8) -> Region {
        Region { name: "R".into(), start, length: len, notes: vec![Note { channel: 0, pitch, velocity: 100, pos: start + 10, len: 100, muted: false }], audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
    }

    #[test]
    fn numbered() {
        assert_eq!(numbered_names("Take 9", 3), ["Take 9", "Take 10", "Take 11"]);
        assert_eq!(numbered_names("Verse", 2), ["Verse", "Verse"]);
        assert_eq!(numbered_names("A01", 2), ["A01", "A02"]);
    }

    #[test]
    fn delete_and_move_closes_gaps() {
        let mut v = vec![reg(0, 3840, 60), reg(3840, 3840, 62), reg(7680, 3840, 64)];
        delete_and_move(&mut v, &[1]);
        assert_eq!(v.iter().map(|r| r.start).collect::<Vec<_>>(), [0, 3840]);
        assert_eq!(v[1].notes[0].pos, 3850);
    }

    #[test]
    fn loops_and_repetitions() {
        let mut v = vec![reg(0, 3840, 60), reg(3840 * 3 + 1920, 3840, 70)];
        v[0].props.looped = true;
        let made = loops_to_regions(&mut v, &[0], 99999);
        assert_eq!(made.len(), 3);
        assert_eq!(v.iter().map(|r| (r.start, r.length)).collect::<Vec<_>>(), [(0, 3840), (3840 * 3 + 1920, 3840), (3840, 3840), (7680, 3840), (11520, 1920)]);
        assert!(!v[0].props.looped);
        assert_eq!(v[2].notes[0].pos, 3850);
        let mut w = vec![reg(0, 3840, 60), reg(3840, 3840, 60), reg(7680, 3840, 60), reg(11520, 3840, 61)];
        assert_eq!(repetitions_to_loops(&mut w, 0), 2);
        assert_eq!(w.len(), 2);
        assert!(w[0].props.looped);
        assert_eq!(w[0].props.loop_to, 0);
        assert_eq!(loop_end(&w, 0, 99999), 11520);
    }
}
