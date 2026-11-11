//! Logic's MIDI Functions on a region's notes (Piano Roll ▸ Edit / Functions, the Event List and
//! the Tracks area's MIDI Functions): Quantize with Strength and Swing, the Note End trims (Force
//! Legato, Overlap Correction), Delete Duplicates and the other Delete MIDI Events, length,
//! split, join, repeat, pickup clock, channels by voice, and the Select commands (Same Pitch,
//! Same Note Name, Highest / Lowest, Overlapped, Inside Locators…). Everything works on a note
//! list and a selection (indexes into it; empty = all notes, as Logic does with nothing
//! selected); functions that add or remove notes keep the selection pointing at the same notes.
//! Positions are song ticks (960 per quarter note), the notes' own positions.

use crate::events::{Note, PPQ};

/// Logic's gap and overlap for the Note End trims: 8 ticks short of the next note, or 4 into it.
pub const TRIM_GAP: u32 = 8;
pub const TRIM_OVERLAP: u32 = 4;

/// The notes a function works on: the selection, or every note when nothing is selected.
pub fn targets(notes: &[Note], sel: &[usize]) -> Vec<usize> {
    if sel.is_empty() {
        (0..notes.len()).collect()
    } else {
        let mut v: Vec<usize> = sel.iter().copied().filter(|&i| i < notes.len()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

fn picked(sel: &[usize], i: usize) -> bool {
    sel.is_empty() || sel.contains(&i)
}

/// Removes the notes `drop` says, keeping `sel` on the same (remaining) notes.
pub fn remove_where(notes: &mut Vec<Note>, sel: &mut Vec<usize>, mut drop: impl FnMut(usize, &Note) -> bool) -> usize {
    let mut map = vec![None; notes.len()];
    let mut kept = Vec::with_capacity(notes.len());
    for (i, n) in notes.iter().enumerate() {
        if !drop(i, n) {
            map[i] = Some(kept.len());
            kept.push(*n);
        }
    }
    let removed = notes.len() - kept.len();
    *notes = kept;
    *sel = sel.iter().filter_map(|&i| map.get(i).copied().flatten()).collect();
    removed
}

// ---- quantize

/// Time Quantize (Q) with Logic's Strength (0..100 %) and Swing (50 = straight; above 50 every
/// second grid point later, up to half a step at 100): note starts move towards the grid, counted
/// from `origin` (the region's start). Lengths stay.
pub fn quantize(notes: &mut [Note], sel: &[usize], origin: u32, grid: u32, strength: u8, swing: u8) {
    if grid == 0 {
        return;
    }
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) {
            n.pos = quantized(n.pos, origin, grid, strength, swing);
        }
    }
}

/// One position quantized (see `quantize`).
pub fn quantized(pos: u32, origin: u32, grid: u32, strength: u8, swing: u8) -> u32 {
    let g = grid as f64;
    let rel = pos as f64 - origin as f64;
    let k = (rel / g).round();
    let mut target = k * g;
    if (k as i64).rem_euclid(2) == 1 {
        target += g * (swing as f64 - 50.0) / 100.0;
    }
    let s = strength.min(100) as f64 / 100.0;
    (origin as f64 + rel + (target - rel) * s).max(0.0).round() as u32
}

/// Quantize to whichever of several grids is nearest (Logic's "1/16 & 1/16 Triplet" values).
pub fn quantize_multi(notes: &mut [Note], sel: &[usize], origin: u32, grids: &[u32]) {
    for (i, n) in notes.iter_mut().enumerate() {
        if !picked(sel, i) {
            continue;
        }
        let best = grids.iter().filter(|&&g| g > 0).map(|&g| quantized(n.pos, origin, g, 100, 50)).min_by_key(|&p| (p as i64 - n.pos as i64).abs());
        if let Some(p) = best {
            n.pos = p;
        }
    }
}

/// Logic's Quantize values by name ("1/16 Note", "1/8 Triplet (1/12)", "Off"): their grids in
/// ticks (several for the combined values; empty for Off).
pub fn quantize_value(name: &str) -> Option<Vec<u32>> {
    let q = PPQ;
    Some(match name {
        "Off" => Vec::new(),
        "1/1 Note" => vec![q * 4],
        "1/2 Note" => vec![q * 2],
        "1/4 Note" => vec![q],
        "1/8 Note" => vec![q / 2],
        "1/16 Note" => vec![q / 4],
        "1/32 Note" => vec![q / 8],
        "1/64 Note" => vec![q / 16],
        "1/2 Triplet (1/3)" => vec![q * 4 / 3],
        "1/4 Triplet (1/6)" => vec![q * 2 / 3],
        "1/8 Triplet (1/12)" => vec![q / 3],
        "1/16 Triplet (1/24)" => vec![q / 6],
        "1/32 Triplet (1/48)" => vec![q / 12],
        "1/64 Triplet (1/96)" => vec![q / 24],
        "1/128 Triplet (1/192)" => vec![q / 48],
        "1/16 & 1/16 Triplet" => vec![q / 4, q / 6],
        "1/16 & 1/8 Triplet" => vec![q / 4, q / 3],
        "1/8 & 1/8 Triplet" => vec![q / 2, q / 3],
        _ => return None,
    })
}

/// Logic's single Quantize values in menu order (the Next / Previous Value commands step these).
pub const QUANTIZE_STEPS: [&str; 15] = ["Off", "1/1 Note", "1/2 Note", "1/4 Note", "1/8 Note", "1/16 Note", "1/32 Note", "1/64 Note", "1/2 Triplet (1/3)", "1/4 Triplet (1/6)", "1/8 Triplet (1/12)", "1/16 Triplet (1/24)", "1/32 Triplet (1/48)", "1/64 Triplet (1/96)", "1/128 Triplet (1/192)"];

// ---- note end trims

/// Which notes a Note End trim looks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimTo {
    /// The next selected note.
    Selected,
    /// The next note of any kind (Force Legato).
    Following,
    /// The next note of the same pitch.
    Repeated,
}

