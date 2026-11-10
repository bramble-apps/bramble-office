//! Standard MIDI files (File > Export > All MIDI Tracks as MIDI File…): type 1, one track per
//! instrument track, the tempo and time signature in the first, at Logic's 960 ticks per
//! quarter note (Logic's own export writes 480; any rate is valid).

use crate::events::PPQ;
use crate::project::{Project, TrackKind};

fn vlq(mut v: u32, out: &mut Vec<u8>) {
    let mut buf = [0u8; 5];
    let mut i = 4;
    buf[i] = (v & 0x7f) as u8;
    v >>= 7;
    while v > 0 {
        i -= 1;
        buf[i] = ((v & 0x7f) as u8) | 0x80;
        v >>= 7;
    }
    out.extend_from_slice(&buf[i..]);
}

fn chunk(tag: &[u8], data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
}

/// Events (tick, bytes) as a track chunk's data, with delta times and the end of track.
fn track_data(mut events: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    // note-offs before note-ons at the same tick
    events.sort_by_key(|(t, b)| (*t, if b.first().is_some_and(|s| s & 0xF0 == 0x80) { 0 } else { 1 }));
    let mut d = Vec::new();
    let mut last = 0;
    for (t, b) in events {
        vlq(t - last, &mut d);
        d.extend_from_slice(&b);
        last = t;
    }
    d.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    d
}

