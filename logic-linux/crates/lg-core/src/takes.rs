//! Take folders (Logic's cycle and overlapping recordings): a range on a track holding several
//! takes of the same passage, one of them (or a comp of sections from several) playing.
//!
//! The folder plays its active take or comp: the app turns `pieces()` into the regions on the
//! track (audio: each piece a region on its take's file, offset into it; MIDI: the take's notes
//! in the piece's range). Comps are lists of sections in time order, each from one take; Quick
//! Swipe Comping (`swipe`) puts a take's range into the active comp, as dragging over a take
//! lane does in Logic.

use crate::project::Region;

/// One part of a comp: [start, end) in song ticks, from take `take`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    pub start: u32,
    pub end: u32,
    pub take: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comp {
    pub name: String,
    pub sections: Vec<Section>,
}

/// What the folder plays: one whole take, or a comp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Active {
    Take(usize),
    Comp(usize),
}

#[derive(Clone, Debug)]
pub struct TakeFolder {
    /// The track (index in the project) the folder is on.
    pub track: usize,
    pub name: String,
    pub start: u32,
    pub length: u32,
    /// The takes in recording order ("Take 1", "Take 2"…), each a whole region.
    pub takes: Vec<Region>,
    pub comps: Vec<Comp>,
    pub active: Active,
    /// Opened (the take lanes shown under the folder).
    pub expanded: bool,
}

impl TakeFolder {
    /// A folder made of its first take.
    pub fn new(track: usize, name: &str, mut first: Region) -> TakeFolder {
        first.name = "Take 1".into();
        TakeFolder { track, name: name.to_string(), start: first.start, length: first.length, takes: vec![first], comps: Vec::new(), active: Active::Take(0), expanded: false }
    }

    pub fn end(&self) -> u32 {
        self.start + self.length
    }

    /// MIDI takes (no audio file).
    pub fn is_midi(&self) -> bool {
        self.takes.iter().all(|t| t.audio.is_none())
    }

    pub fn overlaps(&self, start: u32, end: u32) -> bool {
        start < self.end() && end > self.start
    }

    /// A new take (the latest recording): the folder grows to hold it and plays it, as Logic.
    /// A take shorter than the folder (a punch) plays over what played before: a new comp of
    /// the two.
    pub fn add_take(&mut self, mut r: Region) -> usize {
        let before = self.pieces();
        let (a, b) = (r.start, r.start + r.length);
        let end = self.end().max(b);
        self.start = self.start.min(a);
        self.length = end - self.start;
        r.name = format!("Take {}", self.next_take_number());
        self.takes.push(r);
        let k = self.takes.len() - 1;
        let covered = before.iter().all(|s| s.start >= a && s.end <= b);
        if covered {
            self.active = Active::Take(k);
        } else {
            let sections = put(&before, Section { start: a, end: b, take: k });
            let name = self.next_comp_name();
            self.comps.push(Comp { name, sections });
            self.active = Active::Comp(self.comps.len() - 1);
        }
        k
    }

    fn next_take_number(&self) -> usize {
        self.takes.iter().filter_map(|t| t.name.strip_prefix("Take ")?.parse::<usize>().ok()).max().unwrap_or(0) + 1
    }

    /// The next comp name: Comp A, Comp B…
    fn next_comp_name(&self) -> String {
        for i in 0..26u8 {
            let n = format!("Comp {}", (b'A' + i) as char);
            if !self.comps.iter().any(|c| c.name == n) {
                return n;
            }
        }
        format!("Comp {}", self.comps.len() + 1)
    }

    /// The name of what plays (a take's or a comp's).
    pub fn active_name(&self) -> String {
        match self.active {
            Active::Take(k) => self.takes.get(k).map_or(String::new(), |t| t.name.clone()),
            Active::Comp(c) => self.comps.get(c).map_or(String::new(), |c| c.name.clone()),
        }
    }