/// The Note End trims: each selected note's end moves to the next note's start (as `to` says
/// which) minus 8 ticks (`overlap` false) or plus 4 (true); `only_overlaps` shortens the notes
/// that run into it and leaves the others (Logic's "Remove Overlaps" variants). Notes starting
/// together (chords) don't count as next.
pub fn trim_ends(notes: &mut [Note], sel: &[usize], to: TrimTo, overlap: bool, only_overlaps: bool) {
    let snapshot: Vec<Note> = notes.to_vec();
    let chosen = targets(notes, sel);
    for &i in &chosen {
        let n = snapshot[i];
        let next = snapshot.iter().enumerate().filter(|(k, o)| {
            o.pos > n.pos
                && match to {
                    TrimTo::Selected => chosen.contains(k),
                    TrimTo::Following => true,
                    TrimTo::Repeated => o.pitch == n.pitch,
                }
        }).map(|(_, o)| o.pos).min();
        let Some(next) = next else { continue };
        let end = if overlap { next + TRIM_OVERLAP } else { next.saturating_sub(TRIM_GAP) };
        let end = end.max(n.pos + 1);
        if only_overlaps && n.pos + n.len <= end {
            continue;
        }
        notes[i].len = end - n.pos;
    }
}

/// Note Force Legato (Selected/Any): each selected note lasts until the next note (8 ticks gap).
pub fn force_legato(notes: &mut [Note], sel: &[usize]) {
    trim_ends(notes, sel, TrimTo::Following, false, false);
}

/// Note Overlap Correction (Selected/Selected): selected notes that run into the next selected
/// note end 8 ticks before it.
pub fn overlap_correction(notes: &mut [Note], sel: &[usize]) {
    trim_ends(notes, sel, TrimTo::Selected, false, true);
}

/// Note Start to Playhead: the starts move to `at`, the ends stay (notes ending before it are
/// left alone).
pub fn start_to(notes: &mut [Note], sel: &[usize], at: u32) {
    for (i, n) in notes.iter_mut().enumerate() {
        let end = n.pos + n.len;
        if picked(sel, i) && at < end {
            n.pos = at;
            n.len = end - at;
        }
    }
}

/// Note End to Playhead: the ends move to `at` (notes starting after it are left alone).
pub fn end_to(notes: &mut [Note], sel: &[usize], at: u32) {
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) && at > n.pos {
            n.len = at - n.pos;
        }
    }
}

// ---- delete

/// Delete Duplicated MIDI Events: notes at the same position with the same pitch and channel as
/// an earlier one go. Returns how many.
pub fn delete_duplicates(notes: &mut Vec<Note>, sel: &mut Vec<usize>) -> usize {
    let scope = sel.clone();
    let mut seen = std::collections::HashSet::new();
    remove_where(notes, sel, |i, n| picked(&scope, i) && !seen.insert((n.pos, n.pitch, n.channel)))
}

/// Delete MIDI Events ▸ Inside Locators / Outside Locators: notes starting inside [a, b) (or
/// outside it).
pub fn delete_by_locators(notes: &mut Vec<Note>, sel: &mut Vec<usize>, a: u32, b: u32, inside: bool) -> usize {
    remove_where(notes, sel, |_, n| (n.pos >= a && n.pos < b) == inside)
}

