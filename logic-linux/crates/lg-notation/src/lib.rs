//! Engraving for Bramble Studio's Score editor (Logic Pro's Score, linear view): which clef,
//! how each pitch is spelled in the key, where notes sit on the staff, and how a region's notes
//! become notated values once quantised for display (Logic's "16,24": sixteenths, or eighth
//! triplets where they fit better), with rests filled in, ties across beats and bar lines,
//! chords on one stem, stems and beams. Pure functions, no drawing: the editor draws the result.
//!
//! Ticks are 960 per quarter note, from bar 1 (tick 0), as everywhere in Studio.

pub mod musicxml;
pub mod musicxml_in;

pub const PPQ: u32 = 960;

/// Pitch class letters C D E F G A B.
pub const LETTERS: [char; 7] = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
const LETTER_PC: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clef {
    Treble,
    Bass,
    /// C clefs (Logic's Alto and Tenor staff styles).
    Alto,
    Tenor,
    /// The drum staff: positions from the drum map, not from pitch.
    Percussion,
}

impl Clef {
    /// The diatonic step of the staff's bottom line (E4 for treble, G2 for bass; step = MIDI
    /// octave × 7 + letter, so middle C, MIDI 60, is step 35).
    pub fn bottom_line(self) -> i32 {
        match self {
            Clef::Treble | Clef::Percussion => 37,
            Clef::Bass => 25,
            Clef::Alto => 31,
            Clef::Tenor => 29,
        }
    }
}

/// The staves a part is shown on: Logic's staff styles Treble, Bass or Piano (grand staff).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staves {
    Treble,
    Bass,
    Piano,
}

/// Logic's automatic staff style: by the notes' range (a wide range or a piano part across
/// middle C gets the grand staff), low parts the bass clef.
pub fn staves_for(pitches: &[u8], piano: bool) -> Staves {
    if pitches.is_empty() {
        return if piano { Staves::Piano } else { Staves::Treble };
    }
    let lo = *pitches.iter().min().unwrap_or(&60);
    let hi = *pitches.iter().max().unwrap_or(&60);
    let below = pitches.iter().filter(|&&p| p < 60).count();
    let above = pitches.len() - below;
    if (piano && below > 0 && above > 0) || (lo < 53 && hi > 67) {
        return Staves::Piano;
    }
    if piano && below == 0 {
        return Staves::Piano;
    }
    // more of the part below middle C, or deep: bass clef
    let mean = pitches.iter().map(|&p| p as f32).sum::<f32>() / pitches.len() as f32;
    if mean < 57.0 || hi < 60 { Staves::Bass } else { Staves::Treble }
}

/// A key: the number of sharps (positive) or flats (negative) in its signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Key {
    pub fifths: i8,
}

impl Key {
    /// From Studio's key text ("C major", "E♭ major", "F# minor", "Bb minor").
    pub fn parse(text: &str) -> Key {
        let mut it = text.split_whitespace();
        let root = it.next().unwrap_or("C").replace('♯', "#").replace('♭', "b");
        let minor = it.next().is_some_and(|m| m.to_lowercase().starts_with("min"));
        let major = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"];
        let minors = ["Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#"];
        let table = if minor { &minors } else { &major };
        let i = table.iter().position(|n| n.eq_ignore_ascii_case(&root)).unwrap_or(7);
        Key { fifths: i as i8 - 7 }
    }

    /// The alteration the key signature gives each letter (C..B).
    pub fn letter_alters(self) -> [i8; 7] {
        let mut a = [0i8; 7];
        // order of sharps F C G D A E B; flats the reverse
        let sharps = [3usize, 0, 4, 1, 5, 2, 6];
        if self.fifths > 0 {
            for &l in sharps.iter().take(self.fifths as usize) {
                a[l] = 1;
            }
        } else {
            for &l in sharps.iter().rev().take((-self.fifths) as usize) {
                a[l] = -1;
            }
        }
        a
    }

    /// The staff positions of the signature's sharps or flats (0 = bottom line, half-spaces up),
    /// in the order they are written.
    pub fn signature_positions(self, clef: Clef) -> Vec<(i32, i8)> {
        let sharp_pos = [8, 5, 9, 6, 3, 7, 4];
        let flat_pos = [4, 7, 3, 6, 2, 5, 1];
        let shift = match clef {
            Clef::Bass => -2,
            Clef::Alto => -1,
            Clef::Tenor if self.fifths < 0 => 1,
            _ => 0,
        };
        if self.fifths > 0 && clef == Clef::Tenor {
            // the tenor clef's sharps: F♯ on the second line, then up and down inside the staff
            return sharp_pos.iter().take(self.fifths as usize).map(|p| (if p - 6 < 1 { p + 1 } else { p - 6 }, 1)).collect();
        }
        if self.fifths > 0 {
            sharp_pos.iter().take(self.fifths as usize).map(|p| (p + shift, 1)).collect()
        } else {
            flat_pos.iter().take((-self.fifths) as usize).map(|p| (p + shift, -1)).collect()
        }
    }

    /// How a pitch is written in this key: (letter 0..6, alteration, diatonic step).
    pub fn spell(self, pitch: u8) -> Spelled {
        let pc = (pitch % 12) as i32;
        let oct = (pitch / 12) as i32;
        let alters = self.letter_alters();
        // the scale's own notes first (E♯ in F♯ major, C♭ in G♭ major)
        let in_key = (0..7).find(|&l| (LETTER_PC[l] + alters[l] as i32).rem_euclid(12) == pc);
        let (letter, alter) = if let Some(l) = in_key {
            (l, alters[l])
        } else if self.fifths > 0 {
            // a raised scale note
            let l = (0..7).find(|&l| (LETTER_PC[l] + alters[l] as i32 + 1).rem_euclid(12) == pc).unwrap_or(0);
            (l, alters[l] + 1)
        } else if self.fifths < 0 {
            let l = (0..7).find(|&l| (LETTER_PC[l] + alters[l] as i32 - 1).rem_euclid(12) == pc).unwrap_or(0);
            (l, alters[l] - 1)
        } else {
            // C major / A minor: Logic's C♯ E♭ F♯ G♯ B♭
            match pc {
                1 => (0, 1),
                3 => (2, -1),
                6 => (3, 1),
                8 => (4, 1),
                _ => (6, -1),
            }
        };
        // B♯ / C♭ cross the octave
        let natural = LETTER_PC[letter] + alter as i32;
        let oct = oct + if natural < 0 { 1 } else if natural > 11 { -1 } else { 0 };
        Spelled { letter: letter as u8, alter, step: oct * 7 + letter as i32 }
    }
}

impl Key {
    /// A pitch spelled with a preference (Logic's Enharmonic Shift: 1 = written as a sharp,
    /// −1 as a flat, 0 = as the key spells it). Naturals stay naturals unless shifted onto the
    /// neighbouring letter (E as F♭, B♯ as C…) is the only way to honour the shift.
    pub fn spell_pref(self, pitch: u8, pref: i8) -> Spelled {
        let base = self.spell(pitch);
        if pref == 0 || base.alter.signum() == pref.signum() {
            return base;
        }
        let pc = (pitch % 12) as i32;
        let oct = (pitch / 12) as i32;
        let want = pref.signum();
        // the letter a step away whose alteration by `want` gives this pitch class
        for l in 0..7usize {
            if (LETTER_PC[l] + want as i32).rem_euclid(12) == pc {
                let natural = LETTER_PC[l] + want as i32;
                let oct = oct + if natural < 0 { 1 } else if natural > 11 { -1 } else { 0 };
                return Spelled { letter: l as u8, alter: want, step: oct * 7 + l as i32 };
            }
        }
        base
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spelled {
    pub letter: u8,
    pub alter: i8,
    /// Diatonic step: MIDI octave × 7 + letter (middle C = 35).
    pub step: i32,
}

impl Spelled {
    pub fn name(&self) -> String {
        let acc = match self.alter {
            2 => "𝄪",
            1 => "♯",
            -1 => "♭",
            -2 => "𝄫",
            _ => "",
        };
        format!("{}{}", LETTERS[self.letter as usize], acc)
    }
}

/// The MIDI pitch a staff position stands for in a key (for a click or a diatonic drag: the
/// line or space, altered as the key signature says).
pub fn pitch_at(clef: Clef, pos: i32, key: Key) -> u8 {
    step_pitch(clef.bottom_line() + pos, key)
}

/// The MIDI pitch of a diatonic step (MIDI octave × 7 + letter) in a key.
pub fn step_pitch(step: i32, key: Key) -> u8 {
    let oct = step.div_euclid(7);
    let letter = step.rem_euclid(7) as usize;
    let p = oct * 12 + LETTER_PC[letter] + key.letter_alters()[letter] as i32;
    p.clamp(0, 127) as u8
}

/// Note values: the base (1 = whole, 2 = half … 64) and dots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Value {
    pub base: u8,
    pub dots: u8,
}

impl Value {
    pub fn ticks(self) -> u32 {
        let b = PPQ * 4 / self.base as u32;
        match self.dots {
            0 => b,
            1 => b * 3 / 2,
            _ => b * 7 / 4,
        }
    }
    /// Beams or flags: 8ths 1, 16ths 2, 32nds 3, 64ths 4.
    pub fn flags(self) -> u8 {
        match self.base {
            8 => 1,
            16 => 2,
            32 => 3,
            64 => 4,
            128 => 5,
            _ => 0,
        }
    }
}

/// Display quantisation (Logic's region parameter "Quantize": 16,24 by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quant {
    /// The straight grid in ticks (240 = 1/16).
    pub straight: u32,
    /// The triplet grid tried per beat (320 = 1/24, eighth triplets), if any.
    pub triplet: Option<u32>,
}

