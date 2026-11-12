//! What the Score editor adds to a project beyond the notes (Logic keeps these in the region's
//! score data): per-note attributes (stems, ties, beams, accidentals, enharmonic spelling,
//! interpretation, articulations, fingering), the staff style of each track, and the symbols
//! placed on the staff (dynamics, text, lyrics, chord symbols, slurs, hairpins, clef and key
//! changes). Kept in the Project so Undo covers every Score edit; Studio saves it beside the
//! project (lg-app scorefile.rs).
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
    /// Note head (index into `NOTE_HEADS`), 0 = the normal head.
    pub head: u8,
}

/// The Part box's note heads (visual only). "Gray" shows grey on screen and is left out of print.
pub const NOTE_HEADS: [&str; 12] = [
    "Normal", "Cross", "Circled Cross", "Diamond", "Open Diamond", "Slash", "Triangle", "Square", "Open Square", "Harmonic", "Parenthesized", "Gray (no print)",
];
/// Index of the gray (hidden in print) head.
pub const HEAD_GRAY: u8 = 11;

/// Logic's attachable symbols, in its key-command order ("Attach Symbol: …").
pub const ARTICULATIONS: [&str; 22] = [
    "Fermata Up", "Fermata Down", "Staccato", "Staccatissimo", "Accent", "Strong Accent", "Long Accent", "Tenuto", "Up-Bow", "Down-Bow", "Flageolet", "Pizzicato", "Left Hand", "Right Hand", "Soft", "Heavy", "Jazz 1", "Jazz 2", "Jazz 3", "Jazz 4", "Jazz 5", "Jazz 6",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SymbolKind {
    Dynamic,
    #[default]
    Text,
    Lyric,
    Chord,
    Crescendo,
    Decrescendo,
    SlurUp,
    SlurDown,
    /// A clef change: `text` holds the clef's name (Treble, Bass, Alto, Tenor, Percussion).
    Clef,
    /// A trill, ornament, arpeggio, tremolo or trill accidental: `text` names it (`ORNAMENTS`).
    Ornament,
    /// Automatic text: `text` is REGION, INSTR, SONG or DATE.
    AutoText,
}

/// The Part box's Trills group (free symbols, not attached to notes, visual only).
pub const ORNAMENTS: [&str; 13] = [
    "Trill", "Trill Line", "Mordent", "Inverted Mordent", "Turn", "Inverted Turn", "Arpeggio", "Tremolo 1", "Tremolo 2", "Tremolo 3", "Sharp", "Flat", "Natural",
];
/// The automatic text objects.
pub const AUTO_TEXT: [&str; 4] = ["REGION", "INSTR", "SONG", "DATE"];

/// Text alignment of a text object (Event inspector ▸ Align).
pub const ALIGNS: [&str; 4] = ["Left", "Center", "Right", "Justified"];

/// A symbol on a track's staff at a position (ticks from bar 1), with its length (hairpins,
/// slurs) and text (dynamics, words, lyrics, chord names, clef names).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Symbol {
    pub tick: u32,
    pub kind: SymbolKind,
    pub text: String,
    pub len: u32,
    /// Which staff of a grand staff (0 the upper).
    pub staff: u8,
    /// Text objects: their text style ("" = the kind's default style), alignment (`ALIGNS`)
    /// and graphic offsets in tenths of a staff space (Event inspector Horizontal / Vertical Pos).
    pub style: String,
    pub align: u8,
    pub dx: i16,
    pub dy: i16,
}

/// Bar lines, repeats, endings, segno and coda (global: every staff shows them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum BarKind {
    #[default]
    Single,
    Double,
    Final,
    Dashed,
    RepeatStart,
    RepeatEnd,
    RepeatBoth,
    /// One- and two-bar repeat signs (they hide the bar's notes and rests; playback unchanged).
    Repeat1,
    Repeat2,
    /// First and second endings (`text` "1." / "2.", `len` the bracket's length).
    Ending1,
    Ending2,
    Segno,
    Coda,
}