/// Delete MIDI Events ▸ Outside Region Boundaries: notes starting before the region or at or
/// after its end.
pub fn delete_outside_region(notes: &mut Vec<Note>, sel: &mut Vec<usize>, start: u32, length: u32) -> usize {
    delete_by_locators(notes, sel, start, start + length, false)
}

/// Delete MIDI Events ▸ Unselected Within Selection: the unselected notes that start between
/// the first selected note's start and the last selected note's end.
pub fn delete_unselected_within(notes: &mut Vec<Note>, sel: &mut Vec<usize>) -> usize {
    if sel.is_empty() {
        return 0;
    }
    let scope = sel.clone();
    let a = scope.iter().filter_map(|&i| notes.get(i)).map(|n| n.pos).min().unwrap_or(0);
    let b = scope.iter().filter_map(|&i| notes.get(i)).map(|n| n.pos + n.len).max().unwrap_or(0);
    remove_where(notes, sel, |i, n| !scope.contains(&i) && n.pos >= a && n.pos < b)
}

// ---- length, split, join, repeat, move

/// Length ▸ Double / Halve (num / den): the selected notes' lengths scaled.
pub fn scale_lengths(notes: &mut [Note], sel: &[usize], num: u32, den: u32) {
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) {
            n.len = ((n.len as u64 * num as u64) / den.max(1) as u64).max(1) as u32;
        }
    }
}

/// Length ▸ Change…: every selected note gets `len`.
pub fn set_lengths(notes: &mut [Note], sel: &[usize], len: u32) {
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) {
            n.len = len.max(1);
        }
    }
}

/// Split ▸ Notes at Playhead / Locators: selected notes that sound across a cut become two;
/// the selection keeps both halves.
pub fn split_notes(notes: &mut Vec<Note>, sel: &mut Vec<usize>, cuts: &[u32]) -> usize {
    let chosen = targets(notes, sel);
    let mut made = 0;
    let mut new_sel = Vec::new();
    let mut out = Vec::with_capacity(notes.len());
    for (i, n) in notes.iter().enumerate() {
        let mut pieces = vec![*n];
        if chosen.contains(&i) {
            let mut cs: Vec<u32> = cuts.iter().copied().filter(|&c| c > n.pos && c < n.pos + n.len).collect();
            cs.sort_unstable();
            cs.dedup();
            if !cs.is_empty() {
                pieces.clear();
                let mut from = n.pos;
                for c in cs.iter().copied().chain(std::iter::once(n.pos + n.len)) {
                    pieces.push(Note { pos: from, len: c - from, ..*n });
                    from = c;
                }
                made += pieces.len() - 1;
            }
        }
        for p in pieces {
            if !sel.is_empty() && chosen.contains(&i) {
                new_sel.push(out.len());
            }
            out.push(p);
        }
    }
    *notes = out;
    if !sel.is_empty() {
        *sel = new_sel;
    }
    made
}

/// Join Notes (⌘J): the selected notes of each pitch (and channel) become one, from the first's
/// start to the last's end, with the first's velocity. The joined notes stay selected.
pub fn join_notes(notes: &mut Vec<Note>, sel: &mut Vec<usize>) -> usize {
    let chosen = targets(notes, sel);
    let mut groups: std::collections::BTreeMap<(u8, u8), Vec<usize>> = Default::default();
    for &i in &chosen {
        groups.entry((notes[i].pitch, notes[i].channel)).or_default().push(i);
    }
    let mut gone = std::collections::HashSet::new();
    let mut joined = 0;
    for idx in groups.values().filter(|g| g.len() > 1) {
        let first = *idx.iter().min_by_key(|&&i| notes[i].pos).unwrap_or(&idx[0]);
        let start = notes[first].pos;
        let end = idx.iter().map(|&i| notes[i].pos + notes[i].len).max().unwrap_or(start + 1);
        notes[first].len = (end - start).max(1);
        for &i in idx {
            if i != first {
                gone.insert(i);
            }
        }
        joined += 1;
    }
    remove_where(notes, sel, |i, _| gone.contains(&i));
    joined
}