impl Default for Quant {
    fn default() -> Self {
        Quant { straight: 240, triplet: Some(320) }
    }
}

impl Quant {
    /// Logic's Quantize choices for the Score: 4, 8, 16, 32, 64, 12, 24, 48, 8,12 16,12 16,24 …
    pub const CHOICES: [(&'static str, Quant); 13] = [
        ("4", Quant { straight: 960, triplet: None }),
        ("8", Quant { straight: 480, triplet: None }),
        ("16", Quant { straight: 240, triplet: None }),
        ("32", Quant { straight: 120, triplet: None }),
        ("64", Quant { straight: 60, triplet: None }),
        ("12", Quant { straight: 640, triplet: None }),
        ("24", Quant { straight: 320, triplet: None }),
        ("8,12", Quant { straight: 480, triplet: Some(640) }),
        ("16,12", Quant { straight: 240, triplet: Some(640) }),
        ("16,24", Quant { straight: 240, triplet: Some(320) }),
        // (after the default's index 9)
        ("128", Quant { straight: 30, triplet: None }),
        // (the LCD divisions 1/12 and 1/24 give these as the Insert Defaults' Quantize)
        ("4,12", Quant { straight: 960, triplet: Some(640) }),
        ("8,24", Quant { straight: 480, triplet: Some(320) }),
    ];
}

/// A note as the region has it (ticks from bar 1), with its index in the region, and the
/// Score's per-note attributes (Logic's Functions ▸ Note Attributes; 0 = default for each).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoteIn {
    pub index: usize,
    pub pitch: u8,
    pub pos: u32,
    pub len: u32,
    /// Enharmonic shift: 1 sharp, −1 flat.
    pub spell: i8,
    /// Stem: 1 up, −1 down, 2 hidden.
    pub stem: i8,
    /// Beam: 1 beamed with the next selected (Beam Selected), −1 unbeamed.
    pub beam: i8,
    /// Interpretation / Syncopation: 1 force, −1 defeat.
    pub interp: i8,
    pub synco: i8,
    /// Tie: 1 above, −1 below, 2 hidden.
    pub tie: i8,
    /// Accidental: 1 force, 2 hide, 3 guide (in brackets), 4 force sharps and flats only.
    pub acc: u8,
}

impl NoteIn {
    pub fn new(index: usize, pitch: u8, pos: u32, len: u32) -> NoteIn {
        NoteIn { index, pitch, pos, len, ..Default::default() }
    }
}

/// One notehead of a chord.
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    /// The note's index in the region.
    pub note: usize,
    pub pitch: u8,
    pub spelled: Spelled,
    /// Staff position: 0 = bottom line, one per line or space.
    pub pos: i32,
    /// The accidental written (0 = natural), if one is.
    pub acc: Option<i8>,
    /// Tied to the same pitch in the next item.
    pub tie: bool,
    /// Set off to the other side of the stem (a second in a chord).
    pub shifted: bool,
    /// The tie's side: 1 above, −1 below, 0 automatic; hidden ties aren't drawn.
    pub tie_dir: i8,
    pub tie_hidden: bool,
    /// A guide (cautionary) accidental, drawn in brackets.
    pub guide: bool,
    /// A drum staff's cross head (cymbals and hi-hats).
    pub cross: bool,
}

/// A chord or a rest.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub tick: u32,
    pub dur: u32,
    pub value: Value,
    /// Empty for a rest.
    pub heads: Vec<Head>,
    pub stem_up: bool,
    /// The beam group it's in (index into Voice::beams).
    pub beam: Option<usize>,
    /// Part of a triplet.
    pub triplet: bool,
    /// A whole-bar rest (centred in the bar whatever the metre).
    pub full_bar: bool,
    /// The bar, counted from the engraved span's start (`Options::from`).
    pub bar: u32,
    /// Stem hidden (Stems ▸ Hide).
    pub stem_hidden: bool,
}

