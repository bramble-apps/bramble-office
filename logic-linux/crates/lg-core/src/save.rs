//! Saving back into Logic's own file: the original ProjectData with the edited parts rewritten,
//! everything else byte for byte as Logic wrote it.
//!
//! What's written: each region's notes (its event list rebuilt from its original records:
//! non-note records kept, notes written into copies of Logic's own note records, all in time
//! order, the end marker last); regions moved or deleted (their placement records in their
//! folder's event list) with their Region inspector values (Loop, Velocity, Transpose; audio
//! fades); regions made here: copies as new MSeq / Trak / EvSq chunks modelled on their
//! original, new MIDI regions modelled on one of the track's (or any) MIDI region, new audio
//! regions (recordings, imports) as AuRg (and AuFl for a new file) modelled on the project's own.
//! Then writeback.rs writes names, the mixer, plug-in settings, signature, key, tempo changes
//! and markers. What can't be written is listed (`unsaved`, `unsaved_automation`, `left_out`).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::chunks::{self, Chunk};
use crate::events::{self, Record, BAR_ONE};
use crate::project::{self, Project, Source};

#[derive(Debug, Default)]
pub struct Saved {
    pub data: Vec<u8>,
    /// Regions that couldn't be written (made in Bramble): their names.
    pub unsaved: Vec<String>,
    /// Automation that couldn't be written: tracks made here (no automation sequence of their
    /// own yet), plug-in parameter lanes, curves (Logic's curve bytes aren't decoded).
    pub unsaved_automation: Vec<String>,
    /// Everything else Save couldn't write (writeback.rs): new tracks, plug-in choices…
    pub left_out: Vec<String>,
}

/// Each track's automation sequence (EvSq id) by channel object: the "Track Automation Root
/// Folder" places one "*Automation" sequence per channel.
fn automation_sequences(chunks: &[Chunk]) -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    let folder = chunks.iter().filter(|c| !c.is("USEl") && !c.is("UStk") && c.is("MSeq") && chunks::short_string(c.payload, 16).as_deref() == Some("Track Automation Root Folder")).map(|c| c.id).last();
    let Some(f) = folder else { return out };
    let Some(fe) = chunks.iter().filter(|c| c.is("EvSq") && c.id == f).last() else { return out };
    for pl in events::records(fe.payload).iter().filter_map(events::placement) {
        if let Some(seq) = pl.sequence.map(|s| s << 16) {
            out.entry(pl.channel).or_insert(seq);
        }
    }
    out
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
fn rebuild_placements(payload: &[u8], moved: &HashMap<(u32, Option<u32>, u32), u32>, removed: &HashSet<(u32, Option<u32>, u32)>, props: &HashMap<(u32, Option<u32>, u32), project::RegionProps>) -> Vec<u8> {
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
            if let Some(pr) = props.get(&key) {
                set_props(&mut bytes, pr);
            }
        }
        out.push((pos, bytes));
    }
    out.sort_by_key(|(p, _)| *p);
    let mut bytes: Vec<u8> = out.into_iter().flat_map(|(_, b)| b).collect();
    bytes.extend_from_slice(&end);
    bytes
}

fn set_props(b: &mut [u8], pr: &project::RegionProps) {
    events::set_region_props(b, pr.looped, pr.velocity, pr.transpose, pr.fade_in, pr.fade_out);
}

/// The Region inspector values a placement record keeps.
fn kept_props(pr: &project::RegionProps) -> (bool, i8, i8, (u16, i8), (u16, i8)) {
    (pr.looped, pr.velocity, pr.transpose, pr.fade_in, pr.fade_out)
}

/// A read MIDI region to model a new one on: one of the track's own, else any in the top folder.
fn midi_model(before: &Project, channel: u32) -> Option<Source> {
    let all = || before.tracks.iter().flat_map(|t| t.regions.iter().map(move |r| (t, r))).filter(|(_, r)| r.audio.is_none()).filter_map(|(t, r)| Some((t.channel, r.source?))).filter(|(_, s)| s.seq.is_some());
    all().find(|(c, s)| *c == channel && s.folder == 0x40000).or_else(|| all().find(|(_, s)| s.folder == 0x40000)).map(|x| x.1)
}

