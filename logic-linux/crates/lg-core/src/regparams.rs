//! Logic's region parameters as a region plays (the Region inspector's Quantize with Q-Strength,
//! Q-Range and Q-Flam, Gate Time Legato, Clip Length, Pitch Source) and the region edits of the
//! Tracks area that work on whole regions: Length Halve / Double / Change, Join's parameter
//! normalization, Cut / Insert Time (sections between the locators).

use crate::events::Note;
use crate::project::{Project, Region, RegionProps, GATE_LEGATO};

/// Semitone of a key's tonic ("A minor" → 9; "F♯ major" / "F# major" → 6).
pub fn key_root(name: &str) -> Option<i16> {
    let mut c = name.chars();
    let base = match c.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let acc = match c.next() {
        Some('#') | Some('♯') => 1,
        Some('b') | Some('♭') => -1,
        _ => 0,
    };
    Some((base + acc + 12) % 12)
}

/// Pitch Source Key Signature: how far the key at `tick` is from the project's first key, as
/// the nearest shift (-6 to +5 semitones).
pub fn key_shift(p: &Project, tick: u32) -> i16 {
    if p.global.keys.is_empty() {
        return 0;
    }
    let bar = crate::timeline::Map::of(p).bar_of(tick).0;
    let (Some(a), Some(b)) = (key_root(&p.key), key_root(&crate::timeline::key_at(p, bar))) else { return 0 };
    let d = (b - a).rem_euclid(12);
    if d > 5 { d - 12 } else { d }
}

/// The notes of a region as it plays: each through `RegionProps::apply`, then Q-Flam (notes
/// that start together spread up or down by the flam), Pitch Source (`shift` at each note),
/// Gate Time Legato (every note to the next start) and Clip Length (no note past the end).
pub fn play_notes(props: &RegionProps, notes: &[Note], start: u32, end: u32, shift: &dyn Fn(u32) -> i16) -> Vec<Note> {
    play_notes_groove(props, notes, start, end, shift, None)
}

/// `play_notes` with the region's groove template (when its Quantize is one).
pub fn play_notes_groove(props: &RegionProps, notes: &[Note], start: u32, end: u32, shift: &dyn Fn(u32) -> i16, groove: Option<&crate::groove::Points>) -> Vec<Note> {
    let mut out: Vec<Note> = notes.iter().filter(|n| !n.muted && n.pos < end).map(|n| {
        let x = props.apply(n, start);
        match groove {
            Some(g) => {
                let q = crate::groove::apply(n, g, props.q_strength, props.q_velocity, props.q_length);
                Note { pos: (q.pos as i64 + props.delay as i64).max(0) as u32, velocity: (x.velocity as i16 + q.velocity as i16 - n.velocity as i16).clamp(1, 127) as u8, len: if props.gate_time == 100 { q.len } else { x.len }, ..x }
            }
            None => x,
        }
    }).collect();
    if props.q_flam != 0 {
        out.sort_by_key(|n| (n.pos, n.pitch));
        let mut i = 0;
        while i < out.len() {
            let j = (i..out.len()).find(|&j| out[j].pos != out[i].pos).unwrap_or(out.len());
            let f = props.q_flam as i64;
            for k in i..j {
                // positive: upward (lowest first); negative: downward (highest first)
                let step = if f > 0 { (k - i) as i64 } else { (j - 1 - k) as i64 };
                out[k].pos = (out[k].pos as i64 + step * f.abs()).max(0) as u32;
            }
            i = j;
        }
    }
    if props.pitch_source == 1 {
        for n in &mut out {
            n.pitch = (n.pitch as i16 + shift(n.pos)).clamp(0, 127) as u8;
        }
    }
    out.sort_by_key(|n| (n.pos, n.pitch));
    if props.gate_time == GATE_LEGATO {
        let starts: Vec<u32> = out.iter().map(|n| n.pos).collect();
        for n in &mut out {
            let next = starts.iter().copied().find(|&s| s > n.pos).unwrap_or(end.max(n.pos + 1));
            n.len = (next - n.pos).max(1);
        }
    }
    if props.clip_length {
        for n in &mut out {
            n.len = n.len.min(end.saturating_sub(n.pos)).max(1);
        }
    }
    out
}