impl Item {
    pub fn is_rest(&self) -> bool {
        self.heads.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Beam {
    pub items: Vec<usize>,
    pub up: bool,
}

/// A tuplet bracket over items `first..=last`: `n` notes in the time of `of` (3 in 2 for the
/// automatic triplets), the note value `base` (Show note value: "5:4" style lower number).
#[derive(Clone, Debug, PartialEq)]
pub struct Tuplet {
    pub first: usize,
    pub last: usize,
    pub up: bool,
    pub n: u8,
    pub of: u8,
    pub base: u8,
    /// A user tuplet (Part box N-tuplet), with its display options.
    pub spec: Option<TupletSpec>,
}

/// A user N-tuplet (Logic's Tuplet window): `n` notes of value 1/`base` in the time of `of`
/// such notes, starting at `start` (ticks from bar 1). Notes inside are written on its grid,
/// overriding the display Quantize.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TupletSpec {
    pub start: u32,
    pub n: u8,
    pub of: u8,
    pub base: u8,
    pub hide_bracket: bool,
    pub hide_number: bool,
    pub show_value: bool,
    /// 0 automatic, 1 up, −1 down.
    pub dir: i8,
    /// Allow double / half values (a note of twice or half the tuplet value inside it).
    pub double: bool,
    pub half: bool,
}

impl TupletSpec {
    /// The time it occupies, in ticks.
    pub fn len(&self) -> u32 {
        self.of.max(1) as u32 * (PPQ * 4 / self.base.max(1) as u32)
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    pub fn end(&self) -> u32 {
        self.start + self.len()
    }
    /// One tuplet note, in ticks (fractional).
    pub fn unit(&self) -> f64 {
        self.len() as f64 / self.n.max(1) as f64
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Voice {
    pub items: Vec<Item>,
    pub beams: Vec<Beam>,
    pub tuplets: Vec<Tuplet>,
}

/// The metre: bar and beat lengths in ticks, and how beams group.
#[derive(Clone, Copy, Debug)]
pub struct Metre {
    pub bar: u32,
    pub beat: u32,
    pub compound: bool,
    pub sig: (u8, u8),
}

impl Metre {
    pub fn new(sig: (u8, u8)) -> Metre {
        let (n, d) = (sig.0.max(1) as u32, sig.1.max(1) as u32);
        let bar = PPQ * 4 * n / d;
        let compound = d == 8 && n % 3 == 0 && n > 3;
        let beat = if compound { PPQ * 3 / 2 } else { PPQ * 4 / d };
        Metre { bar, beat, compound, sig }
    }
}

/// What to engrave.
#[derive(Clone, Debug)]
pub struct Options {
    pub sig: (u8, u8),
    pub key: Key,
    pub quant: Quant,
    /// The span shown (ticks from bar 1; whole bars).
    pub from: u32,
    pub to: u32,
    /// Logic's region parameters Interpretation (notes lengthened to read simply) and
    /// Syncopation (a note across a beat written as one value).
    pub interpretation: bool,
    pub syncopation: bool,
    /// Max. Dots (region parameter): the most dots a value may have, 0–3 (default 1).
    pub max_dots: u8,
    /// User tuplets in the span (Part box N-tuplet).
    pub tuplets: Vec<TupletSpec>,
    /// No Overlap (region parameter, on by default): a chord shows the length of its shortest
    /// note, up to the next chord; off, its longest.
    pub no_overlap: bool,
}

impl Options {
    pub fn new(sig: (u8, u8), key: Key, quant: Quant, from: u32, to: u32) -> Options {
        Options { sig, key, quant, from, to, interpretation: false, syncopation: false, max_dots: 1, tuplets: Vec::new(), no_overlap: true }
    }
}

/// Quantised onsets: per beat, the straight or the triplet grid, whichever fits the notes
/// starting in it better (Logic's "16,24").
fn quantise(notes: &[NoteIn], m: Metre, q: Quant, from: u32) -> (Vec<(u32, u32, usize)>, Vec<u32>) {
    use std::collections::BTreeMap;
    let mut per_beat: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        let rel = n.pos.saturating_sub(from);
        // the beat whose grid catches the onset (a note just before a beat belongs to it)
        let b = (rel + q.straight / 2) / m.beat;
        per_beat.entry(b).or_default().push(i);
    }
    let mut triplet_beats = Vec::new();
    let mut out = Vec::new();
    let snap = |rel: u32, base: u32, g: u32| base + (((rel as i64 - base as i64) as f64 / g as f64).round().max(0.0) as u32) * g;
    for (b, idx) in per_beat {
        let base = b * m.beat;
        let mut grid = q.straight;
        if let (Some(t), false) = (q.triplet, m.compound) {
            if m.beat % t == 0 {
                let err = |g: u32| idx.iter().map(|&i| {
                    let rel = notes[i].pos.saturating_sub(from);
                    (rel as i64 - snap(rel, base, g) as i64).unsigned_abs()
                }).sum::<u64>();
                let (es, et) = (err(q.straight), err(t));
                // a triplet only when it's clearly better (and a note sits off the straight grid)
                if et + 10 < es {
                    grid = t;
                    triplet_beats.push(from + base);
                }
            }
        }
        for i in idx {
            let n = notes[i];
            let rel = n.pos.saturating_sub(from);
            let on = snap(rel, base, grid);
            // the end, on the grid (at least one grid step long)
            let end_rel = (n.pos + n.len).saturating_sub(from);
            let off = if grid != q.straight { snap(end_rel.max(base), base, grid) } else { ((end_rel as f64) / q.straight as f64).round() as u32 * q.straight };
            let off = off.max(on + grid.min(q.straight));
            out.push((from + on, from + off, i));
        }
    }
    (out, triplet_beats)
}

/// Where the Score shows each note (Fix Displayed Note Positions): (index, on, off) on the
/// display grid, from `from` (a bar line).
pub fn display_positions(notes: &[NoteIn], sig: (u8, u8), q: Quant, from: u32) -> Vec<(usize, u32, u32)> {
    let (out, _) = quantise(notes, Metre::new(sig), q, from);
    let mut v: Vec<(usize, u32, u32)> = out.into_iter().map(|(on, off, i)| (notes[i].index, on, off)).collect();
    v.sort_unstable();
    v
}

/// The values a span is written in from `p` to `e` within one bar (with ties between them).
fn decompose(mut p: u32, e: u32, bar0: u32, m: Metre, triplets: &[u32], rest: bool, synco: bool, max_dots: u8) -> Vec<(u32, u32, Value, bool)> {
    let mut out = Vec::new();
    let cands: [(u32, Value); 19] = [
        (3840 + 1920 + 960 + 480, Value { base: 1, dots: 3 }),
        (3840 + 1920 + 960, Value { base: 1, dots: 2 }),
        (3840 + 1920, Value { base: 1, dots: 1 }),
        (3840, Value { base: 1, dots: 0 }),
        (1920 + 960 + 480 + 240, Value { base: 2, dots: 3 }),
        (1920 + 960 + 480, Value { base: 2, dots: 2 }),
        (2880, Value { base: 2, dots: 1 }),
        (1920, Value { base: 2, dots: 0 }),
        (960 + 480 + 240, Value { base: 4, dots: 2 }),
        (1440, Value { base: 4, dots: 1 }),
        (960, Value { base: 4, dots: 0 }),
        (480 + 240 + 120, Value { base: 8, dots: 2 }),
        (720, Value { base: 8, dots: 1 }),
        (480, Value { base: 8, dots: 0 }),
        (360, Value { base: 16, dots: 1 }),
        (240, Value { base: 16, dots: 0 }),
        (120, Value { base: 32, dots: 0 }),
        (60, Value { base: 64, dots: 0 }),
        (30, Value { base: 128, dots: 0 }),
    ];
    while p < e {
        let rel = p - bar0;
        let beat_start = bar0 + rel / m.beat * m.beat;
        if triplets.contains(&beat_start) {
            let beat_end = beat_start + m.beat;
            let end = e.min(beat_end);
            let unit = m.beat / 3;
            let len = ((end - p) / unit).max(1) * unit;
            if p == beat_start && len == m.beat {
                out.push((p, len, Value { base: (3840 / m.beat) as u8, dots: 0 }, false));
            } else {
                let base = (3840 / (unit * 3 / 2)) as u8;
                for k in 0..len / unit {
                    // two units on a beat's first or second third: a quarter triplet
                    if k == 0 && len / unit == 2 {
                        out.push((p, 2 * unit, Value { base: base / 2, dots: 0 }, true));
                        break;
                    }
                    out.push((p + k * unit, unit, Value { base, dots: 0 }, true));
                }
            }
            p += len;
            continue;
        }
        // straight values up to the next triplet beat
        let mut lim = e;
        for &t in triplets {
            if t > p && t < lim {
                lim = t;
            }
        }
        let half = m.bar / 2;
        let mut chosen = None;
        for &(v, val) in &cands {
            if p + v > lim || rel + v > m.bar || val.dots > max_dots {
                continue;
            }
            // dotted rests only within a beat (a half and a quarter rest, not a dotted half)
            if rest && val.dots > 0 && v > m.beat && !m.compound {
                continue;
            }
            // syncopation: one value across a beat (an eighth, a quarter, an eighth), placed on
            // half its own length and ending inside the bar
            let sync_ok = synco && !rest && val.dots == 0 && v >= 480 && v <= m.beat * 2 && rel % (v / 2) == 0;
            let ok = sync_ok || if v > m.beat {
                // longer than a beat: on a beat, and not across the middle of a 4/4 bar
                rel % m.beat == 0 && !(m.sig == (4, 4) && rel != 0 && rel < half && rel + v > half) && (val.base != 1 || v == m.bar)
            } else if v == m.beat {
                rel % m.beat == 0 || (m.compound && rel % (m.beat / 3) == 0 && (rel % m.beat) + v <= m.beat)
            } else {
                let base = PPQ * 4 / val.base as u32;
                let unit = if m.compound { base.min(m.beat / 3) } else { base };
                rel % unit == 0 && (rel % m.beat) + v <= m.beat
            };
            if ok {
                chosen = Some((v, val));
                break;
            }
        }
        let (v, val) = chosen.unwrap_or((30.min(lim - p).max(1), Value { base: 128, dots: 0 }));
        out.push((p, v, val, false));
        p += v;
    }
    out
}

/// Engraves one staff's notes.
pub fn engrave(notes: &[NoteIn], clef: Clef, o: &Options) -> Voice {
    let m = Metre::new(o.sig);
    // user tuplets inside the span (whole, not overlapping each other)
    let mut tsp: Vec<TupletSpec> = o.tuplets.iter().copied().filter(|t| t.n > 0 && t.start >= o.from && t.end() <= o.to && t.len() > 0).collect();
    tsp.sort_by_key(|t| t.start);
    tsp.dedup_by(|b, a| b.start < a.end());
    let in_tuplet = |pos: u32| tsp.iter().position(|t| pos + (t.unit() / 2.0) as u32 >= t.start && pos < t.end().saturating_sub((t.unit() / 2.0) as u32));
    let outside: Vec<NoteIn> = notes.iter().copied().filter(|n| in_tuplet(n.pos).is_none()).collect();
    let (q, triplets) = quantise(&outside, m, o.quant, o.from);
    let notes_all = notes;
    let notes = &outside[..];
    // chords: notes starting together (on the display grid)
    let mut onsets: Vec<u32> = q.iter().map(|x| x.0).collect();
    onsets.sort_unstable();
    onsets.dedup();
    struct Chord {
        on: u32,
        end: u32,
        members: Vec<usize>,
    }
    let mut chords: Vec<Chord> = Vec::new();
    for (k, &on) in onsets.iter().enumerate() {
        if on >= o.to {
            break;
        }
        let members: Vec<&(u32, u32, usize)> = q.iter().filter(|x| x.0 == on).collect();
        let next = onsets.get(k + 1).copied().unwrap_or(u32::MAX);
        // (a user tuplet starting before the next chord ends this one)
        let next = tsp.iter().map(|t| t.start).filter(|&s| s > on).min().map_or(next, |s| s.min(next));
        // No Overlap (Logic's default): a note ends where the next chord starts, a chord at its
        // shortest note's end; off, at its longest (still no further than the next chord)
        let ends = members.iter().map(|x| x.1);
        let end = if o.no_overlap { ends.min() } else { ends.max() };
        let mut end = end.unwrap_or(on).min(next).min(o.to);
        // Interpretation: a short note reads as the value up to the next eighth (or the next
        // note), so a staccato quarter isn't a sixteenth and rests
        let f = members.iter().map(|x| notes[x.2].interp).find(|&x| x != 0).unwrap_or(0);
        if if f == 0 { o.interpretation } else { f > 0 } {
            let g = (m.beat / 2).max(1);
            let rel = end.saturating_sub(o.from);
            let up = o.from + rel.div_ceil(g) * g;
            end = end.max(up.min(next).min(o.to));
        }
        if on < o.from {
            continue;
        }
        chords.push(Chord { on, end, members: members.iter().map(|x| x.2).collect() });
    }
    let mut v = Voice::default();
    // the timeline: chords and the rests between, cut at bar lines
    let mut segs: Vec<(u32, u32, Option<usize>)> = Vec::new();
    let mut t = o.from;
    for (ci, c) in chords.iter().enumerate() {
        if c.on > t {
            segs.push((t, c.on, None));
        }
        segs.push((c.on, c.end.max(c.on + 1), Some(ci)));
        t = c.end.max(c.on + 1);
    }
    if t < o.to {
        segs.push((t, o.to, None));
    }
    // rests cut around the user tuplets (their spans are written below)
    let mut cut: Vec<(u32, u32, Option<usize>)> = Vec::new();
    for (s, e, c) in segs {
        if c.is_some() {
            cut.push((s, e, c));
            continue;
        }
        let mut a = s;
        for t in tsp.iter().filter(|t| t.start < e && t.end() > s) {
            if t.start > a {
                cut.push((a, t.start, None));
            }
            a = a.max(t.end());
        }
        if a < e {
            cut.push((a, e, None));
        }
    }
    let segs = cut;
    let spell_head = |n: NoteIn, tie: bool| -> Head {
        let sp = o.key.spell_pref(n.pitch, n.spell);
        let (pos, cross) = if clef == Clef::Percussion { drum_position(n.pitch) } else { (sp.step - clef.bottom_line(), false) };
        Head { note: n.index, pitch: n.pitch, spelled: sp, pos, acc: None, tie, shifted: false, tie_dir: if n.tie == 1 || n.tie == -1 { n.tie } else { 0 }, tie_hidden: n.tie == 2, guide: false, cross }
    };
    // the tuplets' items: notes on the tuplet grid, rests in the empty units
    let mut tuplet_items: Vec<(usize, Vec<Item>)> = Vec::new();
    for (ti, t) in tsp.iter().enumerate() {
        let unit = t.unit();
        let mut slots: Vec<Vec<NoteIn>> = vec![Vec::new(); t.n as usize];
        for n in notes_all.iter().filter(|n| in_tuplet(n.pos) == Some(ti)) {
            let k = (((n.pos as f64 - t.start as f64) / unit).round().max(0.0) as usize).min(t.n as usize - 1);
            slots[k].push(*n);
        }
        let mut items = Vec::new();
        let bar = (t.start - o.from) / m.bar;
        let mut k = 0usize;
        while k < t.n as usize {
            let tick = t.start + (k as f64 * unit).round() as u32;
            if slots[k].is_empty() {
                items.push(Item { tick, dur: unit.round() as u32, value: Value { base: t.base, dots: 0 }, heads: vec![], stem_up: true, beam: None, triplet: false, full_bar: false, bar, stem_hidden: false });
                k += 1;
                continue;
            }
            // how many units the chord lasts (to the next onset), as one value where allowed
            let want = slots[k].iter().map(|n| ((n.len as f64 / unit).round() as usize).max(1)).max().unwrap_or(1);
            let free = (k + 1..t.n as usize).take_while(|&j| slots[j].is_empty()).count() + 1;
            let span = want.min(free);
            let (units, value) = if span >= 2 && t.double && t.base >= 2 { (2, Value { base: t.base / 2, dots: 0 }) } else { (1, Value { base: t.base, dots: 0 }) };
            let mut heads: Vec<Head> = slots[k].iter().map(|&n| spell_head(n, false)).collect();
            heads.sort_by_key(|h| h.pos);
            heads.dedup_by_key(|h| h.pitch);
            items.push(Item { tick, dur: (units as f64 * unit).round() as u32, value, heads, stem_up: true, beam: None, triplet: false, full_bar: false, bar, stem_hidden: false });
            k += units;
            // the rest of a longer note: rests (written as Logic does, on the tuplet's grid)
        }
        tuplet_items.push((ti, items));
    }
    for (s, e, chord) in segs {
        let mut p = s;
        while p < e {
            let bar = (p - o.from) / m.bar;
            let bar0 = o.from + bar * m.bar;
            let bar_end = bar0 + m.bar;
            let end = e.min(bar_end);
            if chord.is_none() && p == bar0 && end == bar_end {
                v.items.push(Item { tick: p, dur: m.bar, value: Value { base: 1, dots: 0 }, heads: vec![], stem_up: true, beam: None, triplet: false, full_bar: true, bar, stem_hidden: false });
                p = end;
                continue;
            }
            let synco = chord.is_some_and(|ci| {
                let f = chords[ci].members.iter().map(|&i| notes[i].synco).find(|&x| x != 0).unwrap_or(0);
                if f == 0 { o.syncopation } else { f > 0 }
            });
            let parts = decompose(p, end, bar0, m, &triplets, chord.is_none(), synco, o.max_dots);
            let last_of_note = end == e;
            let n_parts = parts.len();
            for (k, (tick, dur, value, triplet)) in parts.into_iter().enumerate() {
                let heads = match chord {
                    None => vec![],
                    Some(ci) => {
                        let tie = !(last_of_note && k + 1 == n_parts);
                        let mut hs: Vec<Head> = chords[ci].members.iter().map(|&i| {
                            let n = notes[i];
                            let sp = o.key.spell_pref(n.pitch, n.spell);
                            let (pos, cross) = if clef == Clef::Percussion { drum_position(n.pitch) } else { (sp.step - clef.bottom_line(), false) };
                            Head { note: n.index, pitch: n.pitch, spelled: sp, pos, acc: None, tie, shifted: false, tie_dir: if n.tie == 1 || n.tie == -1 { n.tie } else { 0 }, tie_hidden: n.tie == 2, guide: false, cross }
                        }).collect();
                        hs.sort_by_key(|h| h.pos);
                        hs.dedup_by_key(|h| h.pitch);
                        hs
                    }
                };
                v.items.push(Item { tick, dur, value, heads, stem_up: true, beam: None, triplet, full_bar: false, bar, stem_hidden: false });
            }
            p = end;
        }
    }
    for (_, items) in &tuplet_items {
        v.items.extend(items.iter().cloned());
    }
    v.items.sort_by_key(|it| it.tick);
    let notes = notes_all;
    if clef != Clef::Percussion {
        accidentals(&mut v, o.key);
    }
    let attr = |i: usize| notes.iter().find(|n| n.index == i).copied().unwrap_or_default();
    // the per-note accidental modes
    for it in &mut v.items {
        for h in &mut it.heads {
            let a = attr(h.note);
            match a.acc {
                1 => h.acc = Some(h.spelled.alter),
                2 => h.acc = None,
                3 => {
                    h.acc = Some(h.spelled.alter);
                    h.guide = true;
                }
                4 if h.spelled.alter != 0 => h.acc = Some(h.spelled.alter),
                _ => {}
            }
        }
    }
    let stem_of = |it: &Item| it.heads.iter().map(|h| attr(h.note).stem).find(|&s| s != 0).unwrap_or(0);
    let beam_of = |it: &Item| it.heads.iter().map(|h| attr(h.note).beam).find(|&s| s != 0).unwrap_or(0);
    let flags: Vec<(i8, i8)> = v.items.iter().map(|it| (stem_of(it), beam_of(it))).collect();
    stems_and_beams(&mut v, m, &flags, clef == Clef::Percussion);
    for t in &tsp {
        let idx: Vec<usize> = (0..v.items.len()).filter(|&i| v.items[i].tick >= t.start && v.items[i].tick < t.end()).collect();
        let (Some(&first), Some(&last)) = (idx.first(), idx.last()) else { continue };
        let ps: Vec<i32> = idx.iter().flat_map(|&i| v.items[i].heads.iter().map(|h| h.pos)).collect();
        let auto = v.items[first].beam.map_or(ps.iter().sum::<i32>() < 4 * ps.len().max(1) as i32, |b| v.beams[b].up);
        let up = match t.dir {
            1 => true,
            -1 => false,
            _ => auto,
        };
        v.tuplets.push(Tuplet { first, last, up, n: t.n, of: t.of, base: t.base, spec: Some(*t) });
    }
    v.tuplets.sort_by_key(|t| t.first);
    v
}

/// A drum staff's position (half-spaces up from the bottom line) for a General MIDI drum, and
/// whether it takes a cross head (Logic's default drum mapping: kick in the bottom space, snare
/// on the third space, hi-hats and cymbals above the staff in crosses).
pub fn drum_position(pitch: u8) -> (i32, bool) {
    match pitch {
        35 | 36 => (1, false),
        37 | 38 | 39 | 40 => (5, false),
        41 | 43 => (2, false),
        45 | 47 => (6, false),
        48 | 50 => (7, false),
        42 | 46 => (9, true),
        44 => (-1, true),
        49 | 52 | 55 | 57 => (10, true),
        51 | 53 | 59 => (8, true),
        54 | 56 | 69 | 70 | 75 => (9, false),
        p if p < 35 => (0, false),
        _ => (4, false),
    }
}

/// Accidentals: shown where a note differs from the key signature or from an earlier note on
/// the same line or space in the bar; tied continuations need none.
fn accidentals(v: &mut Voice, key: Key) {
    use std::collections::HashMap;
    let sig = key.letter_alters();
    let mut bar = u32::MAX;
    let mut state: HashMap<i32, i8> = HashMap::new();
    let mut tied_from: Vec<u8> = Vec::new();
    for it in &mut v.items {
        if it.bar != bar {
            bar = it.bar;
            state.clear();
        }
        let mut next_tied = Vec::new();
        for h in &mut it.heads {
            let cur = state.get(&h.spelled.step).copied().unwrap_or(sig[h.spelled.letter as usize]);
            if tied_from.contains(&h.pitch) {
                state.insert(h.spelled.step, h.spelled.alter);
            } else if cur != h.spelled.alter {
                h.acc = Some(h.spelled.alter);
                state.insert(h.spelled.step, h.spelled.alter);
            }
            if h.tie {
                next_tied.push(h.pitch);
            }
        }
        tied_from = next_tied;
    }
}

fn stems_and_beams(v: &mut Voice, m: Metre, flags: &[(i8, i8)], drums: bool) {
    // beam groups: notes shorter than a quarter, within a beat (a 4/4 half bar of plain eighths
    // as one group, as Logic does), broken by rests
    let unit_of = |it: &Item| -> u32 {
        let rel = it.tick % m.bar;
        rel / m.beat
    };
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_key = (u32::MAX, u32::MAX);
    for (i, it) in v.items.iter().enumerate() {
        let beamable = !it.is_rest() && it.value.flags() > 0 && flags.get(i).is_none_or(|f| f.1 >= 0);
        let key = (it.bar, unit_of(it));
        if !beamable || key != cur_key {
            if cur.len() > 1 {
                groups.push(std::mem::take(&mut cur));
            }
            cur.clear();
        }
        if beamable {
            cur.push(i);
            cur_key = key;
        } else {
            cur_key = (u32::MAX, u32::MAX);
        }
    }
    if cur.len() > 1 {
        groups.push(cur);
    }
    // 4/4: two beats of plain straight eighths on a half bar → one group of four
    if m.sig == (4, 4) {
        let plain = |g: &Vec<usize>, v: &Voice| g.len() == 2 && g.iter().all(|&i| v.items[i].value == Value { base: 8, dots: 0 } && !v.items[i].triplet);
        let mut merged: Vec<Vec<usize>> = Vec::new();
        let mut k = 0;
        while k < groups.len() {
            if k + 1 < groups.len() && plain(&groups[k], v) && plain(&groups[k + 1], v) {
                let a = &v.items[groups[k][0]];
                let b = &v.items[groups[k + 1][0]];
                let rel = a.tick % m.bar;
                if a.bar == b.bar && rel % (m.bar / 2) == 0 && b.tick == a.tick + m.beat {
                    let mut g = groups[k].clone();
                    g.extend(groups[k + 1].iter().copied());
                    merged.push(g);
                    k += 2;
                    continue;
                }
            }
            merged.push(groups[k].clone());
            k += 1;
        }
        groups = merged;
    }
    // Beam Selected: neighbouring groups (and lone flagged notes) whose notes are all marked
    // beam into one
    let marked = |i: usize| flags.get(i).is_some_and(|f| f.1 > 0) && !v.items[i].is_rest() && v.items[i].value.flags() > 0;
    let mut joined: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < v.items.len() {
        if marked(i) {
            let mut g = vec![i];
            while i + 1 < v.items.len() && marked(i + 1) {
                i += 1;
                g.push(i);
            }
            if g.len() > 1 {
                joined.push(g);
            }
        }
        i += 1;
    }
    if !joined.is_empty() {
        groups.retain(|g| !g.iter().any(|x| joined.iter().any(|j| j.contains(x))));
        groups.extend(joined);
        groups.sort_by_key(|g| g[0]);
    }
    // stems: away from the middle line (down from it), a group by its notes' average
    let dir = |positions: &[i32]| -> bool {
        if positions.is_empty() {
            return true;
        }
        let hi = *positions.iter().max().unwrap();
        let lo = *positions.iter().min().unwrap();
        // the note furthest from the middle line decides
        (hi - 4).abs() <= (4 - lo).abs() && (hi + lo) < 8 || (hi - 4).abs() < (4 - lo).abs()
    };
    for (k, it) in v.items.iter_mut().enumerate() {
        let ps: Vec<i32> = it.heads.iter().map(|h| h.pos).collect();
        // drums: stems up; Stems ▸ Up / Down / Hide override
        it.stem_up = drums || dir(&ps);
        match flags.get(k).map_or(0, |f| f.0) {
            1 => it.stem_up = true,
            -1 => it.stem_up = false,
            2 => it.stem_hidden = true,
            _ => {}
        }
    }
    for g in groups {
        let ps: Vec<i32> = g.iter().flat_map(|&i| v.items[i].heads.iter().map(|h| h.pos)).collect();
        let mean = ps.iter().sum::<i32>() as f32 / ps.len().max(1) as f32;
        let forced = g.iter().map(|&i| flags.get(i).map_or(0, |f| f.0)).find(|&s| s == 1 || s == -1);
        let up = match forced {
            Some(s) => s > 0,
            None => drums || mean < 4.0,
        };
        let b = v.beams.len();
        for &i in &g {
            v.items[i].stem_up = up;
            v.items[i].beam = Some(b);
        }
        v.beams.push(Beam { items: g, up });
    }
    // seconds in chords: the upper head of a second to the right of an up stem, the lower to
    // the left of a down stem
    for it in v.items.iter_mut() {
        let n = it.heads.len();
        if n < 2 {
            continue;
        }
        if it.stem_up {
            for k in 1..n {
                if it.heads[k].pos - it.heads[k - 1].pos == 1 && !it.heads[k - 1].shifted {
                    it.heads[k].shifted = true;
                }
            }
        } else {
            for k in (0..n - 1).rev() {
                if it.heads[k + 1].pos - it.heads[k].pos == 1 && !it.heads[k + 1].shifted {
                    it.heads[k].shifted = true;
                }
            }
        }
    }
    // triplet brackets: per beat
    let mut k = 0;
    while k < v.items.len() {
        if v.items[k].triplet {
            let beat = v.items[k].tick / m.beat;
            let mut j = k;
            while j + 1 < v.items.len() && v.items[j + 1].triplet && v.items[j + 1].tick / m.beat == beat {
                j += 1;
            }
            let ps: Vec<i32> = (k..=j).flat_map(|i| v.items[i].heads.iter().map(|h| h.pos)).collect();
            let up = v.items[k].beam.map_or(ps.iter().sum::<i32>() < 4 * ps.len().max(1) as i32, |b| v.beams[b].up);
            v.tuplets.push(Tuplet { first: k, last: j, up, n: 3, of: 2, base: 0, spec: None });
            k = j + 1;
        } else {
            k += 1;
        }
    }
}


/// Logic's staff styles (Layout ▸ Staff Styles): the staves a part is written on, their clefs,
/// and the transposition of written notes (a B♭ trumpet is written a tone above sounding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaffStyle {
    pub name: &'static str,
    /// Clefs top to bottom (two for the grand staff); notes from `split` up go on the first.
    pub clefs: &'static [Clef],
    pub split: u8,
    /// Semitones added to the sounding pitch to write it.
    pub transpose: i8,
    /// The clef's octave sign: 1 = 8va, −1 = 8vb.
    pub octave: i8,
}

impl StaffStyle {
    pub const ALL: [StaffStyle; 16] = [
        StaffStyle { name: "Treble", clefs: &[Clef::Treble], split: 0, transpose: 0, octave: 0 },
        StaffStyle { name: "Treble 8va", clefs: &[Clef::Treble], split: 0, transpose: -12, octave: 1 },
        StaffStyle { name: "Treble 8vb", clefs: &[Clef::Treble], split: 0, transpose: 12, octave: -1 },
        StaffStyle { name: "Bass", clefs: &[Clef::Bass], split: 0, transpose: 0, octave: 0 },
        StaffStyle { name: "Bass 8vb", clefs: &[Clef::Bass], split: 0, transpose: 12, octave: -1 },
        StaffStyle { name: "Piano", clefs: &[Clef::Treble, Clef::Bass], split: 60, transpose: 0, octave: 0 },
        StaffStyle { name: "Alto Clef", clefs: &[Clef::Alto], split: 0, transpose: 0, octave: 0 },
        StaffStyle { name: "Tenor Clef", clefs: &[Clef::Tenor], split: 0, transpose: 0, octave: 0 },
        StaffStyle { name: "Drums", clefs: &[Clef::Percussion], split: 0, transpose: 0, octave: 0 },
        StaffStyle { name: "Guitar", clefs: &[Clef::Treble], split: 0, transpose: 12, octave: -1 },
        StaffStyle { name: "Bass Guitar", clefs: &[Clef::Bass], split: 0, transpose: 12, octave: -1 },
        StaffStyle { name: "B♭ Trumpet", clefs: &[Clef::Treble], split: 0, transpose: 2, octave: 0 },
        StaffStyle { name: "B♭ Clarinet", clefs: &[Clef::Treble], split: 0, transpose: 2, octave: 0 },
        StaffStyle { name: "E♭ Alto Sax", clefs: &[Clef::Treble], split: 0, transpose: 9, octave: 0 },
        StaffStyle { name: "B♭ Tenor Sax", clefs: &[Clef::Treble], split: 0, transpose: 14, octave: 0 },
        StaffStyle { name: "F Horn", clefs: &[Clef::Treble], split: 0, transpose: 7, octave: 0 },
    ];