/// An event list with only its end marker.
fn only_end(payload: &[u8]) -> Vec<u8> {
    payload.chunks_exact(16).rev().find(|u| u[0] == 0xF1).map(|u| u.to_vec()).unwrap_or_default()
}

/// A WAV file's format: (frames, rate, channels, bits, data offset).
fn wav_format(path: &std::path::Path) -> Option<(u32, u32, u16, u16, u32)> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut rate, mut ch, mut bits) = (0u32, 0u16, 0u16);
    let mut p = 12;
    while p + 8 <= b.len() {
        let size = u32::from_le_bytes(b[p + 4..p + 8].try_into().ok()?) as usize;
        if &b[p..p + 4] == b"fmt " && p + 24 <= b.len() {
            ch = u16::from_le_bytes(b[p + 10..p + 12].try_into().ok()?);
            rate = u32::from_le_bytes(b[p + 12..p + 16].try_into().ok()?);
            bits = u16::from_le_bytes(b[p + 22..p + 24].try_into().ok()?);
        }
        if &b[p..p + 4] == b"data" && ch > 0 && bits > 0 {
            let frames = size / (ch as usize * bits.div_ceil(8) as usize);
            return Some((frames as u32, rate, ch, bits, (p + 8) as u32));
        }
        p += 8 + size + (size & 1);
    }
    None
}