impl BarKind {
    pub const ALL: [BarKind; 13] = [
        BarKind::Single, BarKind::Double, BarKind::Final, BarKind::Dashed, BarKind::RepeatStart, BarKind::RepeatEnd, BarKind::RepeatBoth, BarKind::Repeat1, BarKind::Repeat2, BarKind::Ending1, BarKind::Ending2, BarKind::Segno, BarKind::Coda,
    ];
    pub fn name(self) -> &'static str {
        match self {
            BarKind::Single => "Bar Line",
            BarKind::Double => "Double Bar Line",
            BarKind::Final => "Final Bar Line",
            BarKind::Dashed => "Dashed Bar Line",
            BarKind::RepeatStart => "Repeat Start",
            BarKind::RepeatEnd => "Repeat End",
            BarKind::RepeatBoth => "Double Repeat",
            BarKind::Repeat1 => "One-Bar Repeat",
            BarKind::Repeat2 => "Two-Bar Repeat",
            BarKind::Ending1 => "First Ending",
            BarKind::Ending2 => "Second Ending",
            BarKind::Segno => "Segno",
            BarKind::Coda => "Coda",
        }
    }
    pub fn from_name(s: &str) -> Option<BarKind> {
        BarKind::ALL.iter().copied().find(|k| k.name() == s)
    }
    /// Replaces the regular bar line where it stands (endings, segno and coda don't).
    pub fn is_line(self) -> bool {
        matches!(self, BarKind::Single | BarKind::Double | BarKind::Final | BarKind::Dashed | BarKind::RepeatStart | BarKind::RepeatEnd | BarKind::RepeatBoth)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarSym {
    pub tick: u32,
    pub kind: BarKind,
    pub text: String,
    pub len: u32,
}

/// Page and line break symbols (at a bar: the break comes before it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    Line,
    Page,
    NoLine,
    NoPage,
}

impl Break {
    pub const ALL: [Break; 4] = [Break::Line, Break::Page, Break::NoLine, Break::NoPage];
    pub fn name(self) -> &'static str {
        match self {
            Break::Line => "Line Break",
            Break::Page => "Page Break",
            Break::NoLine => "No Line Break",
            Break::NoPage => "No Page Break",
        }
    }
    pub fn from_name(s: &str) -> Option<Break> {
        Break::ALL.iter().copied().find(|k| k.name() == s)
    }
}

/// A text style: font family ("" = the app's text font), size in points, bold, italic.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub name: String,
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Logic's default text styles (name, size, bold, italic), always first in the list; each is
/// assigned automatically to its text element.
pub const DEFAULT_STYLES: [(&str, f32, bool, bool); 18] = [
    ("Plain Text", 12.0, false, false),
    ("Page Numbers", 12.0, false, false),
    ("Bar Numbers", 10.0, false, true),
    ("Instrument Names", 12.0, false, false),
    ("Tuplets", 10.0, false, true),
    ("Repeat Endings", 12.0, false, false),
    ("Chord Root", 14.0, true, false),
    ("Chord Extension", 10.0, true, false),
    ("Multiple Rests", 12.0, true, false),
    ("Tablature", 9.0, false, false),
    ("Tempo Symbols", 12.0, true, false),
    ("Octave Symbols", 10.0, false, true),
    ("Note Heads", 9.0, false, false),
    ("Guitar Grid Fingerings", 8.0, false, false),
    ("Guitar Markings", 10.0, false, false),
    ("Fingerings", 9.0, false, false),
    ("Lyric", 12.0, false, false),
    ("Title", 24.0, true, false),
];

impl TextStyle {
    pub fn default_named(name: &str) -> Option<TextStyle> {
        DEFAULT_STYLES.iter().find(|d| d.0 == name).map(|d| TextStyle { name: d.0.into(), family: String::new(), size: d.1, bold: d.2, italic: d.3, underline: false })
    }
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
    /// Max. Dots: 0 = the default (1), otherwise the number of dots + 1.
    pub max_dots: u8,
    pub notes: BTreeMap<(u32, u8), NoteAttr>,
    pub symbols: Vec<Symbol>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Score {
    pub tracks: BTreeMap<u64, TrackScore>,
    /// Key changes inserted from the Part box: (ticks from bar 1, fifths), written in the score
    /// (the project's key stays the first).
    pub keys: Vec<(u32, i8)>,
    /// Bar lines, repeats, endings, segno and coda (global).
    pub bars: Vec<BarSym>,
    /// Manual page and line breaks: (bar index from bar 1, kind).
    pub breaks: BTreeMap<u32, Break>,
    /// Text styles: the defaults' edits (same name) and the user's own, in order.
    pub text_styles: Vec<TextStyle>,
    /// Score sets (besides All Instruments, which isn't stored).
    pub sets: Vec<ScoreSet>,
}

/// An instrument of a score set: its track, names, and whether a bracket and the bar lines join
/// it with the next instrument.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetEntry {
    pub track: u64,
    /// Full and short names ("" = the track's name, its short form).
    pub full: String,
    pub short: String,
    pub bracket: bool,
    pub barline: bool,
}