/// Repeat ▸ Once / Multiple: copies of the selected notes after them, `times` over, each `step`
/// ticks on (Logic: the selection's span, from its first start to its last end, rounded up to
/// `round`). The copies become the selection. Returns the step used.
pub fn repeat_notes(notes: &mut Vec<Note>, sel: &mut Vec<usize>, times: u32, round: u32) -> u32 {
    let chosen = targets(notes, sel);
    if chosen.is_empty() || times == 0 {
        return 0;
    }
    let a = chosen.iter().map(|&i| notes[i].pos).min().unwrap_or(0);
    let b = chosen.iter().map(|&i| notes[i].pos + notes[i].len).max().unwrap_or(0);
    let r = round.max(1);
    let step = (b - a).div_ceil(r).max(1) * r;
    let mut new_sel = Vec::new();
    for k in 1..=times {
        for &i in &chosen {
            let mut n = notes[i];
            n.pos += step * k;
            notes.push(n);
            new_sel.push(notes.len() - 1);
        }
    }
    *sel = new_sel;
    step
}

/// Move ▸ To Playhead (Pickup Clock): the selection's first note starts at `at`, the others keep
/// their distance to it.
pub fn move_to(notes: &mut [Note], sel: &[usize], at: u32) {
    let chosen = targets(notes, sel);
    let Some(first) = chosen.iter().map(|&i| notes[i].pos).min() else { return };
    let d = at as i64 - first as i64;
    for &i in &chosen {
        notes[i].pos = (notes[i].pos as i64 + d).max(0) as u32;
    }
}

/// Transpose by semitones (kept in 0..127).
pub fn transpose(notes: &mut [Note], sel: &[usize], by: i32) {
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) {
            n.pitch = (n.pitch as i32 + by).clamp(0, 127) as u8;
        }
    }
}

/// Velocities: a fixed value, or the old ones scaled by `percent` and offset by `add`.
pub fn set_velocity(notes: &mut [Note], sel: &[usize], fixed: Option<u8>, percent: f32, add: i32) {
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) {
            n.velocity = match fixed {
                Some(v) => v.clamp(1, 127),
                None => ((n.velocity as f32 * percent / 100.0).round() as i32 + add).clamp(1, 127) as u8,
            };
        }
    }
}

// ---- channels

/// Set MIDI Channel to Voice Number: in each chord (notes starting together) the highest note
/// gets channel 1, the next channel 2 and so on (channels as stored: 0 = 1).
pub fn channel_by_voice(notes: &mut [Note], sel: &[usize]) {
    let chosen = targets(notes, sel);
    let mut by_pos: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
    for &i in &chosen {
        by_pos.entry(notes[i].pos).or_default().push(i);
    }
    for idx in by_pos.values_mut() {
        idx.sort_by_key(|&i| std::cmp::Reverse(notes[i].pitch));
        for (v, &i) in idx.iter().enumerate() {
            notes[i].channel = v.min(15) as u8;
        }
    }
}

/// Event Channel +1 / -1 / = n (`set`: a channel 0..15; else `by`).
pub fn change_channel(notes: &mut [Note], sel: &[usize], set: Option<u8>, by: i32) {
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) {
            n.channel = match set {
                Some(c) => c.min(15),
                None => (n.channel as i32 + by).clamp(0, 15) as u8,
            };
        }
    }
}

// ---- select

/// The notes like the selected ones by `same` (Same Note Pitch, Same Note Name, Same MIDI
/// Channel…); nothing selected: nothing.
pub fn select_like(notes: &[Note], sel: &[usize], same: impl Fn(&Note, &Note) -> bool) -> Vec<usize> {
    let refs: Vec<Note> = sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
    (0..notes.len()).filter(|&i| refs.iter().any(|r| same(r, &notes[i]))).collect()
}

pub fn select_same_pitch(notes: &[Note], sel: &[usize]) -> Vec<usize> {
    select_like(notes, sel, |a, b| a.pitch == b.pitch)
}

pub fn select_same_name(notes: &[Note], sel: &[usize]) -> Vec<usize> {
    select_like(notes, sel, |a, b| a.pitch % 12 == b.pitch % 12)
}

pub fn select_same_channel(notes: &[Note], sel: &[usize]) -> Vec<usize> {
    select_like(notes, sel, |a, b| a.channel == b.channel)
}

pub fn select_same_velocity(notes: &[Note], sel: &[usize]) -> Vec<usize> {
    select_like(notes, sel, |a, b| a.velocity == b.velocity)
}

/// Same Subposition: notes at the same place within a bar (`bar` ticks) as a selected one.
pub fn select_same_subposition(notes: &[Note], sel: &[usize], origin: u32, bar: u32) -> Vec<usize> {
    let b = bar.max(1) as i64;
    select_like(notes, sel, |a, n| (a.pos as i64 - origin as i64).rem_euclid(b) == (n.pos as i64 - origin as i64).rem_euclid(b))
}

