//! A sequence's events (EvSq payloads): 16-byte units, a record = one head unit and the units
//! after it whose byte 7 has its top bit set; every list ends with an F1 record at 0x3FFFFFFF.
//! Head unit: +0 kind (MIDI-like: 0x9n note, 0xBn controller, 0xDn pressure, 0xEn pitch bend;
//! 0x5n fader / automation, 0x30 signature, 0x32 key, 0x20 / 0x24 a region placed in a folder,
//! 0x70 chord, 0xF1 end), +4 u32 position in ticks (960 per quarter note), +15 flags.

/// Ticks per quarter note.
pub const PPQ: u32 = 960;
/// The tick of bar 1 beat 1 (Logic counts from a fixed point before the song's start).
pub const BAR_ONE: u32 = 34560;
/// Bar 1 for global events (tempo changes, markers, signatures after the first): Logic counts
/// them from 38400 (format-analysis §11, §10, §25), regions from `BAR_ONE`.
pub const EVENT_BAR_ONE: u32 = 38400;
/// The end marker's position.
pub const END: u32 = 0x3FFF_FFFF;

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub kind: u8,
    pub pos: u32,
    pub bytes: Vec<u8>,
}

impl Record {
    pub fn u8_at(&self, off: usize) -> u8 {
        self.bytes.get(off).copied().unwrap_or(0)
    }

    pub fn u16_at(&self, off: usize) -> u16 {
        self.bytes.get(off..off + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32_at(&self, off: usize) -> u32 {
        self.bytes.get(off..off + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn units(&self) -> usize {
        self.bytes.len() / 16
    }
}

/// The records of an event list (stops at the end marker, or the payload's end).
pub fn records(payload: &[u8]) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    for unit in payload.chunks_exact(16) {
        if unit[7] & 0x80 != 0 {
            if let Some(last) = out.last_mut() {
                last.bytes.extend_from_slice(unit);
                continue;
            }
        }
        out.push(Record { kind: unit[0], pos: u32::from_le_bytes([unit[4], unit[5], unit[6], unit[7] & 0x7f]), bytes: unit.to_vec() });
    }
    if let Some(i) = out.iter().position(|r| r.kind == 0xF1) {
        out.truncate(i);
    }
    out
}

/// A note: MIDI channel, pitch (60 = C3 in Logic's naming), velocity, position and length in ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
    pub pos: u32,
    pub len: u32,
    /// Mute Notes: kept in the region, not played.
    pub muted: bool,
}

/// A controller change, pitch bend, channel or poly pressure or program change (kind 0xB, 0xE,
/// 0xD, 0xA, 0xC), its value(s). Pitch bend: number = LSB, value = MSB (unverified in Logic).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    pub kind: u8,
    pub channel: u8,
    pub pos: u32,
    pub number: u8,
    pub value: u8,
}

pub fn note(r: &Record) -> Option<Note> {
    (r.kind & 0xF0 == 0x90 && r.units() >= 2).then(|| Note { channel: r.kind & 0x0F, pitch: r.u8_at(12), velocity: r.u8_at(11), pos: r.pos, len: r.u32_at(28), muted: false })
}

pub fn control(r: &Record) -> Option<Control> {
    matches!(r.kind & 0xF0, 0xA0 | 0xB0 | 0xC0 | 0xD0 | 0xE0).then(|| Control { kind: r.kind & 0xF0, channel: r.kind & 0x0F, pos: r.pos, number: r.u8_at(12), value: r.u8_at(11) })
}

/// A time signature (kind 0x30): numerator, denominator.
pub fn signature(r: &Record) -> Option<(u8, u8)> {
    (r.kind == 0x30).then(|| (r.u8_at(12), 1u8.checked_shl(r.u8_at(11) as u32).unwrap_or(4)))
}

/// A key signature (kind 0x32): Logic's key number (7 = C).
pub fn key(r: &Record) -> Option<u8> {
    (r.kind == 0x32).then(|| r.u8_at(12))
}

/// An automation point (kind 0x5n): parameter (7 = volume), value (0-127 fader scale).
pub fn automation(r: &Record) -> Option<(u8, u8)> {
    (r.kind & 0xF0 == 0x50).then(|| (r.u8_at(12), r.u8_at(11)))
}

/// A region placed in a folder (kind 0x20 MIDI / 0x24 audio): where, which channel object
/// (Envi id >> 16), which track row, which sequence (MSeq id >> 16; audio: none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub audio: bool,
    pub pos: u32,
    pub channel: u32,
    pub row: u32,
    pub sequence: Option<u32>,
    /// Audio placements: a key for the audio region (1 + its file's AuFl id >> 2 in the high
    /// half, its number among that file's regions in the low half); MIDI: the record's u32 @56.
    pub audio_region: u32,
    /// Audio placements: the file (record @44 = the AuFl's and its AuRg's header id >> 16) and
    /// the region's number among that file's AuRg (record @40 = AuRg header u32 @12 >> 16).
    pub audio_file: u32,
    pub audio_index: u32,
    pub muted: bool,
    /// The Region inspector's values kept in the record (format-analysis §14, §8): Loop (byte
    /// 13 bit 0x02), Velocity offset (52), Transpose (53); audio fades in the fifth unit (ms,
    /// curve).
    pub looped: bool,
    pub velocity: i8,
    pub transpose: i8,
    pub fade_in: (u16, i8),
    pub fade_out: (u16, i8),
}

