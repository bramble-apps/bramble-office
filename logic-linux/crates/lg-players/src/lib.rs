//! Bramble Studio's Session Players: a Drummer, a Bass Player and a Keyboard Player that respond
//! to Logic Pro's Session Player controls the way the Logic research measured them.
//!
//! The patterns are made here, from rules (where a beat is strong, what a style leans on, how a
//! fill travels down the kit); nothing of Apple's pattern pools or style graphs is copied. The
//! numbers the rules are tuned to (notes per bar per kit piece, velocities, timing spread, how
//! complexity, intensity, ghost notes and feel change them) come from Logic's own pre-rendered
//! regions, measured with `examples/measure.rs`; the summary is in [`measured`].
//!
//! - [`drummer`]: [`drummer::generate`] makes a drummer region (characters in [`character`],
//!   kit notes in [`kit`]).
//! - [`bass`] and [`keys`]: [`bass::generate`] and [`keys::generate`] play over chords
//!   ([`chords`]).
//! - [`genm`]: a Session Player region's settings as Logic stores them (the GenM JSON record, the
//!   region chords), read into and written from the parameter types here.
//!
//! Everything is deterministic per seed: the same parameters and seed give the same notes.

pub mod bass;
pub mod character;
pub mod chords;
pub mod drummer;
pub mod genm;
pub mod json;
pub mod keys;
pub mod kit;
pub mod measured;

pub use lg_core::events::{Note, PPQ};

/// Where and how long a Session Player region is, and the song around it.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionSpec {
    /// Start in song ticks (960 per quarter note); the region begins on a bar line.
    pub start: u32,
    /// Length in ticks.
    pub length: u32,
    /// Time signature (numerator, denominator).
    pub signature: (u8, u8),
    /// The arrangement markers of the song (any order; those outside the region still tell the
    /// player which section it is in). Empty: the region is one section.
    pub markers: Vec<Marker>,
    /// Play an ending at the region's end (Logic does when the region is the song's last, or
    /// the section an Outro / End).
    pub ending: bool,
    /// The region follows another player region (a crash may open it).
    pub continues: bool,
}

impl RegionSpec {
    pub fn new(start: u32, length: u32, signature: (u8, u8)) -> Self {
        RegionSpec { start, length, signature, markers: Vec::new(), ending: false, continues: false }
    }

    pub fn end(&self) -> u32 {
        self.start + self.length
    }

    pub fn bar_ticks(&self) -> u32 {
        bar_ticks(self.signature)
    }

    /// Whole and partial bars in the region.
    pub fn bars(&self) -> u32 {
        self.length.div_ceil(self.bar_ticks().max(1)).max(1)
    }

    /// The section (kind, start tick, end tick) a tick is in.
    pub fn section_at(&self, tick: u32) -> (SectionKind, u32, u32) {
        let mut ms: Vec<&Marker> = self.markers.iter().collect();
        ms.sort_by_key(|m| m.pos);
        let mut cur: (SectionKind, u32, u32) = (SectionKind::Other, 0, u32::MAX);
        for (i, m) in ms.iter().enumerate() {
            if m.pos <= tick {
                let end = ms.get(i + 1).map_or(u32::MAX, |n| n.pos);
                cur = (SectionKind::of(&m.name), m.pos, end);
            }
        }
        cur
    }
}

/// An arrangement marker: where it starts and its name ("Intro", "Verse", "Chorus", "Bridge",
/// "Outro" … as Logic's arrangement track names them).
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub pos: u32,
    pub name: String,
}

/// What kind of section a marker name means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    Intro,
    Verse,
    PreChorus,
    Chorus,
    Bridge,
    Breakdown,
    Outro,
    Other,
}

impl SectionKind {
    pub fn of(name: &str) -> SectionKind {
        let n = name.trim().to_ascii_lowercase();
        if n.starts_with("intro") {
            SectionKind::Intro
        } else if n.starts_with("pre") {
            SectionKind::PreChorus
        } else if n.starts_with("chorus") || n.starts_with("refrain") || n.starts_with("hook") || n.starts_with("drop") {
            SectionKind::Chorus
        } else if n.starts_with("verse") {
            SectionKind::Verse
        } else if n.starts_with("bridge") || n.starts_with("middle") {
            SectionKind::Bridge
        } else if n.starts_with("break") || n.starts_with("interlude") || n.starts_with("solo") {
            SectionKind::Breakdown
        } else if n.starts_with("outro") || n.starts_with("end") || n.starts_with("coda") {
            SectionKind::Outro
        } else {
            SectionKind::Other
        }
    }