/// Edit ▸ Length ▸ Halve: the region cut at its centre, the right half dropped (notes there
/// removed, notes across the new end shortened).
pub fn halve(r: &mut Region) {
    let len = (r.length / 2).max(1);
    set_length(r, len, true);
}

/// Edit ▸ Length ▸ Double: twice as long, its MIDI contents repeated in the new half.
pub fn double(r: &mut Region) {
    change_length(r, 2.0);
}

/// Edit ▸ Length ▸ Change…: the length times `factor` (0.25 to 16); MIDI contents repeated to
/// fill a longer region, cut to fit a shorter one.
pub fn change_length(r: &mut Region, factor: f32) {
    let f = factor.clamp(0.25, 16.0);
    let old = r.length.max(1);
    let new = ((old as f64 * f as f64).round() as u32).max(1);
    if new <= old {
        set_length(r, new, true);
        return;
    }
    if r.audio.is_none() {
        let orig: Vec<Note> = r.notes.iter().filter(|n| n.pos < r.start + old).copied().collect();
        let mut k = 1u32;
        while k * old < new {
            for n in &orig {
                let pos = n.pos + k * old;
                if pos < r.start + new {
                    r.notes.push(Note { pos, ..*n });
                }
            }
            k += 1;
        }
        let ev: Vec<crate::events::Control> = r.ext.events.iter().filter(|e| e.pos < old).copied().collect();
        let mut k = 1u32;
        while k * old < new {
            r.ext.events.extend(ev.iter().filter(|e| e.pos + k * old < new).map(|e| crate::events::Control { pos: e.pos + k * old, ..*e }));
            k += 1;
        }
    }
    r.length = new;
}

/// The region `len` long; with `trim` the notes past the end removed and notes across it
/// shortened (Logic's split behaviour).
pub fn set_length(r: &mut Region, len: u32, trim: bool) {
    r.length = len.max(1);
    if trim {
        let end = r.start + r.length;
        r.notes.retain(|n| n.pos < end);
        for n in &mut r.notes {
            n.len = n.len.min(end - n.pos).max(1);
        }
        let l = r.length;
        r.ext.events.retain(|e| e.pos < l);
    }
}

/// Before Join: each region's Transpose, Velocity and Dynamics written into its notes and the
/// parameters set back to neutral, so the joined region keeps the sound and has neutral ones.
pub fn normalize_for_join(r: &mut Region) {
    let p = r.props;
    if p.transpose == 0 && p.velocity == 0 && p.dynamics == 100 {
        return;
    }
    let n = RegionProps { transpose: p.transpose, velocity: p.velocity, dynamics: p.dynamics, ..Default::default() };
    let s = r.start;
    r.notes = r.notes.iter().map(|x| Note { pitch: n.apply(x, s).pitch, velocity: n.apply(x, s).velocity, ..*x }).collect();
    r.props.transpose = 0;
    r.props.velocity = 0;
    r.props.dynamics = 100;
}

/// A looped region's repetitions made real (Join tool on a looped region): its notes repeated
/// up to `until`, the region that long, Loop off.
pub fn unloop(r: &mut Region, until: u32) {
    if !r.props.looped || until <= r.start + r.length {
        r.props.looped = false;
        return;
    }
    let f = (until - r.start) as f32 / r.length.max(1) as f32;
    let len = until - r.start;
    change_length(r, f);
    r.length = len;
    r.props.looped = false;
    r.props.loop_to = 0;
}

/// What Cut / Copy Section put on the clipboard: (track, region) pieces with their positions
/// from the section start, and the section's length.
#[derive(Clone, Debug, Default)]
pub struct Section {
    pub len: u32,
    pub pieces: Vec<(usize, Region)>,
}

/// Ticks `a..b` in audio frames (the caller's tempo map and sample rate).
pub type Frames<'a> = &'a dyn Fn(u32, u32) -> u32;

