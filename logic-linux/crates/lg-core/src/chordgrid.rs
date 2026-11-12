//! Chord grids (Logic: the Chord Grid Library and the score's chord grid symbols): instrument
//! tunings, each with chord grid libraries, each with grids. A grid is a chord's fingering on
//! the strings: per string a fret (muted, open or fretted), the finger, optional dots, barres,
//! the first fret shown and a capo, with its chord data (root, bass, type, difficulty,
//! favourite). Studio ships its own basic libraries (standard shapes, written here) and keeps
//! the user's libraries in a text file; a grid in a score is kept whole in its symbol's text.

/// Note names for the Root / Bass Note pop-ups.
pub const ROOTS: [&str; 12] = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
/// Chord Type pop-up (as written after the root).
pub const TYPES: [&str; 16] = ["", "m", "7", "maj7", "m7", "6", "m6", "sus2", "sus4", "7sus4", "9", "add9", "dim", "dim7", "aug", "m7♭5"];
/// Difficulty pop-up.
pub const DIFFICULTY: [&str; 3] = ["Easy", "Medium", "Hard"];

/// A barre: across strings `from..=to` (indices, low string 0) at `fret`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Barre {
    pub fret: u8,
    pub from: u8,
    pub to: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub name: String,
    pub root: u8,
    /// 255 = no separate bass note.
    pub bass: u8,
    pub kind: u8,
    pub difficulty: u8,
    pub favorite: bool,
    pub capo: u8,
    /// Per string from the lowest: −1 muted, 0 open, n fretted at fret n (absolute).
    pub frets: Vec<i8>,
    /// Finger per string (0 none, 1–4, 5 thumb).
    pub fingers: Vec<u8>,
    /// Optional (white) dots per string, at a fret (0 none).
    pub optional: Vec<u8>,
    pub barres: Vec<Barre>,
}