    pub fn by_name(name: &str) -> Option<StaffStyle> {
        StaffStyle::ALL.iter().copied().find(|s| s.name == name)
    }

    /// Logic's automatic choice (by the notes' range, a piano part on the grand staff).
    pub fn auto(pitches: &[u8], piano: bool) -> StaffStyle {
        match staves_for(pitches, piano) {
            Staves::Treble => StaffStyle::ALL[0],
            Staves::Bass => StaffStyle::ALL[3],
            Staves::Piano => StaffStyle::ALL[5],
        }
    }

    /// The key a transposing style writes in (concert key moved by the transposition).
    pub fn written_key(self, key: Key) -> Key {
        if self.transpose == 0 || self.clefs[0] == Clef::Percussion {
            return key;
        }
        // each semitone up is seven fifths up; keep the signature within seven
        let mut f = key.fifths as i32 + 7 * self.transpose as i32;
        f = f.rem_euclid(12);
        if f > 6 {
            f -= 12;
        }
        Key { fifths: f as i8 }
    }

    /// Which staff (index into `clefs`) a sounding pitch goes on.
    pub fn staff_of(self, pitch: u8) -> usize {
        if self.clefs.len() > 1 && pitch < self.split { 1 } else { 0 }
    }

    /// The pitch as written.
    pub fn written(self, pitch: u8) -> u8 {
        if self.clefs[0] == Clef::Percussion {
            return pitch;
        }
        (pitch as i32 + self.transpose as i32).clamp(0, 127) as u8
    }
}

/// Voice separation (Logic's polyphonic staff styles, Explode Polyphony): notes that start
/// while another, started earlier, still sounds go to a second voice; the upper voice keeps
/// the higher line. Returns each note's voice, 0 (upper, stems up) or 1 (lower, stems down).
pub fn split_voices(notes: &[NoteIn]) -> Vec<u8> {
    use std::collections::BTreeMap;
    // per onset, sub-chords by where they end
    let mut onsets: BTreeMap<u32, BTreeMap<u32, Vec<usize>>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        onsets.entry(n.pos).or_default().entry(n.pos + n.len.max(1)).or_default().push(i);
    }
    let mut voice = vec![0u8; notes.len()];
    let mut ends = [0u32; 2];
    for (pos, subs) in onsets {
        let top = subs.iter().max_by_key(|(_, m)| m.iter().map(|&i| notes[i].pitch).max()).map(|(e, _)| *e);
        for (end, members) in subs {
            let v = if Some(end) == top && ends[0] <= pos {
                0
            } else if ends[1] <= pos || Some(end) != top {
                1
            } else {
                0
            };
            for &i in &members {
                voice[i] = v;
            }
            ends[v as usize] = ends[v as usize].max(end);
        }
    }
    // a lower voice of single stray notes, nothing overlapping at all: one voice after all
    if !notes.iter().enumerate().any(|(i, n)| voice[i] == 1 && notes.iter().enumerate().any(|(j, m)| voice[j] == 0 && m.pos < n.pos + n.len && n.pos < m.pos + m.len)) {
        return vec![0; notes.len()];
    }
    voice
}

/// Wrapped and Page views: bars into lines (systems). Each bar asks for `widths[k]` points; a
/// line holds bars while they fit `line_w` (at least one), and the bars of a line are then
/// stretched to fill it. Returns (first bar, bars on the line, scale) per line.
pub fn systems(widths: &[f32], line_w: f32) -> Vec<(usize, usize, f32)> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < widths.len() {
        let mut w = 0.0;
        let mut n = 0;
        while k + n < widths.len() && (n == 0 || w + widths[k + n] <= line_w) {
            w += widths[k + n];
            n += 1;
        }
        // the last line isn't stretched more than half again (Logic's ragged last line)
        let last = k + n == widths.len();
        let scale = if w > 0.0 { (line_w / w).min(if last { 1.5 } else { f32::MAX }) } else { 1.0 };
        out.push((k, n, scale));
        k += n;
    }
    out
}