/// Region `r` cut at `at` (song ticks): the left part stays in `r`, the right part is returned.
/// Audio regions keep playing the same audio: the right part's file offset
/// (`props.audio_offset`) moves on by the frames cut off.
pub fn cut_at(r: &mut Region, at: u32, frames: Frames) -> Option<Region> {
    if at <= r.start || at >= r.start + r.length {
        return None;
    }
    let mut right = r.clone();
    right.start = at;
    right.length = r.start + r.length - at;
    right.notes.retain(|n| n.pos >= at);
    let d = at - r.start;
    right.ext.events = r.ext.events.iter().filter(|e| e.pos >= d).map(|e| crate::events::Control { pos: e.pos - d, ..*e }).collect();
    right.origin = r.source.or(r.origin);
    right.source = None;
    right.props.fade_in = (0, 0);
    if right.audio.is_some() {
        right.props.audio_offset += frames(r.start, at);
    }
    set_length(r, d, true);
    r.props.fade_out = (0, 0);
    Some(right)
}

fn tracks_of(p: &Project, only: Option<&[usize]>) -> Vec<usize> {
    (0..p.tracks.len()).filter(|t| only.is_none_or(|o| o.contains(t))).collect()
}

fn shift(r: &mut Region, by: i64) {
    let to = (r.start as i64 + by).max(0);
    let d = to - r.start as i64;
    r.start = to as u32;
    for n in &mut r.notes {
        n.pos = (n.pos as i64 + d).max(0) as u32;
    }
}

/// Global events (tempo changes, markers, arrangement markers, key and signature changes, the
/// Transposition track, chords) at or after `at` moved by `by` ticks; those inside a removed
/// range `[at, at - by)` (by < 0) are dropped.
pub fn shift_global(p: &mut Project, at: u32, by: i64) {
    let mv = |t: u32| -> Option<u32> {
        if t < at {
            return Some(t);
        }
        if by < 0 && (t as i64) < at as i64 - by {
            return None;
        }
        Some((t as i64 + by).max(0) as u32)
    };
    p.tempo_map = p.tempo_map.iter().filter_map(|&(t, b)| if t == 0 { Some((t, b)) } else { mv(t).map(|t| (t, b)) }).collect();
    p.tempo_map.dedup_by_key(|x| x.0);
    p.global.tempo_curves = p.global.tempo_curves.iter().filter_map(|&(t, c)| mv(t).map(|t| (t, c))).collect();
    p.markers = p.markers.iter().filter_map(|m| mv(m.0).map(|t| crate::markers::Marker(t, m.1.clone(), m.2.clone()))).collect();
    p.arrangement = p.arrangement.iter().filter_map(|(t, l, n)| mv(*t).map(|t| (t, *l, n.clone()))).collect();
    p.transposition = p.transposition.iter().filter_map(|&(t, s)| if t == 0 { Some((t, s)) } else { mv(t).map(|t| (t, s)) }).collect();
    p.global.chords = p.global.chords.iter().filter_map(|(t, l, n)| mv(*t).map(|t| (t, *l, n.clone()))).collect();
    // key and signature changes are kept by bar: whole bars only
    let map = crate::timeline::Map::of(p);
    let bar_len = crate::timeline::Map::bar_len(map.sig_at(at)).max(1) as i64;
    if by % bar_len == 0 {
        let at_bar = map.bar_of(at).0;
        let bars = by / bar_len;
        let mvb = |b: u32| -> Option<u32> {
            if b < at_bar {
                return Some(b);
            }
            if bars < 0 && (b as i64) < at_bar as i64 - bars {
                return None;
            }
            Some((b as i64 + bars).max(1) as u32)
        };
        p.global.keys = p.global.keys.iter().filter_map(|(b, k)| mvb(*b).map(|b| (b, k.clone()))).collect();
        p.global.signatures = p.global.signatures.iter().filter_map(|&(b, n, d)| mvb(b).map(|b| (b, n, d))).collect();
    }
}

