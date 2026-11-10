//! Chords for the Bass and Keyboard Players: a chord (root, intervals, optional bass note), a
//! chord at a position, names ("Am7", "F/A"), and Logic's chord progressions.
//!
//! Logic stores a region's chords as 0x70 event records whose second unit holds the intervals as
//! a 12-bit mask (bit n = n semitones above the root: 0x91 major, 0x89 minor) and the root's pitch
//! class; [`crate::genm`] reads and writes them.

/// A chord: root pitch class (0 = C), its intervals as a 12-bit mask (bit 0 = the root), and a
/// bass pitch class when it is a slash chord.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub root: u8,
    pub mask: u16,
    pub bass: Option<u8>,
}

pub const MAJOR: u16 = 0x091;
pub const MINOR: u16 = 0x089;
pub const DIM: u16 = 0x049;
pub const AUG: u16 = 0x111;
pub const SUS2: u16 = 0x085;
pub const SUS4: u16 = 0x0A1;
pub const DOM7: u16 = 0x491;
pub const MAJ7: u16 = 0x891;
pub const MIN7: u16 = 0x489;
pub const HALFDIM7: u16 = 0x449;
pub const DIM7: u16 = 0x249;
pub const SIX: u16 = 0x291;
pub const MIN6: u16 = 0x289;
pub const ADD9: u16 = 0x095;

const SUFFIXES: [(&str, u16); 16] = [
    ("maj7", MAJ7),
    ("m7b5", HALFDIM7),
    ("dim7", DIM7),
    ("add9", ADD9),
    ("sus2", SUS2),
    ("sus4", SUS4),
    ("sus", SUS4),
    ("dim", DIM),
    ("aug", AUG),
    ("m7", MIN7),
    ("m6", MIN6),
    ("7", DOM7),
    ("6", SIX),
    ("m", MINOR),
    ("+", AUG),
    ("", MAJOR),
];

const NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

/// A pitch class from a note name ("C", "F#", "Bb").
pub fn pitch_class(name: &str) -> Option<(u8, usize)> {
    let b = name.as_bytes();
    let base = match b.first()?.to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let (shift, used) = match b.get(1) {
        Some(b'#') => (1i32, 2),
        Some(b'b') => (-1, 2),
        _ => (0, 1),
    };
    Some((((base + shift + 12) % 12) as u8, used))
}

impl Chord {
    pub fn new(root: u8, mask: u16) -> Chord {
        Chord { root: root % 12, mask: mask | 1, bass: None }
    }

    pub fn major(root: u8) -> Chord {
        Chord::new(root, MAJOR)
    }

    pub fn minor(root: u8) -> Chord {
        Chord::new(root, MINOR)
    }

    /// Parses "C", "Am", "F#m7", "Bb/D", "Gsus4", "Dm7b5" …
    pub fn parse(s: &str) -> Option<Chord> {
        let s = s.trim();
        let (main, bass) = match s.split_once('/') {
            Some((m, b)) => (m, Some(pitch_class(b)?.0)),
            None => (s, None),
        };
        let (root, used) = pitch_class(main)?;
        let rest = &main[used..];
        let mask = SUFFIXES.iter().find(|(suf, _)| *suf == rest)?.1;
        Some(Chord { root, mask, bass })
    }

    pub fn name(&self) -> String {
        let suf = SUFFIXES.iter().find(|(_, m)| *m == self.mask).map(|(s, _)| *s).unwrap_or("");
        let suf = if suf == "sus" { "sus4" } else { suf };
        let mut s = format!("{}{}", NAMES[self.root as usize % 12], suf);
        if let Some(b) = self.bass {
            s.push('/');
            s.push_str(NAMES[b as usize % 12]);
        }
        s
    }

    pub fn is_minor(&self) -> bool {
        self.mask & 0x008 != 0 && self.mask & 0x010 == 0
    }

    /// The intervals in semitones above the root, low to high.
    pub fn intervals(&self) -> Vec<u8> {
        (0..12).filter(|i| self.mask & (1 << i) != 0).collect()
    }

    /// The pitch classes of the chord.
    pub fn pitch_classes(&self) -> Vec<u8> {
        self.intervals().iter().map(|i| (self.root + i) % 12).collect()
    }

    /// The third (or the sus note) above the root.
    pub fn third(&self) -> u8 {
        [4u8, 3, 5, 2].into_iter().find(|i| self.mask & (1 << i) != 0).unwrap_or(4)
    }

    /// The fifth above the root (diminished / augmented when the chord has one).
    pub fn fifth(&self) -> u8 {
        [7u8, 6, 8].into_iter().find(|i| self.mask & (1 << i) != 0).unwrap_or(7)
    }

    /// The seventh above the root, if any.
    pub fn seventh(&self) -> Option<u8> {
        [10u8, 11, 9].into_iter().find(|i| self.mask & (1 << i) != 0)
    }