/// `systems` with Logic's Global settings: at most `max_bars` bars on a line, and the last line
/// stretched to the full width when `justify_last` (else ragged, as `systems`).
pub fn systems_with(widths: &[f32], line_w: f32, max_bars: usize, justify_last: bool) -> Vec<(usize, usize, f32)> {
    let max_bars = max_bars.max(1);
    let mut out = Vec::new();
    let mut k = 0;
    while k < widths.len() {
        let mut w = 0.0;
        let mut n = 0;
        while k + n < widths.len() && n < max_bars && (n == 0 || w + widths[k + n] <= line_w) {
            w += widths[k + n];
            n += 1;
        }
        let last = k + n == widths.len();
        let scale = if w > 0.0 { (line_w / w).min(if last && !justify_last { 1.5 } else { f32::MAX }) } else { 1.0 };
        out.push((k, n, scale));
        k += n;
    }
    out
}

/// `paginate` with separate top and bottom margins and a header space on the first page.
pub fn paginate_with(lines: usize, line_h: f32, page_h: f32, top: f32, bottom: f32, header: f32) -> Vec<(usize, f32)> {
    let mut out = Vec::new();
    let (mut page, mut y) = (0usize, top + header);
    for _ in 0..lines {
        if y + line_h > page_h - bottom && y > top + if page == 0 { header } else { 0.0 } {
            page += 1;
            y = top;
        }
        out.push((page, y));
        y += line_h;
    }
    out
}

