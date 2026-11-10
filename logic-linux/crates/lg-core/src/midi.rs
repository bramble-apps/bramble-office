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
    let mpq = (60_000_000.0 / p.tempo.max(1.0)).round() as u32;
    let mut meta = vec![(0, {
        let mut b = vec![0xFF, 0x03];
        vlq(p.name.len() as u32, &mut b);
        b.extend_from_slice(p.name.as_bytes());
        b
    })];
    meta.push((0, vec![0xFF, 0x51, 0x03, (mpq >> 16) as u8, (mpq >> 8) as u8, mpq as u8]));
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
        p.tracks.push(Track { name: "Keys".into(), kind: TrackKind::Instrument, channel: 0, row: 1, regions: vec![Region { name: "r".into(), start: 0, length: 3840, notes: vec![Note { channel: 0, pitch: 60, velocity: 100, pos: 0, len: 480 }], audio: None, muted: false }], volume_db: 0.0, pan: 0.0, mute: false, solo: false });
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