    /// The sections that play now (a whole take is one section over its own range).
    pub fn active_sections(&self) -> Vec<Section> {
        match self.active {
            Active::Take(k) => self.takes.get(k).map(|t| vec![Section { start: t.start, end: t.start + t.length, take: k }]).unwrap_or_default(),
            Active::Comp(c) => self.comps.get(c).map(|c| c.sections.clone()).unwrap_or_default(),
        }
    }

    /// What plays, as pieces of takes (section order), clipped to the folder and the takes.
    pub fn pieces(&self) -> Vec<Section> {
        self.active_sections()
            .into_iter()
            .filter_map(|s| {
                let t = self.takes.get(s.take)?;
                let a = s.start.max(t.start).max(self.start);
                let b = s.end.min(t.start + t.length).min(self.end());
                (b > a).then_some(Section { start: a, end: b, take: s.take })
            })
            .collect()
    }

    /// Quick Swipe Comping: take `take` plays over [from, to). From a whole take a new comp
    /// begins (Logic: "Comp A"), made of that take, then the swipe goes into it.
    pub fn swipe(&mut self, take: usize, from: u32, to: u32) {
        let Some(t) = self.takes.get(take) else { return };
        let (a, b) = (from.min(to).max(t.start), from.max(to).min(t.start + t.length));
        if b <= a {
            return;
        }
        let c = match self.active {
            Active::Comp(c) if c < self.comps.len() => c,
            _ => {
                let sections = self.active_sections();
                let name = self.next_comp_name();
                self.comps.push(Comp { name, sections });
                self.comps.len() - 1
            }
        };
        self.active = Active::Comp(c);
        let secs = &mut self.comps[c].sections;
        *secs = put(secs, Section { start: a, end: b, take });
    }

    /// Clicking a take: the whole take plays (Logic: the take becomes active).
    pub fn select(&mut self, a: Active) {
        let ok = match a {
            Active::Take(k) => k < self.takes.len(),
            Active::Comp(c) => c < self.comps.len(),
        };
        if ok {
            self.active = a;
        }
    }

    /// Create New Comp: a copy of what plays, as a new comp, playing.
    pub fn new_comp(&mut self) -> usize {
        let sections = self.active_sections();
        let name = self.next_comp_name();
        self.comps.push(Comp { name, sections });
        self.active = Active::Comp(self.comps.len() - 1);
        self.comps.len() - 1
    }

    /// Takes then comps, the order Select Next / Previous Take or Comp goes through.
    fn order(&self) -> Vec<Active> {
        (0..self.takes.len()).map(Active::Take).chain((0..self.comps.len()).map(Active::Comp)).collect()
    }

    pub fn step(&mut self, forward: bool) {
        let o = self.order();
        if o.is_empty() {
            return;
        }
        let i = o.iter().position(|x| *x == self.active).unwrap_or(0);
        let n = o.len();
        self.active = o[if forward { (i + 1) % n } else { (i + n - 1) % n }];
    }

    pub fn rename_active(&mut self, name: &str) {
        match self.active {
            Active::Take(k) => {
                if let Some(t) = self.takes.get_mut(k) {
                    t.name = name.to_string();
                }
            }
            Active::Comp(c) => {
                if let Some(c) = self.comps.get_mut(c) {
                    c.name = name.to_string();
                }
            }
        }
    }

    /// Delete Take or Comp: what plays goes. A deleted take leaves every comp (its sections
    /// gone, later takes renumbered in them). False when no take is left (the folder goes).
    pub fn delete_active(&mut self) -> bool {
        match self.active {
            Active::Take(k) if k < self.takes.len() => {
                self.takes.remove(k);
                for c in &mut self.comps {
                    c.sections.retain(|s| s.take != k);
                    for s in &mut c.sections {
                        if s.take > k {
                            s.take -= 1;
                        }
                    }
                }
                self.comps.retain(|c| !c.sections.is_empty());
                if self.takes.is_empty() {
                    return false;
                }
                self.active = Active::Take(k.min(self.takes.len() - 1));
            }
            Active::Comp(c) if c < self.comps.len() => {
                self.comps.remove(c);
                self.active = if c > 0 { Active::Comp(c - 1) } else { Active::Take(self.takes.len().saturating_sub(1)) };
            }
            _ => {}
        }
        true
    }