/// New audio regions (recordings, imports, copies): an AuRg each (and an AuFl for a file the
/// project doesn't have yet), modelled on the project's own, and a placement modelled on one
/// of its audio placements. Without a model (no audio in the project yet) they're listed.
fn new_audio_regions(p: &Project, chunks: &[Chunk], before: &Project, regions: &[(&project::Region, (u32, u32))], after: &mut HashMap<usize, Vec<Vec<u8>>>, placements: &mut HashMap<u32, Vec<(u32, Vec<u8>)>>, unsaved: &mut Vec<String>) {
    if regions.is_empty() {
        return;
    }
    let live = |c: &&Chunk| !c.is("USEl") && !c.is("UStk");
    let idx_of = |tag: &str| -> Vec<usize> { (0..chunks.len()).filter(|&i| chunks[i].is(tag) && live(&&chunks[i])).collect() };
    let (files, aurgs) = (idx_of("AuFl"), idx_of("AuRg"));
    // a placement to copy: an audio region's record in its folder
    let model = before.tracks.iter().flat_map(|t| t.regions.iter()).filter(|r| r.audio.is_some()).find_map(|r| {
        let s = r.source?;
        let ev = chunks.iter().filter(|c| c.is("EvSq") && c.id == s.folder).last()?;
        events::records(ev.payload).into_iter().find(|x| x.kind == 0x24 && x.pos == s.pos).map(|x| (s.folder, x.bytes))
    });
    let (Some(&model_file), Some(&model_rg), Some((folder, model_pl))) = (files.last(), aurgs.last(), model) else {
        unsaved.extend(regions.iter().map(|(r, _)| r.name.clone()));
        return;
    };
    // (files and regions by the placement's numbers: header id >> 16, AuRg u32 @12 >> 16)
    let mut ids: HashMap<String, u32> = files.iter().filter_map(|&i| Some((project::audio_file_name(chunks[i].payload)?, chunks[i].id >> 16))).collect();
    let mut next_file = files.iter().map(|&i| chunks[i].id >> 16).max().unwrap_or(0) + 4;
    let mut next_index: HashMap<u32, u32> = HashMap::new();
    for &i in &aurgs {
        let n = next_index.entry(chunks[i].id >> 16).or_insert(0);
        *n = (*n).max((chunks[i].u32_at(12).unwrap_or(0) >> 16) + 1);
    }
    let mut rates: HashMap<u32, u32> = files.iter().filter_map(|&i| Some((chunks[i].id >> 16, project::audio_file_format(chunks[i].payload)?.1))).collect();
    for (k, (r, (channel, row))) in regions.iter().enumerate() {
        let Some(a) = &r.audio else { continue };
        let name = a.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // the file: the project's, else a new AuFl after the last AuRg
        let file = match ids.get(&name) {
            Some(&f) => f,
            None => {
                let Some((frames, rate, ch, bits, data_at)) = wav_format(&a.file) else {
                    unsaved.push(format!("{} (only WAV files are written)", r.name));
                    continue;
                };
                let m = &chunks[model_file];
                let n = u16::from_le_bytes([m.payload[8], m.payload[9]]) as usize;
                let rest = &m.payload[10 + 2 * n..];
                let mut pl = m.payload[..8].to_vec();
                let units: Vec<u16> = name.encode_utf16().collect();
                pl.extend_from_slice(&(units.len() as u16).to_le_bytes());
                for u in &units {
                    pl.extend_from_slice(&u.to_le_bytes());
                }
                let e = pl.len();
                pl.extend_from_slice(rest);
                if pl.len() < e + 484 {
                    unsaved.push(r.name.clone());
                    continue;
                }
                pl[e + 456..e + 460].copy_from_slice(b"EVAW");
                pl[e + 464..e + 468].copy_from_slice(&data_at.to_le_bytes());
                pl[e + 468..e + 472].copy_from_slice(&frames.to_le_bytes());
                pl[e + 476..e + 480].copy_from_slice(&rate.to_le_bytes());
                pl[e + 480..e + 482].copy_from_slice(&ch.to_le_bytes());
                pl[e + 482..e + 484].copy_from_slice(&bits.to_le_bytes());
                let id = next_file;
                next_file += 4;
                after.entry(*aurgs.last().unwrap_or(&model_rg)).or_default().push(chunk_like(m.raw, id << 16, None, pl));
                ids.insert(name.clone(), id);
                rates.insert(id, rate);
                id
            }
        };
        // the region: a copy of an AuRg (the file's own if it has one), its window and name
        let model_r = aurgs.iter().rev().copied().find(|&i| chunks[i].id >> 16 == file).unwrap_or(model_rg);
        let m = &chunks[model_r];
        let index = *next_index.get(&file).unwrap_or(&0);
        next_index.insert(file, index + 1);
        let mut pl = m.payload.to_vec();
        if pl.len() < 0x32 {
            unsaved.push(r.name.clone());
            continue;
        }
        let rate = rates.get(&file).copied().unwrap_or(p.sample_rate.max(1));
        let samples = (r.length as f64 / events::PPQ as f64 * 60.0 / p.tempo.max(1.0) * rate as f64).round() as u32;
        pl[6..10].copy_from_slice(&r.props.audio_offset.to_le_bytes());
        pl[0x16..0x1A].copy_from_slice(&samples.to_le_bytes());
        // (its per-object stamp, @0x2A: made different from the model's)
        let stamp = (r.start as u64) << 20 ^ (k as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ index as u64;
        for (j, b) in pl[0x2A..0x32].iter_mut().enumerate() {
            *b ^= (stamp >> (8 * j)) as u8 | 1;
        }
        if let Some(off) = (0..pl.len().saturating_sub(4).min(160)).find(|&o| {
            let n = u16::from_le_bytes([pl[o], pl[o + 1]]) as usize;
            (2..=200).contains(&n) && pl.get(o + 2..o + 2 + n).is_some_and(|s| s.iter().all(|b| (0x20..0x7f).contains(b)))
        }) {
            if let Some(n) = crate::writeback::set_short_string(&pl, off, &r.name) {
                pl = n;
            }
        }
        let anchor = if aurgs.iter().any(|&i| chunks[i].id >> 16 == file) { model_r } else { *aurgs.last().unwrap_or(&model_rg) };
        after.entry(anchor).or_default().push(chunk_like(m.raw, file << 16, Some(index << 16), pl));
        // the placement
        let mut b = model_pl.clone();
        let pos = (r.start + BAR_ONE).to_le_bytes();
        b[2] = 0;
        b[3] = 0;
        b[4..7].copy_from_slice(&pos[..3]);
        b[7] = pos[3] & 0x7F;
        b[16..20].copy_from_slice(&channel.to_le_bytes());
        b[20..22].copy_from_slice(&(*row as u16).to_le_bytes());
        b[40..44].copy_from_slice(&index.to_le_bytes());
        b[44..48].copy_from_slice(&file.to_le_bytes());
        set_props(&mut b, &r.props);
        placements.entry(folder).or_default().push((r.start + BAR_ONE, b));
    }
}

/// A chunk like `raw` (its header) with another id (and the u32 @12), and this payload.
fn chunk_like(raw: &[u8], id: u32, at12: Option<u32>, payload: Vec<u8>) -> Vec<u8> {
    let mut h = raw[..36].to_vec();
    h[8..12].copy_from_slice(&id.to_le_bytes());
    if let Some(v) = at12 {
        h[12..16].copy_from_slice(&v.to_le_bytes());
    }
    h[28..36].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    h.extend(payload);
    h
}

/// The project written into `original` (the ProjectData it was read from).
pub fn save(p: &Project, original: &[u8]) -> Result<Saved, chunks::Error> {
    let chunks = chunks::walk(original)?;
    let mut before = project::read_project_data(original).map_err(|_| chunks::Error::NotProjectData)?;
    // (as the app opened it: audio regions' lengths from their files where the project has none)
    if p.audio_dir.is_some() {
        before.audio_dir = p.audio_dir.clone();
        project::attach_audio(&mut before);
    }
    let mut unsaved = Vec::new();
    // regions now, by where they came from
    let mut now: HashMap<Source, &project::Region> = HashMap::new();
    // regions made here from a read one (copies, pastes): written as new sequences
    // (with the track it goes on: channel object, row; new MIDI regions with only their notes)
    let mut new_regions: Vec<(&project::Region, Source, (u32, u32), bool)> = Vec::new();
    let mut new_audio: Vec<(&project::Region, (u32, u32))> = Vec::new();
    for t in &p.tracks {
        let at = (t.channel, t.row);
        for r in &t.regions {
            match (r.source, r.origin) {
                (Some(s), _) if !now.contains_key(&s) => {
                    now.insert(s, r);
                }
                _ if t.channel == 0 => unsaved.push(r.name.clone()),
                (_, Some(o)) if o.seq.is_some() && r.audio.is_none() => new_regions.push((r, o, at, false)),
                _ if r.audio.is_some() => new_audio.push((r, at)),
                _ => match midi_model(&before, t.channel) {
                    Some(o) => new_regions.push((r, o, at, true)),
                    None => unsaved.push(r.name.clone()),
                },
            }
        }
    }
    let was: Vec<Source> = before.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| r.source).collect();
    // per folder: moves and removals; per sequence: notes
    let mut moved: BTreeMap<u32, HashMap<(u32, Option<u32>, u32), u32>> = BTreeMap::new();
    let mut removed: BTreeMap<u32, HashSet<(u32, Option<u32>, u32)>> = BTreeMap::new();
    let mut notes: HashMap<u32, Vec<events::Note>> = HashMap::new();
    let mut props: BTreeMap<u32, HashMap<(u32, Option<u32>, u32), project::RegionProps>> = BTreeMap::new();
    let before_regions: HashMap<Source, &project::Region> = before.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| Some((r.source?, r))).collect();
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
                if before_regions.get(s).is_some_and(|b| kept_props(&b.props) != kept_props(&r.props)) {
                    props.entry(s.folder).or_default().insert(key, r.props);
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
    // new sequences: ids after the highest in use; each a copy of its model's MSeq / Trak / EvSq
    // chunks (the last of each id), inserted after the model's EvSq; a placement record (a copy
    // of the model's) added to the folder
    let mut next_id = chunks.iter().filter(|c| c.is("MSeq") || c.is("Trak") || c.is("EvSq")).map(|c| c.id & 0xFFFF_0000).max().unwrap_or(0) + 0x4_0000;
    let mut after_evsq: HashMap<usize, Vec<Vec<u8>>> = HashMap::new();
    let mut added_placements: HashMap<u32, Vec<(u32, Vec<u8>)>> = HashMap::new();
    for (r, o, (channel, row), fresh) in &new_regions {
        let seq = o.seq.unwrap_or(0);
        let find = |tag: &str| chunks.iter().enumerate().filter(|(_, c)| c.is(tag) && c.id == seq).last();
        let (Some((_, m)), Some((_, t)), Some((ei, e))) = (find("MSeq"), find("Trak"), find("EvSq")) else {
            unsaved.push(r.name.clone());
            continue;
        };
        let Some(folder_ev) = chunks.iter().filter(|c| c.is("EvSq") && c.id == o.folder).last() else {
            unsaved.push(r.name.clone());
            continue;
        };
        let Some(model_pl) = events::records(folder_ev.payload).into_iter().find(|x| events::placement(x).is_some_and(|pl| pl.pos == o.pos && pl.sequence.map(|s| s << 16) == o.seq)) else {
            unsaved.push(r.name.clone());
            continue;
        };
        let id = next_id;
        next_id += 0x4_0000;
        let with_id = |raw: &[u8], payload: Option<Vec<u8>>| {
            let mut h = raw[..36].to_vec();
            h[8..12].copy_from_slice(&id.to_le_bytes());
            let pl = payload.unwrap_or_else(|| raw[36..].to_vec());
            h[28..36].copy_from_slice(&(pl.len() as u64).to_le_bytes());
            h.extend(pl);
            h
        };
        let list = after_evsq.entry(ei).or_default();
        let named = crate::writeback::set_short_string(m.payload, 16, &r.name).filter(|_| !r.name.is_empty());
        list.push(with_id(m.raw, named));
        list.push(with_id(t.raw, None));
        // (a new region: the model's notes and controllers left behind)
        let events_from = if *fresh { only_end(e.payload) } else { e.payload.to_vec() };
        list.push(with_id(e.raw, Some(rebuild_notes(&events_from, &r.notes))));
        let mut pl = model_pl.bytes.clone();
        let pos = (r.start + BAR_ONE).to_le_bytes();
        pl[4..7].copy_from_slice(&pos[..3]);
        pl[7] = pos[3] & 0x7F;
        pl[32..36].copy_from_slice(&(id >> 16).to_le_bytes());
        // on the track it's on now (a copy may have been pasted on another)
        pl[16..20].copy_from_slice(&channel.to_le_bytes());
        pl[20..22].copy_from_slice(&(*row as u16).to_le_bytes());
        set_props(&mut pl, &r.props);
        added_placements.entry(o.folder).or_default().push((r.start + BAR_ONE, pl));
    }
    new_audio_regions(p, &chunks, &before, &new_audio, &mut after_evsq, &mut added_placements, &mut unsaved);
    // track automation: each channel's sequence rewritten when its lanes changed
    let mut unsaved_automation = Vec::new();
    let auto_seqs = automation_sequences(&chunks);
    let mut auto_lanes: HashMap<u32, Vec<project::AutoLane>> = HashMap::new();
    for t in &p.tracks {
        let was = before.tracks.iter().find(|b| b.channel == t.channel && t.channel != 0).map_or(&[][..], |b| b.automation.as_slice());
        if t.automation.iter().any(|l| l.plugin.is_some() && !l.points.is_empty()) {
            unsaved_automation.push(format!("plug-in automation on {}", t.name));
        }
        if t.automation.iter().any(|l| l.plugin.is_none() && !l.curves.is_empty()) {
            unsaved_automation.push(format!("automation curves on {}", t.name));
        }
        if !crate::automation::changed(was, &t.automation) {
            continue;
        }
        match auto_seqs.get(&t.channel).filter(|_| t.channel != 0) {
            Some(&seq) => {
                auto_lanes.entry(seq).or_insert_with(|| t.automation.clone());
            }
            None => unsaved_automation.push(format!("automation on {}", t.name)),
        }
    }
    let unchanged = |c: &Chunk| c.raw.to_vec();
    let mut body: Vec<u8> = Vec::with_capacity(original.len());
    for (i, c) in chunks.iter().enumerate() {
        let live = c.is("EvSq") && last.get(&c.id) == Some(&i);
        let new_payload = if live && auto_lanes.contains_key(&c.id) {
            Some(crate::automation::write_lanes(c.payload, &auto_lanes[&c.id]))
        } else if live && notes.contains_key(&c.id) {
            let old_notes: Vec<events::Note> = events::records(c.payload).iter().filter_map(events::note).collect();
            let want: Vec<events::Note> = notes[&c.id].clone();
            let same = old_notes.len() == want.len() && old_notes.iter().zip(&want).all(|(a, b)| a.pitch == b.pitch && a.velocity == b.velocity && a.len == b.len && a.pos == b.pos + BAR_ONE);
            (!same).then(|| rebuild_notes(c.payload, &want))
        } else if live && (moved.contains_key(&c.id) || removed.contains_key(&c.id) || added_placements.contains_key(&c.id) || props.contains_key(&c.id)) {
            let mut pl = rebuild_placements(c.payload, moved.get(&c.id).unwrap_or(&HashMap::new()), removed.get(&c.id).unwrap_or(&HashSet::new()), props.get(&c.id).unwrap_or(&HashMap::new()));
            if let Some(add) = added_placements.get(&c.id) {
                // the new placements in among the others, in time order, before the end marker
                let end = pl.split_off(pl.len().saturating_sub(16));
                let mut recs: Vec<(u32, Vec<u8>)> = events::records(&pl).into_iter().map(|r| (r.pos, r.bytes)).collect();
                recs.extend(add.iter().cloned());
                recs.sort_by_key(|x| x.0);
                pl = recs.into_iter().flat_map(|x| x.1).collect();
                pl.extend(end);
            }
            Some(pl)
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
        if let Some(extra) = after_evsq.get(&i) {
            for x in extra {
                body.extend(x);
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
    let (data, left_out) = crate::writeback::apply(p, &before, data);
    Ok(Saved { data, unsaved, unsaved_automation, left_out })
}

/// The alternative's MetaData.plist summary follows the song (Logic and Finder read it): tempo,
/// signature, key (format-analysis §18) and the audio files the regions use.
pub fn update_metadata(alt: &std::path::Path, p: &Project) {
    let path = alt.join("MetaData.plist");
    let Ok(plist::Value::Dictionary(mut d)) = plist::Value::from_file(&path) else { return };
    let before = d.clone();
    let num = |v: &plist::Value| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64));
    if d.get("BeatsPerMinute").and_then(num).is_some_and(|w| (w - p.tempo).abs() > 1e-6) {
        d.insert("BeatsPerMinute".into(), plist::Value::Real(p.tempo));
    }
    let int = |d: &plist::Dictionary, k: &str| d.get(k).and_then(|v| v.as_signed_integer());
    if int(&d, "SongSignatureNumerator").is_some_and(|n| n != p.signature.0 as i64) {
        d.insert("SongSignatureNumerator".into(), plist::Value::Integer((p.signature.0 as i64).into()));
    }
    if int(&d, "SongSignatureDenominator").is_some_and(|n| n != p.signature.1 as i64) {
        d.insert("SongSignatureDenominator".into(), plist::Value::Integer((p.signature.1 as i64).into()));
    }
    if let Some(k) = events::key_byte(&p.key) {
        let s = events::key_string(k);
        let mut parts = s.split(' ');
        let (root, gender) = (parts.next().unwrap_or("C").replace('\u{266d}', "b").replace('\u{266f}', "#"), parts.next().unwrap_or("major").to_string());
        if d.contains_key("SignatureKey") && int(&d, "SignatureKey") != Some(k as i64) {
            d.insert("SignatureKey".into(), plist::Value::Integer((k as i64).into()));
        }
        if d.get("SongKey").and_then(|v| v.as_string()).is_some_and(|v| v != root) {
            d.insert("SongKey".into(), plist::Value::String(root));
        }
        if d.get("SongGenderKey").and_then(|v| v.as_string()).is_some_and(|v| v != gender) {
            d.insert("SongGenderKey".into(), plist::Value::String(gender));
        }
    }
    // (package-relative: "Audio Files/<name>")
    if let Some(plist::Value::Array(list)) = d.get_mut("AudioFiles") {
        let have: Vec<String> = list.iter().filter_map(|v| v.as_string().map(str::to_string)).collect();
        let mut add: Vec<String> = Vec::new();
        for r in p.tracks.iter().flat_map(|t| t.regions.iter()) {
            if let Some(n) = r.audio.as_ref().and_then(|a| a.file.file_name()).map(|n| format!("Audio Files/{}", n.to_string_lossy())) {
                if !have.contains(&n) && !add.contains(&n) {
                    add.push(n);
                }
            }
        }
        list.extend(add.into_iter().map(plist::Value::String));
    }
    if d != before {
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

    /// The research's sample projects, when this checkout has them (testdata-private, or the
    /// main checkout's).
    fn sample(name: &str) -> Option<Vec<u8>> {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files");
        let main = std::path::PathBuf::from(std::env::var("HOME").ok()?).join("CascadeProjects/bramble-office/logic-linux/testdata-private/sample-files");
        [here, main].iter().map(|d| d.join(name).join("Alternatives/000/ProjectData")).find(|p| p.exists()).and_then(|p| std::fs::read(p).ok())
    }

    #[test]
    fn automation_is_written_back_and_read_again() {
        let Some(orig) = sample("E08-volume-automation.logicx") else { return };
        let p = project::read_project_data(&orig).unwrap();
        assert_eq!(save(&p, &orig).unwrap().data, orig, "unchanged: the same bytes");
        let t = p.tracks.iter().position(|t| !t.automation.is_empty()).expect("E08 has volume automation");
        let mut e = p.clone();
        {
            let lane = &mut e.tracks[t].automation[0];
            lane.points[1].1 = 40;
            lane.points.push((lane.points[1].0 + 1920, 100));
        }
        e.tracks[t].automation.push(project::AutoLane { param: 10, points: vec![(0, 64), (3840, 20)], ..Default::default() });
        let saved = save(&e, &orig).unwrap();
        assert!(saved.unsaved_automation.is_empty(), "{:?}", saved.unsaved_automation);
        let back = project::read_project_data(&saved.data).unwrap();
        let mut want = e.tracks[t].automation.clone();
        let mut got = back.tracks[t].automation.clone();
        want.sort_by_key(|l| l.param);
        got.sort_by_key(|l| l.param);
        assert_eq!(got, want);
        // the rest as it was
        assert_eq!(back.tracks.len(), p.tracks.len());
        assert_eq!(back.tracks[t].regions.len(), p.tracks[t].regions.len());
        // a second track's first automation goes into its own (empty) sequence
        if let Some(u) = (0..e.tracks.len()).find(|&i| i != t && e.tracks[i].channel != 0) {
            let mut e2 = e.clone();
            e2.tracks[u].automation = vec![project::AutoLane { param: 7, points: vec![(0, 90), (7680, 60)], ..Default::default() }];
            let s2 = save(&e2, &orig).unwrap();
            let b2 = project::read_project_data(&s2.data).unwrap();
            if s2.unsaved_automation.is_empty() {
                assert_eq!(b2.tracks[u].automation, e2.tracks[u].automation);
            }
        }
    }
}