impl Grid {
    pub fn empty(strings: usize) -> Grid {
        Grid { name: String::new(), root: 0, bass: 255, kind: 0, difficulty: 0, favorite: false, capo: 0, frets: vec![0; strings], fingers: vec![0; strings], optional: vec![0; strings], barres: Vec::new() }
    }
    /// From a shape like "x32010" (x muted, digits or (12) frets), named by root and type.
    pub fn shape(root: u8, kind: u8, s: &str) -> Grid {
        let mut frets = Vec::new();
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                'x' | 'X' => frets.push(-1),
                '(' => {
                    let mut n = String::new();
                    for d in it.by_ref() {
                        if d == ')' {
                            break;
                        }
                        n.push(d);
                    }
                    frets.push(n.parse().unwrap_or(0));
                }
                d => frets.push(d.to_digit(10).map_or(0, |v| v as i8)),
            }
        }
        let n = frets.len();
        let mut g = Grid { frets, fingers: vec![0; n], optional: vec![0; n], ..Grid::empty(n) };
        g.root = root;
        g.kind = kind;
        g.name = g.chord_name();
        g
    }
    /// "C", "Am", "G/B" …
    pub fn chord_name(&self) -> String {
        let mut s = format!("{}{}", ROOTS[self.root as usize % 12], TYPES.get(self.kind as usize).copied().unwrap_or(""));
        if self.bass < 12 && self.bass != self.root {
            s += &format!("/{}", ROOTS[self.bass as usize]);
        }
        s
    }
    /// The lowest fretted fret (the grid starts there when it is above the 4th).
    pub fn first_fret(&self) -> u8 {
        let lo = self.frets.iter().filter(|&&f| f > 0).map(|&f| f as u8).chain(self.barres.iter().map(|b| b.fret)).min().unwrap_or(1);
        let hi = self.frets.iter().map(|&f| f.max(0) as u8).max().unwrap_or(0);
        if hi <= 4 { 1 } else { lo }
    }
    /// The sounding pitches for a tuning (with the capo), low string first; barres fret the
    /// open strings they cover.
    pub fn pitches(&self, tuning: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for (k, &f) in self.frets.iter().enumerate() {
            let Some(&open) = tuning.get(k) else { break };
            let mut fret = f;
            if fret == 0 {
                if let Some(b) = self.barres.iter().find(|b| (b.from as usize..=b.to as usize).contains(&k)) {
                    fret = b.fret as i8;
                }
            }
            if fret >= 0 {
                out.push((open as i32 + fret as i32 + self.capo as i32).clamp(0, 127) as u8);
            }
        }
        out
    }
    /// Clicking a string at a fret (Chord Grid Editor): a dot there, moved if the string had one;
    /// fret 0 (above the string) toggles muted / open.
    pub fn click(&mut self, string: usize, fret: u8) {
        let Some(f) = self.frets.get_mut(string) else { return };
        if fret == 0 {
            *f = if *f == -1 { 0 } else if *f == 0 { -1 } else { 0 };
        } else {
            *f = fret as i8;
        }
    }
    /// Option-click: an optional (white) dot, the real one kept.
    pub fn optional_click(&mut self, string: usize, fret: u8) {
        if let Some(o) = self.optional.get_mut(string) {
            *o = if *o == fret { 0 } else { fret };
        }
        // (on the real dot itself: it becomes the optional one)
        if self.frets.get(string) == Some(&(fret as i8)) {
            self.frets[string] = 0;
        }
    }

    /// The grid as one line (score symbols, library files): fields split by '|'.
    pub fn encode(&self) -> String {
        let frets: Vec<String> = self.frets.iter().map(|f| f.to_string()).collect();
        let fingers: Vec<String> = self.fingers.iter().map(|f| f.to_string()).collect();
        let opt: Vec<String> = self.optional.iter().map(|f| f.to_string()).collect();
        let barres: Vec<String> = self.barres.iter().map(|b| format!("{}:{}:{}", b.fret, b.from, b.to)).collect();
        format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", self.name.replace('|', "/"), self.root, self.bass, self.kind, self.difficulty, self.favorite as u8, self.capo, frets.join(","), fingers.join(","), opt.join(","), barres.join(","))
    }
    pub fn decode(s: &str) -> Option<Grid> {
        let w: Vec<&str> = s.split('|').collect();
        if w.len() < 11 {
            return None;
        }
        let nums = |t: &str| -> Vec<i64> { t.split(',').filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect() };
        let frets: Vec<i8> = nums(w[7]).into_iter().map(|x| x as i8).collect();
        let n = frets.len();
        let mut fingers: Vec<u8> = nums(w[8]).into_iter().map(|x| x as u8).collect();
        let mut optional: Vec<u8> = nums(w[9]).into_iter().map(|x| x as u8).collect();
        fingers.resize(n, 0);
        optional.resize(n, 0);
        let barres = w[10].split(',').filter_map(|b| {
            let v: Vec<u8> = b.split(':').filter_map(|x| x.parse().ok()).collect();
            (v.len() == 3).then(|| Barre { fret: v[0], from: v[1], to: v[2] })
        }).collect();
        Some(Grid { name: w[0].to_string(), root: w[1].parse().ok()?, bass: w[2].parse().unwrap_or(255), kind: w[3].parse().unwrap_or(0), difficulty: w[4].parse().unwrap_or(0), favorite: w[5] == "1", capo: w[6].parse().unwrap_or(0), frets, fingers, optional, barres })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    pub name: String,
    pub grids: Vec<Grid>,
    /// Studio's own (not deletable, not edited in place: edits go to a user library).
    pub builtin: bool,
}

/// An instrument tuning (Instrument Editor row) with its libraries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tuning {
    pub name: String,
    /// Open strings, lowest first.
    pub strings: Vec<u8>,
    pub libraries: Vec<Library>,
}