/// A score set: instruments in score order, the Scale (25–250 %), and Part layout (else Score).
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreSet {
    pub name: String,
    pub entries: Vec<SetEntry>,
    pub scale: u16,
    pub part_layout: bool,
}

impl ScoreSet {
    pub fn new(name: &str, tracks: &[u64]) -> ScoreSet {
        ScoreSet { name: name.to_string(), entries: tracks.iter().map(|&t| SetEntry { track: t, ..Default::default() }).collect(), scale: 100, part_layout: false }
    }
    pub fn entry(&self, track: u64) -> Option<&SetEntry> {
        self.entries.iter().find(|e| e.track == track)
    }
}

impl Score {
    /// Every text style: the defaults (as edited), then the user's.
    pub fn styles(&self) -> Vec<TextStyle> {
        let mut out: Vec<TextStyle> = DEFAULT_STYLES.iter().map(|d| self.text_styles.iter().find(|s| s.name == d.0).cloned().unwrap_or_else(|| TextStyle::default_named(d.0).unwrap_or(TextStyle { name: d.0.into(), family: String::new(), size: 12.0, bold: false, italic: false, underline: false }))).collect();
        out.extend(self.text_styles.iter().filter(|s| TextStyle::default_named(&s.name).is_none()).cloned());
        out
    }
    /// The style a text symbol is drawn with: its own, else the default for its kind.
    pub fn style_for(&self, s: &Symbol) -> TextStyle {
        let all = self.styles();
        let dflt = match s.kind {
            SymbolKind::Lyric => "Lyric",
            SymbolKind::Chord => "Chord Root",
            _ => "Plain Text",
        };
        let want = if s.style.is_empty() { dflt } else { s.style.as_str() };
        all.iter().find(|x| x.name == want).or_else(|| all.iter().find(|x| x.name == dflt)).cloned().unwrap_or_else(|| all[0].clone())
    }
    /// Sets (adds or replaces) a style by name.
    pub fn set_style(&mut self, st: TextStyle) {
        if let Some(x) = self.text_styles.iter_mut().find(|x| x.name == st.name) {
            *x = st;
        } else {
            self.text_styles.push(st);
        }
    }
    /// The global bar symbols at a tick of a kind group (lines or other).
    pub fn bar_at(&self, tick: u32) -> Option<&BarSym> {
        self.bars.iter().find(|b| b.tick == tick && b.kind.is_line())
    }
    /// Adds a bar symbol (a bar line replaces another bar line at the same place).
    pub fn add_bar(&mut self, b: BarSym) {
        if b.kind.is_line() {
            self.bars.retain(|x| !(x.tick == b.tick && x.kind.is_line()));
        } else {
            self.bars.retain(|x| !(x.tick == b.tick && x.kind == b.kind));
        }
        self.bars.push(b);
        self.bars.sort_by_key(|x| (x.tick, x.kind));
    }
    /// Bars (from bar 1) hidden by one- and two-bar repeat signs.
    pub fn repeated_bars(&self, bar_t: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for b in &self.bars {
            let bar = b.tick / bar_t.max(1);
            match b.kind {
                BarKind::Repeat1 => out.push(bar),
                BarKind::Repeat2 => out.extend([bar, bar + 1]),
                _ => {}
            }
        }
        out
    }
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

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

const KINDS: [(SymbolKind, &str); 12] = [
    (SymbolKind::Dynamic, "dyn"), (SymbolKind::Text, "text"), (SymbolKind::Lyric, "lyric"), (SymbolKind::Chord, "chord"), (SymbolKind::Crescendo, "cresc"), (SymbolKind::Decrescendo, "decresc"), (SymbolKind::SlurUp, "slurup"), (SymbolKind::SlurDown, "slurdown"), (SymbolKind::Clef, "clef"), (SymbolKind::Ornament, "orn"), (SymbolKind::AutoText, "auto"), (SymbolKind::Text, "text"),
];

impl Score {
    /// The score as lines of text (Studio keeps it beside the project): tracks by their index.
    pub fn to_text(&self, index_of: impl Fn(u64) -> Option<usize>) -> String {
        let mut out = String::new();
        for (id, t) in &self.tracks {
            let Some(i) = index_of(*id) else { continue };
            out += &format!("strack\t{i}\t{}\t{}\t{}\t{}\t{}\n", esc(&t.style), esc(&t.quantize), t.no_interpretation as u8, t.syncopation as u8, t.max_dots);
            for (&(pos, pitch), a) in &t.notes {
                out += &format!(
                    "sattr\t{i}\t{pos}\t{pitch}\t{} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
                    a.stem, a.stem_pos, a.stem_end, a.tie, a.beam, a.accidental, a.spell, a.interp, a.synco, a.independent, a.staff, a.artic, a.finger, a.pick, a.color, a.slur, a.head
                );
            }
            for s in &t.symbols {
                let k = KINDS.iter().find(|k| k.0 == s.kind).map_or("text", |k| k.1);
                out += &format!("ssym\t{i}\t{}\t{k}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", s.tick, s.len, s.staff, esc(&s.text), esc(&s.style), s.align, s.dx, s.dy);
            }
        }
        for k in &self.keys {
            out += &format!("skey\t{}\t{}\n", k.0, k.1);
        }
        for b in &self.bars {
            out += &format!("sbar\t{}\t{}\t{}\t{}\n", b.tick, b.kind.name(), b.len, esc(&b.text));
        }
        for (bar, k) in &self.breaks {
            out += &format!("sbreak\t{bar}\t{}\n", k.name());
        }
        for (k, set) in self.sets.iter().enumerate() {
            out += &format!("sset\t{k}\t{}\t{}\t{}\n", esc(&set.name), set.scale, set.part_layout as u8);
            for e in &set.entries {
                let Some(i) = index_of(e.track) else { continue };
                out += &format!("sentry\t{k}\t{i}\t{}\t{}\t{}\t{}\n", esc(&e.full), esc(&e.short), e.bracket as u8, e.barline as u8);
            }
        }
        for t in &self.text_styles {
            out += &format!("sstyle\t{}\t{}\t{}\t{}\t{}\t{}\n", esc(&t.name), esc(&t.family), t.size, t.bold as u8, t.italic as u8, t.underline as u8);
        }
        out
    }