/// `systems_with` honouring manual breaks: `brk(k)` for bar `k` is 1 for a line break before it
/// (Line or Page Break), −1 for none allowed there (No Line Break: the bars may then overflow
/// the line), 0 automatic.
pub fn systems_breaks(widths: &[f32], line_w: f32, max_bars: usize, justify_last: bool, brk: &dyn Fn(usize) -> i8) -> Vec<(usize, usize, f32)> {
    let max_bars = max_bars.max(1);
    let mut out = Vec::new();
    let mut k = 0;
    while k < widths.len() {
        let mut w = 0.0;
        let mut n = 0;
        while k + n < widths.len() {
            let at = k + n;
            if n > 0 && brk(at) > 0 {
                break;
            }
            let fits = n == 0 || (n < max_bars && w + widths[at] <= line_w);
            if !fits && brk(at) >= 0 {
                break;
            }
            w += widths[at];
            n += 1;
        }
        let last = k + n == widths.len();
        let scale = if w > 0.0 { (line_w / w).min(if last && !justify_last { 1.5 } else { f32::MAX }) } else { 1.0 };
        out.push((k, n, scale));
        k += n;
    }
    out
}

/// `paginate_with` honouring page breaks: `brk(line)` is 1 to start a new page with that line,
/// −1 to keep it on the page of the line before (No Page Break), 0 automatic.
pub fn paginate_breaks(lines: usize, line_h: f32, page_h: f32, top: f32, bottom: f32, header: f32, brk: &dyn Fn(usize) -> i8) -> Vec<(usize, f32)> {
    let mut out = Vec::new();
    let (mut page, mut y) = (0usize, top + header);
    for l in 0..lines {
        let b = brk(l);
        let full = y + line_h > page_h - bottom && y > top + if page == 0 { header } else { 0.0 };
        if l > 0 && (b > 0 || (full && b >= 0)) {
            page += 1;
            y = top;
        }
        out.push((page, y));
        y += line_h;
    }
    out
}

/// The width a bar needs: a head room, then per item by its value (shorter values closer).
pub fn bar_width(v: &Voice, bar: u32, space: f32) -> f32 {
    let items = v.items.iter().filter(|it| it.bar == bar);
    let mut w = 2.0 * space;
    for it in items {
        let base = match it.value.base {
            1 if it.full_bar => 6.0,
            1 => 5.0,
            2 => 4.0,
            4 => 3.2,
            8 => 2.6,
            _ => 2.2,
        };
        w += base * space + it.heads.iter().filter(|h| h.acc.is_some()).count().min(1) as f32 * space;
    }
    w.max(12.0 * space)
}

/// Pages for the Page view: lines of `line_h` on pages of `page_h` (after a title block on the
/// first). Returns (page, y on the page) per line.
pub fn paginate(lines: usize, line_h: f32, page_h: f32, top: f32, title: f32) -> Vec<(usize, f32)> {
    let mut out = Vec::new();
    let (mut page, mut y) = (0usize, top + title);
    for _ in 0..lines {
        if y + line_h > page_h - top && y > top + if page == 0 { title } else { 0.0 } {
            page += 1;
            y = top;
        }
        out.push((page, y));
        y += line_h;
    }
    out
}