impl Tuning {
    /// "E A D G B E".
    pub fn tuning_text(&self) -> String {
        const N: [&str; 12] = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
        self.strings.iter().map(|&p| N[(p % 12) as usize]).collect::<Vec<_>>().join(" ")
    }
    pub fn chords(&self) -> usize {
        self.libraries.iter().map(|l| l.grids.len()).sum()
    }
}

/// Studio's own libraries: guitar standard tuning (basic chords and a few barre shapes), two
/// open tunings and a ukulele.
pub fn builtin() -> Vec<Tuning> {
    let g = |r: u8, k: u8, s: &str| Grid::shape(r, k, s);
    let barre = |r: u8, k: u8, s: &str, fret: u8, from: u8, to: u8| {
        let mut x = Grid::shape(r, k, s);
        x.barres.push(Barre { fret, from, to });
        x.difficulty = 1;
        x
    };
    let mut basic = vec![
        g(0, 0, "x32010"), g(2, 0, "xx0232"), g(4, 0, "022100"), g(7, 0, "320003"), g(9, 0, "x02220"),
        g(2, 1, "xx0231"), g(4, 1, "022000"), g(9, 1, "x02210"),
        g(0, 2, "x32310"), g(2, 2, "xx0212"), g(4, 2, "020100"), g(7, 2, "320001"), g(9, 2, "x02020"), g(11, 2, "x21202"),
        g(0, 3, "x32000"), g(5, 3, "xx3210"), g(2, 3, "xx0222"),
        g(9, 4, "x02010"), g(4, 4, "022030"), g(2, 4, "xx0211"),
        g(2, 8, "xx0233"), g(9, 8, "x02230"), g(2, 7, "xx0230"), g(9, 7, "x02200"),
    ];
    // (fingers on the basic shapes' fretted strings: the common ones)
    for gr in basic.iter_mut() {
        let mut next = 1u8;
        for k in 0..gr.frets.len() {
            if gr.frets[k] > 0 {
                gr.fingers[k] = (gr.frets[k] as u8).min(4).max(next.min(4));
                next = gr.fingers[k] + 1;
            }
        }
    }
    let barres = vec![
        barre(5, 0, "133211", 1, 0, 5), barre(5, 1, "133111", 1, 0, 5), barre(11, 1, "x24432", 2, 1, 5), barre(10, 0, "x13331", 1, 1, 5), barre(6, 1, "244222", 2, 0, 5),
    ];
    let open_d = vec![g(2, 0, "000000"), g(7, 0, "555555"), g(9, 0, "777777")];
    let dadgad = vec![g(2, 8, "000000"), g(7, 0, "x00200"), g(9, 8, "x22200")];
    let uke = vec![g(0, 0, "0003"), g(5, 0, "2010"), g(7, 0, "0232"), g(9, 1, "2000"), g(4, 1, "0432")];
    let lib = |n: &str, v: Vec<Grid>| Library { name: n.into(), grids: v, builtin: true };
    vec![
        Tuning { name: "Guitar".into(), strings: vec![40, 45, 50, 55, 59, 64], libraries: vec![lib("Basic Chords", basic), lib("Barre Chords", barres)] },
        Tuning { name: "Guitar Open D".into(), strings: vec![38, 45, 50, 54, 57, 62], libraries: vec![lib("Open D Chords", open_d)] },
        Tuning { name: "Guitar DADGAD".into(), strings: vec![38, 45, 50, 55, 57, 62], libraries: vec![lib("DADGAD Chords", dadgad)] },
        Tuning { name: "Ukulele".into(), strings: vec![67, 60, 64, 69], libraries: vec![lib("Ukulele Chords", uke)] },
    ]
}

/// The user's tunings and libraries as text (one file): `tuning\tname\tpitches`, `library\tname`,
/// `grid\t<encoded>` lines; built-in libraries are left out.
pub fn to_text(t: &[Tuning]) -> String {
    let mut out = String::new();
    for tu in t {
        let user: Vec<&Library> = tu.libraries.iter().filter(|l| !l.builtin).collect();
        if user.is_empty() {
            continue;
        }
        let p: Vec<String> = tu.strings.iter().map(|x| x.to_string()).collect();
        out += &format!("tuning\t{}\t{}\n", tu.name, p.join(","));
        for l in user {
            out += &format!("library\t{}\n", l.name);
            for g in &l.grids {
                out += &format!("grid\t{}\n", g.encode());
            }
        }
    }
    out
}