    /// Reads `to_text`'s lines (others are skipped); true when any were score lines.
    pub fn apply_text(&mut self, text: &str, id_of: impl Fn(usize) -> Option<u64>) -> bool {
        let mut any = false;
        for l in text.lines() {
            let w: Vec<&str> = l.split('\t').collect();
            let num = |k: usize| w.get(k).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
            let track = |k: usize| w.get(k).and_then(|x| x.parse::<usize>().ok()).and_then(&id_of);
            match w.first().copied() {
                Some("strack") if w.len() >= 6 => {
                    if let Some(id) = track(1) {
                        let t = self.track_mut(id);
                        t.style = unesc(w[2]);
                        t.quantize = unesc(w[3]);
                        t.no_interpretation = w[4] == "1";
                        t.syncopation = w[5] == "1";
                        t.max_dots = num(6) as u8;
                        any = true;
                    }
                }
                Some("sattr") if w.len() >= 5 => {
                    if let Some(id) = track(1) {
                        let v: Vec<i64> = w[4].split(' ').map(|x| x.parse().unwrap_or(0)).collect();
                        let g = |k: usize| v.get(k).copied().unwrap_or(0);
                        let a = NoteAttr {
                            stem: g(0) as i8, stem_pos: g(1) as u8, stem_end: g(2) as i8, tie: g(3) as i8, beam: g(4) as i8, accidental: g(5) as u8, spell: g(6) as i8, interp: g(7) as i8, synco: g(8) as i8,
                            independent: g(9) as u8, staff: g(10) as i8, artic: g(11) as u32, finger: g(12) as u8, pick: g(13) as u8, color: g(14) as u8, slur: g(15) as i8, head: g(16) as u8,
                        };
                        self.track_mut(id).notes.insert((num(2) as u32, num(3) as u8), a);
                        any = true;
                    }
                }
                Some("ssym") if w.len() >= 11 => {
                    if let Some(id) = track(1) {
                        let kind = KINDS.iter().find(|k| k.1 == w[3]).map_or(SymbolKind::Text, |k| k.0);
                        let s = Symbol { tick: num(2) as u32, kind, len: num(4) as u32, staff: num(5) as u8, text: unesc(w[6]), style: unesc(w[7]), align: num(8) as u8, dx: num(9) as i16, dy: num(10) as i16 };
                        self.track_mut(id).symbols.push(s);
                        any = true;
                    }
                }
                Some("skey") if w.len() >= 3 => {
                    self.keys.push((num(1) as u32, num(2) as i8));
                    any = true;
                }
                Some("sbar") if w.len() >= 5 => {
                    if let Some(kind) = BarKind::from_name(w[2]) {
                        self.bars.push(BarSym { tick: num(1) as u32, kind, len: num(3) as u32, text: unesc(w[4]) });
                        any = true;
                    }
                }
                Some("sbreak") if w.len() >= 3 => {
                    if let Some(k) = Break::from_name(w[2]) {
                        self.breaks.insert(num(1) as u32, k);
                        any = true;
                    }
                }
                Some("sset") if w.len() >= 5 => {
                    self.sets.push(ScoreSet { name: unesc(w[2]), entries: Vec::new(), scale: w[3].parse().unwrap_or(100), part_layout: w[4] == "1" });
                    any = true;
                }
                Some("sentry") if w.len() >= 7 => {
                    let k = num(1) as usize;
                    if let (Some(id), Some(set)) = (track(2), self.sets.get_mut(k)) {
                        set.entries.push(SetEntry { track: id, full: unesc(w[3]), short: unesc(w[4]), bracket: w[5] == "1", barline: w[6] == "1" });
                    }
                }
                Some("sstyle") if w.len() >= 7 => {
                    self.text_styles.push(TextStyle { name: unesc(w[1]), family: unesc(w[2]), size: w[3].parse().unwrap_or(12.0), bold: w[4] == "1", italic: w[5] == "1", underline: w[6] == "1" });
                    any = true;
                }
                _ => {}
            }
        }
        any
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

    #[test]
    fn score_text_round_trip() {
        let mut s = Score::default();
        s.edit(5, 960, 60, |a| {
            a.head = 3;
            a.artic = 5;
            a.stem = -1;
        });
        s.track_mut(5).style = "Piano".into();
        s.track_mut(5).max_dots = 3;
        s.track_mut(5).symbols.push(Symbol { tick: 10, kind: SymbolKind::AutoText, text: "SONG\ttab".into(), style: "Title".into(), dx: -4, dy: 7, align: 1, ..Default::default() });
        s.add_bar(BarSym { tick: 3840, kind: BarKind::RepeatEnd, ..Default::default() });
        s.add_bar(BarSym { tick: 3840, kind: BarKind::Final, ..Default::default() });
        s.add_bar(BarSym { tick: 0, kind: BarKind::Ending1, text: "1.".into(), len: 3840 });
        s.breaks.insert(4, Break::Page);
        s.set_style(TextStyle { name: "Mine".into(), family: "Serif".into(), size: 17.5, bold: true, italic: false, underline: true });
        s.keys = vec![(1920, -3)];
        let mut set = ScoreSet::new("Strings", &[5]);
        set.entries[0].full = "Violin".into();
        set.entries[0].bracket = true;
        set.scale = 80;
        s.sets.push(set);
        let text = s.to_text(|id| (id == 5).then_some(2));
        let mut back = Score::default();
        assert!(back.apply_text(&text, |i| (i == 2).then_some(5)));
        assert_eq!(back, s);
        assert_eq!(s.bars.iter().filter(|b| b.tick == 3840).count(), 1, "a bar line replaces another");
        assert_eq!(s.styles().last().unwrap().name, "Mine");
        let sym = &s.tracks[&5].symbols[0];
        assert_eq!(s.style_for(sym).size, 24.0);
        assert_eq!(s.style_for(&Symbol { kind: SymbolKind::Lyric, ..Default::default() }).name, "Lyric");
    }

    #[test]
    fn bar_repeats_hide_bars() {
        let mut s = Score::default();
        s.add_bar(BarSym { tick: 3840 * 2, kind: BarKind::Repeat2, ..Default::default() });
        s.add_bar(BarSym { tick: 0, kind: BarKind::Repeat1, ..Default::default() });
        assert_eq!(s.repeated_bars(3840), vec![0, 2, 3]);
    }
}