pub fn placement(r: &Record) -> Option<Placement> {
    if !(r.kind == 0x20 || r.kind == 0x24) || r.units() < 3 {
        return None;
    }
    let seq = r.u32_at(32);
    let audio = r.kind == 0x24;
    let (audio_file, audio_index) = if audio { (r.u32_at(44), r.u32_at(40)) } else { (0, 0) };
    Some(Placement {
        audio,
        pos: r.pos,
        channel: r.u32_at(16),
        // (u16: byte 7 of the unit is the continuation mark)
        row: r.u16_at(20) as u32,
        sequence: (seq != u32::MAX && seq != 0).then_some(seq),
        audio_region: if audio { 1 + ((audio_file >> 2) << 16 | (audio_index & 0xFFFF)) } else { r.u32_at(56) },
        audio_file,
        audio_index,
        muted: false,
        looped: !audio && r.u8_at(13) & 0x02 != 0,
        velocity: if r.units() >= 4 && !audio { r.u8_at(52) as i8 } else { 0 },
        transpose: if r.units() >= 4 && !audio { r.u8_at(53) as i8 } else { 0 },
        fade_in: if audio && r.units() >= 5 { (r.u16_at(76), r.u8_at(79) as i8) } else { (0, 0) },
        fade_out: if audio && r.units() >= 5 { (r.u16_at(72), r.u8_at(75) as i8) } else { (0, 0) },
    })
}

/// Writes the Region inspector's values into a placement record (`placement` reads them).
pub fn set_region_props(b: &mut [u8], looped: bool, velocity: i8, transpose: i8, fade_in: (u16, i8), fade_out: (u16, i8)) {
    if b.len() < 80 {
        return;
    }
    if b[0] == 0x24 {
        b[72..74].copy_from_slice(&fade_out.0.to_le_bytes());
        b[75] = fade_out.1 as u8;
        b[76..78].copy_from_slice(&fade_in.0.to_le_bytes());
        b[79] = fade_in.1 as u8;
    } else {
        b[13] = if looped { b[13] | 0x02 } else { b[13] & !0x02 };
        b[52] = velocity as u8;
        b[53] = transpose as u8;
    }
}

/// Minor keys by Logic's fifths number (the relative major's: F minor shares A♭ major's 4 flats).
const MINOR_NAMES: [&str; 15] = ["A\u{266d}", "E\u{266d}", "B\u{266d}", "F", "C", "G", "D", "A", "E", "B", "F\u{266f}", "C\u{266f}", "G\u{266f}", "D\u{266f}", "A\u{266f}"];

/// Logic's key byte (format-analysis §13: fifths + 7, + 0x10 for minor) as "F minor".
pub fn key_string(k: u8) -> String {
    if k & 0x10 != 0 {
        format!("{} minor", MINOR_NAMES.get((k & 0x0F) as usize).copied().unwrap_or("A"))
    } else {
        format!("{} major", key_name(k & 0x0F))
    }
}

