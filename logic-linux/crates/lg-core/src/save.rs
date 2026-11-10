//! Saving back into Logic's own file: the original ProjectData with the edited parts rewritten,
//! everything else byte for byte as Logic wrote it.
//!
//! What's written: each region's notes (its event list rebuilt from its original records:
//! non-note records kept, notes written into copies of Logic's own note records, all in time
//! order, the end marker last); regions moved or deleted (their placement records in their
//! folder's event list). Regions made here (copies, pastes, new tracks) aren't written yet: Logic
//! needs new MSeq / Trak / EvSq chunks for them — `unsaved` lists them.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::chunks::{self, Chunk};
use crate::events::{self, Record, BAR_ONE};
use crate::project::{self, Project, Source};

#[derive(Debug, Default)]
pub struct Saved {
    pub data: Vec<u8>,
    /// Regions that couldn't be written (made in Bramble): their names.
    pub unsaved: Vec<String>,
}

/// A note record from a template (Logic's own record for that note, or the region's first, or a
/// plain 2-unit one), with the note's values written in.
fn note_record(template: Option<&Record>, n: &events::Note) -> Vec<u8> {
    let mut b = match template {
        Some(t) if t.bytes.len() >= 32 => t.bytes.clone(),
        _ => {
            let mut b = vec![0u8; 32];
            b[23] = 0x89;
            b[16] = 0x40;
            b
        }
    };
    b[0] = 0x90 | (n.channel & 0x0F);
    let pos = (n.pos + BAR_ONE).to_le_bytes();
    b[4..7].copy_from_slice(&pos[..3]);
    b[7] = pos[3] & 0x7F;
    b[11] = n.velocity;
    b[12] = n.pitch;
    b[28..32].copy_from_slice(&n.len.to_le_bytes());
    b
}

/// A region's event list with `notes` in place of its original notes.
fn rebuild_notes(payload: &[u8], notes: &[events::Note]) -> Vec<u8> {
    let records = events::records(payload);
    let end: Vec<u8> = payload.chunks_exact(16).rev().find(|u| u[0] == 0xF1).map(|u| u.to_vec()).unwrap_or_else(|| {
        let mut u = vec![0u8; 16];
        u[0] = 0xF1;
        u[4..8].copy_from_slice(&events::END.to_le_bytes());
        u
    });
    let orig_notes: Vec<&Record> = records.iter().filter(|r| events::note(r).is_some()).collect();
    let mut out: Vec<(u32, Vec<u8>)> = records.iter().filter(|r| events::note(r).is_none()).map(|r| (r.pos, r.bytes.clone())).collect();
    for (i, n) in notes.iter().enumerate() {
        let t = orig_notes.get(i).or(orig_notes.first()).copied();
        out.push((n.pos + BAR_ONE, note_record(t, n)));
    }
    out.sort_by_key(|(p, _)| *p);
    let mut bytes: Vec<u8> = out.into_iter().flat_map(|(_, b)| b).collect();
    bytes.extend_from_slice(&end);
    bytes
}

/// A folder's event list with its placements moved (to `moved[(pos, seq)]`) or removed.
fn rebuild_placements(payload: &[u8], moved: &HashMap<(u32, Option<u32>, u32), u32>, removed: &HashSet<(u32, Option<u32>, u32)>) -> Vec<u8> {
    let records = events::records(payload);
    let end: Vec<u8> = payload.chunks_exact(16).rev().find(|u| u[0] == 0xF1).map(|u| u.to_vec()).unwrap_or_default();
    let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
    for r in records {
        let mut bytes = r.bytes.clone();
        let mut pos = r.pos;
        if let Some(pl) = events::placement(&r) {
            let seq = pl.sequence.map(|s| s << 16);
            let key = (pl.pos, seq, pl.audio_region);
            if removed.contains(&key) {
                continue;
            }
            if let Some(&to) = moved.get(&key) {
                pos = to;
                let b = to.to_le_bytes();
                bytes[4..7].copy_from_slice(&b[..3]);
                bytes[7] = b[3] & 0x7F;
            }
        }
        out.push((pos, bytes));
    }
    out.sort_by_key(|(p, _)| *p);
    let mut bytes: Vec<u8> = out.into_iter().flat_map(|(_, b)| b).collect();
    bytes.extend_from_slice(&end);
    bytes
}