/// Splits a part across a grand staff at middle C.
pub fn split_piano(notes: &[NoteIn]) -> (Vec<NoteIn>, Vec<NoteIn>) {
    notes.iter().partition(|n| n.pitch >= 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systems_max_bars_and_justify() {
        let w = vec![10.0; 7];
        let s = systems_with(&w, 100.0, 3, false);
        assert_eq!(s.iter().map(|x| x.1).collect::<Vec<_>>(), vec![3, 3, 1]);
        assert!((s[2].2 - 1.5).abs() < 1e-6, "ragged last line");
        let j = systems_with(&w, 100.0, 3, true);
        assert!((j[2].2 - 10.0).abs() < 1e-6, "justified last line");
        let p = paginate_with(5, 30.0, 100.0, 5.0, 20.0, 10.0);
        assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 0, 1, 1, 2]);
    }

    fn opts(bars: u32) -> Options {
        Options::new((4, 4), Key::default(), Quant::default(), 0, bars * 3840)
    }
    fn n(i: usize, pitch: u8, pos: u32, len: u32) -> NoteIn {
        NoteIn::new(i, pitch, pos, len)
    }
    fn shape(v: &Voice) -> Vec<String> {
        v.items.iter().map(|it| {
            let mut s = if it.is_rest() { format!("r{}", it.value.base) } else { format!("n{}", it.value.base) };
            for _ in 0..it.value.dots {
                s.push('.');
            }
            if it.triplet {
                s.push('t');
            }
            if it.heads.iter().any(|h| h.tie) {
                s.push('~');
            }
            s
        }).collect()
    }

    #[test]
    fn keys_parse() {
        assert_eq!(Key::parse("C major").fifths, 0);
        assert_eq!(Key::parse("A minor").fifths, 0);
        assert_eq!(Key::parse("E♭ major").fifths, -3);
        assert_eq!(Key::parse("F# minor").fifths, 3);
        assert_eq!(Key::parse("B♭ minor").fifths, -5);
        assert_eq!(Key::parse("G major").letter_alters(), [0, 0, 0, 1, 0, 0, 0]);
    }

    #[test]
    fn spelling_by_key() {
        let c = Key::default();
        assert_eq!(c.spell(61).name(), "C♯");
        assert_eq!(c.spell(63).name(), "E♭");
        assert_eq!(c.spell(70).name(), "B♭");
        assert_eq!(c.spell(60).step, 35);
        let f = Key::parse("F major");
        assert_eq!(f.spell(70).name(), "B♭");
        assert_eq!(f.spell(66).name(), "G♭");
        let fs = Key::parse("F# major");
        assert_eq!(fs.spell(65).name(), "E♯");
        let e = Key::parse("E major");
        assert_eq!(e.spell(67).name(), "F𝄪");
        let gb = Key::parse("Gb major");
        // C♭ is written in the octave below B's
        assert_eq!(gb.spell(59).name(), "C♭");
        assert_eq!(gb.spell(59).step, 35);
    }

    #[test]
    fn staff_positions() {
        // middle C: one ledger line below the treble staff
        assert_eq!(Key::default().spell(60).step - Clef::Treble.bottom_line(), -2);
        assert_eq!(Key::default().spell(43).step - Clef::Bass.bottom_line(), 0);
        assert_eq!(pitch_at(Clef::Treble, 4, Key::default()), 71);
        assert_eq!(pitch_at(Clef::Treble, 8, Key::parse("D major")), 78);
        assert_eq!(Key::parse("A major").signature_positions(Clef::Treble), vec![(8, 1), (5, 1), (9, 1)]);
        assert_eq!(Key::parse("Bb major").signature_positions(Clef::Bass), vec![(2, -1), (5, -1)]);
    }

    #[test]
    fn accidentals_per_bar() {
        let notes = [n(0, 61, 0, 960), n(1, 61, 960, 960), n(2, 60, 1920, 960), n(3, 61, 3840, 960)];
        let v = engrave(&notes, Clef::Treble, &opts(2));
        let acc: Vec<Option<i8>> = v.items.iter().filter(|i| !i.is_rest()).map(|i| i.heads[0].acc).collect();
        // C♯, C♯ (none: still sharp), C♮, then the next bar C♯ again
        assert_eq!(acc, vec![Some(1), None, Some(0), Some(1)]);
        // in D major the C♯ is in the signature
        let mut o = opts(1);
        o.key = Key::parse("D major");
        let v = engrave(&notes[..1], Clef::Treble, &o);
        assert_eq!(v.items[0].heads[0].acc, None);
    }

    #[test]
    fn rests_fill_the_bar() {
        let v = engrave(&[n(0, 64, 960, 960)], Clef::Treble, &opts(1));
        assert_eq!(shape(&v), ["r4", "n4", "r2"]);
        let v = engrave(&[], Clef::Treble, &opts(2));
        assert!(v.items.iter().all(|i| i.full_bar) && v.items.len() == 2);
        // a 16th on the "e" of beat 1 (Logic's screenshot: quarter rest, 8th. rest … )
        let v = engrave(&[n(0, 61, 1680, 240)], Clef::Treble, &opts(1));
        assert_eq!(shape(&v), ["r4", "r8.", "n16", "r2"]);
    }

    #[test]
    fn ties_across_beats_and_bars() {
        // two beats from beat 2: not a half across the middle of the bar
        let v = engrave(&[n(0, 64, 960, 1920)], Clef::Treble, &opts(1));
        assert_eq!(shape(&v), ["r4", "n4~", "n4", "r4"]);
        // from beat 4 into the next bar
        let v = engrave(&[n(0, 64, 2880, 1920)], Clef::Treble, &opts(2));
        assert_eq!(shape(&v), ["r2", "r4", "n4~", "n4", "r4", "r2"]);
        // a dotted half from the downbeat stays one note
        let v = engrave(&[n(0, 64, 0, 2880)], Clef::Treble, &opts(1));
        assert_eq!(shape(&v), ["n2.", "r4"]);
    }

    #[test]
    fn chords_share_a_stem() {
        let v = engrave(&[n(0, 60, 0, 3840), n(1, 64, 5, 3840), n(2, 67, 0, 3800)], Clef::Treble, &opts(1));
        assert_eq!(v.items.len(), 1);
        assert_eq!(v.items[0].heads.len(), 3);
        assert_eq!(v.items[0].value, Value { base: 1, dots: 0 });
        // a second: one head on the other side of the stem
        let v = engrave(&[n(0, 64, 0, 960), n(1, 65, 0, 960)], Clef::Treble, &opts(1));
        assert_eq!(v.items[0].heads.iter().filter(|h| h.shifted).count(), 1);
    }

    #[test]
    fn beaming() {
        let eighths: Vec<NoteIn> = (0..8).map(|k| n(k, 72, k as u32 * 480, 480)).collect();
        let v = engrave(&eighths, Clef::Treble, &opts(1));
        assert_eq!(v.beams.iter().map(|b| b.items.len()).collect::<Vec<_>>(), [4, 4]);
        // high notes: stems down
        assert!(v.beams.iter().all(|b| !b.up));
        let sixteenths: Vec<NoteIn> = (0..16).map(|k| n(k, 62, k as u32 * 240, 240)).collect();
        let v = engrave(&sixteenths, Clef::Treble, &opts(1));
        assert_eq!(v.beams.iter().map(|b| b.items.len()).collect::<Vec<_>>(), [4, 4, 4, 4]);
        assert!(v.beams.iter().all(|b| b.up));
        // a rest breaks a beam; an eighth alone gets a flag
        let v = engrave(&[n(0, 67, 0, 480), n(1, 67, 960, 240), n(2, 67, 1200, 240)], Clef::Treble, &opts(1));
        assert_eq!(shape(&v), ["n8", "r8", "n16", "n16", "r8", "r2"]);
        assert_eq!(v.beams.len(), 1);
        assert_eq!(v.items[0].beam, None);
    }

    #[test]
    fn triplets() {
        let t: Vec<NoteIn> = (0..3).map(|k| n(k, 67, k as u32 * 320, 320)).collect();
        let v = engrave(&t, Clef::Treble, &opts(1));
        assert_eq!(&shape(&v)[..3], ["n8t", "n8t", "n8t"]);
        assert_eq!(v.tuplets.len(), 1);
        assert_eq!(v.beams[0].items.len(), 3);
        // straight sixteenths stay straight with 16,24
        let s: Vec<NoteIn> = (0..4).map(|k| n(k, 67, k as u32 * 240, 240)).collect();
        assert!(engrave(&s, Clef::Treble, &opts(1)).tuplets.is_empty());
    }

    #[test]
    fn user_tuplets() {
        // five sixteenths in the time of four on beat 2, a quarter on beat 1
        let mut t: Vec<NoteIn> = (0..5).map(|k| n(k, 67, 960 + k as u32 * 192, 192)).collect();
        t.push(n(9, 60, 0, 1200));
        let mut o = opts(1);
        o.tuplets.push(TupletSpec { start: 960, n: 5, of: 4, base: 16, ..Default::default() });
        let v = engrave(&t, Clef::Treble, &o);
        assert_eq!(&shape(&v)[..6], ["n4", "n16", "n16", "n16", "n16", "n16"]);
        assert_eq!(v.tuplets.len(), 1);
        assert_eq!((v.tuplets[0].n, v.tuplets[0].first, v.tuplets[0].last), (5, 1, 5));
        // 8:8/32 in an eighth: a run of 32nds whatever the Quantize
        let r: Vec<NoteIn> = (0..4).map(|k| n(k, 67, k as u32 * 120, 120)).collect();
        let mut o = opts(1);
        o.quant = Quant::CHOICES.iter().find(|c| c.0 == "8").unwrap().1;
        o.tuplets.push(TupletSpec { start: 0, n: 8, of: 8, base: 32, hide_bracket: true, hide_number: true, ..Default::default() });
        let v = engrave(&r, Clef::Treble, &o);
        assert_eq!(&shape(&v)[..8], ["n32", "n32", "n32", "n32", "r32", "r32", "r32", "r32"]);
        // an empty tuplet shows as many rests as its count
        let mut o = opts(1);
        o.tuplets.push(TupletSpec { start: 1920, n: 7, of: 4, base: 16, ..Default::default() });
        assert_eq!(engrave(&[], Clef::Treble, &o).items.iter().filter(|i| i.is_rest() && i.value.base == 16).count(), 7);
    }

    #[test]
    fn display_quantisation() {
        // a sloppy note a little early plays on beat 2
        let v = engrave(&[n(0, 64, 950, 1000)], Clef::Treble, &opts(1));
        assert_eq!(v.items[1].tick, 960);
        assert_eq!(shape(&v), ["r4", "n4", "r2"]);
    }

    #[test]
    fn other_metres() {
        let mut o = opts(1);
        o.sig = (3, 4);
        o.to = 2880;
        let v = engrave(&[n(0, 64, 0, 960)], Clef::Treble, &o);
        assert_eq!(shape(&v), ["n4", "r2"]);
        o.sig = (6, 8);
        let v = engrave(&(0..6).map(|k| n(k, 72, k as u32 * 480, 480)).collect::<Vec<_>>(), Clef::Treble, &o);
        assert_eq!(v.beams.iter().map(|b| b.items.len()).collect::<Vec<_>>(), [3, 3]);
    }

    #[test]
    fn staff_choice() {
        assert_eq!(staves_for(&[60, 64, 67, 72], false), Staves::Treble);
        assert_eq!(staves_for(&[36, 40, 43], false), Staves::Bass);
        assert_eq!(staves_for(&[36, 48, 64, 72], false), Staves::Piano);
        assert_eq!(staves_for(&[52, 64], true), Staves::Piano);
    }

    #[test]
    fn fixed_positions() {
        let v = display_positions(&[n(0, 64, 950, 1000), n(1, 67, 10, 230)], (4, 4), Quant::default(), 0);
        assert_eq!(v, [(0, 960, 1920), (1, 0, 240)]);
    }

    #[test]
    fn enharmonic_shift() {
        let c = Key::default();
        assert_eq!(c.spell_pref(63, 1).name(), "D♯");
        assert_eq!(c.spell_pref(61, -1).name(), "D♭");
        assert_eq!(c.spell_pref(61, 1).name(), "C♯");
        // a natural shifted: E as F♭ needs a flat on F
        assert_eq!(c.spell_pref(64, -1).name(), "F♭");
        assert_eq!(c.spell_pref(60, 1).name(), "B♯");
        assert_eq!(c.spell_pref(60, 1).step, 34);
        let v = engrave(&[NoteIn { spell: 1, ..n(0, 63, 0, 960) }], Clef::Treble, &opts(1));
        assert_eq!(v.items[0].heads[0].spelled.name(), "D♯");
        assert_eq!(v.items[0].heads[0].acc, Some(1));
    }

    #[test]
    fn stem_and_beam_overrides() {
        // a high note: stem down by default, Stems ▸ Up forces it
        let v = engrave(&[NoteIn { stem: 1, ..n(0, 79, 0, 960) }], Clef::Treble, &opts(1));
        assert!(v.items[0].stem_up);
        let v = engrave(&[NoteIn { stem: 2, ..n(0, 79, 0, 960) }], Clef::Treble, &opts(1));
        assert!(v.items[0].stem_hidden);
        // Unbeam: two eighths no longer share a beam
        let e = [NoteIn { beam: -1, ..n(0, 67, 0, 480) }, NoteIn { beam: -1, ..n(1, 67, 480, 480) }];
        assert!(engrave(&e, Clef::Treble, &opts(1)).beams.is_empty());
        // Beam Selected across the beat: four eighths over two beats become one beam (3/4)
        let mut o = opts(1);
        o.sig = (3, 4);
        o.to = 2880;
        let e: Vec<NoteIn> = (0..4).map(|k| NoteIn { beam: 1, ..n(k, 67, 480 + k as u32 * 480, 480) }).collect();
        let v = engrave(&e, Clef::Treble, &o);
        assert_eq!(v.beams.len(), 1);
        assert_eq!(v.beams[0].items.len(), 4);
    }

    #[test]
    fn accidental_modes() {
        // Force on a note the key already gives: a natural shown; Hide drops a needed one
        let v = engrave(&[NoteIn { acc: 1, ..n(0, 60, 0, 960) }], Clef::Treble, &opts(1));
        assert_eq!(v.items[0].heads[0].acc, Some(0));
        let v = engrave(&[NoteIn { acc: 2, ..n(0, 61, 0, 960) }], Clef::Treble, &opts(1));
        assert_eq!(v.items[0].heads[0].acc, None);
        let v = engrave(&[NoteIn { acc: 3, ..n(0, 60, 0, 960) }], Clef::Treble, &opts(1));
        assert!(v.items[0].heads[0].guide);
    }

    #[test]
    fn interpretation_and_syncopation() {
        // a staccato quarter (100 ticks): sixteenth and rests plain, an eighth interpreted
        let short = [n(0, 67, 0, 100)];
        assert_eq!(shape(&engrave(&short, Clef::Treble, &opts(1)))[0], "n16");
        let mut o = opts(1);
        o.interpretation = true;
        assert_eq!(shape(&engrave(&short, Clef::Treble, &o))[0], "n8");
        // eighth, quarter, eighth: the quarter across beat 2 tied, or one value syncopated
        let s = [n(0, 67, 0, 480), n(1, 67, 480, 960), n(2, 67, 1440, 480)];
        assert_eq!(&shape(&engrave(&s, Clef::Treble, &opts(1)))[..4], ["n8", "n8~", "n8", "n8"]);
        let mut o = opts(1);
        o.syncopation = true;
        assert_eq!(&shape(&engrave(&s, Clef::Treble, &o))[..3], ["n8", "n4", "n8"]);
    }

    #[test]
    fn drum_staff() {
        let v = engrave(&[n(0, 36, 0, 240), n(1, 42, 0, 240), n(2, 38, 960, 240)], Clef::Percussion, &opts(1));
        let first = &v.items[0];
        assert_eq!(first.heads.iter().map(|h| (h.pos, h.cross)).collect::<Vec<_>>(), [(1, false), (9, true)]);
        assert!(first.stem_up);
        assert!(v.items.iter().all(|it| it.heads.iter().all(|h| h.acc.is_none())));
    }

    #[test]
    fn staff_styles() {
        let t = StaffStyle::by_name("B♭ Trumpet").unwrap();
        assert_eq!(t.written(60), 62);
        // concert E♭ major is written F major for a B♭ instrument
        assert_eq!(t.written_key(Key::parse("Eb major")).fifths, -1);
        let sax = StaffStyle::by_name("E♭ Alto Sax").unwrap();
        assert_eq!(sax.written_key(Key::default()).fifths, 3);
        let p = StaffStyle::by_name("Piano").unwrap();
        assert_eq!((p.staff_of(60), p.staff_of(59)), (0, 1));
        assert_eq!(StaffStyle::auto(&[36, 40], false).name, "Bass");
        // the alto clef's middle line is middle C
        assert_eq!(pitch_at(Clef::Alto, 4, Key::default()), 60);
        assert_eq!(pitch_at(Clef::Tenor, 6, Key::default()), 60);
    }

    #[test]
    fn voices() {
        // a held whole note under moving quarters: two voices, the melody on top
        let notes = [n(0, 72, 0, 960), n(1, 60, 0, 3840), n(2, 74, 960, 960), n(3, 76, 1920, 960)];
        assert_eq!(split_voices(&notes), [0, 1, 0, 0]);
        // plain melody: one voice
        let m: Vec<NoteIn> = (0..4).map(|k| n(k, 60 + k as u8, k as u32 * 960, 960)).collect();
        assert_eq!(split_voices(&m), [0, 0, 0, 0]);
        // a chord: one voice
        assert_eq!(split_voices(&[n(0, 60, 0, 960), n(1, 64, 0, 960)]), [0, 0]);
    }

    #[test]
    fn line_and_page_layout() {
        let l = systems(&[100.0; 10], 350.0);
        assert_eq!(l.iter().map(|x| x.1).collect::<Vec<_>>(), [3, 3, 3, 1]);
        assert!((l[0].2 - 350.0 / 300.0).abs() < 1e-4);
        assert!((l[3].2 - 1.5).abs() < 1e-4);
        // a bar wider than the line still gets a line of its own
        assert_eq!(systems(&[500.0, 50.0], 300.0).len(), 2);
        let pages = paginate(10, 100.0, 1000.0, 50.0, 100.0);
        assert_eq!(pages[0], (0, 150.0));
        assert_eq!(pages.iter().filter(|p| p.0 == 0).count(), 8);
        assert_eq!(pages[8], (1, 50.0));
        // busy bars ask for more room
        let busy = engrave(&(0..16).map(|k| n(k, 67, k as u32 * 240, 240)).collect::<Vec<_>>(), Clef::Treble, &opts(1));
        let empty = engrave(&[], Clef::Treble, &opts(1));
        assert!(bar_width(&busy, 0, 8.0) > bar_width(&empty, 0, 8.0));
    }

    #[test]
    fn max_dots_limits_values() {
        // a dotted half then a quarter rest: with Max. Dots 0 it's a half tied to a quarter
        let n = [NoteIn { pos: 0, len: 2880, pitch: 60, ..NoteIn::default() }];
        let mut o = opts(1);
        let v = engrave(&n, Clef::Treble, &o);
        assert!(v.items.iter().any(|it| it.value == Value { base: 2, dots: 1 }));
        o.max_dots = 0;
        let v = engrave(&n, Clef::Treble, &o);
        assert!(v.items.iter().all(|it| it.value.dots == 0));
        // double dots allowed: a double-dotted half
        let n = [NoteIn { pos: 0, len: 3360, pitch: 60, ..NoteIn::default() }];
        o.max_dots = 2;
        let v = engrave(&n, Clef::Treble, &o);
        assert!(v.items.iter().any(|it| it.value == Value { base: 2, dots: 2 }), "{:?}", v.items.iter().map(|i| i.value).collect::<Vec<_>>());
    }

    #[test]
    fn manual_breaks() {
        let w = [10.0; 8];
        // automatic: 4 bars a line
        assert_eq!(systems_breaks(&w, 40.0, 99, false, &|_| 0).iter().map(|s| s.1).collect::<Vec<_>>(), vec![4, 4]);
        // a line break before bar 2, no line break before bar 6 (that line overflows)
        let lines = systems_breaks(&w, 40.0, 99, false, &|k| match k { 2 => 1, 6 => -1, _ => 0 });
        assert_eq!(lines.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(), vec![(0, 2), (2, 5), (7, 1)]);
        // pages: a page break before line 1 even with room
        let p = paginate_breaks(3, 10.0, 100.0, 0.0, 0.0, 0.0, &|l| (l == 1) as i8);
        assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 1, 1]);
        let p = paginate_breaks(3, 60.0, 100.0, 0.0, 0.0, 0.0, &|l| -((l == 1) as i8));
        assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 0, 1]);
    }
}
