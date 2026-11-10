//! Engraving for Bramble Studio's Score editor (Logic Pro's Score, linear view): which clef,
//! how each pitch is spelled in the key, where notes sit on the staff, and how a region's notes
//! become notated values once quantised for display (Logic's "16,24": sixteenths, or eighth
//! triplets where they fit better), with rests filled in, ties across beats and bar lines,
//! chords on one stem, stems and beams. Pure functions, no drawing: the editor draws the result.
//!
//! Ticks are 960 per quarter note, from bar 1 (tick 0), as everywhere in Studio.

pub const PPQ: u32 = 960;

/// Pitch class letters C D E F G A B.
pub const LETTERS: [char; 7] = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
const LETTER_PC: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clef {
    Treble,
    Bass,
}

impl Clef {
    /// The diatonic step of the staff's bottom line (E4 for treble, G2 for bass; step = MIDI
    /// octave × 7 + letter, so middle C, MIDI 60, is step 35).
    pub fn bottom_line(self) -> i32 {
        match self {
            Clef::Treble => 37,
            Clef::Bass => 25,
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
        let shift = if clef == Clef::Bass { -2 } else { 0 };
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
    pub const CHOICES: [(&'static str, Quant); 10] = [
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
    ];
}

/// A note as the region has it (ticks from bar 1), with its index in the region.
#[derive(Clone, Copy, Debug)]
pub struct NoteIn {
    pub index: usize,
    pub pitch: u8,
    pub pos: u32,
    pub len: u32,
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
    /// The bar (0 = bar 1).
    pub bar: u32,
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

/// A triplet bracket over items `first..=last`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tuplet {
    pub first: usize,
    pub last: usize,
    pub up: bool,
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
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub sig: (u8, u8),
    pub key: Key,
    pub quant: Quant,
    /// The span shown (ticks from bar 1; whole bars).
    pub from: u32,
    pub to: u32,
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

/// The values a span is written in from `p` to `e` within one bar (with ties between them).
fn decompose(mut p: u32, e: u32, bar0: u32, m: Metre, triplets: &[u32], rest: bool) -> Vec<(u32, u32, Value, bool)> {
    let mut out = Vec::new();
    let cands: [(u32, Value); 11] = [
        (3840, Value { base: 1, dots: 0 }),
        (2880, Value { base: 2, dots: 1 }),
        (1920, Value { base: 2, dots: 0 }),
        (1440, Value { base: 4, dots: 1 }),
        (960, Value { base: 4, dots: 0 }),
        (720, Value { base: 8, dots: 1 }),
        (480, Value { base: 8, dots: 0 }),
        (360, Value { base: 16, dots: 1 }),
        (240, Value { base: 16, dots: 0 }),
        (120, Value { base: 32, dots: 0 }),
        (60, Value { base: 64, dots: 0 }),
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
            if p + v > lim || rel + v > m.bar {
                continue;
            }
            // dotted rests only within a beat (a half and a quarter rest, not a dotted half)
            if rest && val.dots > 0 && v > m.beat && !m.compound {
                continue;
            }
            let ok = if v > m.beat {
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
        let (v, val) = chosen.unwrap_or((60.min(lim - p).max(1), Value { base: 64, dots: 0 }));
        out.push((p, v, val, false));
        p += v;
    }
    out
}

/// Engraves one staff's notes.
pub fn engrave(notes: &[NoteIn], clef: Clef, o: &Options) -> Voice {
    let m = Metre::new(o.sig);
    let (q, triplets) = quantise(notes, m, o.quant, o.from);
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
        // No Overlap (Logic's default): a note ends where the next chord starts
        let end = members.iter().map(|x| x.1).max().unwrap_or(on).min(next).min(o.to);
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
    for (s, e, chord) in segs {
        let mut p = s;
        while p < e {
            let bar = (p - o.from) / m.bar;
            let bar0 = o.from + bar * m.bar;
            let bar_end = bar0 + m.bar;
            let end = e.min(bar_end);
            if chord.is_none() && p == bar0 && end == bar_end {
                v.items.push(Item { tick: p, dur: m.bar, value: Value { base: 1, dots: 0 }, heads: vec![], stem_up: true, beam: None, triplet: false, full_bar: true, bar });
                p = end;
                continue;
            }
            let parts = decompose(p, end, bar0, m, &triplets, chord.is_none());
            let last_of_note = end == e;
            let n_parts = parts.len();
            for (k, (tick, dur, value, triplet)) in parts.into_iter().enumerate() {
                let heads = match chord {
                    None => vec![],
                    Some(ci) => {
                        let tie = !(last_of_note && k + 1 == n_parts);
                        let mut hs: Vec<Head> = chords[ci].members.iter().map(|&i| {
                            let n = notes[i];
                            let sp = o.key.spell(n.pitch);
                            Head { note: n.index, pitch: n.pitch, spelled: sp, pos: sp.step - clef.bottom_line(), acc: None, tie, shifted: false }
                        }).collect();
                        hs.sort_by_key(|h| h.pos);
                        hs.dedup_by_key(|h| h.pitch);
                        hs
                    }
                };
                v.items.push(Item { tick, dur, value, heads, stem_up: true, beam: None, triplet, full_bar: false, bar });
            }
            p = end;
        }
    }
    accidentals(&mut v, o.key);
    stems_and_beams(&mut v, m);
    v
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

fn stems_and_beams(v: &mut Voice, m: Metre) {
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
        let beamable = !it.is_rest() && it.value.flags() > 0;
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
    for it in v.items.iter_mut() {
        let ps: Vec<i32> = it.heads.iter().map(|h| h.pos).collect();
        it.stem_up = dir(&ps);
    }
    for g in groups {
        let ps: Vec<i32> = g.iter().flat_map(|&i| v.items[i].heads.iter().map(|h| h.pos)).collect();
        let mean = ps.iter().sum::<i32>() as f32 / ps.len().max(1) as f32;
        let up = mean < 4.0;
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
            v.tuplets.push(Tuplet { first: k, last: j, up });
            k = j + 1;
        } else {
            k += 1;
        }
    }
}

/// Splits a part across a grand staff at middle C.
pub fn split_piano(notes: &[NoteIn]) -> (Vec<NoteIn>, Vec<NoteIn>) {
    notes.iter().partition(|n| n.pitch >= 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(bars: u32) -> Options {
        Options { sig: (4, 4), key: Key::default(), quant: Quant::default(), from: 0, to: bars * 3840 }
    }
    fn n(i: usize, pitch: u8, pos: u32, len: u32) -> NoteIn {
        NoteIn { index: i, pitch, pos, len }
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
}