/// The project written into `original` (the ProjectData it was read from).
pub fn save(p: &Project, original: &[u8]) -> Result<Saved, chunks::Error> {
    let chunks = chunks::walk(original)?;
    let before = project::read_project_data(original).map_err(|_| chunks::Error::NotProjectData)?;
    let mut unsaved = Vec::new();
    // regions now, by where they came from
    let mut now: HashMap<Source, &project::Region> = HashMap::new();
    for r in p.tracks.iter().flat_map(|t| t.regions.iter()) {
        match r.source {
            Some(s) if !now.contains_key(&s) => {
                now.insert(s, r);
            }
            _ => unsaved.push(r.name.clone()),
        }
    }
    let was: Vec<Source> = before.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| r.source).collect();
    // per folder: moves and removals; per sequence: notes
    let mut moved: BTreeMap<u32, HashMap<(u32, Option<u32>, u32), u32>> = BTreeMap::new();
    let mut removed: BTreeMap<u32, HashSet<(u32, Option<u32>, u32)>> = BTreeMap::new();
    let mut notes: HashMap<u32, Vec<events::Note>> = HashMap::new();
    for s in &was {
        let key = (s.pos, s.seq, s.audio_region);
        match now.get(s) {
            None => {
                removed.entry(s.folder).or_default().insert(key);
            }
            Some(r) => {
                if r.start + BAR_ONE != s.pos {
                    moved.entry(s.folder).or_default().insert(key, r.start + BAR_ONE);
                }
                if let Some(seq) = s.seq {
                    if r.audio.is_none() {
                        notes.insert(seq, r.notes.clone());
                    }
                }
            }
        }
    }
    // the last EvSq of each id is the live one (as the reader takes it)
    let mut last: HashMap<u32, usize> = HashMap::new();
    for (i, c) in chunks.iter().enumerate() {
        if c.is("EvSq") {
            last.insert(c.id, i);
        }
    }
    let unchanged = |c: &Chunk| c.raw.to_vec();
    let mut body: Vec<u8> = Vec::with_capacity(original.len());
    for (i, c) in chunks.iter().enumerate() {
        let live = c.is("EvSq") && last.get(&c.id) == Some(&i);
        let new_payload = if live && notes.contains_key(&c.id) {
            let old_notes: Vec<events::Note> = events::records(c.payload).iter().filter_map(events::note).collect();
            let want: Vec<events::Note> = notes[&c.id].clone();
            let same = old_notes.len() == want.len() && old_notes.iter().zip(&want).all(|(a, b)| a.pitch == b.pitch && a.velocity == b.velocity && a.len == b.len && a.pos == b.pos + BAR_ONE);
            (!same).then(|| rebuild_notes(c.payload, &want))
        } else if live && (moved.contains_key(&c.id) || removed.contains_key(&c.id)) {
            Some(rebuild_placements(c.payload, moved.get(&c.id).unwrap_or(&HashMap::new()), removed.get(&c.id).unwrap_or(&HashSet::new())))
        } else {
            None
        };
        match new_payload {
            None => body.extend(unchanged(c)),
            Some(pl) => {
                let mut h = c.raw[..36].to_vec();
                h[28..36].copy_from_slice(&(pl.len() as u64).to_le_bytes());
                body.extend(h);
                body.extend(pl);
            }
        }
    }
    // the tempo: the Song chunk's three copies and the tempo list's first event (BPM × 10000),
    // when it changed (a single tempo; tempo changes aren't written yet)
    let tempo_now = (p.tempo * 10000.0).round() as u32;
    let tempo_was = (before.tempo * 10000.0).round() as u32;
    let mut data = original[..24].to_vec();
    data[0x10..0x14].copy_from_slice(&(body.len() as u32).to_le_bytes());
    data.extend(body);
    if tempo_now != tempo_was && tempo_now > 0 {
        set_tempo(&mut data, tempo_was, tempo_now);
    }
    Ok(Saved { data, unsaved })
}

/// The alternative's MetaData.plist summary follows the song: its tempo (Logic and Finder read it).
pub fn update_metadata(alt: &std::path::Path, p: &Project) {
    let path = alt.join("MetaData.plist");
    let Ok(plist::Value::Dictionary(mut d)) = plist::Value::from_file(&path) else { return };
    let was = d.get("BeatsPerMinute").and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64)));
    if was.is_some_and(|w| (w - p.tempo).abs() > 1e-6) {
        d.insert("BeatsPerMinute".into(), plist::Value::Real(p.tempo));
        let _ = plist::Value::Dictionary(d).to_file_binary(&path);
    }
}

/// Writes a new tempo (BPM × 10000) where the old one sits: the Song chunk's three fields (0x92,
/// 0xEA, 0x3A6 from its start) and the tempo list's events (second EvSq id 0, kind 0x60).
fn set_tempo(data: &mut [u8], was: u32, now: u32) {
    let Ok(chunks) = chunks::walk(data) else { return };
    let mut spots = Vec::new();
    if let Some(song) = chunks.iter().find(|c| c.is("Song")) {
        for off in [0x92usize, 0xEA, 0x3A6] {
            if song.u32_at(off) == Some(was) {
                spots.push(song.start + off);
            }
        }
    }
    if let Some(list) = chunks.iter().filter(|c| c.is("EvSq") && c.id == 0).nth(1) {
        let base = list.start + 36;
        let mut i = 0;
        while i + 32 <= list.payload.len() {
            let u = &list.payload[i..];
            if u[0] == 0x60 && u32::from_le_bytes([u[16], u[17], u[18], u[19]]) == was {
                spots.push(base + i + 16);
            }
            i += 16;
        }
    }
    for at in spots {
        data[at..at + 4].copy_from_slice(&now.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pitch: u8, pos: u32) -> events::Note {
        events::Note { channel: 0, pitch, velocity: 90, pos, len: 240 }
    }

    #[test]
    fn note_records_carry_the_values() {
        let b = note_record(None, &note(64, 960));
        let r = events::records(&b);
        assert_eq!(r.len(), 1, "one record of two units");
        assert_eq!(events::note(&r[0]), Some(events::Note { channel: 0, pitch: 64, velocity: 90, pos: 960 + BAR_ONE, len: 240 }));
    }

    #[test]
    fn rebuilt_notes_are_in_time_order_with_the_end_marker_last() {
        let mut payload = note_record(None, &note(60, 0));
        let mut end = vec![0u8; 16];
        end[0] = 0xF1;
        end[4..8].copy_from_slice(&events::END.to_le_bytes());
        payload.extend(&end);
        let out = rebuild_notes(&payload, &[note(67, 1920), note(62, 480)]);
        let notes: Vec<_> = events::records(&out).iter().filter_map(events::note).map(|n| (n.pitch, n.pos - BAR_ONE)).collect();
        assert_eq!(notes, vec![(62, 480), (67, 1920)]);
        assert_eq!(&out[out.len() - 16..], &end[..]);
    }
}