/// Overlapped Notes: notes that sound into a later note of the same pitch, or into it from an
/// earlier one (Logic's overlaps are by pitch).
pub fn select_overlapped(notes: &[Note]) -> Vec<usize> {
    (0..notes.len()).filter(|&i| {
        let a = notes[i];
        notes.iter().enumerate().any(|(k, b)| k != i && a.pitch == b.pitch && a.channel == b.channel && a.pos < b.pos + b.len && b.pos < a.pos + a.len)
    }).collect()
}

/// All Following (from the first selected note, or `from`); `same_pitch`: of the selected notes'
/// pitches only.
pub fn select_following(notes: &[Note], sel: &[usize], from: u32, same_pitch: bool) -> Vec<usize> {
    let start = sel.iter().filter_map(|&i| notes.get(i)).map(|n| n.pos).min().unwrap_or(from);
    let pitches: Vec<u8> = sel.iter().filter_map(|&i| notes.get(i)).map(|n| n.pitch).collect();
    (0..notes.len()).filter(|&i| notes[i].pos >= start && (!same_pitch || pitches.contains(&notes[i].pitch))).collect()
}

/// All Inside Locators: notes starting in [a, b).
pub fn select_inside(notes: &[Note], a: u32, b: u32) -> Vec<usize> {
    (0..notes.len()).filter(|&i| notes[i].pos >= a && notes[i].pos < b).collect()
}

/// Deselect Outside Locators: the selection minus notes starting outside [a, b).
pub fn deselect_outside(notes: &[Note], sel: &[usize], a: u32, b: u32) -> Vec<usize> {
    sel.iter().copied().filter(|&i| notes.get(i).is_some_and(|n| n.pos >= a && n.pos < b)).collect()
}

/// Highest / Lowest Notes: the top (or bottom) note of each chord among the selected notes (all
/// notes when nothing is selected).
pub fn select_extreme(notes: &[Note], sel: &[usize], highest: bool) -> Vec<usize> {
    let mut by_pos: std::collections::BTreeMap<u32, usize> = Default::default();
    for i in targets(notes, sel) {
        let e = by_pos.entry(notes[i].pos).or_insert(i);
        let better = if highest { notes[i].pitch > notes[*e].pitch } else { notes[i].pitch < notes[*e].pitch };
        if better {
            *e = i;
        }
    }
    by_pos.into_values().collect()
}

/// Invert Selection.
pub fn invert(notes: &[Note], sel: &[usize]) -> Vec<usize> {
    (0..notes.len()).filter(|i| !sel.contains(i)).collect()
}

/// Next / Previous Event (→ / ←): the note after (or before) the last selected one in time
/// order (position, then pitch); nothing selected: the first (or last) note.
pub fn step_selection(notes: &[Note], sel: &[usize], next: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| (notes[i].pos, notes[i].pitch));
    let cur = sel.last().and_then(|s| order.iter().position(|i| i == s));
    let pick = match (cur, next) {
        (Some(k), true) => order.get(k + 1).or(order.get(k)),
        (Some(k), false) => order.get(k.saturating_sub(1)),
        (None, true) => order.first(),
        (None, false) => order.last(),
    };
    pick.map(|&i| vec![i]).unwrap_or_default()
}

/// The selection's span in time (first start, last end).
pub fn span(notes: &[Note], sel: &[usize]) -> Option<(u32, u32)> {
    let t = targets(notes, sel);
    Some((t.iter().map(|&i| notes[i].pos).min()?, t.iter().map(|&i| notes[i].pos + notes[i].len).max()?))
}

/// Time handles: the selected notes stretched (or squeezed) so the span [a, b] becomes [a2, b2]
/// (positions and lengths scale by the same factor).
pub fn stretch(notes: &mut [Note], sel: &[usize], from: (u32, u32), to: (u32, u32)) {
    let (a, b) = (from.0 as f64, from.1 as f64);
    if b <= a || to.1 <= to.0 {
        return;
    }
    let k = (to.1 as f64 - to.0 as f64) / (b - a);
    for (i, n) in notes.iter_mut().enumerate() {
        if picked(sel, i) {
            let s = to.0 as f64 + (n.pos as f64 - a) * k;
            let e = to.0 as f64 + (n.pos as f64 + n.len as f64 - a) * k;
            n.pos = s.max(0.0).round() as u32;
            n.len = (e.round() as i64 - n.pos as i64).max(1) as u32;
        }
    }
}

// ---- Copy MIDI Events…

/// The Copy MIDI Events window's modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyMode {
    CopyMerge,
    CopyReplace,
    CopyInsert,
    MoveMerge,
    MoveReplace,
    MoveInsert,
}

impl CopyMode {
    pub const ALL: [CopyMode; 6] = [CopyMode::CopyMerge, CopyMode::CopyReplace, CopyMode::CopyInsert, CopyMode::MoveMerge, CopyMode::MoveReplace, CopyMode::MoveInsert];

