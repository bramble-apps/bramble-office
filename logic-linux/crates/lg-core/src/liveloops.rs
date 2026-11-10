//! Logic's Live Loops grid in ProjectData (LOGICX-FORMAT.txt §9 / 9b, checked again on
//! E23-live-loops-base and the "Chromium Fray" template):
//!
//! - `Grid` (one per project, 36-byte payload): u32 @+16 = stored SCENES, u32 @+20 = stored ROWS
//!   (the research notes had these the other way round: the "Chromium Fray" template, 13 × 33,
//!   settles it — its 33 rows are its 33 tracks, each row's cells on one channel), u32 @+32
//!   unknown (0xFFFFFFFF in E23, 1 in the template).
//! - `GAdd` chunks, kind = i32 at chunk offset 14, index = u32 at 18:
//!   -2 a scene (14 bytes; one more with index 0xFFFFFFFF ends the list; u32 @+8 = 1 on the first),
//!   -3 a cell slot (5 bytes: u32 clip reference or 0xFFFFFFFF = empty, u8 flag),
//!   -4 a row = a track (20 bytes: u32 its channel object, Envi id >> 16, as a region placement's
//!   channel; then the track's UUID). Cells are row-major: index = row × scenes + scene.
//! - `Clip` (85-byte payload): the cell's clip; its chunk id >> 16 is what a cell slot refers to.
//!   u32 @+16 = the cell's own sequence (MSeq / EvSq id >> 16). That sequence's event list holds
//!   one placement record (0x20 MIDI / 0x24 audio, as a folder's) pointing at the sequence with
//!   the notes, and naming the track's channel object and row. Cells are copies: their notes live
//!   in their own sequence, never shared with a timeline region.
//! - Inside a sequence notes count from 38400 (bar 1) while placements count from 34560: a note
//!   at 38400 in a cell placed at 34560 sounds at the cell's start.
//!
//! Not yet decoded (no sample varies them): where the cell's play mode, Quantize Start, Trigger
//! Mode and Play From live. `ClipParams` keeps the Clip bytes that do vary between cells (a float
//! at +28 that looks like a gain/offset, the float 1.0 at +32, the u16 at +36 and +40) so they
//! survive a save; the app keeps the playback options itself.

use std::collections::BTreeMap;

use crate::chunks::{self, Chunk};
use crate::events::{self, Note, Record, BAR_ONE};

/// Where notes inside a sequence start counting: bar 1 (one bar after placements' origin).
pub const NOTE_ONE: u32 = 38400;

const GADD_CELL: i32 = -3;
const GADD_ROW: i32 = -4;
const EMPTY: u32 = u32::MAX;

/// The grid as saved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    /// Rows stored: one per track that can hold cells.
    pub rows: u32,
    /// Scenes stored (Logic's window pads more empty ones on the right).
    pub scenes: u32,
    /// The Grid payload's u32 at +32 (meaning unknown; kept).
    pub extra: u32,
    /// Each row's track: its channel object (Envi id >> 16), in row order.
    pub row_channels: Vec<u32>,
    pub cells: Vec<Cell>,
}

/// A filled cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    /// Grid row and scene (0-based).
    pub row: u32,
    pub scene: u32,
    /// The Clip it refers to (Clip chunk id >> 16).
    pub clip: u32,
    /// The cell slot's flag byte (1 in the first sample, 0 in later saves: meaning unknown).
    pub flag: u8,
    /// The cell's own sequence (MSeq id >> 16) and the sequence holding its notes.
    pub seq: u32,
    pub inner: Option<u32>,
    pub name: String,
    /// The track it plays on: the placement's channel object (Envi id >> 16) and track row.
    pub channel: Option<u32>,
    pub track_row: Option<u32>,
    /// Notes from the cell's start (0 = the cell's first beat), 960 per quarter note.
    pub notes: Vec<Note>,
    /// Its length in ticks (whole bars that hold the notes, at least one bar).
    pub length: u32,
    /// An audio cell: the audio file's name (as the AuFl entry has it).
    pub audio: Option<String>,
    pub params: ClipParams,
}

