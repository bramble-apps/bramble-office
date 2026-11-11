//! What the Score editor adds to a project beyond the notes (Logic keeps these in the region's
//! score data): per-note attributes (stems, ties, beams, accidentals, enharmonic spelling,
//! interpretation, articulations, fingering), the staff style of each track, and the symbols
//! placed on the staff (dynamics, text, lyrics, chord symbols, slurs, hairpins, clef and key
//! changes). Kept in the Project so Undo covers every Score edit.
//!
//! Tracks are keyed by their stable id (`TrackExtra::id`), notes by (position, pitch): a note
//! moved in the Score keeps its attributes (the editor re-keys it); a note moved elsewhere
//! loses them, as do regions moved in the Tracks area. Not written into the .logicx yet.

use std::collections::BTreeMap;

/// Per-note attributes; 0 is "Default" for each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoteAttr {
    /// 1 up, −1 down, 2 hidden.
    pub stem: i8,
    /// Stem position: 0 default, 1 center, 2 automatic, 3 side.
    pub stem_pos: u8,
    /// Stem end moved by half-spaces.
    pub stem_end: i8,
    /// 1 above, −1 below, 2 hidden.
    pub tie: i8,
    /// 1 beamed with selected neighbours, −1 unbeamed.
    pub beam: i8,
    /// 1 force, 2 hide, 3 guide, 4 force sharps and flats only.
    pub accidental: u8,
    /// Enharmonic shift: 1 sharp, −1 flat.
    pub spell: i8,
    /// Interpretation and Syncopation: 1 force, −1 defeat.
    pub interp: i8,
    pub synco: i8,
    /// 0 not independent, 1 independent, 2 independent grace.
    pub independent: u8,
    /// Voice/Staff assignment: 1 staff above, −1 staff below.
    pub staff: i8,
    /// Articulations attached (bits of `ARTICULATIONS`).
    pub artic: u32,
    /// Fingering: main finger 1–5, pick finger P I M A C (1–5).
    pub finger: u8,
    pub pick: u8,
    /// Note colour (Colors ▸ Color 0–15), 0 = default.
    pub color: u8,
    /// Auto slur: 1 above, −1 below, 2 automatic direction (slurs to the next note).
    pub slur: i8,
}

/// Logic's attachable symbols, in its key-command order ("Attach Symbol: …").
pub const ARTICULATIONS: [&str; 22] = [
    "Fermata Up", "Fermata Down", "Staccato", "Staccatissimo", "Accent", "Strong Accent", "Long Accent", "Tenuto", "Up-Bow", "Down-Bow", "Flageolet", "Pizzicato", "Left Hand", "Right Hand", "Soft", "Heavy", "Jazz 1", "Jazz 2", "Jazz 3", "Jazz 4", "Jazz 5", "Jazz 6",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Dynamic,
    Text,
    Lyric,
    Chord,
    Crescendo,
    Decrescendo,
    SlurUp,
    SlurDown,
    /// A clef change: `text` holds the clef's name (Treble, Bass, Alto, Tenor, Percussion).
    Clef,
}

/// A symbol on a track's staff at a position (ticks from bar 1), with its length (hairpins,
/// slurs) and text (dynamics, words, lyrics, chord names, clef names).
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub tick: u32,
    pub kind: SymbolKind,
    pub text: String,
    pub len: u32,
    /// Which staff of a grand staff (0 the upper).
    pub staff: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackScore {
    /// The staff style's name (lg-notation's StaffStyle), "" = automatic.
    pub style: String,
    /// The region parameters the Score reads with: display Quantize ("" = 16,24),
    /// Interpretation (on unless turned off) and Syncopation.
    pub quantize: String,
    pub no_interpretation: bool,
    pub syncopation: bool,
    pub notes: BTreeMap<(u32, u8), NoteAttr>,
    pub symbols: Vec<Symbol>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Score {
    pub tracks: BTreeMap<u64, TrackScore>,
    /// Key changes inserted from the Part box: (ticks from bar 1, fifths), written in the score
    /// (the project's key stays the first).
    pub keys: Vec<(u32, i8)>,
}

impl Score {
    pub fn track(&self, id: u64) -> Option<&TrackScore> {
        self.tracks.get(&id)
    }
    pub fn track_mut(&mut self, id: u64) -> &mut TrackScore {
        self.tracks.entry(id).or_default()
    }
    pub fn attr(&self, id: u64, pos: u32, pitch: u8) -> NoteAttr {
        self.tracks.get(&id).and_then(|t| t.notes.get(&(pos, pitch))).copied().unwrap_or_default()
    }
    /// Changes a note's attributes (empty ones are dropped).
    pub fn edit(&mut self, id: u64, pos: u32, pitch: u8, f: impl FnOnce(&mut NoteAttr)) {
        let t = self.track_mut(id);
        let mut a = t.notes.get(&(pos, pitch)).copied().unwrap_or_default();
        f(&mut a);
        if a == NoteAttr::default() {
            t.notes.remove(&(pos, pitch));
        } else {
            t.notes.insert((pos, pitch), a);
        }
    }
    /// A note moved (Score drags): its attributes follow.
    pub fn moved(&mut self, id: u64, from: (u32, u8), to: (u32, u8)) {
        if from == to {
            return;
        }
        if let Some(t) = self.tracks.get_mut(&id) {
            if let Some(a) = t.notes.remove(&from) {
                t.notes.insert(to, a);
            }
        }
    }
    /// The key (fifths) in force at a tick, from the project's.
    pub fn key_at(&self, tick: u32, first: i8) -> i8 {
        self.keys.iter().filter(|k| k.0 <= tick).last().map_or(first, |k| k.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_follow_moves_and_drop_when_default() {
        let mut s = Score::default();
        s.edit(7, 960, 60, |a| a.stem = 1);
        assert_eq!(s.attr(7, 960, 60).stem, 1);
        s.moved(7, (960, 60), (1920, 62));
        assert_eq!(s.attr(7, 960, 60), NoteAttr::default());
        assert_eq!(s.attr(7, 1920, 62).stem, 1);
        s.edit(7, 1920, 62, |a| a.stem = 0);
        assert!(s.tracks[&7].notes.is_empty());
        s.keys = vec![(3840, 2)];
        assert_eq!((s.key_at(0, -1), s.key_at(3840, -1)), (-1, 2));
    }
}