    /// The bass's pitch class (the slash note, or the root).
    pub fn bass_pc(&self) -> u8 {
        self.bass.unwrap_or(self.root)
    }
}

/// A chord from a position (song ticks) to the next chord.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChordAt {
    pub pos: u32,
    pub chord: Chord,
}

/// The chord sounding at a tick (the last one at or before it; the first one before them all).
pub fn chord_at(chords: &[ChordAt], tick: u32) -> Option<Chord> {
    chords.iter().rev().find(|c| c.pos <= tick).or(chords.first()).map(|c| c.chord)
}

/// The next chord change after a tick: (position, chord).
pub fn next_change(chords: &[ChordAt], tick: u32) -> Option<ChordAt> {
    let cur = chord_at(chords, tick);
    chords.iter().find(|c| c.pos > tick && Some(c.chord) != cur).copied()
}

/// Logic's chord progressions (Session Player editor > Chords > Chord Progressions), and the
/// default for new regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progression {
    TwoFiveOne,
    OneFiveSixFour,
    OneSixFourFive,
    TwoOneSixFive,
    FourOneSixFive,
    SixFourOneFive,
    /// The default chords of a new region (I - ii - vi - vi, measured)
    Default,
}

impl Progression {
    pub const ALL: [Progression; 7] = [
        Progression::TwoFiveOne,
        Progression::OneFiveSixFour,
        Progression::OneSixFourFive,
        Progression::TwoOneSixFive,
        Progression::FourOneSixFive,
        Progression::SixFourOneFive,
        Progression::Default,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Progression::TwoFiveOne => "ii - V - I",
            Progression::OneFiveSixFour => "I - V - vi - IV",
            Progression::OneSixFourFive => "I - vi - IV - V",
            Progression::TwoOneSixFive => "ii - I - vi - V",
            Progression::FourOneSixFive => "IV - I - vi - V",
            Progression::SixFourOneFive => "vi - IV - I - V",
            Progression::Default => "I - ii - vi - vi",
        }
    }

    /// The scale degrees (1-7) of the progression.
    pub fn degrees(self) -> &'static [u8] {
        match self {
            Progression::TwoFiveOne => &[2, 5, 1, 1],
            Progression::OneFiveSixFour => &[1, 5, 6, 4],
            Progression::OneSixFourFive => &[1, 6, 4, 5],
            Progression::TwoOneSixFive => &[2, 1, 6, 5],
            Progression::FourOneSixFive => &[4, 1, 6, 5],
            Progression::SixFourOneFive => &[6, 4, 1, 5],
            Progression::Default => &[1, 2, 6, 6],
        }
    }

    /// The progression in a major key (tonic pitch class), one chord per bar from `start`,
    /// repeated for `bars` bars.
    pub fn chords(self, key: u8, start: u32, bar: u32, bars: u32) -> Vec<ChordAt> {
        let d = self.degrees();
        (0..bars).map(|b| ChordAt { pos: start + b * bar, chord: diatonic(key, d[b as usize % d.len()]) }).collect()
    }
}

/// The diatonic triad on a degree (1-7) of a major key.
pub fn diatonic(key: u8, degree: u8) -> Chord {
    const STEPS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    const QUAL: [u16; 7] = [MAJOR, MINOR, MINOR, MAJOR, MAJOR, MINOR, DIM];
    let i = (degree.clamp(1, 7) - 1) as usize;
    Chord::new((key + STEPS[i]) % 12, QUAL[i])
}

/// Parses a list like "C Am F G" (one chord per bar from `start`).
pub fn parse_bars(s: &str, start: u32, bar: u32) -> Vec<ChordAt> {
    s.split_whitespace().filter_map(Chord::parse).enumerate().map(|(i, c)| ChordAt { pos: start + i as u32 * bar, chord: c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for s in ["C", "Am", "F#m7", "Bbmaj7", "G7", "Dm7b5", "Esus4", "C/E", "Abdim"] {
            assert_eq!(Chord::parse(s).unwrap().name(), s);
        }
        assert_eq!(Chord::parse("Am").unwrap().mask, 0x89);
        assert_eq!(Chord::parse("C").unwrap().mask, 0x91);
        assert_eq!(Chord::parse("G7").unwrap().pitch_classes(), vec![7, 11, 2, 5]);
    }

    #[test]
    fn progressions() {
        let c = Progression::OneSixFourFive.chords(0, 0, 2880, 4);
        assert_eq!(c.iter().map(|c| c.chord.name()).collect::<Vec<_>>(), ["C", "Am", "F", "G"]);
        let d = Progression::Default.chords(0, 0, 2880, 4);
        assert_eq!(d.iter().map(|c| c.chord.name()).collect::<Vec<_>>(), ["C", "Dm", "Am", "Am"]);
        assert_eq!(chord_at(&c, 3000).unwrap().name(), "Am");
        assert_eq!(next_change(&c, 3000).unwrap().pos, 5760);
    }
}