/// The project as a type 1 MIDI file.
pub fn write(p: &Project) -> Vec<u8> {
    let mut out = Vec::new();
    let tracks: Vec<_> = p.tracks.iter().filter(|t| t.kind != TrackKind::Audio).collect();
    let mut head = Vec::new();
    head.extend_from_slice(&1u16.to_be_bytes());
    head.extend_from_slice(&((tracks.len() + 1) as u16).to_be_bytes());
    head.extend_from_slice(&(PPQ as u16).to_be_bytes());
    chunk(b"MThd", &head, &mut out);
    // the tempo track: name, tempo, time signature
    let mut meta = vec![(0, {
        let mut b = vec![0xFF, 0x03];
        vlq(p.name.len() as u32, &mut b);
        b.extend_from_slice(p.name.as_bytes());
        b
    })];
    let map = if p.tempo_map.is_empty() { vec![(0, p.tempo)] } else { p.tempo_map.clone() };
    for (at, bpm) in map {
        let mpq = (60_000_000.0 / bpm.max(1.0)).round() as u32;
        meta.push((at, vec![0xFF, 0x51, 0x03, (mpq >> 16) as u8, (mpq >> 8) as u8, mpq as u8]));
    }
    let denom_pow = p.signature.1.max(1).trailing_zeros() as u8;
    meta.push((0, vec![0xFF, 0x58, 0x04, p.signature.0, denom_pow, 24, 8]));
    chunk(b"MTrk", &track_data(meta), &mut out);
    let mut next = 0u8;
    for t in tracks {
        let ch = if t.kind == TrackKind::Drummer {
            9
        } else {
            let c = next;
            next = if next + 1 == 9 { 10 } else { (next + 1) % 16 };
            c
        };
        let mut ev: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut name = vec![0xFF, 0x03];
        vlq(t.name.len() as u32, &mut name);
        name.extend_from_slice(t.name.as_bytes());
        ev.push((0, name));
        for r in t.regions.iter().filter(|r| !r.muted) {
            for n in &r.notes {
                ev.push((n.pos, vec![0x90 | ch, n.pitch & 0x7f, n.velocity.clamp(1, 127)]));
                ev.push((n.pos + n.len.max(1), vec![0x80 | ch, n.pitch & 0x7f, 64]));
            }
        }
        chunk(b"MTrk", &track_data(ev), &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Note;
    use crate::project::{Region, Track};

    #[test]
    fn writes_a_type_1_file() {
        let mut p = Project { name: "T".into(), tempo: 120.0, signature: (3, 4), ..Default::default() };
        p.tracks.push(Track { name: "Keys".into(), kind: TrackKind::Instrument, channel: 0, row: 1, regions: vec![Region { name: "r".into(), start: 0, length: 3840, notes: vec![Note { channel: 0, pitch: 60, velocity: 100, pos: 0, len: 480 }], audio: None, muted: false, source: None, origin: None, props: Default::default() }], volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new() });
        let b = write(&p);
        assert_eq!(&b[..4], b"MThd");
        assert_eq!(u16::from_be_bytes([b[10], b[11]]), 2, "tempo track + one track");
        assert_eq!(u16::from_be_bytes([b[12], b[13]]), 960);
        // 120 BPM = 500000 µs per quarter
        assert!(b.windows(6).any(|w| w == [0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]));
        // 3/4
        assert!(b.windows(4).any(|w| w == [0xFF, 0x58, 0x04, 3]));
        // the note on and off
        assert!(b.windows(3).any(|w| w == [0x90, 60, 100]));
        assert!(b.windows(3).any(|w| w == [0x80, 60, 64]));
        let mut v = Vec::new();
        vlq(480, &mut v);
        assert_eq!(v, vec![0x83, 0x60]);
    }
}

/// A Standard MIDI File read for import (File > Import > MIDI File…, or a .mid dropped on the
/// tracks): its tracks' notes rescaled to 960 PPQ from the file's start, the first tempo and
/// time signature, and each track's name and channel.
#[derive(Clone, Debug, Default)]
pub struct Imported {
    pub tempo: Option<f64>,
    pub signature: Option<(u8, u8)>,
    /// (name, channel of its first note, notes at 960 PPQ from the file's start).
    pub tracks: Vec<(String, u8, Vec<crate::events::Note>)>,
}

fn read_vlq(d: &[u8], i: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        let b = *d.get(*i)?;
        *i += 1;
        v = (v << 7) | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    Some(v)
}

pub fn read(data: &[u8]) -> Option<Imported> {
    if data.get(..4)? != b"MThd" {
        return None;
    }
    let ntrk = u16::from_be_bytes([data[10], data[11]]) as usize;
    let division = u16::from_be_bytes([data[12], data[13]]);
    if division & 0x8000 != 0 || division == 0 {
        return None; // SMPTE time: not supported
    }
    let scale = PPQ as f64 / division as f64;
    let mut out = Imported::default();
    let mut at = 8 + u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    for _ in 0..ntrk {
        if data.get(at..at + 4)? != b"MTrk" {
            break;
        }
        let len = u32::from_be_bytes(data[at + 4..at + 8].try_into().ok()?) as usize;
        let d = data.get(at + 8..at + 8 + len)?;
        at += 8 + len;
        let mut i = 0;
        let mut tick = 0u64;
        let mut status = 0u8;
        let mut name = String::new();
        let mut open: std::collections::HashMap<(u8, u8), (u64, u8)> = Default::default();
        let mut notes = Vec::new();
        let mut first_ch = None;
        while i < d.len() {
            tick += read_vlq(d, &mut i)? as u64;
            let mut b = *d.get(i)?;
            if b & 0x80 != 0 {
                i += 1;
                if b < 0xF0 {
                    status = b;
                }
            } else {
                b = status;
            }
            match b {
                0xFF => {
                    let kind = *d.get(i)?;
                    i += 1;
                    let l = read_vlq(d, &mut i)? as usize;
                    let body = d.get(i..i + l)?;
                    i += l;
                    match kind {
                        0x03 if name.is_empty() => name = String::from_utf8_lossy(body).trim().to_string(),
                        0x51 if l == 3 && out.tempo.is_none() => {
                            let us = ((body[0] as u32) << 16) | ((body[1] as u32) << 8) | body[2] as u32;
                            out.tempo = Some(60_000_000.0 / us as f64);
                        }
                        0x58 if l >= 2 && out.signature.is_none() => out.signature = Some((body[0], 1u8 << body[1].min(6))),
                        0x2F => break,
                        _ => {}
                    }
                }
                0xF0 | 0xF7 => {
                    let l = read_vlq(d, &mut i)? as usize;
                    i += l;
                }
                _ => {
                    let ch = b & 0x0F;
                    let kind = b & 0xF0;
                    let n = if kind == 0xC0 || kind == 0xD0 { 1 } else { 2 };
                    let a = *d.get(i)?;
                    let v = if n == 2 { *d.get(i + 1)? } else { 0 };
                    i += n;
                    let t = (tick as f64 * scale).round() as u64;
                    match kind {
                        0x90 if v > 0 => {
                            first_ch.get_or_insert(ch);
                            open.insert((ch, a), (t, v));
                        }
                        0x80 | 0x90 => {
                            if let Some((s, vel)) = open.remove(&(ch, a)) {
                                notes.push(crate::events::Note { channel: ch, pitch: a, velocity: vel, pos: s as u32, len: (t - s).max(1) as u32 });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if !notes.is_empty() {
            notes.sort_by_key(|n| n.pos);
            out.tracks.push((name, first_ch.unwrap_or(0), notes));
        }
    }
    Some(out)
}

#[cfg(test)]
mod read_tests {
    #[test]
    fn reads_back_what_it_writes() {
        let mut p = crate::project::Project { tempo: 97.0, signature: (3, 4), ..Default::default() };
        let notes = vec![crate::events::Note { channel: 0, pitch: 60, velocity: 90, pos: 0, len: 480 }, crate::events::Note { channel: 0, pitch: 64, velocity: 70, pos: 960, len: 960 }];
        p.tracks.push(crate::project::Track { name: "Keys".into(), kind: crate::project::TrackKind::Instrument, channel: 0, row: 1, regions: vec![crate::project::Region { name: "Keys".into(), start: 0, length: 3840, notes: notes.clone(), audio: None, muted: false, source: None, origin: None, props: Default::default() }], volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new() });
        let back = super::read(&super::write(&p)).expect("reads");
        assert_eq!(back.tempo.map(|t| t.round()), Some(97.0));
        assert_eq!(back.signature, Some((3, 4)));
        let (_, _, n) = back.tracks.iter().find(|t| !t.2.is_empty()).unwrap();
        assert_eq!(n.iter().map(|n| (n.pitch, n.velocity, n.len)).collect::<Vec<_>>(), vec![(60, 90, 480), (64, 70, 960)]);
        assert_eq!(n[1].pos - n[0].pos, 960);
    }
}
