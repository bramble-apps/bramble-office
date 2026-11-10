//! What Logic's Session Players do, measured on Logic's own pre-rendered regions (the research's
//! SP-PopRock grid, ghost and feel projects and the bass / keys projects; `examples/measure.rs`
//! reads them). Only statistics are kept here, no patterns.
//!
//! The drummer grid: Acoustic Drummer "Pop Rock" (Kyle, SoCal kit, preset Half-pipe: fills 35,
//! ghost notes 22, humanize 22, feel 5.9), 3/4 at 120 BPM, 8 bars, at complexity × intensity
//! 7.8 / 45.8 / 84.8 × 8.8 / 46.8 / 85.8.

use crate::kit::{family_of_note, Family};
use crate::{bar_ticks, Note, RegionSpec};

/// One grid point: complexity and intensity (0..1), then notes per bar for kick, snare (snare,
/// side-stick, rim), hi-hat (closed, half-open, open), toms, cymbals, and the mean velocity of
/// all notes, the kick, the snare backbeat-family and the hi-hat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridPoint {
    pub complexity: f32,
    pub intensity: f32,
    pub per_bar: [f32; 5],
    pub mean_velocity: f32,
    pub kick_velocity: f32,
    pub hat_velocity: f32,
}

/// Logic's Pop Rock 3×3 grid (3/4, 8 bars).
pub const POP_ROCK_GRID: [GridPoint; 9] = [
    GridPoint { complexity: 0.078, intensity: 0.088, per_bar: [1.5, 2.5, 5.0, 0.75, 0.25], mean_velocity: 56.9, kick_velocity: 74.6, hat_velocity: 44.1 },
    GridPoint { complexity: 0.078, intensity: 0.468, per_bar: [1.5, 2.25, 5.25, 0.75, 0.25], mean_velocity: 79.0, kick_velocity: 95.5, hat_velocity: 68.3 },
    GridPoint { complexity: 0.078, intensity: 0.858, per_bar: [1.5, 2.25, 5.25, 0.75, 0.25], mean_velocity: 101.9, kick_velocity: 117.1, hat_velocity: 93.1 },
    GridPoint { complexity: 0.458, intensity: 0.088, per_bar: [2.25, 3.0, 5.5, 0.0, 0.0], mean_velocity: 56.9, kick_velocity: 73.2, hat_velocity: 44.0 },
    GridPoint { complexity: 0.458, intensity: 0.468, per_bar: [2.25, 2.875, 5.625, 0.0, 0.0], mean_velocity: 79.1, kick_velocity: 94.7, hat_velocity: 68.2 },
    GridPoint { complexity: 0.458, intensity: 0.858, per_bar: [2.25, 2.875, 5.625, 0.0, 0.0], mean_velocity: 102.0, kick_velocity: 116.7, hat_velocity: 93.1 },
    GridPoint { complexity: 0.848, intensity: 0.088, per_bar: [2.0, 3.75, 5.5, 0.0, 0.0], mean_velocity: 59.6, kick_velocity: 72.4, hat_velocity: 48.5 },
    GridPoint { complexity: 0.848, intensity: 0.468, per_bar: [2.0, 3.625, 5.625, 0.0, 0.0], mean_velocity: 81.8, kick_velocity: 94.2, hat_velocity: 72.2 },
    GridPoint { complexity: 0.848, intensity: 0.858, per_bar: [2.0, 3.625, 5.625, 0.0, 0.0], mean_velocity: 104.5, kick_velocity: 116.3, hat_velocity: 96.5 },
];

/// Ghost notes (velocity < 40) per bar at Ghost Notes 0, 22 and 61 (complexity and intensity
/// high, 3/4): 0, 0.6 (0.12-0.6 over three regenerations), 1.1.
pub const GHOSTS_PER_BAR: [(f32, f32); 3] = [(0.0, 0.0), (0.22, 0.4), (0.61, 1.1)];

/// Feel (push/pull, -1..1) against the mean timing offset from the sixteenth grid, in ticks
/// (positive = late): pull -0.33 → +12, default 0.059 → -4, push 1.0 → -33.5.
pub const FEEL_OFFSET: [(f32, f32); 3] = [(-0.33, 12.1), (0.059, -4.2), (1.0, -33.5)];