/// Merges user libraries read from text into the tunings (a tuning by name, or a new one).
pub fn merge_text(into: &mut Vec<Tuning>, text: &str) {
    let mut cur_t: Option<usize> = None;
    for l in text.lines() {
        let w: Vec<&str> = l.splitn(3, '\t').collect();
        match w.first().copied() {
            Some("tuning") if w.len() >= 3 => {
                let strings: Vec<u8> = w[2].split(',').filter_map(|x| x.parse().ok()).collect();
                cur_t = Some(into.iter().position(|t| t.name == w[1]).unwrap_or_else(|| {
                    into.push(Tuning { name: w[1].to_string(), strings, libraries: Vec::new() });
                    into.len() - 1
                }));
            }
            Some("library") if w.len() >= 2 => {
                if let Some(t) = cur_t.and_then(|k| into.get_mut(k)) {
                    t.libraries.retain(|x| !(x.name == w[1] && !x.builtin));
                    t.libraries.push(Library { name: w[1].to_string(), grids: Vec::new(), builtin: false });
                }
            }
            Some("grid") if w.len() >= 2 => {
                if let (Some(t), Some(g)) = (cur_t.and_then(|k| into.get_mut(k)), Grid::decode(w[1])) {
                    if let Some(lib) = t.libraries.last_mut() {
                        lib.grids.push(g);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_names_and_pitches() {
        let c = Grid::shape(0, 0, "x32010");
        assert_eq!(c.name, "C");
        assert_eq!(c.pitches(&[40, 45, 50, 55, 59, 64]), vec![48, 52, 55, 60, 64]);
        let mut f = Grid::shape(5, 0, "133211");
        f.barres.push(Barre { fret: 1, from: 0, to: 5 });
        assert_eq!(f.first_fret(), 1);
        let hi = Grid::shape(9, 0, "(12)(12)(14)(14)(14)(12)");
        assert_eq!(hi.first_fret(), 12);
        let mut capo = c.clone();
        capo.capo = 2;
        assert_eq!(capo.pitches(&[40, 45, 50, 55, 59, 64])[0], 50);
        let mut g = Grid { bass: 11, ..Grid::shape(7, 0, "x20003") };
        assert_eq!(g.chord_name(), "G/B");
        g.click(0, 0);
        assert_eq!(g.frets[0], 0, "muted string clicked above: open");
        g.click(0, 0);
        assert_eq!(g.frets[0], -1);
        g.click(1, 3);
        assert_eq!(g.frets[1], 3, "a dot moves on its string");
        g.optional_click(1, 3);
        assert_eq!((g.frets[1], g.optional[1]), (0, 3));
    }

    #[test]
    fn libraries_round_trip_as_text() {
        let mut t = builtin();
        let mut g = Grid::shape(2, 1, "xx0231");
        g.favorite = true;
        g.barres.push(Barre { fret: 2, from: 3, to: 5 });
        g.fingers[3] = 2;
        t[0].libraries.push(Library { name: "Mine".into(), grids: vec![g.clone()], builtin: false });
        t.push(Tuning { name: "Banjo".into(), strings: vec![67, 50, 55, 59, 62], libraries: vec![Library { name: "B".into(), grids: vec![], builtin: false }] });
        let text = to_text(&t);
        let mut back = builtin();
        merge_text(&mut back, &text);
        assert_eq!(back[0].libraries.last().unwrap().grids, vec![g.clone()]);
        assert_eq!(back.last().unwrap().name, "Banjo");
        assert_eq!(Grid::decode(&g.encode()), Some(g));
        assert_eq!(t[0].tuning_text(), "E A D G B E");
    }
}