/// The Clip payload's fields seen to vary (meanings unconfirmed), and the payload itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipParams {
    /// f32 @+28 (0.0 by default; small values ±0.3 in templates).
    pub gain: f32,
    /// f32 @+32 (1.0 everywhere so far: probably Speed).
    pub speed: f32,
    /// u16 @+36 (0x0080 / 0x0081 / 0x0082 on template cells; 0x4400 / 0x4040 on unused clips).
    pub flags: u16,
    /// u16 @+40 (0x0010 on template cells, 0xFFFF on E23's).
    pub mode: u16,
    pub raw: Vec<u8>,
}

fn i32_at(raw: &[u8], off: usize) -> Option<i32> {
    raw.get(off..off + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u32_at(raw: &[u8], off: usize) -> Option<u32> {
    raw.get(off..off + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn f32_at(raw: &[u8], off: usize) -> f32 {
    raw.get(off..off + 4).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u16_at(raw: &[u8], off: usize) -> u16 {
    raw.get(off..off + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

/// A GAdd chunk's kind and index.
pub fn gadd(c: &Chunk) -> Option<(i32, u32)> {
    Some((i32_at(c.raw, 14)?, u32_at(c.raw, 18)?))
}

/// The chunks that make up the song (the undo history's copies left out).
fn song_chunks(data: &[u8]) -> Option<Vec<Chunk<'_>>> {
    Some(chunks::walk(data).ok()?.into_iter().filter(|c| !c.is("USEl") && !c.is("UStk")).collect())
}

pub fn clip_params(payload: &[u8]) -> ClipParams {
    ClipParams { gain: f32_at(payload, 28), speed: f32_at(payload, 32), flags: u16_at(payload, 36), mode: u16_at(payload, 40), raw: payload.to_vec() }
}

/// An audio file's name in an AuFl payload (UTF-16, length-prefixed).
fn audio_file_name(payload: &[u8]) -> Option<String> {
    for off in 0..payload.len().saturating_sub(4).min(64) {
        let n = u16::from_le_bytes([payload[off], payload[off + 1]]) as usize;
        if !(2..=255).contains(&n) {
            continue;
        }
        let Some(s) = payload.get(off + 2..off + 2 + 2 * n) else { continue };
        let units: Vec<u16> = s.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        if units.iter().all(|&u| (0x20..0xfffe).contains(&u) && u != 0x7f) {
            let name = String::from_utf16_lossy(&units);
            if name.contains('.') {
                return Some(name);
            }
        }
    }
    None
}

/// Ticks of a bar for a signature.
fn bar_of(sig: (u8, u8)) -> u32 {
    crate::project::bar_ticks(sig).max(1)
}

/// Reads the grid; None when the project has none (normal projects have no Grid chunk).
/// `sig` sets the bar a cell's length is rounded to.
pub fn read(data: &[u8], sig: (u8, u8)) -> Option<Grid> {
    let chunks = song_chunks(data)?;
    let grid = chunks.iter().find(|c| c.is("Grid"))?;
    let mut g = Grid { scenes: u32_at(grid.payload, 16).unwrap_or(0), rows: u32_at(grid.payload, 20).unwrap_or(0), extra: u32_at(grid.payload, 32).unwrap_or(EMPTY), ..Default::default() };
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    let mut evs: BTreeMap<u32, &[u8]> = BTreeMap::new();
    let mut clips: BTreeMap<u32, &[u8]> = BTreeMap::new();
    let mut slots: Vec<(u32, u32, u8)> = Vec::new();
    let mut rows: Vec<(u32, u32)> = Vec::new();
    for c in &chunks {
        if c.is("MSeq") {
            if let Some(n) = chunks::short_string(c.payload, 16) {
                names.insert(c.id, n);
            }
        } else if c.is("EvSq") && c.id != 0 {
            evs.insert(c.id, c.payload);
        } else if c.is("Clip") {
            clips.insert(c.id >> 16, c.payload);
        } else if c.is("GAdd") {
            match gadd(c) {
                Some((GADD_CELL, i)) if c.payload.len() >= 5 => {
                    slots.push((i, u32_at(c.payload, 0).unwrap_or(EMPTY), c.payload[4]));
                }
                Some((GADD_ROW, i)) => rows.push((i, u32_at(c.payload, 0).unwrap_or(0))),
                _ => {}
            }
        }
    }
    rows.sort_by_key(|s| s.0);
    g.row_channels = rows.into_iter().map(|s| s.1).collect();
    let audio_files: Vec<String> = chunks.iter().filter(|c| c.is("AuFl")).filter_map(|c| audio_file_name(c.payload)).collect();
    let per_row = g.scenes.max(1);
    let bar = bar_of(sig);
    for (index, clip, flag) in slots {
        if clip == EMPTY {
            continue;
        }
        let mut cell = Cell { row: index / per_row, scene: index % per_row, clip, flag, ..Default::default() };
        if let Some(p) = clips.get(&clip) {
            cell.params = clip_params(p);
            cell.seq = u32_at(p, 16).unwrap_or(EMPTY);
        }
        let wrapper = cell.seq << 16;
        cell.name = names.get(&wrapper).cloned().unwrap_or_default();
        let placement = evs.get(&wrapper).and_then(|e| events::records(e).iter().find_map(events::placement));
        let mut end = 0u32;
        if let Some(pl) = placement {
            cell.channel = Some(pl.channel);
            cell.track_row = Some(pl.row);
            // (notes: from bar 1 inside the sequence, shifted by where the cell's region sits)
            let origin = NOTE_ONE + pl.pos.saturating_sub(BAR_ONE);
            if pl.audio {
                // (the file named as the cell's region, else the n-th; how the placement's audio
                // region number maps onto files in grids is unconfirmed)
                let i = pl.audio_region.saturating_sub(1) as usize;
                let stem = |f: &String| f.rsplit_once('.').map_or(f.clone(), |(s, _)| s.to_string());
                cell.audio = audio_files.iter().find(|f| !cell.name.is_empty() && stem(f) == cell.name).or(audio_files.get(i)).or(audio_files.first()).cloned();
            } else if let Some(s) = pl.sequence {
                cell.inner = Some(s);
                if let Some(n) = names.get(&(s << 16)).filter(|n| !n.is_empty()) {
                    cell.name = n.clone();
                }
                if let Some(ev) = evs.get(&(s << 16)) {
                    cell.notes = events::records(ev).iter().filter_map(events::note).map(|mut n| {
                        n.pos = n.pos.saturating_sub(origin);
                        n
                    }).collect();
                }
            }
        }
        for n in &cell.notes {
            end = end.max(n.pos + n.len);
        }
        cell.length = end.max(1).div_ceil(bar) * bar;
        g.cells.push(cell);
    }
    g.cells.sort_by_key(|c| (c.row, c.scene));
    Some(g)
}

/// What the grid looked like in Logic's window when saved (DisplayState.plist, LiveLoopsUserData
/// sectionVisibility): 1 grid only, 2 grid and tracks, 3 tracks only.
pub fn section_visibility(alternative: &std::path::Path) -> Option<i64> {
    fn find(v: &plist::Value) -> Option<i64> {
        match v {
            plist::Value::Dictionary(d) => {
                if let Some(n) = d.get("sectionVisibility").and_then(|x| x.as_signed_integer()) {
                    return Some(n);
                }
                d.values().find_map(find)
            }
            plist::Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    find(&plist::Value::from_file(alternative.join("DisplayState.plist")).ok()?)
}

/// A change to write into the original file.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// A cell's notes (from the cell's start), in its own sequence.
    Notes { clip: u32, notes: Vec<Note> },
    /// A filled cell moved to another (empty) slot; across rows its placement's channel object
    /// and track row follow.
    Move { from: (u32, u32), to: (u32, u32), channel: u32, track_row: u32 },
    /// A cell emptied (the slot only: the Clip and sequences stay, unreferenced, as Logic leaves
    /// them after Delete until it tidies up).
    Clear { row: u32, scene: u32 },
    /// The Clip's f32 at +32 (Speed, unconfirmed).
    Speed { clip: u32, speed: f32 },
}

/// A note record (two units, as Logic writes them) at `abs`.
fn note_record(template: Option<&Record>, n: &Note, abs: u32) -> Vec<u8> {
    let mut b = match template {
        Some(t) if t.bytes.len() >= 32 => t.bytes.clone(),
        _ => {
            let mut b = vec![0u8; 32];
            b[23] = 0x89;
            b[15] = 0x01;
            b
        }
    };
    b[0] = 0x90 | (n.channel & 0x0F);
    let pos = abs.to_le_bytes();
    b[4..7].copy_from_slice(&pos[..3]);
    b[7] = pos[3] & 0x7F;
    b[11] = n.velocity;
    b[12] = n.pitch;
    b[28..32].copy_from_slice(&n.len.to_le_bytes());
    b
}

/// An event list with `notes` (at `origin` + their position) instead of its notes.
fn rebuild(payload: &[u8], notes: &[Note], origin: u32) -> Vec<u8> {
    let records = events::records(payload);
    let templates: Vec<&Record> = records.iter().filter(|r| events::note(r).is_some()).collect();
    let mut out: Vec<(u32, Vec<u8>)> = records.iter().filter(|r| events::note(r).is_none()).map(|r| (r.pos, r.bytes.clone())).collect();
    for (i, n) in notes.iter().enumerate() {
        let abs = origin + n.pos;
        out.push((abs, note_record(templates.get(i).or(templates.first()).copied(), n, abs)));
    }
    out.sort_by_key(|(p, _)| *p);
    let mut data: Vec<u8> = out.into_iter().flat_map(|(_, b)| b).collect();
    let mut end = vec![0u8; 16];
    end[0] = 0xF1;
    end[4..8].copy_from_slice(&events::END.to_le_bytes());
    data.extend(end);
    data
}

/// The original file with `edits` applied (chunks found by id each time, so it works on a file
/// another save has already rewritten). None if it isn't ProjectData or has no grid.
pub fn write(data: &[u8], edits: &[Edit]) -> Option<Vec<u8>> {
    let chunks = song_chunks(data)?;
    let grid = chunks.iter().find(|c| c.is("Grid"))?;
    let scenes = u32_at(grid.payload, 16)?.max(1);
    // new payloads by chunk start
    let mut new: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let slot = |row: u32, scene: u32| chunks.iter().find(|c| c.is("GAdd") && gadd(c) == Some((GADD_CELL, row * scenes + scene)));
    let clip = |id: u32| chunks.iter().rev().find(|c| c.is("Clip") && c.id >> 16 == id);
    let last = |tag: &str, id: u32| chunks.iter().rev().find(|c| c.is(tag) && c.id == id);
    for e in edits {
        match e {
            Edit::Clear { row, scene } => {
                let c = slot(*row, *scene)?;
                new.insert(c.start, vec![0xff, 0xff, 0xff, 0xff, 0]);
            }
            Edit::Move { from, to, channel, track_row } => {
                let (a, b) = (slot(from.0, from.1)?, slot(to.0, to.1)?);
                let moved = new.get(&a.start).cloned().unwrap_or_else(|| a.payload.to_vec());
                let target = new.get(&b.start).cloned().unwrap_or_else(|| b.payload.to_vec());
                new.insert(b.start, moved.clone());
                new.insert(a.start, target);
                if from.0 != to.0 {
                    let id = u32_at(&moved, 0)?;
                    let seq = u32_at(clip(id)?.payload, 16)?;
                    let ev = last("EvSq", seq << 16)?;
                    let mut p = new.get(&ev.start).cloned().unwrap_or_else(|| ev.payload.to_vec());
                    // the first record is the placement: channel u32 at 16, row u16 at 20
                    if p.len() >= 48 && (p[0] == 0x20 || p[0] == 0x24) {
                        p[16..20].copy_from_slice(&channel.to_le_bytes());
                        p[20..22].copy_from_slice(&(*track_row as u16).to_le_bytes());
                        new.insert(ev.start, p);
                    }
                }
            }
            Edit::Speed { clip: id, speed } => {
                let c = clip(*id)?;
                let mut p = new.get(&c.start).cloned().unwrap_or_else(|| c.payload.to_vec());
                if p.len() >= 36 {
                    p[32..36].copy_from_slice(&speed.to_le_bytes());
                    new.insert(c.start, p);
                }
            }
            Edit::Notes { clip: id, notes } => {
                let seq = u32_at(clip(*id)?.payload, 16)?;
                let wrapper = last("EvSq", seq << 16)?;
                let pl = events::records(wrapper.payload).iter().find_map(events::placement)?;
                let inner = last("EvSq", pl.sequence? << 16)?;
                let origin = NOTE_ONE + pl.pos.saturating_sub(BAR_ONE);
                new.insert(inner.start, rebuild(inner.payload, notes, origin));
            }
        }
    }
    Some(splice(data, &new))
}

/// The file with some chunks' payloads replaced (their lengths and the file's length updated).
fn splice(data: &[u8], new: &BTreeMap<usize, Vec<u8>>) -> Vec<u8> {
    let Ok(all) = chunks::walk(data) else { return data.to_vec() };
    let mut out = data[..24].to_vec();
    for c in &all {
        match new.get(&c.start) {
            Some(p) => {
                let mut h = c.raw[..36].to_vec();
                h[28..36].copy_from_slice(&(p.len() as u64).to_le_bytes());
                out.extend(h);
                out.extend_from_slice(p);
            }
            None => out.extend_from_slice(c.raw),
        }
    }
    let body = (out.len() - 24) as u32;
    out[0x10..0x14].copy_from_slice(&body.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(rel: &str) -> Option<Vec<u8>> {
        let home = std::env::var("HOME").ok()?;
        std::fs::read(std::path::Path::new(&home).join("CascadeProjects/logic-research-private/sample-files").join(rel)).ok()
    }

    #[test]
    fn e23_grid_and_its_cell() {
        let Some(data) = sample("E23-live-loops-base.logicx/Alternatives/000/ProjectData") else { return };
        let g = read(&data, (4, 4)).expect("a grid");
        assert_eq!((g.scenes, g.rows), (8, 27));
        // rows: the tracks' channel objects (Deluxe Classic 0xE0, the audio track 0x58, ...)
        assert_eq!(g.row_channels.len(), 27);
        assert_eq!(&g.row_channels[..4], &[0xE0, 0x58, 0x124, 0x128]);
        assert_eq!(g.cells.len(), 1);
        let c = &g.cells[0];
        // row 0, scene 2 (index 1), Clip 0x10, its own sequence 0x7C -> the notes in 0x78
        assert_eq!((c.row, c.scene, c.clip, c.seq, c.inner), (0, 1, 0x10, 0x7C, Some(0x78)));
        assert_eq!(c.name, "MIDI Region");
        // on track 1 "Deluxe Classic" (channel object 0xE0)
        assert_eq!((c.channel, c.track_row), (Some(0xE0), Some(1)));
        assert_eq!(c.notes.len(), 6);
        // the first note at the cell's start: C2 (48) velocity 40, half a bar long
        assert_eq!((c.notes[0].pos, c.notes[0].pitch, c.notes[0].velocity, c.notes[0].len), (0, 48, 40, 1920));
        assert_eq!(c.notes[5].pos, 5 * 3840);
        // 5.5 bars of notes -> 6 bars
        assert_eq!(c.length, 6 * 3840);
        assert_eq!(c.params.speed, 1.0);
        assert!(c.audio.is_none());
        // the later alternative: more scenes, the same cell
        if let Some(d1) = sample("E23-live-loops-base.logicx/Alternatives/001/ProjectData") {
            let g1 = read(&d1, (4, 4)).unwrap();
            assert_eq!((g1.scenes, g1.rows), (8, 35));
            assert_eq!(g1.cells.iter().map(|c| (c.row, c.scene)).collect::<Vec<_>>(), vec![(0, 1)]);
        }
    }

    #[test]
    fn template_grid_cells_all_resolve() {
        let Some(data) = sample("templates-reference/Chromium Fray.logicx/Alternatives/000/ProjectData") else { return };
        let g = read(&data, (4, 4)).unwrap();
        assert_eq!((g.scenes, g.rows), (13, 33));
        assert_eq!(g.row_channels.len(), 33);
        assert!(g.cells.len() > 50, "{}", g.cells.len());
        // every filled cell finds its track and its contents
        for c in &g.cells {
            assert!(c.channel.is_some(), "{c:?}");
            assert!(!c.notes.is_empty() || c.audio.is_some() || c.inner.is_some(), "{} {}", c.row, c.scene);
            assert!(c.length >= 3840);
        }
        // each cell plays on its row's track
        for c in &g.cells {
            assert_eq!(c.channel, g.row_channels.get(c.row as usize).copied(), "{} {}", c.row, c.scene);
        }
        assert!(g.cells.iter().any(|c| c.audio.is_some()) && g.cells.iter().any(|c| !c.notes.is_empty()));
        // audio cells find their loop's file by name
        let kick = g.cells.iter().find(|c| c.name == "Flow Solo Kick 02").unwrap();
        assert_eq!(kick.audio.as_deref(), Some("Flow Solo Kick 02.caf"));
    }

    #[test]
    fn no_grid_in_a_plain_project() {
        let Some(data) = sample("E02-4-notes.logicx/Alternatives/000/ProjectData") else { return };
        assert!(read(&data, (4, 4)).is_none());
    }

    #[test]
    fn writes_notes_moves_and_clears() {
        let Some(data) = sample("E23-live-loops-base.logicx/Alternatives/000/ProjectData") else { return };
        let g = read(&data, (4, 4)).unwrap();
        let c = &g.cells[0];
        let mut notes = c.notes.clone();
        notes[0].velocity = 99;
        notes.push(Note { channel: 0, pitch: 67, velocity: 80, pos: 960, len: 480 });
        let out = write(&data, &[Edit::Notes { clip: c.clip, notes: notes.clone() }, Edit::Speed { clip: c.clip, speed: 2.0 }]).unwrap();
        assert!(chunks::walk(&out).is_ok());
        let g2 = read(&out, (4, 4)).unwrap();
        let mut got = g2.cells[0].notes.clone();
        got.sort_by_key(|n| (n.pos, n.pitch));
        notes.sort_by_key(|n| (n.pos, n.pitch));
        assert_eq!(got, notes);
        assert_eq!(g2.cells[0].params.speed, 2.0);
        // the timeline's region is untouched (cells are copies)
        let p1 = crate::project::read_project_data(&data).unwrap();
        let p2 = crate::project::read_project_data(&out).unwrap();
        assert_eq!(p1.tracks[0].regions[0].notes, p2.tracks[0].regions[0].notes);
        // moved to scene 4 of the same row, then to row 2 (track row 4, channel 0x124)
        let moved = write(&data, &[Edit::Move { from: (0, 1), to: (0, 3), channel: 0xE0, track_row: 1 }]).unwrap();
        assert_eq!(read(&moved, (4, 4)).unwrap().cells.iter().map(|c| (c.row, c.scene)).collect::<Vec<_>>(), vec![(0, 3)]);
        let across = write(&data, &[Edit::Move { from: (0, 1), to: (2, 0), channel: 0x124, track_row: 4 }]).unwrap();
        let ga = read(&across, (4, 4)).unwrap();
        assert_eq!((ga.cells[0].row, ga.cells[0].scene, ga.cells[0].channel, ga.cells[0].track_row), (2, 0, Some(0x124), Some(4)));
        assert_eq!(ga.cells[0].notes, c.notes);
        let cleared = write(&data, &[Edit::Clear { row: 0, scene: 1 }]).unwrap();
        assert!(read(&cleared, (4, 4)).unwrap().cells.is_empty());
        assert_eq!(cleared.len(), data.len());
    }

    #[test]
    fn splice_keeps_the_file_walkable() {
        let mut body = Vec::new();
        for (tag, payload) in [("Song", vec![1u8, 2, 3]), ("EvSq", vec![0u8; 16])] {
            let mut h = vec![0u8; 36];
            let t: Vec<u8> = tag.bytes().rev().collect();
            h[..4].copy_from_slice(&t);
            h[28..36].copy_from_slice(&(payload.len() as u64).to_le_bytes());
            h.extend(payload);
            body.extend(h);
        }
        let mut file = chunks::MAGIC.to_vec();
        file.extend_from_slice(&[0; 12]);
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend(body);
        let second = chunks::walk(&file).unwrap()[1].start;
        let mut new = BTreeMap::new();
        new.insert(second, vec![7u8; 48]);
        let out = splice(&file, &new);
        let c = chunks::walk(&out).unwrap();
        assert_eq!(c[1].payload, &[7u8; 48][..]);
        assert_eq!(u32::from_le_bytes(out[16..20].try_into().unwrap()) as usize, out.len() - 24);
    }
}