/// Timing spread (sd of the offset from the grid, ticks) at Humanize 22: 8.7 - 10.7.
pub const TIMING_SD_AT_HUMANIZE_22: f32 = 9.5;

/// Fills at the default amount (35): a 2-beat fill every 4 bars when simple (toms, eighths,
/// crash on the next downbeat), one 2-beat fill in 8 bars when complex (snare sixteenths, rim
/// accents).
pub const FILL_BEATS: f32 = 2.0;

/// Bass (Pop Songwriter, intensity 71, complexity 50, 3/4): notes per bar (one root figure of
/// three notes, a pickup 16th before some chord changes) and mean velocity.
pub const BASS_NOTES_PER_BAR: f32 = 3.3;
pub const BASS_MEAN_VELOCITY: f32 = 77.5;
/// Keys (Strummed "Freely", intensity 71, 3/4): notes per bar and mean velocity.
pub const KEYS_NOTES_PER_BAR: f32 = 9.75;
pub const KEYS_MEAN_VELOCITY: f32 = 81.0;

/// Statistics of a drum part: notes per bar per family and mean velocities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrumStats {
    pub bars: f32,
    /// Kick, Snare, Hi-Hat, Toms, Cymbals, Percussion
    pub per_bar: [f32; 6],
    pub mean_velocity: f32,
    pub kick_velocity: f32,
    pub hat_velocity: f32,
    pub ghosts_per_bar: f32,
    /// Mean and sd of the offset from the sixteenth grid
    pub offset_mean: f32,
    pub offset_sd: f32,
}

pub fn drum_stats(notes: &[Note], region: &RegionSpec) -> DrumStats {
    let bars = region.length as f32 / bar_ticks(region.signature) as f32;
    let mut s = DrumStats { bars, ..Default::default() };
    if notes.is_empty() {
        return s;
    }
    let mut kv = (0.0, 0);
    let mut hv = (0.0, 0);
    let mut offs = Vec::new();
    for n in notes {
        let f = family_of_note(n.pitch);
        let k = Family::ALL.iter().position(|x| *x == f).unwrap_or(5);
        s.per_bar[k] += 1.0;
        s.mean_velocity += n.velocity as f32;
        if f == Family::Kick {
            kv = (kv.0 + n.velocity as f32, kv.1 + 1);
        }
        if f == Family::HiHat {
            hv = (hv.0 + n.velocity as f32, hv.1 + 1);
        }
        if n.velocity < 40 {
            s.ghosts_per_bar += 1.0;
        }
        let rel = n.pos as i64 - region.start as i64;
        offs.push((rel - ((rel as f64 / 240.0).round() as i64) * 240) as f32);
    }
    for v in s.per_bar.iter_mut() {
        *v /= bars;
    }
    s.ghosts_per_bar /= bars;
    s.mean_velocity /= notes.len() as f32;
    s.kick_velocity = if kv.1 > 0 { kv.0 / kv.1 as f32 } else { 0.0 };
    s.hat_velocity = if hv.1 > 0 { hv.0 / hv.1 as f32 } else { 0.0 };
    s.offset_mean = offs.iter().sum::<f32>() / offs.len() as f32;
    s.offset_sd = (offs.iter().map(|o| (o - s.offset_mean).powi(2)).sum::<f32>() / offs.len() as f32).sqrt();
    s
}

/// The mean of several statistics.
pub fn mean_stats(all: &[DrumStats]) -> DrumStats {
    let n = all.len().max(1) as f32;
    let mut m = DrumStats::default();
    for s in all {
        m.bars += s.bars / n;
        for k in 0..6 {
            m.per_bar[k] += s.per_bar[k] / n;
        }
        m.mean_velocity += s.mean_velocity / n;
        m.kick_velocity += s.kick_velocity / n;
        m.hat_velocity += s.hat_velocity / n;
        m.ghosts_per_bar += s.ghosts_per_bar / n;
        m.offset_mean += s.offset_mean / n;
        m.offset_sd += s.offset_sd / n;
    }
    m
}