    /// Duplicate Active MIDI Take: the take that plays (for a comp, its first section's) copied
    /// as the newest take.
    pub fn duplicate_take(&mut self) -> Option<usize> {
        let k = self.active_sections().first()?.take;
        let t = self.takes.get(k)?.clone();
        Some(self.add_take(t))
    }

    /// Create New Empty MIDI Take: a take over the whole folder without notes.
    pub fn empty_take(&mut self) -> usize {
        let mut r = self.takes[0].clone();
        r.start = self.start;
        r.length = self.length;
        r.notes.clear();
        r.audio = None;
        self.add_take(r)
    }

    /// Trim to Active Comp Sections: the folder shrinks to what plays.
    pub fn trim_to_active(&mut self) {
        let p = self.pieces();
        if let (Some(a), Some(b)) = (p.iter().map(|s| s.start).min(), p.iter().map(|s| s.end).max()) {
            self.start = a;
            self.length = b - a;
        }
    }

    /// Moves the folder and everything in it by `d` ticks.
    pub fn shift(&mut self, d: i64) {
        let mv = |x: u32| (x as i64 + d).max(0) as u32;
        self.start = mv(self.start);
        for t in &mut self.takes {
            t.start = mv(t.start);
            for n in &mut t.notes {
                n.pos = mv(n.pos);
            }
        }
        for c in &mut self.comps {
            for s in &mut c.sections {
                s.start = mv(s.start);
                s.end = mv(s.end);
            }
        }
    }

    /// A MIDI piece as a region: the take's notes that start in it (cut to its end).
    pub fn midi_region(&self, s: Section, name: &str) -> Region {
        let t = &self.takes[s.take];
        let mut r = t.clone();
        r.name = name.to_string();
        r.start = s.start;
        r.length = s.end - s.start;
        r.source = None;
        r.origin = t.source.or(t.origin);
        r.notes = t.notes.iter().filter(|n| n.pos >= s.start && n.pos < s.end).map(|n| {
            let mut n = *n;
            n.len = n.len.min(s.end - n.pos).max(1);
            n
        }).collect();
        r
    }
}