    /// How much louder / busier the players get in this section (an intensity offset).
    pub fn energy(self) -> f32 {
        match self {
            SectionKind::Intro => -0.15,
            SectionKind::Verse => -0.05,
            SectionKind::PreChorus => 0.03,
            SectionKind::Chorus => 0.1,
            SectionKind::Bridge => 0.0,
            SectionKind::Breakdown => -0.2,
            SectionKind::Outro => 0.0,
            SectionKind::Other => 0.0,
        }
    }
}

/// Ticks in a bar of a signature.
pub fn bar_ticks(sig: (u8, u8)) -> u32 {
    let den = sig.1.max(1) as u32;
    (sig.0.max(1) as u32) * PPQ * 4 / den
}

/// The beats a player counts in a bar: (number of beats, ticks per beat). Compound meters (6/8,
/// 9/8, 12/8) count dotted quarters; other x/8 meters count eighths.
pub fn beats_of(sig: (u8, u8)) -> (u32, u32) {
    let num = sig.0.max(1) as u32;
    let den = sig.1.max(1) as u32;
    if den == 8 && num % 3 == 0 && num > 3 {
        (num / 3, PPQ * 3 / 2)
    } else {
        (num, PPQ * 4 / den)
    }
}

/// A small, fast, deterministic random source (SplitMix64).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x6A09_E667_F3BC_C909)
    }

    /// A random source derived from a seed and a label (independent streams: changing how much
    /// one part draws does not move the others).
    pub fn derive(seed: u64, label: u64) -> Self {
        let mut r = Rng::new(seed.wrapping_add(label.wrapping_mul(0x9E37_79B9_7F4A_7C15)));
        r.next_u64();
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0..1
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }

    /// 0..n
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as u32 }
    }

    /// A normal deviate (mean 0, sd 1).
    pub fn normal(&mut self) -> f32 {
        let u = self.f().max(1e-7);
        let v = self.f();
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }

    /// Rounds `x` up or down at random so that the mean is `x`.
    pub fn round(&mut self, x: f32) -> u32 {
        let x = x.max(0.0);
        let fl = x.floor();
        fl as u32 + self.chance(x - fl) as u32
    }

    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u32) as usize]
    }
}

/// Clamps a velocity into MIDI's 1..=127.
pub fn vel(v: f32) -> u8 {
    v.round().clamp(1.0, 127.0) as u8
}

/// Makes a performance safe to hand over: inside the region, sorted, one note per pitch and
/// position, no overlapping notes of the same pitch, lengths at least one tick.
pub fn finish(mut notes: Vec<Note>, region: &RegionSpec) -> Vec<Note> {
    let (start, end) = (region.start, region.end());
    notes.retain(|n| n.pos >= start && n.pos < end && n.velocity > 0);
    notes.sort_by_key(|n| (n.pos, n.pitch, std::cmp::Reverse(n.velocity)));
    notes.dedup_by(|b, a| a.pos == b.pos && a.pitch == b.pitch && a.channel == b.channel);
    // trim each note at the next note of the same pitch and channel, and at the region's end
    let mut next: std::collections::HashMap<(u8, u8), u32> = std::collections::HashMap::new();
    for n in notes.iter_mut().rev() {
        let key = (n.channel, n.pitch);
        let limit = next.get(&key).copied().unwrap_or(end).min(end);
        if n.pos + n.len > limit {
            n.len = limit - n.pos;
        }
        n.len = n.len.max(1);
        n.velocity = n.velocity.min(127);
        next.insert(key, n.pos);
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters() {
        assert_eq!(bar_ticks((4, 4)), 3840);
        assert_eq!(bar_ticks((3, 4)), 2880);
        assert_eq!(bar_ticks((6, 8)), 2880);
        assert_eq!(beats_of((6, 8)), (2, 1440));
        assert_eq!(beats_of((7, 8)), (7, 480));
        assert_eq!(SectionKind::of("Chorus 2"), SectionKind::Chorus);
    }

    #[test]
    fn rng_is_deterministic() {
        let a: Vec<u64> = {
            let mut r = Rng::new(5);
            (0..4).map(|_| r.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut r = Rng::new(5);
            (0..4).map(|_| r.next_u64()).collect()
        };
        assert_eq!(a, b);
        let mut r = Rng::new(1);
        let m = (0..10000).map(|_| r.normal()).sum::<f32>() / 10000.0;
        assert!(m.abs() < 0.05);
    }
}