/// Insert Silence Between Locators: regions (on `only` tracks, or all) cut at `from`, and
/// everything from there moved right by `to - from`; with `global` the global events too.
pub fn insert_silence(p: &mut Project, from: u32, to: u32, only: Option<&[usize]>, global: bool, frames: Frames) {
    let len = to.saturating_sub(from);
    if len == 0 {
        return;
    }
    for t in tracks_of(p, only) {
        let tr = &mut p.tracks[t];
        let mut extra = Vec::new();
        for r in tr.regions.iter_mut() {
            if let Some(right) = cut_at(r, from, frames) {
                extra.push(right);
            }
        }
        tr.regions.extend(extra);
        for r in tr.regions.iter_mut().filter(|r| r.start >= from) {
            shift(r, len as i64);
        }
    }
    if global {
        shift_global(p, from, len as i64);
    }
}

/// Copy Section Between Locators: the parts of regions between `from` and `to` (cut at the
/// locators), as a Section.
pub fn copy_section(p: &Project, from: u32, to: u32, only: Option<&[usize]>, frames: Frames) -> Section {
    let mut sec = Section { len: to.saturating_sub(from), pieces: Vec::new() };
    for t in tracks_of(p, only) {
        for r in &p.tracks[t].regions {
            if r.start >= to || r.start + r.length <= from {
                continue;
            }
            let mut piece = r.clone();
            if let Some(right) = cut_at(&mut piece, from, frames) {
                piece = right;
            }
            cut_at(&mut piece, to, frames);
            piece.origin = piece.source.or(piece.origin);
            piece.source = None;
            shift(&mut piece, -(from as i64));
            sec.pieces.push((t, piece));
        }
    }
    sec
}

/// Cut Section Between Locators: the section copied (returned), removed, and everything after
/// `to` moved left by its length; with `global` the global events too.
pub fn cut_section(p: &mut Project, from: u32, to: u32, only: Option<&[usize]>, global: bool, frames: Frames) -> Section {
    let sec = copy_section(p, from, to, only, frames);
    if sec.len == 0 {
        return sec;
    }
    for t in tracks_of(p, only) {
        let tr = &mut p.tracks[t];
        let mut extra = Vec::new();
        for r in tr.regions.iter_mut() {
            if let Some(right) = cut_at(r, from, frames) {
                extra.push(right);
            }
        }
        tr.regions.extend(extra);
        let mut extra = Vec::new();
        for r in tr.regions.iter_mut() {
            if let Some(right) = cut_at(r, to, frames) {
                extra.push(right);
            }
        }
        tr.regions.extend(extra);
        tr.regions.retain(|r| r.start < from || r.start >= to);
        for r in tr.regions.iter_mut().filter(|r| r.start >= to) {
            shift(r, -(sec.len as i64));
        }
    }
    if global {
        shift_global(p, from, -(sec.len as i64));
    }
    sec
}

/// Insert Section at Playhead: regions cut at `at`, one section length of time inserted, the
/// section's pieces pasted there.
pub fn insert_section(p: &mut Project, at: u32, sec: &Section, global: bool, frames: Frames) {
    if sec.len == 0 {
        return;
    }
    let only: Vec<usize> = (0..p.tracks.len()).collect();
    insert_silence(p, at, at + sec.len, Some(&only), global, frames);
    paste_section(p, at, sec);
}

/// The section's pieces pasted at `at` (on their tracks).
pub fn paste_section(p: &mut Project, at: u32, sec: &Section) {
    for (t, piece) in &sec.pieces {
        if let Some(tr) = p.tracks.get_mut(*t) {
            let mut r = piece.clone();
            shift(&mut r, at as i64);
            tr.regions.push(r);
        }
    }
}

/// Repeat Section Between Locators: the section copied and inserted at the right locator
/// (later material moved on).
pub fn repeat_section(p: &mut Project, from: u32, to: u32, only: Option<&[usize]>, global: bool, frames: Frames) {
    let sec = copy_section(p, from, to, only, frames);
    if sec.len == 0 {
        return;
    }
    let tracks = tracks_of(p, only);
    insert_silence(p, to, to + sec.len, Some(&tracks), global, frames);
    paste_section(p, to, &sec);
}