/// `new` written over `secs`: what it covers is cut out of the others, adjacent sections of the
/// same take joined, in time order.
pub fn put(secs: &[Section], new: Section) -> Vec<Section> {
    let mut out = Vec::new();
    for s in secs {
        if s.end <= new.start || s.start >= new.end {
            out.push(*s);
            continue;
        }
        if s.start < new.start {
            out.push(Section { start: s.start, end: new.start, take: s.take });
        }
        if s.end > new.end {
            out.push(Section { start: new.end, end: s.end, take: s.take });
        }
    }
    out.push(new);
    out.sort_by_key(|s| s.start);
    let mut joined: Vec<Section> = Vec::new();
    for s in out.into_iter().filter(|s| s.end > s.start) {
        match joined.last_mut() {
            Some(l) if l.take == s.take && l.end == s.start => l.end = s.end,
            _ => joined.push(s),
        }
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Note;

    fn region(start: u32, length: u32) -> Region {
        Region { name: "Audio".into(), start, length, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
    }

    fn folder(n: usize) -> TakeFolder {
        let mut f = TakeFolder::new(0, "Audio", region(0, 3840));
        for _ in 1..n {
            f.add_take(region(0, 3840));
        }
        f
    }

    #[test]
    fn takes_are_numbered_and_the_newest_plays() {
        let mut f = folder(1);
        assert_eq!(f.takes[0].name, "Take 1");
        let k = f.add_take(region(0, 3840));
        assert_eq!(f.takes[k].name, "Take 2");
        assert_eq!(f.active, Active::Take(1));
        assert_eq!(f.active_name(), "Take 2");
        // a later, partly overlapping take: the folder grows; it plays over the take before
        f.add_take(region(1920, 3840));
        assert_eq!((f.start, f.end()), (0, 5760));
        assert_eq!(f.active_name(), "Comp A");
        assert_eq!(f.pieces(), vec![Section { start: 0, end: 1920, take: 1 }, Section { start: 1920, end: 5760, take: 2 }]);
    }

    #[test]
    fn a_swipe_makes_comp_a_from_the_take_that_played() {
        let mut f = folder(3);
        // Take 3 plays; swipe Take 1 over beat 2
        f.swipe(0, 960, 1920);
        assert_eq!(f.active, Active::Comp(0));
        assert_eq!(f.comps[0].name, "Comp A");
        assert_eq!(f.comps[0].sections, vec![Section { start: 0, end: 960, take: 2 }, Section { start: 960, end: 1920, take: 0 }, Section { start: 1920, end: 3840, take: 2 }]);
        // a second swipe into the same comp, joining with Take 1's section
        f.swipe(0, 1920, 2880);
        assert_eq!(f.comps.len(), 1);
        assert_eq!(f.comps[0].sections, vec![Section { start: 0, end: 960, take: 2 }, Section { start: 960, end: 2880, take: 0 }, Section { start: 2880, end: 3840, take: 2 }]);
        // swiping back the whole range: one section
        f.swipe(2, 0, 3840);
        assert_eq!(f.comps[0].sections, vec![Section { start: 0, end: 3840, take: 2 }]);
    }

    #[test]
    fn swipes_stay_inside_the_take() {
        let mut f = folder(1);
        f.add_take(region(1920, 960));
        f.select(Active::Take(0));
        f.swipe(1, 0, 3840);
        assert_eq!(f.pieces(), vec![Section { start: 0, end: 1920, take: 0 }, Section { start: 1920, end: 2880, take: 1 }, Section { start: 2880, end: 3840, take: 0 }]);
    }

    #[test]
    fn deleting_a_take_fixes_the_comps() {
        let mut f = folder(3);
        f.swipe(0, 0, 960);
        f.swipe(1, 960, 1920);
        f.select(Active::Take(0));
        assert!(f.delete_active());
        assert_eq!(f.takes.len(), 2);
        assert_eq!(f.comps[0].sections, vec![Section { start: 960, end: 1920, take: 0 }, Section { start: 1920, end: 3840, take: 1 }]);
        f.select(Active::Comp(0));
        assert!(f.delete_active());
        assert!(f.comps.is_empty());
        assert_eq!(f.active, Active::Take(1));
        f.select(Active::Take(0));
        assert!(f.delete_active());
        assert!(!f.delete_active());
    }

    #[test]
    fn next_and_previous_go_through_takes_then_comps() {
        let mut f = folder(2);
        f.new_comp();
        assert_eq!(f.active, Active::Comp(0));
        f.step(true);
        assert_eq!(f.active, Active::Take(0));
        f.step(false);
        assert_eq!(f.active, Active::Comp(0));
        f.step(false);
        assert_eq!(f.active, Active::Take(1));
    }

    #[test]
    fn midi_pieces_keep_their_notes() {
        let mut r = region(0, 3840);
        r.notes = vec![Note { channel: 0, pitch: 60, velocity: 100, pos: 0, len: 960, muted: false }, Note { channel: 0, pitch: 62, velocity: 100, pos: 1920, len: 2000, muted: false }];
        let f = TakeFolder::new(0, "Keys", r);
        let x = f.midi_region(Section { start: 1920, end: 3000, take: 0 }, "Take 1");
        assert_eq!(x.notes.len(), 1);
        assert_eq!(x.notes[0].len, 1080);
        assert_eq!((x.start, x.length), (1920, 1080));
    }

    #[test]
    fn shift_and_trim() {
        let mut f = folder(2);
        f.swipe(0, 960, 1920);
        f.comps[0].sections = vec![Section { start: 960, end: 1920, take: 0 }];
        f.trim_to_active();
        assert_eq!((f.start, f.length), (960, 960));
        f.shift(3840);
        assert_eq!(f.start, 4800);
        assert_eq!(f.takes[0].start, 3840);
        assert_eq!(f.comps[0].sections[0].start, 4800);
    }
}