    pub fn name(self) -> &'static str {
        match self {
            CopyMode::CopyMerge => "Copy Merge",
            CopyMode::CopyReplace => "Copy Replace",
            CopyMode::CopyInsert => "Copy Insert",
            CopyMode::MoveMerge => "Move Merge",
            CopyMode::MoveReplace => "Move Replace",
            CopyMode::MoveInsert => "Move Insert",
        }
    }
}

/// Copy MIDI Events…: the notes starting in [a, b) copied (or moved) to `dest`, `copies` times
/// one after another; Replace clears what was there, Insert moves later notes on by the copied
/// length. The new notes become the selection.
pub fn copy_events(notes: &mut Vec<Note>, sel: &mut Vec<usize>, a: u32, b: u32, dest: u32, copies: u32, mode: CopyMode) -> usize {
    if b <= a || copies == 0 {
        return 0;
    }
    let len = b - a;
    let total = len * copies;
    let source: Vec<Note> = notes.iter().copied().filter(|n| n.pos >= a && n.pos < b).collect();
    if source.is_empty() {
        return 0;
    }
    let moving = matches!(mode, CopyMode::MoveMerge | CopyMode::MoveReplace | CopyMode::MoveInsert);
    let mut none = Vec::new();
    if moving {
        remove_where(notes, &mut none, |_, n| n.pos >= a && n.pos < b);
    }
    match mode {
        CopyMode::CopyReplace | CopyMode::MoveReplace => {
            remove_where(notes, &mut none, |_, n| n.pos >= dest && n.pos < dest + total);
        }
        CopyMode::CopyInsert | CopyMode::MoveInsert => {
            for n in notes.iter_mut().filter(|n| n.pos >= dest) {
                n.pos += total;
            }
        }
        _ => {}
    }
    sel.clear();
    for k in 0..copies {
        for s in &source {
            notes.push(Note { pos: dest + k * len + (s.pos - a), ..*s });
            sel.push(notes.len() - 1);
        }
    }
    source.len() * copies as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(pos: u32, len: u32, pitch: u8) -> Note {
        Note { channel: 0, pitch, velocity: 80, pos, len, muted: false }
    }

    #[test]
    fn quantize_strength_and_swing() {
        let mut v = vec![n(250, 100, 60), n(470, 100, 62), n(730, 10, 64)];
        quantize(&mut v, &[], 0, 240, 100, 50);
        assert_eq!(v.iter().map(|x| x.pos).collect::<Vec<_>>(), [240, 480, 720]);
        let mut v = vec![n(300, 100, 60)];
        quantize(&mut v, &[], 0, 240, 50, 50);
        assert_eq!(v[0].pos, 270);
        // swing 100: the second sixteenth half a step later
        let mut v = vec![n(240, 100, 60), n(480, 100, 60)];
        quantize(&mut v, &[], 0, 240, 100, 100);
        assert_eq!(v[0].pos, 360);
        assert_eq!(v[1].pos, 480);
        // the selection only; from the region's start
        let mut v = vec![n(1010, 10, 60), n(1010, 10, 61)];
        quantize(&mut v, &[1], 1000, 240, 100, 50);
        assert_eq!((v[0].pos, v[1].pos), (1010, 1000));
    }

    #[test]
    fn quantize_combined_grids() {
        assert_eq!(quantize_value("1/16 Note"), Some(vec![240]));
        assert_eq!(quantize_value("Off"), Some(vec![]));
        let mut v = vec![n(165, 10, 60), n(230, 10, 60)];
        quantize_multi(&mut v, &[], 0, &quantize_value("1/16 & 1/16 Triplet").unwrap());
        assert_eq!(v[0].pos, 160);
        assert_eq!(v[1].pos, 240);
    }

    #[test]
    fn note_end_trims() {
        let mut v = vec![n(0, 1000, 60), n(0, 50, 64), n(480, 100, 62), n(960, 100, 60)];
        force_legato(&mut v, &[]);
        assert_eq!(v[0].len, 472);
        assert_eq!(v[1].len, 472, "short notes lengthen");
        assert_eq!(v[2].len, 472);
        assert_eq!(v[3].len, 100, "last note stays");
        let mut v = vec![n(0, 1000, 60), n(0, 50, 64), n(480, 100, 62)];
        trim_ends(&mut v, &[], TrimTo::Following, false, true);
        assert_eq!((v[0].len, v[1].len), (472, 50), "remove overlaps only shortens");
        let mut v = vec![n(0, 100, 60), n(480, 100, 62)];
        trim_ends(&mut v, &[], TrimTo::Following, true, false);
        assert_eq!(v[0].len, 484);
        // repeated: the next note of the same pitch
        let mut v = vec![n(0, 2000, 60), n(480, 100, 62), n(960, 100, 60)];
        trim_ends(&mut v, &[], TrimTo::Repeated, false, true);
        assert_eq!(v[0].len, 952);
        // to selected notes: the unselected one in between doesn't count
        let mut v = vec![n(0, 100, 60), n(480, 100, 62), n(960, 100, 64)];
        trim_ends(&mut v, &[0, 2], TrimTo::Selected, false, false);
        assert_eq!(v[0].len, 952);
        assert_eq!(v[1].len, 100);
        let mut v = vec![n(0, 1000, 60), n(480, 100, 62)];
        overlap_correction(&mut v, &[0, 1]);
        assert_eq!(v[0].len, 472);
    }

    #[test]
    fn start_and_end_to_playhead() {
        let mut v = vec![n(0, 960, 60), n(2000, 10, 60)];
        start_to(&mut v, &[], 480);
        assert_eq!((v[0].pos, v[0].len), (480, 480));
        assert_eq!(v[1].pos, 480, "a note after the playhead starts there");
        let mut v = vec![n(0, 960, 60), n(2000, 10, 60)];
        end_to(&mut v, &[], 480);
        assert_eq!(v[0].len, 480);
        assert_eq!(v[1].len, 10);
    }

    #[test]
    fn deletes_keep_the_selection() {
        let mut v = vec![n(0, 10, 60), n(0, 20, 60), n(10, 10, 62), n(0, 10, 60)];
        let mut sel = vec![2];
        assert_eq!(delete_duplicates(&mut v, &mut vec![]), 2);
        let mut v2 = vec![n(0, 10, 60), n(0, 20, 60), n(10, 10, 62)];
        delete_duplicates(&mut v2, &mut sel);
        assert_eq!(v2.len(), 3, "only among the selected");
        assert_eq!(v.len(), 2);
        let mut v = vec![n(0, 10, 60), n(500, 10, 60), n(1000, 10, 62)];
        let mut sel = vec![2];
        assert_eq!(delete_by_locators(&mut v, &mut sel, 400, 900, true), 1);
        assert_eq!(sel, vec![1]);
        assert_eq!(v[sel[0]].pos, 1000);
        let mut v = vec![n(0, 10, 60), n(500, 10, 60), n(1000, 10, 62)];
        delete_by_locators(&mut v, &mut vec![], 400, 900, false);
        assert_eq!(v.len(), 1);
        let mut v = vec![n(0, 10, 60), n(500, 10, 60), n(1000, 10, 62)];
        delete_outside_region(&mut v, &mut vec![], 100, 1000);
        assert_eq!(v.len(), 2);
        let mut v = vec![n(0, 10, 60), n(500, 10, 61), n(1000, 10, 62), n(3000, 1, 1)];
        let mut sel = vec![0, 2];
        assert_eq!(delete_unselected_within(&mut v, &mut sel), 1);
        assert_eq!(v.len(), 3);
        assert_eq!(sel, vec![0, 1]);
    }

    #[test]
    fn lengths_split_join() {
        let mut v = vec![n(0, 100, 60), n(0, 100, 61)];
        scale_lengths(&mut v, &[1], 2, 1);
        assert_eq!((v[0].len, v[1].len), (100, 200));
        scale_lengths(&mut v, &[], 1, 2);
        assert_eq!((v[0].len, v[1].len), (50, 100));
        set_lengths(&mut v, &[], 240);
        assert_eq!(v[1].len, 240);
        let mut v = vec![n(0, 1000, 60), n(2000, 100, 61)];
        let mut sel = vec![0];
        assert_eq!(split_notes(&mut v, &mut sel, &[400, 800]), 2);
        assert_eq!(v.iter().map(|x| (x.pos, x.len)).collect::<Vec<_>>(), [(0, 400), (400, 400), (800, 200), (2000, 100)]);
        assert_eq!(sel, vec![0, 1, 2]);
        let mut sel = vec![0, 1, 2];
        assert_eq!(join_notes(&mut v, &mut sel), 1);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].pos, v[0].len), (0, 1000));
        assert_eq!(sel, vec![0]);
    }

    #[test]
    fn repeat_and_pickup() {
        let mut v = vec![n(960, 200, 60), n(1200, 200, 62)];
        let mut sel = vec![0, 1];
        let step = repeat_notes(&mut v, &mut sel, 2, 240);
        assert_eq!(step, 480);
        assert_eq!(v.len(), 6);
        assert_eq!(v[5].pos, 1200 + 960);
        assert_eq!(sel, vec![2, 3, 4, 5]);
        let mut v = vec![n(960, 200, 60), n(1200, 200, 62), n(5000, 1, 1)];
        move_to(&mut v, &[0, 1], 100);
        assert_eq!((v[0].pos, v[1].pos, v[2].pos), (100, 340, 5000));
    }

    #[test]
    fn channels_and_velocity() {
        let mut v = vec![n(0, 10, 60), n(0, 10, 67), n(0, 10, 64), n(100, 10, 50)];
        channel_by_voice(&mut v, &[]);
        assert_eq!(v.iter().map(|x| x.channel).collect::<Vec<_>>(), [2, 0, 1, 0]);
        change_channel(&mut v, &[3], None, 1);
        assert_eq!(v[3].channel, 1);
        change_channel(&mut v, &[], Some(9), 0);
        assert!(v.iter().all(|x| x.channel == 9));
        set_velocity(&mut v, &[0], Some(127), 100.0, 0);
        assert_eq!(v[0].velocity, 127);
        set_velocity(&mut v, &[1], None, 50.0, 10);
        assert_eq!(v[1].velocity, 50);
    }

    #[test]
    fn selections() {
        let v = vec![n(0, 10, 60), n(0, 10, 64), n(480, 10, 72), n(960, 10, 61), n(1920, 10, 60)];
        assert_eq!(select_same_pitch(&v, &[0]), [0, 4]);
        assert_eq!(select_same_name(&v, &[0]), [0, 2, 4]);
        assert_eq!(select_same_channel(&v, &[0]).len(), 5);
        assert_eq!(select_same_subposition(&v, &[0], 0, 1920), [0, 1, 4]);
        assert_eq!(select_extreme(&v, &[], true), [1, 2, 3, 4]);
        assert_eq!(select_extreme(&v, &[], false), [0, 2, 3, 4]);
        assert_eq!(select_following(&v, &[2], 0, false), [2, 3, 4]);
        assert_eq!(select_following(&v, &[3], 0, true), [3]);
        assert_eq!(select_inside(&v, 400, 1000), [2, 3]);
        assert_eq!(deselect_outside(&v, &[0, 2, 3], 400, 1000), [2, 3]);
        assert_eq!(invert(&v, &[0, 1]), [2, 3, 4]);
        assert_eq!(step_selection(&v, &[], true), [0]);
        assert_eq!(step_selection(&v, &[1], true), [2]);
        assert_eq!(step_selection(&v, &[2], false), [1]);
        let o = vec![n(0, 500, 60), n(400, 100, 60), n(400, 100, 61)];
        assert_eq!(select_overlapped(&o), [0, 1]);
        assert_eq!(span(&v, &[1, 2]), Some((0, 490)));
    }

    #[test]
    fn copy_midi_events() {
        let base = vec![n(0, 10, 60), n(240, 10, 62), n(960, 10, 64), n(1920, 10, 65)];
        let mut v = base.clone();
        let mut sel = vec![];
        assert_eq!(copy_events(&mut v, &mut sel, 0, 480, 960, 2, CopyMode::CopyMerge), 4);
        assert_eq!(v.len(), 8);
        assert_eq!(sel.iter().map(|&i| v[i].pos).collect::<Vec<_>>(), [960, 1200, 1440, 1680]);
        let mut v = base.clone();
        copy_events(&mut v, &mut sel, 0, 480, 960, 1, CopyMode::CopyReplace);
        assert!(!v.iter().any(|x| x.pitch == 64), "the note at the destination is replaced");
        let mut v = base.clone();
        copy_events(&mut v, &mut sel, 0, 480, 960, 1, CopyMode::CopyInsert);
        assert_eq!(v.iter().find(|x| x.pitch == 65).map(|x| x.pos), Some(2400));
        let mut v = base.clone();
        copy_events(&mut v, &mut sel, 0, 480, 960, 1, CopyMode::MoveMerge);
        assert_eq!(v.len(), 4);
        assert!(v.iter().all(|x| x.pos >= 960));
    }

    #[test]
    fn time_handles_stretch() {
        let mut v = vec![n(0, 480, 60), n(480, 480, 62)];
        stretch(&mut v, &[], (0, 960), (0, 1920));
        assert_eq!(v.iter().map(|x| (x.pos, x.len)).collect::<Vec<_>>(), [(0, 960), (960, 960)]);
        stretch(&mut v, &[], (0, 1920), (960, 1920));
        assert_eq!(v.iter().map(|x| (x.pos, x.len)).collect::<Vec<_>>(), [(960, 480), (1440, 480)]);
    }
}