/// Sorts each track's regions by start (after section edits added pieces).
pub fn sort_regions(p: &mut Project) {
    for t in &mut p.tracks {
        t.regions.sort_by_key(|r| r.start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pos: u32, pitch: u8, len: u32) -> Note {
        Note { channel: 0, pitch, velocity: 100, pos, len, muted: false }
    }

    fn region(start: u32, length: u32, notes: Vec<Note>) -> Region {
        Region { name: "R".into(), start, length, notes, audio: None, muted: false, source: None, origin: None, props: RegionProps::default(), ext: Default::default() }
    }

    #[test]
    fn strength_and_range() {
        let mut p = RegionProps { quantize: 240, ..Default::default() };
        // 30 ticks late: full strength puts it on the grid
        assert_eq!(p.apply(&note(270, 60, 100), 0).pos, 240);
        p.q_strength = 50;
        assert_eq!(p.apply(&note(270, 60, 100), 0).pos, 255);
        // range 20 + strength 50: only notes more than 20 off move, halfway
        p.q_range = 20;
        assert_eq!(p.apply(&note(250, 60, 100), 0).pos, 250);
        assert_eq!(p.apply(&note(270, 60, 100), 0).pos, 255);
        // negative range: only notes within it
        p.q_range = -20;
        p.q_strength = 100;
        assert_eq!(p.apply(&note(250, 60, 100), 0).pos, 240);
        assert_eq!(p.apply(&note(270, 60, 100), 0).pos, 270);
    }

    #[test]
    fn flam_legato_clip() {
        let chord = vec![note(0, 60, 100), note(0, 64, 100), note(0, 67, 100), note(960, 72, 2000)];
        let p = RegionProps { q_flam: 10, ..Default::default() };
        let out = play_notes(&p, &chord, 0, 1920, &|_| 0);
        assert_eq!(out.iter().map(|n| (n.pos, n.pitch)).collect::<Vec<_>>()[..3], [(0, 60), (10, 64), (20, 67)]);
        let p = RegionProps { q_flam: -10, ..Default::default() };
        let out = play_notes(&p, &chord, 0, 1920, &|_| 0);
        assert!(out.contains(&note(0, 67, 100)) && out.contains(&note(20, 60, 100)), "downward: the top note first");
        let p = RegionProps { gate_time: GATE_LEGATO, ..Default::default() };
        let out = play_notes(&p, &[note(0, 60, 10), note(480, 62, 10)], 0, 1920, &|_| 0);
        assert_eq!((out[0].len, out[1].len), (480, 1440));
        let p = RegionProps { clip_length: true, ..Default::default() };
        let out = play_notes(&p, &chord, 0, 1920, &|_| 0);
        assert_eq!(out[3].len, 960);
        let out = play_notes(&RegionProps::default(), &chord, 0, 1920, &|_| 0);
        assert_eq!(out[3].len, 2000, "Clip Length off: notes play past the border");
    }

    #[test]
    fn pitch_source_follows_keys() {
        let mut pr = Project { key: "C major".into(), signature: (4, 4), ..Default::default() };
        pr.global.keys.push((4, "D major".into()));
        pr.global.keys.push((8, "A major".into()));
        assert_eq!(key_shift(&pr, 0), 0);
        assert_eq!(key_shift(&pr, 3840 * 4), 2);
        assert_eq!(key_shift(&pr, 3840 * 8), -3);
        let p = RegionProps { pitch_source: 1, ..Default::default() };
        let out = play_notes(&p, &[note(3840 * 4, 60, 10)], 0, 3840 * 8, &|t| key_shift(&pr, t));
        assert_eq!(out[0].pitch, 62);
        assert_eq!(key_root("F♯ minor"), Some(6));
        assert_eq!(key_root("Bb major"), Some(10));
    }

    #[test]
    fn length_halve_double_change() {
        let mut r = region(0, 3840, vec![note(0, 60, 100), note(1920, 62, 2000)]);
        double(&mut r);
        assert_eq!((r.length, r.notes.len()), (7680, 4));
        assert!(r.notes.iter().any(|n| n.pos == 3840 && n.pitch == 60));
        halve(&mut r);
        assert_eq!((r.length, r.notes.len()), (3840, 2));
        assert_eq!(r.notes[1].len, 1920, "a note across the new end is shortened");
        change_length(&mut r, 0.25);
        assert_eq!(r.length, 960);
        change_length(&mut r, 100.0);
        assert_eq!(r.length, 960 * 16, "the factor stops at 16");
    }

    #[test]
    fn join_normalizes() {
        let mut r = region(0, 3840, vec![note(0, 60, 100)]);
        r.props.transpose = 3;
        r.props.velocity = 10;
        normalize_for_join(&mut r);
        assert_eq!((r.notes[0].pitch, r.notes[0].velocity, r.props.transpose, r.props.velocity), (63, 110, 0, 0));
        let mut r = region(0, 960, vec![note(0, 60, 100)]);
        r.props.looped = true;
        unloop(&mut r, 3840);
        assert_eq!((r.length, r.notes.len(), r.props.looped), (3840, 4, false));
    }

    #[test]
    fn sections() {
        let bar = 3840;
        let mut p = Project { key: "C major".into(), signature: (4, 4), tempo: 120.0, ..Default::default() };
        let mut t = crate::project::Track::new("T", crate::project::TrackKind::Instrument);
        // a 16-bar region and an 8-bar region from bar 9 (0-based 8)
        t.regions.push(region(0, 16 * bar, vec![note(0, 60, 10), note(6 * bar, 62, 10), note(12 * bar, 64, 10)]));
        p.tracks.push(t);
        p.markers.push(crate::markers::Marker::new(10 * bar, "Chorus"));
        p.tempo_map = vec![(0, 120.0), (12 * bar, 90.0)];
        // Cut Section bars 5-9 (0-based 4..8)
        let sec = cut_section(&mut p, 4 * bar, 8 * bar, None, true, &|_, _| 0);
        sort_regions(&mut p);
        assert_eq!(sec.len, 4 * bar);
        assert_eq!(sec.pieces.len(), 1);
        assert_eq!(sec.pieces[0].1.notes[0].pos, 2 * bar, "the piece's notes from the section start");
        let rs: Vec<(u32, u32)> = p.tracks[0].regions.iter().map(|r| (r.start, r.length)).collect();
        assert_eq!(rs, vec![(0, 4 * bar), (4 * bar, 8 * bar)]);
        assert_eq!(p.markers[0].0, 6 * bar);
        assert_eq!(p.tempo_map[1].0, 8 * bar);
        // Insert Section at bar 3: cut there, 4 bars in, the piece pasted
        insert_section(&mut p, 2 * bar, &sec, true, &|_, _| 0);
        sort_regions(&mut p);
        let rs: Vec<(u32, u32)> = p.tracks[0].regions.iter().map(|r| (r.start, r.length)).collect();
        assert_eq!(rs, vec![(0, 2 * bar), (2 * bar, 4 * bar), (6 * bar, 2 * bar), (8 * bar, 8 * bar)]);
        assert_eq!(p.markers[0].0, 10 * bar);
        // Insert Silence and Repeat
        insert_silence(&mut p, 0, bar, None, false, &|_, _| 0);
        assert_eq!(p.markers[0].0, 10 * bar, "global events stay unless asked");
        sort_regions(&mut p);
        assert_eq!(p.tracks[0].regions[0].start, bar);
        repeat_section(&mut p, bar, 3 * bar, None, true, &|_, _| 0);
        sort_regions(&mut p);
        let rs: Vec<(u32, u32)> = p.tracks[0].regions.iter().map(|r| (r.start, r.length)).take(3).collect();
        assert_eq!(rs, vec![(bar, 2 * bar), (3 * bar, 2 * bar), (5 * bar, 4 * bar)]);
    }

    #[test]
    fn audio_cut_moves_the_file_offset() {
        let mut r = region(0, 3840, Vec::new());
        r.audio = Some(crate::project::AudioRef { file: "a.wav".into(), seconds: 2.0 });
        r.props.audio_offset = 100;
        let right = cut_at(&mut r, 960, &|a, b| (b - a) * 10).unwrap();
        assert_eq!((right.props.audio_offset, right.start, r.length), (100 + 9600, 960, 960));
    }
}