fn pitch_class(name: &str) -> Option<i32> {
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
    let acc: i32 = c.map(|x| match x {
        '\u{266d}' | 'b' => -1,
        '\u{266f}' | '#' => 1,
        _ => 0,
    }).sum();
    Some((base + acc).rem_euclid(12))
}

/// "F minor", "E♭ major", "D♭ minor" (spelled as Logic's C♯ minor) as Logic's key byte.
pub fn key_byte(s: &str) -> Option<u8> {
    let mut words = s.split_whitespace();
    let root = words.next()?;
    let minor = words.next().is_some_and(|w| w.eq_ignore_ascii_case("minor"));
    let pc = pitch_class(root)?;
    let names: Vec<&str> = if minor { MINOR_NAMES.to_vec() } else { (0..15).map(key_name).collect() };
    let norm = root.replace('b', "\u{266d}").replace('#', "\u{266f}");
    let i = names.iter().position(|n| *n == norm).or_else(|| {
        // the simplest spelling (fewest accidentals) of that pitch class
        let mut c: Vec<usize> = (0..15).filter(|&i| pitch_class(names[i]) == Some(pc)).collect();
        c.sort_by_key(|&i| (i as i32 - 7).abs());
        c.first().copied()
    })?;
    Some(i as u8 | if minor { 0x10 } else { 0 })
}

/// Logic's key number as a name (7 = C; the circle runs in fifths around it).
pub fn key_name(k: u8) -> &'static str {
    const NAMES: [&str; 15] = ["C\u{266d}", "G\u{266d}", "D\u{266d}", "A\u{266d}", "E\u{266d}", "B\u{266d}", "F", "C", "G", "D", "A", "E", "B", "F\u{266f}", "C\u{266f}"];
    NAMES.get(k as usize).copied().unwrap_or("C")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(bytes: &[(usize, u8)], cont: bool) -> [u8; 16] {
        let mut u = [0u8; 16];
        for &(i, b) in bytes {
            u[i] = b;
        }
        if cont {
            u[7] |= 0x80;
        }
        u
    }

    #[test]
    fn records_join_continuations_and_stop_at_the_end() {
        let mut p = Vec::new();
        // a note: C#3 (61) velocity 80 at 40080, 240 long
        let pos = 40080u32.to_le_bytes();
        p.extend(unit(&[(0, 0x90), (4, pos[0]), (5, pos[1]), (6, pos[2]), (11, 80), (12, 61)], false));
        p.extend(unit(&[(7, 0x09), (12, 240)], true));
        // a controller, one unit
        p.extend(unit(&[(0, 0xB1), (11, 100), (12, 7)], false));
        // the end
        p.extend(unit(&[(0, 0xF1), (4, 0xff), (5, 0xff), (6, 0xff), (7, 0x3f)], false));
        let r = records(&p);
        assert_eq!(r.len(), 2);
        assert_eq!(note(&r[0]), Some(Note { channel: 0, pitch: 61, velocity: 80, pos: 40080, len: 240, muted: false }));
        assert_eq!(control(&r[1]).map(|c| (c.channel, c.number, c.value)), Some((1, 7, 100)));
    }

    #[test]
    fn key_bytes_both_ways() {
        // format-analysis §13: C major 7, F minor 0x13, E major 0x0B
        assert_eq!(key_byte("C major"), Some(7));
        assert_eq!(key_byte("F minor"), Some(0x13));
        assert_eq!(key_byte("E major"), Some(0x0B));
        assert_eq!(key_byte("D\u{266d} minor"), Some(0x10 | 11), "spelled C\u{266f} minor");
        for k in (0..15u8).chain((0..15).map(|k| k | 0x10)) {
            assert_eq!(key_byte(&key_string(k)), Some(k), "{}", key_string(k));
        }
    }

    #[test]
    fn keys_and_signatures() {
        assert_eq!(key_name(7), "C");
        assert_eq!(key_name(8), "G");
        let mut b = vec![0u8; 48];
        b[0] = 0x30;
        b[11] = 2;
        b[12] = 3;
        assert_eq!(signature(&Record { kind: 0x30, pos: 0, bytes: b }), Some((3, 4)));
    }
}
