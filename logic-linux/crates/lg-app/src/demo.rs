//! The song shown before a project is opened: our own short groove (drums, bass, keys) over
//! C – Am – F – G, built in code; and the empty project of File > New.

use lg_core::events::Note;
use lg_core::project::{Project, Region, Track, TrackKind};

const BAR: u32 = 3840;
const BEAT: u32 = 960;
const BARS: u32 = 8;

fn note(pitch: u8, velocity: u8, pos: u32, len: u32) -> Note {
    Note { channel: 0, pitch, velocity, pos, len }
}

fn track(name: &str, kind: TrackKind, row: u32, regions: Vec<Region>) -> Track {
    Track { name: name.into(), kind, channel: 0x58 + row * 4, row, regions, volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new() }
}

fn region(name: &str, start: u32, length: u32, notes: Vec<Note>) -> Region {
    Region { name: name.into(), start, length, notes, audio: None, muted: false, source: None, origin: None, props: Default::default() }
}

pub fn song() -> Project {
    // chords: C, Am, F, G (roots and triads, middle register)
    let chords: [(u8, [u8; 3]); 4] = [(36, [60, 64, 67]), (33, [57, 60, 64]), (29, [53, 57, 60]), (31, [55, 59, 62])];
    let mut drums = Vec::new();
    let mut bass = Vec::new();
    let mut keys = Vec::new();
    for bar in 0..BARS {
        let b = bar * BAR;
        let (root, triad) = chords[(bar % 4) as usize];
        for beat in 0..4 {
            let t = b + beat * BEAT;
            // kick on 1 and 3 (and the "and" of 3), snare on 2 and 4, eighth-note hats
            if beat % 2 == 0 {
                drums.push(note(36, 110, t, 240));
            }
            if beat == 2 {
                drums.push(note(36, 80, t + 480, 240));
            }
            if beat % 2 == 1 {
                drums.push(note(38, 105, t, 240));
            }
            drums.push(note(42, if beat % 2 == 0 { 90 } else { 70 }, t, 120));
            drums.push(note(42, 55, t + 480, 120));
            // bass: root on the beat, octave pickup at the end of the bar
            let p = if beat == 3 { root + 12 } else { root };
            bass.push(note(p, 100, t, 420));
        }
        if bar % 4 == 3 {
            drums.push(note(49, 100, b + 3 * BEAT + 480, 240));
        }
        // keys: the chord on 1 and the "and" of 2
        for (off, len, vel) in [(0, 1300, 88), (BEAT + 480, 1700, 76)] {
            for &p in &triad {
                keys.push(note(p, vel, b + off, len));
            }
        }
    }
    let len = BARS * BAR;
    Project {
        name: "Bramble Groove".into(),
        tempo: 104.0,
        signature: (4, 4),
        key: "C major".into(),
        sample_rate: 48000,
        tracks: vec![
            track("SoCal", TrackKind::Drummer, 1, vec![region("Drummer", 0, len, drums)]),
            track("Fingerstyle Bass", TrackKind::Instrument, 2, vec![region("Fingerstyle Bass", 0, len, bass)]),
            track("Deluxe Classic", TrackKind::Instrument, 3, vec![region("Deluxe Classic", 0, len, keys)]),
        ],
        ..Default::default()
    }
}

pub fn empty() -> Project {
    Project {
        name: "Untitled".into(),
        tempo: 120.0,
        signature: (4, 4),
        key: "C major".into(),
        sample_rate: 48000,
        tracks: vec![track("Deluxe Classic", TrackKind::Instrument, 1, Vec::new())],
        ..Default::default()
    }
}
